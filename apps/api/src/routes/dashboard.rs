use crate::auth::AuthContext;
use crate::error::ApiResult;
use crate::presenters::dec;
use crate::state::AppState;
use crate::util::ApiQuery;
use axum::extract::State;
use axum::Json;
use chrono::{Duration, Utc};
use invoxa_database::operations;
use invoxa_domain::role::Permission;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
pub struct DashboardQuery {
    pub days: Option<i64>,
}

pub async fn dashboard(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiQuery(query): ApiQuery<DashboardQuery>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ViewDocuments, "documents:read")?;
    let days = query.days.unwrap_or(30).clamp(7, 365);
    let to = Utc::now();
    let from = to - Duration::days(days);
    let organization_id = auth.organization_id;
    let (counts, status, documents, approved, activity, errors) = tokio::try_join!(
        operations::dashboard_counts(&state.db, organization_id, Some(from), Some(to)),
        operations::status_breakdown(&state.db, organization_id, Some(from), Some(to)),
        operations::daily_documents(&state.db, organization_id, days as i32),
        operations::daily_approved(&state.db, organization_id, days as i32),
        operations::recent_activity(&state.db, organization_id, 8),
        operations::recent_errors(&state.db, organization_id, 5),
    )?;

    Ok(Json(json!({
        "generated_at": Utc::now(),
        "period": {
            "days": days,
            "from": from,
            "to": to,
        },
        "counts": {
            "documents_received": counts.documents_received,
            "invoices_extracted": counts.invoices_extracted,
            "awaiting_review": counts.awaiting_review,
            "validation_failures": counts.validation_failures,
            "duplicate_candidates": counts.duplicate_candidates,
            "approved_invoices": counts.approved_invoices,
            "exported_invoices": counts.exported_invoices,
            "processing_failures": counts.processing_failures,
            "pages_processed": counts.pages_processed,
            "avg_processing_ms": counts.avg_processing_ms,
            "total_ai_requests": counts.total_ai_requests,
        },
        "document_status": status.iter().map(|row| json!({
            "status": row.status,
            "count": row.count,
        })).collect::<Vec<_>>(),
        "daily": documents.iter().zip(approved.iter()).map(|(documents, approved)| json!({
            "day": documents.day,
            "documents": documents.count,
            "approved": approved.count,
        })).collect::<Vec<_>>(),
        "recent_activity": activity.iter().map(|row| json!({
            "id": row.id,
            "action": row.action,
            "entity_type": row.entity_type,
            "entity_id": row.entity_id,
            "actor_name": row.actor_name,
            "metadata": row.metadata,
            "created_at": row.created_at,
        })).collect::<Vec<_>>(),
        "recent_errors": errors.iter().map(|row| json!({
            "document_id": row.document_id,
            "filename": row.original_filename,
            "error_code": row.error_code,
            "error_message": row.error_message,
            "created_at": row.created_at,
        })).collect::<Vec<_>>(),
        "mock_note": if invoxa_extraction::mock::MOCK_PROVIDER_NAME == "mock-heuristic" {
            Value::String("The deterministic mock extractor is available for local demos. Its output is always labelled and must be reviewed; it is not an AI model.".into())
        } else { Value::Null },
        "extraction_cost_note": dec(None),
    })))
}
