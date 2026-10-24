import { useSyncExternalStore } from "react";

/**
 * Tiny hash router. Every surface is reachable by URL so it can be screenshotted:
 *   #/timeline                 #/timeline/<eventId>   (expanded + selected)
 *   #/search?q=…&src=mic&range=7d
 *   #/settings/<section>
 *   #/onboarding/<step>
 * Overlays: `&palette` on any route opens the command palette, `&keys` the shortcut sheet.
 */
export type Route =
  | { view: "timeline"; eventId?: string }
  | { view: "search"; params: URLSearchParams }
  | { view: "settings"; section: string }
  | { view: "onboarding"; step: number };

export function parseHash(hash: string): Route {
  const raw = hash.replace(/^#/, "") || "/timeline";
  const [path, qs = ""] = raw.split("?");
  const params = new URLSearchParams(qs);
  const parts = path.split("/").filter(Boolean);
  switch (parts[0]) {
    case "search":
      return { view: "search", params };
    case "settings":
      return { view: "settings", section: parts[1] ?? "audio" };
    case "onboarding":
      return { view: "onboarding", step: Math.max(0, Number(parts[1] ?? 0) || 0) };
    case "timeline":
    default:
      return { view: "timeline", eventId: parts[1] ? decodeURIComponent(parts[1]) : undefined };
  }
}

export function hashParams(): URLSearchParams {
  const raw = window.location.hash.replace(/^#/, "");
  return new URLSearchParams(raw.split("?")[1] ?? "");
}

function subscribe(cb: () => void) {
  window.addEventListener("hashchange", cb);
  return () => window.removeEventListener("hashchange", cb);
}

const getHash = () => window.location.hash;

export function useHash(): string {
  return useSyncExternalStore(subscribe, getHash, getHash);
}

export function useRoute(): Route {
  return parseHash(useHash());
}

export function navigate(to: string, opts: { replace?: boolean } = {}) {
  const hash = to.startsWith("#") ? to : `#${to}`;
  if (window.location.hash === hash) return;
  if (opts.replace) {
    history.replaceState(null, "", hash);
    window.dispatchEvent(new HashChangeEvent("hashchange"));
  } else {
    window.location.hash = hash;
  }
}
