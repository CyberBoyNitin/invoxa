use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::util::{
    append_cookie, build_cookie, check_origin, client_ip, cookie_value, request_id, require_csrf,
    user_agent,
};
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, HeaderValue, Method};
use chrono::{DateTime, Duration, Utc};
use invoxa_database::{auth, operations};
use invoxa_domain::role::{Permission, Role};
use invoxa_security::{generate_session_token, hash_token};
use serde_json::{json, Value};
use uuid::Uuid;

pub const SESSION_COOKIE: &str = "invoxa_session";
pub const CSRF_COOKIE: &str = "invoxa_csrf";
pub const ORG_COOKIE: &str = "invoxa_org";
const SESSION_REQUEST_LIMIT_PER_MINUTE: u32 = 600;
const MAX_SESSION_TOKEN_LEN: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKind {
    User,
    ApiKey,
}

#[derive(Clone)]
pub struct AuthContext {
    pub kind: AuthKind,
    pub user_id: Option<Uuid>,
    pub session_id: Option<Uuid>,
    pub user_email: Option<String>,
    pub user_name: Option<String>,
    pub session_expires_at: Option<DateTime<Utc>>,
    pub api_key_id: Option<Uuid>,
    pub key_created_by: Option<Uuid>,
    pub organization_id: Uuid,
    pub organization_name: String,
    pub organization_slug: String,
    pub role: Option<Role>,
    pub scopes: Vec<String>,
    pub is_demo: bool,
    pub country: String,
    pub base_currency: String,
    pub settings: Value,
    pub ip: String,
    pub request_id: Option<String>,
}

impl AuthContext {
    pub fn is_api_key(&self) -> bool {
        self.kind == AuthKind::ApiKey
    }

    /// The user recorded as the actor for persistence operations. API keys do
    /// not have a person behind them, so writes are attributed to the user who
    /// created the key. The audit log also records the key id.
    pub fn actor_id(&self) -> ApiResult<Uuid> {
        self.user_id.or(self.key_created_by).ok_or_else(|| {
            ApiError::forbidden(
                "This API key has no active owner, so it cannot perform write operations.",
            )
        })
    }

    pub fn session_user_id(&self) -> ApiResult<Uuid> {
        self.user_id
            .ok_or_else(|| ApiError::forbidden("This endpoint requires a signed-in user."))
    }

    pub fn require(&self, permission: Permission) -> ApiResult<()> {
        self.require_scope(permission, permission.as_str())
    }

    pub fn allows(&self, permission: Permission, scope: &str) -> bool {
        match self.kind {
            AuthKind::User => self.role.map(|role| role.can(permission)).unwrap_or(false),
            AuthKind::ApiKey => self.scopes.iter().any(|value| value == scope),
        }
    }

    pub fn require_scope(&self, permission: Permission, scope: &str) -> ApiResult<()> {
        let allowed = self.allows(permission, scope);
        if allowed {
            Ok(())
        } else {
            Err(ApiError::forbidden(format!(
                "This credential does not have the required permission ({scope})."
            )))
        }
    }

    pub fn require_owner(&self) -> ApiResult<()> {
        if self.kind == AuthKind::User && self.role == Some(Role::Owner) {
            Ok(())
        } else {
            Err(ApiError::forbidden(
                "Only an organization owner can perform this action.",
            ))
        }
    }

    pub fn organization_json(&self) -> Value {
        json!({
            "id": self.organization_id,
            "name": self.organization_name,
            "slug": self.organization_slug,
            "country": self.country,
            "base_currency": self.base_currency,
            "is_demo": self.is_demo,
            "settings": self.settings,
        })
    }
}

/// Authenticate a request using an API key or a session cookie. The extractor
/// also enforces the CSRF double-submit token for unsafe session requests and
/// applies per-principal rate limits. Every handler receives an already
/// organization-scoped context; no handler may trust a client-supplied tenant.
impl FromRequestParts<AppState> for AuthContext {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let headers = parts.headers.clone();
        let ip = client_ip(&headers, parts, state.config.trust_proxy);
        let request_id = request_id(&headers);
        let method = parts.method.clone();

        if let Some(raw_key) = api_key_from_headers(&headers)? {
            return authenticate_api_key(state, &headers, &raw_key, &ip, &request_id).await;
        }

        let raw_session = cookie_value(&headers, SESSION_COOKIE)
            .ok_or_else(|| ApiError::unauthorized("Sign in to continue, or use an API key."))?;
        if raw_session.len() < 20 || raw_session.len() > MAX_SESSION_TOKEN_LEN {
            return Err(ApiError::unauthorized(
                "Your session is invalid or has expired. Sign in again.",
            ));
        }
        let session_token_hash = hash_token(&raw_session);
        let session = auth::find_session_user(&state.db, &session_token_hash)
            .await
            .map_err(|error| ApiError::internal("session lookup", error))?
            .ok_or_else(|| {
                ApiError::unauthorized("Your session is invalid or has expired. Sign in again.")
            })?;

