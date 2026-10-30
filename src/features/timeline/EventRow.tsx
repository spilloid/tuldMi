import * as CM from "@radix-ui/react-context-menu";
import { AnimatePresence, motion } from "motion/react";
import { AlertCircle, AudioLines, Clock3, Copy, FileClock, Pause, Play, RotateCcw, Trash2, VolumeX } from "lucide-react";
import { memo } from "react";
import type { MouseEvent, ReactNode } from "react";
import type { SpeechEvent } from "../../lib/ipc/types";
import { clock, clockSeconds, eventText, humanDuration, mediaTime, sourceLabel } from "../../lib/time";
import { formatBytes } from "../../lib/platform";
import { transition, useReducedMotion } from "../../lib/motion";
import { Waveform } from "../../components/viz/Waveform";
import { Button } from "../../components/ui/Button";
import { Tooltip } from "../../components/ui/Tooltip";
import { audioAvailability, usePlayer } from "../../state/player";
import { useBackend } from "../../state/hooks";
import { useEventActions } from "./actions";

interface Props {
  ev: SpeechEvent;
  showLabel: boolean;
  showTime: boolean;
  selected: boolean;
  expanded: boolean;
  flash: boolean;
  arrived: boolean;
  onSelect: (id: string) => void;
  onToggle: (id: string) => void;
}

export const EventRow = memo(function EventRow({ ev, showLabel, showTime, selected, expanded, flash, arrived, onSelect, onToggle }: Props) {
  const reduced = useReducedMotion();
  const me = ev.source === "mic";
  const actions = useEventActions();
  const player = usePlayer();
  const backend = useBackend();
  const avail = audioAvailability(ev, backend);
  const isPlaying = player.eventId === ev.id && player.playing;

  const onClick = (e: MouseEvent) => {
    if (window.getSelection()?.toString()) return; // user is selecting text
    if ((e.target as HTMLElement).closest("button, a, [role=slider]")) return;
    onSelect(ev.id);
    onToggle(ev.id);
  };

  const body = (
    <motion.div
      initial={arrived ? (reduced ? { opacity: 0 } : { opacity: 0, y: 8 }) : false}
      animate={{ opacity: 1, y: 0 }}
      transition={transition.arrival}
      data-event-id={ev.id}
      className={`group/row relative grid grid-cols-[56px_minmax(0,1fr)] rounded-lg has-[[role=button]:focus-visible]:shadow-[inset_0_0_0_1.5px_var(--tm-accent-line)] ${flash ? "flash" : ""} ${
        selected ? "bg-selected" : "hover:bg-hover"
      } transition-colors duration-[140ms]`}
      onClick={onClick}
    >
      <div className="select-none pr-3 pt-[9px] text-right font-mono text-[11px] leading-[20px] text-faint tnum">
        <Tooltip content={new Date(ev.startedAt).toLocaleString()} side="left" delay={700}>
          <time dateTime={new Date(ev.startedAt).toISOString()} className={showTime || selected ? "" : "opacity-0 group-hover/row:opacity-100 transition-opacity"}>{clock(ev.startedAt)}</time>
        </Tooltip>
      </div>
      <div className={`min-w-0 py-1 pr-3 ${me ? "pl-9 max-[1100px]:pl-6" : ""}`}>
        <div className={`relative border-l-2 pl-3.5 ${me ? "border-me-line" : "border-remote-line"} py-1`}>
          <div
            role="button"
            tabIndex={selected ? 0 : -1}
            aria-expanded={expanded}
            aria-label={`${sourceLabel(ev)} at ${clock(ev.startedAt)}: ${summaryText(ev)}`}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                onToggle(ev.id);
              }
            }}
            onFocus={() => !selected && onSelect(ev.id)}
            className="block rounded-sm focus-visible:outline-none"
          >
            {(showLabel || expanded) && (
              <div className="mb-0.5 flex items-center gap-2">
                <span className={`text-[12px] font-semibold tracking-[0.01em] ${me ? "text-me-text" : "text-remote-text"}`}>{sourceLabel(ev)}</span>
                {ev.audio && !expanded && (
                  <Waveform peaks={ev.audio.peaks} source={ev.source} className="h-3.5 w-[72px] opacity-80" />
                )}
                {isPlaying && <AudioLines size={12} className="text-accent-text" aria-label="Playing" />}
              </div>
            )}
            {!(expanded && ev.status === "done" && ev.segments.length > 1) && <EventBody ev={ev} onRetry={() => void actions.retry(ev)} />}
          </div>
          <AnimatePresence initial={false}>
            {expanded && (
              <motion.div
                key="detail"
                initial={reduced ? { opacity: 0 } : { opacity: 0, height: 0 }}
                animate={reduced ? { opacity: 1 } : { opacity: 1, height: "auto" }}
                exit={reduced ? { opacity: 0 } : { opacity: 0, height: 0 }}
                transition={transition.panel}
                className="overflow-hidden"
              >
                <EventDetail ev={ev} avail={avail} />
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      </div>
    </motion.div>
  );

  return (
    <CM.Root>
      <CM.Trigger asChild onContextMenu={() => onSelect(ev.id)}>
        {body}
      </CM.Trigger>
      <CM.Portal>
        <CM.Content className="tm-pop tm-menu z-[70]" collisionPadding={8}>
          <MenuItem icon={<Copy size={14} />} onSelect={() => void actions.copyText(ev)} disabled={!eventText(ev)}>
            Copy text
          </MenuItem>
          <MenuItem icon={<Clock3 size={14} />} onSelect={() => void actions.copyWithTimestamp(ev)} disabled={!eventText(ev)}>
            Copy with timestamp
          </MenuItem>
          <div className="tm-menu-sep" />
          <MenuItem icon={isPlaying ? <Pause size={14} /> : <Play size={14} />} onSelect={() => { onSelect(ev.id); player.toggle(ev); }} disabled={!avail.ok} shortcut="Space">
            {isPlaying ? "Pause" : "Play"}
          </MenuItem>
          <MenuItem icon={<RotateCcw size={14} />} onSelect={() => void actions.retry(ev)} disabled={ev.status !== "failed"}>
            Retry transcription
          </MenuItem>
          <div className="tm-menu-sep" />
          <MenuItem icon={<VolumeX size={14} />} onSelect={() => void actions.deleteAudio(ev)} disabled={!ev.audio} danger>
            Delete audio…
          </MenuItem>
          <MenuItem icon={<Trash2 size={14} />} onSelect={() => void actions.remove(ev)} danger>
            Delete event…
          </MenuItem>
        </CM.Content>
      </CM.Portal>
    </CM.Root>
  );
});

