"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import * as React from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { apiFetch, ApiError, errorMessage } from "@/lib/api";
import type { Session } from "@/lib/types";

export function AcceptInvitation({ token }: { token: string }) {
  const [status, setStatus] = React.useState<"checking" | "signed_out" | "ready" | "busy" | "done" | "error">("checking");
  const [error, setError] = React.useState("");
  const router = useRouter();

  React.useEffect(() => {
    apiFetch<Session>("/auth/session")
      .then(() => setStatus("ready"))
      .catch((caught) => {
        if (caught instanceof ApiError && caught.status === 401) setStatus("signed_out");
        else {
          setError(errorMessage(caught));
          setStatus("error");
        }
      });
  }, []);

  async function accept() {
    setStatus("busy");
    try {
      await apiFetch("/auth/accept-invitation", { method: "POST", body: { token } });
      setStatus("done");
      toast.success("Invitation accepted.");
      router.refresh();
    } catch (caught) {
      setError(errorMessage(caught));
      setStatus("error");
    }
  }

  if (!token) {
    return (
      <div>
        <p className="label-caps">Invitation</p>
        <h2 className="mt-2 font-display text-3xl text-ink">This link is incomplete</h2>
        <p className="mt-3 text-sm leading-6 text-muted">Ask the workspace owner for a new invitation link.</p>
        <Button asChild className="mt-6">
          <Link href="/login">Go to sign in</Link>
        </Button>
      </div>
    );
  }

  return (
    <div>
      <p className="label-caps">Invitation</p>
      <h2 className="mt-2 font-display text-[2.4rem] leading-[1.05] tracking-[-0.03em] text-ink">
        Join the workspace
      </h2>
      <p className="mt-3 text-sm leading-6 text-muted">
        Invitations are tied to the email address they were sent to. Sign in with that account to accept.
      </p>
      <div className="mt-7 space-y-4">
        {status === "checking" ? <div className="shimmer h-11 rounded-md" /> : null}
        {status === "signed_out" ? (
          <div className="space-y-4">
            <div className="rounded-md border border-amber/25 bg-amber-soft px-3.5 py-3 text-xs leading-5 text-amber">
              You need to sign in before accepting this invitation.
            </div>
            <Button asChild className="w-full" size="lg">
              <Link href={`/login?next=${encodeURIComponent(`/accept-invitation?token=${token}`)}`}>Sign in to accept</Link>
            </Button>
            <p className="text-center text-xs text-muted">
              New here?{" "}
              <Link href="/signup" className="font-semibold text-indigo hover:underline">
                Create the account using the invited email
              </Link>
              , then reopen this link.
            </p>
          </div>
        ) : null}
        {status === "ready" ? (
          <Button className="w-full" size="lg" onClick={accept}>
            Accept invitation
          </Button>
        ) : null}
        {status === "busy" ? (
          <Button className="w-full" size="lg" loading>
            Accepting
          </Button>
        ) : null}
        {status === "done" ? (
          <div className="space-y-4">
            <div className="rounded-md border border-moss/25 bg-moss-soft px-3.5 py-3 text-xs text-moss">
              You are now a member of the workspace.
            </div>
            <Button className="w-full" size="lg" onClick={() => router.push("/dashboard")}>
              Open dashboard
            </Button>
          </div>
        ) : null}
        {status === "error" ? (
          <div className="rounded-md border border-rust/25 bg-rust-soft px-3.5 py-3 text-xs leading-5 text-rust">
            {error}
          </div>
        ) : null}
      </div>
    </div>
  );
}
