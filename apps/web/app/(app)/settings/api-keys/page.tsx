"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  Check,
  Copy,
  KeyRound,
  Plus,
  RefreshCw,
  RotateCw,
  ShieldCheck,
  Trash2,
  Webhook,
} from "lucide-react";
import * as React from "react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle, Skeleton } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter } from "@/components/ui/dialog";
import { Checkbox, Field, Input, Select, Textarea } from "@/components/ui/input";
import { Table, TBody, TD, TH, THead, TR } from "@/components/ui/table";
import { apiFetch, errorMessage } from "@/lib/api";
import { formatDateTime, titleCase } from "@/lib/format";
import type { ApiKeyRow, WebhookDelivery, WebhookEndpoint } from "@/lib/types";

interface KeysResponse {
  items: ApiKeyRow[];
  available_scopes: string[];
  warning: string;
}
interface KeyWithSecret extends ApiKeyRow {
  secret: string;
  secret_notice: string;
}
interface WebhooksResponse {
  items: WebhookEndpoint[];
  available_events: string[];
  note: string;
}
interface WebhookWithSecret extends WebhookEndpoint {
  secret: string;
  secret_notice: string;
}
interface DeliveriesResponse {
  items: WebhookDelivery[];
  limit: number;
}

type SecretReveal = { title: string; secret: string; notice: string } | null;

