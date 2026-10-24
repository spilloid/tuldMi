import { createContext, useCallback, useContext, useMemo, useState } from "react";
import type { ReactNode } from "react";

interface UiState {
  selectedId: string | null;
  expanded: Set<string>;
  flashId: string | null;
  /** Bumped to ask the timeline to scroll the selected event into view. */
  revealSeq: number;
  paletteOpen: boolean;
  shortcutsOpen: boolean;
}

interface UiApi extends UiState {
  select(id: string | null, opts?: { reveal?: boolean }): void;
  toggleExpanded(id: string, force?: boolean): void;
  /** Select, expand, scroll to and flash an event (search jump). */
  jumpTo(id: string): void;
  setPaletteOpen(v: boolean): void;
  setShortcutsOpen(v: boolean): void;
}

const Ctx = createContext<UiApi | null>(null);

export function UiProvider({ children, initial }: { children: ReactNode; initial?: Partial<UiState> }) {
  const [state, setState] = useState<UiState>({
    selectedId: null,
    expanded: new Set(),
    flashId: null,
    revealSeq: 0,
    paletteOpen: false,
    shortcutsOpen: false,
    ...initial,
  });

  const select = useCallback((id: string | null, opts?: { reveal?: boolean }) => {
    setState((s) => ({ ...s, selectedId: id, revealSeq: opts?.reveal ? s.revealSeq + 1 : s.revealSeq }));
  }, []);
  const toggleExpanded = useCallback((id: string, force?: boolean) => {
    setState((s) => {
      const next = new Set(s.expanded);
      const open = force ?? !next.has(id);
      if (open) next.add(id);
      else next.delete(id);
      return { ...s, expanded: next };
    });
  }, []);
  const jumpTo = useCallback((id: string) => {
    setState((s) => {
      const next = new Set(s.expanded);
      next.add(id);
      return { ...s, selectedId: id, expanded: next, flashId: id, revealSeq: s.revealSeq + 1, paletteOpen: false };
    });
    setTimeout(() => setState((s) => (s.flashId === id ? { ...s, flashId: null } : s)), 1900);
  }, []);
  const setPaletteOpen = useCallback((v: boolean) => setState((s) => ({ ...s, paletteOpen: v })), []);
  const setShortcutsOpen = useCallback((v: boolean) => setState((s) => ({ ...s, shortcutsOpen: v })), []);

  const api = useMemo(
    () => ({ ...state, select, toggleExpanded, jumpTo, setPaletteOpen, setShortcutsOpen }),
    [state, select, toggleExpanded, jumpTo, setPaletteOpen, setShortcutsOpen],
  );
  return <Ctx.Provider value={api}>{children}</Ctx.Provider>;
}

export function useUi(): UiApi {
  const c = useContext(Ctx);
  if (!c) throw new Error("UiProvider missing");
  return c;
}
