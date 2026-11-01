use crate::auth::{audit, AuthContext};
use crate::error::{ApiError, ApiResult};
use crate::presenters::{dec, document_event_json, document_json, document_list_json};
use crate::state::AppState;
use crate::util::{limit_offset, ApiJson, ApiPath, ApiQuery, ClientIp};
use axum::body::Body;
use axum::extract::{Multipart, State};
use axum::http::{header, Response, StatusCode};
use axum::Json;
use chrono::{Duration, Utc};
use invoxa_database::{documents, invoices, jobs, operations};
use invoxa_documents::validation::validate_upload;
use invoxa_domain::status::DocumentStatus;
use invoxa_security::{sign_download_grant, verify_download_grant, DownloadGrant};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct DocumentFilters {
    pub status: Option<String>,
    pub search: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Deserialize)]
pub struct RetryInput {
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Deserialize)]
pub struct DownloadUrlInput {
    #[serde(default)]
    pub disposition: Option<String>,
}

pub async fn upload(
    State(state): State<AppState>,
    ClientIp(_ip): ClientIp,
    auth: AuthContext,
    mut multipart: Multipart,
) -> ApiResult<impl axum::response::IntoResponse> {
    auth.require_scope(
        invoxa_domain::role::Permission::UploadDocuments,
        "documents:write",
    )?;
    if let Some(retry_after) = state
        .rate_limiter
        .check(
            &format!("upload:{}", uploader_key(&auth)),
            30,
            std::time::Duration::from_secs(60),
        )
        .await
    {
        return Err(ApiError::rate_limited(retry_after));
    }
    let uploader = auth.actor_id()?;
    let source = if auth.is_api_key() { "api" } else { "web" };
    let temp_dir = state.config.storage_root.join(".tmp");
    tokio::fs::create_dir_all(&temp_dir)
        .await
        .map_err(|error| ApiError::internal("create upload temp dir", error))?;

    let mut uploaded: Vec<Value> = Vec::new();
    let mut failed: Vec<Value> = Vec::new();
    let mut file_index = 0usize;
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| ApiError::bad_request("The multipart upload could not be parsed."))?
    {
        let Some(raw_filename) = field.file_name().map(|name| name.to_string()) else {
            continue;
        };
        let field_name = field.name().unwrap_or("file").to_string();
        if !matches!(field_name.as_str(), "file" | "files" | "documents") {
            continue;
        }
        let display_name = invoxa_documents::validation::sanitize_filename(&raw_filename);
        file_index += 1;
        if file_index > state.config.upload_batch_limit {
            failed.push(json!({
                "filename": display_name,
                "code": "batch_limit_exceeded",
                "message": format!(
                    "At most {} files may be uploaded in one batch.",
                    state.config.upload_batch_limit
                ),
            }));
            break;
        }

        let temp_path = temp_dir.join(format!("{}.upload", Uuid::new_v4()));
        let read_result = stream_field_to_disk(
            &mut field,
            &temp_path,
            state.config.upload_limits.max_file_bytes,
        )
        .await;
        if let Err(error) = read_result {
            let _ = tokio::fs::remove_file(&temp_path).await;
            if error.status == StatusCode::INTERNAL_SERVER_ERROR
                || error.status == StatusCode::PAYLOAD_TOO_LARGE
            {
                failed.push(json!({
                    "filename": display_name,
                    "code": error.code,
                    "message": error.message,
                }));
                continue;
            }
            failed.push(json!({
                "filename": display_name,
                "code": error.code,
                "message": error.message,
            }));
            continue;
        }

        let processed =
            process_uploaded_file(&state, &auth, uploader, source, &display_name, &temp_path).await;
        let _ = tokio::fs::remove_file(&temp_path).await;
        match processed {
            Ok((document, job_id)) => {
                let doc_json = document_json(&document);
                try_emit_upload_event(&state, &auth, &document).await;
                record_upload_usage(&state, &auth, &document).await;
                audit(
                    &state,
                    &auth,
                    "document.uploaded",
                    Some("document"),
                    Some(document.id),
                    json!({
                        "filename": document.original_filename,
                        "size_bytes": document.size_bytes,
                        "sha256": document.sha256,
                        "job_id": job_id,
                    }),
                )
                .await;
                uploaded.push(json!({
                    "document": doc_json,
                    "job_id": job_id,
                }));
            }
            Err(error) => {
                if error.status == StatusCode::INTERNAL_SERVER_ERROR {
                    return Err(error);
                }
                failed.push(json!({
                    "filename": display_name,
                    "code": error.code,
                    "message": error.message,
                }));
            }
        }
    }

    if uploaded.is_empty() {
        if let Some(first) = failed.first() {
            return Err(ApiError::validation(format!(
                "No documents were accepted. {}",
                first
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("Check the file and try again.")
            ))
            .with_details(json!({"files": failed})));
        }
        return Err(ApiError::bad_request(
            "No file was provided. Use the 'files' multipart field.",
        ));
    }

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "documents": uploaded,
            "errors": failed,
            "batch_limit": state.config.upload_batch_limit,
        })),
    ))
}