        let memberships = auth::memberships_for_user(&state.db, session.user_id)
            .await
            .map_err(|error| ApiError::internal("membership lookup", error))?;
        if memberships.is_empty() {
            return Err(ApiError::forbidden(
                "Your account does not belong to an active organization.",
            ));
        }
        let requested_org = headers
            .get("x-organization-id")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| cookie_value(&headers, ORG_COOKIE));
        let membership = match requested_org {
            Some(raw) => {
                let org_id = Uuid::parse_str(&raw)
                    .map_err(|_| ApiError::forbidden("The selected organization is invalid."))?;
                memberships
                    .iter()
                    .find(|membership| membership.organization_id == org_id)
                    .cloned()
                    .ok_or_else(|| {
                        ApiError::forbidden("You are not a member of the selected organization.")
                    })?
            }
            None => memberships[0].clone(),
        };
        let role = Role::parse(&membership.role).ok_or_else(|| {
            ApiError::internal("role parsing", format!("unknown role {}", membership.role))
        })?;

        if !matches!(method, Method::GET | Method::HEAD | Method::OPTIONS) {
            check_origin(state, &headers)?;
            require_csrf(state, &headers)?;
        }

        if let Some(retry_after) = state
            .rate_limiter
            .check(
                &format!("session:{}", session.user_id),
                SESSION_REQUEST_LIMIT_PER_MINUTE,
                std::time::Duration::from_secs(60),
            )
            .await
        {
            return Err(ApiError::rate_limited(retry_after));
        }

        let _ = auth::touch_session(&state.db, session.session_id).await;

        Ok(Self {
            kind: AuthKind::User,
            user_id: Some(session.user_id),
            session_id: Some(session.session_id),
            user_email: Some(session.email),
            user_name: Some(session.name),
            session_expires_at: Some(session.session_expires_at),
            api_key_id: None,
            key_created_by: None,
            organization_id: membership.organization_id,
            organization_name: membership.organization_name,
            organization_slug: membership.organization_slug,
            role: Some(role),
            scopes: Vec::new(),
            is_demo: membership.is_demo,
            country: membership.country,
            base_currency: membership.base_currency,
            settings: membership.settings,
            ip,
            request_id,
        })
    }
}

fn api_key_from_headers(headers: &HeaderMap) -> ApiResult<Option<String>> {
    if let Some(value) = headers.get(header::AUTHORIZATION) {
        let value = value
            .to_str()
            .map_err(|_| ApiError::unauthorized("The Authorization header is invalid."))?;
        let Some(raw) = value.strip_prefix("Bearer ") else {
            return Err(ApiError::unauthorized(
                "Authorization must use the Bearer scheme.",
            ));
        };
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(ApiError::unauthorized("The API key is empty."));
        }
        if raw.len() > 200 || !raw.starts_with("ivx_") {
            return Err(ApiError::unauthorized("The API key is invalid."));
        }
        return Ok(Some(raw.to_string()));
    }
    if let Some(value) = headers.get("x-api-key") {
        let raw = value
            .to_str()
            .map_err(|_| ApiError::unauthorized("The API key header is invalid."))?
            .trim();
        if raw.is_empty() || raw.len() > 200 || !raw.starts_with("ivx_") {
            return Err(ApiError::unauthorized("The API key is invalid."));
        }
        return Ok(Some(raw.to_string()));
    }
    Ok(None)
}

async fn authenticate_api_key(
    state: &AppState,
    _headers: &HeaderMap,
    raw_key: &str,
    ip: &str,
    request_id: &Option<String>,
) -> ApiResult<AuthContext> {
    let key_hash = hash_token(raw_key);
    let key = operations::find_api_key_by_hash(&state.db, &key_hash)
        .await
        .map_err(|error| ApiError::internal("API key lookup", error))?
        .ok_or_else(|| ApiError::unauthorized("The API key is invalid, revoked, or expired."))?;

    if let Some(retry_after) = state
        .rate_limiter
        .check(
            &format!("api-key:{}", key.id),
            state.config.api_key_rate_limit_per_minute,
            std::time::Duration::from_secs(60),
        )
        .await
    {
        return Err(ApiError::rate_limited(retry_after));
    }
    let _ = operations::touch_api_key(&state.db, key.id).await;

    Ok(AuthContext {
        kind: AuthKind::ApiKey,
        user_id: None,
        session_id: None,
        user_email: None,
        user_name: None,
        session_expires_at: None,
        api_key_id: Some(key.id),
        key_created_by: key.created_by,
        organization_id: key.organization_id,
        organization_name: key.organization_name,
        organization_slug: key.organization_slug,
        role: None,
        scopes: key.scopes,
        is_demo: key.is_demo,
        country: key.country,
        base_currency: key.base_currency,
        settings: key.settings,
        ip: ip.to_string(),
        request_id: request_id.clone(),
    })
}

// ---------------------------------------------------------------------------
// Sessions and cookies
// ---------------------------------------------------------------------------

