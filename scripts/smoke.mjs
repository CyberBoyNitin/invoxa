// Local end-to-end smoke test against a running Invoxa API.
// Usage: node scripts/smoke.mjs [baseUrl]
import { readFile } from 'node:fs/promises';
import { basename } from 'node:path';

const base = (process.argv[2] || 'http://127.0.0.1:8080').replace(/\/$/, '');
const email = process.env.SMOKE_EMAIL || 'demo@invoxa.local';
const password = process.env.SMOKE_PASSWORD || 'InvoxaDemo!2026';

const jar = new Map();
function cookieHeader() {
  return [...jar.entries()].map(([k, v]) => `${k}=${v}`).join('; ');
}
function storeCookies(response) {
  for (const cookie of response.headers.getSetCookie?.() || []) {
    const [pair] = cookie.split(';');
    const index = pair.indexOf('=');
    if (index > 0) jar.set(pair.slice(0, index), pair.slice(index + 1));
  }
}
async function call(path, { method = 'GET', body, headers = {}, form } = {}) {
  const init = { method, headers: { ...headers }, redirect: 'manual' };
  if (jar.size) init.headers.cookie = cookieHeader();
  if (body !== undefined) {
    init.headers['content-type'] = 'application/json';
    init.body = JSON.stringify(body);
  }
  if (form) init.body = form;
  if (!['GET', 'HEAD', 'OPTIONS'].includes(method) && jar.has('invoxa_csrf')) {
    init.headers['x-csrf-token'] = jar.get('invoxa_csrf');
  }
  const response = await fetch(base + path, init);
  storeCookies(response);
  const text = await response.text();
  let data = null;
  try { data = text ? JSON.parse(text) : null; } catch { data = text; }
  if (!response.ok) {
    const error = new Error(`${method} ${path} -> ${response.status}: ${text.slice(0, 800)}`);
    error.status = response.status;
    error.data = data;
    throw error;
  }
  return { response, data };
}
async function upload(path) {
  const bytes = await readFile(path);
  const form = new FormData();
  form.append('files', new Blob([bytes], { type: path.endsWith('.pdf') ? 'application/pdf' : 'image/png' }), basename(path));
  return call('/api/v1/documents', { method: 'POST', form });
}
async function waitForInvoice(documentId) {
  for (let attempt = 0; attempt < 40; attempt += 1) {
    const { data } = await call(`/api/v1/documents/${documentId}`);
    if (data.status === 'failed') {
      throw new Error(`processing failed: ${JSON.stringify(data.error_code)} ${data.error_message}`);
    }
    if (data.invoice?.id) return data.invoice.id;
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  throw new Error('timed out waiting for extraction');
}

await call('/api/v1/auth/login', { method: 'POST', body: { email, password } });
console.log('login: ok');
const { data: session } = await call('/api/v1/auth/session');
console.log('session:', session.user.email, session.active_organization.organization.name);

const { data: uploaded } = await upload('fixtures/acme_invoice.pdf');
const document = uploaded.documents[0]?.document;
if (!document) throw new Error(`upload failed: ${JSON.stringify(uploaded)}`);
console.log('upload:', document.id, document.filename);
const invoiceId = await waitForInvoice(document.id);
const { data: documentDetail } = await call(`/api/v1/documents/${document.id}`);
const jobId = documentDetail.jobs?.[0]?.id;
if (!jobId) throw new Error('document has no processing job');
const { data: job } = await call(`/api/v1/jobs/${jobId}`);
if (job.status !== 'succeeded') throw new Error(`job lookup returned ${job.status}`);
console.log('job lookup:', job.id, job.status, job.stage);
const { data: detail } = await call(`/api/v1/invoices/${invoiceId}`);
console.log('invoice:', detail.invoice_number, detail.vendor_name, detail.total, detail.status);
console.log('mock:', detail.extraction.is_mock, 'issues:', detail.issues.length, 'duplicates:', detail.duplicates.length);
const taxMismatch = detail.issues.find((issue) => issue.code === 'TAX_TOTAL_MISMATCH');
if (taxMismatch) throw new Error(`tax validation regression: ${taxMismatch.message}`);

for (const duplicate of detail.duplicates.filter((row) => row.status === 'open')) {
  await call(`/api/v1/invoices/${invoiceId}/duplicates/${duplicate.id}/resolve`, {
    method: 'POST',
    body: { status: 'dismissed', note: 'Smoke test reviewed the duplicate candidate.' },
  });
}
const { data: rejected } = await call(`/api/v1/invoices/${invoiceId}/reject`, {
  method: 'POST',
  body: { version: detail.version, reason: 'Smoke test exercises the return-for-correction workflow.' },
});
if (rejected.status !== 'rejected') throw new Error(`rejection failed: ${rejected.status}`);
const { data: returned } = await call(`/api/v1/invoices/${invoiceId}/return`, {
  method: 'POST',
  body: { version: rejected.version, reason: 'Smoke test requested correction after the rejection.' },
});
if (returned.status !== 'review_required') throw new Error(`return for correction failed: ${returned.status}`);
console.log('return for correction:', rejected.status, '->', returned.status);

const { data: approved } = await call(`/api/v1/invoices/${invoiceId}/approve`, {
  method: 'POST',
  body: { version: returned.version, acknowledge_mock: true, reason: 'Automated smoke test after explicit mock acknowledgement.' },
});
if (approved.status !== 'approved') throw new Error(`approval failed: ${approved.status}`);
console.log('approved:', approved.status, 'version:', approved.version);

const { data: exportJob } = await call('/api/v1/exports', {
  method: 'POST',
  body: { format: 'json', filters: {} },
});
let exported = exportJob;
for (let attempt = 0; attempt < 40 && exported.status !== 'succeeded'; attempt += 1) {
  await new Promise((resolve) => setTimeout(resolve, 500));
  exported = (await call(`/api/v1/exports/${exportJob.id}`)).data;
}
if (exported.status !== 'succeeded') throw new Error(`export did not finish: ${JSON.stringify(exported)}`);
const download = await call(`/api/v1/exports/${exportJob.id}/download`);
const rows = Array.isArray(download.data) ? download.data : download.data?.invoices;
if (!Array.isArray(rows) || !rows.some((row) => row.invoice_id === invoiceId)) {
  throw new Error('export artifact does not contain the approved invoice');
}
console.log('export:', exported.id, exported.format, exported.row_count, 'rows');

let rejectedUnsafeStatus = false;
try {
  await call('/api/v1/exports', {
    method: 'POST',
    body: { format: 'json', filters: { status: 'review_required' } },
  });
} catch (error) {
  rejectedUnsafeStatus = error?.status === 400 || error?.status === 422;
}
if (!rejectedUnsafeStatus) throw new Error('unsafe export status was not rejected');

const { data: reExportJob } = await call('/api/v1/exports', {
  method: 'POST',
  body: { format: 'json', filters: { status: 'exported', invoice_ids: [invoiceId] } },
});
let reExported = reExportJob;
for (let attempt = 0; attempt < 40 && reExported.status !== 'succeeded'; attempt += 1) {
  await new Promise((resolve) => setTimeout(resolve, 500));
  reExported = (await call(`/api/v1/exports/${reExportJob.id}`)).data;
}
if (reExported.status !== 'succeeded') {
  throw new Error(`re-export did not finish: ${JSON.stringify(reExported)}`);
}
const reDownload = await call(`/api/v1/exports/${reExportJob.id}/download`);
const reRows = Array.isArray(reDownload.data) ? reDownload.data : reDownload.data?.invoices;
if (!Array.isArray(reRows) || !reRows.some((row) => row.invoice_id === invoiceId)) {
  throw new Error('re-export artifact does not contain the previously exported invoice');
}
console.log('re-export:', reExported.id, 'status=exported', reExported.row_count, 'rows');

const { data: dashboard } = await call('/api/v1/dashboard');
console.log('dashboard:', JSON.stringify(dashboard.counts));

console.log('SMOKE OK');
