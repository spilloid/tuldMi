import { useEffect, useRef } from "react";
import type { CaptureState } from "../../lib/ipc/types";
import { useReducedMotion } from "../../lib/motion";
import { onThemeChange, themeColor, useCanvasSize } from "./canvas";
import { scheduler } from "./scheduler";
import type { VizTask } from "./scheduler";

interface Props {
  state: CaptureState;
  size?: number;
  className?: string;
}

const BREATH_MS = 2600;
const MORPH_MS = 240;

/**
 * Capture-state glyph. Recording: a red core with a slow outward "breath" ring (30 fps is
 * plenty for it). Paused: two calm bars. Stopped: hollow ring. Error: amber core.
 * Between states the glyph morphs over ~240 ms. Reduced motion: static, no breath.
 * The state is conveyed in text by the surrounding UI; the canvas is aria-hidden.
 */
export function RecordingIndicator({ state, size = 16, className = "" }: Props) {
  const ref = useRef<HTMLCanvasElement>(null);
  const reduced = useReducedMotion();
  const s = useRef({ w: size, h: size, dpr: 1, from: state as CaptureState, to: state as CaptureState, t0: 0, born: 0 });
  const taskRef = useRef<VizTask | null>(null);
  const reducedRef = useRef(reduced);
  reducedRef.current = reduced;

  useCanvasSize(ref, (w, h, dpr) => {
    Object.assign(s.current, { w, h, dpr });
    if (taskRef.current) scheduler.wake(taskRef.current);
  });

  useEffect(() => {
    const st = s.current;
    if (st.to !== state) {
      st.from = st.to;
      st.to = state;
      st.t0 = performance.now();
      if (taskRef.current) scheduler.wake(taskRef.current);
    }
  }, [state]);

  useEffect(() => {
    const canvas = ref.current!;
    const ctx = canvas.getContext("2d")!;
    const st = s.current;
    st.born = performance.now();

    const glyph = (kind: CaptureState, alpha: number, now: number) => {
      const { w, h } = st;
      const cx = w / 2;
      const cy = h / 2;
      const R = Math.min(w, h) / 2;
      ctx.globalAlpha = alpha;
      if (kind === "recording" || kind === "starting") {
        const color = themeColor("rec");
        if (!reducedRef.current && kind === "recording") {
          const phase = ((now - st.born) % BREATH_MS) / BREATH_MS;
          const ease = 1 - Math.pow(1 - phase, 2);
          ctx.beginPath();
          ctx.arc(cx, cy, R * (0.42 + 0.56 * ease), 0, Math.PI * 2);
          ctx.globalAlpha = alpha * 0.45 * (1 - phase);
          ctx.fillStyle = color;
          ctx.fill();
          ctx.globalAlpha = alpha;
        }
        ctx.beginPath();
        ctx.arc(cx, cy, R * (kind === "starting" ? 0.3 : 0.42), 0, Math.PI * 2);
        ctx.fillStyle = color;
        ctx.fill();
      } else if (kind === "paused") {
        const color = themeColor("muted");
        const bw = Math.max(2, R * 0.3);
        const bh = R * 1.1;
        ctx.fillStyle = color;
        const r = bw / 2;
        for (const x of [cx - bw * 1.15, cx + bw * 0.15]) {
          ctx.beginPath();
          ctx.roundRect(x, cy - bh / 2, bw, bh, r);
          ctx.fill();
        }
      } else if (kind === "error") {
        ctx.beginPath();
        ctx.arc(cx, cy, R * 0.42, 0, Math.PI * 2);
        ctx.fillStyle = themeColor("warning");
        ctx.fill();
      } else {
        ctx.beginPath();
        ctx.arc(cx, cy, R * 0.38, 0, Math.PI * 2);
        ctx.lineWidth = 1.5;
        ctx.strokeStyle = themeColor("faint");
        ctx.stroke();
      }
      ctx.globalAlpha = 1;
    };

    const frame = (now: number) => {
      const { w, h, dpr } = st;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, w, h);
      const k = reducedRef.current ? 1 : Math.min(1, (now - st.t0) / MORPH_MS);
      if (k < 1) glyph(st.from, 1 - k, now);
      glyph(st.to, k, now);
      if (k < 1) return 60;
      return st.to === "recording" && !reducedRef.current ? 30 : 0;
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
  }, []);

  useEffect(() => {
    if (taskRef.current) scheduler.wake(taskRef.current);
  }, [reduced]);

  return (
    <canvas
      ref={ref}
      aria-hidden="true"
      className={`block shrink-0 ${className}`}
      style={{ width: size, height: size }}
    />
  );
}
