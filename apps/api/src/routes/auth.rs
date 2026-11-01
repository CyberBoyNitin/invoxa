use crate::auth::{
    audit, clear_session_cookies, csrf_cookie_header, existing_csrf, issue_session,
    role_permissions, session_cookie_headers, AuthContext, AuthKind, ORG_COOKIE,
};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::util::{
    append_cookie, build_cookie, check_origin, request_id, required_text, ApiJson, ClientIp,
};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use chrono::{Duration, Utc};
use invoxa_database::{auth, operations};
use invoxa_domain::normalization::{is_known_currency, normalize_currency};
use invoxa_domain::role::Role;
use invoxa_security::{
    generate_reset_token, hash_password_async, hash_token, validate_password, verify_password_async,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct SignupInput {
    pub email: String,
    pub password: String,
    pub name: String,
    pub organization_name: String,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub base_currency: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginInput {
    pub email: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct ChangePasswordInput {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Deserialize)]
pub struct ResetRequestInput {
    pub email: String,
}

#[derive(Deserialize)]
pub struct ResetConfirmInput {
    pub token: String,
    pub new_password: String,
}

#[derive(Deserialize)]
pub struct SwitchOrganizationInput {
    pub organization_id: Uuid,
}

#[derive(Deserialize)]
pub struct AcceptInvitationInput {
    pub token: String,
}

pub async fn signup(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    ApiJson(input): ApiJson<SignupInput>,
) -> ApiResult<impl axum::response::IntoResponse> {
    check_origin(&state, &headers)?;
    rate_limit(&state, &format!("signup:{ip}"), 10, 60).await?;
    let email = validate_email(&input.email)?;
    validate_password(&input.password).map_err(|error| ApiError::validation(error.to_string()))?;
    let name = required_text(&input.name, "name", 1, 160)?;
    let organization_name = required_text(&input.organization_name, "organization_name", 2, 120)?;
    let country = input
        .country
        .as_deref()
        .unwrap_or("IN")
        .trim()
        .to_ascii_uppercase();
    if country.len() != 2 || !country.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(ApiError::validation(
            "country must be a two-letter ISO country code",
        ));
    }
    let requested_currency = input.base_currency.as_deref().unwrap_or("INR").trim();
    let base_currency = normalize_currency(requested_currency)
        .filter(|currency| is_known_currency(currency))
        .ok_or_else(|| ApiError::validation("base_currency must be a supported ISO currency"))?;

    let password_hash = hash_password_async(input.password)
        .await
        .map_err(|error| ApiError::internal("hash password", error))?;
    let (user, organization) = auth::register_user_and_org(
        &state.db,
        &email,
        &password_hash,
        &name,
        &organization_name,
        &country,
        &base_currency,
        false,
    )
    .await?;
    let issued = issue_session(&state, user.id, &ip, &headers).await?;
    let cookies = session_cookie_headers(&state, &issued, organization.id);

    if let Err(error) = operations::insert_audit(
        &state.db,
        &operations::AuditInput {
            organization_id: organization.id,
            actor_user_id: Some(user.id),
            actor_api_key_id: None,
            action: "auth.signup",
            entity_type: Some("user"),
            entity_id: Some(user.id),
            metadata: json!({"email": user.email, "organization": organization.name}),
            ip: Some(&ip),
            request_id: request_id(&headers).as_deref(),
        },
    )
    .await
    {
        tracing::error!(error = %error, "could not audit signup");
    }

    let memberships = auth::memberships_for_user(&state.db, user.id).await?;
    let payload = user_session_payload(
        &state,
        user.id,
        &user.email,
        &user.name,
        &memberships,
        organization.id,
        Some(&issued.csrf_token),
        Some(issued.expires_at),
    )?;
    Ok((StatusCode::CREATED, cookies, Json(payload)))
}

pub async fn login(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    ApiJson(input): ApiJson<LoginInput>,
) -> ApiResult<impl axum::response::IntoResponse> {
    check_origin(&state, &headers)?;
    let email = validate_email(&input.email)?;
    rate_limit(&state, &format!("login-ip:{ip}"), 30, 300).await?;
    rate_limit(&state, &format!("login-email:{email}"), 10, 300).await?;

    let user = auth::find_user_by_email(&state.db, &email).await?;
    let Some(user) = user else {
        // Equalize the dominant cost between existing and non-existing accounts.
        let _ = invoxa_security::hash_password_async(input.password).await;
        return Err(ApiError::unauthorized("Invalid email or password."));
    };
    if user.disabled_at.is_some() {
        return Err(ApiError::unauthorized("Invalid email or password."));
    }
    if let Some(locked) = user.locked_until {
        if locked > Utc::now() {
            let seconds = (locked - Utc::now()).num_seconds().max(1) as u64;
            return Err(ApiError::rate_limited(seconds));
        }
    }
    if !verify_password_async(input.password, user.password_hash.clone()).await {
        auth::record_failed_login(&state.db, user.id).await?;
        return Err(ApiError::unauthorized("Invalid email or password."));
    }
    auth::clear_login_failures(&state.db, user.id).await?;
    let memberships = auth::memberships_for_user(&state.db, user.id).await?;
    let Some(active) = memberships.first() else {
        return Err(ApiError::forbidden(
            "Your account does not belong to an active organization.",
        ));
    };
    let issued = issue_session(&state, user.id, &ip, &headers).await?;
    let cookies = session_cookie_headers(&state, &issued, active.organization_id);

    if let Err(error) = operations::insert_audit(
        &state.db,
        &operations::AuditInput {
            organization_id: active.organization_id,
            actor_user_id: Some(user.id),
            actor_api_key_id: None,
            action: "auth.login",
            entity_type: Some("user"),
            entity_id: Some(user.id),
            metadata: json!({"method": "password"}),
            ip: Some(&ip),
            request_id: request_id(&headers).as_deref(),
        },
    )
    .await
    {
        tracing::error!(error = %error, "could not audit login");
    }

    let payload = user_session_payload(
        &state,
        user.id,
        &user.email,
        &user.name,
        &memberships,
        active.organization_id,
        Some(&issued.csrf_token),
        Some(issued.expires_at),
    )?;
    Ok((StatusCode::OK, cookies, Json(payload)))
}

pub async fn logout(
    State(state): State<AppState>,
    auth: AuthContext,
) -> ApiResult<impl axum::response::IntoResponse> {
    if auth.kind != AuthKind::User {
        return Err(ApiError::forbidden(
            "API keys cannot sign out of a browser session.",
        ));
    }
    if let Some(session_id) = auth.session_id {
        auth::revoke_session(&state.db, session_id).await?;
    }
    audit(
        &state,
        &auth,
        "auth.logout",
        Some("session"),
        auth.session_id,
        json!({}),
    )
    .await;
    let headers = clear_session_cookies(&state);
    Ok((StatusCode::NO_CONTENT, headers))
}

pub async fn current_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    auth: AuthContext,
) -> ApiResult<impl axum::response::IntoResponse> {
    let mut response_headers = HeaderMap::new();
    if auth.kind == AuthKind::ApiKey {
        let payload = json!({
            "auth": {
                "kind": "api_key",
                "scopes": auth.scopes,
                "session_expires_at": Value::Null,
            },
            "user": Value::Null,
            "memberships": [],
            "active_organization": {
                "organization": auth.organization_json(),
                "role": Value::Null,
                "permissions": auth.scopes,
            },
            "csrf_token": Value::Null,
        });
        return Ok((StatusCode::OK, response_headers, Json(payload)));
    }

    let user_id = auth.session_user_id()?;
    let memberships = auth::memberships_for_user(&state.db, user_id).await?;
    let csrf = match existing_csrf(&state, &headers) {
        Some(token) => token,
        None => {
            let token = state
                .csrf
                .issue(state.config.session_ttl_hours * 3600)
                .map_err(|error| ApiError::internal("issue CSRF token", error))?;
            append_cookie(&mut response_headers, csrf_cookie_header(&state, &token));
            token
        }
    };
    let payload = user_session_payload(
        &state,
        user_id,
        auth.user_email.as_deref().unwrap_or_default(),
        auth.user_name.as_deref().unwrap_or_default(),
        &memberships,
        auth.organization_id,
        Some(&csrf),
        auth.session_expires_at,
    )?;
    Ok((StatusCode::OK, response_headers, Json(payload)))
}

