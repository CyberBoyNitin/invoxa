"use client";

import { Slot } from "@radix-ui/react-slot";
import * as React from "react";
import { Loader2 } from "lucide-react";
import { cn } from "@/lib/cn";

type Variant = "primary" | "secondary" | "ghost" | "danger" | "outline";
type Size = "sm" | "md" | "lg" | "icon";

const variants: Record<Variant, string> = {
  primary:
    "bg-indigo text-white border-indigo hover:bg-indigo-dark hover:border-indigo-dark shadow-[0_1px_0_rgba(255,255,255,0.14)_inset]",
  secondary:
    "bg-ink text-white border-ink hover:bg-black",
  ghost:
    "bg-transparent text-ink-soft border-transparent hover:bg-paper-deep hover:text-ink",
  danger:
    "bg-rust text-white border-rust hover:bg-[#8b3429]",
  outline:
    "bg-surface text-ink border-line-strong hover:bg-paper-deep",
};

const sizes: Record<Size, string> = {
  sm: "h-8 px-3 text-xs gap-1.5",
  md: "h-10 px-4 text-sm gap-2",
  lg: "h-11 px-5 text-sm gap-2",
  icon: "h-9 w-9 p-0",
};

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
  loading?: boolean;
  asChild?: boolean;
}

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant = "primary", size = "md", loading, disabled, children, asChild, ...props }, ref) => {
    const classes = cn(
      "inline-flex select-none items-center justify-center rounded-md border font-semibold transition-colors duration-150 disabled:cursor-not-allowed disabled:opacity-50",
      variants[variant],
      sizes[size],
      className,
    );
    if (asChild) {
      return (
        <Slot ref={ref} className={classes} {...props}>
          {children}
        </Slot>
      );
    }
    return (
      <button
        ref={ref}
        className={classes}
        disabled={disabled || loading}
        {...props}
      >
        {loading ? <Loader2 className="size-4 animate-spin" aria-hidden /> : null}
        {children}
      </button>
    );
  },
);
Button.displayName = "Button";
