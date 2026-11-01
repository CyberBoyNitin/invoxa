use crate::auth::{audit, AuthContext, ALL_API_SCOPES};
use crate::error::{ApiError, ApiResult};
use crate::presenters::api_key_json;
use crate::state::AppState;
use crate::util::{required_text, ApiJson, ApiPath};
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::Json;
use chrono::{Duration, Utc};
use invoxa_database::operations;
use invoxa_domain::role::Permission;
use invoxa_security::generate_api_key;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct CreateKeyInput {
    pub name: String,
    pub scopes: Vec<String>,
    #[serde(default)]
    pub expires_in_days: Option<i64>,
}

pub async fn list(State(state): State<AppState>, auth: AuthContext) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ManageApiKeys, "api_keys:read")?;
    let rows = operations::list_api_keys(&state.db, auth.organization_id).await?;
    Ok(Json(json!({
        "items": rows.iter().map(api_key_json).collect::<Vec<_>>(),
        "available_scopes": ALL_API_SCOPES,
        "warning": "The secret is shown only once when a key is created. Secrets are stored as SHA-256 hashes and cannot be retrieved later."
    })))
}

pub async fn create(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiJson(input): ApiJson<CreateKeyInput>,
) -> ApiResult<impl axum::response::IntoResponse> {
    // Only a signed-in user may mint keys. Allowing a key with api_keys:write to
    // mint new keys would let it escalate its own scope.
    let user_id = auth.session_user_id()?;
    auth.require(Permission::ManageApiKeys)?;
    let name = required_text(&input.name, "name", 1, 100)?;
    let scopes = validate_scopes(&input.scopes)?;
    let days = input.expires_in_days.unwrap_or(90);
    if !(1..=365).contains(&days) {
        return Err(ApiError::validation(
            "expires_in_days must be between 1 and 365",
        ));
    }
    let generated =
        generate_api_key().map_err(|error| ApiError::internal("generate API key", error))?;
    let expires_at = Utc::now() + Duration::days(days);
    let key_id = operations::create_api_key(
        &state.db,
        auth.organization_id,
        &name,
        &generated.prefix,
        &generated.hash,
        &scopes,
        user_id,
        Some(expires_at),
    )
    .await?;
    audit(
        &state,
        &auth,
        "api_key.created",
        Some("api_key"),
        Some(key_id),
        json!({"name": name, "scopes": scopes, "expires_at": expires_at}),
    )
    .await;
    let row = operations::list_api_keys(&state.db, auth.organization_id)
        .await?
        .into_iter()
        .find(|row| row.id == key_id)
        .ok_or_else(|| ApiError::internal("api key", "created key could not be loaded"))?;
    let mut payload = api_key_json(&row);
    if let Some(object) = payload.as_object_mut() {
        object.insert("secret".into(), Value::String(generated.secret));
        object.insert(
            "secret_notice".into(),
            Value::String(
                "This secret is shown once. Store it now; it cannot be recovered.".into(),
            ),
        );
    }
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, private"),
    );
    Ok((StatusCode::CREATED, headers, Json(payload)))
}

pub async fn revoke(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(key_id): ApiPath<Uuid>,
) -> ApiResult<Json<Value>> {
    let user_id = auth.session_user_id()?;
    auth.require(Permission::ManageApiKeys)?;
    if auth.api_key_id == Some(key_id) {
        return Err(ApiError::validation("An API key cannot revoke itself."));
    }
    let revoked = operations::revoke_api_key(&state.db, auth.organization_id, key_id).await?;
    if !revoked {
        return Err(ApiError::not_found(
            "API key not found or it was already revoked.",
        ));
    }
    audit(
        &state,
        &auth,
        "api_key.revoked",
        Some("api_key"),
        Some(key_id),
        json!({"revoked_by": user_id}),
    )
    .await;
    Ok(Json(json!({"id": key_id, "revoked": true})))
}

fn validate_scopes(scopes: &[String]) -> ApiResult<Vec<String>> {
    if scopes.is_empty() {
        return Err(ApiError::validation("At least one scope is required."));
    }
    if scopes.len() > ALL_API_SCOPES.len() {
        return Err(ApiError::validation("Too many scopes."));
    }
    let mut unique = BTreeSet::new();
    for scope in scopes {
        let scope = scope.trim();
        if !ALL_API_SCOPES.contains(&scope) {
            return Err(ApiError::validation(format!(
                "Unknown API scope '{scope}'."
            )));
        }
        if !unique.insert(scope.to_string()) {
            return Err(ApiError::validation(format!(
                "Duplicate API scope '{scope}'."
            )));
        }
    }
    Ok(unique.into_iter().collect())
}