fn uploader_key(auth: &AuthContext) -> String {
    auth.user_id
        .map(|id| format!("user:{id}"))
        .or_else(|| auth.api_key_id.map(|id| format!("key:{id}")))
        .unwrap_or_else(|| "unknown".to_string())
}

async fn stream_field_to_disk(
    field: &mut axum::extract::multipart::Field<'_>,
    path: &Path,
    max_bytes: u64,
) -> ApiResult<u64> {
    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::File::create(path)
        .await
        .map_err(|error| ApiError::internal("create upload temp file", error))?;
    let mut total = 0u64;
    loop {
        let chunk = field
            .chunk()
            .await
            .map_err(|_| ApiError::bad_request("The file upload stream was interrupted."))?;
        let Some(chunk) = chunk else {
            break;
        };
        total = total.saturating_add(chunk.len() as u64);
        if total > max_bytes {
            let _ = file.flush().await;
            return Err(ApiError::payload_too_large(format!(
                "Each file must be at most {} MiB.",
                max_bytes / 1024 / 1024
            )));
        }
        file.write_all(&chunk)
            .await
            .map_err(|error| ApiError::internal("write upload temp file", error))?;
    }
    file.flush()
        .await
        .map_err(|error| ApiError::internal("flush upload temp file", error))?;
    if total == 0 {
        return Err(ApiError::validation("The uploaded file is empty."));
    }
    Ok(total)
}

async fn process_uploaded_file(
    state: &AppState,
    auth: &AuthContext,
    uploader: Uuid,
    source: &str,
    filename: &str,
    temp_path: &Path,
) -> ApiResult<(documents::DocumentRow, Uuid)> {
    let preflight = validate_upload(temp_path, filename, &state.config.upload_limits)
        .await
        .map_err(|error| ApiError::validation(error.to_string()))?;
    let sha256 = invoxa_documents::validation::sha256_file(temp_path)
        .await
        .map_err(|error| ApiError::internal("hash upload", error))?;
    let size_bytes = tokio::fs::metadata(temp_path)
        .await
        .map_err(|error| ApiError::internal("upload metadata", error))?
        .len();
    let storage_key = format!(
        "{}/{}.{}",
        auth.organization_id,
        Uuid::new_v4(),
        preflight.extension
    );
    state
        .storage
        .put_file(&storage_key, temp_path)
        .await
        .map_err(|error| ApiError::internal("store upload", error))?;

    let new_document = documents::NewDocument {
        organization_id: auth.organization_id,
        uploaded_by: uploader,
        source,
        original_filename: filename,
        storage_key: &storage_key,
        content_type: preflight.content_type,
        sha256: &sha256,
        size_bytes: size_bytes as i64,
    };
    let (document, job_id) =
        match documents::create_document_with_job(&state.db, &new_document).await {
            Ok(value) => value,
            Err(error) => {
                let _ = state.storage.delete(&storage_key).await;
                return Err(error.into());
            }
        };
    if let Err(error) = documents::set_preflight_metadata(
        &state.db,
        auth.organization_id,
        document.id,
        preflight.page_count as i32,
        "unknown",
    )
    .await
    {
        tracing::error!(document_id = %document.id, error = %error, "could not store preflight metadata");
    }
    let mut document = document;
    document.page_count = Some(preflight.page_count as i32);
    Ok((document, job_id))
}

async fn try_emit_upload_event(
    state: &AppState,
    auth: &AuthContext,
    document: &documents::DocumentRow,
) {
    if let Err(error) = invoxa_workflows::events::emit_document_event(
        &state.db,
        auth.organization_id,
        invoxa_workflows::events::EVENT_DOCUMENT_UPLOADED,
        document.id,
        0,
        Some(json!({"source": document.source})),
    )
    .await
    {
        tracing::error!(document_id = %document.id, error = %error, "could not queue document.uploaded webhook");
    }
}

async fn record_upload_usage(
    state: &AppState,
    auth: &AuthContext,
    document: &documents::DocumentRow,
) {
    let quantity = Decimal::from(document.page_count.unwrap_or(1).max(1));
    let input = operations::UsageInput {
        organization_id: auth.organization_id,
        document_id: Some(document.id),
        kind: "page_processed",
        quantity,
        provider: None,
        model: None,
        input_tokens: None,
        output_tokens: None,
        duration_ms: None,
        estimated_cost_usd: None,
        metadata: json!({"source": "upload"}),
    };
    if let Err(error) = operations::record_usage(&state.db, &input).await {
        tracing::error!(document_id = %document.id, error = %error, "could not record upload usage");
    }
}

