use crate::auth::{audit, AuthContext};
use crate::error::{ApiError, ApiResult};
use crate::presenters::export_json;
use crate::state::AppState;
use crate::util::{parse_iso_date, ApiJson, ApiPath, ApiQuery};
use axum::body::Body;
use axum::extract::State;
use axum::http::{header, Response, StatusCode};
use axum::Json;
use invoxa_database::{invoices as db_invoices, operations, vendors};
use invoxa_domain::role::Permission;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use uuid::Uuid;

const EXPORT_MAX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Deserialize)]
pub struct ExportInput {
    pub format: String,
    #[serde(default)]
    pub filters: Option<Value>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Deserialize)]
pub struct ExportListQuery {
    pub limit: Option<i64>,
}

pub async fn create(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiJson(input): ApiJson<ExportInput>,
) -> ApiResult<impl axum::response::IntoResponse> {
    auth.require_scope(Permission::ExportInvoices, "exports:write")?;
    let format = input.format.trim().to_ascii_lowercase();
    if !matches!(format.as_str(), "csv" | "json") {
        return Err(ApiError::validation("format must be 'csv' or 'json'"));
    }
    let filters = validate_filters(&state, &auth, input.filters.as_ref()).await?;
    let idempotency_key = input
        .idempotency_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(key) = idempotency_key {
        if key.len() > 100
            || !key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'))
        {
            return Err(ApiError::validation(
                "idempotency_key must be 1-100 URL-safe characters",
            ));
        }
    }
    let actor = auth.actor_id()?;
    let job = operations::create_export_job(
        &state.db,
        auth.organization_id,
        actor,
        &format,
        &filters,
        idempotency_key,
    )
    .await?;
    audit(
        &state,
        &auth,
        "export.requested",
        Some("export"),
        Some(job.id),
        json!({"format": format, "filters": filters}),
    )
    .await;
    Ok((StatusCode::ACCEPTED, Json(export_json(&job))))
}

async fn validate_filters(
    state: &AppState,
    auth: &AuthContext,
    filters: Option<&Value>,
) -> ApiResult<Value> {
    let mut output = Map::new();
    let Some(filters) = filters else {
        return Ok(Value::Object(output));
    };
    if filters.is_null() {
        return Ok(Value::Object(output));
    }
    let Some(object) = filters.as_object() else {
        return Err(ApiError::validation("filters must be an object"));
    };
    for key in object.keys() {
        if !matches!(
            key.as_str(),
            "from" | "to" | "vendor_id" | "invoice_ids" | "status"
        ) {
            return Err(ApiError::validation(format!(
                "unsupported export filter '{key}'"
            )));
        }
    }
    let export_status = match object.get("status") {
        None | Some(Value::Null) => "approved".to_string(),
        Some(Value::String(raw)) if raw.trim().is_empty() => "approved".to_string(),
        Some(Value::String(raw)) if matches!(raw.trim(), "approved" | "exported") => {
            raw.trim().to_string()
        }
        _ => {
            return Err(ApiError::validation(
                "filters.status must be 'approved' or 'exported'",
            ))
        }
    };
    output.insert("status".into(), Value::String(export_status.clone()));
    let from = match object.get("from") {
        None | Some(Value::Null) => None,
        Some(Value::String(raw)) if raw.trim().is_empty() => None,
        Some(Value::String(raw)) => Some(parse_iso_date(raw, "filters.from")?),
        _ => return Err(ApiError::validation("filters.from must be a date string")),
    };
    let to = match object.get("to") {
        None | Some(Value::Null) => None,
        Some(Value::String(raw)) if raw.trim().is_empty() => None,
        Some(Value::String(raw)) => Some(parse_iso_date(raw, "filters.to")?),
        _ => return Err(ApiError::validation("filters.to must be a date string")),
    };
    if let (Some(from), Some(to)) = (from, to) {
        if from > to {
            return Err(ApiError::validation(
                "filters.from must not be after filters.to",
            ));
        }
        output.insert("from".into(), Value::String(from.to_string()));
        output.insert("to".into(), Value::String(to.to_string()));
    } else {
        if let Some(from) = from {
            output.insert("from".into(), Value::String(from.to_string()));
        }
        if let Some(to) = to {
            output.insert("to".into(), Value::String(to.to_string()));
        }
    }
    if let Some(value) = object.get("vendor_id") {
        match value {
            Value::Null => {}
            Value::String(raw) if raw.trim().is_empty() => {}
            Value::String(raw) => {
                let vendor_id = Uuid::parse_str(raw.trim())
                    .map_err(|_| ApiError::validation("filters.vendor_id must be a UUID"))?;
                vendors::get_vendor(&state.db, auth.organization_id, vendor_id)
                    .await?
                    .ok_or_else(|| ApiError::validation("filters.vendor_id was not found"))?;
                output.insert("vendor_id".into(), Value::String(vendor_id.to_string()));
            }
            _ => return Err(ApiError::validation("filters.vendor_id must be a UUID")),
        }
    }
    if let Some(value) = object.get("invoice_ids") {
        match value {
            Value::Null => {}
            Value::Array(items) => {
                if items.is_empty() {
                    return Err(ApiError::validation(
                        "filters.invoice_ids must contain at least one invoice id",
                    ));
                }
                if items.len() > 500 {
                    return Err(ApiError::validation(
                        "filters.invoice_ids may contain at most 500 invoices",
                    ));
                }
                let mut invoice_ids = Vec::with_capacity(items.len());
                for item in items {
                    let raw = item.as_str().ok_or_else(|| {
                        ApiError::validation("filters.invoice_ids must contain UUID strings")
                    })?;
                    let invoice_id = Uuid::parse_str(raw.trim()).map_err(|_| {
                        ApiError::validation("filters.invoice_ids must contain UUID strings")
                    })?;
                    if !invoice_ids.contains(&invoice_id) {
                        invoice_ids.push(invoice_id);
                    }
                }
                let statuses = db_invoices::selected_invoice_statuses(
                    &state.db,
                    auth.organization_id,
                    &invoice_ids,
                )
                .await?;
                let all_eligible = statuses.len() == invoice_ids.len()
                    && statuses.iter().all(|(_, status)| status == &export_status);
                if !all_eligible {
                    return Err(ApiError::validation(
                        "Every selected invoice must belong to this organization and match the requested post-approval status before it can be exported.",
                    )
                    .with_details(json!({"code": "invoice_ids_invalid"})));
                }
                output.insert(
                    "invoice_ids".into(),
                    Value::Array(
                        invoice_ids
                            .into_iter()
                            .map(|id| Value::String(id.to_string()))
                            .collect(),
                    ),
                );
            }
            _ => {
                return Err(ApiError::validation(
                    "filters.invoice_ids must be an array of UUID strings",
                ))
            }
        }
    }
    Ok(Value::Object(output))
}

