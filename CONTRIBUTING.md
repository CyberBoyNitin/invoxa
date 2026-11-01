# Contributing to Invoxa

Thanks for taking the time to improve Invoxa. This repository is a working MVP, so changes should preserve the tested invoice workflow and the security boundaries around tenant isolation and approval.

## Development setup

See [`docs/SETUP.md`](docs/SETUP.md) for the full local setup. The short version:

```bash
cp .env.example .env
# Generate ENCRYPTION_KEY and CSRF_SECRET, then configure DATABASE_URL.
cargo run -p invoxa-api --bin invoxa-seed
cargo run -p invoxa-api --bin invoxa-api
cargo run -p invoxa-api --bin invoxa-worker
```

In a second terminal:

```bash
cd apps/web
npm ci
npm run dev
```

## Before opening a pull request

Run the same checks as CI:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo test --workspace

cd apps/web
npm run typecheck
npm run build
```

For changes to the critical path, also run the integration scripts against a running API and worker:

```bash
node scripts/smoke.mjs
node scripts/security-smoke.mjs
node scripts/webhook-smoke.mjs
```

Browser workflow:

```bash
cd apps/web
node scripts/ui-e2e.mjs
node scripts/ui-audit.mjs
```

## Guidelines

- Keep money exact. Do not introduce floating-point arithmetic for invoice values.
- Treat extraction output as untrusted and preserve `null` for missing values.
- Keep every tenant-scoped query scoped by `organization_id`.
- Approval must remain an explicit server-side transition. Never add an auto-approval path.
- Do not commit `.env` files, real invoices, API keys, webhook secrets, or provider credentials.
- Use the synthetic files in `fixtures/` for tests and screenshots.
- Add or update tests when changing validation, approval, export, auth, or webhook behavior.
- Keep API changes reflected in `apps/api/src/openapi.rs` and `docs/API.md`.
- Prefer small, focused pull requests with a clear verification section.

## Reporting security issues

Do not open a public issue for a vulnerability. Follow [`SECURITY.md`](SECURITY.md).
