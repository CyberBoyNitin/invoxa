//! Machine-readable API inventory. The specification intentionally describes
//! every route exposed by this build and is served at `/api/v1/openapi.json`.

use serde_json::{json, Map, Value};

pub fn document() -> Value {
    const ROUTES: &[(&str, &str, &str, &str, bool)] = &[
        (
            "post",
            "/auth/signup",
            "Create an account and organization",
            "Authentication",
            false,
        ),
        (
            "post",
            "/auth/login",
            "Create a browser session",
            "Authentication",
            false,
        ),
        (
            "post",
            "/auth/logout",
            "Revoke the current session",
            "Authentication",
            true,
        ),
        (
            "get",
            "/auth/session",
            "Read the current session",
            "Authentication",
            true,
        ),
        (
            "post",
            "/auth/switch-organization",
            "Switch the active organization",
            "Authentication",
            true,
        ),
        (
            "post",
            "/auth/change-password",
            "Change the account password",
            "Authentication",
            true,
        ),
        (
            "post",
            "/auth/password-reset/request",
            "Request a password reset",
            "Authentication",
            false,
        ),
        (
            "post",
            "/auth/password-reset/confirm",
            "Complete a password reset",
            "Authentication",
            false,
        ),
        (
            "post",
            "/auth/accept-invitation",
            "Accept a team invitation",
            "Authentication",
            true,
        ),
        ("get", "/documents", "List documents", "Documents", true),
        (
            "post",
            "/documents",
            "Upload one or more documents",
            "Documents",
            true,
        ),
        (
            "get",
            "/documents/{document_id}",
            "Read a document and its processing events",
            "Documents",
            true,
        ),
        (
            "delete",
            "/documents/{document_id}",
            "Delete a document",
            "Documents",
            true,
        ),
        (
            "get",
            "/documents/{document_id}/events",
            "List processing events",
            "Documents",
            true,
        ),
        (
            "get",
            "/documents/{document_id}/content",
            "Fetch the stored original file",
            "Documents",
            true,
        ),
        (
            "post",
            "/documents/{document_id}/download-url",
            "Create a short-lived download grant",
            "Documents",
            true,
        ),
        (
            "post",
            "/documents/{document_id}/retry",
            "Retry failed processing",
            "Documents",
            true,
        ),
        (
            "get",
            "/downloads/{token}",
            "Download using a signed grant",
            "Documents",
            false,
        ),
        (
            "get",
            "/jobs/{job_id}",
            "Read a processing job",
            "Documents",
            true,
        ),
        (
            "get",
            "/invoices",
            "List extracted invoices and review queue",
            "Invoices",
            true,
        ),
        (
            "get",
            "/invoices/{invoice_id}",
            "Read invoice detail, lines, evidence, issues and duplicates",
            "Invoices",
            true,
        ),
        (
            "patch",
            "/invoices/{invoice_id}",
            "Edit an invoice with optimistic concurrency",
            "Invoices",
            true,
        ),
        (
            "post",
            "/invoices/{invoice_id}/approve",
            "Approve an invoice",
            "Invoices",
            true,
        ),
        (
            "post",
            "/invoices/{invoice_id}/reject",
            "Reject an invoice",
            "Invoices",
            true,
        ),
        (
            "post",
            "/invoices/{invoice_id}/return",
            "Return an invoice for correction",
            "Invoices",
            true,
        ),
        (
            "post",
            "/invoices/{invoice_id}/duplicates/{duplicate_id}/resolve",
            "Resolve a duplicate candidate",
            "Invoices",
            true,
        ),
        (
            "post",
            "/invoices/{invoice_id}/notes",
            "Add an internal note",
            "Invoices",
            true,
        ),
        ("get", "/vendors", "List vendors", "Vendors", true),
        ("post", "/vendors", "Create a vendor", "Vendors", true),
        (
            "get",
            "/vendors/suggestions",
            "Suggest canonical vendor names",
            "Vendors",
            true,
        ),
        (
            "get",
            "/vendors/{vendor_id}",
            "Read a vendor",
            "Vendors",
            true,
        ),
        (
            "patch",
            "/vendors/{vendor_id}",
            "Update a vendor",
            "Vendors",
            true,
        ),
        ("get", "/exports", "List export jobs", "Exports", true),
        (
            "post",
            "/exports",
            "Queue an approved-invoice export",
            "Exports",
            true,
        ),
        (
            "get",
            "/exports/{export_id}",
            "Read an export job",
            "Exports",
            true,
        ),
        (
            "get",
            "/exports/{export_id}/download",
            "Download a completed export",
            "Exports",
            true,
        ),
        (
            "get",
            "/automation-rules",
            "List automation rules",
            "Automation",
            true,
        ),
        (
            "post",
            "/automation-rules",
            "Create an automation rule",
            "Automation",
            true,
        ),
        (
            "get",
            "/automation-rules/{rule_id}",
            "Read an automation rule",
            "Automation",
            true,
        ),
        (
            "patch",
            "/automation-rules/{rule_id}",
            "Replace an automation rule",
            "Automation",
            true,
        ),
        (
            "delete",
            "/automation-rules/{rule_id}",
            "Delete an automation rule",
            "Automation",
            true,
        ),
        ("get", "/api-keys", "List API keys", "API keys", true),
        (
            "post",
            "/api-keys",
            "Create an API key and reveal its secret once",
            "API keys",
            true,
        ),
        (
            "delete",
            "/api-keys/{key_id}",
            "Revoke an API key",
            "API keys",
            true,
        ),
        (
            "get",
            "/webhooks",
            "List webhook endpoints",
            "Webhooks",
            true,
        ),
        (
            "post",
            "/webhooks",
            "Create a webhook endpoint",
            "Webhooks",
            true,
        ),
        (
            "patch",
            "/webhooks/{endpoint_id}",
            "Update a webhook endpoint",
            "Webhooks",
            true,
        ),
        (
            "delete",
            "/webhooks/{endpoint_id}",
            "Delete a webhook endpoint",
            "Webhooks",
            true,
        ),
        (
            "post",
            "/webhooks/{endpoint_id}/rotate-secret",
            "Rotate the webhook signing secret",
            "Webhooks",
            true,
        ),
        (
            "get",
            "/webhook-deliveries",
            "List webhook deliveries",
            "Webhooks",
            true,
        ),
        (
            "get",
            "/webhook-deliveries/{delivery_id}/attempts",
            "List delivery attempts",
            "Webhooks",
            true,
        ),
        (
            "post",
            "/webhook-deliveries/{delivery_id}/redeliver",
            "Requeue a webhook delivery manually",
            "Webhooks",
            true,
        ),
        (
            "get",
            "/organization",
            "Read organization settings",
            "Organization",
            true,
        ),
        (
            "patch",
            "/organization",
            "Update organization settings",
            "Organization",
            true,
        ),
        (
            "get",
            "/members",
            "List members and pending invitations",
            "Organization",
            true,
        ),
        (
            "patch",
            "/members/{user_id}",
            "Change a member role",
            "Organization",
            true,
        ),
        (
            "delete",
            "/members/{user_id}",
            "Remove a member",
            "Organization",
            true,
        ),
        (
            "post",
            "/invitations",
            "Invite a team member",
            "Organization",
            true,
        ),
        (
            "delete",
            "/invitations/{invitation_id}",
            "Revoke an invitation",
            "Organization",
            true,
        ),
        ("get", "/usage", "Read current usage", "Organization", true),
        (
            "get",
            "/integrations",
            "List integration status",
            "Organization",
            true,
        ),
        (
            "get",
            "/dashboard",
            "Read dashboard metrics",
            "Operations",
            true,
        ),
        (
            "get",
            "/activity",
            "Read the immutable audit log",
            "Operations",
            true,
        ),
        ("get", "/health", "API health", "Operations", false),
        (
            "get",
            "/health/ready",
            "Readiness including database and storage",
            "Operations",
            false,
        ),
        ("get", "/openapi.json", "This document", "Operations", false),
    ];

    let mut paths: Map<String, Value> = Map::new();
    for (method, path, summary, tag, secured) in ROUTES {
        let mut parameters: Vec<Value> = Vec::new();
        let mut rest = *path;
        while let Some(start) = rest.find('{') {
            let tail = &rest[start + 1..];
            let Some(end) = tail.find('}') else { break };
            let name = &tail[..end];
            parameters.push(json!({
                "name": name,
                "in": "path",
                "required": true,
                "schema": { "type": "string", "format": if name == "token" { "string" } else { "uuid" } }
            }));
            rest = &tail[end + 1..];
        }
        let operation_id = format!(
            "{}_{}",
            method,
            path.trim_start_matches('/')
                .replace(['/', '{', '}'], "_")
                .trim_matches('_')
        );
        let mut operation = json!({
            "summary": summary,
            "tags": [tag],
            "operationId": operation_id,
            "parameters": parameters,
            "responses": {
                "200": { "description": "Success" },
                "400": { "$ref": "#/components/responses/BadRequest" },
                "401": { "$ref": "#/components/responses/Unauthorized" },
                "403": { "$ref": "#/components/responses/Forbidden" },
                "422": { "$ref": "#/components/responses/ValidationFailed" },
                "429": { "$ref": "#/components/responses/RateLimited" }
            }
        });
        if *secured {
            operation["security"] = json!([{ "sessionCookie": [] }, { "bearerApiKey": [] }]);
        }
        if *method == "post" || *method == "patch" {
            operation["requestBody"] = json!({
                "required": *path == "/exports" || *path == "/documents",
                "content": {
                    "application/json": { "schema": { "type": "object" } },
                    "multipart/form-data": { "schema": { "type": "object" } }
                }
            });
        }
        let entry = paths
            .entry((*path).to_string())
            .or_insert_with(|| json!({}));
        entry[*method] = operation;
    }

    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Invoxa API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Tenant-scoped invoice extraction and accounts-payable API. Browser sessions require the signed double-submit CSRF token in X-CSRF-Token for unsafe methods. Monetary values are decimal strings. AI output is untrusted and must be reviewed before approval. Use the generated API reference for request and response fields."
        },
        "servers": [{ "url": "/api/v1" }],
        "tags": [
            { "name": "Authentication" },
            { "name": "Documents" },
            { "name": "Invoices" },
            { "name": "Vendors" },
            { "name": "Exports" },
            { "name": "Automation" },
            { "name": "API keys" },
            { "name": "Webhooks" },
            { "name": "Organization" },
            { "name": "Operations" }
        ],
        "paths": paths,
        "components": {
            "securitySchemes": {
                "sessionCookie": { "type": "apiKey", "in": "cookie", "name": "invoxa_session" },
                "bearerApiKey": { "type": "http", "scheme": "bearer", "bearerFormat": "Invoxa API key" }
            },
            "schemas": {
                "Error": {
                    "type": "object",
                    "required": ["error"],
                    "properties": {
                        "error": {
                            "type": "object",
                            "required": ["code", "message"],
                            "properties": {
                                "code": { "type": "string" },
                                "message": { "type": "string" },
                                "details": { "type": ["object", "null"] }
                            }
                        }
                    }
                },
                "Money": {
                    "type": ["string", "null"],
                    "description": "Exact decimal string with at most four decimal places. Never a floating-point number.",
                    "examples": ["1250.00"]
                }
            },
            "responses": {
                "BadRequest": { "description": "Malformed request", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Error" } } } },
                "Unauthorized": { "description": "Missing or invalid credentials", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Error" } } } },
                "Forbidden": { "description": "The credential lacks permission or tenant access", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Error" } } } },
                "ValidationFailed": { "description": "Validation failed", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Error" } } } },
                "RateLimited": { "description": "Rate limit exceeded", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Error" } } } }
            }
        }
    })
}
