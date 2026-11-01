"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import * as React from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Field, Input } from "@/components/ui/input";
import { apiFetch, errorMessage } from "@/lib/api";

export function ResetPasswordForm({ initialToken }: { initialToken: string }) {
  const [token, setToken] = React.useState(initialToken);
  const [password, setPassword] = React.useState("");
  const [confirm, setConfirm] = React.useState("");
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState("");
  const router = useRouter();

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (password.length < 12) return setError("Use at least 12 characters.");
    if (password !== confirm) return setError("The passwords do not match.");
    setBusy(true);
    setError("");
    try {
      await apiFetch("/auth/password-reset/confirm", {
        method: "POST",
        body: { token, new_password: password },
      });
      toast.success("Password reset. Sign in with your new password.");
      router.replace("/login");
    } catch (caught) {
      setError(errorMessage(caught));
      setBusy(false);
    }
  }

  return (
    <div>
      <p className="label-caps">Account recovery</p>
      <h2 className="mt-2 font-display text-[2.4rem] leading-[1.05] tracking-[-0.03em] text-ink">
        Choose a new password
      </h2>
      <p className="mt-3 text-sm leading-6 text-muted">
        This will sign out every existing session on your account.
      </p>
      <form onSubmit={submit} className="mt-8 space-y-4">
        {error ? <div className="rounded-md border border-rust/25 bg-rust-soft px-3.5 py-3 text-xs text-rust">{error}</div> : null}
        <Field label="Reset token" htmlFor="token">
          <Input id="token" required value={token} onChange={(event) => setToken(event.target.value)} />
        </Field>
        <Field label="New password" htmlFor="password" hint="At least 12 characters">
          <Input id="password" type="password" required minLength={12} autoComplete="new-password" value={password} onChange={(event) => setPassword(event.target.value)} />
        </Field>
        <Field label="Confirm password" htmlFor="confirm">
          <Input id="confirm" type="password" required minLength={12} autoComplete="new-password" value={confirm} onChange={(event) => setConfirm(event.target.value)} />
        </Field>
        <Button type="submit" className="w-full" size="lg" loading={busy}>
          Reset password
        </Button>
      </form>
      <p className="mt-6 text-center text-xs text-muted">
        <Link href="/login" className="font-semibold text-indigo hover:underline">
          Back to sign in
        </Link>
      </p>
    </div>
  );
}
