"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  ArrowLeft,
  CheckCircle2,
  Clock3,
  Download,
  FileText,
  RefreshCw,
  RotateCcw,
  Trash2,
} from "lucide-react";
import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import * as React from "react";
import { toast } from "sonner";
import { PdfViewer } from "@/components/pdf-viewer";
import { StatusBadge } from "@/components/status-badge";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle, Skeleton } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter } from "@/components/ui/dialog";
import { Field, Textarea } from "@/components/ui/input";
import { apiFetch, errorMessage } from "@/lib/api";
import { formatDateTime, formatDecimal, titleCase } from "@/lib/format";
import type { DocumentDetail } from "@/lib/types";

const activeStatuses = new Set([
  "uploaded",
  "queued",
  "extracting_text",
  "running_ocr",
  "extracting_fields",
  "validating",
]);

export default function DocumentDetailPage() {
  const params = useParams<{ id: string }>();
  const router = useRouter();
  const queryClient = useQueryClient();
  const [retryOpen, setRetryOpen] = React.useState(false);
  const [deleteOpen, setDeleteOpen] = React.useState(false);
  const [reason, setReason] = React.useState("");
  const [busy, setBusy] = React.useState(false);

  const detail = useQuery({
    queryKey: ["documents", "detail", params.id],
    queryFn: () => apiFetch<DocumentDetail>(`/documents/${params.id}`),
    refetchInterval: (queryResult) =>
      activeStatuses.has(queryResult.state.data?.status ?? "") ? 2_000 : 30_000,
  });
  const document = detail.data;

  async function download() {
    try {
      const grant = await apiFetch<{ url: string }>(`/documents/${params.id}/download-url`, {
        method: "POST",
        body: { disposition: "attachment" },
      });
      window.open(grant.url, "_blank", "noopener,noreferrer");
    } catch (caught) {
      toast.error(errorMessage(caught));
    }
  }

  async function retry() {
    setBusy(true);
    try {
      await apiFetch(`/documents/${params.id}/retry`, {
        method: "POST",
        body: { reason: reason.trim() || null },
      });
      setRetryOpen(false);
      setReason("");
      await queryClient.invalidateQueries({ queryKey: ["documents"] });
      toast.success("Document queued for reprocessing.");
    } catch (caught) {
      toast.error(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    setBusy(true);
    try {
      await apiFetch(`/documents/${params.id}`, { method: "DELETE" });
      queryClient.removeQueries({ queryKey: ["documents", "detail", params.id] });
      await queryClient.invalidateQueries({ queryKey: ["documents"] });
      await queryClient.invalidateQueries({ queryKey: ["dashboard"] });
      toast.success("Document deleted.");
      router.push("/documents");
    } catch (caught) {
      toast.error(errorMessage(caught));
      setBusy(false);
    }
  }

  if (detail.isLoading) {
    return (
      <div className="space-y-5">
        <Skeleton className="h-10 w-64" />
        <div className="grid gap-5 xl:grid-cols-2"><Skeleton className="h-[620px]" /><Skeleton className="h-[620px]" /></div>
      </div>
    );
  }
  if (detail.isError || !document) {
    return (
      <div className="rounded-lg border border-rust/25 bg-rust-soft p-6 text-sm text-rust">
        {detail.error ? errorMessage(detail.error) : "Document not found."}
        <div className="mt-4"><Button asChild variant="outline"><Link href="/documents">Back to documents</Link></Button></div>
      </div>
    );
  }

  const canRetry = document.status === "failed" || document.status === "review_required" || document.status === "ready_for_approval";
  const canDelete = document.invoice?.status !== "approved" && document.invoice?.status !== "exported";

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <Link href="/documents" className="mb-3 inline-flex items-center gap-1.5 text-xs font-semibold text-muted hover:text-ink">
            <ArrowLeft className="size-3.5" /> Back to documents
          </Link>
          <div className="flex flex-wrap items-center gap-2.5">
            <h1 className="max-w-[720px] truncate font-display text-[2rem] leading-tight tracking-[-0.025em]">{document.filename}</h1>
            <StatusBadge status={document.status} kind="document" />
          </div>
          <p className="mt-2 text-sm text-muted">
            {document.document_type.label} · {document.page_count ?? "?"} page{document.page_count === 1 ? "" : "s"} · uploaded {formatDateTime(document.created_at)}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" onClick={() => void detail.refetch()} aria-label="Refresh document"><RefreshCw className={`size-4 ${detail.isFetching ? "animate-spin" : ""}`} /></Button>
          <Button variant="outline" onClick={() => void download()}><Download className="size-4" /> Original</Button>
          {canRetry ? <Button variant="outline" onClick={() => setRetryOpen(true)}><RotateCcw className="size-4" /> Reprocess</Button> : null}
          {canDelete ? <Button variant="danger" onClick={() => setDeleteOpen(true)}><Trash2 className="size-4" /> Delete</Button> : null}
        </div>
      </div>

      {document.error_code ? (
        <div className="flex items-start gap-2.5 rounded-md border border-rust/25 bg-rust-soft px-4 py-3 text-xs leading-5 text-rust">
          <AlertTriangle className="mt-0.5 size-4 shrink-0" />
          <div><p className="font-semibold">{document.error_code}</p><p className="mt-0.5">{document.error_message ?? "Processing failed."}</p></div>
        </div>
      ) : null}

      <div className="grid items-start gap-5 xl:grid-cols-[minmax(0,1.1fr)_minmax(440px,0.9fr)]">
        <div className="xl:sticky xl:top-[84px] xl:h-[calc(100vh-104px)]">
          <PdfViewer documentId={document.id} contentType={document.content_type} filename={document.filename} onDownload={() => void download()} />
        </div>

        <div className="space-y-5">
          <Card>
            <CardHeader><CardTitle>Extraction record</CardTitle><span className="label-caps">Source of truth</span></CardHeader>
            <CardContent className="space-y-4">
              {document.invoice ? (
                <Link href={`/invoices/${document.invoice.id}`} className="flex items-center justify-between gap-3 rounded-md border border-line bg-paper/40 px-3.5 py-3 transition-colors hover:border-line-strong">
                  <div>
                    <p className="text-sm font-semibold text-ink">{document.invoice.invoice_number ?? "Unnumbered invoice"}</p>
                    <p className="mt-0.5 text-[11px] text-muted">{document.invoice.vendor_name ?? "Unknown vendor"}</p>
                  </div>
                  <div className="text-right">
                    <StatusBadge status={document.invoice.status} />
                    {document.invoice.issue_count ? <p className="mt-1 text-[10px] text-rust">{document.invoice.issue_count} open issue{document.invoice.issue_count === 1 ? "" : "s"}</p> : null}
                  </div>
                </Link>
              ) : (
                <div className="rounded-md border border-dashed border-line px-4 py-6 text-center text-xs text-muted">
                  {activeStatuses.has(document.status) ? "Extraction is in progress. This page updates automatically." : "No invoice record was produced."}
                </div>
              )}
              <dl className="grid grid-cols-2 gap-x-4 gap-y-4 text-xs">
                <div><dt className="label-caps">File size</dt><dd className="mt-1">{formatDecimal(String(document.size_bytes / 1024), 0)} KB</dd></div>
                <div><dt className="label-caps">Attempts</dt><dd className="mt-1">{document.processing_attempts}</dd></div>
                <div><dt className="label-caps">Provider</dt><dd className="mt-1">{document.extraction_provider ?? "—"}</dd></div>
                <div><dt className="label-caps">Model</dt><dd className="mt-1">{document.extraction_model ?? "—"}</dd></div>
                <div><dt className="label-caps">Duration</dt><dd className="mt-1">{document.processing_duration_ms ? `${formatDecimal(String(document.processing_duration_ms / 1000), 1)}s` : "—"}</dd></div>
                <div><dt className="label-caps">SHA-256</dt><dd className="mt-1 truncate font-mono text-[10px]" title={document.sha256}>{document.sha256.slice(0, 16)}…</dd></div>
              </dl>
            </CardContent>
          </Card>

          <Card>
            <CardHeader><CardTitle>Processing timeline</CardTitle><span className="label-caps">{document.events.length} events</span></CardHeader>
            <CardContent>
              {document.events.length ? (
                <ol className="relative space-y-4 border-l border-line pl-5">
                  {document.events.map((event) => (
                    <li key={event.id} className="relative">
                      <span className={`absolute -left-[26px] top-1 grid size-2.5 place-items-center rounded-full border-2 border-surface ${event.status === "failed" ? "bg-rust" : event.status === "succeeded" || event.status === "completed" ? "bg-moss" : "bg-indigo"}`} />
                      <div className="flex flex-wrap items-center gap-2">
                        <p className="text-xs font-semibold text-ink">{titleCase(event.stage)}</p>
                        <Badge tone={event.status === "failed" ? "rust" : "neutral"}>{titleCase(event.status)}</Badge>
                        {event.duration_ms ? <span className="text-[10px] text-muted">{formatDecimal(String(event.duration_ms / 1000), 1)}s</span> : null}
                      </div>
                      <p className="mt-1 text-[11px] leading-5 text-muted">{event.message}</p>
                      <p className="mt-0.5 text-[9px] text-muted/80">{formatDateTime(event.created_at)}</p>
                    </li>
                  ))}
                </ol>
              ) : (
                <p className="py-5 text-center text-xs text-muted">No processing events recorded yet.</p>
              )}
            </CardContent>
          </Card>

          <Card>
            <CardHeader><CardTitle>Background jobs</CardTitle><Clock3 className="size-4 text-muted" /></CardHeader>
            <CardContent className="space-y-3">
              {document.jobs.length ? document.jobs.map((job) => (
                <div key={job.id} className="rounded-md border border-line bg-paper/30 px-3 py-2.5">
                  <div className="flex items-center justify-between gap-2">
                    <p className="text-xs font-semibold">{titleCase(job.stage)}</p>
                    <Badge tone={job.status === "failed" ? "rust" : job.status === "succeeded" ? "moss" : "indigo"}>{titleCase(job.status)}</Badge>
                  </div>
                  <p className="mt-1 text-[10px] text-muted">Attempt {job.attempts} of {job.max_attempts} · next run {formatDateTime(job.run_at)}</p>
                  {job.error_message ? <p className="mt-1 text-[10px] leading-4 text-rust">{job.error_code}: {job.error_message}</p> : null}
                </div>
              )) : <p className="text-xs text-muted">No jobs recorded.</p>}
            </CardContent>
          </Card>
        </div>
      </div>

      <Dialog open={retryOpen} onOpenChange={setRetryOpen}>
        <DialogContent title="Reprocess this document?" description="A new background job will re-run text extraction, OCR if needed, field extraction, and validation. The existing invoice record is replaced only if processing succeeds.">
          <div className="px-6 py-5"><Field label="Reason" hint="Optional, audited"><Textarea value={reason} onChange={(event) => setReason(event.target.value)} placeholder="Why is reprocessing needed?" maxLength={500} /></Field></div>
          <DialogFooter><Button variant="ghost" onClick={() => setRetryOpen(false)}>Cancel</Button><Button onClick={retry} loading={busy}><RotateCcw className="size-4" /> Queue reprocessing</Button></DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={deleteOpen} onOpenChange={setDeleteOpen}>
        <DialogContent title="Delete this document?" description="The file and any unapproved invoice record will be removed. This cannot be undone. Approved and exported records are retained.">
          <div className="px-6 py-5 text-sm text-muted"><p>{document.filename}</p></div>
          <DialogFooter><Button variant="ghost" onClick={() => setDeleteOpen(false)}>Cancel</Button><Button variant="danger" onClick={remove} loading={busy}><Trash2 className="size-4" /> Delete permanently</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
