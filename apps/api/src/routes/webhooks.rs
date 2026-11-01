use crate::auth::{audit, AuthContext};
use crate::error::{ApiError, ApiResult};
use crate::presenters::{delivery_json, webhook_json};
use crate::state::AppState;
use crate::util::{optional_text, required_text, ApiJson, ApiPath, ApiQuery};
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use invoxa_database::operations;
use invoxa_domain::role::Permission;
use invoxa_integrations::webhook::validate_webhook_url;
use invoxa_security::generate_webhook_secret;
use invoxa_workflows::events::ALL_EVENTS;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct CreateWebhookInput {
    pub url: String,
    #[serde(default)]
    pub description: Option<String>,
    pub events: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Deserialize)]
pub struct UpdateWebhookInput {
    pub url: String,
    #[serde(default)]
    pub description: Option<String>,
    pub events: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
pub struct DeliveryQuery {
    pub endpoint_id: Option<Uuid>,
    pub limit: Option<i64>,
}

pub async fn list(State(state): State<AppState>, auth: AuthContext) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ManageWebhooks, "webhooks:read")?;
    let rows = operations::list_webhook_endpoints(&state.db, auth.organization_id).await?;
    Ok(Json(json!({
        "items": rows.iter().map(webhook_json).collect::<Vec<_>>(),
        "available_events": ALL_EVENTS,
        "note": "Endpoints must use HTTPS on port 443. URLs are DNS-checked at creation and re-checked for every delivery. Redirects are not followed."
    })))
}

pub async fn create(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiJson(input): ApiJson<CreateWebhookInput>,
) -> ApiResult<impl axum::response::IntoResponse> {
    auth.require_scope(Permission::ManageWebhooks, "webhooks:write")?;
    let actor = auth.actor_id()?;
    let url = validated_url(&state, &input.url).await?;
    let description = optional_text(
        input
            .description
            .as_ref()
            .map(|value| Value::String(value.clone()))
            .as_ref(),
        "description",
        300,
    )?;
    let events = validate_events(&input.events)?;
    let secret =
        generate_webhook_secret().map_err(|error| ApiError::internal("webhook secret", error))?;
    let encrypted = state
        .cipher
        .encrypt(&secret)
        .map_err(|error| ApiError::internal("encrypt webhook secret", error))?;
    let endpoint_id = operations::create_webhook_endpoint(
        &state.db,
        auth.organization_id,
        &url,
        description.as_deref(),
        encrypted.as_bytes(),
        &events,
        actor,
    )
    .await?;
    if !input.enabled {
        let _ = operations::update_webhook_endpoint(
            &state.db,
            auth.organization_id,
            endpoint_id,
            &url,
            description.as_deref(),
            &events,
            false,
        )
        .await;
    }
    audit(
        &state,
        &auth,
        "webhook.created",
        Some("webhook_endpoint"),
        Some(endpoint_id),
        json!({"events": events, "enabled": input.enabled}),
    )
    .await;
    let row = load_endpoint(&state, &auth, endpoint_id).await?;
    let mut payload = webhook_json(&row);
    if let Some(object) = payload.as_object_mut() {
        object.insert("secret".into(), Value::String(secret));
        object.insert(
            "secret_notice".into(),
            Value::String(
                "Use this secret to verify the t=<unix>,v1=<hex> signature. It is shown once."
                    .into(),
            ),
        );
    }
    Ok((StatusCode::CREATED, Json(payload)))
}

pub async fn update(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(endpoint_id): ApiPath<Uuid>,
    ApiJson(input): ApiJson<UpdateWebhookInput>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ManageWebhooks, "webhooks:write")?;
    let url = validated_url(&state, &input.url).await?;
    let description = optional_text(
        input
            .description
            .as_ref()
            .map(|value| Value::String(value.clone()))
            .as_ref(),
        "description",
        300,
    )?;
    let events = validate_events(&input.events)?;
    let changed = operations::update_webhook_endpoint(
        &state.db,
        auth.organization_id,
        endpoint_id,
        &url,
        description.as_deref(),
        &events,
        input.enabled,
    )
    .await?;
    if !changed {
        return Err(ApiError::not_found("Webhook endpoint not found."));
    }
    audit(
        &state,
        &auth,
        "webhook.updated",
        Some("webhook_endpoint"),
        Some(endpoint_id),
        json!({"events": events, "enabled": input.enabled}),
    )
    .await;
    let row = load_endpoint(&state, &auth, endpoint_id).await?;
    Ok(Json(webhook_json(&row)))
}

