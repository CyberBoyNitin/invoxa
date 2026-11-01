"use client";

import { useQuery } from "@tanstack/react-query";
import { Activity, KeyRound, Search, ShieldCheck, User } from "lucide-react";
import * as React from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { EmptyState, Skeleton } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { PageHeader } from "@/components/ui/page-header";
import { Table, TBody, TD, TH, THead, TR } from "@/components/ui/table";
import { apiFetch, errorMessage } from "@/lib/api";
import { formatDateTime, titleCase } from "@/lib/format";
import type { AuditEvent } from "@/lib/types";

interface ActivityResponse {
  events: AuditEvent[];
  total: number;
  limit: number;
  offset: number;
}

export default function ActivityPage() {
  const [search, setSearch] = React.useState("");
  const [debounced, setDebounced] = React.useState("");
  const [offset, setOffset] = React.useState(0);
  const limit = 50;

  React.useEffect(() => {
    const timer = window.setTimeout(() => {
      setDebounced(search.trim());
      setOffset(0);
    }, 280);
    return () => window.clearTimeout(timer);
  }, [search]);

  const query = useQuery({
    queryKey: ["activity", debounced, offset],
    queryFn: () => {
      const params = new URLSearchParams({ limit: String(limit), offset: String(offset) });
      if (debounced) params.set("search", debounced);
      return apiFetch<ActivityResponse>(`/activity?${params.toString()}`);
    },
    placeholderData: (previous) => previous,
  });

  return (
    <div className="space-y-6">
      <PageHeader
        eyebrow="Immutable audit trail"
        title="Activity log"
        description="Every security-relevant action in this organization, with actor, request correlation, and structured metadata. Entries are append-only."
        actions={<Badge tone="moss"><ShieldCheck className="size-3" /> Tenant scoped</Badge>}
      />

      <div className="relative rounded-lg border border-line bg-surface p-3">
        <Search className="pointer-events-none absolute left-6 top-1/2 size-4 -translate-y-1/2 text-muted" />
        <Input className="pl-9" placeholder="Search action, actor, or entity ID…" value={search} onChange={(event) => setSearch(event.target.value)} aria-label="Search activity" />
      </div>

      <div className="overflow-hidden rounded-lg border border-line bg-surface">
        {query.isLoading ? (
          <div className="space-y-2 p-4">{Array.from({ length: 8 }).map((_, index) => <Skeleton key={index} className="h-12" />)}</div>
        ) : query.isError ? (
          <div className="p-5 text-sm text-rust">{errorMessage(query.error)}</div>
        ) : query.data?.events.length ? (
          <Table>
            <THead><tr><TH>Action</TH><TH>Actor</TH><TH>Resource</TH><TH>Source</TH><TH>When</TH><TH>Metadata</TH></tr></THead>
            <TBody>
              {query.data.events.map((event) => (
                <TR key={event.id} className="align-top">
                  <TD>
                    <div className="flex items-center gap-2">
                      <span className={`grid size-6 place-items-center rounded ${event.action.startsWith("api_key") ? "bg-indigo-soft text-indigo" : event.action.startsWith("webhook") ? "bg-amber-soft text-amber" : "bg-paper-deep text-ink-soft"}`}>
                        {event.actor_key_name ? <KeyRound className="size-3" /> : <Activity className="size-3" />}
                      </span>
                      <code className="font-mono text-[10px] font-semibold text-ink">{event.action}</code>
                    </div>
                  </TD>
                  <TD>
                    <p className="flex items-center gap-1.5 text-xs font-medium"><User className="size-3 text-muted" /> {event.actor_name ?? "System"}</p>
                    <p className="mt-0.5 text-[10px] text-muted">{event.actor_key_name ? `API key ${event.actor_key_name}` : event.actor_email ?? "—"}</p>
                  </TD>
                  <TD><p className="text-[10px] text-muted">{event.entity_type ?? "—"}</p><p className="mt-0.5 font-mono text-[9px] text-ink-soft">{event.entity_id ? `${event.entity_id.slice(0, 8)}…` : "—"}</p></TD>
                  <TD><p className="font-mono text-[10px] text-muted">{event.ip ?? "—"}</p><p className="mt-0.5 max-w-[120px] truncate font-mono text-[9px] text-muted/70" title={event.request_id ?? undefined}>{event.request_id ?? ""}</p></TD>
                  <TD className="whitespace-nowrap text-[11px] text-muted">{formatDateTime(event.created_at)}</TD>
                  <TD>
                    {Object.keys(event.metadata ?? {}).length ? (
                      <details className="max-w-[260px]">
                        <summary className="cursor-pointer text-[10px] font-semibold text-indigo">View JSON</summary>
                        <pre className="mt-1.5 max-h-40 overflow-auto rounded border border-line bg-paper p-2 font-mono text-[9px] leading-4 text-ink-soft">{JSON.stringify(event.metadata, null, 2)}</pre>
                      </details>
                    ) : <span className="text-[10px] text-muted">—</span>}
                  </TD>
                </TR>
              ))}
            </TBody>
          </Table>
        ) : (
          <div className="p-5"><EmptyState icon={<Activity className="size-5" />} title={debounced ? "No matching events" : "No activity yet"} description={debounced ? "Try a broader search term." : "Actions such as uploads, edits, approvals, and key changes appear here."} /></div>
        )}
      </div>

      {query.data ? (
        <div className="flex items-center justify-between text-xs text-muted">
          <p>{query.data.total === 0 ? "No events" : `${query.data.offset + 1}–${Math.min(query.data.offset + query.data.events.length, query.data.total)} of ${query.data.total}`}</p>
          <div className="flex gap-2">
            <Button size="sm" variant="outline" disabled={offset === 0} onClick={() => setOffset(Math.max(0, offset - limit))}>Previous</Button>
            <Button size="sm" variant="outline" disabled={offset + limit >= query.data.total} onClick={() => setOffset(offset + limit)}>Next</Button>
          </div>
        </div>
      ) : null}

      <p className="text-[11px] leading-5 text-muted">Audit records are organization-scoped and readable by roles with <code className="font-mono">ViewAuditLog</code>. {query.data ? `Showing ${titleCase("append-only")} entries.` : ""}</p>
    </div>
  );
}
