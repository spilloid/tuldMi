import { Check, FileJson, FileText, FolderOpen, Lock, RotateCcw, Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { DeviceInfo, Retention, Settings, StorageStats, Theme, VadSettings } from "../../lib/ipc/types";
import { DEFAULT_VAD } from "../../lib/ipc/types";
import { errorMessage } from "../../lib/ipc/backend";
import { formatBytes, isMac } from "../../lib/platform";
import { DAY, startOfDay } from "../../lib/time";
import { useBackend, useSettings, useStatus, useStore } from "../../state/hooks";
import { Button } from "../../components/ui/Button";
import { Pill, Select, Slider, Switch } from "../../components/ui/controls";
import { Dialog } from "../../components/ui/Dialog";
import { KeyAlternatives, Keys } from "../../components/ui/Kbd";
import { useToast } from "../../components/ui/Toast";
import { Logo } from "../../components/shell/Logo";
import { ModelList } from "../shared/ModelList";
import { SourceCard } from "../shared/SourceCard";
import { MonitoringNote } from "../shared/MonitoringNote";
import { useAppVersion } from "../../lib/version";
import { IN_APP_SHORTCUTS } from "../shortcuts/ShortcutSheet";
import { Group, Row, SectionHeader } from "./parts";

/** Save a settings patch, surfacing engine errors as toasts. */
function usePatch() {
  const { patch } = useSettings();
  const toast = useToast();
  return async (p: Partial<Settings>) => {
    try {
      await patch(p);
      return true;
    } catch (e) {
      toast.show({ level: "error", title: "Couldn't save setting", message: errorMessage(e) });
      return false;
    }
  };
}

function useDevices() {
  const backend = useBackend();
  const [devices, setDevices] = useState<DeviceInfo[] | null>(null);
  useEffect(() => {
    let alive = true;
    backend.listDevices().then((d) => alive && setDevices(d), () => alive && setDevices([]));
    return () => {
      alive = false;
    };
  }, [backend]);
  return devices;
}

function useStorage() {
  const backend = useBackend();
  const [stats, setStats] = useState<StorageStats | null>(null);
  const refresh = () => backend.storageStats().then(setStats, () => setStats(null));
  useEffect(() => {
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [backend]);
  return { stats, refresh };
}

// ---- Audio sources ---------------------------------------------------------------------------

export function AudioSection() {
  const { settings } = useSettings();
  const status = useStatus();
  const backend = useBackend();
  const devices = useDevices();
  const save = usePatch();
  const [testing, setTesting] = useState(false);
  const recording = status?.state === "recording";

  // A device test opens devices without storing anything. Always closed on leave.
  useEffect(() => {
    if (!testing) return;
    void backend.setMonitoring(true).catch(() => {});
    return () => void backend.setMonitoring(false).catch(() => {});
  }, [testing, backend]);

  if (!settings) return null;
  return (
    <>
      <SectionHeader
        title="Audio sources"
        description="tuldMi keeps your microphone and your system audio as two separate tracks. The microphone is labelled Me; everything you hear through the computer is labelled Remote. Remote isn't split into individual speakers."
      />
      <div className="space-y-3">
        {(["mic", "system"] as const).map((src) => (
          <SourceCard
            key={src}
            source={src}
            settings={settings}
            devices={devices}
            status={status?.sources.find((s) => s.source === src)}
            onChange={(p) => void save(p)}
            metersExpected={recording || testing}
            demo={backend.kind === "demo"}
          />
        ))}
      </div>
      <div className="mt-4 flex items-center gap-3 rounded-xl border border-line-soft bg-sunken px-4 py-3">
        <div className="flex-1 text-sm text-muted">
          {recording
            ? "Meters show the live signal while recording."
            : "Not recording right now. Test your devices to see live levels — nothing is stored during a test."}
        </div>
        {!recording && (
          <Button size="sm" variant={testing ? "secondary" : "primary"} onClick={() => setTesting((t) => !t)}>
            {testing ? "Stop test" : "Test devices"}
          </Button>
        )}
      </div>
      <MonitoringNote requested={testing && !recording} />
    </>
  );
}

// ---- Transcription ---------------------------------------------------------------------------

const LANGUAGES: { value: string; label: string }[] = [
  { value: "auto", label: "Detect automatically" },
  { value: "en", label: "English" },
  { value: "de", label: "German" },
  { value: "es", label: "Spanish" },
  { value: "fr", label: "French" },
  { value: "it", label: "Italian" },
  { value: "nl", label: "Dutch" },
  { value: "pt", label: "Portuguese" },
  { value: "pl", label: "Polish" },
  { value: "sv", label: "Swedish" },
  { value: "da", label: "Danish" },
  { value: "fi", label: "Finnish" },
  { value: "cs", label: "Czech" },
  { value: "uk", label: "Ukrainian" },
  { value: "ru", label: "Russian" },
  { value: "ja", label: "Japanese" },
  { value: "zh", label: "Chinese" },
];

export function TranscriptionSection() {
  const { settings } = useSettings();
  const status = useStatus();
  const { stats } = useStorage();
  const save = usePatch();
  if (!settings) return null;
  const engine =
    status?.transcriber === "noModel"
      ? { tone: "warning" as const, text: "No model — audio is queued" }
      : status?.transcriber === "error"
        ? { tone: "danger" as const, text: "Engine error" }
        : status?.transcriber === "loading"
          ? { tone: "neutral" as const, text: "Loading model…" }
          : { tone: "success" as const, text: status?.backlog ? `Working · ${status.backlog} queued` : "Ready" };
  return (
    <>
      <SectionHeader
        title="Transcription"
        description="Speech is transcribed by a model running on this computer. Models are downloaded once, only when you ask, and never leave your device."
      />
      <Group>
        <Row label="Engine status" description="Capture never waits for transcription — a backlog simply catches up.">
          <Pill tone={engine.tone}>{engine.text}</Pill>
        </Row>
        <Row label="Spoken language" description="Automatic detection works well for most speech; choose a language if detection guesses wrong. English-only models transcribe English only.">
          <Select label="Spoken language" value={settings.language} options={LANGUAGES} onChange={(v) => void save({ language: v })} className="w-[220px]" />
        </Row>
      </Group>
      <div className="mb-2 flex items-baseline justify-between px-0.5">
        <h2 className="caps text-faint">Models</h2>
        {stats?.freeDiskBytes != null && <span className="text-xs text-faint tnum">{formatBytes(stats.freeDiskBytes)} free on this drive</span>}
      </div>
      <ModelList freeDisk={stats?.freeDiskBytes} />
      <p className="mt-3 px-0.5 text-sm text-faint">Model licenses differ from tuldMi's MIT license. Each is shown above and listed in the third-party notices.</p>
    </>
  );
}

// ---- Recording (VAD) ---------------------------------------------------------------------------

const VAD_FIELDS: { key: keyof VadSettings; label: string; description: string; min: number; max: number; step: number; fmt: (v: number) => string }[] = [
  { key: "threshold", label: "Speech sensitivity", description: "How confident the detector must be before it starts a segment. Lower catches quieter speech, and more noise.", min: 0.2, max: 0.9, step: 0.05, fmt: (v) => `${Math.round(v * 100)}%` },
  { key: "minSpeechMs", label: "Minimum speech", description: "Shorter sounds (a cough, a click) are ignored.", min: 100, max: 1000, step: 50, fmt: ms },
  { key: "preRollMs", label: "Pre-roll", description: "Audio kept from just before speech is detected, so first words aren't clipped.", min: 0, max: 1000, step: 50, fmt: ms },
  { key: "postRollMs", label: "Post-roll", description: "Audio kept after speech ends, so trailing words aren't cut off.", min: 100, max: 2000, step: 50, fmt: ms },
  { key: "minSilenceMs", label: "Pause before splitting", description: "Silence needed to end a segment. Longer keeps sentences together.", min: 200, max: 3000, step: 100, fmt: ms },
  { key: "maxSegmentMs", label: "Longest segment", description: "Continuous speech is split at this length so transcripts arrive steadily.", min: 5000, max: 120000, step: 5000, fmt: (v) => `${Math.round(v / 1000)} s` },
];

function ms(v: number) {
  return v >= 1000 ? `${(v / 1000).toFixed(v % 1000 ? 2 : 0)} s` : `${v} ms`;
}

export function RecordingSection() {
  const { settings } = useSettings();
  const save = usePatch();
  const [draft, setDraft] = useState<VadSettings | null>(settings?.vad ?? null);
  useEffect(() => setDraft(settings?.vad ?? null), [settings?.vad]);
  if (!settings || !draft) return null;
  const changed = JSON.stringify(settings.vad) !== JSON.stringify(DEFAULT_VAD);
  return (
    <>
      <SectionHeader
        title="Recording"
        description="tuldMi only keeps audio when it detects speech. Silence between conversations isn't stored, but its timing is, so the timeline stays exact."
      />
      <Group
        title="Speech detection"
        footer={
          <span className="flex items-center justify-between">
            <span>Changes apply to new speech immediately.</span>
            <button type="button" disabled={!changed} onClick={() => void save({ vad: DEFAULT_VAD })} className="inline-flex items-center gap-1 font-medium text-accent-text enabled:hover:underline disabled:text-faint">
              <RotateCcw size={12} /> Restore defaults
            </button>
          </span>
        }
      >
        {VAD_FIELDS.map((f) => (
          <div key={f.key} className="px-4 py-3.5">
            <div className="flex items-baseline justify-between gap-4">
              <span className="text-base font-medium text-text">{f.label}</span>
              <span className="font-mono text-[12px] text-muted tnum">{f.fmt(draft[f.key])}</span>
            </div>
            <p className="mb-2.5 mt-0.5 text-sm text-muted">{f.description}</p>
            <Slider
              label={f.label}
              value={draft[f.key]}
              min={f.min}
              max={f.max}
              step={f.step}
              format={f.fmt}
              onValueChange={(v) => setDraft({ ...draft, [f.key]: v })}
              onValueCommit={(v) => void save({ vad: { ...draft, [f.key]: v } })}
            />
          </div>
        ))}
      </Group>
    </>
  );
}

// ---- Retention -------------------------------------------------------------------------------

const RETENTION: { value: Retention; title: string; detail: string }[] = [
  { value: "deleteAfterTranscription", title: "Delete after transcription", detail: "Audio is removed as soon as its transcript is ready. Smallest footprint; no playback." },
  { value: "days7", title: "Keep for 7 days", detail: "Replay the past week, then audio is removed." },
  { value: "days30", title: "Keep for 30 days", detail: "A month of playback. A good balance for most people." },
  { value: "forever", title: "Keep indefinitely", detail: "Audio stays until you delete it. Uses the most disk over time." },
];

export function RetentionSection() {
  const { settings } = useSettings();
  const save = usePatch();
  if (!settings) return null;
  return (
    <>
      <SectionHeader title="Retention" description="Choose how long recordings are kept. Transcripts are always kept until you delete them — this only affects audio." />
      <div role="radiogroup" aria-label="Audio retention" className="space-y-2">
        {RETENTION.map((r) => {
          const on = settings.retention === r.value;
          return (
            <button
              key={r.value}
              type="button"
              role="radio"
              aria-checked={on}
              onClick={() => void save({ retention: r.value })}
              className={`flex w-full items-start gap-3.5 rounded-xl border px-4 py-3.5 text-left transition-colors duration-[140ms] ${
                on ? "border-accent-line bg-accent-soft/60 shadow-1" : "border-line bg-surface hover:border-line-strong"
              }`}
            >
              <span className={`mt-[3px] inline-flex h-4 w-4 shrink-0 items-center justify-center rounded-full border ${on ? "border-accent bg-accent" : "border-line-strong bg-raised"}`}>
                {on && <span className="h-1.5 w-1.5 rounded-full bg-on-accent" />}
              </span>
              <span>
                <span className="block text-base font-semibold text-text">{r.title}</span>
                <span className="mt-0.5 block text-sm text-muted">{r.detail}</span>
              </span>
            </button>
          );
        })}
      </div>
      <div className="mt-5 flex gap-3 rounded-xl border border-line-soft bg-sunken px-4 py-3 text-sm text-muted">
        <Lock size={15} className="mt-[1px] shrink-0 text-faint" aria-hidden />
        <span>Shortening retention removes older audio at the next cleanup. Expired events keep their transcript and show "audio removed" in the timeline.</span>
      </div>
    </>
  );
}

// ---- Privacy & data -----------------------------------------------------------------------------

export function PrivacySection() {
  const backend = useBackend();
  const store = useStore();
  const toast = useToast();
  const [range, setRange] = useState<"all" | "7d" | "30d">("all");
  const [exporting, setExporting] = useState<string | null>(null);
  const [confirmOpen, setConfirmOpen] = useState(false);

  const doExport = async (format: "markdown" | "json") => {
    setExporting(format);
    const from = range === "all" ? null : startOfDay(Date.now()) - (range === "7d" ? 6 : 29) * DAY;
    try {
      const path = await backend.exportTranscripts(format, from, null);
      toast.show({ level: "success", title: "Export ready", message: path, action: { label: "Show in folder", onClick: () => void backend.revealPath(path).catch(() => {}) } });
    } catch (e) {
      toast.show({ level: "error", title: "Export failed", message: errorMessage(e) });
    } finally {
      setExporting(null);
    }
  };

  const openFolder = async () => {
    try {
      await backend.openDataDir();
    } catch (e) {
      toast.show({ level: "error", message: errorMessage(e) });
    }
  };

  return (
    <>
      <SectionHeader title="Privacy & data" />
      <div className="mb-7 grid grid-cols-3 gap-2.5 max-[1100px]:grid-cols-1">
        {[
          ["Stays on this device", "Audio, transcripts and search live in your data folder. There's no account and no cloud."],
          ["No telemetry", "tuldMi sends nothing about you or your usage. The only network use is a model download you start."],
          ["You're in control", "Pause from anywhere, turn sources off, set retention, and delete anything at any time."],
        ].map(([t, d]) => (
          <div key={t} className="rounded-xl border border-line bg-surface p-3.5 shadow-1">
            <div className="flex items-center gap-1.5 text-base font-semibold text-text">
              <Check size={14} className="text-success" aria-hidden />
              {t}
            </div>
            <p className="mt-1 text-sm text-muted">{d}</p>
          </div>
        ))}
      </div>

      <Group title="Your data">
        <Row label="Data folder" description="The database, audio files and models.">
          <Button size="sm" variant="secondary" icon={<FolderOpen size={13} />} onClick={() => void openFolder()}>
            Open folder
          </Button>
        </Row>
        <Row label="Export transcripts" description={<>Saved to <span className="font-medium text-text">Documents/tuldMi Exports</span>. Audio isn't included.</>}>
          <Select
            label="Export range"
            value={range}
            onChange={setRange}
            options={[
              { value: "all", label: "All time" },
              { value: "30d", label: "Last 30 days" },
              { value: "7d", label: "Last 7 days" },
            ]}
            className="w-[132px]"
          />
          <Button size="sm" variant="secondary" icon={<FileText size={13} />} disabled={!!exporting} onClick={() => void doExport("markdown")}>
            Markdown
          </Button>
          <Button size="sm" variant="secondary" icon={<FileJson size={13} />} disabled={!!exporting} onClick={() => void doExport("json")}>
            JSON
          </Button>
        </Row>
      </Group>

      <Group title="Danger zone">
        <Row label="Delete all history" description="Every transcript and recording, permanently. Settings and downloaded models are kept.">
          <Button size="sm" variant="secondary" className="!text-danger" icon={<Trash2 size={13} />} onClick={() => setConfirmOpen(true)}>
            Delete everything…
          </Button>
        </Row>
      </Group>

      <Group title="Your responsibility">
        <div className="px-4 py-3.5 text-sm text-muted">
          Recording conversations may require the consent of everyone involved, depending on where you live and work. You are responsible for following recording-consent laws and your workplace's policies.
        </div>
      </Group>

      <DeleteAllDialog
        open={confirmOpen}
        onOpenChange={setConfirmOpen}
        onConfirm={async () => {
          try {
            await backend.deleteAllHistory();
            store.timeline.update((t) => ({ ...t, events: [], hasMore: false }));
            toast.show({ level: "success", message: "All history deleted." });
            setConfirmOpen(false);
          } catch (e) {
            toast.show({ level: "error", title: "Couldn't delete history", message: errorMessage(e) });
          }
        }}
      />
    </>
  );
}

const CONFIRM_PHRASE = "delete all";

function DeleteAllDialog({ open, onOpenChange, onConfirm }: { open: boolean; onOpenChange: (v: boolean) => void; onConfirm: () => Promise<void> }) {
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (!open) setText("");
  }, [open]);
  const ok = text.trim().toLowerCase() === CONFIRM_PHRASE;
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title="Delete all history?"
      description="Every transcript and every recording will be permanently removed from this computer. This can't be undone."
      initialFocus={() => input.current}
    >
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          if (!ok) return;
          setBusy(true);
          await onConfirm();
          setBusy(false);
        }}
      >
        <label htmlFor="confirm-delete" className="mt-4 block text-sm text-muted">
          Type <span className="font-mono font-semibold text-text">{CONFIRM_PHRASE}</span> to confirm
        </label>
        <input
          id="confirm-delete"
          ref={input}
          value={text}
          onChange={(e) => setText(e.target.value)}
          autoComplete="off"
          spellCheck={false}
          className="mt-1.5 h-9 w-full rounded-md border border-line bg-raised px-3 font-mono text-base text-text outline-none focus:border-danger"
        />
        <div className="mt-5 flex justify-end gap-2">
          <Button variant="secondary" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button type="submit" variant="danger" disabled={!ok || busy}>
            Delete everything
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

