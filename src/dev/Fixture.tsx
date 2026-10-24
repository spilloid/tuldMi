// DEV-ONLY visual fixture for the playback path (never bundled in production builds).
//
// It synthesizes a short test signal in the browser (tone bursts with a speech-like envelope),
// encodes it as WAV, computes the peak envelope FROM THOSE SAMPLES, and feeds one event through
// the real Timeline + PlayerDock. The waveform therefore matches the audio exactly — it is a
// test tone, not a recording, and the page says so.

import { useEffect, useState } from "react";
import { DemoBackend } from "../lib/ipc/demo";
import type { SpeechEvent } from "../lib/ipc/types";
import { PEAKS_PER_SECOND } from "../lib/ipc/types";
import { AppStore } from "../state/store";
import { StoreProvider, useTimeline } from "../state/hooks";
import { UiProvider, useUi } from "../state/ui";
import { PlayerProvider, usePlayer } from "../state/player";
import { Shell } from "../app/Shell";
import { TooltipProvider } from "../components/ui/Tooltip";
import { ToastProvider } from "../components/ui/Toast";
import { ConfirmProvider } from "../components/ui/Confirm";
import { useTheme } from "../app/theme";

const SR = 16_000;
const SECONDS = 9;

function synthesize(): Float32Array {
  const n = SR * SECONDS;
  const out = new Float32Array(n);
  // Deterministic "syllables": bursts of a few harmonics with attack/decay.
  const bursts = [0.2, 0.55, 0.8, 1.3, 1.6, 2.4, 2.7, 3.1, 4.4, 4.7, 5.0, 5.5, 6.6, 6.9, 7.3, 7.9];
  bursts.forEach((t0, i) => {
    const f0 = 140 + ((i * 37) % 90);
    const len = 0.16 + ((i * 13) % 7) / 40;
    const amp = 0.25 + ((i * 29) % 10) / 20;
    const s0 = Math.floor(t0 * SR);
    const sl = Math.floor(len * SR);
    for (let k = 0; k < sl && s0 + k < n; k++) {
      const t = k / SR;
      const env = Math.sin((Math.PI * k) / sl) ** 1.5;
      out[s0 + k] += amp * env * (Math.sin(2 * Math.PI * f0 * t) * 0.6 + Math.sin(2 * Math.PI * f0 * 2 * t) * 0.3 + Math.sin(2 * Math.PI * f0 * 3 * t) * 0.1);
    }
  });
  return out;
}

function peaksOf(samples: Float32Array): number[] {
  const win = SR / PEAKS_PER_SECOND;
  const peaks: number[] = [];
  for (let i = 0; i < samples.length; i += win) {
    let m = 0;
    for (let j = i; j < i + win && j < samples.length; j++) m = Math.max(m, Math.abs(samples[j]));
    peaks.push(Math.min(255, Math.round(m * 255)));
  }
  return peaks;
}

function wav(samples: Float32Array): Blob {
  const buf = new ArrayBuffer(44 + samples.length * 2);
  const v = new DataView(buf);
  const w = (o: number, s: string) => [...s].forEach((c, i) => v.setUint8(o + i, c.charCodeAt(0)));
  w(0, "RIFF");
  v.setUint32(4, 36 + samples.length * 2, true);
  w(8, "WAVE");
  w(12, "fmt ");
  v.setUint32(16, 16, true);
  v.setUint16(20, 1, true);
  v.setUint16(22, 1, true);
  v.setUint32(24, SR, true);
  v.setUint32(28, SR * 2, true);
  v.setUint16(32, 2, true);
  v.setUint16(34, 16, true);
  w(36, "data");
  v.setUint32(40, samples.length * 2, true);
  samples.forEach((s, i) => v.setInt16(44 + i * 2, Math.max(-1, Math.min(1, s)) * 0x7fff, true));
  return new Blob([buf], { type: "audio/wav" });
}

class FixtureBackend extends DemoBackend {
  constructor(private ev: SpeechEvent) {
    super({ still: true, empty: true });
  }
  override async listEvents() {
    return [structuredClone(this.ev)];
  }
  override async getEvent() {
    return structuredClone(this.ev);
  }
  override audioUrl(path: string) {
    return path; // a blob: URL created above
  }
}

export function Fixture() {
  const [store, setStore] = useState<AppStore | null>(null);
  useEffect(() => {
    const samples = synthesize();
    const url = URL.createObjectURL(wav(samples));
    const now = Date.now();
    const id = "f1x7u4e0-0000-4000-8000-000000000001";
    const ev: SpeechEvent = {
      id,
      sessionId: "f1x7u4e0-0000-4000-8000-0000000000aa",
      source: "system",
      speaker: { id: "s", label: "Remote", kind: "remote" },
      startedAt: now - 60_000,
      endedAt: now - 60_000 + SECONDS * 1000,
      status: "done",
      audio: { assetId: "a", path: url, durationMs: SECONDS * 1000, bytes: samples.length * 2, peaks: peaksOf(samples) },
      audioExpired: false,
      segments: [
        { id: "g1", eventId: id, startMs: 0, endMs: 3400, text: "Dev fixture: a synthesized test tone, not a recording.", confidence: null },
        { id: "g2", eventId: id, startMs: 3800, endMs: 9000, text: "Its waveform is computed from the same samples you hear, so scrubbing can be checked end to end.", confidence: null },
      ],
      language: null,
      modelId: null,
      error: null,
    };
    const s = new AppStore(new FixtureBackend(ev));
    void s.start();
    setStore(s);
    return () => {
      s.stop();
      URL.revokeObjectURL(url);
    };
  }, []);
  if (!store) return null;
  return (
    <StoreProvider store={store}>
      <TooltipProvider>
        <ToastProvider>
          <ConfirmProvider>
            <UiProvider>
              <PlayerProvider>
                <Themed />
                <AutoSeek />
              </PlayerProvider>
            </UiProvider>
          </ConfirmProvider>
        </ToastProvider>
      </TooltipProvider>
    </StoreProvider>
  );
}

function Themed() {
  useTheme("system");
  return <Shell />;
}

/** `&seek=3.9` loads the fixture at that position (paused) so the progress state can be seen. */
function AutoSeek() {
  const player = usePlayer();
  const { events } = useTimeline();
  const ui = useUi();
  useEffect(() => {
    const at = Number(new URLSearchParams(window.location.search).get("seek"));
    const ev = events[0];
    if (!ev || !at) return;
    ui.select(ev.id);
    player.play(ev, at * 1000);
    const t = setTimeout(() => player.pause(), 400);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [events.length]);
  return null;
}
