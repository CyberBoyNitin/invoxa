// Capture clean README screenshots from the running production frontend.
// Usage:
//   cd apps/web
//   node scripts/capture-readme-screenshots.mjs
//
// Requires the demo workspace (cargo run -p invoxa-api --bin invoxa-seed)
// and the API, worker, and frontend to be running.
import { chromium } from "playwright";
import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";

const base = (process.env.UI_BASE || "http://localhost:3000").replace(/\/$/, "");
const email = process.env.INVOXA_SCREENSHOT_EMAIL || "demo@invoxa.local";
const password = process.env.INVOXA_SCREENSHOT_PASSWORD || "InvoxaDemo!2026";
const outDir = resolve(process.cwd(), "../../docs/screenshots");

await mkdir(outDir, { recursive: true });

const browser = await chromium.launch({ headless: true });
const context = await browser.newContext({
  viewport: { width: 1440, height: 1000 },
  deviceScaleFactor: 1,
  colorScheme: "light",
});
const page = await context.newPage();

async function shot(name) {
  await page.waitForTimeout(1200);
  await page.screenshot({ path: resolve(outDir, `${name}.png`), fullPage: false });
  console.log(`captured ${name}.png`);
}

try {
  await page.goto(`${base}/login`, { waitUntil: "domcontentloaded" });
  await page.getByLabel("Work email").fill(email);
  await page.getByLabel("Password").fill(password);
  await page.getByRole("button", { name: "Sign in" }).click();
  await page.waitForURL(/\/dashboard/, { timeout: 20000 });
  await page.getByRole("heading", { level: 1 }).waitFor({ timeout: 15000 });
  await page.getByText("Awaiting review", { exact: false }).first().waitFor({ timeout: 20000 });
  await page.getByText("Processing volume", { exact: false }).first().waitFor({ timeout: 20000 });
  await page.waitForTimeout(4500);
  await shot("dashboard");

  await page.goto(`${base}/review`, { waitUntil: "domcontentloaded" });
  await page.getByRole("heading", { name: /review queue/i }).waitFor({ timeout: 15000 });
  await shot("review");

  const invoices = await page.request.get(`${base}/api/v1/invoices?status=review_required&limit=10`);
  if (invoices.ok()) {
    const payload = await invoices.json();
    const invoice = payload.items?.[0];
    if (invoice?.id) {
      await page.goto(`${base}/invoices/${invoice.id}`, { waitUntil: "domcontentloaded" });
      await page.getByRole("heading", { level: 1 }).waitFor({ timeout: 15000 });
      await shot("invoice-review");
    }
  }

  await page.goto(`${base}/exports`, { waitUntil: "domcontentloaded" });
  await page.getByRole("heading", { name: "Exports", exact: true }).waitFor({ timeout: 15000 });
  await shot("exports");

  await page.goto(`${base}/documents`, { waitUntil: "domcontentloaded" });
  await page.getByRole("heading", { name: "Documents", exact: true }).waitFor({ timeout: 15000 });
  await shot("documents");

  await page.goto(`${base}/settings/api-keys`, { waitUntil: "domcontentloaded" });
  await page.getByRole("heading", { name: "Settings", exact: true }).waitFor({ timeout: 15000 });
  await shot("api-keys");
} finally {
  await browser.close();
}
