use crate::auth::{audit, AuthContext};
use crate::error::{ApiError, ApiResult};
use crate::presenters::{integration_json, invitation_json, member_json};
use crate::state::AppState;
use crate::util::{required_text, ApiJson, ApiPath};
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use chrono::{Duration, Utc};
use invoxa_database::{auth, operations};
use invoxa_domain::normalization::{is_known_currency, normalize_currency};
use invoxa_domain::role::{Permission, Role};
use invoxa_security::{generate_invitation_token, hash_token};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::str::FromStr;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct OrganizationUpdateInput {
    pub name: String,
    pub country: String,
    pub base_currency: String,
    #[serde(default)]
    pub settings: Option<Value>,
}

#[derive(Deserialize)]
pub struct InviteInput {
    pub email: String,
    pub role: String,
    #[serde(default)]
    pub expires_in_days: Option<i64>,
}

#[derive(Deserialize)]
pub struct MemberRoleInput {
    pub role: String,
}

pub async fn get_organization(
    State(state): State<AppState>,
    auth: AuthContext,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ViewDocuments, "organization:read")?;
    let organization = auth::organization_by_id(&state.db, auth.organization_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Organization not found."))?;
    Ok(Json(json!({
        "id": organization.id,
        "name": organization.name,
        "slug": organization.slug,
        "country": organization.country,
        "base_currency": organization.base_currency,
        "settings": organization.settings,
        "is_demo": organization.is_demo,
        "created_at": organization.created_at,
        "updated_at": organization.updated_at,
        "role": auth.role.map(|role| role.as_str()),
        "permissions": auth.role.map(crate::auth::role_permissions).unwrap_or_default(),
    })))
}