pub async fn switch_organization(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiJson(input): ApiJson<SwitchOrganizationInput>,
) -> ApiResult<impl axum::response::IntoResponse> {
    let user_id = auth.session_user_id()?;
    let membership = auth::membership_for_org(&state.db, user_id, input.organization_id)
        .await?
        .ok_or_else(|| ApiError::forbidden("You are not a member of the selected organization."))?;
    let role =
        Role::parse(&membership.role).ok_or_else(|| ApiError::internal("role", "unknown role"))?;
    let mut headers = HeaderMap::new();
    append_cookie(
        &mut headers,
        build_cookie(
            ORG_COOKIE,
            &membership.organization_id.to_string(),
            "/",
            state.config.session_ttl_hours * 3600,
            state.config.secure_cookies(),
            true,
            "Lax",
        ),
    );
    audit(
        &state,
        &auth,
        "organization.switched",
        Some("organization"),
        Some(membership.organization_id),
        json!({"to": membership.organization_name}),
    )
    .await;
    Ok((
        StatusCode::OK,
        headers,
        Json(json!({
            "organization": membership_json(&membership),
            "role": role.as_str(),
            "permissions": role_permissions(role),
        })),
    ))
}

pub async fn change_password(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiJson(input): ApiJson<ChangePasswordInput>,
) -> ApiResult<impl axum::response::IntoResponse> {
    let user_id = auth.session_user_id()?;
    validate_password(&input.new_password)
        .map_err(|error| ApiError::validation(error.to_string()))?;
    let user = auth::find_user_by_id(&state.db, user_id)
        .await?
        .ok_or_else(|| ApiError::not_found("User not found."))?;
    if !verify_password_async(input.current_password, user.password_hash).await {
        return Err(ApiError::forbidden("The current password is incorrect."));
    }
    let hash = hash_password_async(input.new_password)
        .await
        .map_err(|error| ApiError::internal("hash password", error))?;
    auth::update_password(&state.db, user_id, &hash).await?;
    auth::revoke_all_sessions(&state.db, user_id).await?;
    audit(
        &state,
        &auth,
        "auth.password_changed",
        Some("user"),
        Some(user_id),
        json!({"sessions_revoked": true}),
    )
    .await;
    let headers = clear_session_cookies(&state);
    Ok((StatusCode::OK, headers, Json(json!({"changed": true}))))
}