// ---- Appearance --------------------------------------------------------------------------------

export function AppearanceSection() {
  const { settings } = useSettings();
  const save = usePatch();
  if (!settings) return null;
  const opts: { value: Theme; label: string; detail: string }[] = [
    { value: "system", label: "System", detail: "Follows your OS" },
    { value: "light", label: "Paper", detail: "Light" },
    { value: "dark", label: "Ink", detail: "Dark" },
  ];
  return (
    <>
      <SectionHeader title="Appearance" description="Paper and Ink share one design; pick whichever is kinder to your eyes." />
      <div role="radiogroup" aria-label="Theme" className="grid grid-cols-3 gap-3">
        {opts.map((o) => {
          const on = settings.theme === o.value;
          return (
            <button
              key={o.value}
              type="button"
              role="radio"
              aria-checked={on}
              onClick={() => void save({ theme: o.value })}
              className={`rounded-xl border p-2 text-left transition-colors duration-[140ms] ${on ? "border-accent shadow-[0_0_0_1px_var(--tm-accent)]" : "border-line hover:border-line-strong"}`}
            >
              <ThemePreview kind={o.value} />
              <div className="mt-2 flex items-center justify-between px-1 pb-0.5">
                <span>
                  <span className="block text-base font-semibold text-text">{o.label}</span>
                  <span className="block text-xs text-faint">{o.detail}</span>
                </span>
                {on && <Check size={15} className="text-accent-text" />}
              </div>
            </button>
          );
        })}
      </div>
    </>
  );
}

