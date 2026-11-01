use crate::auth::{audit, AuthContext};
use crate::error::{ApiError, ApiResult};
use crate::presenters::automation_json;
use crate::state::AppState;
use crate::util::{optional_text, required_text, ApiJson, ApiPath};
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use invoxa_database::operations;
use invoxa_domain::automation::{validate_spec, AutomationSpec};
use invoxa_domain::role::Permission;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct AutomationInput {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_priority")]
    pub priority: i32,
    pub spec: Value,
}

fn default_true() -> bool {
    true
}

fn default_priority() -> i32 {
    100
}

pub async fn list(State(state): State<AppState>, auth: AuthContext) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ManageAutomations, "automations:read")?;
    let rows = operations::list_automation_rules(&state.db, auth.organization_id).await?;
    Ok(Json(json!({
        "items": rows.iter().map(automation_json).collect::<Vec<_>>(),
        "supported": {
            "fields": [
                "document_type", "vendor_name", "currency", "total", "extraction_quality",
                "validation_status", "has_duplicates", "has_blocking_issues",
                "missing_required_fields", "country"
            ],
            "operators": ["eq", "neq", "gt", "gte", "lt", "lte", "contains"],
            "actions": [
                "add_label", "assign_reviewer", "hold_for_review",
                "send_notification", "queue_export", "trigger_webhook"
            ],
            "notes": "queue_export is evaluated but never bypasses review or approval. Notification and webhook actions are recorded in the automation run outcome; email delivery is not configured."
        }
    })))
}

pub async fn detail(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(rule_id): ApiPath<Uuid>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ManageAutomations, "automations:read")?;
    let rule = load_rule(&state, &auth, rule_id).await?;
    Ok(Json(automation_json(&rule)))
}

pub async fn create(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiJson(input): ApiJson<AutomationInput>,
) -> ApiResult<impl axum::response::IntoResponse> {
    auth.require_scope(Permission::ManageAutomations, "automations:write")?;
    let fields = validate_input(input)?;
    let actor = auth.actor_id()?;
    let rule_id = operations::create_automation_rule(
        &state.db,
        auth.organization_id,
        &fields.name,
        fields.description.as_deref(),
        fields.enabled,
        fields.priority,
        &fields.spec,
        actor,
    )
    .await?;
    audit(
        &state,
        &auth,
        "automation.created",
        Some("automation_rule"),
        Some(rule_id),
        json!({"name": fields.name, "enabled": fields.enabled}),
    )
    .await;
    let rule = load_rule(&state, &auth, rule_id).await?;
    Ok((StatusCode::CREATED, Json(automation_json(&rule))))
}

pub async fn update(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(rule_id): ApiPath<Uuid>,
    ApiJson(input): ApiJson<AutomationInput>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ManageAutomations, "automations:write")?;
    let fields = validate_input(input)?;
    let changed = operations::update_automation_rule(
        &state.db,
        auth.organization_id,
        rule_id,
        &fields.name,
        fields.description.as_deref(),
        fields.enabled,
        fields.priority,
        &fields.spec,
    )
    .await?;
    if !changed {
        return Err(ApiError::not_found("Automation rule not found."));
    }
    audit(
        &state,
        &auth,
        "automation.updated",
        Some("automation_rule"),
        Some(rule_id),
        json!({"name": fields.name, "enabled": fields.enabled}),
    )
    .await;
    let rule = load_rule(&state, &auth, rule_id).await?;
    Ok(Json(automation_json(&rule)))
}

pub async fn delete(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(rule_id): ApiPath<Uuid>,
) -> ApiResult<impl axum::response::IntoResponse> {
    auth.require_scope(Permission::ManageAutomations, "automations:write")?;
    let deleted =
        operations::delete_automation_rule(&state.db, auth.organization_id, rule_id).await?;
    if !deleted {
        return Err(ApiError::not_found("Automation rule not found."));
    }
    audit(
        &state,
        &auth,
        "automation.deleted",
        Some("automation_rule"),
        Some(rule_id),
        json!({}),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

async fn load_rule(
    state: &AppState,
    auth: &AuthContext,
    rule_id: Uuid,
) -> ApiResult<operations::AutomationRuleRow> {
    operations::list_automation_rules(&state.db, auth.organization_id)
        .await?
        .into_iter()
        .find(|rule| rule.id == rule_id)
        .ok_or_else(|| ApiError::not_found("Automation rule not found."))
}

struct ValidAutomation {
    name: String,
    description: Option<String>,
    enabled: bool,
    priority: i32,
    spec: Value,
}

fn validate_input(input: AutomationInput) -> ApiResult<ValidAutomation> {
    let name = required_text(&input.name, "name", 1, 160)?;
    let description = optional_text(
        input
            .description
            .as_ref()
            .map(|value| Value::String(value.clone()))
            .as_ref(),
        "description",
        500,
    )?;
    if !(0..=10_000).contains(&input.priority) {
        return Err(ApiError::validation("priority must be between 0 and 10000"));
    }
    let spec: AutomationSpec = serde_json::from_value(input.spec)
        .map_err(|_| ApiError::validation("spec is not a valid automation rule"))?;
    if let Err(errors) = validate_spec(&spec) {
        return Err(ApiError::validation(errors.join("; ")));
    }
    let spec = serde_json::to_value(&spec)
        .map_err(|error| ApiError::internal("serialize automation", error))?;
    Ok(ValidAutomation {
        name,
        description,
        enabled: input.enabled,
        priority: input.priority,
        spec,
    })
}
