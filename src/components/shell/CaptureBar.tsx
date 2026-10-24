import { AlertTriangle, Loader2, Pause, Play } from "lucide-react";
import { useEffect, useState } from "react";
import type { CaptureStatus, Source, SourceStatus } from "../../lib/ipc/types";
import { errorMessage } from "../../lib/ipc/backend";
import { humanDuration } from "../../lib/time";
import { navigate } from "../../lib/router";
import { useBackend, useLevels, useSettings, useStatus } from "../../state/hooks";
import { LiveMeter } from "../viz/LiveMeter";
import { RecordingIndicator } from "../viz/RecordingIndicator";
import { Button } from "../ui/Button";
import { Tooltip } from "../ui/Tooltip";
import { useToast } from "../ui/Toast";

function useNow(intervalMs: number) {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(t);
  }, [intervalMs]);
  return now;
}

export function stateText(s: CaptureStatus | null): { title: string; detail?: string } {
  if (!s) return { title: "Connecting…" };
  switch (s.state) {
    case "recording":
      return { title: "Recording" };
    case "starting":
      return { title: "Starting…" };
    case "paused":
      return { title: "Paused", detail: "Nothing is being captured" };
    case "stopped":
      return { title: "Not recording" };
    case "error":
      return { title: "Can't capture", detail: "Every audio source failed" };
  }
}

