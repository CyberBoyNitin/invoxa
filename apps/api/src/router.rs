//! HTTP surface. Every tenant-scoped handler obtains its organization from the
//! authenticated request context; route parameters are resource ids only.

use crate::error::ApiError;
use crate::routes::{
    activity, api_keys, auth as auth_routes, automation, dashboard, documents, exports, health,
    invoices, organization, vendors, webhooks,
};
use crate::state::AppState;
use axum::extract::DefaultBodyLimit;
use axum::http::header::{self, HeaderName, HeaderValue};
use axum::http::{Method, StatusCode};
use axum::routing::{delete, get, patch, post};
use axum::Router;
use std::time::Duration;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

const JSON_BODY_LIMIT: usize = 4 * 1024 * 1024;

pub fn build_router(state: AppState) -> Router {
    let api = Router::new()
        .route("/auth/signup", post(auth_routes::signup))
        .route("/auth/login", post(auth_routes::login))
        .route("/auth/logout", post(auth_routes::logout))
        .route("/auth/session", get(auth_routes::current_session))
        .route(
            "/auth/switch-organization",
            post(auth_routes::switch_organization),
        )
        .route("/auth/change-password", post(auth_routes::change_password))
        .route(
            "/auth/password-reset/request",
            post(auth_routes::request_password_reset),
        )
        .route(
            "/auth/password-reset/confirm",
            post(auth_routes::confirm_password_reset),
        )
        .route(
            "/auth/accept-invitation",
            post(auth_routes::accept_invitation),
        )
        .route(
            "/documents",
            get(documents::list)
                .post(documents::upload)
                .layer(DefaultBodyLimit::max(state.config.upload_body_limit)),
        )
        .route(
            "/documents/{document_id}",
            get(documents::detail).delete(documents::delete),
        )
        .route("/documents/{document_id}/events", get(documents::events))
        .route("/documents/{document_id}/content", get(documents::content))
        .route(
            "/documents/{document_id}/download-url",
            post(documents::create_download_url),
        )
        .route("/documents/{document_id}/retry", post(documents::retry))
        .route("/downloads/{token}", get(documents::download_with_grant))
        .route("/jobs/{job_id}", get(documents::job_detail))
        .route("/invoices", get(invoices::list))
        .route(
            "/invoices/{invoice_id}",
            get(invoices::detail).patch(invoices::update),
        )
        .route("/invoices/{invoice_id}/approve", post(invoices::approve))
        .route("/invoices/{invoice_id}/reject", post(invoices::reject))
        .route(
            "/invoices/{invoice_id}/return",
            post(invoices::return_for_correction),
        )
        .route(
            "/invoices/{invoice_id}/duplicates/{duplicate_id}/resolve",
            post(invoices::resolve_duplicate),
        )
        .route("/invoices/{invoice_id}/notes", post(invoices::add_note))
        .route("/vendors", get(vendors::list).post(vendors::create))
        .route("/vendors/suggestions", get(vendors::suggestions))
        .route(
            "/vendors/{vendor_id}",
            get(vendors::detail).patch(vendors::update),
        )
        .route("/exports", get(exports::list).post(exports::create))
        .route("/exports/{export_id}", get(exports::detail))
        .route("/exports/{export_id}/download", get(exports::download))
        .route(
            "/automation-rules",
            get(automation::list).post(automation::create),
        )
        .route(
            "/automation-rules/{rule_id}",
            get(automation::detail)
                .patch(automation::update)
                .delete(automation::delete),
        )
        .route("/api-keys", get(api_keys::list).post(api_keys::create))
        .route("/api-keys/{key_id}", delete(api_keys::revoke))
        .route("/webhooks", get(webhooks::list).post(webhooks::create))
        .route(
            "/webhooks/{endpoint_id}",
            patch(webhooks::update).delete(webhooks::delete),
        )
        .route(
            "/webhooks/{endpoint_id}/rotate-secret",
            post(webhooks::rotate_secret),
        )
        .route("/webhook-deliveries", get(webhooks::deliveries))
        .route(
            "/webhook-deliveries/{delivery_id}/attempts",
            get(webhooks::attempts),
        )
        .route(
            "/webhook-deliveries/{delivery_id}/redeliver",
            post(webhooks::redeliver),
        )
        .route(
            "/organization",
            get(organization::get_organization).patch(organization::update_organization),
        )
        .route("/members", get(organization::list_members))
        .route(
            "/members/{user_id}",
            patch(organization::update_member_role).delete(organization::remove_member),
        )
        .route("/invitations", post(organization::invite_member))
        .route(
            "/invitations/{invitation_id}",
            delete(organization::revoke_invitation),
        )
        .route("/usage", get(organization::usage))
        .route("/integrations", get(organization::integrations))
        .route("/dashboard", get(dashboard::dashboard))
        .route("/activity", get(activity::list_activity))
        .route("/health", get(health::api_health))
        .route("/openapi.json", get(health::openapi))
        .fallback(api_not_found)
        .method_not_allowed_fallback(api_method_not_allowed)
        .layer(DefaultBodyLimit::max(JSON_BODY_LIMIT))
        .with_state(state.clone());

    let mut router = Router::new()
        .nest("/api/v1", api)
        .route("/health/live", get(health::live))
        .route("/health/ready", get(health::ready))
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .with_state(state.clone());

    let origins: Vec<HeaderValue> = state
        .config
        .allowed_origins
        .iter()
        .filter_map(|origin| HeaderValue::from_str(origin).ok())
        .collect();
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_credentials(true)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            header::ACCEPT,
            HeaderName::from_static("x-api-key"),
            HeaderName::from_static("x-csrf-token"),
            HeaderName::from_static("x-organization-id"),
            HeaderName::from_static("x-request-id"),
        ])
        .expose_headers([
            HeaderName::from_static("x-request-id"),
            header::RETRY_AFTER,
            header::CONTENT_DISPOSITION,
        ])
        .max_age(Duration::from_secs(600));

    router = router
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static("camera=(), microphone=(), geolocation=(), payment=()"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("content-security-policy"),
            HeaderValue::from_static(
                "default-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'",
            ),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ));
    if state.config.environment.is_production() {
        router = router.layer(SetResponseHeaderLayer::overriding(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        ));
    }

    router
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
}

async fn api_not_found() -> ApiError {
    ApiError::not_found("The requested API endpoint does not exist.")
}

async fn api_method_not_allowed() -> ApiError {
    ApiError::new(
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
        "This HTTP method is not supported for the endpoint.",
    )
}

async fn not_found() -> ApiError {
    ApiError::not_found("The requested resource does not exist.")
}

async fn method_not_allowed() -> ApiError {
    ApiError::new(
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
        "This HTTP method is not supported for the endpoint.",
    )
}
