# Invoxa — Final Delivery Report

Date: 2026-10-31

## 1. What was built

Invoxa is a working multi-tenant B2B accounts-payable application, not a static mockup. The implemented vertical slice covers:

- Organization signup/login, session cookies, CSRF protection, password hashing, invitations, roles, and organization switching.
- PDF/PNG/JPEG/TIFF upload with file validation, private local storage, SHA-256 hashing, and durable processing jobs.
- PDF text extraction and Tesseract OCR for images.
- Configurable extraction provider abstraction with a deterministic mock provider and an OpenAI-compatible structured-output provider.
- Field/line/tax persistence with provenance, exact decimal money, validation issues, duplicate candidates, and review history.
- Review workspace with corrections, optimistic concurrency, notes, approve/reject/return-for-correction, and explicit mock acknowledgement.
- CSV/JSON exports, selected-invoice export, status filters (`approved`/`exported`), durable export jobs, artifact download, and export history.
- Vendor management, automation rules, activity/audit log, usage metrics, dashboard, integrations status page, and settings.
- API keys with scopes, one-time secret reveal, hashing at rest, expiry, rate limits, and immediate revocation.
- Webhook endpoints, signing-secret rotation, encrypted secrets, signed deliveries, retry history, dead-letter visibility, and manual redelivery.
- Cross-tenant authorization checks and automated security tests.
- Next.js production frontend with dashboard, review queue, invoice detail/PDF viewer, documents, exports, vendors, automations, integrations, activity, and settings.

## 2. Actual stack and versions

| Component | Version |
|---|---|
| Rust | 1.98.1 |
| Cargo | 1.98.1 |
| Axum | 0.8 |
| SQLx | 0.8 |
| tower-http | 0.6 |
| reqwest | 0.12 |
| PostgreSQL | 17.11 |
| Tesseract | 5.4.0.20240606 |
| pdf-extract | 0.12 |
| Node.js | 24.15.0 |
| npm | 11.12.1 |
| Next.js | 16.4.3 |
| React | 19.3.1 |
| TypeScript | 5.9 |
| Tailwind CSS | 4.3.3 |
| TanStack Query | 5.105 |
| pdfjs-dist | 6.4.299 |
| Playwright | 1.62.1 |

The workspace is split into `apps/api`, `apps/web`, and the crates `domain`, `database`, `documents`, `extraction`, `workflows`, `integrations`, and `security`.

## 3. Architecture and important decisions

- **PostgreSQL is the source of truth.** Documents, processing jobs, invoices, approvals, exports, webhook deliveries, and audit events are durable rows. The worker claims jobs with leases and `FOR UPDATE SKIP LOCKED`.
- **Tenant isolation is enforced in SQL.** Every tenant-scoped query filters by the authenticated organization id. Cross-tenant resources return `404`.
- **Approval is a server-side transition.** The UI is advisory. The server re-checks status, version, blocking issues, duplicate state, and mock acknowledgement. No invoice is auto-approved.
- **Money is exact.** Values use `NUMERIC(18,4)` and `rust_decimal`, and API responses serialize decimal strings. Missing AI values stay `null`.
- **AI output is untrusted.** Provider output is schema-validated, provenance is stored, and mock extraction is always labelled `is_mock=true`. External AI requires owner consent.
- **Webhook outbox.** Events are written to `webhook_deliveries` in the same database as the state change. Unique `(endpoint_id, event_id)` makes duplicate emissions idempotent. Delivery is HMAC-signed, SSRF-checked, and bounded by retries.
- **Storage abstraction.** Private local disk is implemented. The S3 adapter is intentionally not claimed.
- **Same-origin frontend proxy.** Next.js rewrites `/api/v1/*` to the Rust API, keeping browser cookies same-origin.

## 4. Implemented versus incomplete

### Implemented and exercised

- Signup, login, logout, session, CSRF, roles, invitations, password change/reset flows.
- Multi-tenant organization scoping and cross-tenant isolation.
- Upload validation, private storage, durable jobs, retry, job detail.
- Digital PDF text extraction and direct image OCR.
- Mock extraction and OpenAI-compatible provider wiring with consent.
- Invoice review, corrections, provenance, validation issues, duplicate resolution, notes.
- Approve, reject, return-for-correction, mock acknowledgement, optimistic concurrency.
- CSV/JSON export, selected invoices, approved/exported status filter, artifact download.
- API key lifecycle and scope enforcement.
- Webhook endpoint lifecycle, secret rotation, signed delivery, history, redelivery.
- Dashboard, vendors, automation rules, integrations status, activity, usage, settings.
- PDF viewer, accessible tables/forms, responsive editorial UI.
- Dockerfiles, local Compose stack plus production override, `.env.example`, setup/architecture/security/API/deployment docs.