pub async fn request_password_reset(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    ApiJson(input): ApiJson<ResetRequestInput>,
) -> ApiResult<impl axum::response::IntoResponse> {
    check_origin(&state, &headers)?;
    rate_limit(&state, &format!("reset-ip:{ip}"), 5, 900).await?;
    let email = validate_email(&input.email)?;
    let mut dev_token: Option<String> = None;
    if let Some(user) = auth::find_user_by_email(&state.db, &email).await? {
        let token =
            generate_reset_token().map_err(|error| ApiError::internal("reset token", error))?;
        auth::create_password_reset(
            &state.db,
            user.id,
            &hash_token(&token),
            Utc::now() + Duration::minutes(30),
        )
        .await?;
        if state.config.expose_dev_tokens {
            dev_token = Some(token);
        }
    }
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "message": "If an account exists for this email, a password reset link has been created.",
            "delivery": "email_not_configured",
            "dev_reset_token": dev_token,
        })),
    ))
}

pub async fn confirm_password_reset(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    ApiJson(input): ApiJson<ResetConfirmInput>,
) -> ApiResult<impl axum::response::IntoResponse> {
    check_origin(&state, &headers)?;
    rate_limit(&state, &format!("reset-confirm:{ip}"), 10, 900).await?;
    validate_password(&input.new_password)
        .map_err(|error| ApiError::validation(error.to_string()))?;
    let hash = hash_password_async(input.new_password)
        .await
        .map_err(|error| ApiError::internal("hash password", error))?;
    auth::consume_password_reset(&state.db, &hash_token(&input.token), &hash).await?;
    Ok((
        StatusCode::OK,
        Json(json!({
            "message": "Password reset. All existing sessions have been signed out."
        })),
    ))
}

