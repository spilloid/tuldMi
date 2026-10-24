import * as RD from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import type { ReactNode } from "react";

export function Dialog({
  open,
  onOpenChange,
  title,
  description,
  children,
  width = 460,
  hideClose,
  initialFocus,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  title: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
  width?: number;
  hideClose?: boolean;
  initialFocus?: () => HTMLElement | null;
}) {
  return (
    <RD.Root open={open} onOpenChange={onOpenChange}>
      <RD.Portal>
        <RD.Overlay className="tm-scrim fixed inset-0 z-[60]" />
        <RD.Content
          onOpenAutoFocus={(e) => {
            // Focus the requested control, or the dialog itself — not the close button,
            // whose focus ring would read as a selected state on open.
            e.preventDefault();
            const el = initialFocus?.() ?? (e.currentTarget as HTMLElement | null);
            el?.focus();
          }}
          className="tm-dialog outline-none fixed left-1/2 top-[18vh] z-[61] max-h-[72vh] w-[calc(100vw-32px)] -translate-x-1/2 overflow-auto rounded-xl border border-line bg-overlay p-5 shadow-3"
          style={{ maxWidth: width }}
        >
          <div className="flex items-start justify-between gap-4">
            <div className="min-w-0">
              <RD.Title className="text-md font-semibold tracking-[-0.01em] text-text">{title}</RD.Title>
              {description ? (
                <RD.Description className="mt-1 text-base text-muted">{description}</RD.Description>
              ) : (
                <RD.Description className="sr-only">Dialog</RD.Description>
              )}
            </div>
            {!hideClose && (
              <RD.Close
                aria-label="Close"
                className="-mr-1 -mt-1 inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-faint hover:bg-hover hover:text-text"
              >
                <X size={15} />
              </RD.Close>
            )}
          </div>
          {children}
        </RD.Content>
      </RD.Portal>
    </RD.Root>
  );
}