### Incomplete / deliberately limited

- Scanned PDF rasterization is not implemented. Digital PDFs and image OCR work; a scanned PDF without a text layer fails extraction rather than inventing values.
- Only local-disk object storage exists; no S3 adapter.
- No accounting/OAuth connectors. The Integrations page states this.
- No email delivery. Invitation/reset links are manual in development.
- Mock provider is the default. Real AI requires an OpenAI-compatible endpoint, key, and owner consent.
- Automation rules are deterministic and do not perform external side effects.
- No frontend ESLint config; `tsc --noEmit` and `next build` are the frontend static gates.
- Docker artifacts could not be built in this environment because Docker is not installed. They are provided but unverified locally.

## 5. Database setup and migration status

- PostgreSQL 17.11 runs locally on port `55432`.
- Databases: `invoxa` and `invoxa_test`.
- Role: `invoxa`; local password: `invoxa_dev_password`.
- Migrations are embedded in the Rust binaries and applied automatically at API/worker startup.
- The API log reports `database migrations are up to date` for `invoxa`.
- `invoxa_test` was migrated and seeded successfully with the test `DATABASE_URL`.
- Demo seed: organization `Northstar Demo Company` (`is_demo=true`), demo user `demo@invoxa.local`.
- The seed refuses production defaults unless `SEED_ALLOW_PRODUCTION=true` is deliberately set.

## 6. Required environment variables

Required in every environment:

```dotenv
DATABASE_URL=postgres://invoxa:...@host:5432/invoxa
ENCRYPTION_KEY=<base64 32 bytes>
CSRF_SECRET=<different base64 32 bytes>
```

Important optional/operational variables:

```dotenv
APP_ENV=development
APP_URL=http://localhost:3000
BIND_ADDR=127.0.0.1:8080
ALLOWED_ORIGINS=http://localhost:3000
STORAGE_ROOT=.local/storage
COOKIE_SECURE=false
TRUST_PROXY=false
TESSERACT_PATH=C:/Program Files/Tesseract-OCR/tesseract.exe
OCR_LANGUAGE=eng
AI_PROVIDER=mock
ALLOW_MOCK_AI=true
EXPOSE_DEV_TOKENS=true
WEBHOOK_ALLOW_HTTP=false
WORKER_POLL_SECONDS=2
WORKER_LEASE_SECONDS=120
WORKER_CONCURRENCY=2
```

External AI only:

```dotenv
AI_PROVIDER=openai
AI_BASE_URL=https://api.openai.com/v1
AI_MODEL=<model>
AI_API_KEY=<secret>
AI_STRUCTURED_OUTPUT=json_schema
```

Generate keys:

```bash
openssl rand -base64 32
# or
node -e "console.log(require('crypto').randomBytes(32).toString('base64'))"
```

## 7. Commands to start

### Local Windows workspace

```powershell
# PostgreSQL
& .\.local\pgsql\bin\pg_ctl.exe -D .\.local\pgdata -l .\.local\pg.log start

# Backend build
cargo build -p invoxa-api --bin invoxa-api --bin invoxa-worker --bin invoxa-seed

# Seed/migrate demo workspace
cargo run -p invoxa-api --bin invoxa-seed

# Terminal A
.\target\debug\invoxa-api.exe

# Terminal B
.\target\debug\invoxa-worker.exe

# Terminal C
cd apps\web
npm ci
npm run build
npm run start
```

Open <http://localhost:3000/login>.

Demo credentials:

- Email: `demo@invoxa.local`
- Password: `InvoxaDemo!2026`
- Organization: `Northstar Demo Company`

Change the password if the workspace is reachable by anyone else.

### Docker Compose

```bash
cp .env.docker.example .env.docker
# Replace POSTGRES_PASSWORD, ENCRYPTION_KEY, CSRF_SECRET.
docker compose --env-file .env.docker up --build
```

The Compose stack is local/staging configuration over HTTP. Production requires TLS, secure cookies, real secrets, backups, and the checklist in `docs/DEPLOYMENT.md`.

## 8. Commands to run all tests

