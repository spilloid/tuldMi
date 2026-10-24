import * as RSwitch from "@radix-ui/react-switch";
import * as RSlider from "@radix-ui/react-slider";
import * as DM from "@radix-ui/react-dropdown-menu";
import { Check, ChevronDown } from "lucide-react";
import { useId } from "react";
import type { ReactNode } from "react";

// ---- Switch ------------------------------------------------------------------------------------

export function Switch({
  checked,
  onCheckedChange,
  label,
  disabled,
  id,
}: {
  checked: boolean;
  onCheckedChange: (v: boolean) => void;
  label: string;
  disabled?: boolean;
  id?: string;
}) {
  return (
    <RSwitch.Root
      id={id}
      checked={checked}
      onCheckedChange={onCheckedChange}
      disabled={disabled}
      aria-label={label}
      className="group relative inline-flex h-[20px] w-[34px] shrink-0 items-center rounded-full border border-line-strong bg-sunken transition-colors duration-[180ms] ease-out data-[state=checked]:border-accent-strong data-[state=checked]:bg-accent disabled:opacity-45"
    >
      <RSwitch.Thumb className="block h-[14px] w-[14px] translate-x-[2px] rounded-full bg-raised shadow-1 ring-1 ring-black/5 transition-transform duration-[180ms] ease-out data-[state=checked]:translate-x-[16px] data-[state=checked]:bg-white" />
    </RSwitch.Root>
  );
}

// ---- Slider ------------------------------------------------------------------------------------

export function Slider({
  value,
  min,
  max,
  step,
  onValueChange,
  onValueCommit,
  label,
  format,
}: {
  value: number;
  min: number;
  max: number;
  step: number;
  onValueChange: (v: number) => void;
  onValueCommit?: (v: number) => void;
  label: string;
  format?: (v: number) => string;
}) {
  return (
    <RSlider.Root
      value={[value]}
      min={min}
      max={max}
      step={step}
      onValueChange={(v) => onValueChange(v[0])}
      onValueCommit={(v) => onValueCommit?.(v[0])}
      className="relative flex h-5 w-full touch-none select-none items-center"
    >
      <RSlider.Track className="relative h-[4px] grow overflow-hidden rounded-full bg-line">
        <RSlider.Range className="absolute h-full rounded-full bg-accent" />
      </RSlider.Track>
      <RSlider.Thumb
        aria-label={label}
        aria-valuetext={format?.(value)}
        className="block h-4 w-4 rounded-full border border-line-strong bg-raised shadow-2 transition-transform duration-[140ms] hover:scale-110 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--tm-focus)]"
      />
    </RSlider.Root>
  );
}

// ---- Select (dropdown with radio items) ---------------------------------------------------------

export interface SelectOption<T extends string> {
  value: T;
  label: string;
  hint?: string;
}

export function Select<T extends string>({
  value,
  options,
  onChange,
  label,
  className = "",
  disabled,
}: {
  value: T;
  options: SelectOption<T>[];
  onChange: (v: T) => void;
  label: string;
  className?: string;
  disabled?: boolean;
}) {
  const current = options.find((o) => o.value === value);
  return (
    <DM.Root modal={false}>
      <DM.Trigger
        disabled={disabled}
        aria-label={`${label}: ${current?.label ?? value}`}
        className={`inline-flex h-8 min-w-0 items-center justify-between gap-2 rounded-md border border-line bg-raised px-2.5 text-left text-base text-text shadow-1 transition-colors duration-[140ms] hover:border-line-strong data-[state=open]:border-accent-line disabled:opacity-45 ${className}`}
      >
        <span className="truncate">{current?.label ?? "Choose…"}</span>
        <ChevronDown size={14} className="shrink-0 text-faint" aria-hidden />
      </DM.Trigger>
      <DM.Portal>
        <DM.Content align="start" sideOffset={4} collisionPadding={8} className="tm-pop tm-menu z-[70] min-w-[var(--radix-dropdown-menu-trigger-width)] max-w-[420px]">
          <DM.RadioGroup value={value} onValueChange={(v) => onChange(v as T)}>
            {options.map((o) => (
              <DM.RadioItem key={o.value} value={o.value} className="tm-menu-item pl-7">
                <DM.ItemIndicator className="absolute left-2 inline-flex">
                  <Check size={14} className="text-accent-text" />
                </DM.ItemIndicator>
                <span className="min-w-0 flex-1">
                  <span className="block truncate">{o.label}</span>
                  {o.hint && <span className="block truncate text-xs text-faint">{o.hint}</span>}
                </span>
              </DM.RadioItem>
            ))}
          </DM.RadioGroup>
        </DM.Content>
      </DM.Portal>
    </DM.Root>
  );
}

