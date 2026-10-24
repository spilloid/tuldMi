import { createContext, useCallback, useContext, useEffect, useState, useSyncExternalStore } from "react";
import type { ReactNode } from "react";
import type { Backend } from "../lib/ipc/backend";
import type { SearchHit, SearchQuery, Settings } from "../lib/ipc/types";
import { errorMessage } from "../lib/ipc/backend";
import { useDebouncedValue } from "../lib/hooks/useDebounce";
import type { AppStore, Observable } from "./store";

const StoreContext = createContext<AppStore | null>(null);

export function StoreProvider({ store, children }: { store: AppStore; children: ReactNode }) {
  return <StoreContext.Provider value={store}>{children}</StoreContext.Provider>;
}

export function useStore(): AppStore {
  const s = useContext(StoreContext);
  if (!s) throw new Error("StoreProvider missing");
  return s;
}

export function useBackend(): Backend {
  return useStore().backend;
}

export function useObservable<T>(o: Observable<T>): T {
  return useSyncExternalStore(o.subscribe, o.get, o.get);
}

export function useStatus() {
  return useObservable(useStore().status);
}

/** Level store for canvases plus a low-frequency summary for text equivalents. */
export function useLevels() {
  const store = useStore();
  const summary = useObservable(store.levels.summary);
  return { levels: store.levels, summary };
}

export function useTimeline() {
  const store = useStore();
  const state = useObservable(store.timeline);
  return {
    ...state,
    loadOlder: useCallback(() => store.loadOlder(), [store]),
    ensureLoaded: useCallback((id: string) => store.ensureLoaded(id), [store]),
  };
}

export function useSettings() {
  const store = useStore();
  const settings = useObservable(store.settings);
  const save = useCallback((next: Settings) => store.saveSettings(next), [store]);
  const patch = useCallback(
    (p: Partial<Settings>) => {
      const cur = store.settings.get();
      if (!cur) return Promise.reject("Settings not loaded");
      return store.saveSettings({ ...cur, ...p });
    },
    [store],
  );
  return { settings, save, patch };
}

export function useModels() {
  const store = useStore();
  const models = useObservable(store.models);
  const progress = useObservable(store.progress);
  return { models, progress, refresh: useCallback(() => store.refreshModels(), [store]), store };
}

export const SEARCH_DEBOUNCE_MS = 120;

/** Debounced search. Results stay visible while the next query is in flight. */
export function useSearch(query: SearchQuery, enabled = true) {
  const backend = useBackend();
  const key = JSON.stringify(query);
  const debouncedKey = useDebouncedValue(key, SEARCH_DEBOUNCE_MS);
  const [state, setState] = useState<{ hits: SearchHit[]; loading: boolean; error: string | null; forKey: string | null }>({
    hits: [],
    loading: false,
    error: null,
    forKey: null,
  });

  useEffect(() => {
    if (!enabled) return;
    const q: SearchQuery = JSON.parse(debouncedKey);
    if (!q.text.trim()) {
      setState({ hits: [], loading: false, error: null, forKey: debouncedKey });
      return;
    }
    let cancelled = false;
    setState((s) => ({ ...s, loading: true }));
    backend
      .search(q)
      .then((hits) => !cancelled && setState({ hits, loading: false, error: null, forKey: debouncedKey }))
      .catch((e) => !cancelled && setState({ hits: [], loading: false, error: errorMessage(e), forKey: debouncedKey }));
    return () => {
      cancelled = true;
    };
  }, [backend, debouncedKey, enabled]);

  return { ...state, pending: key !== debouncedKey || state.loading, settled: state.forKey === key };
}
