"use client";

import { ChevronLeft, ChevronRight, Download, FileText, Loader2, Minus, Plus, RotateCw } from "lucide-react";
import * as React from "react";
import { Button } from "@/components/ui/button";
import { apiBlob, errorMessage } from "@/lib/api";
import { formatDecimal } from "@/lib/format";

interface PdfDocument {
  numPages: number;
  getPage: (page: number) => Promise<{
    getViewport: (options: { scale: number }) => { width: number; height: number };
    render: (options: { canvas: HTMLCanvasElement; viewport: unknown }) => { promise: Promise<void>; cancel: () => void };
  }>;
  destroy: () => Promise<void>;
}

export function PdfViewer({
  documentId,
  contentType,
  filename,
  onDownload,
}: {
  documentId: string;
  contentType: string;
  filename: string;
  onDownload?: () => void;
}) {
  const [blobUrl, setBlobUrl] = React.useState<string | null>(null);
  const [pdf, setPdf] = React.useState<PdfDocument | null>(null);
  const [page, setPage] = React.useState(1);
  const [pages, setPages] = React.useState(1);
  const [zoom, setZoom] = React.useState(1);
  const [loading, setLoading] = React.useState(true);
  const [rendering, setRendering] = React.useState(false);
  const [error, setError] = React.useState("");
  const canvasRef = React.useRef<HTMLCanvasElement>(null);
  const isPdf = contentType === "application/pdf";

  React.useEffect(() => {
    let cancelled = false;
    let createdUrl: string | null = null;
    let loaded: PdfDocument | null = null;

    async function load() {
      setLoading(true);
      setError("");
      try {
        const blob = await apiBlob(`/documents/${documentId}/content`);
        if (cancelled) return;
        if (!isPdf) {
          createdUrl = URL.createObjectURL(blob);
          setBlobUrl(createdUrl);
          setLoading(false);
          return;
        }
        const pdfjs = await import("pdfjs-dist");
        pdfjs.GlobalWorkerOptions.workerSrc = "/pdf.worker.min.mjs";
        const bytes = new Uint8Array(await blob.arrayBuffer());
        const task = pdfjs.getDocument({ data: bytes, useSystemFonts: true });
        const document = (await task.promise) as unknown as PdfDocument;
        if (cancelled) {
          await document.destroy();
          return;
        }
        loaded = document;
        setPdf(document);
        setPages(document.numPages);
        setPage(1);
        setLoading(false);
      } catch (caught) {
        if (!cancelled) {
          setError(errorMessage(caught));
          setLoading(false);
        }
      }
    }

    void load();
    return () => {
      cancelled = true;
      if (createdUrl) URL.revokeObjectURL(createdUrl);
      if (loaded) void loaded.destroy();
    };
  }, [documentId, isPdf]);

  React.useEffect(() => {
    if (!pdf) return;
    let cancelled = false;
    let task: { promise: Promise<void>; cancel: () => void } | null = null;

    async function renderPage() {
      setRendering(true);
      try {
        const pdfPage = await pdf!.getPage(page);
        if (cancelled || !canvasRef.current) return;
        const viewport = pdfPage.getViewport({ scale: 1.25 * zoom });
        const canvas = canvasRef.current;
        const ratio = Math.min(window.devicePixelRatio || 1, 2);
        canvas.width = Math.floor(viewport.width * ratio);
        canvas.height = Math.floor(viewport.height * ratio);
        canvas.style.width = `${Math.floor(viewport.width)}px`;
        canvas.style.height = `${Math.floor(viewport.height)}px`;
        const scaled = pdfPage.getViewport({ scale: 1.25 * zoom * ratio });
        task = pdfPage.render({ canvas, viewport: scaled });
        await task.promise;
      } catch (caught) {
        if (!cancelled && (caught as { name?: string }).name !== "RenderingCancelledException") {
          setError(errorMessage(caught));
        }
      } finally {
        if (!cancelled) setRendering(false);
      }
    }
    void renderPage();
    return () => {
      cancelled = true;
      task?.cancel();
    };
  }, [pdf, page, zoom]);

  return (
    <div className="flex h-full min-h-[480px] flex-col overflow-hidden rounded-lg border border-line bg-[#2a2b30]">
      <div className="flex flex-wrap items-center gap-2 border-b border-white/10 px-3 py-2 text-white">
        <div className="flex min-w-0 items-center gap-2">
          <FileText className="size-4 shrink-0 text-white/60" />
          <p className="max-w-[240px] truncate text-xs font-medium">{filename}</p>
        </div>
        <div className="ml-auto flex items-center gap-1">
          {isPdf ? (
            <>
              <button className="grid size-7 place-items-center rounded text-white/70 hover:bg-white/10 hover:text-white disabled:opacity-30" onClick={() => setPage((value) => Math.max(1, value - 1))} disabled={page <= 1} aria-label="Previous page">
                <ChevronLeft className="size-4" />
              </button>
              <span className="tabular min-w-14 text-center text-[11px] text-white/75">{page} / {pages}</span>
              <button className="grid size-7 place-items-center rounded text-white/70 hover:bg-white/10 hover:text-white disabled:opacity-30" onClick={() => setPage((value) => Math.min(pages, value + 1))} disabled={page >= pages} aria-label="Next page">
                <ChevronRight className="size-4" />
              </button>
              <span className="mx-1 h-4 w-px bg-white/15" />
              <button className="grid size-7 place-items-center rounded text-white/70 hover:bg-white/10 hover:text-white" onClick={() => setZoom((value) => Math.max(0.5, value - 0.2))} aria-label="Zoom out">
                <Minus className="size-3.5" />
              </button>
              <span className="tabular w-10 text-center text-[10px] text-white/60">{formatDecimal(String(zoom), 1)}×</span>
              <button className="grid size-7 place-items-center rounded text-white/70 hover:bg-white/10 hover:text-white" onClick={() => setZoom((value) => Math.min(3, value + 0.2))} aria-label="Zoom in">
                <Plus className="size-3.5" />
              </button>
              <button className="grid size-7 place-items-center rounded text-white/70 hover:bg-white/10 hover:text-white" onClick={() => setZoom(1)} aria-label="Reset zoom">
                <RotateCw className="size-3.5" />
              </button>
            </>
          ) : null}
          {onDownload ? (
            <button className="grid size-7 place-items-center rounded text-white/70 hover:bg-white/10 hover:text-white" onClick={onDownload} aria-label="Download original">
              <Download className="size-3.5" />
            </button>
          ) : null}
        </div>
      </div>
      <div className="relative flex flex-1 items-start justify-center overflow-auto p-4">
        {loading ? (
          <div className="flex h-64 flex-col items-center justify-center gap-3 text-white/60">
            <Loader2 className="size-6 animate-spin" />
            <p className="text-xs">Loading secure document preview…</p>
          </div>
        ) : error ? (
          <div className="m-auto max-w-sm text-center text-sm text-white/70">
            <p>{error}</p>
            {onDownload ? <Button className="mt-4" variant="outline" size="sm" onClick={onDownload}><Download className="size-3.5" /> Download original</Button> : null}
          </div>
        ) : isPdf ? (
          <div className="relative">
            <canvas ref={canvasRef} className="max-w-full bg-white shadow-2xl" aria-label={`Page ${page} of ${pages}`} />
            {rendering ? <div className="absolute inset-0 grid place-items-center bg-white/30"><Loader2 className="size-5 animate-spin text-indigo" /></div> : null}
          </div>
        ) : blobUrl ? (
          // eslint-disable-next-line @next/next/no-img-element
          <img src={blobUrl} alt={`Preview of ${filename}`} className="max-w-full bg-white shadow-2xl" />
        ) : null}
      </div>
      <div className="border-t border-white/10 px-3 py-2 text-[10px] text-white/40">
        Preview rendered locally in the browser. Embedded PDF scripts and actions are not executed.
      </div>
    </div>
  );
}
