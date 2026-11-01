"use client";

import { useQuery } from "@tanstack/react-query";
import {
  AlertTriangle,
  ArrowRight,
  CheckCircle2,
  Clock3,
  FileStack,
  Inbox,
  Layers3,
  ReceiptText,
  RefreshCw,
  Upload,
} from "lucide-react";
import Link from "next/link";
import * as React from "react";
import { UploadDialog } from "@/components/upload-dialog";
import { StatusBadge } from "@/components/status-badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle, EmptyState, Skeleton } from "@/components/ui/card";
import { PageHeader } from "@/components/ui/page-header";
import { Select } from "@/components/ui/input";
import { Table, TBody, TD, TH, THead, TR } from "@/components/ui/table";
import { apiFetch, errorMessage } from "@/lib/api";
import { formatDateTime, formatDecimal, formatMoney, relativeTime, titleCase } from "@/lib/format";
import type { DashboardData, InvoiceListRow } from "@/lib/types";

interface InvoiceListResponse {
  items: InvoiceListRow[];
  total: number;
}

function Metric({
  label,
  value,
  note,
  icon: Icon,
  tone = "neutral",
  href,
}: {
  label: string;
  value: string | number;
  note: string;
  icon: React.ComponentType<{ className?: string }>;
  tone?: "neutral" | "amber" | "moss" | "rust" | "indigo";
  href?: string;
}) {
  const toneClass = {
    neutral: "text-ink-soft bg-paper-deep",
    amber: "text-amber bg-amber-soft",
    moss: "text-moss bg-moss-soft",
    rust: "text-rust bg-rust-soft",
    indigo: "text-indigo bg-indigo-soft",
  }[tone];
  const body = (
    <Card className="h-full transition-colors hover:border-line-strong">
      <CardContent className="p-5">
        <div className="flex items-start justify-between gap-3">
          <p className="text-[11px] font-semibold uppercase tracking-[0.1em] text-muted">{label}</p>
          <span className={`grid size-8 shrink-0 place-items-center rounded-md ${toneClass}`}>
            <Icon className="size-4" />
          </span>
        </div>
        <p className="tabular mt-4 font-display text-[2.15rem] leading-none tracking-[-0.03em] text-ink">{value}</p>
        <p className="mt-2 text-[11px] leading-4 text-muted">{note}</p>
      </CardContent>
    </Card>
  );
  return href ? <Link href={href}>{body}</Link> : body;
}

function DailyChart({ data }: { data: DashboardData["daily"] }) {
  const max = Math.max(1, ...data.map((day) => day.documents));
  return (
    <div>
      <div className="flex h-40 items-end gap-[3px] border-b border-line pb-px" role="img" aria-label="Daily documents received and approved">
        {data.map((day) => {
          const height = day.documents === 0 ? 2 : Math.max(6, Math.round((day.documents / max) * 100));
          const approvedHeight = day.approved === 0 ? 0 : Math.max(4, Math.round((day.approved / max) * 100));
          return (
            <div key={day.day} className="group relative flex h-full flex-1 items-end">
              <div className="relative w-full rounded-t-[2px] bg-indigo/80 transition-colors group-hover:bg-indigo" style={{ height: `${height}%` }}>
                {approvedHeight ? (
                  <div className="absolute inset-x-0 bottom-0 rounded-t-[2px] bg-moss" style={{ height: `${Math.min(100, (approvedHeight / height) * 100)}%` }} />
                ) : null}
              </div>
              <span className="pointer-events-none absolute bottom-full left-1/2 z-10 mb-1 hidden -translate-x-1/2 whitespace-nowrap rounded bg-ink px-2 py-1 text-[10px] text-white shadow-lg group-hover:block">
                {day.day}: {day.documents} in, {day.approved} approved
              </span>
            </div>
          );
        })}
      </div>
      <div className="mt-3 flex items-center justify-between text-[10px] text-muted">
        <span>{data[0]?.day ?? ""}</span>
        <div className="flex items-center gap-4">
          <span className="flex items-center gap-1.5"><span className="size-2 rounded-sm bg-indigo/80" /> Received</span>
          <span className="flex items-center gap-1.5"><span className="size-2 rounded-sm bg-moss" /> Approved</span>
        </div>
        <span>{data[data.length - 1]?.day ?? ""}</span>
      </div>
    </div>
  );
}

