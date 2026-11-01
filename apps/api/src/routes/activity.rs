use crate::auth::AuthContext;
use crate::error::ApiResult;
use crate::presenters::audit_json;
use crate::state::AppState;
use crate::util::{limit_offset, ApiQuery};
use axum::extract::State;
use axum::Json;
use invoxa_database::operations;
use invoxa_domain::role::Permission;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
pub struct ActivityQuery {
    pub search: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn list_activity(
    State(state): State<AppState>,
    auth: AuthContext,
    ApiQuery(query): ApiQuery<ActivityQuery>,
) -> ApiResult<Json<Value>> {
    auth.require_scope(Permission::ViewAuditLog, "audit:read")?;
    let (limit, offset) = limit_offset(query.limit, query.offset, 50);
    let search = query
        .search
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(100).collect::<String>());
    let (events, total) = operations::list_audit_events(
        &state.db,
        auth.organization_id,
        search.as_deref(),
        limit,
        offset,
    )
    .await?;
    Ok(Json(json!({
        "events": events.iter().map(audit_json).collect::<Vec<_>>(),
        "total": total,
        "limit": limit,
        "offset": offset,
    })))
}
