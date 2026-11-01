# Architecture

## Overview

Invoxa is split into a Next.js browser application, an Axum HTTP API, and a background worker. The API and worker share the same PostgreSQL database and private local object storage. Durable state transitions live in PostgreSQL; no in-memory queue is required.

```
Browser (Next.js App Router)
        │  same-origin /api/v1 rewrite
        ▼
Rust API (Axum)
  ├─ session/CSRF/API-key auth
  ├─ tenant-scoped queries
  ├─ upload + document endpoints
  ├─ invoice review/approval/export endpoints
  ├─ API keys, webhooks, settings, audit
  └─ PostgreSQL
        │
        ├─ document_processing_jobs
        ├─ export_jobs
        └─ webhook_deliveries (outbox)
                │
                ▼
        Rust worker
          ├─ document workers (extract → validate → review state)
          ├─ export worker (CSV/JSON → private storage)
          └─ webhook worker (HMAC delivery with retries)
```

## Backend crates

| Crate | Responsibility |
|---|---|
| `invoxa-domain` | Domain types, statuses, roles/permissions, validation rules, duplicate heuristics, normalization. |
| `invoxa-database` | SQLx queries and transactions for users, organizations, documents, invoices, jobs, exports, webhooks, audit, usage. |
| `invoxa-documents` | Upload validation, local `ObjectStorage`, PDF text extraction, Tesseract OCR, download grants. |
| `invoxa-extraction` | Provider abstraction, deterministic mock provider, OpenAI-compatible structured-output provider, circuit breaker, schema validation. |
| `invoxa-workflows` | Durable processing state machine, export builder workflow, webhook event/outbox construction. |
| `invoxa-integrations` | CSV/JSON generation and signed webhook HTTP delivery with SSRF controls. |
| `invoxa-security` | Password hashing, sessions, CSRF, API-key generation/hash, AES-GCM secret encryption, HMAC signing. |
| `apps/api` | HTTP surface, auth extractors, presenters, worker binaries, seed binary, config. |

## Multi-tenancy

- The authenticated request context resolves a user session or API key to exactly one active organization.
- Every resource query includes `organization_id = $1`; route IDs are treated as opaque UUIDs, not as authorization claims.
- A resource that exists in another tenant is reported as `404`, not `403`, to avoid confirming its existence.
- Cross-tenant security smoke test creates two organizations and verifies reads, edits, approvals, deletes, content downloads, export details, and export artifacts all return `404`.
- The browser UI never supplies organization ids for tenant-scoped operations.

## Data model

Main tables:

- `users`, `organizations`, `organization_members`, `sessions`, `invitations`, `password_reset_tokens`.
- `documents`, `document_processing_jobs`, `document_events`.
- `vendors`.
- `extracted_invoices`, `invoice_line_items`, `invoice_tax_lines`, `invoice_field_provenance`.
- `validation_issues`, `duplicate_candidates`, `invoice_reviews`, `approval_events`.
- `automation_rules`, `automation_runs`, `integration_connections`.
- `api_keys`, `webhook_endpoints`, `webhook_deliveries`, `webhook_delivery_attempts`.
- `export_jobs`, `audit_events`, `usage_records`, `system_locks`.

Money columns are `NUMERIC(18,4)`. API responses serialize money as exact decimal strings. Missing extracted values remain `null`; they are not replaced with zero or invented defaults. Line totals are pre-tax. The default arithmetic tolerance is `0.02`.

## Document processing state machine

1. Upload validates MIME type, extension, magic bytes, size, page count, and image pixel limits.
2. The original bytes are hashed with SHA-256 and written to private storage under an opaque key.
3. A `document_processing_jobs` row is inserted in the same transaction as the document.
4. The worker claims a job with a lease (`FOR UPDATE SKIP LOCKED`), records attempts, and moves through explicit stages.
5. Digital PDFs use `pdf-extract`; images use Tesseract CLI TSV OCR. Scanned PDF rasterization is not implemented.
6. Extraction produces untrusted structured fields plus provenance. The provider result is validated against the schema.
7. Deterministic validation creates issues (missing critical fields, arithmetic mismatch, ambiguous dates, poor OCR, duplicate candidates).
8. A conservative policy sets `review_required` or `ready_for_approval`. Nothing is auto-approved.
9. Failed transient jobs retry with bounded exponential backoff; permanent failures become terminal `dead` jobs. Manual retry is available for eligible documents.

## Review and approval

- `PATCH /invoices/{id}` applies corrections with optimistic concurrency (`version`). Field edits update provenance and record a review.
- Approval is a separate `POST /invoices/{id}/approve` transition. It re-checks version, unresolved blocking errors, open/confirmed duplicates, mock acknowledgement, and status on the server.
- `reject` and `return` are distinct recorded actions. Returning moves a `ready_for_approval` or `rejected` invoice back to `review_required` with a required reason.
- Approval events are append-only and audited.

## Export workflow

- `POST /exports` accepts `format` (`csv`/`json`), optional `filters`, and an idempotency key.
- Supported filters: `from`, `to`, `vendor_id`, `status` (`approved` or `exported`), and `invoice_ids` (up to 500). Selected IDs are re-checked for tenant ownership and the requested post-approval status before the job is queued.
- The worker re-queries only the requested post-approval status, writes a stable CSV/JSON artifact, records row count/filename, marks newly included approved invoices `exported`, and emits an outbox event. Unreviewed statuses are rejected at both the API and worker layers.
- Artifacts are private and downloaded through an authenticated endpoint.

## Webhooks

- Endpoints require HTTPS on port 443 by default. The URL is DNS-resolved and every resolved address is checked against private, loopback, link-local, multicast, and reserved ranges.
- Redirects are not followed. The validated host is pinned for the actual delivery.
- Each event is inserted into `webhook_deliveries` in the same database as the state change. Unique `(endpoint_id, event_id)` makes duplicate emissions idempotent.
- Delivery is signed with `X-Invoxa-Signature: t=<unix>,v1=<hex hmac>` over the timestamp and raw body.
- Failed deliveries use bounded retries; terminal failures become `dead_letter`.
- Operators can inspect delivery history/attempts and manually requeue a terminal delivery. The event id is preserved.

## Frontend

- App Router routes: dashboard, review queue, invoices, invoice detail/review workspace, vendors, documents, exports, automation, integrations, activity, and settings.
- Same-origin rewrite: `/api/v1/*` → `INVOXA_API_URL` (default `http://127.0.0.1:8080`).
- TanStack Query handles server state and polling for jobs/exports.
- PDF.js renders the original PDF with a worker copied to `public/pdf.worker.min.mjs`.
- Tables are keyboard/reader accessible; charts include text alternatives; forms use explicit labels and validation.
- The visual language is a warm editorial finance system with restrained indigo accents, not a generic dashboard template.

## Failure and recovery model

- Durable jobs survive process restarts.
- Leases expire; abandoned jobs can be reclaimed.
- Export artifacts are written only after the job is claimed; failures are recorded on the job.
- Webhook outbox entries are durable; delivery attempts are recorded.
- Audit events record sensitive state transitions without storing document contents.
- The readiness endpoint checks database and storage. `/health` is liveness.
