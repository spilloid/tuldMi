import { Download, Mic, Monitor, Play } from "lucide-react";
import type { CaptureStatus } from "../../lib/ipc/types";
import { navigate } from "../../lib/router";
import { useBackend, useSettings } from "../../state/hooks";
import { RecordingIndicator } from "../../components/viz/RecordingIndicator";
import { Button } from "../../components/ui/Button";
import { Keys } from "../../components/ui/Kbd";

/** The timeline before anything has been said. Should feel calm and finished, not broken. */
export function TimelineEmpty({ status }: { status: CaptureStatus | null }) {
  const backend = useBackend();
  const { settings } = useSettings();
  const state = status?.state ?? "stopped";
  const paused = state === "paused";
  const noModel = status?.transcriber === "noModel";

  return (
    <div className="flex min-h-0 flex-1 items-center justify-center overflow-y-auto px-6 py-10">
      <div className="w-full max-w-[520px] text-center">
        <div className="mx-auto mb-7 flex h-16 w-16 items-center justify-center rounded-2xl border border-line bg-surface shadow-1">
          <RecordingIndicator state={state} size={26} />
        </div>
        <h1 className="font-display text-[30px] font-[440] leading-[1.15] tracking-[-0.02em] text-text">
          {paused ? "Paused. Nothing is being captured." : state === "recording" ? "Listening for conversation" : "Ready when you are"}
        </h1>
        <p className="mx-auto mt-3 max-w-[440px] text-md leading-[25px] text-muted [text-wrap:pretty]">
          {paused
            ? "Resume whenever you like. Your timeline picks up exactly where you left it."
            : "When someone speaks — you on the microphone, or anyone through your speakers — it appears here, transcribed on this device."}
        </p>

        <div className="mx-auto mt-8 grid max-w-[440px] grid-cols-2 gap-3 text-left">
          <SourceCard icon={<Mic size={15} />} title="Me" detail="Your microphone" tone="me" on={settings?.micEnabled ?? true} />
          <SourceCard icon={<Monitor size={15} />} title="Remote" detail="System audio" tone="remote" on={settings?.systemEnabled ?? true} />
        </div>

        <div className="mt-8 flex flex-col items-center gap-3">
          {(paused || state === "stopped") && (
            <Button variant="primary" size="lg" icon={<Play size={14} fill="currentColor" />} onClick={() => void backend.setPaused(false)}>
              {paused ? "Resume recording" : "Start recording"}
            </Button>
          )}
          {noModel && (
            <div className="flex items-center gap-2 rounded-lg border border-line bg-surface px-3 py-2 text-sm text-muted">
              <Download size={14} className="text-accent-text" aria-hidden />
              Speech is captured now and transcribed once a model is installed.
              <button type="button" className="font-semibold text-accent-text hover:underline" onClick={() => navigate("/settings/transcription")}>
                Set up
              </button>
            </div>
          )}
          {settings && (
            <p className="flex items-center gap-2 text-sm text-faint">
              Pause from anywhere with <Keys combo={settings.pauseShortcut} small />
            </p>
          )}
        </div>
      </div>
    </div>
  );
}

function SourceCard({ icon, title, detail, tone, on }: { icon: React.ReactNode; title: string; detail: string; tone: "me" | "remote"; on: boolean }) {
  return (
    <div className="flex items-center gap-3 rounded-lg border border-line bg-surface px-3 py-2.5">
      <span className={`inline-flex h-8 w-8 items-center justify-center rounded-md ${tone === "me" ? "bg-me-soft text-me-text" : "bg-remote-soft text-remote-text"}`}>{icon}</span>
      <span className="min-w-0">
        <span className={`block text-base font-semibold ${tone === "me" ? "text-me-text" : "text-remote-text"}`}>{title}</span>
        <span className="block truncate text-xs text-faint">{on ? detail : "Off in Settings"}</span>
      </span>
    </div>
  );
}