pub async fn list(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiQuery(filters): ApiQuery<DocumentFilters>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(
        invoxa_domain::role::Permission::ViewDocuments,
        "documents:read",
    )?;
    let (limit, offset) = limit_offset(filters.limit, filters.offset, 25);
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
    let status = filters
        .status
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(status) = status {
        DocumentStatus::parse(status)
            .ok_or_else(|| ApiError::validation("Unknown document status filter."))?;
    }
    let (rows, total) = documents::list_documents(
        &state.db,
        auth.organization_id,
        status,
        search,
        limit,
        offset,
    )
    .await?;
    Ok(Json(json!({
        "items": rows.iter().map(document_list_json).collect::<Vec<_>>(),
        "total": total,
        "limit": limit,
        "offset": offset,
    })))
}

pub async fn detail(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(document_id): ApiPath<Uuid>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(
        invoxa_domain::role::Permission::ViewDocuments,
        "documents:read",
    )?;
    let document = documents::get_document(&state.db, auth.organization_id, document_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Document not found."))?;
    let events = documents::document_events(&state.db, auth.organization_id, document_id).await?;
    let job_rows = jobs::jobs_for_document(&state.db, auth.organization_id, document_id).await?;
    let invoice =
        invoices::find_invoice_by_document(&state.db, auth.organization_id, document_id).await?;
    let mut detail = document_json(&document);
    if let Some(object) = detail.as_object_mut() {
        object.insert(
            "events".into(),
            Value::Array(events.iter().map(document_event_json).collect()),
        );
        object.insert(
            "jobs".into(),
            Value::Array(job_rows.iter().map(job_json).collect()),
        );
        object.insert(
            "invoice".into(),
            invoice
                .as_ref()
                .map(invoice_list_like)
                .unwrap_or(Value::Null),
        );
    }
    Ok(Json(detail))
}

fn invoice_list_like(row: &invoices::InvoiceRow) -> Value {
    json!({
        "id": row.id,
        "status": row.status,
        "vendor_name": row.vendor_name,
        "invoice_number": row.invoice_number,
        "currency": row.currency,
        "total": dec(row.total),
        "version": row.version,
        "review_url": format!("/invoices/{}", row.id),
    })
}

fn job_json(row: &jobs::JobRow) -> Value {
    json!({
        "id": row.id,
        "status": row.status,
        "attempts": row.attempts,
        "max_attempts": row.max_attempts,
        "run_at": row.run_at,
        "stage": row.stage,
        "error_code": row.last_error_code,
        "error_message": row.last_error_message,
        "started_at": row.started_at,
        "completed_at": row.completed_at,
        "created_at": row.created_at,
        "updated_at": row.updated_at,
    })
}

pub async fn job_detail(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(job_id): ApiPath<Uuid>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(
        invoxa_domain::role::Permission::ViewDocuments,
        "documents:read",
    )?;
    let job = jobs::get_job(&state.db, auth.organization_id, job_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Processing job not found."))?;
    Ok(Json(job_json(&job)))
}

pub async fn events(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(document_id): ApiPath<Uuid>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(
        invoxa_domain::role::Permission::ViewDocuments,
        "documents:read",
    )?;
    documents::get_document(&state.db, auth.organization_id, document_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Document not found."))?;
    let rows = documents::document_events(&state.db, auth.organization_id, document_id).await?;
    Ok(Json(json!({
        "items": rows.iter().map(document_event_json).collect::<Vec<_>>()
    })))
}

pub async fn content(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(document_id): ApiPath<Uuid>,
) -> ApiResult<Response<Body>> {
    auth.require_scope(
        invoxa_domain::role::Permission::ViewDocuments,
        "documents:read",
    )?;
    let document = documents::get_document(&state.db, auth.organization_id, document_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Document not found."))?;
    let bytes = state
        .storage
        .get_bytes(
            &document.storage_key,
            state.config.upload_limits.max_file_bytes as usize,
        )
        .await?;
    let disposition = if document.content_type == "application/pdf" {
        "inline"
    } else {
        "inline"
    };
    file_response(
        &document.original_filename,
        &document.content_type,
        disposition,
        bytes,
    )
}

pub async fn create_download_url(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(document_id): ApiPath<Uuid>,
    ApiJson(input): ApiJson<DownloadUrlInput>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(
        invoxa_domain::role::Permission::ViewDocuments,
        "documents:read",
    )?;
    documents::get_document(&state.db, auth.organization_id, document_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Document not found."))?;
    let actor = auth.actor_id()?;
    let expires_at = Utc::now() + Duration::minutes(5);
    let disposition = match input.disposition.as_deref().unwrap_or("inline") {
        "attachment" => "attachment",
        _ => "inline",
    };
    let grant = DownloadGrant {
        organization_id: auth.organization_id,
        document_id,
        user_id: actor,
        expires_at: expires_at.timestamp(),
    };
    let token = sign_download_grant(&state.config.download_signing_key, &grant)
        .map_err(|error| ApiError::internal("sign download grant", error))?;
    let path = format!("/api/v1/downloads/{token}?disposition={disposition}");
    audit(
        &state,
        &auth,
        "document.download_url_created",
        Some("document"),
        Some(document_id),
        json!({"expires_at": expires_at}),
    )
    .await;
    Ok(Json(json!({
        "url": path,
        "expires_at": expires_at,
        "single_use": false,
    })))
}

#[derive(Deserialize)]
pub struct GrantQuery {
    #[serde(default)]
    pub disposition: Option<String>,
}

pub async fn download_with_grant(
    State(state): State<AppState>,
    ApiPath(token): ApiPath<String>,
    ApiQuery(query): ApiQuery<GrantQuery>,
) -> ApiResult<Response<Body>> {
    let grant = verify_download_grant(
        &state.config.download_signing_key,
        &token,
        Utc::now().timestamp(),
    )
    .map_err(|_| ApiError::unauthorized("The download link is invalid or has expired."))?;
    let document = documents::get_document(&state.db, grant.organization_id, grant.document_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Document not found."))?;
    let bytes = state
        .storage
        .get_bytes(
            &document.storage_key,
            state.config.upload_limits.max_file_bytes as usize,
        )
        .await?;
    let disposition = match query.disposition.as_deref().unwrap_or("inline") {
        "attachment" => "attachment",
        _ => "inline",
    };
    file_response(
        &document.original_filename,
        &document.content_type,
        disposition,
        bytes,
    )
}

fn file_response(
    filename: &str,
    content_type: &str,
    disposition: &str,
    bytes: Vec<u8>,
) -> ApiResult<Response<Body>> {
    let safe_name: String = filename
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ' '))
        .take(120)
        .collect();
    let safe_name = if safe_name.trim().is_empty() {
        "document".to_string()
    } else {
        safe_name
    };
    let content_type = match content_type {
        "application/pdf" | "image/png" | "image/jpeg" | "image/tiff" => content_type,
        _ => "application/octet-stream",
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(
            header::CONTENT_DISPOSITION,
            format!("{disposition}; filename=\"{safe_name}\""),
        )
        .header(header::CACHE_CONTROL, "private, no-store")
        .header("x-content-type-options", "nosniff")
        .body(Body::from(bytes))
        .map_err(|error| ApiError::internal("build download response", error))
}

pub async fn retry(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(document_id): ApiPath<Uuid>,
    ApiJson(input): ApiJson<RetryInput>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(
        invoxa_domain::role::Permission::UploadDocuments,
        "documents:write",
    )?;
    let job_id = documents::retry_document(&state.db, auth.organization_id, document_id).await?;
    audit(
        &state,
        &auth,
        "document.retry_requested",
        Some("document"),
        Some(document_id),
        json!({"job_id": job_id, "reason": input.reason}),
    )
    .await;
    Ok(Json(json!({"job_id": job_id, "status": "queued"})))
}

pub async fn delete(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiPath(document_id): ApiPath<Uuid>,
) -> ApiResult<impl axum::response::IntoResponse> {
    auth.require(DocumentPermission::DeleteDocuments)?;
    let invoice =
        invoices::find_invoice_by_document(&state.db, auth.organization_id, document_id).await?;
    if let Some(invoice) = &invoice {
        if matches!(invoice.status.as_str(), "approved" | "exported") {
            return Err(ApiError::conflict(
                "Approved and exported records are retained and cannot be deleted.",
            ));
        }
    }
    let Some(document) =
        documents::delete_document(&state.db, auth.organization_id, document_id).await?
    else {
        return Err(ApiError::not_found("Document not found."));
    };
    if invoice.is_none() {
        if let Err(error) = state.storage.delete(&document.storage_key).await {
            tracing::warn!(document_id = %document_id, error = %error, "could not delete stored file");
        }
    }
    audit(
        &state,
        &auth,
        "document.deleted",
        Some("document"),
        Some(document_id),
        json!({"filename": document.original_filename}),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// Small local alias to avoid importing the whole Permission enum name at each
// call site.
use invoxa_domain::role::Permission as DocumentPermission;