export default function ApiKeysPage() {
  const queryClient = useQueryClient();
  const [keyOpen, setKeyOpen] = React.useState(false);
  const [keyName, setKeyName] = React.useState("");
  const [keyScopes, setKeyScopes] = React.useState<string[]>(["invoices:read"]);
  const [keyDays, setKeyDays] = React.useState(90);
  const [webhookOpen, setWebhookOpen] = React.useState(false);
  const [editingWebhook, setEditingWebhook] = React.useState<WebhookEndpoint | null>(null);
  const [webhookUrl, setWebhookUrl] = React.useState("");
  const [webhookDescription, setWebhookDescription] = React.useState("");
  const [webhookEvents, setWebhookEvents] = React.useState<string[]>(["invoice.approved"]);
  const [webhookEnabled, setWebhookEnabled] = React.useState(true);
  const [busy, setBusy] = React.useState(false);
  const [reveal, setReveal] = React.useState<SecretReveal>(null);
  const [revokeTarget, setRevokeTarget] = React.useState<ApiKeyRow | null>(null);
  const [deleteTarget, setDeleteTarget] = React.useState<WebhookEndpoint | null>(null);
  const [rotateTarget, setRotateTarget] = React.useState<WebhookEndpoint | null>(null);

  const keys = useQuery({
    queryKey: ["api-keys"],
    queryFn: () => apiFetch<KeysResponse>("/api-keys"),
    retry: false,
  });
  const webhooks = useQuery({
    queryKey: ["webhooks"],
    queryFn: () => apiFetch<WebhooksResponse>("/webhooks"),
    retry: false,
  });
  const deliveries = useQuery({
    queryKey: ["webhook-deliveries"],
    queryFn: () => apiFetch<DeliveriesResponse>("/webhook-deliveries?limit=50"),
    retry: false,
    refetchInterval: 15_000,
  });

  function toggleScope(scope: string) {
    setKeyScopes((current) => current.includes(scope) ? current.filter((item) => item !== scope) : [...current, scope]);
  }
  function toggleEvent(eventName: string) {
    setWebhookEvents((current) => current.includes(eventName) ? current.filter((item) => item !== eventName) : [...current, eventName]);
  }

  async function createKey(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    try {
      const created = await apiFetch<KeyWithSecret>("/api-keys", {
        method: "POST",
        body: { name: keyName.trim(), scopes: keyScopes, expires_in_days: keyDays },
      });
      setKeyOpen(false);
      setKeyName("");
      setKeyScopes(["invoices:read"]);
      setReveal({ title: `API key “${created.name}” created`, secret: created.secret, notice: created.secret_notice });
      await queryClient.invalidateQueries({ queryKey: ["api-keys"] });
    } catch (caught) {
      toast.error(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  }

  async function revokeKey() {
    if (!revokeTarget) return;
    setBusy(true);
    try {
      await apiFetch(`/api-keys/${revokeTarget.id}`, { method: "DELETE" });
      setRevokeTarget(null);
      await queryClient.invalidateQueries({ queryKey: ["api-keys"] });
      toast.success("API key revoked.");
    } catch (caught) {
      toast.error(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  }

  function openCreateWebhook() {
    setEditingWebhook(null);
    setWebhookUrl("");
    setWebhookDescription("");
    setWebhookEvents(["invoice.approved"]);
    setWebhookEnabled(true);
    setWebhookOpen(true);
  }

  function openEditWebhook(endpoint: WebhookEndpoint) {
    setEditingWebhook(endpoint);
    setWebhookUrl(endpoint.url);
    setWebhookDescription(endpoint.description ?? "");
    setWebhookEvents(endpoint.events);
    setWebhookEnabled(endpoint.enabled);
    setWebhookOpen(true);
  }

  async function saveWebhook(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    const body = { url: webhookUrl.trim(), description: webhookDescription.trim() || null, events: webhookEvents, enabled: webhookEnabled };
    try {
      if (editingWebhook) {
        await apiFetch(`/webhooks/${editingWebhook.id}`, { method: "PATCH", body });
        toast.success("Webhook endpoint updated.");
      } else {
        const created = await apiFetch<WebhookWithSecret>("/webhooks", { method: "POST", body });
        setReveal({ title: `Webhook “${created.url}” created`, secret: created.secret, notice: created.secret_notice });
      }
      setWebhookOpen(false);
      await queryClient.invalidateQueries({ queryKey: ["webhooks"] });
    } catch (caught) {
      toast.error(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  }

  async function deleteWebhook() {
    if (!deleteTarget) return;
    setBusy(true);
    try {
      await apiFetch(`/webhooks/${deleteTarget.id}`, { method: "DELETE" });
      setDeleteTarget(null);
      await queryClient.invalidateQueries({ queryKey: ["webhooks"] });
      await queryClient.invalidateQueries({ queryKey: ["webhook-deliveries"] });
      toast.success("Webhook endpoint deleted.");
    } catch (caught) {
      toast.error(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  }

  async function rotateSecret() {
    if (!rotateTarget) return;
    setBusy(true);
    try {
      const rotated = await apiFetch<{ id: string; secret: string; secret_notice: string }>(`/webhooks/${rotateTarget.id}/rotate-secret`, { method: "POST" });
      setRotateTarget(null);
      setReveal({ title: `Secret rotated for ${rotateTarget.url}`, secret: rotated.secret, notice: rotated.secret_notice });
      await queryClient.invalidateQueries({ queryKey: ["webhooks"] });
    } catch (caught) {
      toast.error(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  }

  async function redeliver(delivery: WebhookDelivery) {
    setBusy(true);
    try {
      await apiFetch(`/webhook-deliveries/${delivery.id}/redeliver`, { method: "POST" });
      await queryClient.invalidateQueries({ queryKey: ["webhook-deliveries"] });
      toast.success("Webhook delivery requeued with the original event id.");
    } catch (caught) {
      toast.error(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  }

  async function copySecret() {
    if (!reveal) return;
    try {
      await navigator.clipboard.writeText(reveal.secret);
      toast.success("Secret copied. Store it in your secret manager.");
    } catch {
      toast.error("Clipboard unavailable. Select the secret manually.");
    }
  }

  return (
    <div className="space-y-6">
      <div className="rounded-lg border border-amber/25 bg-amber-soft/70 px-4 py-3 text-[11px] leading-5 text-amber">
        <strong>Secrets are shown once.</strong> API key hashes and encrypted webhook secrets cannot be recovered. Rotate or recreate a credential if it is lost.
      </div>

      <Card>
        <CardHeader>
          <div><CardTitle>API keys</CardTitle><p className="mt-1 text-xs text-muted">High-entropy bearer tokens with explicit scopes and a fixed expiry. Keys are hashed at rest and rate-limited per principal.</p></div>
          <Button onClick={() => setKeyOpen(true)}><Plus className="size-4" /> Create key</Button>
        </CardHeader>
        <CardContent className="px-0 pb-0">
          {keys.isLoading ? (
            <div className="space-y-2 px-5 pb-5">{Array.from({ length: 3 }).map((_, index) => <Skeleton key={index} className="h-12" />)}</div>
          ) : keys.isError ? (
            <div className="flex items-start gap-2 p-5 text-sm text-rust"><AlertTriangle className="mt-0.5 size-4" /> {errorMessage(keys.error)}</div>
          ) : keys.data?.items.length ? (
            <Table>
              <THead><tr><TH>Key</TH><TH>Scopes</TH><TH>Last used</TH><TH>Expires</TH><TH>Status</TH><TH className="text-right">Action</TH></tr></THead>
              <TBody>
                {keys.data.items.map((key) => (
                  <TR key={key.id}>
                    <TD><p className="text-xs font-semibold">{key.name}</p><p className="mt-0.5 font-mono text-[10px] text-muted">{key.prefix}…</p><p className="text-[10px] text-muted">Created by {key.created_by_name ?? "—"}</p></TD>
                    <TD><div className="flex max-w-[360px] flex-wrap gap-1">{key.scopes.map((scope) => <span key={scope} className="rounded bg-paper-deep px-1.5 py-0.5 font-mono text-[9px] text-ink-soft">{scope}</span>)}</div></TD>
                    <TD className="whitespace-nowrap text-[11px] text-muted">{key.last_used_at ? formatDateTime(key.last_used_at) : "Never"}</TD>
                    <TD className="whitespace-nowrap text-[11px] text-muted">{formatDateTime(key.expires_at)}</TD>
                    <TD><Badge tone={key.revoked_at ? "rust" : key.expires_at && new Date(key.expires_at) < new Date() ? "amber" : "moss"}>{key.revoked_at ? "Revoked" : key.expires_at && new Date(key.expires_at) < new Date() ? "Expired" : "Active"}</Badge></TD>
                    <TD className="text-right">{key.revoked_at ? <span className="text-[10px] text-muted">—</span> : <Button size="sm" variant="ghost" className="text-rust hover:text-rust" onClick={() => setRevokeTarget(key)}><Trash2 className="size-3.5" /> Revoke</Button>}</TD>
                  </TR>
                ))}
              </TBody>
            </Table>
          ) : <div className="px-5 pb-5 text-sm text-muted">No API keys yet.</div>}
          <div className="border-t border-line bg-paper/30 px-5 py-3 text-[11px] leading-5 text-muted">{keys.data?.warning ?? "Use a bearer token in the Authorization header."}</div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <div><CardTitle>Webhook endpoints</CardTitle><p className="mt-1 text-xs text-muted">Durable outbox delivery with HMAC signatures, timestamp replay protection, exponential retries, and delivery history.</p></div>
          <Button onClick={openCreateWebhook}><Plus className="size-4" /> Add endpoint</Button>
        </CardHeader>
        <CardContent className="px-0 pb-0">
          {webhooks.isLoading ? (
            <div className="space-y-2 px-5 pb-5">{Array.from({ length: 3 }).map((_, index) => <Skeleton key={index} className="h-12" />)}</div>
          ) : webhooks.isError ? (
            <div className="flex items-start gap-2 p-5 text-sm text-rust"><AlertTriangle className="mt-0.5 size-4" /> {errorMessage(webhooks.error)}</div>
          ) : webhooks.data?.items.length ? (
            <div className="divide-y divide-line">
              {webhooks.data.items.map((endpoint) => (
                <div key={endpoint.id} className="flex flex-col gap-3 px-5 py-4 sm:flex-row sm:items-start sm:justify-between">
                  <div className="min-w-0">
                    <div className="flex flex-wrap items-center gap-2">
                      <Webhook className="size-4 text-indigo" />
                      <p className="max-w-[560px] truncate font-mono text-xs font-semibold">{endpoint.url}</p>
                      <Badge tone={endpoint.enabled ? "moss" : "neutral"}>{endpoint.enabled ? "Enabled" : "Disabled"}</Badge>
                      {endpoint.failed_count ? <Badge tone="rust">{endpoint.failed_count} failed</Badge> : null}
                    </div>
                    {endpoint.description ? <p className="mt-1 text-[11px] text-muted">{endpoint.description}</p> : null}
                    <div className="mt-2 flex flex-wrap gap-1">{endpoint.events.map((eventName) => <span key={eventName} className="rounded bg-paper-deep px-1.5 py-0.5 font-mono text-[9px] text-ink-soft">{eventName}</span>)}</div>
                    <p className="mt-2 text-[10px] text-muted">{endpoint.delivery_count} deliveries · last success {endpoint.last_success_at ? formatDateTime(endpoint.last_success_at) : "never"} · last failure {endpoint.last_failure_at ? formatDateTime(endpoint.last_failure_at) : "never"}</p>
                  </div>
                  <div className="flex shrink-0 flex-wrap gap-1">
                    <Button size="sm" variant="outline" onClick={() => openEditWebhook(endpoint)}>Edit</Button>
                    <Button size="sm" variant="outline" onClick={() => setRotateTarget(endpoint)}><RotateCw className="size-3.5" /> Rotate</Button>
                    <Button size="sm" variant="ghost" className="text-rust hover:text-rust" onClick={() => setDeleteTarget(endpoint)}><Trash2 className="size-3.5" /></Button>
                  </div>
                </div>
              ))}
            </div>
          ) : <div className="px-5 pb-5 text-sm text-muted">No webhook endpoints configured.</div>}
          <div className="border-t border-line bg-paper/30 px-5 py-3 text-[11px] leading-5 text-muted">{webhooks.data?.note ?? "HTTPS on port 443 is required. Private and loopback destinations are rejected."}</div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader><div><CardTitle>Recent webhook deliveries</CardTitle><p className="mt-1 text-xs text-muted">Last 50 delivery attempts across all endpoints. Refresh to see worker progress.</p></div><Button size="sm" variant="outline" onClick={() => void deliveries.refetch()}><RefreshCw className={`size-3.5 ${deliveries.isFetching ? "animate-spin" : ""}`} /> Refresh</Button></CardHeader>
        <CardContent className="px-0 pb-0">
          {deliveries.isLoading ? <div className="space-y-2 px-5 pb-5"><Skeleton className="h-12" /><Skeleton className="h-12" /></div> : deliveries.isError ? <div className="p-5 text-sm text-rust">{errorMessage(deliveries.error)}</div> : deliveries.data?.items.length ? (
            <Table>
              <THead><tr><TH>Event</TH><TH>Endpoint</TH><TH>Status</TH><TH>Attempts</TH><TH>HTTP</TH><TH>Last error</TH><TH>Next attempt</TH><TH className="text-right">Action</TH></tr></THead>
              <TBody>
                {deliveries.data.items.map((delivery) => (
                  <TR key={delivery.id}>
                    <TD className="whitespace-nowrap font-mono text-[10px]">{delivery.event_type}</TD>
                    <TD><p className="max-w-[220px] truncate font-mono text-[10px] text-muted">{delivery.endpoint_url}</p></TD>
                    <TD><Badge tone={delivery.status === "succeeded" ? "moss" : delivery.status === "failed" || delivery.status === "dead_letter" ? "rust" : "indigo"}>{titleCase(delivery.status)}</Badge></TD>
                    <TD className="tabular text-xs">{delivery.attempts}</TD>
                    <TD className="tabular text-xs">{delivery.last_status_code ?? "—"}</TD>
                    <TD><p className="max-w-[220px] truncate text-[10px] text-muted" title={delivery.last_error ?? undefined}>{delivery.last_error ?? "—"}</p></TD>
                    <TD className="whitespace-nowrap text-[10px] text-muted">{delivery.next_attempt_at ? formatDateTime(delivery.next_attempt_at) : delivery.delivered_at ? formatDateTime(delivery.delivered_at) : "—"}</TD>
                    <TD className="text-right">
                      {["failed", "dead_letter", "succeeded"].includes(delivery.status) ? (
                        <Button size="sm" variant="ghost" onClick={() => void redeliver(delivery)} disabled={busy} title="Requeue this delivery">
                          <RotateCw className="size-3.5" /> Redeliver
                        </Button>
                      ) : <span className="text-[10px] text-muted">In progress</span>}
                    </TD>
                  </TR>
                ))}
              </TBody>
            </Table>
          ) : <div className="px-5 pb-5 text-sm text-muted">No deliveries recorded. Events are queued when subscribed events occur.</div>}
        </CardContent>
      </Card>

      <Dialog open={keyOpen} onOpenChange={setKeyOpen}>
        <DialogContent title="Create API key" description="Choose only the scopes this integration needs. The secret is displayed once and stored only as a SHA-256 hash.">
          <form onSubmit={createKey}>
            <div className="space-y-4 px-6 py-5">
              <div className="grid gap-4 sm:grid-cols-[1fr_130px]">
                <Field label="Key name" htmlFor="key-name"><Input id="key-name" required maxLength={100} value={keyName} onChange={(event) => setKeyName(event.target.value)} placeholder="Accounting sync" /></Field>
                <Field label="Expires in" htmlFor="key-days" hint="1–365 days"><Input id="key-days" type="number" min={1} max={365} value={keyDays} onChange={(event) => setKeyDays(Number(event.target.value))} /></Field>
              </div>
              <div>
                <p className="mb-2 text-xs font-semibold">Scopes <span className="font-normal text-muted">({keyScopes.length} selected)</span></p>
                <div className="grid max-h-64 grid-cols-1 gap-1.5 overflow-y-auto rounded-md border border-line p-3 sm:grid-cols-2">
                  {keys.data?.available_scopes.map((scope) => (
                    <label key={scope} className="flex items-center gap-2 rounded px-1.5 py-1 text-[11px] hover:bg-paper"><Checkbox checked={keyScopes.includes(scope)} onChange={() => toggleScope(scope)} /> <code className="font-mono">{scope}</code></label>
                  ))}
                </div>
              </div>
              <div className="flex items-start gap-2 rounded-md border border-line bg-paper/30 px-3 py-2.5 text-[11px] leading-5 text-muted"><ShieldCheck className="mt-0.5 size-3.5 shrink-0 text-moss" /> Keys with api_keys:write still cannot mint new keys. Only a signed-in user can create or revoke credentials.</div>
            </div>
            <DialogFooter><Button type="button" variant="ghost" onClick={() => setKeyOpen(false)}>Cancel</Button><Button type="submit" loading={busy} disabled={!keyScopes.length}><KeyRound className="size-4" /> Create key</Button></DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <Dialog open={webhookOpen} onOpenChange={setWebhookOpen}>
        <DialogContent title={editingWebhook ? "Edit webhook endpoint" : "Add webhook endpoint"} description="Only public HTTPS destinations on port 443 are accepted. Redirects are not followed and private addresses are blocked at delivery time.">
          <form onSubmit={saveWebhook}>
            <div className="space-y-4 px-6 py-5">
              <Field label="Endpoint URL" htmlFor="webhook-url" hint="https://…"><Input id="webhook-url" type="url" required value={webhookUrl} onChange={(event) => setWebhookUrl(event.target.value)} placeholder="https://hooks.example.com/invoxa" /></Field>
              <Field label="Description" htmlFor="webhook-description"><Textarea id="webhook-description" className="min-h-16" maxLength={300} value={webhookDescription} onChange={(event) => setWebhookDescription(event.target.value)} placeholder="Production accounting sync" /></Field>
              <div>
                <p className="mb-2 text-xs font-semibold">Events <span className="font-normal text-muted">({webhookEvents.length} selected)</span></p>
                <div className="grid max-h-60 grid-cols-1 gap-1.5 overflow-y-auto rounded-md border border-line p-3 sm:grid-cols-2">
                  {webhooks.data?.available_events.map((eventName) => (
                    <label key={eventName} className="flex items-center gap-2 rounded px-1.5 py-1 text-[11px] hover:bg-paper"><Checkbox checked={webhookEvents.includes(eventName)} onChange={() => toggleEvent(eventName)} /> <code className="font-mono">{eventName}</code></label>
                  ))}
                </div>
              </div>
              <label className="flex items-center gap-2 text-xs"><Checkbox checked={webhookEnabled} onChange={(event) => setWebhookEnabled(event.target.checked)} /> Endpoint enabled</label>
            </div>
            <DialogFooter><Button type="button" variant="ghost" onClick={() => setWebhookOpen(false)}>Cancel</Button><Button type="submit" loading={busy} disabled={!webhookEvents.length}>{editingWebhook ? "Save endpoint" : "Create endpoint"}</Button></DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <Dialog open={Boolean(reveal)} onOpenChange={(open) => !open && setReveal(null)}>
        <DialogContent title={reveal?.title ?? "Secret"} description={reveal?.notice}>
          <div className="space-y-3 px-6 py-5">
            <Field label="Secret — copy now" htmlFor="revealed-secret">
              <div className="flex gap-2"><Input id="revealed-secret" readOnly value={reveal?.secret ?? ""} className="font-mono text-xs" onFocus={(event) => event.target.select()} /><Button type="button" variant="outline" onClick={() => void copySecret()}><Copy className="size-4" /> Copy</Button></div>
            </Field>
            <div className="flex items-start gap-2 rounded-md border border-amber/25 bg-amber-soft px-3 py-2.5 text-[11px] leading-5 text-amber"><AlertTriangle className="mt-0.5 size-3.5 shrink-0" /> This value will not be shown again. Do not paste it into chat, tickets, or source control.</div>
          </div>
          <DialogFooter><Button onClick={() => setReveal(null)}><Check className="size-4" /> I stored the secret</Button></DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={Boolean(revokeTarget)} onOpenChange={(open) => !open && setRevokeTarget(null)}>
        <DialogContent title="Revoke this API key?" description="Requests using this key will fail immediately. This cannot be undone.">
          <div className="px-6 py-5 text-sm"><p className="font-semibold">{revokeTarget?.name}</p><p className="font-mono text-xs text-muted">{revokeTarget?.prefix}…</p></div>
          <DialogFooter><Button variant="ghost" onClick={() => setRevokeTarget(null)}>Cancel</Button><Button variant="danger" loading={busy} onClick={revokeKey}>Revoke key</Button></DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={Boolean(deleteTarget)} onOpenChange={(open) => !open && setDeleteTarget(null)}>
        <DialogContent title="Delete this webhook endpoint?" description="Pending deliveries for this endpoint will stop. Delivery history may be retained for audit.">
          <div className="px-6 py-5 text-sm"><p className="break-all font-mono text-xs">{deleteTarget?.url}</p></div>
          <DialogFooter><Button variant="ghost" onClick={() => setDeleteTarget(null)}>Cancel</Button><Button variant="danger" loading={busy} onClick={deleteWebhook}>Delete endpoint</Button></DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={Boolean(rotateTarget)} onOpenChange={(open) => !open && setRotateTarget(null)}>
        <DialogContent title="Rotate signing secret?" description="The previous secret becomes invalid immediately. Update your verifier before the next delivery.">
          <div className="px-6 py-5 text-sm"><p className="break-all font-mono text-xs">{rotateTarget?.url}</p></div>
          <DialogFooter><Button variant="ghost" onClick={() => setRotateTarget(null)}>Cancel</Button><Button loading={busy} onClick={rotateSecret}><RotateCw className="size-4" /> Rotate secret</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