export function CaptureBar() {
  const status = useStatus();
  const backend = useBackend();
  const toast = useToast();
  const { settings } = useSettings();
  const now = useNow(15_000);
  const [busy, setBusy] = useState(false);
  const paused = status?.state === "paused";
  const live = status?.state === "recording" || status?.state === "starting";
  const st = stateText(status);

  const toggle = async () => {
    if (!status) return;
    setBusy(true);
    try {
      await backend.setPaused(!paused && status.state !== "stopped" && status.state !== "error" ? true : false);
    } catch (e) {
      toast.show({ level: "error", title: "Couldn't change recording state", message: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  };

  const elapsed = live && status?.sessionStartedAt ? humanDuration(now - status.sessionStartedAt) : null;

  return (
    <footer
      aria-label="Capture status"
      className={`relative z-20 flex h-[var(--tm-capturebar-h)] shrink-0 items-center gap-4 border-t border-line-soft px-4 transition-colors duration-[320ms] ${
        paused ? "bg-sunken" : "bg-canvas"
      }`}
    >
      {/* State */}
      <div className="flex min-w-[168px] items-center gap-2.5" aria-live="polite">
        <RecordingIndicator state={status?.state ?? "stopped"} size={18} />
        <div className="min-w-0 leading-tight">
          <div className={`text-base font-semibold ${status?.state === "error" ? "text-warning" : "text-text"}`}>{st.title}</div>
          <div className="truncate text-xs text-faint">
            {elapsed ? <>Session · {elapsed}</> : st.detail ?? " "}
          </div>
        </div>
      </div>

      <div className="h-6 w-px bg-line-soft" aria-hidden />

      {/* Sources */}
      <div className={`flex min-w-0 flex-1 items-center gap-5 transition-opacity duration-[320ms] ${paused ? "opacity-55" : ""}`}>
        {(["mic", "system"] as Source[]).map((src) => (
          <SourceMeter key={src} source={src} status={status?.sources.find((s) => s.source === src)} capturing={live} />
        ))}
      </div>

      {/* Transcriber */}
      <TranscriberChip status={status} hasModel={!!settings?.modelId} />

      <Tooltip content={paused ? "Resume recording" : "Pause recording"} shortcut={settings?.pauseShortcut}>
        <Button
          variant={paused || status?.state === "stopped" ? "primary" : "secondary"}
          size="md"
          disabled={!status || busy || status.state === "starting"}
          onClick={toggle}
          icon={paused || status?.state === "stopped" || status?.state === "error" ? <Play size={14} fill="currentColor" /> : <Pause size={14} fill="currentColor" />}
          className="min-w-[92px]"
        >
          {paused ? "Resume" : status?.state === "stopped" ? "Start" : status?.state === "error" ? "Retry" : "Pause"}
        </Button>
      </Tooltip>
    </footer>
  );
}

function SourceMeter({ source, status, capturing }: { source: Source; status?: SourceStatus; capturing: boolean }) {
  const { summary } = useLevels();
  const label = source === "mic" ? "Me" : "Remote";
  const sum = summary[source];
  const problem = status && (status.state === "error" || status.state === "unavailable");
  const off = !status?.enabled;
  const device = status?.deviceName ?? (source === "mic" ? "Default microphone" : "Default output");
  let detail: string;
  if (off) detail = "Off";
  else if (problem) detail = status?.state === "unavailable" ? "Device unavailable" : "Error";
  else if (!capturing) detail = "Idle";
  else if (!sum.live) detail = "No live audio";
  else detail = sum.speaking ? "Speech" : "Listening";

  const tip = (
    <span className="block max-w-72 leading-snug">
      <span className="font-semibold">{source === "mic" ? "Microphone → Me" : "System audio → Remote"}</span>
      <span className="block text-muted">{off ? "Disabled in Settings" : device}</span>
      {status?.format && !off && <span className="block text-faint">{status.format}</span>}
      {status?.error && <span className="mt-1 block text-danger">{status.error}</span>}
    </span>
  );

  return (
    <Tooltip content={tip}>
      <button
        type="button"
        onClick={() => navigate("/settings/audio")}
        className="group flex min-w-0 items-center gap-2.5 rounded-md px-1.5 py-1 hover:bg-hover"
        aria-label={`${label} (${source === "mic" ? "microphone" : "system audio"}): ${detail}. Open audio settings.`}
      >
        <span className="flex w-[58px] shrink-0 flex-col items-start leading-tight">
          <span className={`flex items-center gap-1.5 text-sm font-semibold ${source === "mic" ? "text-me-text" : "text-remote-text"}`}>
            <span aria-hidden className={`h-1.5 w-1.5 rounded-full ${source === "mic" ? "bg-me" : "bg-remote"} ${off || problem ? "opacity-30" : ""}`} />
            {label}
          </span>
          <span className={`truncate text-[11px] ${problem ? "text-warning" : "text-faint"}`}>{detail}</span>
        </span>
        {problem ? (
          <AlertTriangle size={14} className="text-warning" aria-hidden />
        ) : (
          <LiveMeter source={source} className={`h-[22px] w-[84px] max-[1100px]:w-[56px] ${off ? "opacity-40" : ""}`} label={label} />
        )}
      </button>
    </Tooltip>
  );
}

function TranscriberChip({ status, hasModel }: { status: CaptureStatus | null; hasModel: boolean }) {
  if (!status) return null;
  const backlog = status.backlog;
  let text: string;
  let tone = "text-muted";
  let spin = false;
  switch (status.transcriber) {
    case "noModel":
      text = hasModel ? "Model unavailable" : "No model installed";
      tone = "text-warning";
      break;
    case "loading":
      text = "Loading model…";
      spin = true;
      break;
    case "working":
      text = "Transcribing";
      spin = true;
      break;
    case "idle":
      text = backlog ? "Waiting" : "Up to date";
      break;
    case "error":
      text = "Transcriber error";
      tone = "text-danger";
      break;
  }
  const tip =
    status.transcriber === "noModel"
      ? "Audio is still captured and queued. Install a model in Settings → Transcription to transcribe it."
      : backlog
        ? `${backlog} speech ${backlog === 1 ? "event" : "events"} waiting for transcription. Capture continues regardless.`
        : "Every captured event has been transcribed.";
  return (
    <Tooltip content={<span className="block max-w-64 leading-snug">{tip}</span>}>
      <button
        type="button"
        onClick={() => navigate("/settings/transcription")}
        className="flex items-center gap-2 rounded-md px-2 py-1 text-sm hover:bg-hover"
      >
        {spin ? <Loader2 size={13} className="animate-spin text-faint motion-reduce:animate-none" aria-hidden /> : <span aria-hidden className={`h-1.5 w-1.5 rounded-full ${status.transcriber === "idle" ? "bg-success" : "bg-current"} ${tone}`} />}
        <span className={tone}>{text}</span>
        {backlog > 0 && (
          <span className="tnum rounded-full bg-sunken px-1.5 text-xs font-semibold text-muted" aria-label={`${backlog} in queue`}>
            {backlog}
          </span>
        )}
      </button>
    </Tooltip>
  );
}
