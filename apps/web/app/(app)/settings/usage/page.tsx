"use client";

import { useQuery } from "@tanstack/react-query";
import { Bot, Database, FileStack, Gauge, ScanText } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle, Skeleton } from "@/components/ui/card";
import { apiFetch, errorMessage } from "@/lib/api";
import { formatDecimal, formatMoney } from "@/lib/format";

interface UsageResponse {
  period: string;
  documents_this_month: number;
  pages_this_month: number;
  ai_requests_this_month: number;
  ocr_pages_this_month: number;
  storage_bytes: number;
  estimated_ai_cost_usd: string | null;
  billing_configured: boolean;
}

function bytesLabel(value: number) {
  if (value >= 1024 ** 3) return `${formatDecimal(String(value / 1024 ** 3), 2)} GB`;
  if (value >= 1024 ** 2) return `${formatDecimal(String(value / 1024 ** 2), 1)} MB`;
  return `${formatDecimal(String(value / 1024), 0)} KB`;
}

export default function UsageSettingsPage() {
  const usage = useQuery({
    queryKey: ["usage"],
    queryFn: () => apiFetch<UsageResponse>("/usage"),
  });

  if (usage.isLoading) return <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">{Array.from({ length: 6 }).map((_, index) => <Skeleton key={index} className="h-32" />)}</div>;
  if (usage.isError || !usage.data) return <div className="rounded-lg border border-rust/25 bg-rust-soft p-5 text-sm text-rust">{usage.error ? errorMessage(usage.error) : "Usage unavailable."}</div>;

  const data = usage.data;
  const metrics = [
    { label: "Documents this month", value: String(data.documents_this_month), icon: FileStack, note: "Uploaded source files" },
    { label: "Pages processed", value: String(data.pages_this_month), icon: ScanText, note: "Text and OCR pages" },
    { label: "OCR pages", value: String(data.ocr_pages_this_month), icon: ScanText, note: "Rasterized or image pages" },
    { label: "AI requests", value: String(data.ai_requests_this_month), icon: Bot, note: "Provider extraction calls" },
    { label: "Storage", value: bytesLabel(data.storage_bytes), icon: Database, note: "Private source documents" },
    { label: "Estimated AI cost", value: data.estimated_ai_cost_usd ? formatMoney(data.estimated_ai_cost_usd, "USD") : "Not configured", icon: Gauge, note: "Provider pricing not set" },
  ];

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-line bg-surface px-4 py-3">
        <div><p className="text-xs font-semibold">Current calendar month</p><p className="mt-0.5 text-[11px] text-muted">Counts come from tenant-scoped operational records, not estimates.</p></div>
        <Badge tone={data.billing_configured ? "moss" : "neutral"}>{data.billing_configured ? "Billing connected" : "No billing integration"}</Badge>
      </div>
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        {metrics.map((metric) => {
          const Icon = metric.icon;
          return (
            <Card key={metric.label}>
              <CardContent className="py-5">
                <div className="flex items-start justify-between"><p className="label-caps">{metric.label}</p><Icon className="size-4 text-muted" /></div>
                <p className="mt-4 font-display text-3xl tracking-[-0.02em]">{metric.value}</p>
                <p className="mt-1 text-[11px] text-muted">{metric.note}</p>
              </CardContent>
            </Card>
          );
        })}
      </div>
      <Card>
        <CardHeader><CardTitle>Metering honesty</CardTitle></CardHeader>
        <CardContent className="space-y-2 text-[11px] leading-5 text-muted">
          <p>AI cost is reported only when provider pricing is configured in the deployment. Invoxa does not invent a cost estimate.</p>
          <p>Usage counters are for workspace visibility; this build has no payment processing, plan enforcement, or invoice generation.</p>
          <p>Storage measures original documents in private object storage. Derived export artifacts are tracked separately by expiry.</p>
        </CardContent>
      </Card>
    </div>
  );
}
