use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

pub async fn live(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "invoxa",
        "version": env!("CARGO_PKG_VERSION"),
        "environment": environment_name(&state),
        "provider": state.provider_name(),
        "provider_is_mock": state.provider_is_mock(),
        "storage": state.storage_backend(),
        "uptime_seconds": state.started_at.elapsed().as_secs(),
    }))
}

pub async fn ready(State(state): State<AppState>) -> Response {
    let database = invoxa_database::ping(&state.db).await;
    let storage = state.storage.health().await;
    let ready = database && storage;
    let body = Json(json!({
        "status": if ready { "ready" } else { "not_ready" },
        "checks": {
            "database": if database { "ok" } else { "failed" },
            "storage": if storage { "ok" } else { "failed" },
        },
    }));
    if ready {
        (StatusCode::OK, body).into_response()
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, body).into_response()
    }
}

pub async fn api_health(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "api": "v1",
        "version": env!("CARGO_PKG_VERSION"),
        "environment": environment_name(&state),
        "provider": state.provider_name(),
        "provider_is_mock": state.provider_is_mock(),
        "storage": state.storage_backend(),
    }))
}

pub async fn openapi() -> Json<Value> {
    Json(crate::openapi::document())
}

fn environment_name(state: &AppState) -> &'static str {
    match state.config.environment {
        crate::config::Environment::Development => "development",
        crate::config::Environment::Test => "test",
        crate::config::Environment::Production => "production",
    }
}
