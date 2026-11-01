import { Badge, type BadgeTone } from "@/components/ui/badge";
import { titleCase } from "@/lib/format";

const invoiceTones: Record<string, BadgeTone> = {
  review_required: "amber",
  ready_for_approval: "indigo",
  approved: "moss",
  exported: "moss",
  rejected: "rust",
  failed: "rust",
  cancelled: "neutral",
  processing: "indigo",
};

const documentTones: Record<string, BadgeTone> = {
  uploaded: "neutral",
  queued: "neutral",
  extracting_text: "indigo",
  running_ocr: "indigo",
  extracting_fields: "indigo",
  validating: "indigo",
  review_required: "amber",
  ready_for_approval: "indigo",
  approved: "moss",
  exported: "moss",
  rejected: "rust",
  failed: "rust",
  cancelled: "neutral",
};

export function StatusBadge({ status, kind = "invoice" }: { status: string; kind?: "invoice" | "document" }) {
  const tones = kind === "document" ? documentTones : invoiceTones;
  return <Badge tone={tones[status] ?? "neutral"}>{titleCase(status)}</Badge>;
}

export function SeverityBadge({ severity }: { severity: string }) {
  const tone: BadgeTone = severity === "error" ? "rust" : severity === "warning" ? "amber" : "neutral";
  return <Badge tone={tone}>{titleCase(severity)}</Badge>;
}

export function MockBadge({ compact = false }: { compact?: boolean }) {
  return (
    <Badge tone="indigo" className="uppercase tracking-[0.08em]">
      <span className="size-1.5 rounded-full bg-indigo" aria-hidden />
      {compact ? "Mock" : "Mock extraction"}
    </Badge>
  );
}