// ---- Segmented control -------------------------------------------------------------------------

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
  size = "md",
}: {
  value: T;
  options: { value: T; label: ReactNode; title?: string }[];
  onChange: (v: T) => void;
  label: string;
  size?: "sm" | "md";
}) {
  const name = useId();
  const h = size === "sm" ? "h-7 text-sm" : "h-8 text-base";
  return (
    <div role="radiogroup" aria-label={label} className={`inline-flex items-center gap-0.5 rounded-md border border-line bg-sunken p-0.5 ${h}`}>
      {options.map((o) => {
        const on = o.value === value;
        return (
          <button
            key={o.value}
            type="button"
            role="radio"
            aria-checked={on}
            name={name}
            title={o.title}
            onClick={() => onChange(o.value)}
            onKeyDown={(e) => {
              const i = options.findIndex((x) => x.value === value);
              if (e.key === "ArrowRight" || e.key === "ArrowDown") {
                e.preventDefault();
                onChange(options[(i + 1) % options.length].value);
              } else if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
                e.preventDefault();
                onChange(options[(i - 1 + options.length) % options.length].value);
              }
            }}
            tabIndex={on ? 0 : -1}
            className={`inline-flex h-full items-center gap-1.5 rounded-[5px] px-2.5 font-medium transition-[background-color,color,box-shadow] duration-[140ms] ${
              on ? "bg-raised text-text shadow-1" : "text-muted hover:text-text"
            }`}
          >
            {o.label}
          </button>
        );
      })}
    </div>
  );
}

// ---- Misc ----------------------------------------------------------------------------------------

export function Pill({ children, tone = "neutral", className = "" }: { children: ReactNode; tone?: "neutral" | "accent" | "me" | "remote" | "warning" | "danger" | "success"; className?: string }) {
  const tones = {
    neutral: "bg-sunken text-muted border-line",
    accent: "bg-accent-soft text-accent-text border-accent-line",
    me: "bg-me-soft text-me-text border-me-line",
    remote: "bg-remote-soft text-remote-text border-remote-line",
    warning: "bg-warning-soft text-warning border-[color-mix(in_oklch,var(--tm-warning),transparent_60%)]",
    danger: "bg-danger-soft text-danger border-[color-mix(in_oklch,var(--tm-danger),transparent_60%)]",
    success: "bg-success-soft text-success border-[color-mix(in_oklch,var(--tm-success),transparent_60%)]",
  } as const;
  return (
    <span className={`inline-flex h-5 items-center gap-1 rounded-full border px-2 text-xs font-medium ${tones[tone]} ${className}`}>
      {children}
    </span>
  );
}

export function ProgressBar({ value, tone = "accent", label }: { value: number; tone?: "accent" | "muted"; label: string }) {
  const pct = Math.max(0, Math.min(1, value)) * 100;
  return (
    <div role="progressbar" aria-label={label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(pct)} className="h-1.5 w-full overflow-hidden rounded-full bg-line">
      <div
        className={`h-full rounded-full transition-[width] duration-[220ms] ease-out ${tone === "accent" ? "bg-accent" : "bg-muted"}`}
        style={{ width: `${pct}%` }}
      />
    </div>
  );
}
