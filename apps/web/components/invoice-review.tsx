"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertCircle,
  AlertTriangle,
  ArrowLeft,
  Check,
  CheckCircle2,
  Clock3,
  Download,
  FileText,
  History,
  Info,
  ListTree,
  MessageSquare,
  Plus,
  ReceiptText,
  RefreshCw,
  ShieldAlert,
  Trash2,
  Undo2,
  X,
} from "lucide-react";
import Link from "next/link";
import { useParams } from "next/navigation";
import * as React from "react";
import { toast } from "sonner";
import { PdfViewer } from "@/components/pdf-viewer";
import { MockBadge, SeverityBadge, StatusBadge } from "@/components/status-badge";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle, Separator, Skeleton } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter } from "@/components/ui/dialog";
import { Checkbox, Field, Input, Select, Textarea } from "@/components/ui/input";
import { Table, TBody, TD, TH, THead, TR } from "@/components/ui/table";
import { apiFetch, ApiError, errorMessage } from "@/lib/api";
import { cn } from "@/lib/cn";
import { formatDate, formatDateTime, formatDecimal, formatMoney, titleCase } from "@/lib/format";
import type { InvoiceDetail, LineItem, TaxLine } from "@/lib/types";

const editableTextFields = [
  ["invoice_number", "Invoice number", "text"],
  ["invoice_date", "Invoice date", "date"],
  ["due_date", "Due date", "date"],
  ["purchase_order", "Purchase order", "text"],
  ["payment_terms", "Payment terms", "text"],
] as const;

const amountFields = [
  ["subtotal", "Subtotal"],
  ["discount", "Discount"],
  ["shipping", "Shipping / charges"],
  ["tax_total", "Total tax"],
  ["total", "Grand total"],
  ["amount_paid", "Amount paid"],
  ["balance_due", "Balance due"],
] as const;

function valueOrEmpty(value: string | null | undefined) {
  return value ?? "";
}

interface FieldForm {
  vendor_name: string;
  currency: string;
  invoice_number: string;
  invoice_date: string;
  due_date: string;
  purchase_order: string;
  payment_terms: string;
  subtotal: string;
  discount: string;
  shipping: string;
  tax_total: string;
  total: string;
  amount_paid: string;
  balance_due: string;
  is_credit_note: boolean;
}

function formFromInvoice(invoice: InvoiceDetail): FieldForm {
  return {
    vendor_name: valueOrEmpty(invoice.vendor_name),
    currency: valueOrEmpty(invoice.currency),
    invoice_number: valueOrEmpty(invoice.invoice_number),
    invoice_date: valueOrEmpty(invoice.invoice_date),
    due_date: valueOrEmpty(invoice.due_date),
    purchase_order: valueOrEmpty(invoice.purchase_order),
    payment_terms: valueOrEmpty(invoice.payment_terms),
    subtotal: valueOrEmpty(invoice.subtotal),
    discount: valueOrEmpty(invoice.discount),
    shipping: valueOrEmpty(invoice.shipping),
    tax_total: valueOrEmpty(invoice.tax_total),
    total: valueOrEmpty(invoice.total),
    amount_paid: valueOrEmpty(invoice.amount_paid),
    balance_due: valueOrEmpty(invoice.balance_due),
    is_credit_note: invoice.is_credit_note,
  };
}

function Detail({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div>
      <dt className="label-caps">{label}</dt>
      <dd className="mt-1 text-sm leading-6 text-ink">{value}</dd>
    </div>
  );
}

function Tabs({
  value,
  onChange,
  items,
}: {
  value: string;
  onChange: (value: string) => void;
  items: Array<{ id: string; label: string; count?: number }>;
}) {
  return (
    <div role="tablist" className="flex gap-1 overflow-x-auto border-b border-line px-2">
      {items.map((item) => (
        <button
          key={item.id}
          role="tab"
          aria-selected={value === item.id}
          onClick={() => onChange(item.id)}
          className={cn(
            "relative whitespace-nowrap px-3 py-3 text-xs font-semibold transition-colors",
            value === item.id ? "text-ink" : "text-muted hover:text-ink",
          )}
        >
          {item.label}
          {item.count ? <span className="ml-1.5 rounded-full bg-paper-deep px-1.5 py-0.5 text-[10px]">{item.count}</span> : null}
          {value === item.id ? <span className="absolute inset-x-2 bottom-0 h-0.5 rounded-full bg-indigo" /> : null}
        </button>
      ))}
    </div>
  );
}

