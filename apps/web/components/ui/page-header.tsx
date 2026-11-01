import * as React from "react";
import { cn } from "@/lib/cn";

export function PageHeader({
  eyebrow,
  title,
  description,
  actions,
  className,
}: {
  eyebrow?: string;
  title: string;
  description?: React.ReactNode;
  actions?: React.ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between", className)}>
      <div>
        {eyebrow ? <p className="label-caps mb-2">{eyebrow}</p> : null}
        <h1 className="font-display text-3xl leading-[1.08] tracking-[-0.02em] text-ink sm:text-[2.35rem]">
          {title}
        </h1>
        {description ? (
          <div className="mt-2 max-w-2xl text-sm leading-6 text-muted">{description}</div>
        ) : null}
      </div>
      {actions ? <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div> : null}
    </div>
  );
}