pub struct IssuedSession {
    pub token: String,
    pub csrf_token: String,
    pub session_id: Uuid,
    pub expires_at: DateTime<Utc>,
}

pub async fn issue_session(
    state: &AppState,
    user_id: Uuid,
    ip: &str,
    headers: &HeaderMap,
) -> ApiResult<IssuedSession> {
    let token =
        generate_session_token().map_err(|error| ApiError::internal("session token", error))?;
    let expires_at = Utc::now() + Duration::hours(state.config.session_ttl_hours);
    let session_id = auth::create_session(
        &state.db,
        user_id,
        &hash_token(&token),
        Some(ip),
        user_agent(headers),
        expires_at,
    )
    .await
    .map_err(|error| ApiError::internal("create session", error))?;
    let signed_csrf = state
        .csrf
        .issue(state.config.session_ttl_hours * 3600)
        .map_err(|error| ApiError::internal("issue CSRF token", error))?;
    Ok(IssuedSession {
        token,
        csrf_token: signed_csrf,
        session_id,
        expires_at,
    })
}

pub fn session_cookie_headers(
    state: &AppState,
    issued: &IssuedSession,
    organization_id: Uuid,
) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let max_age = state.config.session_ttl_hours * 3600;
    append_cookie(
        &mut headers,
        build_cookie(
            SESSION_COOKIE,
            &issued.token,
            "/",
            max_age,
            state.config.secure_cookies(),
            true,
            "Lax",
        ),
    );
    append_cookie(
        &mut headers,
        build_cookie(
            CSRF_COOKIE,
            &issued.csrf_token,
            "/",
            max_age,
            state.config.secure_cookies(),
            false,
            "Lax",
        ),
    );
    append_cookie(
        &mut headers,
        build_cookie(
            ORG_COOKIE,
            &organization_id.to_string(),
            "/",
            max_age,
            state.config.secure_cookies(),
            true,
            "Lax",
        ),
    );
    headers
}

pub fn csrf_cookie_header(state: &AppState, token: &str) -> HeaderValue {
    build_cookie(
        CSRF_COOKIE,
        token,
        "/",
        state.config.session_ttl_hours * 3600,
        state.config.secure_cookies(),
        false,
        "Lax",
    )
}

pub fn existing_csrf(state: &AppState, headers: &HeaderMap) -> Option<String> {
    cookie_value(headers, CSRF_COOKIE).filter(|token| state.csrf.verify(token))
}

pub fn clear_session_cookies(state: &AppState) -> HeaderMap {
    let mut headers = HeaderMap::new();
    append_cookie(
        &mut headers,
        crate::util::clear_cookie(SESSION_COOKIE, "/", state.config.secure_cookies()),
    );
    append_cookie(
        &mut headers,
        crate::util::clear_cookie(CSRF_COOKIE, "/", state.config.secure_cookies()),
    );
    append_cookie(
        &mut headers,
        crate::util::clear_cookie(ORG_COOKIE, "/", state.config.secure_cookies()),
    );
    headers
}

// ---------------------------------------------------------------------------
// Audit
// ---------------------------------------------------------------------------

pub async fn audit(
    state: &AppState,
    auth: &AuthContext,
    action: &str,
    entity_type: Option<&str>,
    entity_id: Option<Uuid>,
    metadata: Value,
) {
    let input = operations::AuditInput {
        organization_id: auth.organization_id,
        actor_user_id: auth.user_id.or(auth.key_created_by),
        actor_api_key_id: auth.api_key_id,
        action,
        entity_type,
        entity_id,
        metadata,
        ip: Some(auth.ip.as_str()),
        request_id: auth.request_id.as_deref(),
    };
    if let Err(error) = operations::insert_audit(&state.db, &input).await {
        tracing::error!(action, error = %error, "could not write audit event");
    }
}

pub fn role_permissions(role: Role) -> Vec<&'static str> {
    [
        Permission::ViewDocuments,
        Permission::UploadDocuments,
        Permission::DeleteDocuments,
        Permission::EditInvoice,
        Permission::ApproveInvoice,
        Permission::RejectInvoice,
        Permission::ExportInvoices,
        Permission::ManageVendors,
        Permission::ManageAutomations,
        Permission::ManageApiKeys,
        Permission::ManageWebhooks,
        Permission::ManageIntegrations,
        Permission::ManageMembers,
        Permission::ManageOrganization,
        Permission::ViewAuditLog,
    ]
    .into_iter()
    .filter(|permission| role.can(*permission))
    .map(Permission::as_str)
    .collect()
}

pub const ALL_API_SCOPES: &[&str] = &[
    "documents:read",
    "documents:write",
    "documents:delete",
    "invoices:read",
    "invoices:write",
    "invoices:approve",
    "invoices:reject",
    "exports:read",
    "exports:write",
    "vendors:read",
    "vendors:write",
    "automations:read",
    "automations:write",
    "api_keys:read",
    "api_keys:write",
    "webhooks:read",
    "webhooks:write",
    "members:write",
    "organization:read",
    "organization:write",
    "audit:read",
];
