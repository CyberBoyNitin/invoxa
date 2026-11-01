"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import * as React from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Field, Input } from "@/components/ui/input";
import { apiFetch, errorMessage } from "@/lib/api";
import type { Session } from "@/lib/types";

export function LoginForm({ nextPath, initialError }: { nextPath: string; initialError?: string }) {
  const [email, setEmail] = React.useState("");
  const [password, setPassword] = React.useState("");
  const [error, setError] = React.useState(initialError ?? "");
  const [busy, setBusy] = React.useState(false);
  const router = useRouter();

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      const session = await apiFetch<Session>("/auth/login", {
        method: "POST",
        body: { email, password },
      });
      toast.success(`Welcome back, ${session.user?.name ?? "there"}.`);
      router.replace(nextPath);
      router.refresh();
    } catch (caught) {
      setError(errorMessage(caught));
      setBusy(false);
    }
  }

  return (
    <div>
      <p className="label-caps">Welcome back</p>
      <h2 className="mt-2 font-display text-[2.4rem] leading-[1.05] tracking-[-0.03em] text-ink">
        Sign in to your workspace
      </h2>
      <p className="mt-3 text-sm leading-6 text-muted">
        Review invoices, resolve exceptions, and keep every approval accountable.
      </p>

      <form className="mt-8 space-y-4" onSubmit={submit}>
        {error ? (
          <div role="alert" className="rounded-md border border-rust/25 bg-rust-soft px-3.5 py-3 text-xs leading-5 text-rust">
            {error}
          </div>
        ) : null}
        <Field label="Work email" htmlFor="email">
          <Input
            id="email"
            type="email"
            autoComplete="email"
            required
            placeholder="you@company.com"
            value={email}
            onChange={(event) => setEmail(event.target.value)}
          />
        </Field>
        <Field
          label="Password"
          htmlFor="password"
          hint={
            <Link href="/forgot-password" className="font-semibold text-indigo hover:underline">
              Forgot password?
            </Link>
          }
        >
          <Input
            id="password"
            type="password"
            autoComplete="current-password"
            required
            placeholder="Your password"
            value={password}
            onChange={(event) => setPassword(event.target.value)}
          />
        </Field>
        <Button type="submit" className="w-full" size="lg" loading={busy}>
          Sign in
        </Button>
      </form>

      <p className="mt-6 text-center text-xs text-muted">
        New to Invoxa?{" "}
        <Link href="/signup" className="font-semibold text-indigo hover:underline">
          Create a workspace
        </Link>
      </p>
      <div className="mt-8 rounded-md border border-line bg-surface px-4 py-3 text-[11px] leading-5 text-muted">
        <strong className="font-semibold text-ink-soft">Local demo:</strong> if you ran the seed command, sign in with{" "}
        <span className="font-mono text-ink-soft">demo@invoxa.local</span>. The password is printed by the seed command.
      </div>
    </div>
  );
}
