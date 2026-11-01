# Local Setup

## Prerequisites

- Rust stable (tested with 1.98.1).
- Node.js 24+ and npm (tested with Node 24.15.0 / npm 11.12.1).
- PostgreSQL 17 (this workspace uses portable PostgreSQL 17.11 at `localhost:55432`).
- Tesseract 5.4 (this workspace uses `C:\Program Files\Tesseract-OCR\tesseract.exe`).
- Optional: Docker + Docker Compose.

## 1. Configure environment

```powershell
Copy-Item .env.example .env
```

Generate independent keys:

```powershell
node -e "console.log(require('crypto').randomBytes(32).toString('base64'))"
node -e "console.log(require('crypto').randomBytes(32).toString('base64'))"
```

Put the first in `ENCRYPTION_KEY`, the second in `CSRF_SECRET`. Leave `DOWNLOAD_SIGNING_KEY` empty to derive it from `ENCRYPTION_KEY`.

Set:

```dotenv
DATABASE_URL=postgres://invoxa:invoxa_dev_password@localhost:55432/invoxa
TESSERACT_PATH="C:/Program Files/Tesseract-OCR/tesseract.exe"
AI_PROVIDER=mock
ALLOW_MOCK_AI=true
EXPOSE_DEV_TOKENS=true
```

The `.env` file must be UTF-8 **without BOM**. Use forward slashes in Windows paths.

## 2. Start PostgreSQL

Portable installation in this repository:

```powershell
& .\.local\pgsql\bin\pg_ctl.exe -D .\.local\pgdata -l .\.local\pg.log start
& .\.local\pgsql\bin\pg_ctl.exe -D .\.local\pgdata status
```

If using a system PostgreSQL, create the role/database:

```sql
CREATE ROLE invoxa LOGIN PASSWORD 'invoxa_dev_password';
CREATE DATABASE invoxa OWNER invoxa;
CREATE DATABASE invoxa_test OWNER invoxa;
```

Migrations are embedded in the binaries and run automatically on API/worker startup. They can also be applied by the seed command.

## 3. Build and seed

```powershell
cargo build -p invoxa-api --bin invoxa-api --bin invoxa-worker --bin invoxa-seed
cargo run -p invoxa-api --bin invoxa-seed
```

Seed output:

```
Invoxa demo workspace is ready.
  Email:        demo@invoxa.local
  Password:     InvoxaDemo!2026
  Organization: Northstar Demo Company
  Org id:       <uuid>
```

The demo organization is `is_demo=true` and uses mock extraction by default.

To migrate/seed the test database without changing your shell globally:

```powershell
$env:DATABASE_URL='postgres://invoxa:invoxa_dev_password@localhost:55432/invoxa_test'
cargo run -p invoxa-api --bin invoxa-seed
Remove-Item Env:DATABASE_URL
```

## 4. Run API and worker

Terminal A:

```powershell
.\target\debug\invoxa-api.exe
```

Terminal B:

```powershell
.\target\debug\invoxa-worker.exe
```

Readiness:

```powershell
Invoke-RestMethod http://127.0.0.1:8080/health/ready
# {"checks":{"database":"ok","storage":"ok"},"status":"ready"}
```

## 5. Run frontend

```powershell
cd apps\web
npm ci
npm run typecheck
npm run build
npm run start
```

Open <http://localhost:3000>. The Next.js rewrite proxies `/api/v1/*` to `INVOXA_API_URL` (default `http://127.0.0.1:8080`).

For development with hot reload, use `npm run dev` instead of `npm run build && npm run start`.

## 6. Run tests

From the repository root:

```powershell
cargo test --workspace
node scripts\smoke.mjs
node scripts\security-smoke.mjs
node scripts\webhook-smoke.mjs
```

From `apps\web`:

```powershell
npm run typecheck
npm run build
node scripts\ui-e2e.mjs
node scripts\ui-audit.mjs
```

The browser scripts require Playwright Chromium. Install once if needed:

```powershell
npx playwright install chromium
```

## Troubleshooting

| Symptom | Cause / fix |
|---|---|
| `pool timed out while waiting for an open connection` | PostgreSQL is not running, or `DATABASE_URL` is wrong. |
| API starts but OCR is unavailable | Set `TESSERACT_PATH` to the real executable; use forward slashes on Windows. |
| `ENCRYPTION_KEY must decode to exactly 32 bytes` | Generate with the Node/OpenSSL command above; do not use a passphrase. |
| `next start does not work with output: standalone` | This build intentionally does not set standalone output. |
| PDF preview blank | Run `npm ci` (postinstall copies `pdf.worker.min.mjs`) and check the browser console. |
| Webhook creation rejected | HTTPS on port 443 is required; private/loopback destinations are blocked. |
| Invitation link not emailed | Email delivery is not configured. In development the link is returned in the UI/API. |
| Mock invoice cannot be approved | Check the explicit mock acknowledgement checkbox. This is intentional. |

## Data and reset

- Local storage: `.local/storage`.
- PostgreSQL data: `.local/pgdata`.
- To reset the demo database, drop/recreate `invoxa` and rerun the seed. Do not do this in production.
- Test scripts create disposable organizations; they do not modify the demo organization's invoices except `scripts/smoke.mjs`, which uses the demo login.