function ThemePreview({ kind }: { kind: Theme }) {
  const paper = { bg: "oklch(0.962 0.009 78)", card: "oklch(0.982 0.006 80)", line: "oklch(0.885 0.011 76)", text: "oklch(0.3 0.014 58)", me: "oklch(0.5 0.1 256)", remote: "oklch(0.52 0.085 168)" };
  const ink = { bg: "oklch(0.172 0.007 68)", card: "oklch(0.2 0.008 68)", line: "oklch(0.29 0.009 68)", text: "oklch(0.85 0.011 80)", me: "oklch(0.72 0.095 256)", remote: "oklch(0.73 0.08 168)" };
  const pane = (c: typeof paper, clip?: string) => (
    <div className="absolute inset-0 p-2.5" style={{ background: c.bg, clipPath: clip }}>
      <div className="h-full rounded-md border p-2" style={{ background: c.card, borderColor: c.line }}>
        <div className="mb-1.5 h-1.5 w-8 rounded-full" style={{ background: c.text, opacity: 0.8 }} />
        <div className="mb-1 flex gap-1.5">
          <span className="w-[2px] rounded-full" style={{ background: c.remote }} />
          <span className="flex-1 space-y-1 py-0.5">
            <span className="block h-1 w-[90%] rounded-full" style={{ background: c.text, opacity: 0.35 }} />
            <span className="block h-1 w-[60%] rounded-full" style={{ background: c.text, opacity: 0.35 }} />
          </span>
        </div>
        <div className="ml-3 flex gap-1.5">
          <span className="w-[2px] rounded-full" style={{ background: c.me }} />
          <span className="flex-1 space-y-1 py-0.5">
            <span className="block h-1 w-[80%] rounded-full" style={{ background: c.text, opacity: 0.35 }} />
          </span>
        </div>
      </div>
    </div>
  );
  return (
    <div className="relative h-[86px] overflow-hidden rounded-lg border border-line-soft" aria-hidden>
      {kind === "dark" ? pane(ink) : pane(paper)}
      {kind === "system" && pane(ink, "polygon(100% 0, 100% 100%, 0 100%)")}
    </div>
  );
}

