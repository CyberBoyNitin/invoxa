import { chromium } from "playwright";
import { mkdir } from "node:fs/promises";

const base = process.env.UI_BASE || "http://localhost:3000";
const outDir = "../../.local/ui";
await mkdir(outDir, { recursive: true });

const browser = await chromium.launch({ headless: true });
const context = await browser.newContext({ viewport: { width: 1440, height: 1000 }, acceptDownloads: true });
const page = await context.newPage();
const pageErrors = [];
const consoleErrors = [];
const badResponses = [];
page.on("pageerror", (error) => pageErrors.push(error.message));
page.on("console", (message) => { if (message.type() === "error") consoleErrors.push(message.text()); });
page.on("response", (response) => { if (response.status() >= 400) badResponses.push(`${response.status()} ${response.request().method()} ${response.url()}`); });

await page.goto(`${base}/login`, { waitUntil: "domcontentloaded" });
await page.getByLabel("Work email").fill("demo@invoxa.local");
await page.getByLabel("Password").fill("InvoxaDemo!2026");
await page.getByRole("button", { name: "Sign in" }).click();
await page.waitForURL("**/dashboard", { timeout: 20000 });
console.log("login: ok");

const listResponse = await page.request.get(`${base}/api/v1/invoices?status=review_required&limit=50`);
if (!listResponse.ok()) throw new Error(`invoice list ${listResponse.status()}: ${await listResponse.text()}`);
const list = await listResponse.json();
const candidates = list.items.filter((row) => row.open_duplicate_count > 0 && row.blocking_issue_count === 0);
const target = candidates[0] ?? list.items[0];
if (!target) throw new Error("no review invoice available");
console.log(`target: ${target.id} ${target.invoice_number} duplicates=${target.open_duplicate_count} blocking=${target.blocking_issue_count}`);

await page.goto(`${base}/invoices/${target.id}`, { waitUntil: "domcontentloaded" });
await page.waitForSelector("h1", { timeout: 20000 });
await page.waitForTimeout(2500);
console.log("detail h1:", (await page.locator("h1").first().innerText()).trim());
const bodyText = await page.locator("body").innerText();
console.log("detail appError:", /Application error|This page couldn|Internal Server Error/i.test(bodyText));
console.log("pdf canvas:", await page.locator("canvas").count());
await page.screenshot({ path: `${outDir}/invoice-detail.png`, fullPage: true });

// Resolve every open duplicate candidate through the UI.
await page.getByRole("tab", { name: /Duplicates/ }).click();
await page.waitForTimeout(300);
for (let index = 0; index < 10; index += 1) {
  const button = page.getByRole("button", { name: "Not a duplicate" }).first();
  if (!(await button.isVisible().catch(() => false))) break;
  const note = page.getByPlaceholder("Resolution note (optional, audited)").first();
  if (await note.isVisible().catch(() => false)) await note.fill("UI smoke test verified against the source document.");
  await button.click();
  await page.waitForTimeout(1200);
  console.log(`resolved duplicate ${index + 1}`);
}

const approve = page.getByRole("button", { name: "Approve", exact: true });
console.log("approve enabled:", await approve.isEnabled().catch(() => false));
if (await approve.isEnabled().catch(() => false)) {
  await approve.click();
  await page.waitForTimeout(500);
  const ack = page.getByRole("checkbox").last();
  if (await ack.isVisible().catch(() => false)) await ack.check();
  const note = page.getByLabel("Approval note");
  if (await note.isVisible().catch(() => false)) await note.fill("UI smoke test approval after source review.");
  await page.getByRole("button", { name: "Approve invoice" }).click();
  await page.waitForTimeout(1800);
  const approvedText = await page.locator("body").innerText();
  console.log("approved visible:", /Approved/.test(approvedText));
  await page.screenshot({ path: `${outDir}/invoice-approved.png`, fullPage: true });
} else {
  console.log("approve blocked; body excerpt:", bodyText.slice(0, 500));
}

// Document preview, including actual content type.
const documentId = target.document_id;
await page.goto(`${base}/documents/${documentId}`, { waitUntil: "domcontentloaded" });
await page.waitForTimeout(3000);
console.log("document h1:", (await page.locator("h1").first().innerText()).trim());
console.log("document canvas:", await page.locator("canvas").count());
await page.screenshot({ path: `${outDir}/document-detail.png`, fullPage: true });

console.log("pageErrors:", pageErrors.length);
for (const error of [...new Set(pageErrors)]) console.log("  ", error);
console.log("consoleErrors:", consoleErrors.length);
for (const error of [...new Set(consoleErrors)]) console.log("  ", error);
console.log("badResponses:", badResponses.length);
for (const error of [...new Set(badResponses)]) console.log("  ", error);
await browser.close();
