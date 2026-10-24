import { useEffect, useRef } from "react";
import type { RefObject } from "react";

/** Device-pixel-ratio aware canvas sizing. Calls `onResize` with CSS-pixel dimensions. */
export function useCanvasSize(
  ref: RefObject<HTMLCanvasElement | null>,
  onResize: (w: number, h: number, dpr: number) => void,
) {
  const cb = useRef(onResize);
  cb.current = onResize;
  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    let mq: MediaQueryList | null = null;
    const apply = () => {
      const rect = canvas.getBoundingClientRect();
      const dpr = Math.min(3, window.devicePixelRatio || 1);
      const w = Math.max(1, Math.round(rect.width));
      const h = Math.max(1, Math.round(rect.height));
      const pw = Math.round(w * dpr);
      const ph = Math.round(h * dpr);
      if (canvas.width !== pw || canvas.height !== ph) {
        canvas.width = pw;
        canvas.height = ph;
      }
      cb.current(w, h, dpr);
    };
    const ro = new ResizeObserver(apply);
    ro.observe(canvas);
    // Re-apply when the DPR changes (window dragged to another monitor, zoom).
    const watchDpr = () => {
      mq?.removeEventListener("change", onDpr);
      mq = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
      mq.addEventListener("change", onDpr);
    };
    const onDpr = () => {
      apply();
      watchDpr();
    };
    watchDpr();
    apply();
    return () => {
      ro.disconnect();
      mq?.removeEventListener("change", onDpr);
    };
  }, [ref]);
}

// ---- Theme colors for canvas -----------------------------------------------------------------

type ColorName =
  | "me"
  | "remote"
  | "accent"
  | "rec"
  | "muted"
  | "faint"
  | "line"
  | "line-strong"
  | "text"
  | "warning"
  | "danger"
  | "me-line"
  | "remote-line"
  | "surface";

let cache: Partial<Record<ColorName, string>> = {};
const themeListeners = new Set<() => void>();

if (typeof document !== "undefined" && typeof MutationObserver !== "undefined") {
  new MutationObserver(() => {
    cache = {};
    themeListeners.forEach((l) => l());
  }).observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
}

export function themeColor(name: ColorName): string {
  let c = cache[name];
  if (!c) {
    c = getComputedStyle(document.documentElement).getPropertyValue(`--tm-${name}`).trim() || "#888";
    cache[name] = c;
  }
  return c;
}

/** Subscribe to theme switches (to redraw static canvases). */
export function onThemeChange(fn: () => void): () => void {
  themeListeners.add(fn);
  return () => {
    themeListeners.delete(fn);
  };
}