function FieldEditor({ invoice, onSaved }: { invoice: InvoiceDetail; onSaved: (next: InvoiceDetail) => void }) {
  const [form, setForm] = React.useState<FieldForm>(() => formFromInvoice(invoice));
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState("");
  const queryClient = useQueryClient();

  React.useEffect(() => {
    setForm(formFromInvoice(invoice));
  }, [invoice]);

  function set(key: string, value: string | boolean) {
    setForm((current) => ({ ...current, [key]: value }));
  }

  async function save() {
    setBusy(true);
    setError("");
    const changes: Record<string, unknown> = { version: invoice.version };
    const original: Record<string, string> = {
      vendor_name: valueOrEmpty(invoice.vendor_name),
      currency: valueOrEmpty(invoice.currency),
      ...Object.fromEntries(editableTextFields.map(([field]) => [field, valueOrEmpty(invoice[field] as string | null)])),
      ...Object.fromEntries(amountFields.map(([field]) => [field, valueOrEmpty(invoice[field] as string | null)])),
    };
    for (const [key, value] of Object.entries(form)) {
      if (key === "is_credit_note") {
        if (value !== invoice.is_credit_note) changes[key] = value;
      } else if (String(value).trim() !== original[key]) {
        changes[key] = String(value).trim() === "" ? null : String(value).trim();
      }
    }
    if (Object.keys(changes).length === 1) {
      toast.info("No field changes to save.");
      setBusy(false);
      return;
    }
    try {
      const next = await apiFetch<InvoiceDetail>(`/invoices/${invoice.id}`, { method: "PATCH", body: changes });
      onSaved(next);
      queryClient.setQueryData(["invoice", invoice.id], next);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["invoices"] }),
        queryClient.invalidateQueries({ queryKey: ["dashboard"] }),
      ]);
      toast.success("Invoice corrections saved. Validation was recalculated.");
    } catch (caught) {
      setError(caught instanceof ApiError && caught.status === 409 ? "Another person changed this invoice. Reload the page before saving." : errorMessage(caught));
    } finally {
      setBusy(false);
    }
  }

  const issueFor = (field: string) =>
    invoice.issues.find((issue) => !issue.resolved_at && (issue.field === field || issue.field?.startsWith(`${field}.`)));

  return (
    <div className="space-y-5 p-5">
      {error ? <div className="rounded-md border border-rust/25 bg-rust-soft px-3.5 py-3 text-xs leading-5 text-rust">{error}</div> : null}
      <div className="grid gap-4 sm:grid-cols-2">
        <Field label="Vendor name" htmlFor="vendor_name" className="sm:col-span-2" error={issueFor("vendor.name")?.message}>
          <Input id="vendor_name" value={form.vendor_name} onChange={(event) => set("vendor_name", event.target.value)} />
        </Field>
        <Field label="Invoice number" htmlFor="invoice_number" error={issueFor("invoice_number")?.message}>
          <Input id="invoice_number" value={String(form["invoice_number"] ?? "")} onChange={(event) => set("invoice_number", event.target.value)} />
        </Field>
        <Field label="Currency" htmlFor="currency" hint="ISO code" error={issueFor("currency")?.message}>
          <Input id="currency" maxLength={3} value={form.currency} onChange={(event) => set("currency", event.target.value.toUpperCase())} />
        </Field>
        {editableTextFields.slice(1).map(([field, label, type]) => (
          <Field key={field} label={label} htmlFor={field} error={issueFor(field)?.message}>
            <Input id={field} type={type} value={String(form[field as keyof typeof form] ?? "")} onChange={(event) => set(field, event.target.value)} />
          </Field>
        ))}
      </div>

      <div className="rounded-md border border-line bg-paper/35 p-4">
        <div className="flex items-center justify-between">
          <p className="text-xs font-semibold text-ink">Amounts</p>
          <p className="text-[10px] uppercase tracking-[0.1em] text-muted">Exact decimal strings</p>
        </div>
        <div className="mt-4 grid gap-4 sm:grid-cols-2">
          {amountFields.map(([field, label]) => (
            <Field key={field} label={label} htmlFor={field} error={issueFor(field)?.message}>
              <Input
                id={field}
                inputMode="decimal"
                value={String(form[field as keyof typeof form] ?? "")}
                onChange={(event) => set(field, event.target.value)}
                placeholder="Leave blank if not present"
              />
            </Field>
          ))}
        </div>
      </div>

      <label className="flex items-start gap-3 rounded-md border border-line px-4 py-3">
        <Checkbox checked={form.is_credit_note} onChange={(event) => set("is_credit_note", event.target.checked)} />
        <span>
          <span className="block text-xs font-semibold text-ink">Credit note</span>
          <span className="mt-0.5 block text-[11px] text-muted">Enable only when the source document explicitly identifies a credit note.</span>
        </span>
      </label>

      <div className="flex items-center justify-between gap-3 border-t border-line pt-4">
        <p className="text-[11px] text-muted">Saving creates an audit event and re-runs deterministic validation.</p>
        <Button onClick={save} loading={busy} disabled={!invoice.available_actions.edit}>Save corrections</Button>
      </div>
    </div>
  );
}

