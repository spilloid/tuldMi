import { PEAKS_PER_SECOND } from "./ipc/types";

/**
 * Resample a 0..255 peak envelope into `bars` buckets, returning 0..1 heights.
 * Each bucket takes the max of the source samples it covers (so short transients survive
 * downsampling); when upsampling, buckets interpolate linearly between neighbours.
 */
export function mapPeaks(peaks: ArrayLike<number>, bars: number): Float32Array {
  const out = new Float32Array(Math.max(0, Math.floor(bars)));
  const n = peaks.length;
  if (n === 0 || out.length === 0) return out;
  if (n >= out.length) {
    for (let i = 0; i < out.length; i++) {
      const a = Math.floor((i * n) / out.length);
      const b = Math.max(a + 1, Math.floor(((i + 1) * n) / out.length));
      let m = 0;
      for (let j = a; j < b && j < n; j++) if (peaks[j] > m) m = peaks[j];
      out[i] = clamp01(m / 255);
    }
  } else {
    for (let i = 0; i < out.length; i++) {
      const x = out.length === 1 ? 0 : (i * (n - 1)) / (out.length - 1);
      const a = Math.floor(x);
      const b = Math.min(n - 1, a + 1);
      const t = x - a;
      out[i] = clamp01((peaks[a] * (1 - t) + peaks[b] * t) / 255);
    }
  }
  return out;
}

/**
 * Perceptual shaping for display: a gentle sqrt curve lifts quiet speech so it reads as
 * activity, with a floor so silent stretches still draw a hairline.
 */
export function shapeHeight(v: number, floor = 0.04): number {
  return floor + (1 - floor) * Math.sqrt(clamp01(v));
}

/** Duration implied by a peaks array. */
export function peaksDurationMs(count: number): number {
  return (count / PEAKS_PER_SECOND) * 1000;
}

/** Convert an x position within a waveform of `width` to a time in ms. */
export function xToTime(x: number, width: number, durationMs: number): number {
  if (width <= 0) return 0;
  return clamp01(x / width) * durationMs;
}

/** Linear 0..1 amplitude to a meter position using a dBFS scale (-60 dB .. 0 dB). */
export function levelToMeter(linear: number, floorDb = -60): number {
  if (linear <= 0) return 0;
  const db = 20 * Math.log10(linear);
  return clamp01((db - floorDb) / -floorDb);
}

export function clamp01(v: number): number {
  return v < 0 ? 0 : v > 1 ? 1 : Number.isFinite(v) ? v : 0;
}
