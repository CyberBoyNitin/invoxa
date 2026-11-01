// One-off local helper used to create a clean workspace for README screenshots.
// This file lives under .local/ and is intentionally not committed.
import { chromium } from "playwright";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

const base = "http://localhost:3000";
const suffix = `${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
const email = `shots-${suffix}@example.test`;
const password = "InvoxaShots!2026";
const orgName = "Meridian Finance";
const userName = "Avery Chen";
const outDir = resolve(process.cwd(), "../../.local");
await mkdir(outDir, { recursive: true });

const browser = await chromium.launch({ headless: true });
const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
const page = await context.newPage();
let csrf = "";

async function api(path, { method = "GET", body } = {}) {
  const headers = {};
  if (body !== undefined) headers["content-type"] = "application/json";
  if (method !== "GET") headers["x-csrf-token"] = csrf;
  const response = await page.request.fetch(`${base}${path}`, {
    method,
    headers,
    data: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await response.text();
  let data = null;
  try { data = text ? JSON.parse(text) : null; } catch { data = text; }
  if (!response.ok()) throw new Error(`${method} ${path} -> ${response.status()}: ${text.slice(0, 500)}`);
  return data;
}

async function uploadFixture() {
  await page.goto(`${base}/documents`, { waitUntil: "domcontentloaded" });
  await page.getByRole("button", { name: "Add documents" }).first().click();
  await page.locator('input[type="file"]').setInputFiles("../../fixtures/acme_invoice.pdf");
  await page.getByRole("button", { name: /^Upload 1 file/ }).click();
  await page.getByText("1 document accepted").waitFor({ timeout: 30000 });
  await page.getByRole("button", { name: "View documents" }).click();
}

async function waitForInvoice(previousIds = new Set()) {
  for (let attempt = 0; attempt < 120; attempt += 1) {
    const docs = await api("/api/v1/documents?limit=20");
    const doc = docs.items.find((item) => item.invoice?.id && !previousIds.has(item.id));
    if (doc?.invoice?.id) return doc;
    await page.waitForTimeout(500);
  }
  throw new Error("uploaded document never produced an invoice");
}

async function dismissDuplicates(invoiceId) {
  let detail = await api(`/api/v1/invoices/${invoiceId}`);
  for (const duplicate of (detail.duplicates || []).filter((row) => row.status === "open")) {
    await api(`/api/v1/invoices/${invoiceId}/duplicates/${duplicate.id}/resolve`, {
      method: "POST",
      body: { status: "dismissed", note: "Screenshot fixture review." },
    });
  }
  return api(`/api/v1/invoices/${invoiceId}`);
}

try {
  await page.goto(`${base}/signup`, { waitUntil: "domcontentloaded" });
  await page.getByLabel("Your name").fill(userName);
  await page.getByLabel("Work email").fill(email);
  await page.getByLabel("Organization name").fill(orgName);
  await page.getByLabel("Password").fill(password);
  await page.getByRole("button", { name: "Create workspace" }).click();
  await page.waitForURL("**/dashboard", { timeout: 30000 });
  csrf = (await context.cookies()).find((cookie) => cookie.name === "invoxa_csrf")?.value || "";

  const known = new Set();
  await uploadFixture();
  const first = await waitForInvoice(known);
  known.add(first.id);
  const firstDetail = await dismissDuplicates(first.invoice.id);
  if (firstDetail.status === "review_required") {
    await api(`/api/v1/invoices/${first.invoice.id}/approve`, {
      method: "POST",
      body: {
        version: firstDetail.version,
        acknowledge_mock: true,
        reason: "Screenshot fixture: reviewed and approved.",
      },
    });
  }
  const exportJob = await api("/api/v1/exports", {
    method: "POST",
    body: {
      format: "json",
      filters: { status: "approved", invoice_ids: [first.invoice.id] },
      idempotency_key: crypto.randomUUID(),
    },
  });
  for (let attempt = 0; attempt < 80; attempt += 1) {
    const job = await api(`/api/v1/exports/${exportJob.id}`);
    if (job.status === "succeeded") break;
    await page.waitForTimeout(500);
  }

  await uploadFixture();
  const second = await waitForInvoice(known);
  await dismissDuplicates(second.invoice.id);

  await writeFile(
    resolve(outDir, "screenshot-credentials.json"),
    JSON.stringify({ email, password, orgName, firstInvoiceId: first.invoice.id, secondInvoiceId: second.invoice.id }, null, 2),
  );
  console.log(JSON.stringify({ email, password, orgName, firstInvoiceId: first.invoice.id, secondInvoiceId: second.invoice.id }, null, 2));
} finally {
  await browser.close();
}
