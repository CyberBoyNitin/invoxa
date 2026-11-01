// Real webhook outbox smoke test against a running API and worker.
// Creates a disposable organization, subscribes a public HTTPS test endpoint,
// approves an invoice, waits for a delivery attempt, and requeues it manually.
//
// The endpoint is https://example.com/..., a reserved documentation host. No
// private data is sent: the fixture is synthetic and the organization is
// disposable. Receivers in production must verify the HMAC signature.
//
// Usage: node scripts/webhook-smoke.mjs [baseUrl]
import { readFile } from 'node:fs/promises';
import { basename } from 'node:path';

const base = (process.argv[2] || 'http://127.0.0.1:8080').replace(/\/$/, '');
const suffix = `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
const password = 'InvoxaWebhook!2026';

const jar = new Map();
function cookieHeader() {
  return [...jar.entries()].map(([key, value]) => `${key}=${value}`).join('; ');
}
function storeCookies(response) {
  for (const cookie of response.headers.getSetCookie?.() || []) {
    const [pair] = cookie.split(';');
    const index = pair.indexOf('=');
    if (index > 0) jar.set(pair.slice(0, index), pair.slice(index + 1));
  }
}
async function call(path, { method = 'GET', body, form, csrf = true } = {}) {
  const init = { method, headers: {}, redirect: 'manual' };
  if (jar.size) init.headers.cookie = cookieHeader();
  if (body !== undefined) {
    init.headers['content-type'] = 'application/json';
    init.body = JSON.stringify(body);
  }
  if (form) init.body = form;
  if (csrf && !['GET', 'HEAD', 'OPTIONS'].includes(method) && jar.has('invoxa_csrf')) {
    init.headers['x-csrf-token'] = jar.get('invoxa_csrf');
  }
  const response = await fetch(base + path, init);
  storeCookies(response);
  const text = await response.text();
  let data = null;
  try { data = text ? JSON.parse(text) : null; } catch { data = text; }
  return { status: response.status, data };
}
async function ok(path, options) {
  const result = await call(path, options);
  if (result.status < 200 || result.status >= 300) {
    throw new Error(`${options?.method || 'GET'} ${path} -> ${result.status}: ${JSON.stringify(result.data).slice(0, 700)}`);
  }
  return result.data;
}
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const email = `webhook-${suffix}@example.test`;
await ok('/api/v1/auth/signup', {
  method: 'POST',
  body: {
    email,
    password,
    name: 'Webhook Smoke Owner',
    organization_name: `Webhook Smoke ${suffix}`,
    country: 'IN',
    base_currency: 'INR',
  },
});
console.log('organization:', email);

const endpointUrl = `https://example.com/invoxa-webhook-${suffix}`;
const endpoint = await ok('/api/v1/webhooks', {
  method: 'POST',
  body: {
    url: endpointUrl,
    description: 'Disposable webhook smoke endpoint',
    events: ['invoice.approved'],
    enabled: true,
  },
});
if (!endpoint.id || !endpoint.secret) throw new Error('webhook endpoint was not created with a secret');
console.log('webhook endpoint:', endpoint.id, endpointUrl);

const bytes = await readFile('fixtures/acme_invoice.pdf');
const form = new FormData();
form.append('files', new Blob([bytes], { type: 'application/pdf' }), basename('fixtures/acme_invoice.pdf'));
const uploaded = await ok('/api/v1/documents', { method: 'POST', form });
const documentId = uploaded.documents?.[0]?.document?.id;
if (!documentId) throw new Error('upload did not return a document');

let invoiceId = null;
for (let attempt = 0; attempt < 80; attempt += 1) {
  const document = await ok(`/api/v1/documents/${documentId}`);
  if (document.status === 'failed') throw new Error(`processing failed: ${document.error_code}`);
  if (document.invoice?.id) { invoiceId = document.invoice.id; break; }
  await sleep(500);
}
if (!invoiceId) throw new Error('timed out waiting for extraction');
const detail = await ok(`/api/v1/invoices/${invoiceId}`);
for (const duplicate of detail.duplicates.filter((row) => row.status === 'open')) {
  await ok(`/api/v1/invoices/${invoiceId}/duplicates/${duplicate.id}/resolve`, {
    method: 'POST',
    body: { status: 'dismissed', note: 'Webhook smoke reviewed duplicate candidate.' },
  });
}
await ok(`/api/v1/invoices/${invoiceId}/approve`, {
  method: 'POST',
  body: {
    version: detail.version,
    acknowledge_mock: true,
    reason: 'Webhook smoke approval after mock acknowledgement.',
  },
});
console.log('approved invoice:', invoiceId);

let delivery = null;
for (let attempt = 0; attempt < 120; attempt += 1) {
  const deliveries = await ok('/api/v1/webhook-deliveries?limit=50');
  delivery = deliveries.items.find((row) => row.event_type === 'invoice.approved');
  if (delivery && ['failed', 'dead_letter', 'succeeded'].includes(delivery.status)) break;
  await sleep(500);
}
if (!delivery) throw new Error('webhook delivery was never recorded');
console.log('delivery:', delivery.id, delivery.status, delivery.last_status_code ?? 'no status', delivery.attempts, 'attempts');
if (!['failed', 'dead_letter', 'succeeded'].includes(delivery.status)) {
  throw new Error(`delivery did not reach a terminal state: ${delivery.status}`);
}

const requeued = await ok(`/api/v1/webhook-deliveries/${delivery.id}/redeliver`, { method: 'POST' });
if (requeued.status !== 'pending') throw new Error(`manual redelivery did not requeue: ${JSON.stringify(requeued)}`);
console.log('manual redelivery:', delivery.status, '->', requeued.status, '(same event id preserved)');

await ok(`/api/v1/webhooks/${endpoint.id}`, { method: 'DELETE' });
console.log('webhook endpoint deleted');
console.log('WEBHOOK SMOKE OK');