pub async fn update_organization(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiJson(input): ApiJson<OrganizationUpdateInput>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ManageOrganization, "organization:write")?;
    let name = required_text(&input.name, "name", 2, 160)?;
    let country = input.country.trim().to_ascii_uppercase();
    if country.len() != 2 || !country.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(ApiError::validation(
            "country must be a two-letter ISO country code",
        ));
    }
    let currency = normalize_currency(&input.base_currency)
        .filter(|code| is_known_currency(code))
        .ok_or_else(|| ApiError::validation("base_currency must be a supported ISO currency"))?;
    let current = auth::organization_by_id(&state.db, auth.organization_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Organization not found."))?;
    let mut settings = match current.settings {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    if let Some(patch) = input.settings {
        merge_settings(&mut settings, &patch, &auth)?;
    }
    let settings = Value::Object(settings);
    let updated = auth::update_organization(
        &state.db,
        auth.organization_id,
        &name,
        &country,
        &currency,
        &settings,
    )
    .await?;
    audit(
        &state,
        &auth,
        "organization.updated",
        Some("organization"),
        Some(auth.organization_id),
        json!({
            "name": updated.name,
            "country": updated.country,
            "base_currency": updated.base_currency,
            "settings_keys": updated.settings.as_object().map(|map| map.keys().cloned().collect::<Vec<_>>()).unwrap_or_default(),
        }),
    )
    .await;
    Ok(Json(json!({
        "id": updated.id,
        "name": updated.name,
        "slug": updated.slug,
        "country": updated.country,
        "base_currency": updated.base_currency,
        "settings": updated.settings,
        "is_demo": updated.is_demo,
        "updated_at": updated.updated_at,
    })))
}

fn merge_settings(
    current: &mut Map<String, Value>,
    patch: &Value,
    auth: &AuthContext,
) -> ApiResult<()> {
    let Some(patch) = patch.as_object() else {
        return Err(ApiError::validation("settings must be an object"));
    };
    for key in patch.keys() {
        if !matches!(key.as_str(), "ai" | "validation" | "extraction") {
            return Err(ApiError::validation(format!(
                "unsupported settings section '{key}'"
            )));
        }
    }
    if let Some(ai) = patch.get("ai") {
        let Some(ai) = ai.as_object() else {
            return Err(ApiError::validation("settings.ai must be an object"));
        };
        for key in ai.keys() {
            if key != "provider_consent" {
                return Err(ApiError::validation(format!(
                    "unsupported settings.ai key '{key}'"
                )));
            }
        }
        if let Some(consent) = ai.get("provider_consent") {
            let Some(consent) = consent.as_bool() else {
                return Err(ApiError::validation(
                    "settings.ai.provider_consent must be true or false",
                ));
            };
            let previous = current
                .get("ai")
                .and_then(Value::as_object)
                .and_then(|ai| ai.get("provider_consent"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if consent && !previous {
                // Sending invoice text to a third-party model is a
                // consequential decision that must be made by an owner.
                auth.require_owner()?;
            }
            let ai_section = current
                .entry("ai".to_string())
                .or_insert_with(|| Value::Object(Map::new()));
            if !ai_section.is_object() {
                *ai_section = Value::Object(Map::new());
            }
            ai_section
                .as_object_mut()
                .expect("object inserted above")
                .insert("provider_consent".into(), Value::Bool(consent));
        }
    }
    if let Some(validation) = patch.get("validation") {
        let Some(validation) = validation.as_object() else {
            return Err(ApiError::validation(
                "settings.validation must be an object",
            ));
        };
        let target = current
            .entry("validation".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if !target.is_object() {
            *target = Value::Object(Map::new());
        }
        let target = target.as_object_mut().expect("object inserted above");
        for (key, value) in validation {
            match key.as_str() {
                "require_invoice_date"
                | "require_po_number"
                | "require_vendor_tax_id"
                | "require_gstin_for_india" => {
                    let Some(value) = value.as_bool() else {
                        return Err(ApiError::validation(format!(
                            "settings.validation.{key} must be a boolean"
                        )));
                    };
                    target.insert(key.clone(), Value::Bool(value));
                }
                "amount_tolerance" => {
                    let parsed = parse_decimal_setting(value, key, Decimal::ONE)?;
                    target.insert(key.clone(), Value::String(parsed.to_string()));
                }
                "max_due_date_days" => {
                    let Some(value) = value.as_i64() else {
                        return Err(ApiError::validation(
                            "settings.validation.max_due_date_days must be an integer",
                        ));
                    };
                    if !(0..=3650).contains(&value) {
                        return Err(ApiError::validation(
                            "settings.validation.max_due_date_days must be between 0 and 3650",
                        ));
                    }
                    target.insert(key.clone(), Value::Number(value.into()));
                }
                _ => {
                    return Err(ApiError::validation(format!(
                        "unsupported settings.validation key '{key}'"
                    )));
                }
            }
        }
    }
    if let Some(extraction) = patch.get("extraction") {
        let Some(extraction) = extraction.as_object() else {
            return Err(ApiError::validation(
                "settings.extraction must be an object",
            ));
        };
        let target = current
            .entry("extraction".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if !target.is_object() {
            *target = Value::Object(Map::new());
        }
        let target = target.as_object_mut().expect("object inserted above");
        for (key, value) in extraction {
            if key != "min_ocr_quality" {
                return Err(ApiError::validation(format!(
                    "unsupported settings.extraction key '{key}'"
                )));
            }
            let parsed = parse_decimal_setting(value, key, Decimal::ONE)?;
            target.insert(key.clone(), Value::String(parsed.to_string()));
        }
    }
    Ok(())
}

fn parse_decimal_setting(value: &Value, key: &str, max: Decimal) -> ApiResult<Decimal> {
    let raw = match value {
        Value::String(raw) => raw.clone(),
        Value::Number(number) => number.to_string(),
        _ => {
            return Err(ApiError::validation(format!(
                "settings value {key} must be a decimal string"
            )));
        }
    };
    let parsed = Decimal::from_str(raw.trim())
        .map_err(|_| ApiError::validation(format!("settings value {key} is not a decimal")))?;
    if parsed < Decimal::ZERO || parsed > max {
        return Err(ApiError::validation(format!(
            "settings value {key} must be between 0 and {max}"
        )));
    }
    Ok(parsed)
}

// ---------------------------------------------------------------------------
// Team
// ---------------------------------------------------------------------------

pub async fn list_members(
    State(state): State<AppState>,
    auth: AuthContext,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ManageMembers, "members:write")?;
    let members = auth::list_members(&state.db, auth.organization_id).await?;
    let invitations = auth::list_invitations(&state.db, auth.organization_id).await?;
    Ok(Json(json!({
        "members": members.iter().map(member_json).collect::<Vec<_>>(),
        "invitations": invitations.iter().map(invitation_json).collect::<Vec<_>>(),
        "roles": [
            {"value":"owner","label":"Owner"},
            {"value":"admin","label":"Admin"},
            {"value":"accountant","label":"Accountant"},
            {"value":"reviewer","label":"Reviewer"},
            {"value":"read_only","label":"Read-only"}
        ],
        "email_delivery": false,
    })))
}

pub async fn invite_member(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiJson(input): ApiJson<InviteInput>,
) -> ApiResult<impl axum::response::IntoResponse> {
    let actor = auth.session_user_id()?;
    auth.require(Permission::ManageMembers)?;
    let email = validate_email(&input.email)?;
    let role = Role::parse(&input.role).ok_or_else(|| ApiError::validation("role is invalid"))?;
    if role == Role::Owner && auth.role != Some(Role::Owner) {
        return Err(ApiError::forbidden(
            "Only an owner can invite another owner.",
        ));
    }
    let members = auth::list_members(&state.db, auth.organization_id).await?;
    if members
        .iter()
        .any(|member| member.email.eq_ignore_ascii_case(&email))
    {
        return Err(ApiError::conflict("This person is already a member."));
    }
    let days = input.expires_in_days.unwrap_or(7);
    if !(1..=30).contains(&days) {
        return Err(ApiError::validation(
            "expires_in_days must be between 1 and 30",
        ));
    }
    let token = generate_invitation_token()
        .map_err(|error| ApiError::internal("generate invitation", error))?;
    let invitation_id = auth::create_invitation(
        &state.db,
        auth.organization_id,
        &email,
        role.as_str(),
        &hash_token(&token),
        actor,
        Utc::now() + Duration::days(days),
    )
    .await
    .map_err(|error| {
        if matches!(&error, sqlx::Error::Database(d) if d.is_unique_violation()) {
            ApiError::conflict("A pending invitation already exists for this email.")
        } else {
            ApiError::internal("create invitation", error)
        }
    })?;
    audit(
        &state,
        &auth,
        "member.invited",
        Some("invitation"),
        Some(invitation_id),
        json!({"email": email, "role": role.as_str()}),
    )
    .await;
    let invitations = auth::list_invitations(&state.db, auth.organization_id).await?;
    let row = invitations
        .iter()
        .find(|row| row.id == invitation_id)
        .ok_or_else(|| ApiError::internal("invitation", "created invitation disappeared"))?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "invitation": invitation_json(row),
            "invite_url": format!("/accept-invitation?token={token}"),
            "token": token,
            "delivery": "manual_link",
            "note": "Email delivery is not configured. Copy this single-use link and send it to the invitee securely."
        })),
    ))
}

