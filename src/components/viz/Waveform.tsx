import { useEffect, useRef } from "react";
import type { KeyboardEvent, PointerEvent } from "react";
import type { Source } from "../../lib/ipc/types";
import { mapPeaks, shapeHeight, xToTime } from "../../lib/peaks";
import { mediaTime } from "../../lib/time";
import { onThemeChange, themeColor, useCanvasSize } from "./canvas";
import { scheduler } from "./scheduler";
import type { VizTask } from "./scheduler";

interface Props {
  peaks: number[];
  source: Source;
  variant?: "mini" | "player";
  className?: string;
  /** Player: current position 0..1, read every frame while `playing`. */
  getProgress?: () => number;
  playing?: boolean;
  durationMs?: number;
  /** Player: seek to a fraction 0..1. Makes the waveform an accessible slider. */
  onSeek?: (fraction: number) => void;
  /** Segment boundaries as fractions 0..1 (drawn as faint ticks). */
  marks?: number[];
}

/**
 * Canvas waveform from the stored peak envelope (real audio only — there is no fallback
 * drawing for missing peaks; callers show the unavailable state instead).
 */
export function Waveform({
  peaks,
  source,
  variant = "mini",
  className = "",
  getProgress,
  playing = false,
  durationMs = 0,
  onSeek,
  marks,
}: Props) {
  const ref = useRef<HTMLCanvasElement>(null);
  const st = useRef({ w: 0, h: 0, dpr: 1, hover: -1, scrubbing: false, bars: new Float32Array(0) as Float32Array<ArrayBufferLike>, barsFor: -1 });
  const props = useRef({ peaks, getProgress, playing, marks, source });
  props.current = { peaks, getProgress, playing, marks, source };
  const taskRef = useRef<VizTask | null>(null);
  const wake = () => {
    if (taskRef.current) scheduler.wake(taskRef.current);
  };

  useCanvasSize(ref, (w, h, dpr) => {
    Object.assign(st.current, { w, h, dpr, barsFor: -1 });
    wake();
  });

  useEffect(() => {
    st.current.barsFor = -1;
    wake();
  }, [peaks, marks]);
  useEffect(wake, [playing, source]);

  useEffect(() => {
    const canvas = ref.current!;
    const ctx = canvas.getContext("2d")!;
    const s = st.current;
    const mini = variant === "mini";
    const bw = mini ? 2 : 2;
    const gap = mini ? 1.5 : 1.5;
    const step = bw + gap;

    const frame = () => {
      const { w, h, dpr } = s;
      const p = props.current;
      const count = Math.max(1, Math.floor((w + gap) / step));
      if (s.barsFor !== count) {
        s.bars = mapPeaks(p.peaks, count);
        s.barsFor = count;
      }
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, w, h);
      const color = themeColor(p.source === "mic" ? "me" : "remote");
      const rest = themeColor(mini ? (p.source === "mic" ? "me-line" : "remote-line") : "line-strong");
      const progress = p.getProgress ? Math.max(0, Math.min(1, p.getProgress())) : -1;
      const playedX = progress >= 0 ? progress * w : -1;
      const hoverX = s.hover;
      const mid = h / 2;
      for (let i = 0; i < count; i++) {
        const x = i * step;
        const v = shapeHeight(s.bars[i], mini ? 0.08 : 0.05);
        const bh = Math.max(1.5, v * (h - 2));
        let fill = mini ? color : rest;
        let alpha = mini ? 0.7 : 1;
        if (!mini) {
          if (x + bw <= playedX) {
            fill = color;
          } else if (x < playedX) {
            fill = color;
            alpha = 0.75;
          } else if (hoverX >= 0 && x < hoverX) {
            fill = color;
            alpha = 0.35;
          }
        }
        ctx.globalAlpha = alpha;
        ctx.fillStyle = fill;
        ctx.beginPath();
        ctx.roundRect(x, mid - bh / 2, bw, bh, 1);
        ctx.fill();
      }
      ctx.globalAlpha = 1;
      if (!mini && p.marks) {
        ctx.fillStyle = themeColor("faint");
        for (const m of p.marks) {
          if (m <= 0 || m >= 1) continue;
          ctx.fillRect(Math.round(m * w) - 0.5, h - 3, 1, 3);
          ctx.fillRect(Math.round(m * w) - 0.5, 0, 1, 3);
        }
      }
      if (!mini && playedX >= 0) {
        ctx.fillStyle = themeColor("text");
        ctx.fillRect(Math.round(playedX) - 0.75, 1, 1.5, h - 2);
      }
      if (!mini && hoverX >= 0 && !s.scrubbing) {
        ctx.globalAlpha = 0.5;
        ctx.fillStyle = themeColor("muted");
        ctx.fillRect(Math.round(hoverX) - 0.5, 0, 1, h);
        ctx.globalAlpha = 1;
      }
      return p.playing ? 60 : 0;
    };
    const task: VizTask = { el: canvas, frame };
    taskRef.current = task;
    const remove = scheduler.add(task);
    const offTheme = onThemeChange(() => scheduler.wake(task));
    return () => {
      remove();
      offTheme();
      taskRef.current = null;
    };
  }, [variant]);

  if (!onSeek) {
    return <canvas ref={ref} aria-hidden="true" className={`block ${className}`} />;
  }

  const fractionAt = (e: PointerEvent) => {
    const rect = e.currentTarget.getBoundingClientRect();
    return xToTime(e.clientX - rect.left, rect.width, 1);
  };
  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    st.current.scrubbing = true;
    onSeek(fractionAt(e));
    wake();
  };
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    st.current.hover = Math.max(0, Math.min(rect.width, e.clientX - rect.left));
    if (st.current.scrubbing) onSeek(fractionAt(e));
    wake();
  };
  const onPointerUp = (e: PointerEvent<HTMLDivElement>) => {
    st.current.scrubbing = false;
    e.currentTarget.releasePointerCapture(e.pointerId);
    wake();
  };
  const onLeave = () => {
    st.current.hover = -1;
    wake();
  };
  const current = (getProgress?.() ?? 0) * durationMs;
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (!durationMs) return;
    const big = e.shiftKey ? 1000 : 5000;
    let next: number | null = null;
    if (e.key === "ArrowRight") next = current + big;
    else if (e.key === "ArrowLeft") next = current - big;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = durationMs;
    else if (e.key === "PageUp") next = current + 10_000;
    else if (e.key === "PageDown") next = current - 10_000;
    if (next == null) return;
    e.preventDefault();
    e.stopPropagation();
    onSeek(Math.max(0, Math.min(1, next / durationMs)));
    wake();
  };

  return (
    <div
      role="slider"
      tabIndex={0}
      aria-label="Playback position"
      aria-valuemin={0}
      aria-valuemax={Math.round(durationMs / 1000)}
      aria-valuenow={Math.round(current / 1000)}
      aria-valuetext={`${mediaTime(current)} of ${mediaTime(durationMs)}`}
      className={`relative touch-none rounded-sm ${className}`}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onPointerLeave={onLeave}
      onKeyDown={onKeyDown}
    >
      <canvas ref={ref} aria-hidden="true" className="block h-full w-full" />
    </div>
  );
}
