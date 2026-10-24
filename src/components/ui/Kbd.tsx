import { isMac } from "../../lib/platform";

/** Human labels for accelerator tokens ("CommandOrControl+Shift+Backquote" -> Ctrl ⇧ `). */
export function acceleratorParts(combo: string): string[] {
  const mac = isMac();
  return combo
    .split("+")
    .filter(Boolean)
    .map((raw) => {
      const k = raw.trim();
      const lower = k.toLowerCase();
      if (["commandorcontrol", "cmdorctrl", "mod"].includes(lower)) return mac ? "⌘" : "Ctrl";
      if (["command", "cmd", "super", "meta"].includes(lower)) return mac ? "⌘" : "Win";
      if (["control", "ctrl"].includes(lower)) return mac ? "⌃" : "Ctrl";
      if (["alt", "option"].includes(lower)) return mac ? "⌥" : "Alt";
      if (lower === "shift") return mac ? "⇧" : "Shift";
      if (lower === "backquote") return "`";
      if (lower === "space") return "Space";
      if (lower === "escape" || lower === "esc") return "Esc";
      if (lower === "enter" || lower === "return") return "Enter";
      if (lower === "comma") return ",";
      if (lower === "period") return ".";
      if (lower === "slash") return "/";
      if (lower === "backslash") return "\\";
      if (lower === "minus") return "-";
      if (lower === "equal") return "=";
      if (lower === "arrowup") return "↑";
      if (lower === "arrowdown") return "↓";
      if (lower === "arrowleft") return "←";
      if (lower === "arrowright") return "→";
      if (/^key[a-z]$/.test(lower)) return k.slice(3).toUpperCase();
      if (/^digit\d$/.test(lower)) return k.slice(5);
      return k.length === 1 ? k.toUpperCase() : k;
    });
}

export function Kbd({ children, className = "" }: { children: string; className?: string }) {
  return <kbd className={`kbd ${className}`}>{children}</kbd>;
}

export function Keys({ combo, small, className = "" }: { combo: string; small?: boolean; className?: string }) {
  const parts = acceleratorParts(combo);
  return (
    <span className={`inline-flex items-center gap-0.5 ${className}`} aria-label={parts.join(" ")}>
      {parts.map((p, i) => (
        <kbd key={i} className={`kbd ${small ? "!h-[18px] !min-w-[18px] !text-[10.5px]" : ""}`}>
          {p}
        </kbd>
      ))}
    </span>
  );
}

/** "CommandOrControl+K|/" -> Ctrl K or / */
export function KeyAlternatives({ combos }: { combos: string }) {
  const list = combos.split("|");
  return (
    <span className="inline-flex items-center gap-1.5">
      {list.map((c, i) => (
        <span key={c} className="inline-flex items-center gap-1.5">
          {i > 0 && <span className="text-xs text-faint">or</span>}
          <Keys combo={c} small />
        </span>
      ))}
    </span>
  );
}
