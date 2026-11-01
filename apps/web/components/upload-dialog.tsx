"use client";

import { useQueryClient } from "@tanstack/react-query";
import { FileText, UploadCloud, X } from "lucide-react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import * as React from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogFooter } from "@/components/ui/dialog";
import { apiFetch, errorMessage } from "@/lib/api";
import { cn } from "@/lib/cn";
import { formatDecimal } from "@/lib/format";

const MAX_FILES = 20;
const MAX_BYTES = 20 * 1024 * 1024;

interface UploadResponse {
  documents: Array<{
    document: { id: string; filename: string; status: string };
  }>;
  errors: Array<{ filename?: string; code?: string; message?: string }>;
  batch_limit: number;
}

function sizeLabel(bytes: number) {
  if (bytes > 1024 * 1024) return `${formatDecimal(String(bytes / 1024 / 1024), 1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

export function UploadDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const [files, setFiles] = React.useState<File[]>([]);
  const [dragging, setDragging] = React.useState(false);
  const [busy, setBusy] = React.useState(false);
  const [result, setResult] = React.useState<UploadResponse | null>(null);
  const inputRef = React.useRef<HTMLInputElement>(null);
  const queryClient = useQueryClient();
  const router = useRouter();

  function reset() {
    setFiles([]);
    setResult(null);
    setBusy(false);
    if (inputRef.current) inputRef.current.value = "";
  }

  function addFiles(incoming: FileList | File[]) {
    const accepted = Array.from(incoming).filter((file) => {
      if (file.size > MAX_BYTES) {
        toast.error(`${file.name} is larger than 20 MB.`);
        return false;
      }
      const extension = file.name.split(".").pop()?.toLowerCase();
      if (!extension || !["pdf", "png", "jpg", "jpeg", "tif", "tiff"].includes(extension)) {
        toast.error(`${file.name} is not a supported PDF or image.`);
        return false;
      }
      return true;
    });
    setFiles((current) => {
      const combined = [...current, ...accepted];
      if (combined.length > MAX_FILES) {
        toast.error(`You can upload at most ${MAX_FILES} files at a time.`);
      }
      return combined.slice(0, MAX_FILES);
    });
    setResult(null);
  }

  async function upload() {
    if (!files.length) return;
    setBusy(true);
    const form = new FormData();
    for (const file of files) form.append("files", file, file.name);
    try {
      const data = await apiFetch<UploadResponse>("/documents", {
        method: "POST",
        rawBody: form,
      });
      setResult(data);
      setFiles([]);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["documents"] }),
        queryClient.invalidateQueries({ queryKey: ["invoices"] }),
        queryClient.invalidateQueries({ queryKey: ["dashboard"] }),
      ]);
      if (data.documents.length) {
        toast.success(
          `${data.documents.length} document${data.documents.length === 1 ? "" : "s"} queued for processing.`,
        );
      }
      if (data.errors.length) {
        toast.warning(`${data.errors.length} file${data.errors.length === 1 ? "" : "s"} could not be accepted.`);
      }
    } catch (error) {
      toast.error(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && busy) return;
        onOpenChange(next);
        if (!next) reset();
      }}
    >
      <DialogContent
        title="Add documents"
        description="Upload supplier invoices as PDF, PNG, JPEG, or TIFF. Files are processed in the background and always require review."
        className="max-w-2xl"
      >
        <div className="space-y-5 px-6 py-5">
          {!result ? (
            <>
              <div
                role="button"
                tabIndex={0}
                onClick={() => inputRef.current?.click()}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") inputRef.current?.click();
                }}
                onDragOver={(event) => {
                  event.preventDefault();
                  setDragging(true);
                }}
                onDragLeave={() => setDragging(false)}
                onDrop={(event) => {
                  event.preventDefault();
                  setDragging(false);
                  addFiles(event.dataTransfer.files);
                }}
                className={cn(
                  "flex cursor-pointer flex-col items-center justify-center rounded-lg border border-dashed px-6 py-12 text-center transition-colors",
                  dragging
                    ? "border-indigo bg-indigo-soft/60"
                    : "border-line-strong bg-paper/45 hover:border-indigo/50 hover:bg-indigo-soft/30",
                )}
              >
                <div className="grid size-12 place-items-center rounded-full border border-line bg-surface text-indigo">
                  <UploadCloud className="size-5" aria-hidden />
                </div>
                <p className="mt-4 text-sm font-semibold text-ink">Drop invoices here, or choose files</p>
                <p className="mt-1 text-xs text-muted">Up to 20 files · 20 MB each · PDF, PNG, JPEG, TIFF</p>
                <input
                  ref={inputRef}
                  type="file"
                  multiple
                  accept=".pdf,.png,.jpg,.jpeg,.tif,.tiff,application/pdf,image/png,image/jpeg,image/tiff"
                  className="sr-only"
                  onChange={(event) => {
                    if (event.target.files) addFiles(event.target.files);
                  }}
                />
              </div>

              {files.length ? (
                <div className="space-y-2">
                  <p className="label-caps">Ready to upload</p>
                  <div className="max-h-52 divide-y divide-line overflow-y-auto rounded-md border border-line bg-surface">
                    {files.map((file, index) => (
                      <div key={`${file.name}-${index}`} className="flex items-center gap-3 px-3 py-2.5">
                        <FileText className="size-4 shrink-0 text-indigo" aria-hidden />
                        <div className="min-w-0 flex-1">
                          <p className="truncate text-sm font-medium text-ink">{file.name}</p>
                          <p className="text-[11px] text-muted">{sizeLabel(file.size)}</p>
                        </div>
                        <button
                          type="button"
                          className="grid size-7 place-items-center rounded text-muted hover:bg-paper-deep hover:text-ink"
                          aria-label={`Remove ${file.name}`}
                          onClick={() => setFiles((current) => current.filter((_, i) => i !== index))}
                        >
                          <X className="size-3.5" />
                        </button>
                      </div>
                    ))}
                  </div>
                </div>
              ) : null}
            </>
          ) : (
            <div className="space-y-5">
              <div className="rounded-lg border border-moss/25 bg-moss-soft px-4 py-3">
                <p className="text-sm font-semibold text-moss">
                  {result.documents.length} document{result.documents.length === 1 ? "" : "s"} accepted
                </p>
                <p className="mt-1 text-xs leading-5 text-moss/80">
                  Processing usually takes a few seconds. Open a document to follow extraction and review status.
                </p>
              </div>
              {result.errors.length ? (
                <div className="rounded-lg border border-amber/30 bg-amber-soft px-4 py-3">
                  <p className="text-sm font-semibold text-amber">Some files were rejected</p>
                  <ul className="mt-2 space-y-1.5 text-xs leading-5 text-amber/90">
                    {result.errors.map((error, index) => (
                      <li key={index}>
                        {error.filename ? <strong>{error.filename}: </strong> : null}
                        {error.message ?? "Unsupported or unreadable file."}
                      </li>
                    ))}
                  </ul>
                </div>
              ) : null}
              <div className="max-h-52 divide-y divide-line overflow-y-auto rounded-md border border-line">
                {result.documents.map(({ document }) => (
                  <div key={document.id} className="flex items-center justify-between gap-4 px-4 py-3">
                    <div className="min-w-0">
                      <p className="truncate text-sm font-medium text-ink">{document.filename}</p>
                      <p className="text-[11px] capitalize text-muted">{document.status.replaceAll("_", " ")}</p>
                    </div>
                    <Link
                      href={`/documents/${document.id}`}
                      className="text-xs font-semibold text-indigo hover:underline"
                      onClick={() => onOpenChange(false)}
                    >
                      View
                    </Link>
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
        <DialogFooter>
          {result ? (
            <>
              <Button variant="outline" onClick={reset}>
                Upload more
              </Button>
              <Button
                onClick={() => {
                  onOpenChange(false);
                  router.push("/documents");
                }}
              >
                View documents
              </Button>
            </>
          ) : (
            <>
              <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={busy}>
                Cancel
              </Button>
              <Button onClick={upload} loading={busy} disabled={!files.length}>
                Upload {files.length ? `${files.length} file${files.length === 1 ? "" : "s"}` : ""}
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
