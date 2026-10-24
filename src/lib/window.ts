import { isTauri } from "./ipc";

/** Window controls for the frameless window. No-ops (and hidden in UI) outside Tauri. */
async function win() {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  return getCurrentWindow();
}

export const windowControls = {
  available: () => isTauri(),
  minimize: async () => (await win()).minimize(),
  toggleMaximize: async () => (await win()).toggleMaximize(),
  /** Close = hide-to-tray; the shell decides. */
  close: async () => (await win()).close(),
  isMaximized: async () => (await win()).isMaximized(),
  onResized: async (fn: () => void) => (await win()).onResized(fn),
};