```powershell
# Backend
cargo fmt --all -- --check
cargo check -p invoxa-api --all-targets
cargo clippy --workspace --all-targets
cargo test --workspace

# Frontend
cd apps\web
npm run typecheck
npm run build
node scripts\ui-e2e.mjs
node scripts\ui-audit.mjs

# API / security / webhook (from repository root, API + worker running)
node scripts\smoke.mjs
node scripts\security-smoke.mjs
node scripts\webhook-smoke.mjs
```

## 9. Test results

### Unit tests

`cargo test --workspace` completed with exit code 0. All test binaries passed:

- `invoxa-api`: 5 passed
- `invoxa-documents`: 6 passed
- `invoxa-domain`: 33 passed
- `invoxa-extraction`: 12 passed
- `invoxa-integrations`: 13 passed
- `invoxa-security`: 8 passed
- `invoxa-workflows`: 11 passed
- Remaining binaries: 0 tests, passed

Total: **88 passed, 0 failed**.

### Rust static checks

- `cargo fmt --all -- --check`: exit 0.
- `cargo check -p invoxa-api --all-targets`: exit 0.
- `cargo clippy --workspace --all-targets`: exit 0 with non-blocking style warnings (mostly `too_many_arguments` and minor idiom suggestions). No errors.

### Frontend static/build

- `npm run typecheck`: exit 0.
- `npm run build`: exit 0. All routes compiled, including `/dashboard`, `/review`, `/invoices/[id]`, `/documents/[id]`, `/exports`, `/settings/*`, and auth pages.

### Critical-path API smoke

`node scripts/smoke.mjs` output:

```text
login: ok
session: demo@invoxa.local Northstar Demo Company
upload: 9a4849d7-fe2b-4e93-8e7e-338d3e5b0b67 acme_invoice.pdf
job lookup: 6f52aed6-03b9-4c3d-8ed6-9c591663e558 succeeded completed
invoice: INV-2026-0042 Acme Cloud Services Pvt Ltd 23600.0000 review_required
mock: true issues: 9 duplicates: 7
return for correction: rejected -> review_required
approved: approved version: 4
export: 6e40b848-0c0e-47a9-93c1-71985bf8193f json 1 rows
re-export: 24fec530-d694-46a2-b8bd-ff7e00499b2f status=exported 1 rows
dashboard: {"approved_invoices":6,"avg_processing_ms":176.125,"awaiting_review":2,"documents_received":8,"duplicate_candidates":1,"exported_invoices":6,"invoices_extracted":8,"pages_processed":8,"processing_failures":2,"total_ai_requests":10,"validation_failures":2}
SMOKE OK
```

### Cross-tenant security smoke

`node scripts/security-smoke.mjs` output:

```text
base: http://127.0.0.1:8080
created organizations for tenanta-...@example.test and tenantb-...@example.test
csrf: rejected cookie mutation without token
tenant A invoice: 1c66e99e-25ae-4726-8544-8045bcb6e326
list isolation: ok
resource isolation: all cross-tenant reads and mutations returned 404
anonymous access: rejected with 401
api key: bearer auth, scope enforcement, and invalid-key rejection work
selected export: exactly one authorized invoice
export isolation: cross-tenant detail and artifact download returned 404
api key revocation: immediate
SECURITY SMOKE OK
```

### Webhook smoke

`node scripts/webhook-smoke.mjs` output:

```text
organization: webhook-...@example.test
webhook endpoint: 8f9ee4dd-2be0-442d-b5d3-f52bb81ca204 https://example.com/invoxa-webhook-...
approved invoice: f1afd9a9-84af-49fc-8fbd-889b96b71935
delivery: 3755b9ca-9ea2-44bf-a55b-ca1962588a61 dead_letter 405 1 attempts
manual redelivery: dead_letter -> pending (same event id preserved)
webhook endpoint deleted
WEBHOOK SMOKE OK
```

The `405` is the test receiver's response to the synthetic `example.com` URL; the important checks are durable event creation, attempt recording, terminal state, and manual requeue preserving the event id.

### Browser end-to-end test

`node scripts/ui-e2e.mjs` output:

```text
signup: ok ui-...@example.test
upload: accepted through the UI
extraction: invoice bb28a812-319a-4fd4-afd4-a3a3edb799c1 review_required
review page: INV-2026-0042
approval: explicit server transition visible
export: selected invoice job succeeded cfb8434a-1636-41ea-9345-330394d39c9d rows 1
download: artifact saved and contains the selected invoice
re-export: previously exported invoice job succeeded 83168e04-51ff-46e6-9270-3b838685c42a rows 1
document preview: canvas rendered 1
api key UI: create, reveal once, revoke
team UI: invite and revoke
webhook UI: create, rotate secret, delete
pageErrors: 0
consoleErrors: 0
badResponses: 0
UI E2E OK
```