pub async fn list(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiQuery(query): ApiQuery<ExportListQuery>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ViewDocuments, "exports:read")?;
    let limit = query.limit.unwrap_or(25).clamp(1, 100);
    let rows = operations::list_export_jobs(&state.db, auth.organization_id, limit).await?;
    Ok(Json(json!({
        "items": rows.iter().map(export_json).collect::<Vec<_>>(),
        "limit": limit,
    })))
}

pub async fn detail(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(export_id): ApiPath<Uuid>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ViewDocuments, "exports:read")?;
    let job = operations::get_export_job(&state.db, auth.organization_id, export_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Export not found."))?;
    Ok(Json(export_json(&job)))
}

pub async fn download(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(export_id): ApiPath<Uuid>,
) -> ApiResult<Response<Body>> {
    auth.require_scope(Permission::ViewDocuments, "exports:read")?;
    let job = operations::get_export_job(&state.db, auth.organization_id, export_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Export not found."))?;
    if job.status != "succeeded" {
        return Err(ApiError::conflict("The export is not ready for download."));
    }
    if job
        .expires_at
        .map(|expires| expires < chrono::Utc::now())
        .unwrap_or(false)
    {
        return Err(ApiError::new(
            StatusCode::GONE,
            "export_expired",
            "This export artifact has expired. Request a new export.",
        ));
    }
    let key = job
        .storage_key
        .as_deref()
        .ok_or_else(|| ApiError::internal("export", "export has no stored artifact"))?;
    let bytes = state.storage.get_bytes(key, EXPORT_MAX_BYTES).await?;
    let filename = job
        .filename
        .as_deref()
        .unwrap_or("invoxa-export")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect::<String>();
    let filename = if filename.is_empty() {
        "invoxa-export".to_string()
    } else {
        filename
    };
    let content_type = if job.format == "json" {
        "application/json"
    } else {
        "text/csv; charset=utf-8"
    };
    audit(
        &state,
        &auth,
        "export.downloaded",
        Some("export"),
        Some(export_id),
        json!({"format": job.format, "row_count": job.row_count}),
    )
    .await;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )
        .header(header::CACHE_CONTROL, "private, no-store")
        .body(Body::from(bytes))
        .map_err(|error| ApiError::internal("build export download", error))
}
