import * as React from "react";
import { cn } from "@/lib/cn";

const tones = {
  neutral: "border-line-strong bg-paper text-ink-soft",
  indigo: "border-indigo/20 bg-indigo-soft text-indigo-dark",
  moss: "border-moss/20 bg-moss-soft text-moss",
  amber: "border-amber/25 bg-amber-soft text-amber",
  rust: "border-rust/20 bg-rust-soft text-rust",
  dark: "border-ink bg-ink text-white",
} as const;

export type BadgeTone = keyof typeof tones;

export function Badge({
  tone = "neutral",
  className,
  children,
}: {
  tone?: BadgeTone;
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 whitespace-nowrap rounded-full border px-2 py-0.5 text-[11px] font-semibold leading-5",
        tones[tone],
        className,
      )}
    >
      {children}
    </span>
  );
}
