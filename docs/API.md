# API Reference

Base path: `/api/v1`. The machine-readable route inventory is served at `GET /api/v1/openapi.json`.

## Authentication

### Browser sessions

1. `POST /auth/login` with `{ "email", "password" }`.
2. The response sets an HttpOnly `invoxa_session` cookie and a readable CSRF cookie.
3. Send the CSRF value as `X-CSRF-Token` on every `POST`, `PATCH`, and `DELETE`.
4. `POST /auth/logout` revokes the session.

### API keys

```http
Authorization: Bearer ivx_...
```

Keys are created in **Settings → API keys**, are shown once, and carry explicit scopes such as `invoices:read`, `invoices:write`, `invoices:approve`, `exports:write`, `documents:read`, `documents:write`, `webhooks:read`, `webhooks:write`, `organization:read`, `organization:write`, `api_keys:manage`.

API keys cannot mint new API keys.

## Conventions

- JSON request/response bodies unless noted.
- Money is an exact decimal **string** (for example `"23600.0000"`), never a JSON float.
- Missing extracted values are `null`.
- IDs are UUID strings.
- Timestamps are UTC ISO-8601.
- Errors use:

```json
{
  "error": {
    "code": "validation_failed",
    "message": "Human-readable message.",
    "details": null
  }
}
```

- Tenant-scoped resources from another organization return `404`.
- Unsafe browser-session requests without a valid CSRF token return `403`.
- Rate-limited requests return `429`.

## Endpoints

### Authentication

| Method | Path | Purpose |
|---|---|---|
| POST | `/auth/signup` | Create a user + organization |
| POST | `/auth/login` | Create a session |
| POST | `/auth/logout` | Revoke the session |
| GET | `/auth/session` | Current user, organization, role |
| POST | `/auth/switch-organization` | Switch active organization |
| POST | `/auth/change-password` | Change password |
| POST | `/auth/password-reset/request` | Request a reset token |
| POST | `/auth/password-reset/confirm` | Complete reset |
| POST | `/auth/accept-invitation` | Accept an invitation |

### Documents

| Method | Path | Purpose |
|---|---|---|
| GET | `/documents` | List documents with filters |
| POST | `/documents` | Multipart upload (`files`) |
| GET | `/documents/{document_id}` | Document, jobs, events, invoice summary |
| DELETE | `/documents/{document_id}` | Soft-delete document |
| GET | `/documents/{document_id}/events` | Processing event history |
| GET | `/documents/{document_id}/content` | Authenticated original file |
| POST | `/documents/{document_id}/download-url` | Short-lived signed download grant |
| POST | `/documents/{document_id}/retry` | Retry eligible failed processing |
| GET | `/downloads/{token}` | Signed grant download |
| GET | `/jobs/{job_id}` | Processing job status |

Upload constraints (defaults): 20 MiB per file, 20 files per batch, 50 pages, 25M image pixels. PDF, PNG, JPEG, and TIFF are accepted. Scanned PDF rasterization is not implemented; image OCR works.

### Invoices

| Method | Path | Purpose |
|---|---|---|
| GET | `/invoices` | List/review queue with blocking counts |
| GET | `/invoices/{invoice_id}` | Full detail, lines, taxes, provenance, issues, duplicates, history |
| PATCH | `/invoices/{invoice_id}` | Correct fields/line items with `version` |
| POST | `/invoices/{invoice_id}/approve` | Explicit approval |
| POST | `/invoices/{invoice_id}/reject` | Reject with reason |
| POST | `/invoices/{invoice_id}/return` | Return to review for correction |
| POST | `/invoices/{invoice_id}/duplicates/{duplicate_id}/resolve` | Dismiss or confirm a candidate |
| POST | `/invoices/{invoice_id}/notes` | Internal note |

Approval requires:

- status `review_required` or `ready_for_approval`;
- no unresolved blocking validation errors;
- no open duplicate candidates;
- no confirmed duplicate;
- `acknowledge_mock: true` for mock-extracted invoices;
- matching optimistic-concurrency `version`.

### Exports

| Method | Path | Purpose |
|---|---|---|
| GET | `/exports` | List export jobs |
| POST | `/exports` | Queue CSV/JSON export |
| GET | `/exports/{export_id}` | Export status |
| GET | `/exports/{export_id}/download` | Authenticated artifact |

Create request:

```json
{
  "format": "csv",
  "filters": {
    "from": "2026-01-01",
    "to": "2026-12-31",
    "vendor_id": "uuid",
    "status": "approved",
    "invoice_ids": ["uuid"]
  },
  "idempotency_key": "client-generated-key"
}
```