function MenuItem({ icon, children, onSelect, disabled, danger, shortcut }: { icon: ReactNode; children: ReactNode; onSelect: () => void; disabled?: boolean; danger?: boolean; shortcut?: string }) {
  return (
    <CM.Item className="tm-menu-item" onSelect={onSelect} disabled={disabled} data-danger={danger ? "" : undefined}>
      {icon}
      <span>{children}</span>
      {shortcut && <span className="tm-menu-shortcut">{shortcut}</span>}
    </CM.Item>
  );
}

function summaryText(ev: SpeechEvent): string {
  switch (ev.status) {
    case "queued":
      return "waiting to be transcribed";
    case "transcribing":
      return "transcribing";
    case "failed":
      return "transcription failed";
    case "empty":
      return "no words recognized";
    default:
      return eventText(ev);
  }
}

function EventBody({ ev, onRetry }: { ev: SpeechEvent; onRetry: () => void }) {
  const dur = ev.endedAt - ev.startedAt;
  if (ev.status === "queued" || ev.status === "transcribing") {
    // Skeleton width follows real speech duration: longer events, longer placeholder.
    const lines = Math.min(3, Math.max(1, Math.round(dur / 6000)));
    return (
      <div className="py-1" aria-live="polite">
        <div className="space-y-2 py-1" aria-hidden>
          {Array.from({ length: lines }, (_, i) => (
            <div key={i} className="skeleton h-[9px]" style={{ width: i === lines - 1 ? `${38 + ((dur / 100) % 30)}%` : "92%" }} />
          ))}
        </div>
        <div className="mt-1.5 flex items-center gap-1.5 text-xs text-faint">
          <FileClock size={12} aria-hidden />
          {ev.status === "transcribing" ? "Transcribing…" : "Waiting in queue"} · {humanDuration(dur)} of speech
        </div>
      </div>
    );
  }
  if (ev.status === "failed") {
    return (
      <div className="my-0.5 flex items-start gap-2.5 rounded-md border border-[color-mix(in_oklch,var(--tm-danger),transparent_70%)] bg-danger-soft px-3 py-2">
        <AlertCircle size={15} className="mt-[2px] shrink-0 text-danger" aria-hidden />
        <div className="min-w-0 flex-1">
          <div className="text-base font-medium text-text">Couldn't transcribe {humanDuration(dur)} of speech</div>
          {ev.error && <div className="selectable mt-0.5 text-sm text-muted">{ev.error}</div>}
        </div>
        <Button size="sm" variant="secondary" icon={<RotateCcw size={13} />} onClick={onRetry}>
          Retry
        </Button>
      </div>
    );
  }
  if (ev.status === "empty") {
    // The engine marks audio its energy gate skipped with QUIET_AUDIO_NOTE. Retrying would be
    // gated the same way, so say what happened; the audio stays playable until retention
    // removes it.
    if (isQuiet(ev)) {
      return <p className="py-0.5 text-base italic text-faint">Too quiet to transcribe · {humanDuration(dur)}</p>;
    }
    return <p className="py-0.5 text-base italic text-faint">No words recognized · {humanDuration(dur)}</p>;
  }
  return <p className="selectable text-read text-text [text-wrap:pretty]">{eventText(ev)}</p>;
}