function ItemsEditor({ invoice, onSaved }: { invoice: InvoiceDetail; onSaved: (next: InvoiceDetail) => void }) {
  const [lines, setLines] = React.useState<LineItem[]>(invoice.line_items);
  const [taxes, setTaxes] = React.useState<TaxLine[]>(invoice.tax_lines);
  const [busy, setBusy] = React.useState(false);
  const queryClient = useQueryClient();

  React.useEffect(() => {
    setLines(invoice.line_items);
    setTaxes(invoice.tax_lines);
  }, [invoice]);

  function updateLine(index: number, key: keyof LineItem, value: string) {
    setLines((current) => current.map((line, i) => (i === index ? { ...line, [key]: value.trim() === "" ? null : value } : line)));
  }
  function updateTax(index: number, key: keyof TaxLine, value: string) {
    setTaxes((current) => current.map((line, i) => (i === index ? { ...line, [key]: value.trim() === "" ? null : value } : line)));
  }
  function addLine() {
    setLines((current) => [...current, { description: "", sku: null, quantity: null, unit_of_measure: null, unit_price: null, discount: null, tax_rate: null, tax_amount: null, line_total: null, source_page: null }]);
  }
  function addTax() {
    setTaxes((current) => [...current, { tax_type: "", rate: null, taxable_amount: null, amount: null, source_page: null }]);
  }

  async function save() {
    setBusy(true);
    try {
      const next = await apiFetch<InvoiceDetail>(`/invoices/${invoice.id}`, {
        method: "PATCH",
        body: {
          version: invoice.version,
          line_items: lines.map((line) => ({
            description: line.description,
            sku: line.sku,
            quantity: line.quantity,
            unit_of_measure: line.unit_of_measure,
            unit_price: line.unit_price,
            discount: line.discount,
            tax_rate: line.tax_rate,
            tax_amount: line.tax_amount,
            line_total: line.line_total,
            source_page: line.source_page ?? null,
            raw_text: line.raw_text ?? null,
          })),
          tax_lines: taxes.map((line) => ({
            tax_type: line.tax_type,
            rate: line.rate,
            taxable_amount: line.taxable_amount,
            amount: line.amount,
            source_page: line.source_page ?? null,
          })),
        },
      });
      onSaved(next);
      queryClient.setQueryData(["invoice", invoice.id], next);
      await queryClient.invalidateQueries({ queryKey: ["invoices"] });
      toast.success("Line items and tax lines saved.");
    } catch (caught) {
      toast.error(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="space-y-6 p-5">
      <div>
        <div className="flex items-center justify-between gap-3">
          <div>
            <h3 className="text-sm font-semibold">Line items</h3>
            <p className="mt-0.5 text-[11px] text-muted">Line totals are pre-tax. Blank values stay blank.</p>
          </div>
          <Button size="sm" variant="outline" onClick={addLine}><Plus className="size-3.5" /> Add line</Button>
        </div>
        <div className="mt-3 overflow-x-auto rounded-md border border-line">
          <table className="w-full min-w-[900px] text-left text-xs">
            <thead className="border-b border-line bg-paper/60 text-[10px] uppercase tracking-wide text-muted">
              <tr>
                <th className="px-2 py-2">Description</th>
                <th className="px-2 py-2">SKU</th>
                <th className="w-20 px-2 py-2">Qty</th>
                <th className="w-16 px-2 py-2">UOM</th>
                <th className="w-28 px-2 py-2">Unit price</th>
                <th className="w-24 px-2 py-2">Tax rate</th>
                <th className="w-28 px-2 py-2">Line total</th>
                <th className="w-9 px-1 py-2" />
              </tr>
            </thead>
            <tbody className="divide-y divide-line">
              {lines.map((line, index) => (
                <tr key={line.id ?? index}>
                  <td className="p-1.5"><Input className="h-8 min-w-[180px] text-xs" value={line.description ?? ""} onChange={(event) => updateLine(index, "description", event.target.value)} /></td>
                  <td className="p-1.5"><Input className="h-8 w-24 text-xs" value={line.sku ?? ""} onChange={(event) => updateLine(index, "sku", event.target.value)} /></td>
                  <td className="p-1.5"><Input className="tabular h-8 w-20 text-right text-xs" inputMode="decimal" value={line.quantity ?? ""} onChange={(event) => updateLine(index, "quantity", event.target.value)} /></td>
                  <td className="p-1.5"><Input className="h-8 w-16 text-xs" value={line.unit_of_measure ?? ""} onChange={(event) => updateLine(index, "unit_of_measure", event.target.value)} /></td>
                  <td className="p-1.5"><Input className="tabular h-8 w-28 text-right text-xs" inputMode="decimal" value={line.unit_price ?? ""} onChange={(event) => updateLine(index, "unit_price", event.target.value)} /></td>
                  <td className="p-1.5"><Input className="tabular h-8 w-24 text-right text-xs" inputMode="decimal" value={line.tax_rate ?? ""} onChange={(event) => updateLine(index, "tax_rate", event.target.value)} /></td>
                  <td className="p-1.5"><Input className="tabular h-8 w-28 text-right text-xs" inputMode="decimal" value={line.line_total ?? ""} onChange={(event) => updateLine(index, "line_total", event.target.value)} /></td>
                  <td className="p-1.5"><button className="grid size-7 place-items-center rounded text-muted hover:bg-rust-soft hover:text-rust" onClick={() => setLines((current) => current.filter((_, i) => i !== index))} aria-label="Remove line"><Trash2 className="size-3.5" /></button></td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>

      <div>
        <div className="flex items-center justify-between gap-3">
          <div>
            <h3 className="text-sm font-semibold">Tax lines</h3>
            <p className="mt-0.5 text-[11px] text-muted">CGST, SGST, IGST, VAT, or other taxes. Do not add a summary line.</p>
          </div>
          <Button size="sm" variant="outline" onClick={addTax}><Plus className="size-3.5" /> Add tax</Button>
        </div>
        <div className="mt-3 overflow-x-auto rounded-md border border-line">
          <table className="w-full min-w-[620px] text-left text-xs">
            <thead className="border-b border-line bg-paper/60 text-[10px] uppercase tracking-wide text-muted">
              <tr>
                <th className="px-2 py-2">Tax type</th>
                <th className="w-24 px-2 py-2">Rate</th>
                <th className="w-32 px-2 py-2">Taxable amount</th>
                <th className="w-28 px-2 py-2">Tax amount</th>
                <th className="w-9 px-1 py-2" />
              </tr>
            </thead>
            <tbody className="divide-y divide-line">
              {taxes.map((line, index) => (
                <tr key={line.id ?? index}>
                  <td className="p-1.5"><Input className="h-8 text-xs" value={line.tax_type ?? ""} onChange={(event) => updateTax(index, "tax_type", event.target.value)} placeholder="CGST" /></td>
                  <td className="p-1.5"><Input className="tabular h-8 text-right text-xs" inputMode="decimal" value={line.rate ?? ""} onChange={(event) => updateTax(index, "rate", event.target.value)} /></td>
                  <td className="p-1.5"><Input className="tabular h-8 text-right text-xs" inputMode="decimal" value={line.taxable_amount ?? ""} onChange={(event) => updateTax(index, "taxable_amount", event.target.value)} /></td>
                  <td className="p-1.5"><Input className="tabular h-8 text-right text-xs" inputMode="decimal" value={line.amount ?? ""} onChange={(event) => updateTax(index, "amount", event.target.value)} /></td>
                  <td className="p-1.5"><button className="grid size-7 place-items-center rounded text-muted hover:bg-rust-soft hover:text-rust" onClick={() => setTaxes((current) => current.filter((_, i) => i !== index))} aria-label="Remove tax line"><Trash2 className="size-3.5" /></button></td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>

      <div className="flex items-center justify-between gap-3 border-t border-line pt-4">
        <p className="text-[11px] text-muted">Subtotal, tax, and total fields are edited on the Fields tab.</p>
        <Button onClick={save} loading={busy} disabled={!invoice.available_actions.edit}>Save items</Button>
      </div>
    </div>
  );
}

function Evidence({ invoice }: { invoice: InvoiceDetail }) {
  if (!invoice.provenance.length) {
    return <p className="p-6 text-center text-sm text-muted">No field provenance was stored for this invoice.</p>;
  }
  return (
    <div className="p-5">
      <div className="mb-3 flex items-start gap-2 rounded-md border border-line bg-paper/40 px-3 py-2 text-[11px] leading-5 text-muted">
        <Info className="mt-0.5 size-3.5 shrink-0 text-indigo" />
        Confidence is only shown when the provider supplied a meaningful value. The deterministic mock does not provide calibrated confidence.
      </div>
      <div className="overflow-x-auto rounded-md border border-line">
        <Table>
          <THead><tr><TH>Field</TH><TH>Extracted</TH><TH>Normalized</TH><TH>Method</TH><TH>Page</TH><TH>Source</TH><TH>Review</TH></tr></THead>
          <TBody>
            {invoice.provenance.map((row) => (
              <TR key={row.id}>
                <TD className="font-mono text-[11px]">{row.field}</TD>
                <TD className="max-w-[160px] truncate text-xs">{row.extracted_value ?? "—"}</TD>
                <TD className="max-w-[160px] truncate text-xs">{row.normalized_value ?? "—"}</TD>
                <TD className="text-[11px]">{row.extraction_method ?? "—"}</TD>
                <TD className="text-[11px]">{row.source_page ?? "—"}</TD>
                <TD className="max-w-[220px] text-[11px] text-muted"><span className="line-clamp-2">{row.source_text ?? "—"}</span></TD>
                <TD>{row.human_edited ? <Badge tone="indigo">Human edited</Badge> : <span className="text-[10px] text-muted">Original</span>}</TD>
              </TR>
            ))}
          </TBody>
        </Table>
      </div>
    </div>
  );
}

function Duplicates({ invoice, onChanged }: { invoice: InvoiceDetail; onChanged: () => void }) {
  const [notes, setNotes] = React.useState<Record<string, string>>({});
  const [busy, setBusy] = React.useState<string | null>(null);

  async function resolve(duplicateId: string, status: "dismissed" | "confirmed") {
    setBusy(duplicateId);
    try {
      await apiFetch(`/invoices/${invoice.id}/duplicates/${duplicateId}/resolve`, {
        method: "POST",
        body: { status, note: notes[duplicateId]?.trim() || null },
      });
      toast.success(status === "dismissed" ? "Marked as not a duplicate." : "Duplicate confirmed. Approval remains blocked.");
      onChanged();
    } catch (caught) {
      toast.error(errorMessage(caught));
    } finally {
      setBusy(null);
    }
  }

  if (!invoice.duplicates.length) {
    return (
      <div className="flex flex-col items-center justify-center px-6 py-14 text-center">
        <CheckCircle2 className="size-8 text-moss" />
        <h3 className="mt-3 font-display text-xl">No duplicate candidates</h3>
        <p className="mt-1 max-w-sm text-xs leading-5 text-muted">Exact file hashes and vendor/number/amount matches were checked.</p>
      </div>
    );
  }

  return (
    <div className="space-y-4 p-5">
      <div className="rounded-md border border-amber/25 bg-amber-soft px-3.5 py-3 text-[11px] leading-5 text-amber">
        Candidates are never rejected automatically. Resolve each open candidate with a note. Confirming a duplicate blocks approval.
      </div>
      {invoice.duplicates.map((duplicate) => (
        <div key={duplicate.id} className="rounded-lg border border-line bg-paper/25 p-4">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div>
              <div className="flex flex-wrap items-center gap-2">
                <Badge tone={duplicate.status === "open" ? "amber" : duplicate.status === "confirmed" ? "rust" : "moss"}>
                  {titleCase(duplicate.status)}
                </Badge>
                <span className="text-xs font-semibold text-ink">{titleCase(duplicate.kind)}</span>
              </div>
              <p className="mt-2 text-xs leading-5 text-ink-soft">
                {duplicate.kind === "exact_file"
                  ? "The uploaded file is byte-for-byte identical to another document."
                  : duplicate.kind === "probable_invoice"
                    ? "Same vendor and invoice number as another record."
                    : "Similar vendor, amount, and date. Verify before deciding."}
              </p>
            </div>
            <div className="text-right">
              <p className="tabular text-sm font-semibold">{formatMoney(duplicate.other.total, duplicate.other.currency ?? "INR")}</p>
              <p className="text-[10px] text-muted">{duplicate.other.invoice_number ?? "Unnumbered"}</p>
            </div>
          </div>
          <div className="mt-3 grid gap-2 rounded-md border border-line bg-surface px-3 py-2 text-[11px] sm:grid-cols-3">
            <div><span className="text-muted">Vendor</span><p className="mt-0.5 font-medium">{duplicate.other.vendor_name ?? "—"}</p></div>
            <div><span className="text-muted">Other status</span><p className="mt-0.5 capitalize">{duplicate.other.status.replaceAll("_", " ")}</p></div>
            <div><span className="text-muted">Created</span><p className="mt-0.5">{formatDateTime(duplicate.other.created_at)}</p></div>
          </div>
          {duplicate.status === "open" ? (
            <div className="mt-3 flex flex-col gap-2 sm:flex-row">
              <Input
                placeholder="Resolution note (optional, audited)"
                value={notes[duplicate.id] ?? ""}
                onChange={(event) => setNotes((current) => ({ ...current, [duplicate.id]: event.target.value }))}
                className="h-9 flex-1 text-xs"
              />
              <div className="flex gap-2">
                <Button size="sm" variant="outline" loading={busy === duplicate.id} onClick={() => void resolve(duplicate.id, "dismissed")}>
                  <Check className="size-3.5" /> Not a duplicate
                </Button>
                <Button size="sm" variant="danger" loading={busy === duplicate.id} onClick={() => void resolve(duplicate.id, "confirmed")}>
                  <ShieldAlert className="size-3.5" /> Confirm
                </Button>
              </div>
            </div>
          ) : duplicate.resolution_note ? (
            <p className="mt-3 text-[11px] text-muted">Resolution: {duplicate.resolution_note}</p>
          ) : null}
        </div>
      ))}
    </div>
  );
}

function HistoryTab({ invoice, onChanged }: { invoice: InvoiceDetail; onChanged: () => void }) {
  const [note, setNote] = React.useState("");
  const [busy, setBusy] = React.useState(false);

  async function addNote() {
    if (!note.trim()) return;
    setBusy(true);
    try {
      await apiFetch(`/invoices/${invoice.id}/notes`, { method: "POST", body: { notes: note.trim() } });
      setNote("");
      onChanged();
      toast.success("Internal note added.");
    } catch (caught) {
      toast.error(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="space-y-6 p-5">
      <div>
        <h3 className="text-sm font-semibold">Internal note</h3>
        <p className="mt-0.5 text-[11px] text-muted">Visible to organization members. Notes are included in the audit history.</p>
        <Textarea className="mt-2 min-h-20" value={note} onChange={(event) => setNote(event.target.value)} placeholder="Add context for the next reviewer…" maxLength={2000} />
        <div className="mt-2 flex justify-end"><Button size="sm" onClick={addNote} loading={busy} disabled={!note.trim()}><MessageSquare className="size-3.5" /> Add note</Button></div>
      </div>
      <Separator />
      <div>
        <h3 className="flex items-center gap-2 text-sm font-semibold"><History className="size-4" /> Decision history</h3>
        <div className="mt-3 space-y-3">
          {[...invoice.reviews.map((review) => ({ id: review.id, action: review.action, actor: review.reviewer_name, notes: review.notes, created_at: review.created_at, kind: "review" as const })),
            ...invoice.approval_events.map((event) => ({ id: event.id, action: event.action, actor: event.actor_name, notes: event.reason, created_at: event.created_at, kind: "approval" as const }))]
            .sort((a, b) => b.created_at.localeCompare(a.created_at))
            .map((event) => (
              <div key={`${event.kind}-${event.id}`} className="flex gap-3">
                <span className="mt-1.5 size-2 shrink-0 rounded-full bg-indigo/60" />
                <div>
                  <p className="text-xs font-semibold text-ink">{titleCase(event.action)}</p>
                  <p className="text-[10px] text-muted">{event.actor ?? "System"} · {formatDateTime(event.created_at)}</p>
                  {event.notes ? <p className="mt-1 rounded border border-line bg-paper/40 px-2 py-1.5 text-[11px] text-ink-soft">{event.notes}</p> : null}
                </div>
              </div>
            ))}
          {!invoice.reviews.length && !invoice.approval_events.length ? <p className="text-xs text-muted">No decisions recorded yet.</p> : null}
        </div>
      </div>
    </div>
  );
}

export function InvoiceReviewPage() {
  const params = useParams<{ id: string }>();
  const id = params.id;
  const queryClient = useQueryClient();
  const [tab, setTab] = React.useState("fields");
  const [approveOpen, setApproveOpen] = React.useState(false);
  const [rejectOpen, setRejectOpen] = React.useState(false);
  const [returnOpen, setReturnOpen] = React.useState(false);
  const [acknowledgeMock, setAcknowledgeMock] = React.useState(false);
  const [reason, setReason] = React.useState("");
  const [rejectReason, setRejectReason] = React.useState("");
  const [returnReason, setReturnReason] = React.useState("");
  const [actionBusy, setActionBusy] = React.useState(false);

  const detail = useQuery({
    queryKey: ["invoice", id],
    queryFn: () => apiFetch<InvoiceDetail>(`/invoices/${id}`),
  });
  const invoice = detail.data;

  function apply(next: InvoiceDetail) {
    queryClient.setQueryData(["invoice", id], next);
    void queryClient.invalidateQueries({ queryKey: ["invoice", id] });
    void queryClient.invalidateQueries({ queryKey: ["invoices"] });
    void queryClient.invalidateQueries({ queryKey: ["dashboard"] });
  }

  async function approve() {
    if (!invoice) return;
    setActionBusy(true);
    try {
      const next = await apiFetch<InvoiceDetail>(`/invoices/${invoice.id}/approve`, {
        method: "POST",
        body: { version: invoice.version, acknowledge_mock: acknowledgeMock, reason: reason.trim() || null },
      });
      apply(next);
      setApproveOpen(false);
      setReason("");
      toast.success("Invoice approved. It is now eligible for export.");
    } catch (caught) {
      toast.error(errorMessage(caught));
    } finally {
      setActionBusy(false);
    }
  }

  async function returnForCorrection() {
    if (!invoice) return;
    if (returnReason.trim().length < 3) {
      toast.error("Add a short reason so the reviewer knows what to correct.");
      return;
    }
    setActionBusy(true);
    try {
      const next = await apiFetch<InvoiceDetail>(`/invoices/${invoice.id}/return`, {
        method: "POST",
        body: { version: invoice.version, reason: returnReason.trim() },
      });
      apply(next);
      setReturnOpen(false);
      setReturnReason("");
      toast.success("Invoice returned to the review queue for correction.");
    } catch (caught) {
      toast.error(errorMessage(caught));
    } finally {
      setActionBusy(false);
    }
  }

  async function reject() {
    if (!invoice) return;
    if (rejectReason.trim().length < 3) {
      toast.error("Add a short reason for the rejection.");
      return;
    }
    setActionBusy(true);
    try {
      const next = await apiFetch<InvoiceDetail>(`/invoices/${invoice.id}/reject`, {
        method: "POST",
        body: { version: invoice.version, reason: rejectReason.trim() },
      });
      apply(next);
      setRejectOpen(false);
      setRejectReason("");
      toast.success("Invoice rejected and returned to the exception history.");
    } catch (caught) {
      toast.error(errorMessage(caught));
    } finally {
      setActionBusy(false);
    }
  }

  async function download() {
    if (!invoice?.document) return;
    try {
      const grant = await apiFetch<{ url: string }>(`/documents/${invoice.document.id}/download-url`, {
        method: "POST",
        body: { disposition: "attachment" },
      });
      window.open(grant.url, "_blank", "noopener,noreferrer");
    } catch (caught) {
      toast.error(errorMessage(caught));
    }
  }

  if (detail.isLoading) {
    return (
      <div className="space-y-4">
        <Skeleton className="h-10 w-48" />
        <div className="grid gap-5 xl:grid-cols-2">
          <Skeleton className="h-[640px]" />
          <Skeleton className="h-[640px]" />
        </div>
      </div>
    );
  }
  if (detail.isError || !invoice) {
    return (
      <div className="rounded-lg border border-rust/25 bg-rust-soft p-6 text-sm text-rust">
        {detail.error ? errorMessage(detail.error) : "Invoice not found."}
        <div className="mt-4"><Button asChild variant="outline"><Link href="/invoices">Back to invoices</Link></Button></div>
      </div>
    );
  }

  const unresolvedIssues = invoice.issues.filter((issue) => !issue.resolved_at);
  const errors = unresolvedIssues.filter((issue) => issue.severity === "error");
  const warnings = unresolvedIssues.filter((issue) => issue.severity === "warning");
  const canAct = invoice.status === "review_required" || invoice.status === "ready_for_approval";
  const approveBlocked = !canAct || !invoice.available_actions.approve || errors.length > 0 || invoice.blocking.open_duplicates > 0 || invoice.blocking.confirmed_duplicates > 0;
  const mockRequired = invoice.blocking.mock_requires_acknowledgement;
  const typeLabel = typeof invoice.document_type === "string" ? titleCase(invoice.document_type) : invoice.document_type.label;

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <Link href={invoice.status === "review_required" ? "/review" : "/invoices"} className="mb-3 inline-flex items-center gap-1.5 text-xs font-semibold text-muted hover:text-ink">
            <ArrowLeft className="size-3.5" /> {invoice.status === "review_required" ? "Back to review queue" : "Back to invoices"}
          </Link>
          <div className="flex flex-wrap items-center gap-2.5">
            <h1 className="font-display text-[2.2rem] leading-none tracking-[-0.025em]">{invoice.invoice_number ?? "Unnumbered invoice"}</h1>
            <StatusBadge status={invoice.status} />
            {invoice.is_mock ? <MockBadge /> : null}
            {invoice.is_credit_note ? <Badge tone="amber">Credit note</Badge> : null}
          </div>
          <p className="mt-2 text-sm text-muted">
            {invoice.vendor_name ?? "Unknown vendor"} · {typeLabel} · Version {invoice.version}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="outline" onClick={() => void download()} disabled={!invoice.document}>
            <Download className="size-4" /> Original
          </Button>
          <Button variant="outline" onClick={() => void detail.refetch()} aria-label="Refresh invoice">
            <RefreshCw className={cn("size-4", detail.isFetching && "animate-spin")} />
          </Button>
          {invoice.available_actions.return_for_correction ? (
            <Button variant="outline" onClick={() => setReturnOpen(true)}>
              <Undo2 className="size-4" /> Return for correction
            </Button>
          ) : null}
          {canAct ? (
            <>
              <Button variant="outline" onClick={() => setRejectOpen(true)} disabled={!invoice.available_actions.reject}>
                <X className="size-4" /> Reject
              </Button>
              <Button onClick={() => setApproveOpen(true)} disabled={approveBlocked}>
                <Check className="size-4" /> Approve
              </Button>
            </>
          ) : invoice.status === "approved" ? (
            <Button asChild><Link href="/exports"><ReceiptText className="size-4" /> Create export</Link></Button>
          ) : null}
        </div>
      </div>

      {mockRequired && canAct ? (
        <div className="flex items-start gap-2.5 rounded-md border border-indigo/25 bg-indigo-soft px-4 py-3 text-xs leading-5 text-indigo-dark">
          <Info className="mt-0.5 size-4 shrink-0" />
          <p><strong>Mock extraction.</strong> These values were parsed by deterministic rules, not AI. Review every field. Approval requires an explicit acknowledgement.</p>
        </div>
      ) : null}

      {approveBlocked && canAct ? (
        <div className="flex items-start gap-2.5 rounded-md border border-rust/25 bg-rust-soft px-4 py-3 text-xs leading-5 text-rust">
          <AlertCircle className="mt-0.5 size-4 shrink-0" />
          <div>
            <p className="font-semibold">Approval is blocked.</p>
            <p className="mt-0.5">
              {errors.length ? `${errors.length} unresolved validation error${errors.length === 1 ? "" : "s"}. ` : ""}
              {invoice.blocking.open_duplicates ? `${invoice.blocking.open_duplicates} open duplicate candidate${invoice.blocking.open_duplicates === 1 ? "" : "s"}. ` : ""}
              {invoice.blocking.confirmed_duplicates ? `${invoice.blocking.confirmed_duplicates} confirmed duplicate${invoice.blocking.confirmed_duplicates === 1 ? "" : "s"}. ` : ""}
              Fix the fields or resolve the candidates below.
            </p>
          </div>
        </div>
      ) : null}

      <div className="grid items-start gap-5 xl:grid-cols-[minmax(0,1.05fr)_minmax(500px,0.95fr)]">
        <div className="xl:sticky xl:top-[84px] xl:h-[calc(100vh-104px)]">
          {invoice.document ? (
            <PdfViewer
              documentId={invoice.document.id}
              contentType={invoice.document.content_type}
              filename={invoice.document.filename}
              onDownload={() => void download()}
            />
          ) : (
            <div className="grid h-96 place-items-center rounded-lg border border-dashed border-line-strong text-sm text-muted">Source document unavailable.</div>
          )}
        </div>

        <div className="space-y-5">
          <Card>
            <CardContent className="p-5">
              <div className="flex flex-wrap items-start justify-between gap-4">
                <div>
                  <p className="label-caps">Grand total</p>
                  <p className="tabular mt-1 font-display text-[2.1rem] leading-none tracking-[-0.03em]">
                    {formatMoney(invoice.total, invoice.currency ?? "INR")}
                    <span className="ml-2 text-sm tracking-normal text-muted">{invoice.currency ?? ""}</span>
                  </p>
                </div>
                <div className="text-right text-[11px] leading-5 text-muted">
                  <p>Invoice date <span className="font-medium text-ink">{formatDate(invoice.invoice_date)}</span></p>
                  <p>Due date <span className="font-medium text-ink">{formatDate(invoice.due_date)}</span></p>
                </div>
              </div>
              <dl className="mt-5 grid grid-cols-2 gap-x-4 gap-y-4 border-t border-line pt-4 sm:grid-cols-3">
                <Detail label="Vendor" value={invoice.vendor_name ?? <span className="text-rust">Missing</span>} />
                <Detail label="Purchase order" value={invoice.purchase_order ?? "—"} />
                <Detail label="Payment terms" value={invoice.payment_terms ?? "—"} />
                <Detail label="Subtotal" value={formatMoney(invoice.subtotal, invoice.currency ?? "INR")} />
                <Detail label="Tax total" value={formatMoney(invoice.tax_total, invoice.currency ?? "INR")} />
                <Detail label="Balance due" value={formatMoney(invoice.balance_due, invoice.currency ?? "INR")} />
              </dl>
              <div className="mt-4 flex flex-wrap items-center gap-2 border-t border-line pt-3 text-[10px] text-muted">
                <Badge tone="neutral">{invoice.extraction.provider ?? "Unknown provider"}</Badge>
                {invoice.extraction.model ? <Badge tone="neutral">{invoice.extraction.model}</Badge> : null}
                <span>Uploaded {formatDateTime(invoice.created_at)}</span>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="pb-3">
              <CardTitle>Validation & exceptions</CardTitle>
              <div className="flex gap-1.5">
                {errors.length ? <Badge tone="rust">{errors.length} errors</Badge> : null}
                {warnings.length ? <Badge tone="amber">{warnings.length} warnings</Badge> : null}
                {!unresolvedIssues.length ? <Badge tone="moss">No open issues</Badge> : null}
              </div>
            </CardHeader>
            <CardContent>
              {unresolvedIssues.length ? (
                <ul className="space-y-2.5">
                  {unresolvedIssues.map((issue) => (
                    <li key={issue.id} className={cn("rounded-md border px-3 py-2.5", issue.severity === "error" ? "border-rust/20 bg-rust-soft/55" : issue.severity === "warning" ? "border-amber/20 bg-amber-soft/55" : "border-line bg-paper/40")}>
                      <div className="flex flex-wrap items-center gap-2">
                        <SeverityBadge severity={issue.severity} />
                        <span className="font-mono text-[10px] text-muted">{issue.code}</span>
                        {issue.field ? <span className="text-[10px] text-muted">{issue.field}</span> : null}
                      </div>
                      <p className="mt-1.5 text-xs leading-5 text-ink-soft">{issue.message}</p>
                    </li>
                  ))}
                </ul>
              ) : (
                <div className="flex items-center gap-2 py-2 text-xs text-moss"><CheckCircle2 className="size-4" /> Deterministic checks passed.</div>
              )}
            </CardContent>
          </Card>

          <Card className="overflow-hidden">
            <Tabs
              value={tab}
              onChange={setTab}
              items={[
                { id: "fields", label: "Fields" },
                { id: "items", label: "Items & tax", count: invoice.line_items.length + invoice.tax_lines.length },
                { id: "evidence", label: "Evidence" },
                { id: "duplicates", label: "Duplicates", count: invoice.blocking.open_duplicates },
                { id: "history", label: "History", count: invoice.reviews.length + invoice.approval_events.length },
              ]}
            />
            {tab === "fields" ? <FieldEditor invoice={invoice} onSaved={apply} /> : null}
            {tab === "items" ? <ItemsEditor invoice={invoice} onSaved={apply} /> : null}
            {tab === "evidence" ? <Evidence invoice={invoice} /> : null}
            {tab === "duplicates" ? <Duplicates invoice={invoice} onChanged={() => void detail.refetch()} /> : null}
            {tab === "history" ? <HistoryTab invoice={invoice} onChanged={() => void detail.refetch()} /> : null}
          </Card>
        </div>
      </div>

      <Dialog open={approveOpen} onOpenChange={(open) => { setApproveOpen(open); if (!open) { setAcknowledgeMock(false); setReason(""); } }}>
        <DialogContent
          title="Approve this invoice?"
          description="Approval is an explicit server-side transition. The invoice becomes eligible for export; it is not sent anywhere automatically."
        >
          <div className="space-y-4 px-6 py-5">
            {mockRequired ? (
              <label className="flex items-start gap-3 rounded-md border border-indigo/25 bg-indigo-soft/60 p-3.5">
                <Checkbox checked={acknowledgeMock} onChange={(event) => setAcknowledgeMock(event.target.checked)} />
                <span className="text-xs leading-5 text-indigo-dark">
                  <strong className="block">I reviewed the mock-extracted values.</strong>
                  I understand this invoice was parsed by deterministic rules, not an AI model, and the values match the source document.
                </span>
              </label>
            ) : null}
            <Field label="Approval note" hint="Optional, audited">
              <Textarea value={reason} onChange={(event) => setReason(event.target.value)} placeholder="Why is this invoice approved?" maxLength={1000} />
            </Field>
            <div className="rounded-md border border-line bg-paper/45 px-3 py-2 text-[11px] leading-5 text-muted">
              {invoice.invoice_number ?? "Unnumbered"} · {invoice.vendor_name ?? "Unknown vendor"} · {formatMoney(invoice.total, invoice.currency ?? "INR")}
            </div>
          </div>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setApproveOpen(false)}>Cancel</Button>
            <Button onClick={approve} loading={actionBusy} disabled={mockRequired && !acknowledgeMock}>
              <Check className="size-4" /> Approve invoice
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={returnOpen} onOpenChange={(open) => { setReturnOpen(open); if (!open) setReturnReason(""); }}>
        <DialogContent title="Return this invoice for correction?" description="The invoice moves back to the review queue. The reason is recorded in the audit history and the original document is preserved.">
          <div className="px-6 py-5">
            <Field label="Correction needed" hint="Required">
              <Textarea value={returnReason} onChange={(event) => setReturnReason(event.target.value)} placeholder="Explain what the reviewer or supplier must correct." maxLength={500} />
            </Field>
          </div>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setReturnOpen(false)}>Cancel</Button>
            <Button variant="outline" onClick={returnForCorrection} loading={actionBusy} disabled={returnReason.trim().length < 3}><Undo2 className="size-4" /> Return for correction</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={rejectOpen} onOpenChange={(open) => { setRejectOpen(open); if (!open) setRejectReason(""); }}>
        <DialogContent title="Reject this invoice?" description="The rejection and reason are recorded permanently in the audit history.">
          <div className="px-6 py-5">
            <Field label="Reason" hint="Required">
              <Textarea value={rejectReason} onChange={(event) => setRejectReason(event.target.value)} placeholder="Explain what is wrong or what the supplier must correct." maxLength={1000} />
            </Field>
          </div>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setRejectOpen(false)}>Cancel</Button>
            <Button variant="danger" onClick={reject} loading={actionBusy} disabled={rejectReason.trim().length < 3}>Reject invoice</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
