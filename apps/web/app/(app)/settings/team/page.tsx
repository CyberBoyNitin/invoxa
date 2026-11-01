"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Copy, Mail, Plus, Trash2, UserCog, Users } from "lucide-react";
import * as React from "react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle, Skeleton } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter } from "@/components/ui/dialog";
import { Field, Input, Select } from "@/components/ui/input";
import { Table, TBody, TD, TH, THead, TR } from "@/components/ui/table";
import { apiFetch, errorMessage } from "@/lib/api";
import { formatDateTime, titleCase } from "@/lib/format";
import type { Invitation, Member, Role } from "@/lib/types";

interface MembersResponse {
  members: Member[];
  invitations: Invitation[];
  roles: Array<{ value: Role; label: string }>;
  email_delivery: boolean;
}

interface InviteResponse {
  invitation: Invitation;
  invite_url: string;
  token: string;
  delivery: string;
  note: string;
}

export default function TeamSettingsPage() {
  const queryClient = useQueryClient();
  const [inviteOpen, setInviteOpen] = React.useState(false);
  const [email, setEmail] = React.useState("");
  const [role, setRole] = React.useState<Role>("reviewer");
  const [days, setDays] = React.useState(7);
  const [busy, setBusy] = React.useState(false);
  const [invite, setInvite] = React.useState<InviteResponse | null>(null);
  const [removeTarget, setRemoveTarget] = React.useState<Member | null>(null);

  const team = useQuery({
    queryKey: ["members"],
    queryFn: () => apiFetch<MembersResponse>("/members"),
  });

  async function sendInvite(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    try {
      const created = await apiFetch<InviteResponse>("/invitations", {
        method: "POST",
        body: { email: email.trim(), role, expires_in_days: days },
      });
      setInvite(created);
      setEmail("");
      await queryClient.invalidateQueries({ queryKey: ["members"] });
      toast.success("Invitation created. Copy the single-use link.");
    } catch (caught) {
      toast.error(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  }

  async function changeRole(member: Member, nextRole: Role) {
    try {
      await apiFetch(`/members/${member.user_id}`, { method: "PATCH", body: { role: nextRole } });
      await queryClient.invalidateQueries({ queryKey: ["members"] });
      toast.success("Member role updated.");
    } catch (caught) {
      toast.error(errorMessage(caught));
    }
  }

  async function removeMember() {
    if (!removeTarget) return;
    setBusy(true);
    try {
      await apiFetch(`/members/${removeTarget.user_id}`, { method: "DELETE" });
      setRemoveTarget(null);
      await queryClient.invalidateQueries({ queryKey: ["members"] });
      toast.success("Member removed and their sessions revoked.");
    } catch (caught) {
      toast.error(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  }

  async function revokeInvitation(invitation: Invitation) {
    try {
      await apiFetch(`/invitations/${invitation.id}`, { method: "DELETE" });
      await queryClient.invalidateQueries({ queryKey: ["members"] });
      toast.success("Invitation revoked.");
    } catch (caught) {
      toast.error(errorMessage(caught));
    }
  }

  async function copyInvite() {
    if (!invite) return;
    const url = `${window.location.origin}${invite.invite_url}`;
    try {
      await navigator.clipboard.writeText(url);
      toast.success("Invite link copied.");
    } catch {
      toast.error("Clipboard unavailable. Select and copy the link manually.");
    }
  }

  const pending = team.data?.invitations.filter((invitation) => !invitation.accepted_at) ?? [];

  return (
    <div className="space-y-5">
      <Card>
        <CardHeader>
          <div><CardTitle>Members</CardTitle><p className="mt-1 text-xs text-muted">Roles determine server-enforced permissions. Every session is revoked when a member is removed.</p></div>
          <Button onClick={() => { setInvite(null); setInviteOpen(true); }}><Plus className="size-4" /> Invite member</Button>
        </CardHeader>
        <CardContent className="px-0 pb-0">
          {team.isLoading ? (
            <div className="space-y-2 px-5 pb-5">{Array.from({ length: 4 }).map((_, index) => <Skeleton key={index} className="h-12" />)}</div>
          ) : team.isError ? (
            <div className="p-5 text-sm text-rust">{errorMessage(team.error)}</div>
          ) : team.data?.members.length ? (
            <Table>
              <THead><tr><TH>Member</TH><TH>Role</TH><TH>Joined</TH><TH>Last seen</TH><TH className="text-right">Actions</TH></tr></THead>
              <TBody>
                {team.data.members.map((member) => (
                  <TR key={member.user_id}>
                    <TD>
                      <p className="text-xs font-semibold text-ink">{member.name}</p>
                      <p className="mt-0.5 text-[11px] text-muted">{member.email}</p>
                    </TD>
                    <TD>
                      <Select value={member.role} onChange={(event) => void changeRole(member, event.target.value as Role)} className="h-8 w-36 text-xs" aria-label={`Role for ${member.name}`}>
                        {team.data.roles.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
                      </Select>
                    </TD>
                    <TD className="whitespace-nowrap text-[11px] text-muted">{formatDateTime(member.created_at)}</TD>
                    <TD className="whitespace-nowrap text-[11px] text-muted">{member.last_seen_at ? formatDateTime(member.last_seen_at) : "Never"}</TD>
                    <TD className="text-right"><Button size="sm" variant="ghost" className="text-rust hover:text-rust" onClick={() => setRemoveTarget(member)}><Trash2 className="size-3.5" /> Remove</Button></TD>
                  </TR>
                ))}
              </TBody>
            </Table>
          ) : <div className="p-5 text-sm text-muted">No members loaded.</div>}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <div><CardTitle>Pending invitations</CardTitle><p className="mt-1 text-xs text-muted">Email delivery is not configured. Send the generated single-use link securely.</p></div>
          <Badge tone={team.data?.email_delivery ? "moss" : "amber"}>{team.data?.email_delivery ? "Email enabled" : "Manual link"}</Badge>
        </CardHeader>
        <CardContent className="px-0 pb-0">
          {pending.length ? (
            <Table>
              <THead><tr><TH>Email</TH><TH>Role</TH><TH>Invited by</TH><TH>Expires</TH><TH className="text-right">Actions</TH></tr></THead>
              <TBody>
                {pending.map((invitation) => (
                  <TR key={invitation.id}>
                    <TD className="text-xs font-medium">{invitation.email}</TD>
                    <TD><Badge tone="neutral">{titleCase(invitation.role)}</Badge></TD>
                    <TD className="text-xs text-muted">{invitation.invited_by_name}</TD>
                    <TD className="whitespace-nowrap text-[11px] text-muted">{formatDateTime(invitation.expires_at)}</TD>
                    <TD className="text-right"><Button size="sm" variant="ghost" className="text-rust hover:text-rust" onClick={() => void revokeInvitation(invitation)}>Revoke</Button></TD>
                  </TR>
                ))}
              </TBody>
            </Table>
          ) : <div className="flex items-center gap-2 px-5 pb-5 text-xs text-muted"><Users className="size-4" /> No pending invitations.</div>}
        </CardContent>
      </Card>

      <Dialog open={inviteOpen} onOpenChange={setInviteOpen}>
        <DialogContent title="Invite a team member" description="The invitee must sign in with the invited email before accepting. Invitations expire and can be revoked.">
          {invite ? (
            <>
              <div className="space-y-4 px-6 py-5">
                <div className="rounded-md border border-moss/20 bg-moss-soft/60 p-3 text-[11px] leading-5 text-moss">
                  <p className="font-semibold">Invitation created for {invite.invitation.email}</p>
                  <p className="mt-1">{invite.note}</p>
                </div>
                <Field label="Single-use invite link" htmlFor="invite-link">
                  <div className="flex gap-2">
                    <Input id="invite-link" readOnly value={`${typeof window !== "undefined" ? window.location.origin : ""}${invite.invite_url}`} onFocus={(event) => event.target.select()} />
                    <Button type="button" variant="outline" onClick={() => void copyInvite()}><Copy className="size-4" /> Copy</Button>
                  </div>
                </Field>
              </div>
              <DialogFooter><Button onClick={() => setInviteOpen(false)}><Check className="size-4" /> Done</Button></DialogFooter>
            </>
          ) : (
            <form onSubmit={sendInvite}>
              <div className="space-y-4 px-6 py-5">
                <Field label="Email address" htmlFor="invite-email"><Input id="invite-email" type="email" required maxLength={320} value={email} onChange={(event) => setEmail(event.target.value)} placeholder="accountant@company.com" /></Field>
                <div className="grid gap-4 sm:grid-cols-2">
                  <Field label="Role" htmlFor="invite-role">
                    <Select id="invite-role" value={role} onChange={(event) => setRole(event.target.value as Role)}>
                      {team.data?.roles.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
                    </Select>
                  </Field>
                  <Field label="Expires in" htmlFor="invite-days" hint="1–30 days"><Input id="invite-days" type="number" min={1} max={30} value={days} onChange={(event) => setDays(Number(event.target.value))} /></Field>
                </div>
                <div className="flex items-start gap-2 rounded-md border border-line bg-paper/30 px-3 py-2.5 text-[11px] leading-5 text-muted">
                  <Mail className="mt-0.5 size-3.5 shrink-0" />
                  No email will be sent by Invoxa. The link is returned once in this dialog.
                </div>
              </div>
              <DialogFooter><Button type="button" variant="ghost" onClick={() => setInviteOpen(false)}>Cancel</Button><Button type="submit" loading={busy}><UserCog className="size-4" /> Create invitation</Button></DialogFooter>
            </form>
          )}
        </DialogContent>
      </Dialog>

      <Dialog open={Boolean(removeTarget)} onOpenChange={(open) => !open && setRemoveTarget(null)}>
        <DialogContent title="Remove this member?" description="They immediately lose access to this organization and all their sessions are revoked. Their audit history is retained.">
          <div className="px-6 py-5 text-sm"><p className="font-semibold">{removeTarget?.name}</p><p className="text-xs text-muted">{removeTarget?.email}</p></div>
          <DialogFooter><Button variant="ghost" onClick={() => setRemoveTarget(null)}>Cancel</Button><Button variant="danger" loading={busy} onClick={removeMember}>Remove member</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
