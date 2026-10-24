import { useEffect } from "react";
import type { Theme } from "../lib/ipc/types";

/** Apply settings.theme (System follows prefers-color-scheme). `?theme=light|dark` overrides. */
export function useTheme(theme: Theme | undefined) {
  useEffect(() => {
    const forced = new URLSearchParams(window.location.search).get("theme") as Theme | null;
    const t: Theme = forced === "light" || forced === "dark" ? forced : theme ?? "system";
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const dark = t === "dark" || (t === "system" && mq.matches);
      document.documentElement.dataset.theme = dark ? "dark" : "light";
    };
    apply();
    try {
      if (!forced) localStorage.setItem("tuldmi.theme", t);
    } catch {
      /* storage unavailable: pre-paint falls back to system */
    }
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [theme]);
}
