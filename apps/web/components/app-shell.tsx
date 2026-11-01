"use client";

import { useQueryClient } from "@tanstack/react-query";
import {
  Activity,
  BadgeCheck,
  Building2,
  ChevronDown,
  FileStack,
  Inbox,
  LayoutDashboard,
  LogOut,
  Menu,
  Plug,
  ReceiptText,
  Settings,
  Sparkles,
  Upload,
  Users,
  Webhook,
  X,
} from "lucide-react";
import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import * as React from "react";
import { toast } from "sonner";
import { UploadDialog } from "@/components/upload-dialog";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { apiFetch, errorMessage } from "@/lib/api";
import { cn } from "@/lib/cn";
import { initials } from "@/lib/format";
import type { Session } from "@/lib/types";

const primaryNav = [
  { href: "/dashboard", label: "Overview", icon: LayoutDashboard },
  { href: "/review", label: "Review Queue", icon: Inbox },
  { href: "/invoices", label: "Invoices", icon: ReceiptText },
  { href: "/vendors", label: "Vendors", icon: Building2 },
  { href: "/documents", label: "Documents", icon: FileStack },
  { href: "/automation", label: "Automations", icon: Sparkles },
  { href: "/integrations", label: "Integrations", icon: Plug },
  { href: "/settings/api-keys", label: "API & Webhooks", icon: Webhook },
  { href: "/activity", label: "Activity Log", icon: Activity },
];

function NavLink({
  href,
  label,
  icon: Icon,
  onNavigate,
}: {
  href: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  onNavigate?: () => void;
}) {
  const pathname = usePathname();
  const active =
    pathname === href ||
    (href !== "/dashboard" && pathname.startsWith(`${href}/`)) ||
    (href === "/settings/api-keys" && pathname.startsWith("/settings/api-keys"));
  return (
    <Link
      href={href}
      onClick={onNavigate}
      className={cn(
        "group flex items-center gap-3 rounded-md px-3 py-2 text-[13px] font-medium transition-colors",
        active
          ? "bg-white/10 text-white"
          : "text-[#b9bdc8] hover:bg-white/[0.06] hover:text-white",
      )}
      aria-current={active ? "page" : undefined}
    >
      <Icon className={cn("size-4 shrink-0", active ? "text-[#c8cdf7]" : "text-[#8e94a4] group-hover:text-[#c8cdf7]")} />
      <span>{label}</span>
      {label === "Review Queue" ? (
        <span className="ml-auto rounded-full bg-[#c8cdf7]/15 px-1.5 py-0.5 text-[10px] font-semibold text-[#dfe2ff]">
          queue
        </span>
      ) : null}
    </Link>
  );
}

function SidebarContent({ session, onNavigate }: { session: Session; onNavigate?: () => void }) {
  return (
    <>
      <div className="flex h-[72px] items-center gap-3 border-b border-white/10 px-5">
        <div className="grid size-9 place-items-center rounded-md border border-white/20 bg-white/[0.07]">
          <span className="font-display text-lg font-semibold tracking-[-0.04em] text-white">IX</span>
        </div>
        <div>
          <p className="font-display text-xl leading-none text-white">Invoxa</p>
          <p className="mt-1 text-[9px] uppercase tracking-[0.18em] text-[#8e94a4]">Invoice intelligence</p>
        </div>
      </div>
      <nav className="flex-1 overflow-y-auto px-3 py-5">
        <p className="mb-2 px-3 text-[10px] font-semibold uppercase tracking-[0.16em] text-[#707684]">Workspace</p>
        <div className="space-y-0.5">
          {primaryNav.map((item) => (
            <NavLink key={item.href} {...item} onNavigate={onNavigate} />
          ))}
        </div>
        <p className="mb-2 mt-7 px-3 text-[10px] font-semibold uppercase tracking-[0.16em] text-[#707684]">Administration</p>
        <div className="space-y-0.5">
          <NavLink href="/settings/organization" label="Settings" icon={Settings} onNavigate={onNavigate} />
          <NavLink href="/settings/team" label="Team & access" icon={Users} onNavigate={onNavigate} />
        </div>
      </nav>
      <div className="border-t border-white/10 p-3">
        <div className="rounded-md border border-white/10 bg-white/[0.04] px-3 py-3">
          <div className="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-[0.1em] text-[#c8cdf7]">
            <BadgeCheck className="size-3.5" />
            Human approval required
          </div>
          <p className="mt-1.5 text-[11px] leading-4 text-[#8e94a4]">
            No invoice leaves Invoxa without an explicit review decision.
          </p>
        </div>
      </div>
    </>
  );
}

