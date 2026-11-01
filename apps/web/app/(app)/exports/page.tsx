"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckCircle2, Download, FileJson, FileSpreadsheet, Plus, RefreshCw, ShieldCheck } from "lucide-react";
import * as React from "react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle, EmptyState, Skeleton } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter } from "@/components/ui/dialog";
import { Field, Input, Select } from "@/components/ui/input";
import { PageHeader } from "@/components/ui/page-header";
import { Table, TBody, TD, TH, THead, TR } from "@/components/ui/table";
import { Checkbox } from "@/components/ui/input";
import { apiFetch, errorMessage } from "@/lib/api";
import { formatDateTime, formatMoney, titleCase } from "@/lib/format";
import type { ExportJob, InvoiceListRow, VendorRow } from "@/lib/types";

interface ExportListResponse {
  items: ExportJob[];
  limit: number;
}
interface VendorListResponse {
  items: VendorRow[];
  total: number;
}
interface InvoiceListResponse {
  items: InvoiceListRow[];
  total: number;
}

const activeStatuses = new Set(["queued", "running"]);

export default function ExportsPage() {
  const [dialogOpen, setDialogOpen] = React.useState(false);
  const [format, setFormat] = React.useState<"csv" | "json">("csv");
  const [from, setFrom] = React.useState("");
  const [to, setTo] = React.useState("");
  const [vendorId, setVendorId] = React.useState("");
  const [status, setStatus] = React.useState<"approved" | "exported">("approved");
  const [selectedInvoiceIds, setSelectedInvoiceIds] = React.useState<string[]>([]);
  const [busy, setBusy] = React.useState(false);
  const queryClient = useQueryClient();

  const exports = useQuery({
    queryKey: ["exports"],
    queryFn: () => apiFetch<ExportListResponse>("/exports?limit=100"),
    refetchInterval: (queryResult) => (queryResult.state.data?.items.some((item) => activeStatuses.has(item.status)) ? 2_000 : 30_000),
  });
  const vendors = useQuery({
    queryKey: ["vendors", "export-filter"],
    queryFn: () => apiFetch<VendorListResponse>("/vendors?limit=100"),
  });
  const approvedInvoices = useQuery({
    queryKey: ["invoices", "export-select", status],
    queryFn: () => apiFetch<InvoiceListResponse>(`/invoices?status=${status}&limit=100`),
    enabled: dialogOpen,
  });

  function toggleInvoice(invoiceId: string) {
    setSelectedInvoiceIds((current) =>
      current.includes(invoiceId)
        ? current.filter((id) => id !== invoiceId)
        : [...current, invoiceId],
    );
  }

  function toggleAllInvoices() {
    const visible = approvedInvoices.data?.items.map((invoice) => invoice.id) ?? [];
    setSelectedInvoiceIds((current) =>
      current.length === visible.length ? [] : visible,
    );
  }

  async function create() {
    setBusy(true);
    try {
      const filters: Record<string, string | string[]> = { status };
      if (from) filters.from = from;
      if (to) filters.to = to;
      if (vendorId) filters.vendor_id = vendorId;
      if (selectedInvoiceIds.length) filters.invoice_ids = selectedInvoiceIds;
      await apiFetch<ExportJob>("/exports", {
        method: "POST",
        body: { format, filters, idempotency_key: crypto.randomUUID() },
      });
      setDialogOpen(false);
      setFrom("");
      setTo("");
      setVendorId("");
      setSelectedInvoiceIds([]);
      await queryClient.invalidateQueries({ queryKey: ["exports"] });
      toast.success(
        selectedInvoiceIds.length
          ? `${selectedInvoiceIds.length} selected ${status === "exported" ? "previously exported" : "approved"} invoice${selectedInvoiceIds.length === 1 ? "" : "s"} queued for export.`
          : `Export queued. Only ${status === "exported" ? "previously exported" : "approved"} invoices will be included.`,
      );
    } catch (caught) {
      toast.error(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  }

  function download(job: ExportJob) {
    if (!job.download_url) return;
    window.open(job.download_url, "_blank", "noopener,noreferrer");
  }

  return (
    <div className="space-y-6">
      <PageHeader
        eyebrow="Post-approval records only"
        title="Exports"
        description="Generate durable CSV or JSON artifacts from approved or previously exported invoices. Requesting an export never approves an invoice."
        actions={
          <>
            <Button variant="outline" onClick={() => void exports.refetch()} aria-label="Refresh exports"><RefreshCw className={`size-4 ${exports.isFetching ? "animate-spin" : ""}`} /></Button>
            <Button onClick={() => setDialogOpen(true)}><Plus className="size-4" /> New export</Button>
          </>
        }
      />

      <div className="grid gap-4 sm:grid-cols-3">
        {[
          ["Human approval required", "Only approved or previously exported records enter an export query."],
          ["Exact decimal money", "Amounts are serialized from NUMERIC values, never floating point."],
          ["Durable artifacts", "Completed files are stored privately and can be downloaded until expiry."],
        ].map(([title, description]) => (
          <div key={title} className="flex items-start gap-3 rounded-lg border border-line bg-surface px-4 py-3">
            <ShieldCheck className="mt-0.5 size-4 shrink-0 text-moss" />
            <div><p className="text-xs font-semibold text-ink">{title}</p><p className="mt-0.5 text-[11px] leading-4 text-muted">{description}</p></div>
          </div>
        ))}
      </div>

      <Card className="overflow-hidden">
        <CardHeader><CardTitle>Export history</CardTitle><span className="label-caps">Last 100 jobs</span></CardHeader>
        <CardContent className="px-0 pb-0">
          {exports.isLoading ? (
            <div className="space-y-2 px-5 pb-5">{Array.from({ length: 5 }).map((_, index) => <Skeleton key={index} className="h-12" />)}</div>
          ) : exports.isError ? (
            <div className="p-5 text-sm text-rust">{errorMessage(exports.error)}</div>
          ) : exports.data?.items.length ? (
            <Table>
              <THead><tr><TH>Artifact</TH><TH>Format</TH><TH>Status</TH><TH>Rows</TH><TH>Filters</TH><TH>Requested by</TH><TH>Created</TH><TH className="text-right">Download</TH></tr></THead>
              <TBody>
                {exports.data.items.map((job) => (
                  <TR key={job.id}>
                    <TD>
                      <div className="flex items-center gap-2">
                        {job.format === "csv" ? <FileSpreadsheet className="size-4 text-moss" /> : <FileJson className="size-4 text-indigo" />}
                        <div><p className="max-w-[220px] truncate text-xs font-semibold">{job.filename ?? `${job.format.toUpperCase()} export`}</p><p className="text-[10px] text-muted">{job.id.slice(0, 8)}</p></div>
                      </div>
                    </TD>
                    <TD><Badge tone="neutral">{job.format.toUpperCase()}</Badge></TD>
                    <TD>
                      <Badge tone={job.status === "succeeded" ? "moss" : job.status === "failed" ? "rust" : "indigo"}>{titleCase(job.status)}</Badge>
                      {job.error_message ? <p className="mt-1 max-w-[220px] text-[10px] leading-4 text-rust">{job.error_message}</p> : null}
                    </TD>
                    <TD className="tabular text-xs">{job.row_count ?? "—"}</TD>
                    <TD className="max-w-[180px] text-[10px] leading-4 text-muted">{Object.entries(job.filters).length ? Object.entries(job.filters).map(([key, value]) => `${key}: ${String(value)}`).join(" · ") : "All approved"}</TD>
                    <TD className="text-xs">{job.requested_by_name ?? "—"}</TD>
                    <TD className="whitespace-nowrap text-[11px] text-muted">{formatDateTime(job.created_at)}</TD>
                    <TD className="text-right">
                      {job.status === "succeeded" ? <Button size="sm" variant="outline" onClick={() => download(job)}><Download className="size-3.5" /> Download</Button> : job.status === "failed" ? <span className="text-[10px] text-rust">Failed</span> : <span className="text-[10px] text-muted">Processing…</span>}
                    </TD>
                  </TR>
                ))}
              </TBody>
            </Table>
          ) : (
            <div className="p-5"><EmptyState icon={<FileSpreadsheet className="size-5" />} title="No exports yet" description="Approve invoices in the review workspace, then create a CSV or JSON export." action={<Button onClick={() => setDialogOpen(true)}><Plus className="size-4" /> New export</Button>} /></div>
          )}
        </CardContent>
      </Card>

      <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
        <DialogContent title="Create export" description="The background worker selects only post-approval invoices for this organization. Newly exported invoices are marked as exported after the artifact is stored.">
          <div className="space-y-4 px-6 py-5">
            <Field label="Format" htmlFor="export-format">
              <Select id="export-format" value={format} onChange={(event) => setFormat(event.target.value as "csv" | "json")}>
                <option value="csv">CSV — spreadsheet</option>
                <option value="json">JSON — API/archive</option>
              </Select>
            </Field>
            <div className="grid gap-4 sm:grid-cols-2">
              <Field label="Invoice date from" htmlFor="export-from"><Input id="export-from" type="date" value={from} max={to || undefined} onChange={(event) => setFrom(event.target.value)} /></Field>
              <Field label="Invoice date to" htmlFor="export-to"><Input id="export-to" type="date" value={to} min={from || undefined} onChange={(event) => setTo(event.target.value)} /></Field>
            </div>
            <div className="grid gap-4 sm:grid-cols-2">
              <Field label="Status" htmlFor="export-status" hint="Approval is never bypassed">
                <Select
                  id="export-status"
                  value={status}
                  onChange={(event) => {
                    setStatus(event.target.value as "approved" | "exported");
                    setSelectedInvoiceIds([]);
                  }}
                >
                  <option value="approved">Approved, not yet exported</option>
                  <option value="exported">Previously exported</option>
                </Select>
              </Field>
              <Field label="Vendor" htmlFor="export-vendor">
                <Select id="export-vendor" value={vendorId} onChange={(event) => setVendorId(event.target.value)}>
                  <option value="">All vendors</option>
                  {vendors.data?.items.map((vendor) => <option key={vendor.id} value={vendor.id}>{vendor.name}</option>)}
                </Select>
              </Field>
            </div>
            <div>
              <div className="mb-2 flex items-center justify-between gap-3">
                <p className="text-xs font-semibold">
                  {status === "exported" ? "Previously exported invoices" : "Approved invoices"} <span className="font-normal text-muted">({selectedInvoiceIds.length} selected)</span>
                </p>
                <button type="button" className="text-[11px] font-semibold text-indigo hover:underline" onClick={toggleAllInvoices} disabled={!approvedInvoices.data?.items.length}>
                  {selectedInvoiceIds.length && selectedInvoiceIds.length === approvedInvoices.data?.items.length ? "Clear selection" : "Select all"}
                </button>
              </div>
              <div className="max-h-52 overflow-y-auto rounded-md border border-line bg-paper/25 p-2">
                {approvedInvoices.isLoading ? (
                  <div className="space-y-2 p-2"><Skeleton className="h-9" /><Skeleton className="h-9" /><Skeleton className="h-9" /></div>
                ) : approvedInvoices.isError ? (
                  <p className="p-2 text-[11px] text-rust">{errorMessage(approvedInvoices.error)}</p>
                ) : approvedInvoices.data?.items.length ? (
                  <div className="space-y-1">
                    {approvedInvoices.data.items.map((invoice) => (
                      <label key={invoice.id} className="flex cursor-pointer items-start gap-2 rounded px-2 py-1.5 hover:bg-surface">
                        <Checkbox
                          checked={selectedInvoiceIds.includes(invoice.id)}
                          onChange={() => toggleInvoice(invoice.id)}
                          className="mt-0.5"
                        />
                        <span className="min-w-0 flex-1">
                          <span className="block truncate text-[11px] font-semibold text-ink">{invoice.invoice_number ?? "Unnumbered"} · {invoice.vendor_name ?? "Unknown vendor"}</span>
                          <span className="block text-[10px] text-muted">{formatMoney(invoice.total, invoice.currency ?? "INR")} · {invoice.invoice_date ?? "No date"} · {invoice.id.slice(0, 8)}</span>
                        </span>
                      </label>
                    ))}
                  </div>
                ) : (
                  <p className="p-2 text-[11px] leading-5 text-muted">No {status === "exported" ? "previously exported" : "approved"} invoices are available yet. Leave the selection empty to export every invoice that matches the filters.</p>
                )}
              </div>
              <p className="mt-1.5 text-[10px] leading-4 text-muted">Selecting specific invoices is optional. The server re-checks tenant ownership and post-approval status before queueing the job.</p>
            </div>
            <div className="flex items-start gap-2 rounded-md border border-moss/20 bg-moss-soft/60 px-3 py-2.5 text-[11px] leading-5 text-moss">
              <CheckCircle2 className="mt-0.5 size-3.5 shrink-0" />
              Invoices with unresolved validation errors, open duplicates, or no explicit approval are never eligible. The server query re-checks status at export time.
            </div>
          </div>
          <DialogFooter><Button variant="ghost" onClick={() => setDialogOpen(false)}>Cancel</Button><Button onClick={create} loading={busy}><FileSpreadsheet className="size-4" /> Queue export</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
