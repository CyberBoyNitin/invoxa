"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Bot, Building2, CheckCircle2, FlaskConical, ShieldCheck } from "lucide-react";
import * as React from "react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle, Skeleton } from "@/components/ui/card";
import { Checkbox, Field, Input, Select } from "@/components/ui/input";
import { apiFetch, errorMessage } from "@/lib/api";
import type { Organization, Role } from "@/lib/types";

interface OrganizationResponse extends Organization {
  role: Role | null;
  permissions: string[];
  created_at: string;
  updated_at: string;
}
interface HealthResponse {
  provider: string;
  provider_is_mock: boolean;
  storage: string;
  environment: string;
  version: string;
}
interface ValidationSettings {
  require_invoice_date?: boolean;
  require_po_number?: boolean;
  require_vendor_tax_id?: boolean;
  require_gstin_for_india?: boolean;
  amount_tolerance?: string;
  max_due_date_days?: number;
}

export default function OrganizationSettingsPage() {
  const queryClient = useQueryClient();
  const organization = useQuery({
    queryKey: ["organization"],
    queryFn: () => apiFetch<OrganizationResponse>("/organization"),
  });
  const health = useQuery({
    queryKey: ["health"],
    queryFn: () => apiFetch<HealthResponse>("/health"),
    retry: false,
  });

  const [name, setName] = React.useState("");
  const [country, setCountry] = React.useState("");
  const [currency, setCurrency] = React.useState("");
  const [validation, setValidation] = React.useState<ValidationSettings>({});
  const [minOcrQuality, setMinOcrQuality] = React.useState("0.55");
  const [consent, setConsent] = React.useState(false);
  const [busy, setBusy] = React.useState<"identity" | "policy" | null>(null);

  React.useEffect(() => {
    if (!organization.data) return;
    const data = organization.data;
    setName(data.name);
    setCountry(data.country);
    setCurrency(data.base_currency);
    const stored = (data.settings ?? {}) as {
      validation?: ValidationSettings;
      extraction?: { min_ocr_quality?: string };
      ai?: { provider_consent?: boolean };
    };
    setValidation(stored.validation ?? {});
    setMinOcrQuality(stored.extraction?.min_ocr_quality ?? "0.55");
    setConsent(Boolean(stored.ai?.provider_consent));
  }, [organization.data]);

  async function saveIdentity(event: React.FormEvent) {
    event.preventDefault();
    setBusy("identity");
    try {
      await apiFetch("/organization", {
        method: "PATCH",
        body: { name: name.trim(), country: country.trim().toUpperCase(), base_currency: currency.trim().toUpperCase() },
      });
      await queryClient.invalidateQueries({ queryKey: ["organization"] });
      toast.success("Organization identity updated.");
    } catch (caught) {
      toast.error(errorMessage(caught));
    } finally {
      setBusy(null);
    }
  }

  async function savePolicy(event: React.FormEvent) {
    event.preventDefault();
    setBusy("policy");
    try {
      await apiFetch("/organization", {
        method: "PATCH",
        body: {
          name: name.trim(),
          country: country.trim().toUpperCase(),
          base_currency: currency.trim().toUpperCase(),
          settings: {
            validation,
            extraction: { min_ocr_quality: minOcrQuality },
            ai: { provider_consent: consent },
          },
        },
      });
      await queryClient.invalidateQueries({ queryKey: ["organization"] });
      toast.success("Validation and AI policy updated.");
    } catch (caught) {
      toast.error(errorMessage(caught));
    } finally {
      setBusy(null);
    }
  }

  if (organization.isLoading) {
    return <div className="space-y-4"><Skeleton className="h-52" /><Skeleton className="h-80" /></div>;
  }
  if (organization.isError || !organization.data) {
    return <div className="rounded-lg border border-rust/25 bg-rust-soft p-5 text-sm text-rust">{organization.error ? errorMessage(organization.error) : "Organization not found."}</div>;
  }
  const org = organization.data;
  const canManage = org.permissions.includes("organization:write");
  const isOwner = org.role === "owner";

  return (
    <div className="grid items-start gap-5 xl:grid-cols-[minmax(0,1fr)_360px]">
      <div className="space-y-5">
        <Card>
          <CardHeader>
            <div><CardTitle>Organization identity</CardTitle><p className="mt-1 text-xs text-muted">Used across the workspace and export metadata.</p></div>
            <Building2 className="size-4 text-muted" />
          </CardHeader>
          <CardContent>
            <form onSubmit={saveIdentity} className="space-y-4">
              <div className="grid gap-4 sm:grid-cols-2">
                <Field label="Legal name" htmlFor="org-name" className="sm:col-span-2"><Input id="org-name" required minLength={2} maxLength={160} value={name} onChange={(event) => setName(event.target.value)} disabled={!canManage} /></Field>
                <Field label="Country" htmlFor="org-country" hint="Two-letter ISO code"><Input id="org-country" required minLength={2} maxLength={2} value={country} onChange={(event) => setCountry(event.target.value.toUpperCase())} disabled={!canManage} /></Field>
                <Field label="Base currency" htmlFor="org-currency" hint="ISO 4217"><Input id="org-currency" required minLength={3} maxLength={3} value={currency} onChange={(event) => setCurrency(event.target.value.toUpperCase())} disabled={!canManage} /></Field>
              </div>
              <div className="flex items-center justify-between gap-3 border-t border-line pt-4">
                <p className="text-[11px] text-muted">Slug: <code className="font-mono">{org.slug}</code>{org.is_demo ? " · demo workspace" : ""}</p>
                <Button type="submit" loading={busy === "identity"} disabled={!canManage}>Save identity</Button>
              </div>
            </form>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <div><CardTitle>Validation policy</CardTitle><p className="mt-1 text-xs text-muted">Applied after every extraction and correction. Errors block approval; warnings remain visible.</p></div>
            <ShieldCheck className="size-4 text-muted" />
          </CardHeader>
          <CardContent>
            <form onSubmit={savePolicy} className="space-y-5">
              <div className="grid gap-3 sm:grid-cols-2">
                {[
                  ["require_invoice_date", "Require invoice date", "An invoice without a date becomes an error."],
                  ["require_po_number", "Require purchase order", "Useful for PO-controlled spend."],
                  ["require_vendor_tax_id", "Require vendor tax ID", "Missing tax registration blocks approval."],
                  ["require_gstin_for_india", "Require GSTIN for India", "Applies to India-country invoices."],
                ].map(([key, label, description]) => (
                  <label key={key} className="flex items-start gap-3 rounded-md border border-line bg-paper/30 px-3.5 py-3">
                    <Checkbox
                      checked={Boolean(validation[key as keyof ValidationSettings])}
                      onChange={(event) => setValidation({ ...validation, [String(key)]: event.target.checked })}
                      disabled={!canManage}
                    />
                    <span><span className="block text-xs font-semibold">{label}</span><span className="mt-0.5 block text-[11px] leading-4 text-muted">{description}</span></span>
                  </label>
                ))}
              </div>
              <div className="grid gap-4 sm:grid-cols-3">
                <Field label="Amount tolerance" htmlFor="amount-tolerance" hint="Decimal, 0–1">
                  <Input id="amount-tolerance" inputMode="decimal" value={validation.amount_tolerance ?? "0.02"} onChange={(event) => setValidation({ ...validation, amount_tolerance: event.target.value })} disabled={!canManage} />
                </Field>
                <Field label="Max due-date days" htmlFor="due-days" hint="0–3650">
                  <Input id="due-days" type="number" min={0} max={3650} value={validation.max_due_date_days ?? 365} onChange={(event) => setValidation({ ...validation, max_due_date_days: Number(event.target.value) })} disabled={!canManage} />
                </Field>
                <Field label="Minimum OCR quality" htmlFor="ocr-quality" hint="Decimal, 0–1">
                  <Input id="ocr-quality" inputMode="decimal" value={minOcrQuality} onChange={(event) => setMinOcrQuality(event.target.value)} disabled={!canManage} />
                </Field>
              </div>

              <div className="rounded-md border border-line bg-paper/30 p-4">
                <div className="flex items-start justify-between gap-3">
                  <div className="flex items-start gap-2.5">
                    <Bot className="mt-0.5 size-4 text-indigo" />
                    <div>
                      <p className="text-xs font-semibold">External AI provider consent</p>
                      <p className="mt-1 max-w-xl text-[11px] leading-5 text-muted">
                        When an external provider is configured, enabling this permits extracted invoice text to be sent outside this deployment. The mock provider does not send data anywhere. Only an owner can enable consent.
                      </p>
                    </div>
                  </div>
                  <Badge tone={health.data?.provider_is_mock ? "amber" : consent ? "moss" : "neutral"}>
                    {health.data?.provider_is_mock ? "Mock provider" : consent ? "Consent granted" : "No consent"}
                  </Badge>
                </div>
                <label className="mt-3 flex items-center gap-2 text-xs">
                  <Checkbox checked={consent} onChange={(event) => setConsent(event.target.checked)} disabled={!canManage || !isOwner} />
                  I authorize external AI processing for this organization.
                </label>
                {!isOwner ? <p className="mt-1.5 text-[10px] text-muted">Only the workspace owner can change consent.</p> : null}
              </div>

              <div className="flex justify-end border-t border-line pt-4"><Button type="submit" loading={busy === "policy"} disabled={!canManage}>Save policy</Button></div>
            </form>
          </CardContent>
        </Card>
      </div>

      <div className="space-y-5">
        <Card>
          <CardHeader><CardTitle>Runtime</CardTitle><FlaskConical className="size-4 text-muted" /></CardHeader>
          <CardContent className="space-y-3 text-xs">
            <div className="flex items-center justify-between"><span className="text-muted">Environment</span><Badge tone="neutral">{health.data?.environment ?? "—"}</Badge></div>
            <div className="flex items-center justify-between"><span className="text-muted">API version</span><span className="font-mono text-[11px]">{health.data?.version ?? "—"}</span></div>
            <div className="flex items-center justify-between"><span className="text-muted">AI provider</span><span className="font-mono text-[11px]">{health.data?.provider ?? "—"}</span></div>
            <div className="flex items-center justify-between"><span className="text-muted">Storage</span><span className="font-mono text-[11px]">{health.data?.storage ?? "—"}</span></div>
            <div className="flex items-start gap-2 rounded-md border border-line bg-paper/30 p-3 text-[11px] leading-5 text-muted">
              <CheckCircle2 className="mt-0.5 size-3.5 shrink-0 text-moss" />
              Provider credentials and base URL are deployment environment variables, never editable from the browser.
            </div>
          </CardContent>
        </Card>
        <Card>
          <CardHeader><CardTitle>Data boundary</CardTitle><ShieldCheck className="size-4 text-muted" /></CardHeader>
          <CardContent className="space-y-2.5 text-[11px] leading-5 text-muted">
            <p>Every record is scoped by organization on the server. Resource IDs from another tenant return not found.</p>
            <p>Original documents are private; previews and downloads require an authenticated session or a scoped API key.</p>
            <p>Approval is an explicit server-side transition. Automations and exports cannot approve an invoice.</p>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
