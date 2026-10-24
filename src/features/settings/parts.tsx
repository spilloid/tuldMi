import type { ReactNode } from "react";

export function SectionHeader({ title, description }: { title: string; description?: ReactNode }) {
  return (
    <header className="mb-6">
      <h1 className="font-display text-[26px] font-[460] leading-tight tracking-[-0.02em] text-text">{title}</h1>
      {description && <p className="mt-1.5 max-w-[560px] text-base text-muted [text-wrap:pretty]">{description}</p>}
    </header>
  );
}

export function Group({ title, children, footer, className = "" }: { title?: string; children: ReactNode; footer?: ReactNode; className?: string }) {
  return (
    <section className={`mb-7 ${className}`}>
      {title && <h2 className="caps mb-2 px-0.5 text-faint">{title}</h2>}
      <div className="divide-y divide-line-soft overflow-hidden rounded-xl border border-line bg-surface shadow-1">{children}</div>
      {footer && <p className="mt-2 px-0.5 text-sm text-faint">{footer}</p>}
    </section>
  );
}

export function Row({
  label,
  description,
  children,
  htmlFor,
  align = "center",
}: {
  label: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
  htmlFor?: string;
  align?: "center" | "start";
}) {
  return (
    <div className={`flex gap-6 px-4 py-3.5 ${align === "center" ? "items-center" : "items-start"}`}>
      <div className="min-w-0 flex-1">
        <label htmlFor={htmlFor} className="block text-base font-medium text-text">
          {label}
        </label>
        {description && <div className="mt-0.5 text-sm text-muted [text-wrap:pretty]">{description}</div>}
      </div>
      {children && <div className="flex shrink-0 items-center gap-2">{children}</div>}
    </div>
  );
}
