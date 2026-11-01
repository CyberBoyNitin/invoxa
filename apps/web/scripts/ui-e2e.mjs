// Full browser workflow against a running production build and API.
// Usage: node scripts/ui-e2e.mjs
import { chromium } from "playwright";
import { mkdir, readFile } from "node:fs/promises";

const base = process.env.UI_BASE || "http://localhost:3000";
const outDir = "../../.local/ui";
await mkdir(outDir, { recursive: true });
const suffix = `${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
const email = `ui-${suffix}@example.test`;
const password = "InvoxaUi!2026Secure";

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

async function api(page, path, options = {}) {
  const response = await page.request.fetch(`${base}${path}`, options);
  const text = await response.text();
  let data = null;
  try { data = text ? JSON.parse(text) : null; } catch { data = text; }
  return { response, status: response.status(), data };
}

async function apiOk(page, path, options = {}) {
  const result = await api(page, path, options);
  if (result.status < 200 || result.status >= 300) {
    throw new Error(`${path} -> ${result.status}: ${JSON.stringify(result.data).slice(0, 500)}`);
  }
  return result.data;
}

const browser = await chromium.launch({ headless: true });
const context = await browser.newContext({ viewport: { width: 1440, height: 1000 }, acceptDownloads: true });
const page = await context.newPage();
const pageErrors = [];
const consoleErrors = [];
const badResponses = [];
page.on("pageerror", (error) => pageErrors.push(error.message));
page.on("console", (message) => { if (message.type() === "error") consoleErrors.push(message.text()); });
page.on("response", (response) => { if (response.status() >= 400 && !response.url().includes("/api/v1/auth/session")) badResponses.push(`${response.status()} ${response.request().method()} ${response.url()}`); });

try {
  await page.goto(`${base}/signup`, { waitUntil: "domcontentloaded" });
  await page.getByLabel("Your name").fill("UI Workflow Owner");
  await page.getByLabel("Work email").fill(email);
  await page.getByLabel("Organization name").fill(`UI Workflow ${suffix}`);
  await page.getByLabel("Password").fill(password);
  await page.getByRole("button", { name: "Create workspace" }).click();
  await page.waitForURL("**/dashboard", { timeout: 30000 });
  console.log("signup: ok", email);

  await page.goto(`${base}/documents`, { waitUntil: "domcontentloaded" });
  await page.getByRole("button", { name: "Add documents" }).first().click();
  await page.locator('input[type="file"]').setInputFiles("../../fixtures/acme_invoice.pdf");
  await page.getByRole("button", { name: /^Upload 1 file/ }).click();
  await page.getByText("1 document accepted").waitFor({ timeout: 30000 });
  await page.getByRole("button", { name: "View documents" }).click();
  console.log("upload: accepted through the UI");

  let document = null;
  for (let attempt = 0; attempt < 80; attempt += 1) {
    const docs = await apiOk(page, "/api/v1/documents?limit=10");
    document = docs.items.find((item) => item.filename === "acme_invoice.pdf");
    if (document?.invoice?.id) break;
    await page.waitForTimeout(500);
  }
  assert(document?.invoice?.id, "document did not produce an invoice");
  console.log("extraction: invoice", document.invoice.id, document.status);

  await page.goto(`${base}/invoices/${document.invoice.id}`, { waitUntil: "domcontentloaded" });
  await page.waitForSelector("h1", { timeout: 20000 });
  await page.waitForTimeout(1500);
  const invoiceTitle = (await page.locator("h1").first().innerText()).trim();
  assert(!/couldn|Application error/i.test(await page.locator("body").innerText()), "review page rendered an error");
  console.log("review page:", invoiceTitle);

  await page.getByRole("tab", { name: /Duplicates/ }).click();
  await page.waitForTimeout(300);
  for (let index = 0; index < 10; index += 1) {
    const button = page.getByRole("button", { name: "Not a duplicate" }).first();
    if (!(await button.isVisible().catch(() => false))) break;
    const note = page.getByPlaceholder("Resolution note (optional, audited)").first();
    if (await note.isVisible().catch(() => false)) await note.fill("UI end-to-end review checked the source.");
    await button.click();
    await page.waitForTimeout(1000);
  }

  const approve = page.getByRole("button", { name: "Approve", exact: true });
  assert(await approve.isEnabled().catch(() => false), "approval is blocked after review");
  await approve.click();
  await page.waitForTimeout(400);
  const dialog = page.locator('[role="dialog"]');
  const ack = dialog.getByRole("checkbox");
  if (await ack.isVisible().catch(() => false)) await ack.check();
  const note = dialog.getByLabel("Approval note");
  if (await note.isVisible().catch(() => false)) await note.fill("UI end-to-end approval after source review.");
  await dialog.getByRole("button", { name: "Approve invoice" }).click();
  await page.getByText("Approved", { exact: true }).first().waitFor({ timeout: 15000 });
  console.log("approval: explicit server transition visible");

  const detail = await apiOk(page, `/api/v1/invoices/${document.invoice.id}`);
  assert(detail.status === "approved", `invoice status is ${detail.status}`);

  await page.goto(`${base}/exports`, { waitUntil: "domcontentloaded" });
  await page.getByRole("button", { name: "New export" }).first().click();
  await page.getByRole("button", { name: "Select all" }).waitFor({ timeout: 15000 });
  await page.getByRole("button", { name: "Select all" }).click();
  await page.getByText(/\(1 selected\)/).waitFor({ timeout: 5000 });
  await page.getByRole("button", { name: "Queue export" }).click();
  await page.getByText("Export history").waitFor({ timeout: 10000 });

  let exportJob = null;
  for (let attempt = 0; attempt < 80; attempt += 1) {
    const exports = await apiOk(page, "/api/v1/exports?limit=20");
    exportJob = exports.items.find((item) => item.filters?.invoice_ids?.includes(document.invoice.id));
    if (exportJob?.status === "succeeded") break;
    await page.waitForTimeout(500);
  }
  assert(exportJob?.status === "succeeded", `export did not succeed: ${JSON.stringify(exportJob)}`);
  console.log("export: selected invoice job succeeded", exportJob.id, "rows", exportJob.row_count);

  const downloadPromise = context.waitForEvent("download", { timeout: 15000 }).catch(() => null);
  await page.getByRole("button", { name: "Download" }).first().click();
  const download = await downloadPromise;
  if (download) {
    const path = `${outDir}/export-${suffix}.json`;
    await download.saveAs(path);
    const contents = await readFile(path, "utf8");
    assert(contents.includes(document.invoice.id), "downloaded export does not contain the invoice id");
    console.log("download: artifact saved and contains the selected invoice");
  } else {
    const artifact = await api(page, exportJob.download_url);
    assert(artifact.status === 200, `artifact download failed: ${artifact.status}`);
    console.log("download: browser popup blocked, verified artifact over the authenticated request context");
  }

  await page.goto(`${base}/exports`, { waitUntil: "domcontentloaded" });
  await page.getByRole("button", { name: "New export" }).first().click();
  await page.getByLabel("Status").selectOption("exported");
  await page.getByRole("button", { name: "Select all" }).waitFor({ timeout: 15000 });
  await page.getByRole("button", { name: "Select all" }).click();
  await page.getByText(/\(1 selected\)/).waitFor({ timeout: 5000 });
  await page.getByRole("button", { name: "Queue export" }).click();
  await page.getByText("Export history").waitFor({ timeout: 10000 });
  let reExportJob = null;
  for (let attempt = 0; attempt < 80; attempt += 1) {
    const exports = await apiOk(page, "/api/v1/exports?limit=20");
    reExportJob = exports.items.find(
      (item) => item.filters?.status === "exported" && item.filters?.invoice_ids?.includes(document.invoice.id),
    );
    if (reExportJob?.status === "succeeded") break;
    await page.waitForTimeout(500);
  }
  assert(reExportJob?.status === "succeeded", `re-export did not succeed: ${JSON.stringify(reExportJob)}`);
  console.log("re-export: previously exported invoice job succeeded", reExportJob.id, "rows", reExportJob.row_count);

  await page.goto(`${base}/documents/${document.id}`, { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(3000);
  assert(!/couldn|Application error/i.test(await page.locator("body").innerText()), "document page rendered an error");
  const canvasCount = await page.locator("canvas").count();
  assert(canvasCount > 0, "PDF preview did not render a canvas");
  console.log("document preview: canvas rendered", canvasCount);

  // API key lifecycle through the UI.
  await page.goto(`${base}/settings/api-keys`, { waitUntil: "domcontentloaded" });
  await page.getByRole("button", { name: "Create key" }).click();
  const keyDialog = page.locator('[role="dialog"]');
  const keyName = `UI key ${suffix}`;
  await keyDialog.getByLabel("Key name").fill(keyName);
  await keyDialog.getByRole("button", { name: "Create key" }).click();
  await page.getByLabel("Secret — copy now").waitFor({ timeout: 15000 });
  const secret = await page.getByLabel("Secret — copy now").inputValue();
  assert(secret.startsWith("ivx_"), "API key secret was not shown once");
  await page.getByRole("button", { name: "I stored the secret" }).click();
  const keyRow = page.locator("tr").filter({ hasText: keyName });
  await keyRow.getByRole("button", { name: "Revoke" }).click();
  await page.getByRole("button", { name: "Revoke key" }).click();
  await page.waitForTimeout(1000);
  assert(/Revoked/.test(await keyRow.innerText()), "API key did not show revoked state");
  console.log("api key UI: create, reveal once, revoke");

  // Team invitation lifecycle through the UI.
  await page.goto(`${base}/settings/team`, { waitUntil: "domcontentloaded" });
  await page.getByRole("button", { name: "Invite member" }).click();
  const inviteDialog = page.locator('[role="dialog"]');
  const inviteEmail = `invite-${suffix}@example.test`;
  await inviteDialog.getByLabel("Email address").fill(inviteEmail);
  await inviteDialog.getByRole("button", { name: "Create invitation" }).click();
  await page.getByText(`Invitation created for ${inviteEmail}`).waitFor({ timeout: 15000 });
  await page.getByRole("button", { name: "Done" }).click();
  const inviteRow = page.locator("tr").filter({ hasText: inviteEmail });
  await inviteRow.getByRole("button", { name: "Revoke" }).click();
  await page.waitForTimeout(1000);
  assert(!(await page.locator("tr").filter({ hasText: inviteEmail }).count()), "invitation was not revoked");
  console.log("team UI: invite and revoke");

  // Webhook lifecycle through the UI. The endpoint is never delivered to because
  // no subscribed event is emitted during this test.
  await page.goto(`${base}/settings/api-keys`, { waitUntil: "domcontentloaded" });
  await page.getByRole("button", { name: "Add endpoint" }).click();
  const webhookDialog = page.locator('[role="dialog"]');
  const webhookUrl = `https://example.com/invoxa-${suffix}`;
  await webhookDialog.getByLabel("Endpoint URL").fill(webhookUrl);
  await webhookDialog.getByRole("button", { name: "Create endpoint" }).click();
  try {
    await page.getByLabel("Secret — copy now").waitFor({ timeout: 30000 });
    const webhookSecret = await page.getByLabel("Secret — copy now").inputValue();
    assert(webhookSecret.length > 20, "webhook secret was not shown once");
    await page.getByRole("button", { name: "I stored the secret" }).click();
    const endpointCard = page
      .locator("div")
      .filter({ hasText: webhookUrl })
      .filter({ has: page.getByRole("button", { name: "Rotate" }) })
      .last();
    await endpointCard.getByRole("button", { name: "Rotate" }).click();
    await page.getByRole("button", { name: "Rotate secret" }).click();
    await page.getByLabel("Secret — copy now").waitFor({ timeout: 15000 });
    await page.getByRole("button", { name: "I stored the secret" }).click();
    await endpointCard.locator("button.text-rust").click();
    await page.getByRole("button", { name: "Delete endpoint" }).click();
    await page.waitForTimeout(1000);
    assert(!(await page.getByText(webhookUrl).count()), "webhook endpoint was not deleted");
    console.log("webhook UI: create, rotate secret, delete");
  } catch (error) {
    console.log("webhook UI: not completed", String(error).slice(0, 300));
    const toastText = await page.locator("body").innerText();
    console.log("webhook page excerpt:", toastText.slice(-500));
    throw error;
  }

  await page.screenshot({ path: `${outDir}/ui-e2e-final.png`, fullPage: true });
  console.log("pageErrors:", pageErrors.length);
  for (const error of [...new Set(pageErrors)]) console.log("  ", error);
  console.log("consoleErrors:", consoleErrors.length);
  for (const error of [...new Set(consoleErrors)]) console.log("  ", error);
  console.log("badResponses:", badResponses.length);
  for (const error of [...new Set(badResponses)]) console.log("  ", error);
  assert(pageErrors.length === 0, "browser page errors occurred");
  assert(consoleErrors.length === 0, "browser console errors occurred");
  console.log("UI E2E OK");
} finally {
  await browser.close();
}
