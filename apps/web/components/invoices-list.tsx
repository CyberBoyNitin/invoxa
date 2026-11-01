"use client";

import { useQuery } from "@tanstack/react-query";
import { ArrowUpRight, FileSearch, Search } from "lucide-react";
import Link from "next/link";
import * as React from "react";
import { MockBadge, StatusBadge } from "@/components/status-badge";
import { Button } from "@/components/ui/button";
import { EmptyState, Skeleton } from "@/components/ui/card";
import { Input, Select } from "@/components/ui/input";
import { Table, TBody, TD, TH, THead, TR } from "@/components/ui/table";
import { apiFetch, errorMessage } from "@/lib/api";
import { formatDate, formatMoney } from "@/lib/format";
import type { InvoiceListRow } from "@/lib/types";

interface InvoiceListResponse {
  items: InvoiceListRow[];
  total: number;
  limit: number;
  offset: number;
}

export function InvoicesList({
  initialStatus = "",
  lockStatus = false,
  compact = false,
}: {
  initialStatus?: string;
  lockStatus?: boolean;
  compact?: boolean;
}) {
  const [search, setSearch] = React.useState("");
  const [debounced, setDebounced] = React.useState("");
  const [status, setStatus] = React.useState(initialStatus);
  const [from, setFrom] = React.useState("");
  const [to, setTo] = React.useState("");
  const [offset, setOffset] = React.useState(0);
  const limit = compact ? 10 : 25;

  React.useEffect(() => {
    const timer = window.setTimeout(() => {
      setDebounced(search.trim());
      setOffset(0);
    }, 280);
    return () => window.clearTimeout(timer);
  }, [search]);

  const query = useQuery({
    queryKey: ["invoices", "list", { debounced, status, from, to, offset, limit }],
    queryFn: () => {
      const params = new URLSearchParams();
      if (debounced) params.set("search", debounced);
      if (status) params.set("status", status);
      if (from) params.set("from", from);
      if (to) params.set("to", to);
      params.set("limit", String(limit));
      params.set("offset", String(offset));
      return apiFetch<InvoiceListResponse>(`/invoices?${params.toString()}`);
    },
    placeholderData: (previous) => previous,
    refetchInterval: 20_000,
  });

  return (
    <div className="space-y-4">
      <div className="flex flex-col gap-3 rounded-lg border border-line bg-surface p-3 sm:flex-row sm:items-center">
        <div className="relative flex-1">
          <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted" />
          <Input
            className="pl-9"
            placeholder="Search invoice number, vendor, PO…"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            aria-label="Search invoices"
          />
        </div>
        {!lockStatus ? (
          <Select
            value={status}
            onChange={(event) => {
              setStatus(event.target.value);
              setOffset(0);
            }}
            className="sm:w-48"
            aria-label="Filter by status"
          >
            <option value="">All statuses</option>
            <option value="review_required">Review required</option>
            <option value="ready_for_approval">Ready for approval</option>
            <option value="approved">Approved</option>
            <option value="rejected">Rejected</option>
            <option value="exported">Exported</option>
          </Select>
        ) : null}
        <div className="flex items-center gap-2">
          <Input
            type="date"
            value={from}
            max={to || undefined}
            onChange={(event) => {
              setFrom(event.target.value);
              setOffset(0);
            }}
            className="w-[148px]"
            aria-label="Invoice date from"
          />
          <span className="text-xs text-muted">to</span>
          <Input
            type="date"
            value={to}
            min={from || undefined}
            onChange={(event) => {
              setTo(event.target.value);
              setOffset(0);
            }}
            className="w-[148px]"
            aria-label="Invoice date to"
          />
        </div>
      </div>

      <div className="overflow-hidden rounded-lg border border-line bg-surface">
        {query.isLoading ? (
          <div className="space-y-2 p-4">{Array.from({ length: 6 }).map((_, index) => <Skeleton key={index} className="h-12" />)}</div>
        ) : query.isError ? (
          <div className="p-5 text-sm text-rust">{errorMessage(query.error)}</div>
        ) : query.data?.items.length ? (
          <Table>
            <THead>
              <tr>
                <TH>Invoice</TH>
                <TH>Vendor</TH>
                <TH>Date</TH>
                <TH className="text-right">Total</TH>
                <TH>Status</TH>
                <TH>Exceptions</TH>
                <TH className="text-right">Open</TH>
              </tr>
            </THead>
            <TBody>
              {query.data.items.map((invoice) => (
                <TR key={invoice.id}>
                  <TD>
                    <Link href={`/invoices/${invoice.id}`} className="group flex items-center gap-2 font-semibold text-ink hover:text-indigo">
                      {invoice.invoice_number ?? "Unnumbered"}
                      <ArrowUpRight className="size-3 opacity-0 transition-opacity group-hover:opacity-100" />
                    </Link>
                    <div className="mt-1 flex items-center gap-1.5">
                      <span className="text-[10px] text-muted">{invoice.source_filename ?? "Document"}</span>
                      {invoice.is_mock ? <MockBadge compact /> : null}
                    </div>
                  </TD>
                  <TD className="max-w-[240px]">
                    <p className="truncate text-sm">{invoice.vendor_name ?? <span className="text-rust">Missing vendor</span>}</p>
                    <p className="truncate text-[10px] text-muted">{invoice.vendor_tax_id ?? "No tax ID"}</p>
                  </TD>
                  <TD className="whitespace-nowrap text-xs text-ink-soft">{formatDate(invoice.invoice_date)}</TD>
                  <TD className="tabular whitespace-nowrap text-right text-sm font-semibold">
                    {formatMoney(invoice.total, invoice.currency ?? "INR")}
                    <span className="ml-1 text-[10px] font-normal text-muted">{invoice.currency ?? ""}</span>
                  </TD>
                  <TD><StatusBadge status={invoice.status} /></TD>
                  <TD>
                    <div className="flex flex-wrap gap-1">
                      {invoice.blocking_issue_count ? <span className="rounded bg-rust-soft px-1.5 py-0.5 text-[10px] font-semibold text-rust">{invoice.blocking_issue_count} blocking</span> : null}
                      {invoice.open_duplicate_count ? <span className="rounded bg-amber-soft px-1.5 py-0.5 text-[10px] font-semibold text-amber">{invoice.open_duplicate_count} duplicate</span> : null}
                      {!invoice.blocking_issue_count && !invoice.open_duplicate_count ? <span className="text-[10px] text-muted">{invoice.issue_count ? `${invoice.issue_count} notes` : "None"}</span> : null}
                    </div>
                  </TD>
                  <TD className="text-right">
                    <Button asChild size="sm" variant="outline">
                      <Link href={`/invoices/${invoice.id}`}>{invoice.status === "review_required" ? "Review" : "Open"}</Link>
                    </Button>
                  </TD>
                </TR>
              ))}
            </TBody>
          </Table>
        ) : (
          <div className="p-5">
            <EmptyState
              icon={<FileSearch className="size-5" />}
              title={status === "review_required" ? "Review queue is clear" : "No invoices match"}
              description={
                status === "review_required"
                  ? "Every extracted invoice has a decision. New uploads will appear here after processing."
                  : "Adjust the search or filters, or upload a new document to get started."
              }
              action={<Button asChild variant="outline"><Link href="/documents">Open documents</Link></Button>}
            />
          </div>
        )}
      </div>

      {query.data ? (
        <div className="flex items-center justify-between gap-4 text-xs text-muted">
          <p>
            {query.data.total === 0
              ? "No results"
              : `${query.data.offset + 1}–${Math.min(query.data.offset + query.data.items.length, query.data.total)} of ${query.data.total}`}
          </p>
          <div className="flex gap-2">
            <Button size="sm" variant="outline" disabled={offset === 0} onClick={() => setOffset(Math.max(0, offset - limit))}>
              Previous
            </Button>
            <Button size="sm" variant="outline" disabled={offset + limit >= query.data.total} onClick={() => setOffset(offset + limit)}>
              Next
            </Button>
          </div>
        </div>
      ) : null}
    </div>
  );
}