### Route/console/network audit

`node scripts/ui-audit.mjs` visited 12 main routes with:

```text
consoleErrors: 0
pageErrors: 0
badResponses: 0
```

## 10. Known limitations and unresolved risks

1. **Scanned PDF rasterization is missing.** A PDF with no text layer cannot be OCR'd. Image OCR and digital PDF text extraction work. PDFium rasterization is the next adapter.
2. **Local disk only.** No S3/KMS adapter; production must use replicated/encrypted storage or implement one.
3. **No email delivery.** Invitations and reset links are returned manually when `EXPOSE_DEV_TOKENS=true`.
4. **Mock extraction is the default.** It is deterministic, visibly labelled, and requires acknowledgement. It must not be used for real accounting decisions.
5. **No OAuth/accounting integrations.** Exports, API keys, and webhooks are the integration surface.
6. **No malware scanner.** Upload validation checks type/size/pages/pixels, but does not antivirus-scan files.
7. **No third-party penetration test, load test, or disaster-recovery exercise.**
8. **No frontend ESLint config.** TypeScript and production build are the static gates.
9. **Docker artifacts are unverified** because Docker is not installed in this environment. The Compose file and Dockerfiles are provided for a Docker host.
10. **Clippy emits non-blocking warnings.** It exits successfully; warnings are mostly API-signature style.
11. **No git repository was initialized**; the workspace is uncommitted source.
12. **Automation rules do not perform external side effects.** They evaluate deterministic conditions and record runs.
13. **Production deployment requires TLS, secure cookies, secret management, backups, WAF/rate limiting, and monitoring.** Local success is not production readiness.

## 11. External credentials/services still required

- `ENCRYPTION_KEY` and `CSRF_SECRET`: generate independent 32-byte base64 values.
- `DOWNLOAD_SIGNING_KEY`: optional; otherwise derived from `ENCRYPTION_KEY`.
- A PostgreSQL instance for deployment (local one is included for development).
- An OpenAI-compatible AI endpoint, model name, and API key **only if** real AI extraction is enabled; organization owner consent is also required.
- SMTP or an email API for real invitation/password-reset delivery (not implemented).
- S3/object-storage credentials **only after** an S3 adapter is implemented (not present).
- OAuth credentials for accounting systems **only after** those connectors are implemented (not present).
- TLS certificate/DNS and reverse-proxy configuration for production.
- Customer-side webhook receiver URL and signing-secret verification.

## 12. Step-by-step demonstration

1. Start PostgreSQL, API, worker, and frontend using the commands in section 7.
2. Open <http://localhost:3000/login> and sign in with `demo@invoxa.local` / `InvoxaDemo!2026`.
3. Go to **Documents** and upload `fixtures/acme_invoice.pdf` (or use the UI upload action).
4. Watch the document move through extraction/validation. Open the document to view the PDF canvas and processing events.
5. Open the invoice in **Review queue**. Inspect fields, line items, provenance, validation issues, and duplicate candidates.
6. Resolve open duplicate candidates. Confirm the mock warning is visible.
7. Click **Approve invoice**, acknowledge mock extraction, and confirm the server transition to `approved`.
8. Go to **Exports**, click **New export**, leave **Status** as `Approved, not yet exported`, select the invoice, and queue a JSON or CSV export.
9. Wait for the export job to succeed, download the artifact, and verify it contains the invoice id.
10. Open a second export, change **Status** to `Previously exported`, select the same invoice, and confirm re-export succeeds without bypassing approval.
11. Go to **Settings → API keys**, create a key, copy the one-time secret, then revoke it and confirm immediate invalidation.
12. Go to **Settings → Team**, invite a member, then revoke the invitation.
13. Go to **Settings → API keys** and use the **Webhooks** section to create an endpoint, rotate its secret, and delete it. For delivery/redelivery verification, run `node scripts/webhook-smoke.mjs`.
14. Check **Dashboard**, **Activity**, **Usage**, and **Integrations** for the recorded state.

The executable versions of these flows are `scripts/smoke.mjs`, `scripts/security-smoke.mjs`, `scripts/webhook-smoke.mjs`, and `apps/web/scripts/ui-e2e.mjs`.
