"use client";

import { Building2, Gauge, KeyRound, Users } from "lucide-react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { cn } from "@/lib/cn";

const tabs = [
  { href: "/settings/organization", label: "Organization", icon: Building2 },
  { href: "/settings/team", label: "Team", icon: Users },
  { href: "/settings/api-keys", label: "API & webhooks", icon: KeyRound },
  { href: "/settings/usage", label: "Usage", icon: Gauge },
];

export function SettingsTabs() {
  const pathname = usePathname();
  return (
    <nav aria-label="Settings sections" className="flex gap-1 overflow-x-auto border-b border-line">
      {tabs.map((tab) => {
        const active = pathname === tab.href;
        const Icon = tab.icon;
        return (
          <Link
            key={tab.href}
            href={tab.href}
            aria-current={active ? "page" : undefined}
            className={cn(
              "relative flex shrink-0 items-center gap-2 px-3.5 py-3 text-xs font-semibold transition-colors",
              active ? "text-ink" : "text-muted hover:text-ink",
            )}
          >
            <Icon className="size-3.5" />
            {tab.label}
            {active ? <span className="absolute inset-x-2 bottom-0 h-0.5 rounded-full bg-indigo" /> : null}
          </Link>
        );
      })}
    </nav>
  );
}
