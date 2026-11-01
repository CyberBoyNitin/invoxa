"use client";

import { useQuery } from "@tanstack/react-query";
import { AlertCircle, ArrowUpRight, CheckCircle2, Mail, Plug, Webhook } from "lucide-react";
import Link from "next/link";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle, Skeleton } from "@/components/ui/card";
import { PageHeader } from "@/components/ui/page-header";
import { apiFetch, errorMessage } from "@/lib/api";
import { formatDateTime, titleCase } from "@/lib/format";

interface IntegrationRow {
  provider: string;
  status: string;
  config: Record<string, unknown>;
  last_verified_at: string | null;
  updated_at: string | null;
}
interface IntegrationsResponse {
  configured: IntegrationRow[];
  catalog: Array<{ provider: string; name: string; status: string; description: string }>;
  webhooks_note: string;
}

const icons: Record<string, React.ComponentType<{ className?: string }>> = {
  quickbooks: Plug,
  xero: Plug,
  tally: Plug,
  google_drive: Plug,
  email: Mail,
};

export default function IntegrationsPage() {
  const query = useQuery({
    queryKey: ["integrations"],
    queryFn: () => apiFetch<IntegrationsResponse>("/integrations"),
  });

  return (
    <div className="space-y-6">
      <PageHeader
        eyebrow="Connections"
        title="Integrations"
        description="Direct accounting sync and email ingestion are not implemented in this build. This page reports the real server capability rather than simulating a connection."
        actions={<Button asChild variant="outline"><Link href="/settings/api-keys"><Webhook className="size-4" /> API & webhooks</Link></Button>}
      />

      <div className="rounded-lg border border-amber/25 bg-amber-soft px-4 py-3 text-xs leading-5 text-amber">
        <strong>No OAuth connector is active.</strong> CSV and JSON exports are fully functional. Webhooks deliver signed events. Do not expect invoices to sync to an accounting package.
      </div>

      {query.isLoading ? (
        <div className="grid gap-4 md:grid-cols-2">{Array.from({ length: 4 }).map((_, index) => <Skeleton key={index} className="h-40" />)}</div>
      ) : query.isError ? (
        <div className="rounded-lg border border-rust/25 bg-rust-soft p-5 text-sm text-rust">{errorMessage(query.error)}</div>
      ) : (
        <>
          {query.data?.configured.length ? (
            <Card>
              <CardHeader><CardTitle>Configured providers</CardTitle></CardHeader>
              <CardContent className="space-y-3">
                {query.data.configured.map((integration) => (
                  <div key={integration.provider} className="flex items-center justify-between rounded-md border border-line px-3.5 py-3">
                    <div><p className="text-sm font-semibold">{titleCase(integration.provider)}</p><p className="mt-0.5 text-[11px] text-muted">Last verified {formatDateTime(integration.last_verified_at)}</p></div>
                    <Badge tone={integration.status === "connected" ? "moss" : "neutral"}>{titleCase(integration.status)}</Badge>
                  </div>
                ))}
              </CardContent>
            </Card>
          ) : null}

          <div className="grid gap-4 md:grid-cols-2">
            {query.data?.catalog.map((item) => {
              const Icon = icons[item.provider] ?? Plug;
              const unavailable = item.status === "not_implemented";
              return (
                <Card key={item.provider}>
                  <CardHeader>
                    <div className="flex items-center gap-3">
                      <span className="grid size-9 place-items-center rounded-md border border-line bg-paper text-ink-soft"><Icon className="size-4" /></span>
                      <div><CardTitle>{item.name}</CardTitle><p className="mt-0.5 text-[10px] uppercase tracking-[0.12em] text-muted">{titleCase(item.status)}</p></div>
                    </div>
                    <Badge tone={unavailable ? "amber" : "neutral"}>{unavailable ? "Not available" : titleCase(item.status)}</Badge>
                  </CardHeader>
                  <CardContent>
                    <p className="text-xs leading-5 text-muted">{item.description}</p>
                    <div className="mt-4 flex items-center gap-2">
                      {unavailable ? <><AlertCircle className="size-3.5 text-amber" /><span className="text-[11px] text-amber">Requires implementation and provider credentials</span></> : <><CheckCircle2 className="size-3.5 text-moss" /><span className="text-[11px] text-moss">Available</span></>}
                    </div>
                  </CardContent>
                </Card>
              );
            })}
          </div>
        </>
      )}

      <Card>
        <CardHeader><CardTitle>Working alternatives</CardTitle><ArrowUpRight className="size-4 text-muted" /></CardHeader>
        <CardContent className="grid gap-3 sm:grid-cols-3">
          <Link href="/exports" className="rounded-md border border-line bg-paper/35 p-3 hover:border-line-strong"><p className="text-xs font-semibold">CSV / JSON export</p><p className="mt-1 text-[11px] leading-4 text-muted">Approved invoices only, with exact money values and source metadata.</p></Link>
          <Link href="/settings/api-keys" className="rounded-md border border-line bg-paper/35 p-3 hover:border-line-strong"><p className="text-xs font-semibold">Signed webhooks</p><p className="mt-1 text-[11px] leading-4 text-muted">HMAC-signed, retried event delivery with a delivery history.</p></Link>
          <Link href="/settings/api-keys" className="rounded-md border border-line bg-paper/35 p-3 hover:border-line-strong"><p className="text-xs font-semibold">Scoped API keys</p><p className="mt-1 text-[11px] leading-4 text-muted">Build a direct integration against the same tenant-scoped API used by this UI.</p></Link>
        </CardContent>
      </Card>
    </div>
  );
}
