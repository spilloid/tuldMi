/** List prices checked 2026-11-14. Estimates, not an account balance or invoice. */
export const TRANSCRIPTION_PRICE_DATE = "2026-11-14";
export const OPENAI_MODELS = [
  { value: "gpt-transcribe", label: "GPT Transcribe", perMinute: 0.0045, legacy: false },
  { value: "gpt-4o-mini-transcribe", label: "GPT-4o Mini Transcribe · retiring Feb 26, 2027", perMinute: 0.003, legacy: true },
  { value: "gpt-4o-transcribe", label: "GPT-4o Transcribe · retiring Feb 26, 2027", perMinute: 0.006, legacy: true },
  { value: "whisper-1", label: "Whisper · retiring Feb 26, 2027", perMinute: 0.006, legacy: true },
] as const;
export function transcriptionDayCost(model: string, hours: number, tracks: number, speechPercent: number) {
  const price = OPENAI_MODELS.find((m) => m.value === model);
  if (!price || ![hours, tracks, speechPercent].every(Number.isFinite) || hours < 0 || tracks < 0 || speechPercent < 0 || speechPercent > 100) return null;
  const audioMinutes = hours * 60 * tracks * speechPercent / 100;
  return { audioMinutes, dollars: audioMinutes * price.perMinute };
}