export default function DashboardPage() {
  const [days, setDays] = React.useState(30);
  const [uploadOpen, setUploadOpen] = React.useState(false);
  const dashboard = useQuery({
    queryKey: ["dashboard", days],
    queryFn: () => apiFetch<DashboardData>(`/dashboard?days=${days}`),
    refetchInterval: 30_000,
  });
  const queue = useQuery({
    queryKey: ["invoices", "review-preview"],
    queryFn: () => apiFetch<InvoiceListResponse>("/invoices?status=review_required&limit=5"),
    refetchInterval: 20_000,
  });

  const counts = dashboard.data?.counts;
  const failureRate =
    counts && counts.documents_received > 0
      ? `${formatDecimal(String((counts.processing_failures / counts.documents_received) * 100), 1)}%`
      : "0%";
  const avgSeconds = counts?.avg_processing_ms ? formatDecimal(String(counts.avg_processing_ms / 1000), 1) : "—";

  return (
    <div className="space-y-7">
      <PageHeader
        eyebrow="Operations"
        title="Invoice operations at a glance"
        description="Live figures from this organization. Nothing here is projected or estimated."
        actions={
          <>
            <Select value={days} onChange={(event) => setDays(Number(event.target.value))} className="w-32" aria-label="Date range">
              <option value={7}>Last 7 days</option>
              <option value={30}>Last 30 days</option>
              <option value={90}>Last 90 days</option>
              <option value={365}>Last year</option>
            </Select>
            <Button variant="outline" onClick={() => void dashboard.refetch()} aria-label="Refresh dashboard">
              <RefreshCw className={`size-4 ${dashboard.isFetching ? "animate-spin" : ""}`} />
            </Button>
            <Button onClick={() => setUploadOpen(true)}>
              <Upload className="size-4" />
              Add documents
            </Button>
          </>
        }
      />

      {dashboard.isError ? (
        <div className="rounded-lg border border-rust/25 bg-rust-soft px-4 py-3 text-sm text-rust">{errorMessage(dashboard.error)}</div>
      ) : null}

      <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
        {dashboard.isLoading || !counts ? (
          Array.from({ length: 4 }).map((_, index) => <Skeleton key={index} className="h-40" />)
        ) : (
          <>
            <Metric label="Awaiting review" value={counts.awaiting_review} note="Extracted invoices that need a human decision." icon={Inbox} tone="amber" href="/review" />
            <Metric label="Documents received" value={counts.documents_received} note={`${counts.pages_processed} pages processed in this period.`} icon={FileStack} tone="indigo" href="/documents" />
            <Metric label="Approved invoices" value={counts.approved_invoices} note={`${counts.exported_invoices} exported after approval.`} icon={CheckCircle2} tone="moss" href="/invoices?status=approved" />
            <Metric label="Processing failure rate" value={failureRate} note={`${counts.processing_failures} failed document${counts.processing_failures === 1 ? "" : "s"} · average ${avgSeconds}s`} icon={AlertTriangle} tone={counts.processing_failures ? "rust" : "neutral"} href="/documents?status=failed" />
          </>
        )}
      </div>

      <div className="grid gap-5 xl:grid-cols-[1.5fr_1fr]">
        <Card>
          <CardHeader>
            <div>
              <CardTitle>Processing volume</CardTitle>
              <p className="mt-1 text-xs text-muted">Documents received and invoices approved by day.</p>
            </div>
            <span className="label-caps">{days} days</span>
          </CardHeader>
          <CardContent>
            {dashboard.isLoading ? <Skeleton className="h-48" /> : dashboard.data ? <DailyChart data={dashboard.data.daily} /> : null}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Document status</CardTitle>
            <span className="label-caps">Current</span>
          </CardHeader>
          <CardContent>
            {dashboard.isLoading ? (
              <div className="space-y-3">{Array.from({ length: 4 }).map((_, index) => <Skeleton key={index} className="h-8" />)}</div>
            ) : dashboard.data?.document_status.length ? (
              <div className="space-y-3.5">
                {dashboard.data.document_status.map((row) => {
                  const total = dashboard.data.document_status.reduce((sum, item) => sum + item.count, 0);
                  const width = total ? Math.max(3, Math.round((row.count / total) * 100)) : 0;
                  return (
                    <div key={row.status}>
                      <div className="flex items-center justify-between gap-3 text-xs">
                        <StatusBadge status={row.status} kind="document" />
                        <span className="tabular font-semibold text-ink-soft">{row.count}</span>
                      </div>
                      <div className="mt-1.5 h-1.5 overflow-hidden rounded-full bg-paper-deep">
                        <div className="h-full rounded-full bg-indigo/55" style={{ width: `${width}%` }} />
                      </div>
                    </div>
                  );
                })}
              </div>
            ) : (
              <p className="py-8 text-center text-sm text-muted">No documents yet.</p>
            )}
            <div className="mt-5 grid grid-cols-2 gap-3 border-t border-line pt-4">
              <div>
                <p className="label-caps">Validation failures</p>
                <p className="tabular mt-1 text-xl font-semibold">{counts?.validation_failures ?? "—"}</p>
              </div>
              <div>
                <p className="label-caps">Duplicate candidates</p>
                <p className="tabular mt-1 text-xl font-semibold">{counts?.duplicate_candidates ?? "—"}</p>
              </div>
            </div>
          </CardContent>
        </Card>
      </div>

      <div className="grid gap-5 xl:grid-cols-[1.5fr_1fr]">
        <Card>
          <CardHeader>
            <div>
              <CardTitle>Review queue</CardTitle>
              <p className="mt-1 text-xs text-muted">Oldest exceptions are shown first.</p>
            </div>
            <Link href="/review" className="flex items-center gap-1 text-xs font-semibold text-indigo hover:underline">
              Open queue <ArrowRight className="size-3" />
            </Link>
          </CardHeader>
          <CardContent className="px-0 pb-0">
            {queue.isLoading ? (
              <div className="space-y-2 px-5 pb-5">{Array.from({ length: 3 }).map((_, index) => <Skeleton key={index} className="h-12" />)}</div>
            ) : queue.data?.items.length ? (
              <Table>
                <THead>
                  <tr>
                    <TH>Invoice</TH>
                    <TH>Vendor</TH>
                    <TH>Total</TH>
                    <TH>Exceptions</TH>
                    <TH />
                  </tr>
                </THead>
                <TBody>
                  {queue.data.items.map((invoice) => (
                    <TR key={invoice.id}>
                      <TD>
                        <Link href={`/invoices/${invoice.id}`} className="font-semibold text-ink hover:text-indigo">
                          {invoice.invoice_number ?? "Unnumbered"}
                        </Link>
                        <p className="mt-0.5 text-[11px] text-muted">{invoice.invoice_date ?? "No date"}</p>
                      </TD>
                      <TD className="max-w-[180px]"><span className="block truncate">{invoice.vendor_name ?? "Unknown vendor"}</span></TD>
                      <TD className="tabular whitespace-nowrap font-medium">{formatMoney(invoice.total, invoice.currency ?? "INR")}</TD>
                      <TD>
                        <div className="flex flex-wrap gap-1">
                          {invoice.blocking_issue_count ? <span className="rounded bg-rust-soft px-1.5 py-0.5 text-[10px] font-semibold text-rust">{invoice.blocking_issue_count} blocking</span> : null}
                          {invoice.open_duplicate_count ? <span className="rounded bg-amber-soft px-1.5 py-0.5 text-[10px] font-semibold text-amber">{invoice.open_duplicate_count} duplicate</span> : null}
                          {!invoice.blocking_issue_count && !invoice.open_duplicate_count ? <span className="text-[10px] text-muted">{invoice.issue_count} notes</span> : null}
                        </div>
                      </TD>
                      <TD className="text-right"><Link href={`/invoices/${invoice.id}`} className="text-xs font-semibold text-indigo hover:underline">Review</Link></TD>
                    </TR>
                  ))}
                </TBody>
              </Table>
            ) : (
              <div className="px-5 pb-5">
                <EmptyState icon={<CheckCircle2 className="size-5" />} title="Queue is clear" description="No extracted invoices are waiting for review right now." className="py-10" />
              </div>
            )}
          </CardContent>
        </Card>

        <div className="space-y-5">
          <Card>
            <CardHeader>
              <CardTitle>Recent activity</CardTitle>
              <Link href="/activity" className="text-xs font-semibold text-indigo hover:underline">Audit log</Link>
            </CardHeader>
            <CardContent>
              {dashboard.isLoading ? (
                <div className="space-y-3">{Array.from({ length: 4 }).map((_, index) => <Skeleton key={index} className="h-8" />)}</div>
              ) : dashboard.data?.recent_activity.length ? (
                <ol className="space-y-3.5">
                  {dashboard.data.recent_activity.map((event) => (
                    <li key={event.id} className="flex gap-3">
                      <span className="mt-1.5 size-1.5 shrink-0 rounded-full bg-indigo/70" />
                      <div className="min-w-0">
                        <p className="text-xs font-medium leading-5 text-ink">{titleCase(event.action)}</p>
                        <p className="text-[10px] text-muted">{event.actor_name ?? "System"} · {relativeTime(event.created_at)}</p>
                      </div>
                    </li>
                  ))}
                </ol>
              ) : (
                <p className="py-6 text-center text-xs text-muted">No activity in this period.</p>
              )}
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Processing errors</CardTitle>
              <Link href="/documents?status=failed" className="text-xs font-semibold text-indigo hover:underline">View all</Link>
            </CardHeader>
            <CardContent>
              {dashboard.data?.recent_errors.length ? (
                <div className="space-y-3">
                  {dashboard.data.recent_errors.map((error) => (
                    <Link key={error.document_id} href={`/documents/${error.document_id}`} className="block rounded-md border border-line bg-paper/40 px-3 py-2.5 hover:border-line-strong">
                      <p className="truncate text-xs font-semibold text-ink">{error.filename}</p>
                      <p className="mt-0.5 line-clamp-2 text-[10px] leading-4 text-rust">{error.error_message ?? error.error_code ?? "Processing failed"}</p>
                      <p className="mt-1 text-[9px] uppercase tracking-wide text-muted">{formatDateTime(error.created_at)}</p>
                    </Link>
                  ))}
                </div>
              ) : (
                <div className="flex items-center gap-2 py-4 text-xs text-muted"><CheckCircle2 className="size-4 text-moss" /> No processing errors in this period.</div>
              )}
            </CardContent>
          </Card>
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-x-8 gap-y-2 border-t border-line pt-5 text-[11px] text-muted">
        <span className="flex items-center gap-1.5"><Layers3 className="size-3.5" /> {counts?.invoices_extracted ?? 0} invoices extracted</span>
        <span className="flex items-center gap-1.5"><ReceiptText className="size-3.5" /> {counts?.total_ai_requests ?? 0} extraction requests</span>
        <span className="flex items-center gap-1.5"><Clock3 className="size-3.5" /> Average processing {avgSeconds}s</span>
      </div>

      <UploadDialog open={uploadOpen} onOpenChange={setUploadOpen} />
    </div>
  );
}
