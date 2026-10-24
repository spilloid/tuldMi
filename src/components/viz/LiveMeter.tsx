import { useEffect, useRef } from "react";
import type { Source } from "../../lib/ipc/types";
import { levelToMeter } from "../../lib/peaks";
import { useReducedMotion } from "../../lib/motion";
import { useLevels } from "../../state/hooks";
import { onThemeChange, themeColor, useCanvasSize } from "./canvas";
import { scheduler } from "./scheduler";
import type { VizTask } from "./scheduler";

interface Props {
  source: Source;
  /** compact: capture bar (bars 2px). large: settings/onboarding (bars 3px). */
  variant?: "compact" | "large";
  className?: string;
  /** Text shown to screen readers is "<label> level: …". */
  label?: string;
}

const HISTORY = 160;
const FRAME_MS = 33;

/**
 * Scrolling level history driven only by real `tuldmi://levels` frames. With no live frames
 * it renders a static dotted baseline ("no live audio") and stops requesting animation frames.
 */
export function LiveMeter({ source, variant = "compact", className = "", label }: Props) {
  const ref = useRef<HTMLCanvasElement>(null);
  const { levels, summary } = useLevels();
  const reduced = useReducedMotion();
  const state = useRef({
    rms: new Float32Array(HISTORY),
    peak: new Float32Array(HISTORY),
    speak: new Uint8Array(HISTORY),
    head: 0,
    lastPush: 0,
    w: 0,
    h: 0,
    dpr: 1,
  });
  const taskRef = useRef<VizTask | null>(null);
  const reducedRef = useRef(reduced);
  reducedRef.current = reduced;

  useCanvasSize(ref, (w, h, dpr) => {
    Object.assign(state.current, { w, h, dpr });
    if (taskRef.current) {
      taskRef.current.frame(performance.now());
      scheduler.wake(taskRef.current);
    }
  });

  useEffect(() => {
    const canvas = ref.current!;
    const ctx = canvas.getContext("2d")!;
    const s = state.current;
    const bw = variant === "large" ? 3 : 2;
    const gap = variant === "large" ? 2 : 1;
    const step = bw + gap;

    const draw = (now: number): number => {
      const { w, h, dpr } = s;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, w, h);
      const live = levels.isLive(source, now);
      const color = themeColor(source === "mic" ? "me" : "remote");
      const count = Math.min(HISTORY, Math.floor(w / step));
      const mid = h / 2;
      let energy = 0;
      // Sub-step scroll so motion is continuous between 30 Hz frames.
      const frac = reducedRef.current || !live ? 0 : Math.min(1, (now - s.lastPush) / FRAME_MS);
      for (let i = 0; i < count; i++) {
        const idx = (s.head - 1 - i + HISTORY * 2) % HISTORY;
        const r = s.rms[idx];
        const p = s.peak[idx];
        energy += r;
        const x = w - (i + 1) * step - frac * step + gap;
        if (x < -bw) break;
        const fade = Math.min(1, (count - i) / 14); // soft left edge
        if (p > 0.01) {
          ctx.globalAlpha = 0.28 * fade;
          ctx.fillStyle = color;
          const ph = Math.max(1, p * (h - 2));
          ctx.fillRect(x, mid - ph / 2, bw, ph);
        }
        ctx.globalAlpha = (s.speak[idx] ? 1 : 0.55) * fade;
        ctx.fillStyle = color;
        const rh = Math.max(1, r * (h - 2));
        ctx.fillRect(x, mid - rh / 2, bw, rh);
      }
      ctx.globalAlpha = 1;
      if (!live && energy < 0.001) {
        // Idle baseline: quiet dotted line, nothing animating.
        ctx.fillStyle = themeColor("line-strong");
        for (let x = gap; x < w; x += step * 2) ctx.fillRect(x, mid - 0.5, bw, 1);
        return 0;
      }
      if (!live) {
        // Stream stopped: let history drain once, then sleep.
        s.rms.fill(0);
        s.peak.fill(0);
        return 30;
      }
      return reducedRef.current ? 0 : 60;
    };

    const task: VizTask = { el: canvas, frame: draw };
    taskRef.current = task;
    const remove = scheduler.add(task);
    const offFrames = levels.onFrames((frames) => {
      for (const f of frames) {
        if (f.source !== source) continue;
        s.rms[s.head] = levelToMeter(f.rms);
        s.peak[s.head] = levelToMeter(f.peak);
        s.speak[s.head] = f.speaking ? 1 : 0;
        s.head = (s.head + 1) % HISTORY;
        s.lastPush = performance.now();
      }
      scheduler.wake(task);
    });
    const offTheme = onThemeChange(() => scheduler.wake(task));
    return () => {
      remove();
      offFrames();
      offTheme();
      taskRef.current = null;
    };
  }, [levels, source, variant]);

  const sum = summary[source];
  const name = label ?? (source === "mic" ? "Mic" : "System audio");
  const text = !sum.live ? "no live audio" : sum.speaking ? "speech detected" : "listening, quiet";
  return (
    <span className={`relative inline-block ${className}`}>
      <canvas ref={ref} aria-hidden="true" className="block h-full w-full" />
      <span className="sr-only">
        {name} level: {text}
      </span>
    </span>
  );
}
