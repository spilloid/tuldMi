import { forwardRef } from "react";
import type { ButtonHTMLAttributes, ReactNode } from "react";
import { Tooltip } from "./Tooltip";

type Variant = "primary" | "secondary" | "ghost" | "danger" | "quiet";
type Size = "sm" | "md" | "lg";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
  icon?: ReactNode;
  trailing?: ReactNode;
}

const base =
  "inline-flex items-center justify-center gap-1.5 whitespace-nowrap rounded-md font-medium select-none " +
  "transition-[background-color,border-color,color,box-shadow,transform] duration-[140ms] ease-out " +
  "active:translate-y-px disabled:pointer-events-none disabled:opacity-45";

const variants: Record<Variant, string> = {
  primary:
    "bg-accent text-on-accent shadow-1 hover:bg-accent-strong border border-[color-mix(in_oklch,var(--tm-accent-strong),black_8%)]",
  secondary: "bg-raised text-text border border-line shadow-1 hover:border-line-strong hover:bg-surface",
  ghost: "text-muted hover:text-text hover:bg-hover active:bg-press",
  quiet: "text-text hover:bg-hover active:bg-press",
  danger: "bg-danger text-on-accent shadow-1 hover:brightness-95 border border-[color-mix(in_oklch,var(--tm-danger),black_10%)]",
};

const sizes: Record<Size, string> = {
  sm: "h-7 px-2.5 text-sm",
  md: "h-8 px-3 text-base",
  lg: "h-10 px-4 text-base",
};

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { variant = "secondary", size = "md", icon, trailing, className = "", children, type = "button", ...rest },
  ref,
) {
  return (
    <button ref={ref} type={type} className={`${base} ${variants[variant]} ${sizes[size]} ${className}`} {...rest}>
      {icon}
      {children}
      {trailing}
    </button>
  );
});

interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  label: string;
  shortcut?: string;
  size?: "sm" | "md";
  tooltipSide?: "top" | "bottom" | "left" | "right";
  active?: boolean;
}

export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton(
  { label, shortcut, size = "md", tooltipSide = "top", active, className = "", children, type = "button", ...rest },
  ref,
) {
  const dim = size === "sm" ? "h-7 w-7" : "h-8 w-8";
  return (
    <Tooltip content={label} shortcut={shortcut} side={tooltipSide}>
      <button
        ref={ref}
        type={type}
        aria-label={label}
        className={`inline-flex ${dim} items-center justify-center rounded-md text-muted transition-colors duration-[140ms] hover:bg-hover hover:text-text active:bg-press disabled:opacity-40 ${
          active ? "bg-selected text-text" : ""
        } ${className}`}
        {...rest}
      >
        {children}
      </button>
    </Tooltip>
  );
});
