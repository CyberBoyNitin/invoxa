"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Building2, Mail, MapPin, Pencil, Phone, Plus, Search } from "lucide-react";
import Link from "next/link";
import * as React from "react";
import { toast } from "sonner";
import { StatusBadge } from "@/components/status-badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle, EmptyState, Skeleton } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter } from "@/components/ui/dialog";
import { Field, Input, Textarea } from "@/components/ui/input";
import { PageHeader } from "@/components/ui/page-header";
import { Table, TBody, TD, TH, THead, TR } from "@/components/ui/table";
import { apiFetch, errorMessage } from "@/lib/api";
import { formatDate, formatMoney } from "@/lib/format";
import type { VendorRow } from "@/lib/types";

interface VendorListResponse {
  items: VendorRow[];
  total: number;
  limit: number;
  offset: number;
}

interface VendorDetail extends VendorRow {
  updated_at: string;
  invoices: Array<{
    id: string;
    document_id: string;
    invoice_number: string | null;
    invoice_date: string | null;
    currency: string | null;
    total: string | null;
    status: string;
    created_at: string;
  }>;
}

interface VendorForm {
  name: string;
  tax_id: string;
  email: string;
  phone: string;
  address: string;
}

const emptyForm: VendorForm = { name: "", tax_id: "", email: "", phone: "", address: "" };

