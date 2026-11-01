# Deployment

## What this deployment is

The repository ships a working local/staging Docker Compose stack. A production deployment still requires TLS termination, secret management, backups, monitoring, and a security review. Building successfully is not evidence of production readiness.

## Docker Compose (local/staging)

```bash
cp .env.docker.example .env.docker
```

Replace:

- `POSTGRES_PASSWORD`
- `ENCRYPTION_KEY` (base64 32 bytes)
- `CSRF_SECRET` (base64 32 bytes, different from encryption key)

Then:

```bash
docker compose --env-file .env.docker up --build
```

Services:

- `db`: PostgreSQL 17, persistent volume `invoxa_pgdata`.
- `api`: Axum API, applies migrations on startup, private storage volume `invoxa_storage`.
- `worker`: same image, runs document/export/webhook workers.
- `web`: Next.js production build, proxies `/api/v1` to `http://api:8080`.

The base compose file defaults to `APP_ENV=development` so it works over plain HTTP on localhost. Do not expose this directly to the public internet. For a production-shaped stack, use the production override after setting an HTTPS `APP_URL` and real provider credentials:

```bash
docker compose --env-file .env.docker \
  -f docker-compose.yml -f docker-compose.prod.yml up -d --build
```

The override sets `APP_ENV=production`, secure cookies, trusted proxy headers, disables exposed dev tokens, and disables mock extraction unless `ALLOW_MOCK_AI=true` is explicitly set for a demonstration.

## Production configuration

Required:

```dotenv
APP_ENV=production
APP_URL=https://invoices.example.com
COOKIE_SECURE=true
TRUST_PROXY=true
ENCRYPTION_KEY=<base64 32 bytes>
CSRF_SECRET=<different base64 32 bytes>
DATABASE_URL=postgres://invoxa:<strong-password>@db:5432/invoxa
STORAGE_ROOT=/data/storage
TESSERACT_PATH=/usr/bin/tesseract
EXPOSE_DEV_TOKENS=false
```

The API refuses to start in production if:

- `APP_URL` is not HTTPS;
- `COOKIE_SECURE` is false;
- `EXPOSE_DEV_TOKENS` is true;
- `AI_PROVIDER=mock` without explicit `ALLOW_MOCK_AI=true` (do not enable mock for real accounting).

For real extraction:

```dotenv
AI_PROVIDER=openai
AI_BASE_URL=https://api.openai.com/v1
AI_MODEL=<model>
AI_API_KEY=<secret>
AI_STRUCTURED_OUTPUT=json_schema
```

An organization owner must also enable external-AI consent in **Settings → Organization**. Without consent, the external provider is not called.

## TLS / reverse proxy

Terminate TLS at nginx, Caddy, Traefik, or a cloud load balancer. Forward:

- `Host`
- `X-Forwarded-For`
- `X-Forwarded-Proto`
- request bodies up to the upload limit (default ~20 MiB per file; the API enforces its own limit)

Set `TRUST_PROXY=true` only when the API is behind a trusted proxy that overwrites forwarded headers.

Example nginx location:

```nginx
location / {
    proxy_pass http://127.0.0.1:3000;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
    client_max_body_size 25m;
}
```

The Next.js server proxies API requests to the Rust API over the private network. Do not expose the API port publicly unless you add an explicit CORS/CSRF review.

## Secrets

- Use a secret manager (AWS Secrets Manager, GCP Secret Manager, Vault, etc.), not a committed `.env`.
- Rotate `ENCRYPTION_KEY` only with a re-encryption migration; rotating it without re-encrypting webhook secrets makes them unreadable.
- Rotate `CSRF_SECRET` and `DOWNLOAD_SIGNING_KEY` on a schedule; this invalidates outstanding CSRF/download grants.
- Store the database password and AI API key outside the image.

## Database

- Run PostgreSQL with automated backups and point-in-time recovery.
- Migrations are embedded and applied on API/worker startup. In a multi-replica deployment, run one migration job first and then start replicas to avoid concurrent migration races.
- Use `DATABASE_MAX_CONNECTIONS` per instance and size the PostgreSQL connection limit accordingly.
- Back up both PostgreSQL and the object-storage volume/prefix. An export artifact or original document is not recoverable from the database alone.
- Test restore procedures, not just backup creation.

## Storage

The only implemented adapter is private local disk (`STORAGE_ROOT`). For production:

- Put it on encrypted, replicated storage.
- Ensure the API and worker share the same volume (they do in Compose).
- Add lifecycle/retention policy for originals and exports.
- Implement and test the S3 adapter before claiming S3 support. It is not implemented in this build.

## Scaling

- API: stateless apart from shared database/storage; can scale horizontally.
- Worker: safe to run multiple replicas; jobs are claimed with `FOR UPDATE SKIP LOCKED` and leases.
- Exports: the export worker claims export jobs; avoid running an unbounded number of export workers.
- Webhooks: multiple webhook workers are safe; delivery rows are claimed and leases expire.
- OCR/AI calls happen outside database transactions; size worker concurrency for CPU/OCR and provider rate limits.
- Consider a separate worker pool for OCR vs. AI vs. exports at higher scale.

## Observability

- Structured tracing logs are enabled with `RUST_LOG`.
- `/health` is liveness; `/health/ready` checks database and storage.
- The usage page exposes organization-scoped counts (documents, pages, AI requests, failures).
- Add external metrics/alerts for queue depth, dead-letter count, API latency, error rate, storage growth, and database saturation.
- Do not log document text, API keys, webhook secrets, or AI provider keys.

## Webhooks

- Egress must allow HTTPS to customer endpoints.
- The SSRF policy rejects private/loopback/link-local/reserved addresses on every attempt and does not follow redirects.
- Receivers must verify `X-Invoxa-Signature` and reject stale timestamps.
- Dead-letter deliveries are visible in the UI and can be manually requeued.
- If you use an egress proxy, ensure DNS pinning and address validation still apply.

## Backup / restore checklist

1. PostgreSQL base backup + WAL archiving.
2. Object-storage snapshot/versioning.
3. Encrypted off-site copies.
4. Restore into an isolated environment and run `scripts/smoke.mjs` against it.
5. Verify export artifacts and original documents are readable after restore.
6. Verify webhook signing secrets decrypt after restore (same `ENCRYPTION_KEY`).

## Production checklist

- [ ] HTTPS and secure cookies enforced.
- [ ] Unique secrets in a secret manager.
- [ ] External AI consent configured per organization.
- [ ] Mock provider disabled for real data.
- [ ] Malware scanning added at ingest.
- [ ] Backups and restore tested.
- [ ] Rate limits/WAF configured.
- [ ] Logs redacted and retained per policy.
- [ ] Worker and queue alerts configured.
- [ ] Penetration/security review completed.
- [ ] Load test at expected document volume.
- [ ] Data-retention/deletion policy approved.