// ---- Shortcuts ---------------------------------------------------------------------------------

const MODIFIER_CODES = new Set(["ControlLeft", "ControlRight", "ShiftLeft", "ShiftRight", "AltLeft", "AltRight", "MetaLeft", "MetaRight"]);

/** KeyboardEvent -> Tauri accelerator, or null when it isn't a usable global shortcut. */
export function acceleratorFromEvent(e: Pick<KeyboardEvent, "code" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey">, mac = isMac()): string | null {
  if (MODIFIER_CODES.has(e.code)) return null;
  const mods: string[] = [];
  if (mac ? e.metaKey : e.ctrlKey) mods.push("CommandOrControl");
  if (mac && e.ctrlKey) mods.push("Control");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  let key: string | null = null;
  const c = e.code;
  if (/^Key[A-Z]$/.test(c)) key = c.slice(3);
  else if (/^Digit\d$/.test(c)) key = c.slice(5);
  else if (/^F([1-9]|1[0-9]|2[0-4])$/.test(c)) key = c;
  else if (["Backquote", "Minus", "Equal", "BracketLeft", "BracketRight", "Backslash", "Semicolon", "Quote", "Comma", "Period", "Slash", "Space", "Insert", "Home", "End", "PageUp", "PageDown", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Pause"].includes(c)) key = c;
  if (!key) return null;
  const isF = /^F\d+$/.test(key);
  // A global shortcut needs Ctrl/Cmd or Alt (or be a function key) so it can't eat normal typing.
  if (!isF && !mods.some((m) => m === "CommandOrControl" || m === "Alt" || m === "Control")) return null;
  return [...mods, key].join("+");
}

export function ShortcutsSection() {
  const { settings } = useSettings();
  const save = usePatch();
  const [capturing, setCapturing] = useState(false);
  const [candidate, setCandidate] = useState<string | null>(null);
  const [hint, setHint] = useState<string | null>(null);

  useEffect(() => {
    if (!capturing) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setCapturing(false);
        setCandidate(null);
        return;
      }
      const acc = acceleratorFromEvent(e);
      if (acc) {
        setCandidate(acc);
        setHint(null);
      } else if (!MODIFIER_CODES.has(e.code)) {
        setHint("Include Ctrl or Alt so the shortcut can't interfere with typing.");
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [capturing]);

  if (!settings) return null;
  return (
    <>
      <SectionHeader title="Shortcuts" description="One global shortcut pauses and resumes recording from anywhere — even when tuldMi is hidden in the tray." />
      <Group title="Global">
        <div className="px-4 py-4">
          <div className="flex items-center gap-4">
            <div className="min-w-0 flex-1">
              <div className="text-base font-medium text-text">Pause / resume recording</div>
              <div className="mt-0.5 text-sm text-muted">{capturing ? "Press the new key combination. Esc cancels." : "Works system-wide."}</div>
            </div>
            <div
              aria-live="polite"
              className={`flex h-10 min-w-[180px] items-center justify-center rounded-lg border px-3 ${capturing ? "border-accent bg-accent-soft/50 shadow-[0_0_0_3px_var(--tm-focus)]" : "border-line bg-sunken"}`}
            >
              {capturing && !candidate ? <span className="text-sm text-muted">Waiting for keys…</span> : <Keys combo={candidate ?? settings.pauseShortcut} />}
            </div>
            {capturing ? (
              <>
                <Button size="sm" variant="secondary" onClick={() => { setCapturing(false); setCandidate(null); setHint(null); }}>
                  Cancel
                </Button>
                <Button
                  size="sm"
                  variant="primary"
                  disabled={!candidate}
                  onClick={async () => {
                    if (candidate && (await save({ pauseShortcut: candidate }))) {
                      setCapturing(false);
                      setCandidate(null);
                    }
                  }}
                >
                  Save
                </Button>
              </>
            ) : (
              <Button size="sm" variant="secondary" onClick={() => setCapturing(true)}>
                Change…
              </Button>
            )}
          </div>
          {hint && <p className="mt-2 text-sm text-warning">{hint}</p>}
        </div>
      </Group>
      <h2 className="caps mb-2 px-0.5 text-faint">In the app</h2>
      <div className="grid grid-cols-2 gap-3 max-[1100px]:grid-cols-1">
        {IN_APP_SHORTCUTS.map((g) => (
          <div key={g.group} className="rounded-xl border border-line bg-surface px-4 py-3 shadow-1">
            <div className="mb-2 text-sm font-semibold text-text">{g.group}</div>
            <ul className="space-y-1.5">
              {g.items.map(([k, label]) => (
                <li key={k + label} className="flex items-center justify-between gap-3 text-sm text-muted">
                  {label}
                  <KeyAlternatives combos={k} />
                </li>
              ))}
            </ul>
          </div>
        ))}
      </div>
    </>
  );
}

// ---- Startup ---------------------------------------------------------------------------------

export function StartupSection() {
  const { settings } = useSettings();
  const save = usePatch();
  if (!settings) return null;
  return (
    <>
      <SectionHeader title="Startup" description="tuldMi lives in the system tray. Closing the window never stops recording — use Pause or Quit from the tray." />
      <Group>
        <Row label="Launch at login" description="Start tuldMi in the tray when you sign in. Off by default.">
          <Switch checked={settings.launchAtLogin} label="Launch at login" onCheckedChange={(v) => void save({ launchAtLogin: v })} />
        </Row>
        <Row label="Start recording when tuldMi starts" description="If off, tuldMi opens paused and waits for you to press Resume.">
          <Switch checked={settings.captureOnLaunch} label="Start recording when tuldMi starts" onCheckedChange={(v) => void save({ captureOnLaunch: v })} />
        </Row>
      </Group>
    </>
  );
}

// ---- Storage -------------------------------------------------------------------------------------

export function StorageSection() {
  const { stats } = useStorage();
  const backend = useBackend();
  const toast = useToast();
  if (!stats) return <SectionHeader title="Storage" description="Measuring…" />;
  const parts: { label: string; bytes: number; color: string; detail: string }[] = [
    { label: "Transcripts & search", bytes: stats.databaseBytes, color: "var(--tm-accent)", detail: `${stats.transcribedEvents.toLocaleString()} of ${stats.events.toLocaleString()} events transcribed` },
    { label: "Audio", bytes: stats.audioBytes, color: "var(--tm-remote)", detail: `${stats.audioFiles.toLocaleString()} Opus ${stats.audioFiles === 1 ? "file" : "files"}` },
    { label: "Models", bytes: stats.modelsBytes, color: "var(--tm-me)", detail: "Downloaded transcription models" },
  ];
  const total = parts.reduce((n, p) => n + p.bytes, 0);
  const disk = stats.freeDiskBytes != null ? total + stats.freeDiskBytes : null;
  return (
    <>
      <SectionHeader title="Storage" description="Everything tuldMi keeps, on this computer." />
      <div className="mb-7 rounded-xl border border-line bg-surface p-4 shadow-1">
        <div className="flex items-baseline justify-between">
          <div>
            <div className="text-xs text-faint">tuldMi uses</div>
            <div className="font-display text-[30px] font-[460] leading-tight tracking-[-0.02em] text-text tnum">{formatBytes(total)}</div>
          </div>
          {stats.freeDiskBytes != null && <div className="text-right text-sm text-muted tnum">{formatBytes(stats.freeDiskBytes)} free on this drive</div>}
        </div>
        <div className="mt-3 flex h-2.5 overflow-hidden rounded-full bg-line" role="img" aria-label={parts.map((p) => `${p.label} ${formatBytes(p.bytes)}`).join(", ")}>
          {parts.map((p) => (
            <div key={p.label} style={{ width: `${total ? (p.bytes / total) * 100 : 0}%`, background: p.color }} className="h-full border-r border-surface last:border-r-0" />
          ))}
        </div>
        <ul className="mt-4 space-y-2.5">
          {parts.map((p) => (
            <li key={p.label} className="flex items-center gap-3">
              <span className="h-2.5 w-2.5 shrink-0 rounded-[3px]" style={{ background: p.color }} aria-hidden />
              <span className="flex-1">
                <span className="text-base text-text">{p.label}</span>
                <span className="ml-2 text-sm text-faint">{p.detail}</span>
              </span>
              <span className="font-mono text-[12px] text-muted tnum">{formatBytes(p.bytes)}</span>
            </li>
          ))}
        </ul>
        {disk && (
          <div className="mt-4 border-t border-line-soft pt-3">
            <div className="mb-1.5 flex justify-between text-xs text-faint tnum">
              <span>Share of this drive's available space</span>
              <span>{((total / disk) * 100).toFixed(total / disk < 0.01 ? 2 : 1)}%</span>
            </div>
            <div className="h-1.5 overflow-hidden rounded-full bg-line">
              <div className="h-full rounded-full bg-muted" style={{ width: `${Math.max(0.5, (total / disk) * 100)}%` }} />
            </div>
          </div>
        )}
      </div>
      <Group>
        <Row label="Data folder" description={<span className="selectable break-all font-mono text-[11.5px]">{stats.dataDir}</span>}>
          <Button size="sm" variant="secondary" icon={<FolderOpen size={13} />} onClick={() => void backend.openDataDir().catch((e) => toast.show({ level: "error", message: errorMessage(e) }))}>
            Open
          </Button>
        </Row>
      </Group>
    </>
  );
}

// ---- About ---------------------------------------------------------------------------------------

const NOTICES: [string, string, string][] = [
  ["whisper.cpp, OpenAI Whisper weights", "MIT", "On-device transcription (models you download)"],
  ["Silero VAD", "MIT", "Speech detection, bundled"],
  ["Tauri", "MIT or Apache-2.0", "Desktop shell"],
  ["React, React DOM", "MIT", "Interface"],
  ["Radix UI primitives", "MIT", "Accessible dialogs, menus, tooltips, switches, sliders"],
  ["Motion", "MIT", "Animation"],
  ["Lucide", "ISC", "Icons"],
  ["Tailwind CSS", "MIT", "Styling"],
  ["Inter", "SIL Open Font License 1.1", "Interface typeface"],
  ["Fraunces", "SIL Open Font License 1.1", "Display typeface"],
  ["JetBrains Mono", "SIL Open Font License 1.1", "Timestamps"],
];

export function AboutSection() {
  const backend = useBackend();
  const version = useAppVersion(backend.kind === "tauri");
  const [open, setOpen] = useState(false);
  return (
    <>
      <div className="flex flex-col items-center pb-6 pt-10 text-center">
        <Logo size={56} />
        <div className="mt-5 font-display text-[34px] font-[480] tracking-[-0.02em] text-text">tuldMi</div>
        <div className="mt-1 font-display text-lg italic text-muted">Your conversations, remembered.</div>
        <div className="mt-4 flex items-center gap-2">
          <Pill>Version {version}</Pill>
          <Pill>MIT License</Pill>
        </div>
      </div>
      <Group>
        <Row label="Third-party notices" description="Open-source software and fonts tuldMi is built with.">
          <Button size="sm" variant="secondary" onClick={() => setOpen(true)}>
            View notices
          </Button>
        </Row>
        <Row label="Transcription models" description="Downloaded models carry their own licenses, shown in Settings → Transcription." />
      </Group>
      <p className="text-center text-sm text-faint">A Spillers Technology product. Private by design.</p>
      <Dialog open={open} onOpenChange={setOpen} title="Third-party notices" description="tuldMi is MIT licensed. It includes the following components under their own licenses." width={540}>
        <ul className="mt-4 divide-y divide-line-soft rounded-lg border border-line">
          {NOTICES.map(([name, lic, what]) => (
            <li key={name} className="flex items-baseline gap-3 px-3 py-2">
              <span className="flex-1">
                <span className="text-base font-medium text-text">{name}</span>
                <span className="ml-2 text-sm text-faint">{what}</span>
              </span>
              <span className="text-sm text-muted">{lic}</span>
            </li>
          ))}
        </ul>
        <p className="mt-3 text-sm text-faint">The full license texts ship with the installer in THIRD_PARTY_NOTICES.</p>
      </Dialog>
    </>
  );
}

