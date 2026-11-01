"use client";

import Link from "next/link";
import * as React from "react";
import { Button } from "@/components/ui/button";
import { Field, Input } from "@/components/ui/input";
import { apiFetch, errorMessage } from "@/lib/api";

export function ForgotPasswordForm() {
  const [email, setEmail] = React.useState("");
  const [busy, setBusy] = React.useState(false);
  const [message, setMessage] = React.useState("");
  const [devToken, setDevToken] = React.useState<string | null>(null);
  const [error, setError] = React.useState("");

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError("");
    setMessage("");
    try {
      const result = await apiFetch<{ message: string; dev_reset_token: string | null }>(
        "/auth/password-reset/request",
        { method: "POST", body: { email } },
      );
      setMessage(result.message);
      setDevToken(result.dev_reset_token);
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div>
      <p className="label-caps">Account recovery</p>
      <h2 className="mt-2 font-display text-[2.4rem] leading-[1.05] tracking-[-0.03em] text-ink">
        Reset your password
      </h2>
      <p className="mt-3 text-sm leading-6 text-muted">
        Enter the email on your account. Reset tokens expire after 30 minutes and can be used once.
      </p>
      <form onSubmit={submit} className="mt-8 space-y-4">
        {error ? <div className="rounded-md border border-rust/25 bg-rust-soft px-3.5 py-3 text-xs text-rust">{error}</div> : null}
        {message ? (
          <div className="rounded-md border border-moss/25 bg-moss-soft px-3.5 py-3 text-xs leading-5 text-moss">
            {message}
            {devToken ? (
              <div className="mt-3 border-t border-moss/20 pt-3">
                <p className="font-semibold">Development delivery</p>
                <p className="mt-1">Email is not configured, so this one-time link is shown here:</p>
                <Link
                  href={`/reset-password?token=${encodeURIComponent(devToken)}`}
                  className="mt-2 block break-all font-mono text-[11px] font-semibold underline"
                >
                  Open reset link
                </Link>
              </div>
            ) : null}
          </div>
        ) : null}
        <Field label="Work email" htmlFor="email">
          <Input id="email" type="email" required autoComplete="email" value={email} onChange={(event) => setEmail(event.target.value)} />
        </Field>
        <Button type="submit" className="w-full" size="lg" loading={busy}>
          Create reset link
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
