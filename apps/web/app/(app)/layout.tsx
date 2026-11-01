import { cookies } from "next/headers";
import { redirect } from "next/navigation";
import { AppShell } from "@/components/app-shell";
import type { Session } from "@/lib/types";

export const dynamic = "force-dynamic";

async function loadSession(): Promise<Session | null> {
  const cookieStore = await cookies();
  const apiUrl = process.env.INVOXA_API_URL ?? "http://127.0.0.1:8080";
  try {
    const response = await fetch(`${apiUrl}/api/v1/auth/session`, {
      headers: { cookie: cookieStore.toString(), accept: "application/json" },
      cache: "no-store",
    });
    if (!response.ok) return null;
    return (await response.json()) as Session;
  } catch {
    return null;
  }
}

export default async function AppLayout({ children }: { children: React.ReactNode }) {
  const session = await loadSession();
  if (!session) redirect("/login");
  return <AppShell session={session}>{children}</AppShell>;
}