pub async fn revoke_invitation(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(invitation_id): ApiPath<Uuid>,
) -> ApiResult<impl axum::response::IntoResponse> {
    auth.session_user_id()?;
    auth.require(Permission::ManageMembers)?;
    let revoked = auth::revoke_invitation(&state.db, auth.organization_id, invitation_id).await?;
    if !revoked {
        return Err(ApiError::not_found(
            "Invitation not found or already accepted.",
        ));
    }
    audit(
        &state,
        &auth,
        "member.invitation_revoked",
        Some("invitation"),
        Some(invitation_id),
        json!({}),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn update_member_role(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(user_id): ApiPath<Uuid>,
    ApiJson(input): ApiJson<MemberRoleInput>,
) -> ApiResult<Json<Value>> {
    let actor = auth.session_user_id()?;
    auth.require(Permission::ManageMembers)?;
    let role = Role::parse(&input.role).ok_or_else(|| ApiError::validation("role is invalid"))?;
    let members = auth::list_members(&state.db, auth.organization_id).await?;
    let target = members
        .iter()
        .find(|member| member.user_id == user_id)
        .ok_or_else(|| ApiError::not_found("Member not found."))?;
    let target_role = Role::parse(&target.role)
        .ok_or_else(|| ApiError::internal("role", "stored role is invalid"))?;
    if (target_role == Role::Owner || role == Role::Owner) && auth.role != Some(Role::Owner) {
        return Err(ApiError::forbidden(
            "Only an owner can change an owner membership.",
        ));
    }
    if target_role == Role::Owner
        && role != Role::Owner
        && auth::count_owners(&state.db, auth.organization_id).await? <= 1
    {
        return Err(ApiError::conflict(
            "The organization must keep at least one owner.",
        ));
    }
    let changed =
        auth::update_member_role(&state.db, auth.organization_id, user_id, role.as_str()).await?;
    if !changed {
        return Err(ApiError::not_found("Member not found."));
    }
    audit(
        &state,
        &auth,
        "member.role_changed",
        Some("user"),
        Some(user_id),
        json!({"from": target_role.as_str(), "to": role.as_str(), "actor": actor}),
    )
    .await;
    Ok(Json(json!({"user_id": user_id, "role": role.as_str()})))
}

pub async fn remove_member(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(user_id): ApiPath<Uuid>,
) -> ApiResult<impl axum::response::IntoResponse> {
    let actor = auth.session_user_id()?;
    auth.require(Permission::ManageMembers)?;
    let members = auth::list_members(&state.db, auth.organization_id).await?;
    let target = members
        .iter()
        .find(|member| member.user_id == user_id)
        .ok_or_else(|| ApiError::not_found("Member not found."))?;
    let target_role = Role::parse(&target.role)
        .ok_or_else(|| ApiError::internal("role", "stored role is invalid"))?;
    if target_role == Role::Owner && auth.role != Some(Role::Owner) {
        return Err(ApiError::forbidden("Only an owner can remove an owner."));
    }
    if target_role == Role::Owner && auth::count_owners(&state.db, auth.organization_id).await? <= 1
    {
        return Err(ApiError::conflict(
            "The organization must keep at least one owner.",
        ));
    }
    if user_id == actor {
        return Err(ApiError::validation(
            "You cannot remove yourself from the organization.",
        ));
    }
    let removed = auth::remove_member(&state.db, auth.organization_id, user_id).await?;
    if !removed {
        return Err(ApiError::not_found("Member not found."));
    }
    // Revoke all of the removed user's sessions. If they belong to other
    // organizations they can sign in again, but this session loses access.
    let _ = auth::revoke_all_sessions(&state.db, user_id).await;
    audit(
        &state,
        &auth,
        "member.removed",
        Some("user"),
        Some(user_id),
        json!({"role": target_role.as_str(), "actor": actor}),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Usage and integrations
// ---------------------------------------------------------------------------

pub async fn usage(State(state): State<AppState>, auth: AuthContext) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ViewDocuments, "organization:read")?;
    let summary = operations::usage_summary(&state.db, auth.organization_id).await?;
    Ok(Json(json!({
        "period": "current_calendar_month",
        "documents_this_month": summary.documents_this_month,
        "pages_this_month": summary.pages_this_month,
        "ai_requests_this_month": summary.ai_requests_this_month,
        "ocr_pages_this_month": summary.ocr_pages_this_month,
        "storage_bytes": summary.storage_bytes,
        "estimated_ai_cost_usd": match summary.estimated_ai_cost_usd {
            Some(value) => Value::String(value.to_string()),
            None => Value::Null,
        },
        "billing_configured": false,
    })))
}

pub async fn integrations(
    State(state): State<AppState>,
    auth: AuthContext,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ManageIntegrations, "organization:read")?;
    let rows = operations::list_integrations(&state.db, auth.organization_id).await?;
    Ok(Json(json!({
        "configured": rows.iter().map(integration_json).collect::<Vec<_>>(),
        "catalog": [
            {
                "provider": "quickbooks",
                "name": "QuickBooks Online",
                "status": "not_implemented",
                "description": "OAuth sync is not included in this build. CSV and JSON exports are available."
            },
            {
                "provider": "xero",
                "name": "Xero",
                "status": "not_implemented",
                "description": "OAuth sync is not included in this build. CSV and JSON exports are available."
            },
            {
                "provider": "tally",
                "name": "Tally",
                "status": "not_implemented",
                "description": "Tally XML export is not included in this build."
            },
            {
                "provider": "google_drive",
                "name": "Google Drive",
                "status": "not_implemented",
                "description": "Cloud document import is not included in this build. Upload files directly."
            },
            {
                "provider": "email",
                "name": "Email ingestion",
                "status": "not_configured",
                "description": "Inbound email delivery and invitation emails are not configured."
            }
        ],
        "webhooks_note": "Use API & Webhooks for signed outbound event delivery."
    })))
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