export default function VendorsPage() {
  const [search, setSearch] = React.useState("");
  const [debounced, setDebounced] = React.useState("");
  const [selectedId, setSelectedId] = React.useState<string | null>(null);
  const [dialogOpen, setDialogOpen] = React.useState(false);
  const [editing, setEditing] = React.useState<VendorRow | null>(null);
  const [form, setForm] = React.useState<VendorForm>(emptyForm);
  const [busy, setBusy] = React.useState(false);
  const queryClient = useQueryClient();

  React.useEffect(() => {
    const timer = window.setTimeout(() => setDebounced(search.trim()), 280);
    return () => window.clearTimeout(timer);
  }, [search]);

  const list = useQuery({
    queryKey: ["vendors", "list", debounced],
    queryFn: () => {
      const params = new URLSearchParams({ limit: "100" });
      if (debounced) params.set("search", debounced);
      return apiFetch<VendorListResponse>(`/vendors?${params.toString()}`);
    },
    placeholderData: (previous) => previous,
  });
  const detail = useQuery({
    queryKey: ["vendors", "detail", selectedId],
    queryFn: () => apiFetch<VendorDetail>(`/vendors/${selectedId}`),
    enabled: Boolean(selectedId),
  });

  function openCreate() {
    setEditing(null);
    setForm(emptyForm);
    setDialogOpen(true);
  }
  function openEdit(vendor: VendorRow) {
    setEditing(vendor);
    setForm({
      name: vendor.name,
      tax_id: vendor.tax_id ?? "",
      email: vendor.email ?? "",
      phone: vendor.phone ?? "",
      address: vendor.address ?? "",
    });
    setDialogOpen(true);
  }

  async function save(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    const body = {
      name: form.name.trim(),
      tax_id: form.tax_id.trim() || null,
      email: form.email.trim() || null,
      phone: form.phone.trim() || null,
      address: form.address.trim() || null,
    };
    try {
      if (editing) {
        await apiFetch(`/vendors/${editing.id}`, { method: "PATCH", body });
        toast.success("Vendor updated.");
      } else {
        const created = await apiFetch<VendorRow>("/vendors", { method: "POST", body });
        setSelectedId(created.id);
        toast.success("Vendor created.");
      }
      setDialogOpen(false);
      await queryClient.invalidateQueries({ queryKey: ["vendors"] });
    } catch (caught) {
      toast.error(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  }

  const vendor = detail.data;

  return (
    <div className="space-y-6">
      <PageHeader
        eyebrow="Supplier master"
        title="Vendors"
        description="Normalized supplier records linked to extracted invoices. Edits never rewrite historical extraction evidence."
        actions={<Button onClick={openCreate}><Plus className="size-4" /> Add vendor</Button>}
      />

      <div className="grid items-start gap-5 xl:grid-cols-[minmax(0,1fr)_420px]">
        <div className="space-y-4">
          <div className="relative rounded-lg border border-line bg-surface p-3">
            <Search className="pointer-events-none absolute left-6 top-1/2 size-4 -translate-y-1/2 text-muted" />
            <Input className="pl-9" placeholder="Search vendors by name, tax ID, or email…" value={search} onChange={(event) => setSearch(event.target.value)} aria-label="Search vendors" />
          </div>

          <div className="overflow-hidden rounded-lg border border-line bg-surface">
            {list.isLoading ? (
              <div className="space-y-2 p-4">{Array.from({ length: 6 }).map((_, index) => <Skeleton key={index} className="h-12" />)}</div>
            ) : list.isError ? (
              <div className="p-5 text-sm text-rust">{errorMessage(list.error)}</div>
            ) : list.data?.items.length ? (
              <Table>
                <THead><tr><TH>Vendor</TH><TH>Tax ID</TH><TH>Invoices</TH><TH>Needs review</TH><TH>Last invoice</TH><TH /></tr></THead>
                <TBody>
                  {list.data.items.map((row) => (
                    <TR key={row.id} className={selectedId === row.id ? "bg-indigo-soft/35" : "cursor-pointer"} onClick={() => setSelectedId(row.id)}>
                      <TD>
                        <p className="font-semibold text-ink">{row.name}</p>
                        <p className="mt-0.5 text-[10px] text-muted">{row.email ?? row.phone ?? "No contact details"}</p>
                      </TD>
                      <TD className="font-mono text-[11px]">{row.tax_id ?? "—"}</TD>
                      <TD className="tabular text-xs">{row.invoice_count}</TD>
                      <TD>{row.review_count ? <span className="rounded bg-amber-soft px-1.5 py-0.5 text-[10px] font-semibold text-amber">{row.review_count}</span> : <span className="text-[11px] text-muted">0</span>}</TD>
                      <TD className="whitespace-nowrap text-[11px] text-muted">{formatDate(row.last_invoice_at)}</TD>
                      <TD className="text-right"><Button size="sm" variant="ghost" onClick={(event) => { event.stopPropagation(); openEdit(row); }}><Pencil className="size-3.5" /> Edit</Button></TD>
                    </TR>
                  ))}
                </TBody>
              </Table>
            ) : (
              <div className="p-5"><EmptyState icon={<Building2 className="size-5" />} title="No vendors yet" description="Vendors are created automatically when extraction resolves a supplier, or you can add one manually." action={<Button onClick={openCreate}><Plus className="size-4" /> Add vendor</Button>} /></div>
            )}
          </div>
        </div>

        <Card className="xl:sticky xl:top-[84px]">
          {!selectedId ? (
            <CardContent className="flex min-h-[320px] flex-col items-center justify-center py-12 text-center">
              <Building2 className="size-8 text-muted/60" />
              <p className="mt-3 font-display text-xl">Select a vendor</p>
              <p className="mt-1 max-w-xs text-xs leading-5 text-muted">See contact details and the invoices linked to this supplier.</p>
            </CardContent>
          ) : detail.isLoading ? (
            <CardContent className="space-y-3 py-5">{Array.from({ length: 6 }).map((_, index) => <Skeleton key={index} className="h-8" />)}</CardContent>
          ) : detail.isError || !vendor ? (
            <CardContent className="py-6 text-sm text-rust">{detail.error ? errorMessage(detail.error) : "Vendor not found."}</CardContent>
          ) : (
            <>
              <CardHeader>
                <div><CardTitle className="text-lg">{vendor.name}</CardTitle><p className="mt-1 text-[11px] text-muted">Vendor since {formatDate(vendor.created_at)}</p></div>
                <Button size="sm" variant="outline" onClick={() => openEdit(vendor)}><Pencil className="size-3.5" /> Edit</Button>
              </CardHeader>
              <CardContent className="space-y-5">
                <div className="space-y-2.5 text-xs">
                  <p className="flex items-center gap-2"><Mail className="size-3.5 text-muted" /> {vendor.email ?? "No email"}</p>
                  <p className="flex items-center gap-2"><Phone className="size-3.5 text-muted" /> {vendor.phone ?? "No phone"}</p>
                  <p className="flex items-start gap-2"><MapPin className="mt-0.5 size-3.5 shrink-0 text-muted" /> <span className="leading-5">{vendor.address ?? "No address"}</span></p>
                  <p className="font-mono text-[11px] text-muted">Tax ID: {vendor.tax_id ?? "—"}</p>
                </div>
                <div>
                  <p className="label-caps mb-2">Recent invoices</p>
                  {vendor.invoices.length ? (
                    <div className="divide-y divide-line rounded-md border border-line">
                      {vendor.invoices.slice(0, 8).map((invoice) => (
                        <Link key={invoice.id} href={`/invoices/${invoice.id}`} className="flex items-center justify-between gap-3 px-3 py-2.5 hover:bg-paper-deep/60">
                          <div className="min-w-0"><p className="truncate text-xs font-semibold">{invoice.invoice_number ?? "Unnumbered"}</p><p className="text-[10px] text-muted">{formatDate(invoice.invoice_date)}</p></div>
                          <div className="text-right"><p className="tabular text-xs font-semibold">{formatMoney(invoice.total, invoice.currency ?? "INR")}</p><StatusBadge status={invoice.status} /></div>
                        </Link>
                      ))}
                    </div>
                  ) : <p className="text-xs text-muted">No invoices linked yet.</p>}
                </div>
              </CardContent>
            </>
          )}
        </Card>
      </div>

      <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
        <DialogContent title={editing ? "Edit vendor" : "Add vendor"} description="Supplier details are organization-scoped and never shared across tenants.">
          <form onSubmit={save}>
            <div className="space-y-4 px-6 py-5">
              <Field label="Vendor name" htmlFor="vendor-name"><Input id="vendor-name" required maxLength={200} value={form.name} onChange={(event) => setForm({ ...form, name: event.target.value })} /></Field>
              <div className="grid gap-4 sm:grid-cols-2">
                <Field label="Tax ID" htmlFor="vendor-tax"><Input id="vendor-tax" maxLength={80} value={form.tax_id} onChange={(event) => setForm({ ...form, tax_id: event.target.value })} /></Field>
                <Field label="Email" htmlFor="vendor-email"><Input id="vendor-email" type="email" maxLength={320} value={form.email} onChange={(event) => setForm({ ...form, email: event.target.value })} /></Field>
                <Field label="Phone" htmlFor="vendor-phone"><Input id="vendor-phone" maxLength={50} value={form.phone} onChange={(event) => setForm({ ...form, phone: event.target.value })} /></Field>
              </div>
              <Field label="Address" htmlFor="vendor-address"><Textarea id="vendor-address" maxLength={1000} value={form.address} onChange={(event) => setForm({ ...form, address: event.target.value })} /></Field>
            </div>
            <DialogFooter><Button type="button" variant="ghost" onClick={() => setDialogOpen(false)}>Cancel</Button><Button type="submit" loading={busy}>{editing ? "Save vendor" : "Create vendor"}</Button></DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </div>
  );
}
