import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import type { Backend } from "../lib/ipc/backend";
import type { SpeechEvent } from "../lib/ipc/types";
import { useBackend } from "./hooks";

export type Availability =
  | { ok: true; url: string }
  | { ok: false; reason: "demo" | "expired" | "notKept" | "pending"; message: string };

export function audioAvailability(ev: SpeechEvent, backend: Backend): Availability {
  if (ev.audio) {
    const url = backend.audioUrl(ev.audio.path);
    if (url) return { ok: true, url };
  }
  if (backend.kind === "demo" && !ev.audioExpired)
    return { ok: false, reason: "demo", message: "Demo events have no recordings" };
  if (ev.audioExpired)
    return { ok: false, reason: "expired", message: "Audio removed under your retention setting — transcript kept" };
  return { ok: false, reason: "notKept", message: "Audio wasn't kept for this event" };
}

/** A pause() racing a pending play() rejects with AbortError — that is not a failure. */
function playErrorMessage(e: unknown): string | null {
  if (e instanceof DOMException && e.name === "AbortError") return null;
  if (e instanceof DOMException && e.name === "NotAllowedError") return "Playback was blocked. Press Play to start.";
  return "This recording couldn't be played.";
}

export const SPEEDS = [0.75, 1, 1.25, 1.5, 2] as const;

interface PlayerApi {
  eventId: string | null;
  playing: boolean;
  rate: number;
  currentMs: number;
  durationMs: number;
  error: string | null;
  /** Load (and optionally start) an event's audio. */
  play(ev: SpeechEvent, atMs?: number): void;
  toggle(ev?: SpeechEvent): void;
  pause(): void;
  seekFraction(f: number): void;
  setRate(r: number): void;
  stop(): void;
  /** Smooth position for canvases (reads the media element directly). */
  getProgress(): number;
}

const Ctx = createContext<PlayerApi | null>(null);

export function PlayerProvider({ children }: { children: ReactNode }) {
  const backend = useBackend();
  const audio = useRef<HTMLAudioElement | null>(null);
  const [eventId, setEventId] = useState<string | null>(null);
  const [playing, setPlaying] = useState(false);
  const [rate, setRateState] = useState(1);
  const [currentMs, setCurrentMs] = useState(0);
  const [durationMs, setDurationMs] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const loadedUrl = useRef<string | null>(null);
  const onPlayError = useCallback((e: unknown) => {
    const m = playErrorMessage(e);
    if (m) setError(m);
  }, []);
  const fallbackDuration = useRef(0);

  if (!audio.current && typeof Audio !== "undefined") {
    audio.current = new Audio();
    audio.current.preload = "metadata";
  }

  useEffect(() => {
    const a = audio.current;
    if (!a) return;
    const onTime = () => setCurrentMs(a.currentTime * 1000);
    const onPlay = () => setPlaying(true);
    const onPause = () => setPlaying(false);
    const onMeta = () => setDurationMs(Number.isFinite(a.duration) ? a.duration * 1000 : fallbackDuration.current);
    const onErr = () => {
      setPlaying(false);
      setError("This recording couldn't be played. The file may have been moved or deleted.");
    };
    a.addEventListener("timeupdate", onTime);
    a.addEventListener("play", onPlay);
    a.addEventListener("pause", onPause);
    a.addEventListener("ended", onPause);
    a.addEventListener("loadedmetadata", onMeta);
    a.addEventListener("error", onErr);
    return () => {
      a.pause();
      a.removeEventListener("timeupdate", onTime);
      a.removeEventListener("play", onPlay);
      a.removeEventListener("pause", onPause);
      a.removeEventListener("ended", onPause);
      a.removeEventListener("loadedmetadata", onMeta);
      a.removeEventListener("error", onErr);
      a.removeAttribute("src");
      a.load();
    };
  }, []);

  const load = useCallback(
    (ev: SpeechEvent): boolean => {
      const a = audio.current;
      const avail = audioAvailability(ev, backend);
      setEventId(ev.id);
      if (!a || !avail.ok) {
        a?.pause();
        setPlaying(false);
        return false;
      }
      if (loadedUrl.current !== avail.url) {
        setError(null);
        loadedUrl.current = avail.url;
        fallbackDuration.current = ev.audio?.durationMs ?? 0;
        setDurationMs(fallbackDuration.current);
        setCurrentMs(0);
        a.src = avail.url;
        a.playbackRate = rate;
      }
      return true;
    },
    [backend, rate],
  );

  const play = useCallback(
    (ev: SpeechEvent, atMs?: number) => {
      if (!load(ev)) return;
      const a = audio.current!;
      if (atMs != null) a.currentTime = atMs / 1000;
      void a.play().catch(onPlayError);
    },
    [load],
  );

  const pause = useCallback(() => audio.current?.pause(), []);

  const toggle = useCallback(
    (ev?: SpeechEvent) => {
      const a = audio.current;
      if (!a) return;
      if (ev && ev.id !== eventId) return play(ev);
      if (!loadedUrl.current) {
        if (ev) play(ev);
        return;
      }
      if (a.paused) void a.play().catch(onPlayError);
      else a.pause();
    },
    [eventId, play],
  );

  const seekFraction = useCallback((f: number) => {
    const a = audio.current;
    if (!a || !loadedUrl.current) return;
    const d = Number.isFinite(a.duration) ? a.duration : fallbackDuration.current / 1000;
    a.currentTime = Math.max(0, Math.min(d, f * d));
    setCurrentMs(a.currentTime * 1000);
  }, []);

  const setRate = useCallback((r: number) => {
    setRateState(r);
    if (audio.current) audio.current.playbackRate = r;
  }, []);

  const stop = useCallback(() => {
    const a = audio.current;
    a?.pause();
    setEventId(null);
    setPlaying(false);
  }, []);

  const getProgress = useCallback(() => {
    const a = audio.current;
    if (!a || !loadedUrl.current) return 0;
    const d = Number.isFinite(a.duration) && a.duration > 0 ? a.duration : fallbackDuration.current / 1000;
    return d > 0 ? a.currentTime / d : 0;
  }, []);

  const api = useMemo(
    () => ({ eventId, playing, rate, currentMs, durationMs, error, play, toggle, pause, seekFraction, setRate, stop, getProgress }),
    [eventId, playing, rate, currentMs, durationMs, error, play, toggle, pause, seekFraction, setRate, stop, getProgress],
  );
  return <Ctx.Provider value={api}>{children}</Ctx.Provider>;
}

export function usePlayer(): PlayerApi {
  const c = useContext(Ctx);
  if (!c) throw new Error("PlayerProvider missing");
  return c;
}