function EventDetail({ ev, avail }: { ev: SpeechEvent; avail: ReturnType<typeof audioAvailability> }) {
  const actions = useEventActions();
  const player = usePlayer();
  const isCurrent = player.eventId === ev.id;
  const isPlaying = isCurrent && player.playing;
  const conf = ev.segments.filter((s) => s.confidence != null);
  const avgConf = conf.length ? conf.reduce((n, s) => n + (s.confidence ?? 0), 0) / conf.length : null;
  const dur = ev.endedAt - ev.startedAt;

  return (
    <div className="pb-2 pt-1">
      {ev.segments.length > 1 && (
        <ol className="mb-3 space-y-1">
          {ev.segments.map((s) => {
            const active = isCurrent && player.currentMs >= s.startMs && player.currentMs < s.endMs;
            return (
              <li key={s.id} className="grid grid-cols-[44px_1fr] gap-2">
                <button
                  type="button"
                  disabled={!avail.ok}
                  onClick={() => player.play(ev, s.startMs)}
                  className="h-[23px] rounded text-left font-mono text-[11px] text-faint tnum enabled:hover:text-accent-text disabled:cursor-default"
                  title={avail.ok ? `Play from ${mediaTime(s.startMs)}` : clockSeconds(ev.startedAt + s.startMs)}
                >
                  +{mediaTime(s.startMs)}
                </button>
                <p className={`selectable text-read [text-wrap:pretty] transition-colors duration-[140ms] ${active ? "text-text" : isPlaying ? "text-muted" : "text-text"}`}>{s.text}</p>
              </li>
            );
          })}
        </ol>
      )}

      <div className="flex flex-wrap items-center gap-1.5 border-t border-line-soft pt-2.5">
        {avail.ok ? (
          <Button size="sm" variant="secondary" icon={isPlaying ? <Pause size={13} fill="currentColor" /> : <Play size={13} fill="currentColor" />} onClick={() => player.toggle(ev)}>
            {isPlaying ? "Pause" : "Play"}
          </Button>
        ) : (
          <span className="mr-1 inline-flex h-7 items-center gap-1.5 rounded-md bg-sunken px-2.5 text-sm text-faint">
            <VolumeX size={13} aria-hidden />
            {avail.message}
          </span>
        )}
        <Button size="sm" variant="ghost" icon={<Copy size={13} />} onClick={() => void actions.copyText(ev)} disabled={!eventText(ev)}>
          Copy
        </Button>
        <Button size="sm" variant="ghost" icon={<Clock3 size={13} />} onClick={() => void actions.copyWithTimestamp(ev)} disabled={!eventText(ev)}>
          Copy with time
        </Button>
        {ev.audio && (
          <Button size="sm" variant="ghost" icon={<VolumeX size={13} />} onClick={() => void actions.deleteAudio(ev)}>
            Delete audio
          </Button>
        )}
        <Button size="sm" variant="ghost" icon={<Trash2 size={13} />} onClick={() => void actions.remove(ev)} className="hover:!text-danger">
          Delete
        </Button>
        <span className="ml-auto flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-faint">
          <span className="tnum">{clockSeconds(ev.startedAt)}–{clockSeconds(ev.endedAt)}</span>
          <span>{humanDuration(dur)}</span>
          {ev.language && <span className="uppercase">{ev.language}</span>}
          {avgConf != null && <span className="tnum">{Math.round(avgConf * 100)}% confidence</span>}
          {ev.audio && <span className="tnum">{formatBytes(ev.audio.bytes)} Opus</span>}
          {ev.modelId && <span className="max-[1100px]:hidden">{ev.modelId}</span>}
        </span>
      </div>
    </div>
  );
}

/** Matches the engine's `QUIET_AUDIO_NOTE`. */
const QUIET_AUDIO_NOTE = "Too quiet to transcribe; audio kept";
function isQuiet(ev: SpeechEvent): boolean {
  return ev.error === QUIET_AUDIO_NOTE;
}