export function AppShell({ session, children }: { session: Session; children: React.ReactNode }) {
  const [uploadOpen, setUploadOpen] = React.useState(false);
  const [mobileOpen, setMobileOpen] = React.useState(false);
  const [switching, setSwitching] = React.useState<string | null>(null);
  const router = useRouter();
  const queryClient = useQueryClient();
  const org = session.active_organization.organization;
  const role = session.active_organization.role;

  async function switchOrganization(organizationId: string) {
    if (organizationId === org.id) return;
    setSwitching(organizationId);
    try {
      await apiFetch("/auth/switch-organization", {
        method: "POST",
        body: { organization_id: organizationId },
      });
      queryClient.clear();
      router.refresh();
      toast.success("Organization switched.");
    } catch (error) {
      toast.error(errorMessage(error));
    } finally {
      setSwitching(null);
    }
  }

  async function logout() {
    try {
      await apiFetch("/auth/logout", { method: "POST" });
      queryClient.clear();
      router.push("/login");
      router.refresh();
    } catch (error) {
      toast.error(errorMessage(error));
    }
  }

  return (
    <div className="min-h-screen">
      <aside className="fixed inset-y-0 left-0 z-30 hidden w-[244px] flex-col bg-[var(--sidebar)] lg:flex">
        <SidebarContent session={session} />
      </aside>

      {mobileOpen ? (
        <div className="fixed inset-0 z-50 lg:hidden">
          <button
            className="absolute inset-0 bg-ink/50 backdrop-blur-[2px]"
            aria-label="Close navigation"
            onClick={() => setMobileOpen(false)}
          />
          <aside className="relative flex h-full w-[280px] flex-col bg-[var(--sidebar)] shadow-2xl">
            <button
              className="absolute right-3 top-5 z-10 grid size-8 place-items-center rounded text-[#9da1ad] hover:bg-white/10 hover:text-white"
              onClick={() => setMobileOpen(false)}
              aria-label="Close navigation"
            >
              <X className="size-4" />
            </button>
            <SidebarContent session={session} onNavigate={() => setMobileOpen(false)} />
          </aside>
        </div>
      ) : null}

      <div className="lg:pl-[244px]">
        <header className="sticky top-0 z-20 border-b border-line bg-paper/92 backdrop-blur-md">
          <div className="flex h-[64px] items-center gap-3 px-4 sm:px-6 lg:px-8">
            <button
              className="grid size-9 place-items-center rounded-md border border-line bg-surface text-ink-soft lg:hidden"
              onClick={() => setMobileOpen(true)}
              aria-label="Open navigation"
            >
              <Menu className="size-4" />
            </button>

            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button className="flex min-w-0 items-center gap-2 rounded-md px-1.5 py-1.5 text-left hover:bg-paper-deep">
                  <span className="grid size-7 shrink-0 place-items-center rounded bg-ink text-[10px] font-bold text-white">
                    {initials(org.name)}
                  </span>
                  <span className="hidden min-w-0 sm:block">
                    <span className="block max-w-[240px] truncate text-xs font-semibold text-ink">{org.name}</span>
                    <span className="block text-[10px] capitalize text-muted">{role?.replace("_", " ")} workspace</span>
                  </span>
                  <ChevronDown className="size-3.5 shrink-0 text-muted" />
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent className="w-64">
                <DropdownMenuLabel>Organizations</DropdownMenuLabel>
                {session.memberships.map((membership) => (
                  <DropdownMenuItem
                    key={membership.organization.id}
                    onSelect={() => void switchOrganization(membership.organization.id)}
                    disabled={switching !== null}
                    className="justify-between"
                  >
                    <span className="truncate">{membership.organization.name}</span>
                    <span className="text-[10px] capitalize text-muted">{membership.role.replace("_", " ")}</span>
                  </DropdownMenuItem>
                ))}
                <DropdownMenuSeparator />
                <DropdownMenuItem asChild>
                  <Link href="/settings/organization">Organization settings</Link>
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>

            <div className="ml-auto flex items-center gap-2">
              <Button size="sm" onClick={() => setUploadOpen(true)} className="hidden sm:inline-flex">
                <Upload className="size-3.5" />
                Add documents
              </Button>
              <Button size="icon" variant="outline" onClick={() => setUploadOpen(true)} className="sm:hidden" aria-label="Add documents">
                <Upload className="size-4" />
              </Button>
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <button className="grid size-9 place-items-center rounded-full border border-line bg-surface text-xs font-semibold text-ink hover:bg-paper-deep" aria-label="Account menu">
                    {initials(session.user?.name ?? "User")}
                  </button>
                </DropdownMenuTrigger>
                <DropdownMenuContent className="w-60">
                  <DropdownMenuLabel>Signed in</DropdownMenuLabel>
                  <div className="px-2.5 pb-2">
                    <p className="truncate text-sm font-semibold text-ink">{session.user?.name}</p>
                    <p className="truncate text-xs text-muted">{session.user?.email}</p>
                  </div>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem asChild>
                    <Link href="/settings/organization">Settings</Link>
                  </DropdownMenuItem>
                  <DropdownMenuItem onSelect={() => void logout()} className="text-rust focus:text-rust">
                    <LogOut className="size-3.5" />
                    Sign out
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
          </div>
        </header>

        {org.is_demo ? (
          <div className="border-b border-amber/20 bg-amber-soft/70 px-4 py-2 text-center text-[11px] leading-5 text-amber sm:px-6">
            <strong>Demo workspace.</strong> The sample data and deterministic mock extractor are for evaluation only.
            Configure a production AI provider and review consent before processing real invoices.
          </div>
        ) : null}

        <main className="mx-auto min-h-[calc(100vh-64px)] max-w-[1480px] px-4 py-7 sm:px-6 lg:px-8 lg:py-9">
          <div className="page-enter">{children}</div>
        </main>
      </div>

      <UploadDialog open={uploadOpen} onOpenChange={setUploadOpen} />
    </div>
  );
}
