import { chromium } from "playwright";
import { mkdir } from "node:fs/promises";

const base = process.env.UI_BASE || "http://localhost:3000";
const outDir = process.env.UI_OUT || "../../.local/ui";
await mkdir(outDir, { recursive: true });

const browser = await chromium.launch({ headless: true });
const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
const page = await context.newPage();

const consoleErrors = [];
const pageErrors = [];
const badResponses = [];
page.on("console", (message) => {
  if (message.type() === "error") consoleErrors.push(message.text());
});
page.on("pageerror", (error) => pageErrors.push(error.message));
page.on("response", (response) => {
  if (response.status() >= 400) badResponses.push(`${response.status()} ${response.request().method()} ${response.url()}`);
});

await page.goto(`${base}/login`, { waitUntil: "domcontentloaded" });
await page.getByLabel("Work email").fill("demo@invoxa.local");
await page.getByLabel("Password").fill("InvoxaDemo!2026");
await page.getByRole("button", { name: "Sign in" }).click();
await page.waitForURL("**/dashboard", { timeout: 20000 });
await page.waitForTimeout(1200);
await page.screenshot({ path: `${outDir}/dashboard.png`, fullPage: true });
console.log("login: ok");

const routes = [
  ["/dashboard", "Overview"],
  ["/review", "Review queue"],
  ["/invoices", "Invoices"],
  ["/vendors", "Vendors"],
  ["/documents", "Documents"],
  ["/automation", "Automations"],
  ["/integrations", "Integrations"],
  ["/activity", "Activity log"],
  ["/settings/organization", "Organization"],
  ["/settings/team", "Team"],
  ["/settings/api-keys", "API keys"],
  ["/settings/usage", "Usage"],
];

for (const [path, expected] of routes) {
  const before = badResponses.length;
  await page.goto(`${base}${path}`, { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(900);
  const h1 = (await page.locator("h1").first().textContent().catch(() => ""))?.trim() ?? "";
  const bodyText = await page.locator("body").innerText();
  const newBad = badResponses.slice(before);
  const errorVisible = /Application error|Internal Server Error|This page could not be found/i.test(bodyText);
  console.log(`${path}: h1="${h1}" expected~"${expected}" errors=${newBad.length} appError=${errorVisible}`);
  if (newBad.length) console.log("  bad:", newBad.join(" | "));
  await page.screenshot({ path: `${outDir}/${path.replaceAll("/", "_").replace(/^_/, "") || "root"}.png`, fullPage: true });
}

console.log("consoleErrors:", consoleErrors.length);
for (const error of [...new Set(consoleErrors)].slice(0, 20)) console.log("  ", error);
console.log("pageErrors:", pageErrors.length);
for (const error of [...new Set(pageErrors)].slice(0, 20)) console.log("  ", error);
console.log("badResponses:", badResponses.length);
for (const response of [...new Set(badResponses)].slice(0, 30)) console.log("  ", response);

await browser.close();