pub async fn accept_invitation(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiJson(input): ApiJson<AcceptInvitationInput>,
) -> ApiResult<impl axum::response::IntoResponse> {
    let user_id = auth.session_user_id()?;
    let email = auth
        .user_email
        .as_deref()
        .ok_or_else(|| ApiError::forbidden("This endpoint requires a signed-in user."))?;
    let invitation = auth::find_invitation_by_hash(&state.db, &hash_token(&input.token))
        .await?
        .ok_or_else(|| ApiError::not_found("The invitation is invalid or expired."))?;
    if !invitation.email.eq_ignore_ascii_case(email) {
        return Err(ApiError::forbidden(
            "This invitation was issued to a different email address.",
        ));
    }
    if invitation.accepted_at.is_some() {
        return Err(ApiError::conflict(
            "This invitation has already been accepted.",
        ));
    }
    if invitation.expires_at < Utc::now() {
        return Err(ApiError::validation("This invitation has expired."));
    }
    auth::accept_invitation(&state.db, invitation.id, user_id).await?;
    let membership = auth::membership_for_org(&state.db, user_id, invitation.organization_id)
        .await?
        .ok_or_else(|| ApiError::internal("invitation membership", "membership was not created"))?;
    let mut headers = HeaderMap::new();
    append_cookie(
        &mut headers,
        build_cookie(
            ORG_COOKIE,
            &invitation.organization_id.to_string(),
            "/",
            state.config.session_ttl_hours * 3600,
            state.config.secure_cookies(),
            true,
            "Lax",
        ),
    );
    if let Err(error) = operations::insert_audit(
        &state.db,
        &operations::AuditInput {
            organization_id: invitation.organization_id,
            actor_user_id: Some(user_id),
            actor_api_key_id: None,
            action: "member.invitation_accepted",
            entity_type: Some("organization"),
            entity_id: Some(invitation.organization_id),
            metadata: json!({"email": email}),
            ip: Some(&auth.ip),
            request_id: auth.request_id.as_deref(),
        },
    )
    .await
    {
        tracing::error!(error = %error, "could not audit invitation acceptance");
    }
    Ok((
        StatusCode::OK,
        headers,
        Json(json!({
            "organization": membership_json(&membership),
            "role": membership.role,
        })),
    ))
}

fn validate_email(value: &str) -> ApiResult<String> {
    let email = value.trim().to_lowercase();
    let valid = email.len() >= 3
        && email.len() <= 320
        && email.matches('@').count() == 1
        && !email.chars().any(char::is_whitespace)
        && email
            .split_once('@')
            .map(|(local, domain)| {
                !local.is_empty()
                    && domain.contains('.')
                    && !domain.starts_with('.')
                    && !domain.ends_with('.')
                    && !domain.contains("..")
            })
            .unwrap_or(false);
    if !valid {
        return Err(ApiError::validation("Enter a valid email address."));
    }
    Ok(email)
}

async fn rate_limit(state: &AppState, key: &str, limit: u32, window_seconds: u64) -> ApiResult<()> {
    if let Some(retry_after) = state
        .rate_limiter
        .check(key, limit, std::time::Duration::from_secs(window_seconds))
        .await
    {
        return Err(ApiError::rate_limited(retry_after));
    }
    Ok(())
}

fn membership_json(membership: &auth::MembershipContext) -> Value {
    let role = Role::parse(&membership.role);
    json!({
        "organization": {
            "id": membership.organization_id,
            "name": membership.organization_name,
            "slug": membership.organization_slug,
            "country": membership.country,
            "base_currency": membership.base_currency,
            "is_demo": membership.is_demo,
            "settings": membership.settings,
        },
        "role": membership.role,
        "permissions": role.map(role_permissions).unwrap_or_default(),
    })
}

fn user_session_payload(
    _state: &AppState,
    user_id: Uuid,
    email: &str,
    name: &str,
    memberships: &[auth::MembershipContext],
    active_organization_id: Uuid,
    csrf_token: Option<&str>,
    session_expires_at: Option<chrono::DateTime<Utc>>,
) -> ApiResult<Value> {
    let active = memberships
        .iter()
        .find(|membership| membership.organization_id == active_organization_id)
        .or_else(|| memberships.first())
        .ok_or_else(|| ApiError::forbidden("Your account has no active organization."))?;
    let role = Role::parse(&active.role)
        .ok_or_else(|| ApiError::internal("role", "unknown membership role"))?;
    Ok(json!({
        "user": {
            "id": user_id,
            "email": email,
            "name": name,
        },
        "auth": {
            "kind": "session",
            "session_expires_at": session_expires_at,
            "scopes": Value::Null,
        },
        "memberships": memberships.iter().map(membership_json).collect::<Vec<_>>(),
        "active_organization": {
            "organization": {
                "id": active.organization_id,
                "name": active.organization_name,
                "slug": active.organization_slug,
                "country": active.country,
                "base_currency": active.base_currency,
                "is_demo": active.is_demo,
                "settings": active.settings,
            },
            "role": role.as_str(),
            "permissions": role_permissions(role),
        },
        "csrf_token": csrf_token,
    }))
}
