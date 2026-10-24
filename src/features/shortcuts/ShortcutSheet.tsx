import { Dialog } from "../../components/ui/Dialog";
import { KeyAlternatives, Keys } from "../../components/ui/Kbd";
import { useSettings } from "../../state/hooks";
import { useUi } from "../../state/ui";

export const IN_APP_SHORTCUTS: { group: string; items: [string, string][] }[] = [
  {
    group: "Anywhere",
    items: [
      ["CommandOrControl+K|/", "Search"],
      ["CommandOrControl+Comma", "Settings"],
      ["?", "Keyboard shortcuts"],
      ["Esc", "Close overlay or player"],
    ],
  },
  {
    group: "Timeline",
    items: [
      ["J", "Next event"],
      ["K", "Previous event"],
      ["Enter", "Expand or collapse"],
      ["Space", "Play or pause"],
      ["L|End", "Back to live"],
    ],
  },
  {
    group: "Player",
    items: [
      ["ArrowLeft", "Back 5 seconds (waveform focused)"],
      ["ArrowRight", "Forward 5 seconds"],
      ["Shift+ArrowRight", "Fine seek, 1 second"],
    ],
  },
];

export function ShortcutSheet() {
  const ui = useUi();
  const { settings } = useSettings();
  return (
    <Dialog open={ui.shortcutsOpen} onOpenChange={ui.setShortcutsOpen} title="Keyboard shortcuts" width={620}>
      <div className="mt-4 grid grid-cols-2 gap-x-8 gap-y-5 max-[700px]:grid-cols-1">
        {settings && (
          <div className="col-span-2 flex items-center justify-between rounded-lg border border-accent-line bg-accent-soft px-3.5 py-2.5 max-[700px]:col-span-1">
            <div>
              <div className="text-base font-semibold text-text">Pause or resume recording</div>
              <div className="text-sm text-muted">Works system-wide, even when tuldMi is hidden.</div>
            </div>
            <Keys combo={settings.pauseShortcut} />
          </div>
        )}
        {IN_APP_SHORTCUTS.map((g) => (
          <div key={g.group}>
            <div className="caps mb-2 text-faint">{g.group}</div>
            <ul className="space-y-1.5">
              {g.items.map(([k, label]) => (
                <li key={k + label} className="flex items-center justify-between gap-4 text-base text-muted">
                  <span>{label}</span>
                  <KeyAlternatives combos={k} />
                </li>
              ))}
            </ul>
          </div>
        ))}
      </div>
    </Dialog>
  );
}
