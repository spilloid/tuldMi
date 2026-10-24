import type { Backend } from "./backend";
import { DemoBackend } from "./demo";
import { TauriBackend } from "./tauri";
import { installDemoClock } from "./demoClock";

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * Tauri when running inside the shell, unless the URL has `?demo`; otherwise the demo backend.
 * Demo flags (query string): `fresh` (first-run onboarding), `empty` (no history), `still`
 * (no scripted live arrivals).
 */
export function createBackend(search: string = window.location.search): Backend {
  const q = new URLSearchParams(search);
  if (isTauri() && !q.has("demo")) return new TauriBackend();
  installDemoClock(search);
  return new DemoBackend({ fresh: q.has("fresh"), empty: q.has("empty"), still: q.has("still"), paused: q.has("paused") });
}

export * from "./backend";
export * from "./types";
