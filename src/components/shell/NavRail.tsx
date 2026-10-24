import { AudioLines, Keyboard, Search, Settings2 } from "lucide-react";
import type { ReactNode } from "react";
import { navigate } from "../../lib/router";
import type { Route } from "../../lib/router";
import { Tooltip } from "../ui/Tooltip";

export function NavRail({ route, onShortcuts }: { route: Route; onShortcuts: () => void }) {
  return (
    <nav aria-label="Main" className="flex w-[var(--tm-rail-w)] shrink-0 flex-col items-center gap-1 border-r border-line-soft bg-canvas py-3">
      <RailItem label="Timeline" shortcut="T" active={route.view === "timeline"} onClick={() => navigate("/timeline")}>
        <AudioLines size={18} strokeWidth={1.75} />
      </RailItem>
      <RailItem label="Search" shortcut="CommandOrControl+K" active={route.view === "search"} onClick={() => navigate("/search")}>
        <Search size={18} strokeWidth={1.75} />
      </RailItem>
      <div className="flex-1" />
      <RailItem label="Keyboard shortcuts" shortcut="?" onClick={onShortcuts}>
        <Keyboard size={18} strokeWidth={1.75} />
      </RailItem>
      <RailItem label="Settings" shortcut="CommandOrControl+Comma" active={route.view === "settings"} onClick={() => navigate("/settings/audio")}>
        <Settings2 size={18} strokeWidth={1.75} />
      </RailItem>
    </nav>
  );
}

function RailItem({ label, shortcut, active, onClick, children }: { label: string; shortcut?: string; active?: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <Tooltip content={label} shortcut={shortcut} side="right">
      <button
        type="button"
        aria-label={label}
        aria-current={active ? "page" : undefined}
        onClick={onClick}
        className={`relative inline-flex h-9 w-9 items-center justify-center rounded-lg transition-colors duration-[140ms] ${
          active ? "bg-selected text-text" : "text-faint hover:bg-hover hover:text-text"
        }`}
      >
        {active && <span aria-hidden className="absolute -left-[12px] top-2 bottom-2 w-[3px] rounded-r-full bg-accent" />}
        {children}
      </button>
    </Tooltip>
  );
}
