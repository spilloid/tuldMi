import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef } from "react";
import { isModKey } from "../lib/platform";
import { isTypingTarget, overlayOpen, plain } from "../lib/keys";
import { hashParams, navigate, useRoute } from "../lib/router";
import { transition, useReducedMotion } from "../lib/motion";
import { useBackend, useStore, useTimeline } from "../state/hooks";
import { useUi } from "../state/ui";
import { audioAvailability, usePlayer } from "../state/player";
import { Titlebar } from "../components/shell/Titlebar";
import { NavRail } from "../components/shell/NavRail";
import { CaptureBar } from "../components/shell/CaptureBar";
import { useToast } from "../components/ui/Toast";
import { Timeline } from "../features/timeline/Timeline";
import { PlayerDock } from "../features/player/PlayerDock";
import { CommandPalette } from "../features/search/CommandPalette";
import { SearchView } from "../features/search/SearchView";
import { SettingsView } from "../features/settings/SettingsView";
import { ShortcutSheet } from "../features/shortcuts/ShortcutSheet";

export function Shell() {
  const route = useRoute();
  const ui = useUi();
  const backend = useBackend();
  const store = useStore();
  const toast = useToast();
  const player = usePlayer();
  const reduced = useReducedMotion();
  const { events } = useTimeline();
  const refs = useRef({ ui, player, events, backend });
  refs.current = { ui, player, events, backend };

  // URL-driven overlays (for screenshots and deep links).
  useEffect(() => {
    const p = hashParams();
    if (p.has("palette")) ui.setPaletteOpen(true);
    if (p.has("keys")) ui.setShortcutsOpen(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Shell -> UI: notices and tray navigation.
  useEffect(() => {
    const offNotice = backend.on("tuldmi://notice", (n) =>
      toast.show({ level: n.level, message: n.message }),
    );
    const offNav = backend.on("tuldmi://navigate", (n) => {
      if (n.view === "settings") navigate(`/settings/${n.section ?? "audio"}`);
      else if (n.view === "search") refs.current.ui.setPaletteOpen(true);
      else navigate("/timeline");
    });
    return () => {
      offNotice();
      offNav();
    };
  }, [backend, toast, store]);

  // Global shortcuts.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const { ui, player, events, backend } = refs.current;
      if (isModKey(e) && !e.altKey && e.key.toLowerCase() === "k") {
        e.preventDefault();
        ui.setPaletteOpen(!ui.paletteOpen);
        return;
      }
      if (isModKey(e) && e.key === ",") {
        e.preventDefault();
        navigate("/settings/audio");
        return;
      }
      if (e.defaultPrevented || isTypingTarget(e.target) || overlayOpen()) return;
      if (plain(e) && e.key === "/") {
        e.preventDefault();
        ui.setPaletteOpen(true);
      } else if (plain(e, true) && e.key === "?") {
        e.preventDefault();
        ui.setShortcutsOpen(true);
      } else if (plain(e) && e.key === "t") {
        navigate("/timeline");
      } else if (plain(e) && e.key === " ") {
        const t = e.target as HTMLElement;
        if (t.closest?.("button:not([role=button]), a, [role=slider], [role=radio], [role=switch]")) return;
        const ev = events.find((x) => x.id === (player.eventId ?? ui.selectedId));
        if (!ev) return;
        e.preventDefault();
        const a = audioAvailability(ev, backend);
        if (a.ok) player.toggle(ev);
        else toast.show({ level: "info", message: a.message, key: "noaudio", duration: 2600 });
      } else if (e.key === "Escape" && !e.ctrlKey) {
        if (player.eventId || ui.selectedId) {
          player.stop();
          ui.select(null);
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [toast]);

  const viewKey = route.view === "settings" ? "settings" : route.view;

  return (
    <div className="flex h-full flex-col bg-canvas text-text">
      <Titlebar demo={backend.kind === "demo"} onSearch={() => ui.setPaletteOpen(true)} />
      <div className="flex min-h-0 flex-1">
        <NavRail route={route} onShortcuts={() => ui.setShortcutsOpen(true)} />
        <main className="flex min-h-0 min-w-0 flex-1 flex-col">
          <AnimatePresence mode="wait" initial={false}>
            <motion.div
              key={viewKey}
              initial={reduced ? { opacity: 0 } : { opacity: 0, y: 6 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, transition: { duration: 0.12 } }}
              transition={transition.view}
              className="flex min-h-0 flex-1 flex-col"
            >
              {route.view === "timeline" && <Timeline routeEventId={route.eventId} />}
              {route.view === "search" && <SearchView params={route.params} />}
              {route.view === "settings" && <SettingsView section={route.section} />}
            </motion.div>
          </AnimatePresence>
          {route.view === "timeline" && <PlayerDock />}
        </main>
      </div>
      <CaptureBar />
      <CommandPalette initialText={hashParams().get("palette") || undefined} />
      <ShortcutSheet />
    </div>
  );
}
