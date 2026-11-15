import { useEffect, useState } from "react";
import type { DeviceLogin, RemoteTranscription, Settings } from "../../lib/ipc/types";
import { useBackend, useStatus } from "../../state/hooks";
import { errorMessage } from "../../lib/ipc/backend";
import { OPENAI_MODELS, TRANSCRIPTION_PRICE_DATE, transcriptionDayCost } from "../../lib/transcriptionCost";
import { Button } from "../../components/ui/Button";
import { Select } from "../../components/ui/controls";
import { Group } from "./parts";
import { isMac } from "../../lib/platform";

const DEFAULT_SERVER: RemoteTranscription = {
  endpoint: "https://tuldmi.spillerstech.us/v1/audio/transcriptions", model: "base.en",
  issuer: "https://auth.spillerstech.us/application/o/tuldmi/", clientId: "tuldmi-desktop", authentication: "authentik",
};
const DEFAULT_OPENAI: RemoteTranscription = {
  endpoint: "https://api.openai.com/v1/audio/transcriptions", model: "gpt-transcribe",
  issuer: "", clientId: "", authentication: "apiKey",
};
export function ServerTranscription({ settings, save }: { settings: Settings; save: (patch: Partial<Settings>) => Promise<boolean> }) {
  const backend = useBackend();
  const status = useStatus();
  const keySupported = backend.kind === "tauri" && !isMac();
  const [draft, setDraft] = useState(settings.remoteTranscription ?? (isMac() ? DEFAULT_SERVER : DEFAULT_OPENAI));
  const [login, setLogin] = useState<DeviceLogin | null>(null);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const [apiKey, setApiKey] = useState("");
  const [hasKey, setHasKey] = useState(false);
  const [speechPercent, setSpeechPercent] = useState(25);
  const cloud = draft.authentication === "apiKey";
  const tracks = Number(settings.micEnabled) + Number(settings.systemEnabled);
  const estimate = transcriptionDayCost(draft.model, 8, tracks, speechPercent);
  const fullDay = transcriptionDayCost(draft.model, 8, tracks, 100);
  const queuedMinutes = (status?.queue?.audioMs ?? 0) / 60_000;
  const queuedCost = queuedMinutes * (OPENAI_MODELS.find((m) => m.value === draft.model)?.perMinute ?? 0);
  useEffect(() => { if (settings.remoteTranscription) setDraft(settings.remoteTranscription); setLogin(null); }, [settings.remoteTranscription]);
  useEffect(() => {
    let stopped = false;
    if (!keySupported) { setHasKey(false); return; }
    void backend.hasTranscriptionApiKey().then((present) => { if (!stopped) setHasKey(present); }).catch(() => {});
    return () => { stopped = true; };
  }, [backend, keySupported]);
  useEffect(() => {
    if (!login) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    const deadline = Date.now() + login.expiresIn * 1000;
    const poll = async () => {
      try {
        if (Date.now() > deadline) throw new Error("Sign-in expired. Try again.");
        const ready = await backend.pollRemoteSignIn();
        if (stopped) return;
        if (ready) { setLogin(null); setMessage("Signed in. Queued recordings will now be sent to your server."); return; }
        timer = setTimeout(() => void poll(), login.interval * 1000);
      } catch (e) { if (!stopped) { setLogin(null); setMessage(errorMessage(e)); } }
    };
    timer = setTimeout(() => void poll(), login.interval * 1000);
    return () => { stopped = true; clearTimeout(timer); };
  }, [login, backend]);
  const action = async (fn: () => Promise<void>) => {
    setBusy(true); setMessage("");
    try { await fn(); } catch (e) { setMessage(errorMessage(e)); } finally { setBusy(false); }
  };
  return (
    <Group title="Offload transcription" footer={cloud ? !keySupported ? "OpenAI API-key uploads require the Windows desktop app in this release. On a Mac, use local Metal processing or your Authentik server." : "The API key is saved in Windows Credential Manager and survives restarts. Transcripts and the durable queue stay local." : "Authentik sign-in stays in memory and refreshes while the app runs. Sign in again after restarting."}>
      <div className="space-y-4 px-4 py-4">
        <p className="text-sm text-muted">Active: <strong>{settings.remoteTranscription ? settings.remoteTranscription.authentication === "apiKey" ? "OpenAI" : "Your server" : "Local processing"}</strong>. Enabling uploads sends existing queued recordings and future speech to the selected service.</p>
        <Select label="Transcription service" value={cloud ? "openai" : "server"} options={[{ value: "openai", label: "OpenAI · API key" }, { value: "server", label: "Your server · Authentik" }]} onChange={(value) => { setDraft(value === "openai" ? DEFAULT_OPENAI : DEFAULT_SERVER); setMessage(""); setLogin(null); }} />
        {cloud ? <>
          <Select label="OpenAI model" value={draft.model} options={OPENAI_MODELS.map((m) => ({ value: m.value, label: m.label }))} onChange={(model) => setDraft({ ...draft, model })} />
          <p className="text-sm text-muted">GPT Transcribe is the current recommended model. Clip timestamps are retained; GPT models do not provide word-level timing here. Older models retire February 26, 2027.</p>
          <label className="block text-sm text-muted">OpenAI API key {hasKey && <span className="text-success">· saved</span>}
            <input type="password" autoComplete="off" spellCheck={false} value={apiKey} disabled={!keySupported} onChange={(e) => setApiKey(e.target.value)} placeholder={!keySupported ? "API-key storage requires Windows" : hasKey ? "Paste to replace the saved key" : "Paste your API key"} className="mt-1 block w-full rounded-md border border-line bg-sunken px-3 py-2 text-text" />
          </label>
          <div className="flex gap-2">
            <Button size="sm" variant="secondary" disabled={busy || !apiKey.trim() || !keySupported} onClick={() => void action(async () => { await backend.setTranscriptionApiKey(apiKey); setApiKey(""); setHasKey(true); setMessage("API key saved securely. Enable OpenAI uploads when ready."); })}>Save API key</Button>
            {hasKey && <Button size="sm" variant="ghost" disabled={busy} onClick={() => void action(async () => { await backend.deleteTranscriptionApiKey(); setHasKey(false); setApiKey(""); setMessage("API key removed. OpenAI uploads will wait for a new key."); })}>Remove saved key</Button>}
          </div>
          <div className="rounded-lg border border-line bg-sunken p-3 text-sm text-muted">
            <label>Speech per enabled track: <input type="number" min={0} max={100} step={5} value={speechPercent} onChange={(e) => setSpeechPercent(Number(e.target.value))} className="mx-2 w-16 rounded border border-line bg-surface px-2 py-1 text-text" />%</label>
            <p className="mt-2">8-hour day · {tracks} enabled {tracks === 1 ? "track" : "tracks"}: <strong className="text-text">{estimate ? `$${estimate.dollars.toFixed(2)}` : "—"}/day</strong>{estimate ? ` for ${estimate.audioMinutes.toFixed(0)} uploaded minutes` : ""}.</p>
            <p className="mt-1">At continuous speech: {fullDay ? `$${fullDay.dollars.toFixed(2)}` : "—"}/day. List prices checked {TRANSCRIPTION_PRICE_DATE}; excludes retries, billing-rounding effects and taxes. Speech filtering reduces uploaded minutes.</p>
            {!!status?.backlog && <p className="mt-1">Existing queue: about {queuedMinutes.toFixed(1)} minutes, or ${queuedCost.toFixed(2)} extra if uploaded once.</p>}
          </div>
          <p className="text-sm text-faint">Claude currently accepts text and images, not audio transcription. It is not an audio provider in this release.</p>
        </> : <div className="grid gap-3">
          {([["endpoint", "Transcription URL"], ["model", "Server model"], ["issuer", "Authentik issuer URL"], ["clientId", "OAuth public client ID"]] as const).map(([key, label]) => (
            <label key={key} className="block text-sm text-muted">{label}<input className="mt-1 block w-full rounded-md border border-line bg-sunken px-3 py-2 text-text" value={draft[key]} onChange={(e) => setDraft({ ...draft, [key]: e.target.value })} spellCheck={false} /></label>
          ))}
        </div>}
        <div className="flex flex-wrap gap-2">
          <Button size="sm" variant="secondary" disabled={busy || (cloud && (!keySupported || !hasKey))} onClick={() => void save({ remoteTranscription: { ...draft } })}>{cloud ? "Enable OpenAI uploads" : "Enable server uploads"}</Button>
          {settings.remoteTranscription?.authentication !== "apiKey" && settings.remoteTranscription && <Button size="sm" variant="secondary" onClick={() => void action(async () => setLogin(await backend.beginRemoteSignIn()))} disabled={busy || !!login || backend.kind === "demo"}>Sign in with Authentik</Button>}
          {settings.remoteTranscription && <Button size="sm" variant="ghost" onClick={() => void save({ remoteTranscription: null })}>Use local processing</Button>}
        </div>
        {login && <p role="status" className="text-sm text-text">Enter <strong className="font-mono select-all">{login.userCode}</strong> in the opened Authentik page. Waiting for approval…</p>}
        {message && <p role="status" className="text-sm text-muted">{message}</p>}
      </div>
    </Group>
  );
}
