use crate::auth::{audit, AuthContext};
use crate::error::{ApiError, ApiResult};
use crate::presenters::{vendor_invoice_json, vendor_json, vendor_list_json};
use crate::state::AppState;
use crate::util::{limit_offset, optional_text, required_text, ApiJson, ApiPath, ApiQuery};
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use invoxa_database::vendors;
use invoxa_domain::role::Permission;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct VendorFilters {
    pub search: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Deserialize)]
pub struct VendorInput {
    pub name: String,
    #[serde(default)]
    pub tax_id: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
}

#[derive(Deserialize)]
pub struct VendorSuggestionQuery {
    pub q: String,
    pub limit: Option<i64>,
}

pub async fn list(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiQuery(filters): ApiQuery<VendorFilters>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ViewDocuments, "vendors:read")?;
    let (limit, offset) = limit_offset(filters.limit, filters.offset, 50);
    let search = filters
        .search
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if search.map(str::len).unwrap_or(0) > 200 {
        return Err(ApiError::validation(
            "search must be at most 200 characters",
        ));
    }
    let (rows, total) =
        vendors::list_vendors(&state.db, auth.organization_id, search, limit, offset).await?;
    Ok(Json(json!({
        "items": rows.iter().map(vendor_list_json).collect::<Vec<_>>(),
        "total": total,
        "limit": limit,
        "offset": offset,
    })))
}

pub async fn detail(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(vendor_id): ApiPath<Uuid>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ViewDocuments, "vendors:read")?;
    let vendor = vendors::get_vendor(&state.db, auth.organization_id, vendor_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Vendor not found."))?;
    let invoices =
        vendors::vendor_invoices(&state.db, auth.organization_id, vendor_id, 100, 0).await?;
    let mut payload = vendor_json(&vendor);
    if let Some(object) = payload.as_object_mut() {
        object.insert(
            "invoices".into(),
            Value::Array(invoices.iter().map(vendor_invoice_json).collect()),
        );
    }
    Ok(Json(payload))
}

pub async fn suggestions(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiQuery(query): ApiQuery<VendorSuggestionQuery>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ViewDocuments, "vendors:read")?;
    let q = required_text(&query.q, "q", 1, 200)?;
    let limit = query.limit.unwrap_or(10).clamp(1, 25);
    let rows = vendors::suggest_vendors(&state.db, auth.organization_id, &q, limit).await?;
    Ok(Json(json!({
        "items": rows.iter().map(|row| json!({
            "id": row.id,
            "name": row.name,
            "tax_id": row.tax_id,
            "similarity": row.similarity,
        })).collect::<Vec<_>>()
    })))
}

pub async fn create(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiJson(input): ApiJson<VendorInput>,
) -> ApiResult<impl axum::response::IntoResponse> {
    auth.require_scope(Permission::ManageVendors, "vendors:write")?;
    let fields = validate_vendor_input(input)?;
    let vendor = vendors::create_vendor(
        &state.db,
        auth.organization_id,
        &fields.name,
        fields.tax_id.as_deref(),
        fields.email.as_deref(),
        fields.phone.as_deref(),
        fields.address.as_deref(),
    )
    .await?;
    audit(
        &state,
        &auth,
        "vendor.created",
        Some("vendor"),
        Some(vendor.id),
        json!({"name": vendor.name}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(vendor_json(&vendor))))
}

pub async fn update(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(vendor_id): ApiPath<Uuid>,
    ApiJson(input): ApiJson<VendorInput>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ManageVendors, "vendors:write")?;
    let fields = validate_vendor_input(input)?;
    let vendor = vendors::update_vendor(
        &state.db,
        auth.organization_id,
        vendor_id,
        &fields.name,
        fields.tax_id.as_deref(),
        fields.email.as_deref(),
        fields.phone.as_deref(),
        fields.address.as_deref(),
    )
    .await?;
    audit(
        &state,
        &auth,
        "vendor.updated",
        Some("vendor"),
        Some(vendor_id),
        json!({"name": vendor.name}),
    )
    .await;
    Ok(Json(vendor_json(&vendor)))
}

struct ValidVendor {
    name: String,
    tax_id: Option<String>,
    email: Option<String>,
    phone: Option<String>,
    address: Option<String>,
}

fn validate_vendor_input(input: VendorInput) -> ApiResult<ValidVendor> {
    let name = required_text(&input.name, "name", 1, 300)?;
    let tax_id = optional_text(
        input
            .tax_id
            .as_ref()
            .map(|value| Value::String(value.clone()))
            .as_ref(),
        "tax_id",
        60,
    )?;
    let email = optional_text(
        input
            .email
            .as_ref()
            .map(|value| Value::String(value.clone()))
            .as_ref(),
        "email",
        320,
    )?;
    if let Some(email) = &email {
        if !email.contains('@') || email.chars().any(char::is_whitespace) {
            return Err(ApiError::validation("email must be a valid email address"));
        }
    }
    let phone = optional_text(
        input
            .phone
            .as_ref()
            .map(|value| Value::String(value.clone()))
            .as_ref(),
        "phone",
        60,
    )?;
    let address = optional_text(
        input
            .address
            .as_ref()
            .map(|value| Value::String(value.clone()))
            .as_ref(),
        "address",
        1_000,
    )?;
    Ok(ValidVendor {
        name,
        tax_id,
        email,
        phone,
        address,
    })
}
