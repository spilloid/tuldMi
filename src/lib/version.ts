import { useEffect, useState } from "react";
import pkg from "../../package.json";

/** Version from package.json — the fallback for the demo backend and plain browsers. */
export const PACKAGE_VERSION: string = pkg.version;

/**
 * Resolve the app version. In Tauri this is the shell's version (`getVersion()`); in the demo or
 * a browser it is the package.json version.
 */
export async function resolveAppVersion(tauri: boolean, get?: () => Promise<string>): Promise<string> {
  if (!tauri) return PACKAGE_VERSION;
  try {
    const g = get ?? (await import("@tauri-apps/api/app")).getVersion;
    return await g();
  } catch {
    return PACKAGE_VERSION;
  }
}

export function useAppVersion(tauri: boolean): string {
  const [v, setV] = useState(PACKAGE_VERSION);
  useEffect(() => {
    let live = true;
    void resolveAppVersion(tauri).then((x) => live && setV(x));
    return () => {
      live = false;
    };
  }, [tauri]);
  return v;
}
