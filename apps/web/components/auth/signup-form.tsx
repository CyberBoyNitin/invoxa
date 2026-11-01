"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import * as React from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Field, Input, Select } from "@/components/ui/input";
import { apiFetch, errorMessage } from "@/lib/api";
import type { Session } from "@/lib/types";

const countries = [
  { code: "IN", label: "India", currency: "INR" },
  { code: "US", label: "United States", currency: "USD" },
  { code: "GB", label: "United Kingdom", currency: "GBP" },
  { code: "AE", label: "United Arab Emirates", currency: "AED" },
  { code: "SG", label: "Singapore", currency: "SGD" },
  { code: "AU", label: "Australia", currency: "AUD" },
  { code: "CA", label: "Canada", currency: "CAD" },
  { code: "DE", label: "Germany", currency: "EUR" },
];

export function SignupForm() {
  const [form, setForm] = React.useState({
    name: "",
    email: "",
    password: "",
    organization_name: "",
    country: "IN",
    base_currency: "INR",
  });
  const [error, setError] = React.useState("");
  const [busy, setBusy] = React.useState(false);
  const router = useRouter();

  function update<K extends keyof typeof form>(key: K, value: (typeof form)[K]) {
    setForm((current) => {
      const next = { ...current, [key]: value };
      if (key === "country") {
        next.base_currency = countries.find((country) => country.code === value)?.currency ?? "INR";
      }
      return next;
    });
  }

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (form.password.length < 12) {
      setError("Use at least 12 characters for your password.");
      return;
    }
    setBusy(true);
    setError("");
    try {
      await apiFetch<Session>("/auth/signup", { method: "POST", body: form });
      toast.success("Workspace created.");
      router.replace("/dashboard");
      router.refresh();
    } catch (caught) {
      setError(errorMessage(caught));
      setBusy(false);
    }
  }

  return (
    <div>
      <p className="label-caps">Create a workspace</p>
      <h2 className="mt-2 font-display text-[2.4rem] leading-[1.05] tracking-[-0.03em] text-ink">
        Start with a clean ledger
      </h2>
      <p className="mt-3 text-sm leading-6 text-muted">
        Your workspace is isolated from every other organization. You will be its owner.
      </p>

      <form className="mt-7 space-y-4" onSubmit={submit}>
        {error ? (
          <div role="alert" className="rounded-md border border-rust/25 bg-rust-soft px-3.5 py-3 text-xs leading-5 text-rust">
            {error}
          </div>
        ) : null}
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label="Your name" htmlFor="name">
            <Input id="name" autoComplete="name" required maxLength={160} value={form.name} onChange={(event) => update("name", event.target.value)} />
          </Field>
          <Field label="Work email" htmlFor="email">
            <Input id="email" type="email" autoComplete="email" required value={form.email} onChange={(event) => update("email", event.target.value)} />
          </Field>
        </div>
        <Field label="Organization name" htmlFor="organization_name">
          <Input
            id="organization_name"
            required
            minLength={2}
            maxLength={120}
            placeholder="Northstar Finance"
            value={form.organization_name}
            onChange={(event) => update("organization_name", event.target.value)}
          />
        </Field>
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label="Country" htmlFor="country">
            <Select id="country" value={form.country} onChange={(event) => update("country", event.target.value)}>
              {countries.map((country) => (
                <option key={country.code} value={country.code}>
                  {country.label}
                </option>
              ))}
            </Select>
          </Field>
          <Field label="Base currency" htmlFor="base_currency" hint="ISO code">
            <Input
              id="base_currency"
              required
              maxLength={3}
              value={form.base_currency}
              onChange={(event) => update("base_currency", event.target.value.toUpperCase())}
            />
          </Field>
        </div>
        <Field label="Password" htmlFor="password" hint="At least 12 characters">
          <Input
            id="password"
            type="password"
            autoComplete="new-password"
            required
            minLength={12}
            value={form.password}
            onChange={(event) => update("password", event.target.value)}
          />
        </Field>
        <Button type="submit" size="lg" className="w-full" loading={busy}>
          Create workspace
        </Button>
      </form>

      <p className="mt-6 text-center text-xs text-muted">
        Already have an account?{" "}
        <Link href="/login" className="font-semibold text-indigo hover:underline">
          Sign in
        </Link>
      </p>
    </div>
  );
}
