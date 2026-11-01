# Security Model

## Scope and posture

Invoxa is a locally working MVP. The controls below are implemented and exercised by tests, but the system has **not** undergone a third-party penetration test, production-scale abuse testing, or a formal compliance audit.

## Assets

- Original invoice documents and extracted financial data.
- Organization membership, roles, and invitations.
- Session cookies, CSRF tokens, password hashes.
- API key secrets and webhook signing secrets.
- Export artifacts.
- Audit history and usage records.

## Tenant isolation

- Auth resolves a user/API key to one active organization.
- Every tenant-scoped SQL query filters by `organization_id`.
- Route ids are UUIDs but are not trusted as authorization.
- Cross-tenant resources return `404` to avoid existence disclosure.
- Cross-tenant tests cover list, detail, content, download grants, delete, patch, approve, reject, notes, export detail, and export artifact download.

## Authentication

- Passwords are hashed with Argon2.
- Browser sessions are random, server-side records with an HttpOnly, SameSite cookie.
- Session cookies are secure when `COOKIE_SECURE=true`; production startup requires HTTPS and secure cookies.
- Unsafe cookie-authenticated requests require a signed double-submit CSRF token in `X-CSRF-Token`.
- Login and API-key authentication have configurable per-minute rate limits.
- Password reset and invitation tokens are hashed/expiring; email delivery is not configured in this MVP.
- Invitation acceptance requires a signed-in account whose email matches the invitation.

## API keys

- Format: `ivx_` plus high-entropy random bytes.
- Stored as SHA-256 hashes; the plaintext secret is returned exactly once.
- Explicit scopes are required per endpoint.
- Revocation is immediate and checked on each request.
- Only a signed-in user may mint keys; an API key cannot mint new keys (prevents scope self-escalation).
- Keys can have an expiry and last-used tracking.

## Webhook security

- HTTPS on port 443 by default; HTTP requires explicit `WEBHOOK_ALLOW_HTTP` and allowed ports.
- DNS is resolved for creation and every delivery. All resolved addresses must be public; private, loopback, link-local, multicast, and reserved ranges are rejected.
- Host is pinned after validation to reduce DNS rebinding.
- Redirects are not followed.
- Payloads are signed with HMAC-SHA256 over `timestamp.body`; signature header is `t=<unix>,v1=<hex>`.
- Receivers must reject stale timestamps and verify signatures.
- Signing secrets are encrypted at rest with AES-GCM using `ENCRYPTION_KEY`.
- Delivery events are durable and idempotent by `(endpoint_id, event_id)`.
- Retries are bounded; terminal failures become `dead_letter`.
- Manual redelivery preserves the event id and is audited.
- Response snippets and errors are bounded and stored for diagnostics; secrets are never stored in delivery rows.

## AI and extracted data

- All extraction output is untrusted and schema-validated.
- Missing values remain missing; no default zero/placeholder is invented.
- Mock extraction is always `is_mock=true` with a visible warning and requires explicit approval acknowledgement.
- External AI is gated by owner-controlled organization consent.
- Document contents are not logged in ordinary logs.
- Prompt/response bodies are not persisted by default; usage/latency metadata is tracked.

## Upload and storage

- Extension, declared MIME type, magic bytes, file size, page count, and image pixel limits are checked.
- Files are stored under opaque keys with private access.
- Downloads require an authenticated session/API key or a short-lived signed grant.
- `Content-Disposition: attachment` and `X-Content-Type-Options: nosniff` are set.
- No antivirus/malware scanner is integrated. Production deployments should add one at ingest.
- Local disk storage is not replicated. S3/object-store adapter is unimplemented.

## Data integrity

- Money is exact decimal (`NUMERIC(18,4)`), never floating point.
- Approval is server-authoritative and version-checked.
- Unresolved blocking errors, open/confirmed duplicates, and unacknowledged mock data block approval.
- Exports query only post-approval invoices (`approved` or explicitly selected `exported`) and re-check selected invoice ownership/status before queueing. Unreviewed statuses are rejected.
- State transitions are transactional; long OCR/AI work happens outside database transactions.
- Audit events are append-only.

## Secrets and configuration

- `ENCRYPTION_KEY`, `CSRF_SECRET`, and optional `DOWNLOAD_SIGNING_KEY` must decode to exactly 32 bytes.
- Production startup rejects insecure configuration (non-HTTPS app URL, insecure cookies, exposed dev tokens).
- `.env` is ignored by Git; `.env.example` contains placeholders only.
- Logs redact API keys, passwords, provider keys, and document text.

## Tested security properties

Verified by `scripts/security-smoke.mjs` against a real PostgreSQL database:

- CSRF rejection for cookie mutations without a token.
- Two-tenant list isolation.
- Cross-tenant detail/content/download-grant/delete/patch/approve/reject/notes all return `404`.
- Anonymous requests return `401`.
- Scoped API-key bearer authentication works; missing scope returns `403`; invalid keys return `401`.
- API-key revocation takes effect immediately.
- Selected export contains exactly the authorized invoice; cross-tenant export detail/download returns `404`.

Verified by `scripts/webhook-smoke.mjs`:

- Real outbox event creation after approval.
- Delivery attempt to a public HTTPS test endpoint.
- Terminal delivery state.
- Manual redelivery requeues with the same event id.
- Endpoint deletion.

## Unresolved risks / production gaps

- No third-party security audit or penetration test.
- No malware scanning.
- No rate limiting at a reverse proxy/WAF layer; app-level limits only.
- No field-level encryption for invoice contents at rest beyond database/disk controls.
- No S3/KMS integration; local disk and environment-held keys only.
- No formal data-retention scheduler. Deletion is explicit; audit retention policy is deployment-specific.
- No email delivery; invitation/reset links are manual in development.
- No production load test or disaster-recovery exercise.
- PDF parser and OCR are exposed to untrusted files; run in a sandboxed/limited worker in production.
