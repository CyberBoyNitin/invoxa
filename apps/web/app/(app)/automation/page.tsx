"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowRight, Pencil, Plus, Sparkles, Trash2 } from "lucide-react";
import * as React from "react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle, EmptyState, Skeleton } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter } from "@/components/ui/dialog";
import { Checkbox, Field, Input, Select, Textarea } from "@/components/ui/input";
import { PageHeader } from "@/components/ui/page-header";
import { apiFetch, errorMessage } from "@/lib/api";
import { titleCase } from "@/lib/format";
import type { AutomationRule, Member } from "@/lib/types";

interface AutomationResponse {
  items: AutomationRule[];
  supported: { fields: string[]; operators: string[]; actions: string[]; notes: string };
}
interface MembersResponse {
  members: Member[];
  roles: Array<{ value: string; label: string }>;
}

type ConditionDraft = { field: string; op: string; value: string };
type ActionDraft = { action: string; value: string };

const numericFields = new Set(["total", "extraction_quality"]);
const boolFields = new Set(["has_duplicates", "has_blocking_issues", "missing_required_fields"]);
const valueActions = new Set(["add_label", "assign_reviewer"]);

const actionLabels: Record<string, string> = {
  add_label: "Add label",
  assign_reviewer: "Assign reviewer",
  hold_for_review: "Hold for review",
  send_notification: "Record notification",
  queue_export: "Mark export-eligible",
  trigger_webhook: "Request webhook",
};