pub async fn rotate_secret(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(endpoint_id): ApiPath<Uuid>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ManageWebhooks, "webhooks:write")?;
    load_endpoint(&state, &auth, endpoint_id).await?;
    let secret =
        generate_webhook_secret().map_err(|error| ApiError::internal("webhook secret", error))?;
    let encrypted = state
        .cipher
        .encrypt(&secret)
        .map_err(|error| ApiError::internal("encrypt webhook secret", error))?;
    let changed = operations::rotate_webhook_secret(
        &state.db,
        auth.organization_id,
        endpoint_id,
        encrypted.as_bytes(),
    )
    .await?;
    if !changed {
        return Err(ApiError::not_found("Webhook endpoint not found."));
    }
    audit(
        &state,
        &auth,
        "webhook.secret_rotated",
        Some("webhook_endpoint"),
        Some(endpoint_id),
        json!({}),
    )
    .await;
    Ok(Json(json!({
        "id": endpoint_id,
        "secret": secret,
        "secret_notice": "The previous secret is no longer valid. Update your verifier now; this secret is shown once."
    })))
}

pub async fn delete(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(endpoint_id): ApiPath<Uuid>,
) -> ApiResult<impl axum::response::IntoResponse> {
    auth.require_scope(Permission::ManageWebhooks, "webhooks:write")?;
    let deleted =
        operations::delete_webhook_endpoint(&state.db, auth.organization_id, endpoint_id).await?;
    if !deleted {
        return Err(ApiError::not_found("Webhook endpoint not found."));
    }
    audit(
        &state,
        &auth,
        "webhook.deleted",
        Some("webhook_endpoint"),
        Some(endpoint_id),
        json!({}),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn deliveries(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiQuery(query): ApiQuery<DeliveryQuery>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ManageWebhooks, "webhooks:read")?;
    if let Some(endpoint_id) = query.endpoint_id {
        load_endpoint(&state, &auth, endpoint_id).await?;
    }
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let rows = operations::list_webhook_deliveries(
        &state.db,
        auth.organization_id,
        query.endpoint_id,
        limit,
    )
    .await?;
    Ok(Json(json!({
        "items": rows.iter().map(delivery_json).collect::<Vec<_>>(),
        "limit": limit,
    })))
}

pub async fn attempts(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(delivery_id): ApiPath<Uuid>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ManageWebhooks, "webhooks:read")?;
    let rows = operations::delivery_attempts(&state.db, auth.organization_id, delivery_id).await?;
    Ok(Json(json!({
        "items": rows.iter().map(|row| json!({
            "id": row.id,
            "attempt_number": row.attempt_number,
            "status_code": row.status_code,
            "error": row.error,
            "response_snippet": row.response_snippet,
            "duration_ms": row.duration_ms,
            "created_at": row.created_at,
        })).collect::<Vec<_>>()
    })))
}

pub async fn redeliver(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(delivery_id): ApiPath<Uuid>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ManageWebhooks, "webhooks:write")?;
    let delivery = operations::get_webhook_delivery(&state.db, auth.organization_id, delivery_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Webhook delivery not found."))?;
    if !matches!(
        delivery.status.as_str(),
        "failed" | "dead_letter" | "succeeded"
    ) {
        return Err(ApiError::conflict(
            "Only a completed, failed, or dead-letter delivery can be requeued manually.",
        ));
    }
    let requeued =
        operations::requeue_webhook_delivery(&state.db, auth.organization_id, delivery_id).await?;
    if !requeued {
        return Err(ApiError::not_found("Webhook delivery not found."));
    }
    audit(
        &state,
        &auth,
        "webhook.delivery_requeued",
        Some("webhook_delivery"),
        Some(delivery_id),
        json!({"previous_status": delivery.status, "event_type": delivery.event_type}),
    )
    .await;
    Ok(Json(json!({
        "id": delivery_id,
        "status": "pending",
        "notice": "The delivery is queued for the worker. The same event id is preserved for receiver idempotency."
    })))
}

async fn load_endpoint(
    state: &AppState,
    auth: &AuthContext,
    endpoint_id: Uuid,
) -> ApiResult<operations::WebhookEndpointRow> {
    operations::list_webhook_endpoints(&state.db, auth.organization_id)
        .await?
        .into_iter()
        .find(|row| row.id == endpoint_id)
        .ok_or_else(|| ApiError::not_found("Webhook endpoint not found."))
}

async fn validated_url(state: &AppState, url: &str) -> ApiResult<String> {
    let url = required_text(url, "url", 8, 2_048)?;
    validate_webhook_url(&url, &state.webhook_policy)
        .await
        .map(|validated| validated.url)
        .map_err(|error| {
            ApiError::validation(format!("Webhook URL rejected: {error}"))
                .with_details(json!({"code": error.code()}))
        })
}

fn validate_events(events: &[String]) -> ApiResult<Vec<String>> {
    if events.is_empty() {
        return Err(ApiError::validation("At least one event is required."));
    }
    let mut unique = BTreeSet::new();
    for event in events {
        let event = event.trim();
        if event == "*" {
            if events.len() != 1 {
                return Err(ApiError::validation(
                    "'*' must be the only event subscription.",
                ));
            }
            return Ok(vec!["*".into()]);
        }
        if !ALL_EVENTS.contains(&event) {
            return Err(ApiError::validation(format!(
                "Unknown webhook event '{event}'."
            )));
        }
        unique.insert(event.to_string());
    }
    Ok(unique.into_iter().collect())
}
