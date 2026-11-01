// Real cross-tenant and authorization smoke test against a running Invoxa API.
// It creates two disposable organizations, uploads an invoice into the first,
// and proves the second cannot read, mutate, approve, export, or download it.
//
// Usage: node scripts/security-smoke.mjs [baseUrl]
import { readFile } from 'node:fs/promises';
import { basename } from 'node:path';

const base = (process.argv[2] || 'http://127.0.0.1:8080').replace(/\/$/, '');
const suffix = `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
const password = 'InvoxaSecurity!2026';

class Client {
  constructor(label) {
    this.label = label;
    this.jar = new Map();
  }

  cookieHeader() {
    return [...this.jar.entries()].map(([key, value]) => `${key}=${value}`).join('; ');
  }

  storeCookies(response) {
    for (const cookie of response.headers.getSetCookie?.() || []) {
      const [pair] = cookie.split(';');
      const index = pair.indexOf('=');
      if (index > 0) this.jar.set(pair.slice(0, index), pair.slice(index + 1));
    }
  }

  async call(path, { method = 'GET', body, headers = {}, form, csrf = true } = {}) {
    const init = { method, headers: { ...headers }, redirect: 'manual' };
    if (this.jar.size) init.headers.cookie = this.cookieHeader();
    if (body !== undefined) {
      init.headers['content-type'] = 'application/json';
      init.body = JSON.stringify(body);
    }
    if (form) init.body = form;
    if (csrf && !['GET', 'HEAD', 'OPTIONS'].includes(method) && this.jar.has('invoxa_csrf')) {
      init.headers['x-csrf-token'] = this.jar.get('invoxa_csrf');
    }
    const response = await fetch(base + path, init);
    this.storeCookies(response);
    const text = await response.text();
    let data = null;
    try { data = text ? JSON.parse(text) : null; } catch { data = text; }
    return { response, status: response.status, data };
  }

  async ok(path, options) {
    const result = await this.call(path, options);
    if (result.status < 200 || result.status >= 300) {
      throw new Error(`${this.label}: ${options?.method || 'GET'} ${path} -> ${result.status}: ${JSON.stringify(result.data).slice(0, 600)}`);
    }
    return result.data;
  }

  async expect(path, expectedStatus, options) {
    const result = await this.call(path, options);
    if (result.status !== expectedStatus) {
      throw new Error(`${this.label}: expected ${expectedStatus} from ${options?.method || 'GET'} ${path}, got ${result.status}: ${JSON.stringify(result.data).slice(0, 600)}`);
    }
    return result.data;
  }
}

async function signup(client, organizationName) {
  const email = `${client.label.toLowerCase().replace(/[^a-z0-9]/g, '')}-${suffix}@example.test`;
  await client.ok('/api/v1/auth/signup', {
    method: 'POST',
    body: {
      email,
      password,
      name: `${client.label} Owner`,
      organization_name: organizationName,
      country: 'IN',
      base_currency: 'INR',
    },
  });
  return email;
}

async function uploadInvoice(client, path) {
  const bytes = await readFile(path);
  const form = new FormData();
  form.append('files', new Blob([bytes], { type: 'application/pdf' }), basename(path));
  const uploaded = await client.ok('/api/v1/documents', { method: 'POST', form });
  const document = uploaded.documents?.[0]?.document;
  if (!document?.id) throw new Error('upload did not return a document');
  return document.id;
}

async function waitForInvoice(client, documentId) {
  for (let attempt = 0; attempt < 60; attempt += 1) {
    const document = await client.ok(`/api/v1/documents/${documentId}`);
    if (document.status === 'failed') {
      throw new Error(`processing failed: ${document.error_code} ${document.error_message}`);
    }
    if (document.invoice?.id) return document.invoice.id;
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  throw new Error('timed out waiting for extraction');
}

const a = new Client('TenantA');
const b = new Client('TenantB');
const anonymous = new Client('Anonymous');

console.log('base:', base);
const emailA = await signup(a, `Tenant A ${suffix}`);
const emailB = await signup(b, `Tenant B ${suffix}`);
console.log('created organizations for', emailA, 'and', emailB);

// Cookie-authenticated mutations without the CSRF header must be rejected.
const noCsrf = await a.call('/api/v1/exports', {
  method: 'POST',
  csrf: false,
  body: { format: 'csv', filters: {} },
});
if (noCsrf.status !== 403) {
  throw new Error(`CSRF check failed: expected 403, got ${noCsrf.status}`);
}
console.log('csrf: rejected cookie mutation without token');

const documentId = await uploadInvoice(a, 'fixtures/acme_invoice.pdf');
const invoiceId = await waitForInvoice(a, documentId);
console.log('tenant A invoice:', invoiceId);

const aInvoices = await a.ok('/api/v1/invoices?limit=100');
if (!aInvoices.items.some((invoice) => invoice.id === invoiceId)) {
  throw new Error('tenant A cannot see its own invoice');
}
const bInvoices = await b.ok('/api/v1/invoices?limit=100');
if (bInvoices.items.some((invoice) => invoice.id === invoiceId)) {
  throw new Error('tenant B list leaked tenant A invoice');
}
console.log('list isolation: ok');

await b.expect(`/api/v1/invoices/${invoiceId}`, 404);
await b.expect(`/api/v1/documents/${documentId}`, 404);
await b.expect(`/api/v1/documents/${documentId}/content`, 404);
await b.expect(`/api/v1/documents/${documentId}/download-url`, 404, { method: 'POST', body: {} });
await b.expect(`/api/v1/documents/${documentId}`, 404, { method: 'DELETE' });
await b.expect(`/api/v1/invoices/${invoiceId}`, 404, {
  method: 'PATCH',
  body: { version: 1, invoice_number: 'TENANT-B-OVERWRITE' },
});
await b.expect(`/api/v1/invoices/${invoiceId}/approve`, 404, {
  method: 'POST',
  body: { version: 1, acknowledge_mock: true },
});
await b.expect(`/api/v1/invoices/${invoiceId}/reject`, 404, {
  method: 'POST',
  body: { version: 1, reason: 'cross tenant attempt' },
});
await b.expect(`/api/v1/invoices/${invoiceId}/notes`, 404, {
  method: 'POST',
  body: { notes: 'cross tenant attempt' },
});
console.log('resource isolation: all cross-tenant reads and mutations returned 404');

await anonymous.expect('/api/v1/invoices', 401);
await anonymous.expect(`/api/v1/documents/${documentId}/content`, 401);
console.log('anonymous access: rejected with 401');

// Create a scoped API key for A and prove bearer auth works without cookies.
const key = await a.ok('/api/v1/api-keys', {
  method: 'POST',
  body: { name: `Security smoke ${suffix}`, scopes: ['invoices:read', 'documents:read'], expires_in_days: 1 },
});
if (!key.secret?.startsWith('ivx_')) throw new Error('API key secret was not returned');
const keyClient = new Client('ApiKeyA');
const keyInvoices = await keyClient.ok('/api/v1/invoices?limit=100', {
  headers: { authorization: `Bearer ${key.secret}` },
});
if (!keyInvoices.items.some((invoice) => invoice.id === invoiceId)) {
  throw new Error('API key could not read its own organization invoice');
}
await keyClient.expect('/api/v1/invoices', 401, {
  headers: { authorization: 'Bearer inv_invalid_key_value' },
});
await keyClient.expect('/api/v1/exports', 403, {
  method: 'POST',
  headers: { authorization: `Bearer ${key.secret}` },
  body: { format: 'csv', filters: {} },
});
console.log('api key: bearer auth, scope enforcement, and invalid-key rejection work');

// Approve A's invoice, export it, then prove B cannot see or download the artifact.
const detail = await a.ok(`/api/v1/invoices/${invoiceId}`);
for (const duplicate of detail.duplicates.filter((row) => row.status === 'open')) {
  await a.ok(`/api/v1/invoices/${invoiceId}/duplicates/${duplicate.id}/resolve`, {
    method: 'POST',
    body: { status: 'dismissed', note: 'Security smoke reviewed duplicate candidate.' },
  });
}
const approved = await a.ok(`/api/v1/invoices/${invoiceId}/approve`, {
  method: 'POST',
  body: {
    version: detail.version,
    acknowledge_mock: true,
    reason: 'Security smoke approved after explicit mock acknowledgement.',
  },
});
if (approved.status !== 'approved') throw new Error(`tenant A approval failed: ${approved.status}`);

const selectedExport = await a.ok('/api/v1/exports', {
  method: 'POST',
  body: {
    format: 'json',
    filters: { invoice_ids: [invoiceId] },
    idempotency_key: `security-${suffix}`,
  },
});
let exportJob = selectedExport;
for (let attempt = 0; attempt < 60 && exportJob.status !== 'succeeded'; attempt += 1) {
  await new Promise((resolve) => setTimeout(resolve, 500));
  exportJob = await a.ok(`/api/v1/exports/${exportJob.id}`);
}
if (exportJob.status !== 'succeeded') {
  throw new Error(`selected export did not succeed: ${JSON.stringify(exportJob)}`);
}
const artifact = await a.ok(`/api/v1/exports/${exportJob.id}/download`);
const rows = Array.isArray(artifact) ? artifact : artifact?.invoices;
if (!Array.isArray(rows) || rows.length !== 1 || rows[0].invoice_id !== invoiceId) {
  throw new Error('selected export did not contain exactly the selected approved invoice');
}
console.log('selected export: exactly one authorized invoice');

await b.expect(`/api/v1/exports/${exportJob.id}`, 404);
await b.expect(`/api/v1/exports/${exportJob.id}/download`, 404);
console.log('export isolation: cross-tenant detail and artifact download returned 404');

// API key revocation takes effect immediately.
await a.ok(`/api/v1/api-keys/${key.id}`, { method: 'DELETE' });
await keyClient.expect('/api/v1/invoices', 401, {
  headers: { authorization: `Bearer ${key.secret}` },
});
console.log('api key revocation: immediate');

console.log('SECURITY SMOKE OK');
