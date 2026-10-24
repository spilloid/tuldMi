import * as RD from "@radix-ui/react-dialog";
import { useEffect, useRef, useState } from "react";
import { useUi } from "../../state/ui";
import { EMPTY_FILTERS, SearchPanel } from "./SearchPanel";
import type { SearchFilters } from "./SearchPanel";

export function CommandPalette({ initialText }: { initialText?: string }) {
  const ui = useUi();
  const [filters, setFilters] = useState<SearchFilters>({ ...EMPTY_FILTERS, text: initialText ?? "" });
  const input = useRef<HTMLInputElement>(null);

  // Re-opening selects the previous query so typing replaces it.
  useEffect(() => {
    if (ui.paletteOpen) requestAnimationFrame(() => input.current?.select());
  }, [ui.paletteOpen]);

  return (
    <RD.Root open={ui.paletteOpen} onOpenChange={ui.setPaletteOpen}>
      <RD.Portal>
        <RD.Overlay className="tm-scrim fixed inset-0 z-[60]" />
        <RD.Content
          aria-describedby={undefined}
          className="tm-dialog outline-none fixed left-1/2 top-[12vh] z-[61] flex max-h-[min(580px,76vh)] w-[calc(100vw-32px)] max-w-[680px] -translate-x-1/2 flex-col overflow-hidden rounded-xl border border-line bg-overlay shadow-3"
        >
          <RD.Title className="sr-only">Search conversations</RD.Title>
          <SearchPanel variant="palette" filters={filters} onFilters={setFilters} inputRef={input} />
        </RD.Content>
      </RD.Portal>
    </RD.Root>
  );
}
