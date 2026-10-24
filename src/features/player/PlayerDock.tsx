import * as DM from "@radix-ui/react-dropdown-menu";
import { AnimatePresence, motion } from "motion/react";
import { Check, Pause, Play, VolumeX, X } from "lucide-react";
import { useMemo } from "react";
import type { SpeechEvent } from "../../lib/ipc/types";
import { clock, mediaTime, sourceLabel } from "../../lib/time";
import { transition, useReducedMotion } from "../../lib/motion";
import { Waveform } from "../../components/viz/Waveform";
import { IconButton } from "../../components/ui/Button";
import { Tooltip } from "../../components/ui/Tooltip";
import { SPEEDS, audioAvailability, usePlayer } from "../../state/player";
import { useBackend, useTimeline } from "../../state/hooks";
import { useUi } from "../../state/ui";

/** Docked player for the selected event. Sits above the capture bar. */
export function PlayerDock() {
  const ui = useUi();
  const { events } = useTimeline();
  const reduced = useReducedMotion();
  const player = usePlayer();
  const id = player.eventId ?? ui.selectedId;
  const ev = useMemo(() => events.find((e) => e.id === id) ?? null, [events, id]);

  return (
    <AnimatePresence initial={false}>
      {ev && (
        <motion.div
          key="dock"
          initial={reduced ? { opacity: 0 } : { opacity: 0, y: 12 }}
          animate={{ opacity: 1, y: 0 }}
          exit={reduced ? { opacity: 0 } : { opacity: 0, y: 12 }}
          transition={transition.panel}
          className="shrink-0 border-t border-line-soft bg-surface"
        >
          <DockBody ev={ev} onClose={() => { player.stop(); ui.select(null); }} />
        </motion.div>
      )}
    </AnimatePresence>
  );
}

function DockBody({ ev, onClose }: { ev: SpeechEvent; onClose: () => void }) {
  const backend = useBackend();
  const player = usePlayer();
  const avail = audioAvailability(ev, backend);
  const current = player.eventId === ev.id;
  const playing = current && player.playing;
  const duration = (current && player.durationMs) || ev.audio?.durationMs || ev.endedAt - ev.startedAt;
  const pos = current ? player.currentMs : 0;
  const segIndex = ev.segments.findIndex((s) => pos >= s.startMs && pos < s.endMs);
  const seg = segIndex >= 0 ? ev.segments[segIndex] : null;
  const segPct = seg ? (pos - seg.startMs) / Math.max(1, seg.endMs - seg.startMs) : 0;
  const marks = ev.segments.length > 1 && duration ? ev.segments.slice(1).map((s) => s.startMs / duration) : undefined;
  const me = ev.source === "mic";

  return (
    <div className="mx-auto flex h-[64px] w-full max-w-[820px] items-center gap-4 px-6 max-[1100px]:px-4">
      <Tooltip content={avail.ok ? (playing ? "Pause" : "Play") : avail.message} shortcut={avail.ok ? "Space" : undefined}>
        <button
          type="button"
          aria-label={playing ? "Pause" : "Play"}
          disabled={!avail.ok}
          onClick={() => player.toggle(ev)}
          className={`inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-full transition-[background-color,transform] duration-[140ms] active:scale-95 ${
            avail.ok ? "bg-text text-canvas hover:opacity-90" : "bg-sunken text-faint"
          }`}
        >
          {playing ? <Pause size={15} fill="currentColor" /> : <Play size={15} fill="currentColor" className="translate-x-[1px]" />}
        </button>
      </Tooltip>

      <div className="w-[150px] shrink-0 leading-tight max-[1100px]:w-[120px]">
        <div className="flex items-center gap-1.5">
          <span aria-hidden className={`h-1.5 w-1.5 rounded-full ${me ? "bg-me" : "bg-remote"}`} />
          <span className={`text-sm font-semibold ${me ? "text-me-text" : "text-remote-text"}`}>{sourceLabel(ev)}</span>
          <span className="font-mono text-[11px] text-faint tnum">{clock(ev.startedAt)}</span>
        </div>
        <div className="mt-0.5 truncate text-xs text-faint">
          {avail.ok
            ? seg
              ? `Segment ${segIndex + 1} of ${ev.segments.length} · ${Math.round(segPct * 100)}%`
              : `${ev.segments.length || 1} ${ev.segments.length === 1 ? "segment" : "segments"}`
            : new Date(ev.startedAt).toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" })}
        </div>
      </div>

      <div className="min-w-0 flex-1">
        {avail.ok && ev.audio ? (
          <Waveform
            variant="player"
            peaks={ev.audio.peaks}
            source={ev.source}
            playing={playing}
            getProgress={current ? player.getProgress : () => 0}
            durationMs={duration}
            marks={marks}
            onSeek={(f) => (current ? player.seekFraction(f) : player.play(ev, f * duration))}
            className="h-10 w-full"
          />
        ) : (
          <div className="flex h-10 items-center gap-2.5 rounded-md border border-dashed border-line-strong px-3 text-sm text-faint">
            <VolumeX size={14} aria-hidden />
            <span className="truncate">{avail.ok ? "Waveform unavailable" : avail.message}</span>
          </div>
        )}
        {player.error && current && <div className="mt-0.5 truncate text-xs text-danger">{player.error}</div>}
      </div>

      <div className="w-[92px] shrink-0 text-right font-mono text-[12px] text-muted tnum" aria-live="off">
        {mediaTime(pos)} <span className="text-faint">/ {mediaTime(duration)}</span>
      </div>

      <SpeedMenu disabled={!avail.ok} />
      <IconButton label="Close player" onClick={onClose} size="sm" shortcut="Esc">
        <X size={15} />
      </IconButton>
    </div>
  );
}

function SpeedMenu({ disabled }: { disabled: boolean }) {
  const player = usePlayer();
  return (
    <DM.Root modal={false}>
      <DM.Trigger
        disabled={disabled}
        aria-label={`Playback speed ${player.rate}×`}
        className="inline-flex h-7 min-w-[44px] items-center justify-center rounded-md border border-line bg-raised px-2 font-mono text-[11.5px] font-medium text-muted tnum hover:text-text disabled:opacity-40"
      >
        {player.rate}×
      </DM.Trigger>
      <DM.Portal>
        <DM.Content side="top" align="end" sideOffset={6} className="tm-pop tm-menu z-[70] !min-w-[120px]">
          <DM.Label className="px-2 pb-1 pt-1 text-xs text-faint">Speed</DM.Label>
          <DM.RadioGroup value={String(player.rate)} onValueChange={(v) => player.setRate(Number(v))}>
            {SPEEDS.map((s) => (
              <DM.RadioItem key={s} value={String(s)} className="tm-menu-item pl-7 font-mono text-[12px] tnum">
                <DM.ItemIndicator className="absolute left-2 inline-flex">
                  <Check size={13} className="text-accent-text" />
                </DM.ItemIndicator>
                {s}×
              </DM.RadioItem>
            ))}
          </DM.RadioGroup>
        </DM.Content>
      </DM.Portal>
    </DM.Root>
  );
}