export default function AutomationPage() {
  const [dialogOpen, setDialogOpen] = React.useState(false);
  const [editing, setEditing] = React.useState<AutomationRule | null>(null);
  const [name, setName] = React.useState("");
  const [description, setDescription] = React.useState("");
  const [enabled, setEnabled] = React.useState(true);
  const [priority, setPriority] = React.useState(100);
  const [matchAll, setMatchAll] = React.useState(true);
  const [conditions, setConditions] = React.useState<ConditionDraft[]>([{ field: "currency", op: "eq", value: "" }]);
  const [actions, setActions] = React.useState<ActionDraft[]>([{ action: "add_label", value: "needs-review" }]);
  const [busy, setBusy] = React.useState(false);
  const [deleteTarget, setDeleteTarget] = React.useState<AutomationRule | null>(null);
  const queryClient = useQueryClient();

  const rules = useQuery({
    queryKey: ["automation-rules"],
    queryFn: () => apiFetch<AutomationResponse>("/automation-rules"),
  });
  const members = useQuery({
    queryKey: ["members", "automation"],
    queryFn: () => apiFetch<MembersResponse>("/members"),
    retry: false,
  });

  function openCreate() {
    setEditing(null);
    setName("");
    setDescription("");
    setEnabled(true);
    setPriority(100);
    setMatchAll(true);
    setConditions([{ field: "currency", op: "eq", value: "" }]);
    setActions([{ action: "add_label", value: "needs-review" }]);
    setDialogOpen(true);
  }

  function openEdit(rule: AutomationRule) {
    setEditing(rule);
    setName(rule.name);
    setDescription(rule.description ?? "");
    setEnabled(rule.enabled);
    setPriority(rule.priority);
    setMatchAll(rule.spec.match_all);
    setConditions(rule.spec.conditions.map((condition) => ({ field: condition.field, op: condition.op, value: String(condition.value ?? "") })));
    setActions(rule.spec.actions.map((action) => ({ action: action.action, value: action.value === null || action.value === undefined ? "" : String(action.value) })));
    setDialogOpen(true);
  }

  function buildSpec() {
    return {
      match_all: matchAll,
      conditions: conditions.map((condition) => {
        let value: unknown = condition.value;
        if (numericFields.has(condition.field)) value = Number(condition.value);
        else if (boolFields.has(condition.field)) value = condition.value === "true";
        return { field: condition.field, op: condition.op, value };
      }),
      actions: actions.map((action) => ({
        action: action.action,
        value: valueActions.has(action.action) ? action.value : null,
      })),
    };
  }

  async function save(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    const body = { name: name.trim(), description: description.trim() || null, enabled, priority, spec: buildSpec() };
    try {
      if (editing) {
        await apiFetch(`/automation-rules/${editing.id}`, { method: "PATCH", body });
        toast.success("Automation rule updated.");
      } else {
        await apiFetch("/automation-rules", { method: "POST", body });
        toast.success("Automation rule created.");
      }
      setDialogOpen(false);
      await queryClient.invalidateQueries({ queryKey: ["automation-rules"] });
    } catch (caught) {
      toast.error(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  }

  async function toggle(rule: AutomationRule) {
    try {
      await apiFetch(`/automation-rules/${rule.id}`, {
        method: "PATCH",
        body: { name: rule.name, description: rule.description, enabled: !rule.enabled, priority: rule.priority, spec: rule.spec },
      });
      await queryClient.invalidateQueries({ queryKey: ["automation-rules"] });
      toast.success(rule.enabled ? "Rule disabled." : "Rule enabled.");
    } catch (caught) {
      toast.error(errorMessage(caught));
    }
  }

  async function remove() {
    if (!deleteTarget) return;
    setBusy(true);
    try {
      await apiFetch(`/automation-rules/${deleteTarget.id}`, { method: "DELETE" });
      setDeleteTarget(null);
      await queryClient.invalidateQueries({ queryKey: ["automation-rules"] });
      toast.success("Rule deleted.");
    } catch (caught) {
      toast.error(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="space-y-6">
      <PageHeader
        eyebrow="Deterministic rules"
        title="Automations"
        description="Label, assign, and route extracted invoices. Rules run after validation and can never approve an invoice or bypass human review."
        actions={<Button onClick={openCreate}><Plus className="size-4" /> New rule</Button>}
      />

      <div className="rounded-lg border border-indigo/20 bg-indigo-soft/50 px-4 py-3 text-[11px] leading-5 text-indigo-dark">
        <strong>Review remains mandatory.</strong> {rules.data?.supported.notes ?? "queue_export only marks an already approved invoice as eligible; it never sends data."}
      </div>

      {rules.isLoading ? (
        <div className="space-y-3">{Array.from({ length: 3 }).map((_, index) => <Skeleton key={index} className="h-32" />)}</div>
      ) : rules.isError ? (
        <div className="rounded-lg border border-rust/25 bg-rust-soft p-5 text-sm text-rust">{errorMessage(rules.error)}</div>
      ) : rules.data?.items.length ? (
        <div className="grid gap-4 lg:grid-cols-2">
          {rules.data.items.map((rule) => (
            <Card key={rule.id}>
              <CardHeader>
                <div>
                  <div className="flex items-center gap-2">
                    <CardTitle>{rule.name}</CardTitle>
                    <Badge tone={rule.enabled ? "moss" : "neutral"}>{rule.enabled ? "Enabled" : "Disabled"}</Badge>
                    <Badge tone="neutral">Priority {rule.priority}</Badge>
                  </div>
                  {rule.description ? <p className="mt-1 text-xs leading-5 text-muted">{rule.description}</p> : null}
                </div>
                <div className="flex gap-1">
                  <Button size="sm" variant="ghost" onClick={() => openEdit(rule)}><Pencil className="size-3.5" /></Button>
                  <Button size="sm" variant="ghost" onClick={() => void toggle(rule)}>{rule.enabled ? "Disable" : "Enable"}</Button>
                  <Button size="sm" variant="ghost" className="text-rust hover:text-rust" onClick={() => setDeleteTarget(rule)}><Trash2 className="size-3.5" /></Button>
                </div>
              </CardHeader>
              <CardContent>
                <div className="rounded-md border border-line bg-paper/35 p-3">
                  <p className="label-caps">When {rule.spec.match_all ? "all" : "any"} match</p>
                  <div className="mt-2 space-y-1.5">
                    {rule.spec.conditions.length ? rule.spec.conditions.map((condition, index) => (
                      <div key={index} className="flex flex-wrap items-center gap-1.5 text-[11px]">
                        <code className="rounded bg-surface px-1.5 py-0.5 font-mono text-indigo-dark">{condition.field}</code>
                        <span className="text-muted">{condition.op}</span>
                        <code className="rounded bg-surface px-1.5 py-0.5 font-mono">{String(condition.value)}</code>
                      </div>
                    )) : <p className="text-[11px] text-muted">No conditions — applies to every extracted invoice.</p>}
                  </div>
                  <div className="mt-3 flex items-center gap-2 border-t border-line pt-2.5">
                    <ArrowRight className="size-3.5 text-muted" />
                    <div className="flex flex-wrap gap-1.5">
                      {rule.spec.actions.map((action, index) => (
                        <Badge key={index} tone="indigo">{actionLabels[action.action] ?? titleCase(action.action)}{action.value ? `: ${String(action.value)}` : ""}</Badge>
                      ))}
                    </div>
                  </div>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      ) : (
        <EmptyState icon={<Sparkles className="size-5" />} title="No automation rules" description="Create deterministic rules to label, assign, or hold invoices after extraction. Every result still requires a reviewer." action={<Button onClick={openCreate}><Plus className="size-4" /> New rule</Button>} />
      )}

      <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
        <DialogContent className="max-w-3xl" title={editing ? "Edit automation rule" : "New automation rule"} description="Conditions are evaluated against extracted, validated values. Actions are recorded and may affect routing; they cannot approve.">
          <form onSubmit={save}>
            <div className="max-h-[65vh] space-y-5 overflow-y-auto px-6 py-5">
              <div className="grid gap-4 sm:grid-cols-[1fr_130px]">
                <Field label="Rule name" htmlFor="rule-name"><Input id="rule-name" required maxLength={160} value={name} onChange={(event) => setName(event.target.value)} placeholder="High-value invoices" /></Field>
                <Field label="Priority" htmlFor="rule-priority"><Input id="rule-priority" type="number" min={0} max={10000} value={priority} onChange={(event) => setPriority(Number(event.target.value))} /></Field>
              </div>
              <Field label="Description" htmlFor="rule-description"><Textarea id="rule-description" className="min-h-16" maxLength={1000} value={description} onChange={(event) => setDescription(event.target.value)} /></Field>
              <label className="flex items-center gap-2 text-xs font-medium"><Checkbox checked={enabled} onChange={(event) => setEnabled(event.target.checked)} /> Enabled</label>

              <div>
                <div className="flex flex-wrap items-center justify-between gap-3">
                  <div>
                    <p className="text-sm font-semibold">Conditions</p>
                    <p className="mt-0.5 text-[11px] text-muted">Match {matchAll ? "all" : "any"} of the following.</p>
                  </div>
                  <div className="flex items-center gap-2">
                    <Select value={matchAll ? "all" : "any"} onChange={(event) => setMatchAll(event.target.value === "all")} className="h-8 w-28 text-xs"><option value="all">Match all</option><option value="any">Match any</option></Select>
                    <Button type="button" size="sm" variant="outline" onClick={() => setConditions([...conditions, { field: "currency", op: "eq", value: "" }])}><Plus className="size-3.5" /> Add</Button>
                  </div>
                </div>
                <div className="mt-3 space-y-2">
                  {conditions.map((condition, index) => (
                    <div key={index} className="grid gap-2 rounded-md border border-line bg-paper/35 p-2.5 sm:grid-cols-[1fr_110px_1fr_36px]">
                      <Select value={condition.field} onChange={(event) => setConditions(conditions.map((item, i) => i === index ? { ...item, field: event.target.value, value: "" } : item))} className="h-9 text-xs">
                        {rules.data?.supported.fields.map((field) => <option key={field} value={field}>{titleCase(field)}</option>)}
                      </Select>
                      <Select value={condition.op} onChange={(event) => setConditions(conditions.map((item, i) => i === index ? { ...item, op: event.target.value } : item))} className="h-9 text-xs">
                        {rules.data?.supported.operators.map((op) => <option key={op} value={op}>{op}</option>)}
                      </Select>
                      {boolFields.has(condition.field) ? (
                        <Select value={condition.value} onChange={(event) => setConditions(conditions.map((item, i) => i === index ? { ...item, value: event.target.value } : item))} className="h-9 text-xs"><option value="">Choose…</option><option value="true">True</option><option value="false">False</option></Select>
                      ) : (
                        <Input className="h-9 text-xs" type={numericFields.has(condition.field) ? "number" : "text"} step={numericFields.has(condition.field) ? "any" : undefined} required value={condition.value} onChange={(event) => setConditions(conditions.map((item, i) => i === index ? { ...item, value: event.target.value } : item))} placeholder="Value" />
                      )}
                      <Button type="button" size="icon" variant="ghost" className="size-9 text-rust" onClick={() => setConditions(conditions.filter((_, i) => i !== index))} aria-label="Remove condition"><Trash2 className="size-3.5" /></Button>
                    </div>
                  ))}
                  {!conditions.length ? <p className="text-[11px] text-muted">No conditions means every extracted invoice matches.</p> : null}
                </div>
              </div>

              <div>
                <div className="flex items-center justify-between gap-3">
                  <div><p className="text-sm font-semibold">Actions</p><p className="mt-0.5 text-[11px] text-muted">At least one action is required.</p></div>
                  <Button type="button" size="sm" variant="outline" onClick={() => setActions([...actions, { action: "hold_for_review", value: "" }])}><Plus className="size-3.5" /> Add</Button>
                </div>
                <div className="mt-3 space-y-2">
                  {actions.map((action, index) => (
                    <div key={index} className="grid gap-2 rounded-md border border-line bg-paper/35 p-2.5 sm:grid-cols-[1fr_1fr_36px]">
                      <Select value={action.action} onChange={(event) => setActions(actions.map((item, i) => i === index ? { action: event.target.value, value: "" } : item))} className="h-9 text-xs">
                        {rules.data?.supported.actions.map((name) => <option key={name} value={name}>{actionLabels[name] ?? name}</option>)}
                      </Select>
                      {valueActions.has(action.action) ? (
                        action.action === "assign_reviewer" ? (
                          <Select value={action.value} onChange={(event) => setActions(actions.map((item, i) => i === index ? { ...item, value: event.target.value } : item))} className="h-9 text-xs">
                            <option value="">Choose member…</option>
                            {members.data?.members.map((member) => <option key={member.user_id} value={member.user_id}>{member.name} ({member.role})</option>)}
                          </Select>
                        ) : (
                          <Input className="h-9 text-xs" required maxLength={60} value={action.value} onChange={(event) => setActions(actions.map((item, i) => i === index ? { ...item, value: event.target.value } : item))} placeholder="Label name" />
                        )
                      ) : <div className="flex h-9 items-center rounded-md border border-dashed border-line px-3 text-[10px] text-muted">No value required</div>}
                      <Button type="button" size="icon" variant="ghost" className="size-9 text-rust" onClick={() => setActions(actions.filter((_, i) => i !== index))} aria-label="Remove action"><Trash2 className="size-3.5" /></Button>
                    </div>
                  ))}
                </div>
              </div>
            </div>
            <DialogFooter><Button type="button" variant="ghost" onClick={() => setDialogOpen(false)}>Cancel</Button><Button type="submit" loading={busy}>{editing ? "Save rule" : "Create rule"}</Button></DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <Dialog open={Boolean(deleteTarget)} onOpenChange={(open) => !open && setDeleteTarget(null)}>
        <DialogContent title="Delete this rule?" description="The rule stops applying to future invoices. Existing labels and review assignments are retained.">
          <div className="px-6 py-5 text-sm text-muted">{deleteTarget?.name}</div>
          <DialogFooter><Button variant="ghost" onClick={() => setDeleteTarget(null)}>Cancel</Button><Button variant="danger" onClick={remove} loading={busy}>Delete rule</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