Only post-approval invoices are exportable. `status` defaults to `approved`; `exported` explicitly re-exports previously exported records. `invoice_ids` is optional; when present, every id must belong to the organization and match the requested status, otherwise the request is rejected. Exports mark newly included approved invoices as `exported`.

### Vendors

| Method | Path | Purpose |
|---|---|---|
| GET | `/vendors` | List/search vendors |
| POST | `/vendors` | Create vendor |
| GET | `/vendors/suggestions` | Canonical-name suggestions |
| GET | `/vendors/{vendor_id}` | Vendor detail |
| PATCH | `/vendors/{vendor_id}` | Update vendor |

### Automation

| Method | Path | Purpose |
|---|---|---|
| GET | `/automation-rules` | List rules |
| POST | `/automation-rules` | Create rule |
| GET | `/automation-rules/{rule_id}` | Read rule |
| PATCH | `/automation-rules/{rule_id}` | Replace rule |
| DELETE | `/automation-rules/{rule_id}` | Delete rule |

Automation rules currently evaluate deterministic conditions and record runs; they do not perform external side effects.

### API keys

| Method | Path | Purpose |
|---|---|---|
| GET | `/api-keys` | List keys (no secrets) |
| POST | `/api-keys` | Create and reveal secret once |
| DELETE | `/api-keys/{key_id}` | Revoke |

### Webhooks

| Method | Path | Purpose |
|---|---|---|
| GET | `/webhooks` | List endpoints |
| POST | `/webhooks` | Create endpoint + reveal secret |
| PATCH | `/webhooks/{endpoint_id}` | Update endpoint |
| DELETE | `/webhooks/{endpoint_id}` | Delete endpoint |
| POST | `/webhooks/{endpoint_id}/rotate-secret` | Rotate signing secret |
| GET | `/webhook-deliveries` | Delivery history |
| GET | `/webhook-deliveries/{delivery_id}/attempts` | Attempt history |
| POST | `/webhook-deliveries/{delivery_id}/redeliver` | Requeue a terminal delivery |

Webhook events include `invoice.processed`, `invoice.updated`, `invoice.approved`, `invoice.rejected`, `invoice.exported`, `invoice.export_failed`, `document.uploaded`, `document.processing_completed`, `document.processing_failed`, `export.completed`, and `automation.triggered`. `*` subscribes to all events.

Signature header:

```http
X-Invoxa-Signature: t=1760000000,v1=hex-encoded-hmac-sha256
```

Verification input is the timestamp, a literal `.`, and the exact raw request body. Reject timestamps outside the receiver's tolerance and compare signatures in constant time.

### Organization and operations

| Method | Path | Purpose |
|---|---|---|
| GET/PATCH | `/organization` | Organization settings and AI consent |
| GET | `/members` | Members and invitations |
| PATCH/DELETE | `/members/{user_id}` | Change role / remove member |
| POST | `/invitations` | Invite member |
| DELETE | `/invitations/{invitation_id}` | Revoke invitation |
| GET | `/usage` | Usage records |
| GET | `/integrations` | Honest integration status |
| GET | `/dashboard` | Operational metrics |
| GET | `/activity` | Audit log |
| GET | `/health` | Liveness |
| GET | `/health/ready` | Database/storage readiness |
| GET | `/openapi.json` | This inventory |

## Example: complete API flow

```bash
# Login (store cookies and CSRF)
curl -c cookies.txt -b cookies.txt -X POST http://127.0.0.1:8080/api/v1/auth/login \
  -H 'content-type: application/json' \
  -d '{"email":"demo@invoxa.local","password":"InvoxaDemo!2026"}'

# Upload
curl -c cookies.txt -b cookies.txt -X POST http://127.0.0.1:8080/api/v1/documents \
  -H "x-csrf-token: $CSRF" -F 'files=@fixtures/acme_invoice.pdf'

# Poll the document, resolve duplicates, then approve
curl -c cookies.txt -b cookies.txt -X POST \
  http://127.0.0.1:8080/api/v1/invoices/$INVOICE_ID/approve \
  -H "x-csrf-token: $CSRF" -H 'content-type: application/json' \
  -d '{"version":2,"acknowledge_mock":true}'

# Export selected approved invoice
curl -c cookies.txt -b cookies.txt -X POST http://127.0.0.1:8080/api/v1/exports \
  -H "x-csrf-token: $CSRF" -H 'content-type: application/json' \
  -d '{"format":"json","filters":{"invoice_ids":["'$INVOICE_ID'"]}}'
```

The checked-in `scripts/smoke.mjs` performs this flow and is the authoritative executable example.
