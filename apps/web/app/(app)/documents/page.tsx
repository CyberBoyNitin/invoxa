"use client";

import { useQuery } from "@tanstack/react-query";
import { AlertTriangle, ArrowUpRight, FileImage, FileStack, RefreshCw, Search, Upload } from "lucide-react";
import Link from "next/link";
import * as React from "react";
import { StatusBadge } from "@/components/status-badge";
import { UploadDialog } from "@/components/upload-dialog";
import { Button } from "@/components/ui/button";
import { EmptyState, Skeleton } from "@/components/ui/card";
import { Input, Select } from "@/components/ui/input";
import { PageHeader } from "@/components/ui/page-header";
import { Table, TBody, TD, TH, THead, TR } from "@/components/ui/table";
import { apiFetch, errorMessage } from "@/lib/api";
import { formatDateTime, formatDecimal } from "@/lib/format";
import type { DocumentRow } from "@/lib/types";

interface DocumentListResponse {
  items: DocumentRow[];
  total: number;
  limit: number;
  offset: number;
}

function sizeLabel(bytes: number) {
  if (bytes >= 1024 * 1024) return `${formatDecimal(String(bytes / 1024 / 1024), 1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

const activeStatuses = new Set([
  "uploaded",
  "queued",
  "extracting_text",
  "running_ocr",
  "extracting_fields",
  "validating",
]);

export default function DocumentsPage() {
  const [search, setSearch] = React.useState("");
  const [debounced, setDebounced] = React.useState("");
  const [status, setStatus] = React.useState("");
  const [offset, setOffset] = React.useState(0);
  const [uploadOpen, setUploadOpen] = React.useState(false);
  const limit = 25;

  React.useEffect(() => {
    const timer = window.setTimeout(() => {
      setDebounced(search.trim());
      setOffset(0);
    }, 280);
    return () => window.clearTimeout(timer);
  }, [search]);

  const query = useQuery({
    queryKey: ["documents", "list", { debounced, status, offset }],
    queryFn: () => {
      const params = new URLSearchParams();
      if (debounced) params.set("search", debounced);
      if (status) params.set("status", status);
      params.set("limit", String(limit));
      params.set("offset", String(offset));
      return apiFetch<DocumentListResponse>(`/documents?${params.toString()}`);
    },
    placeholderData: (previous) => previous,
    refetchInterval: (queryResult) => {
      const items = queryResult.state.data?.items ?? [];
      return items.some((item) => activeStatuses.has(item.status)) ? 2_500 : 20_000;
    },
  });

  return (
    <div className="space-y-6">
      <PageHeader
        eyebrow="Source records"
        title="Documents"
        description="Every uploaded file, its processing state, and the invoice record it produced. Original files stay private to this organization."
        actions={
          <>
            <Button variant="outline" onClick={() => void query.refetch()} aria-label="Refresh documents">
              <RefreshCw className={`size-4 ${query.isFetching ? "animate-spin" : ""}`} />
            </Button>
            <Button onClick={() => setUploadOpen(true)}><Upload className="size-4" /> Add documents</Button>
          </>
        }
      />

      <div className="flex flex-col gap-3 rounded-lg border border-line bg-surface p-3 sm:flex-row">
        <div className="relative flex-1">
          <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted" />
          <Input className="pl-9" placeholder="Search filename or invoice…" value={search} onChange={(event) => setSearch(event.target.value)} aria-label="Search documents" />
        </div>
        <Select
          value={status}
          onChange={(event) => {
            setStatus(event.target.value);
            setOffset(0);
          }}
          className="sm:w-52"
          aria-label="Filter document status"
        >
          <option value="">All processing states</option>
          <option value="queued">Queued</option>
          <option value="extracting_text">Extracting text</option>
          <option value="running_ocr">Running OCR</option>
          <option value="extracting_fields">Extracting fields</option>
          <option value="validating">Validating</option>
          <option value="review_required">Review required</option>
          <option value="ready_for_approval">Ready for approval</option>
          <option value="approved">Approved</option>
          <option value="exported">Exported</option>
          <option value="failed">Failed</option>
        </Select>
      </div>

      <div className="overflow-hidden rounded-lg border border-line bg-surface">
        {query.isLoading ? (
          <div className="space-y-2 p-4">{Array.from({ length: 6 }).map((_, index) => <Skeleton key={index} className="h-12" />)}</div>
        ) : query.isError ? (
          <div className="p-5 text-sm text-rust">{errorMessage(query.error)}</div>
        ) : query.data?.items.length ? (
          <Table>
            <THead>
              <tr><TH>File</TH><TH>Type</TH><TH>Invoice</TH><TH>Status</TH><TH>Uploaded</TH><TH>Pages</TH><TH className="text-right">Open</TH></tr>
            </THead>
            <TBody>
              {query.data.items.map((document) => (
                <TR key={document.id}>
                  <TD>
                    <Link href={`/documents/${document.id}`} className="group flex items-center gap-2 font-semibold text-ink hover:text-indigo">
                      {document.content_type === "application/pdf" ? <FileStack className="size-4 shrink-0 text-indigo/70" /> : <FileImage className="size-4 shrink-0 text-indigo/70" />}
                      <span className="max-w-[280px] truncate">{document.filename}</span>
                      <ArrowUpRight className="size-3 opacity-0 group-hover:opacity-100" />
                    </Link>
                    <p className="mt-0.5 text-[10px] text-muted">{sizeLabel(document.size_bytes)} · {document.content_type}</p>
                  </TD>
                  <TD className="text-xs text-ink-soft">{document.document_type.label}</TD>
                  <TD>
                    {document.invoice ? (
                      <Link href={`/invoices/${document.invoice.id}`} className="text-xs font-semibold text-indigo hover:underline">
                        {document.invoice.invoice_number ?? document.invoice.vendor_name ?? "View invoice"}
                      </Link>
                    ) : <span className="text-[11px] text-muted">Not created</span>}
                  </TD>
                  <TD>
                    <StatusBadge status={document.status} kind="document" />
                    {document.error_code ? (
                      <p className="mt-1 flex items-center gap-1 text-[10px] text-rust"><AlertTriangle className="size-3" /> {document.error_code}</p>
                    ) : null}
                  </TD>
                  <TD className="whitespace-nowrap text-[11px] text-muted">{formatDateTime(document.created_at)}</TD>
                  <TD className="tabular text-xs">{document.page_count ?? "—"}</TD>
                  <TD className="text-right">
                    <Button asChild size="sm" variant="outline"><Link href={`/documents/${document.id}`}>Open</Link></Button>
                  </TD>
                </TR>
              ))}
            </TBody>
          </Table>
        ) : (
          <div className="p-5">
            <EmptyState
              icon={<FileStack className="size-5" />}
              title={status ? "No documents match" : "No documents yet"}
              description={status ? "Adjust the search or status filter." : "Upload supplier invoices to start extraction and review."}
              action={<Button onClick={() => setUploadOpen(true)}><Upload className="size-4" /> Add documents</Button>}
            />
          </div>
        )}
      </div>

      {query.data ? (
        <div className="flex items-center justify-between text-xs text-muted">
          <p>{query.data.total === 0 ? "No results" : `${query.data.offset + 1}–${Math.min(query.data.offset + query.data.items.length, query.data.total)} of ${query.data.total}`}</p>
          <div className="flex gap-2">
            <Button size="sm" variant="outline" disabled={offset === 0} onClick={() => setOffset(Math.max(0, offset - limit))}>Previous</Button>
            <Button size="sm" variant="outline" disabled={offset + limit >= query.data.total} onClick={() => setOffset(offset + limit)}>Next</Button>
          </div>
        </div>
      ) : null}

      <UploadDialog open={uploadOpen} onOpenChange={setUploadOpen} />
    </div>
  );
}
