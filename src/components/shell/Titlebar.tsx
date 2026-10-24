import { Minus, Search, Square, X, Copy } from "lucide-react";
import { useEffect, useState } from "react";
import { windowControls } from "../../lib/window";
import { Tooltip } from "../ui/Tooltip";
import { Keys } from "../ui/Kbd";
import { Logo } from "./Logo";

export function Titlebar({ demo, onSearch, title }: { demo: boolean; onSearch?: () => void; title?: string }) {
  const native = windowControls.available();
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    if (!native) return;
    let off: (() => void) | undefined;
    const sync = () => void windowControls.isMaximized().then(setMaximized);
    sync();
    void windowControls.onResized(sync).then((u) => (off = u));
    return () => off?.();
  }, [native]);

  return (
    <header
      data-tauri-drag-region
      className="relative z-30 flex h-[var(--tm-titlebar-h)] shrink-0 items-center border-b border-line-soft bg-canvas pl-3"
    >
      <div data-tauri-drag-region className="flex w-[220px] min-w-0 items-center gap-2">
        <Logo size={20} />
        <span data-tauri-drag-region className="font-display text-[15.5px] font-[560] tracking-[-0.01em] text-text">
          tuldMi
        </span>
        {demo && <DemoPill />}
      </div>

      <div data-tauri-drag-region className="flex flex-1 justify-center px-3">
        {onSearch && (
          <button
            type="button"
            onClick={onSearch}
            className="group flex h-7 w-full max-w-[420px] items-center gap-2 rounded-md border border-line bg-surface px-2.5 text-sm text-faint transition-colors duration-[140ms] hover:border-line-strong hover:text-muted"
          >
            <Search size={14} aria-hidden />
            <span className="flex-1 text-left">Search your conversations</span>
            <Keys combo="CommandOrControl+K" small />
          </button>
        )}
        {!onSearch && title && <span className="text-sm text-muted">{title}</span>}
      </div>

      <div className="flex w-[220px] items-center justify-end self-stretch">
        {native && (
          <div className="flex h-full">
            <WinButton label="Minimize" onClick={() => void windowControls.minimize()}>
              <Minus size={15} strokeWidth={1.5} />
            </WinButton>
            <WinButton label={maximized ? "Restore" : "Maximize"} onClick={() => void windowControls.toggleMaximize()}>
              {maximized ? <Copy size={12} strokeWidth={1.5} className="-scale-x-100" /> : <Square size={12} strokeWidth={1.5} />}
            </WinButton>
            <WinButton label="Close to tray" onClick={() => void windowControls.close()} danger>
              <X size={16} strokeWidth={1.5} />
            </WinButton>
          </div>
        )}
      </div>
    </header>
  );
}

function WinButton({ label, onClick, children, danger }: { label: string; onClick: () => void; children: React.ReactNode; danger?: boolean }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className={`inline-flex h-full w-[46px] items-center justify-center text-muted transition-colors duration-[100ms] ${
        danger ? "hover:bg-[#c42b1c] hover:text-white" : "hover:bg-hover hover:text-text"
      }`}
    >
      {children}
    </button>
  );
}

function DemoPill() {
  return (
    <Tooltip
      side="bottom"
      content={
        <span className="block max-w-64 py-0.5 leading-snug">
          <strong className="font-semibold">You're looking at demo data.</strong> These conversations, devices and models are
          samples. Nothing was recorded, nothing is stored, and meters stay idle because there is no live audio.
        </span>
      }
    >
      <span
        tabIndex={0}
        className="ml-1 inline-flex h-[20px] items-center gap-1.5 rounded-full border border-warning/50 bg-warning-soft px-2 text-[11px] font-semibold tracking-[0.02em] text-warning"
      >
        <span className="h-1.5 w-1.5 rounded-full bg-warning" aria-hidden />
        Demo data
      </span>
    </Tooltip>
  );
}
