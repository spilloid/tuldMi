import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { Button } from "../../components/ui/Button";
import { useConfirm } from "../../components/ui/Confirm";
import { useDebouncedValue } from "../../lib/hooks/useDebounce";
import { errorMessage } from "../../lib/ipc/backend";
import { navigate } from "../../lib/router";
import { useBackend, useSettings } from "../../state/hooks";

interface Clip {
  id: string;
  started: number;
  device: string;
  source: "mic" | "system";
  transcript: string;
}
interface Page { clips: Clip[]; more: boolean }

function ClipPlayer({ id }: { id: string }) {
  const [url, setUrl] = useState("");
  const [error, setError] = useState("");
  useEffect(() => {
    let active = true;
    let objectUrl = "";
    invoke<ArrayBuffer>("library_audio", { id }).then((bytes) => {
      if (active) {
        objectUrl = URL.createObjectURL(new Blob([bytes], { type: "audio/wav" }));
        setUrl(objectUrl);
      }
    }).catch((e) => { if (active) setError(errorMessage(e)); });
    return () => { active = false; if (objectUrl) URL.revokeObjectURL(objectUrl); };
  }, [id]);
  if (error) return <p role="alert">{error}</p>;
  return url ? <audio className="mt-3 w-full" controls autoPlay src={url} aria-label="Shared recording"
    onError={() => setError("This recording could not be played.")} /> : <p role="status">Loading audio...</p>;
}

function LibraryList({ query }: { query: string }) {
  const [clips, setClips] = useState<Clip[]>([]);
  const [more, setMore] = useState(false);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState("");
  const [playing, setPlaying] = useState<string | null>(null);
  const confirm = useConfirm();
  const generation = useRef(0);

  async function load(older: boolean) {
    const current = ++generation.current;
    const last = older ? clips.at(-1) : undefined;
    setBusy(true);
    setError("");
    try {
      const page = await invoke<Page>("library_clips", { query, before: last?.started, beforeId: last?.id });
      if (generation.current !== current) return;
      setClips((prev) => older ? [...prev, ...page.clips] : page.clips);
      setMore(page.more);
    } catch (e) {
      if (generation.current === current) setError(errorMessage(e));
    } finally {
      if (generation.current === current) setBusy(false);
    }
  }

  useEffect(() => {
    void load(false);
    return () => { ++generation.current; };
    // The parent remounts this list for a new search or account configuration.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function remove(clip: Clip) {
    if (!await confirm({ title: "Delete shared recording?", message: "This removes this clip and transcript from the shared timeline on every laptop.", confirmLabel: "Delete shared copy", danger: true })) return;
    try {
      await invoke("library_delete", { id: clip.id });
      if (playing === clip.id) setPlaying(null);
      await load(false);
    } catch (e) { setError(errorMessage(e)); }
  }

  return <>
    <div className="mb-3 flex items-center justify-between">
      <span className="text-sm text-muted" role="status">{busy ? "Loading recordings..." : ""}</span>
      <Button variant="ghost" size="sm" disabled={busy} onClick={() => void load(false)}>Refresh</Button>
    </div>
    {error && <p role="alert" className="mb-3 text-sm">{error}</p>}
    {!busy && !error && !clips.length && <p className="text-muted">{query ? "No matching transcripts." : "Recordings shared by your laptops will appear here."}</p>}
    {clips.map((clip, index) => {
      const date = new Date(clip.started);
      const newDay = !index || new Date(clips[index - 1].started).toDateString() !== date.toDateString();
      return <div key={clip.id}>
        {newDay && <h2 className="mt-5 mb-2 font-display text-xl">{date.toLocaleDateString(undefined, { dateStyle: "long" })}</h2>}
        <article className="border-t border-line-soft py-3">
          <div className="flex flex-wrap items-center gap-2 text-sm text-muted">
            <time dateTime={date.toISOString()}>{date.toLocaleTimeString()}</time>
            <span>{clip.device}</span><span>{clip.source === "mic" ? "Microphone" : "System audio"}</span>
          </div>
          <p className="my-2 whitespace-pre-wrap">{clip.transcript || "No speech transcribed."}</p>
          <Button variant="secondary" size="sm" onClick={() => setPlaying(playing === clip.id ? null : clip.id)}>{playing === clip.id ? "Close player" : "Play"}</Button>
          <Button variant="ghost" size="sm" onClick={() => void remove(clip)}>Delete shared copy</Button>
          {playing === clip.id && <ClipPlayer id={clip.id} />}
        </article>
      </div>;
    })}
    {more && <Button variant="secondary" disabled={busy} onClick={() => void load(true)}>Older recordings</Button>}
  </>;
}

export function SharedTimeline() {
  const backend = useBackend();
  const { settings } = useSettings();
  const [search, setSearch] = useState("");
  const query = useDebouncedValue(search.trim(), 250);
  const config = settings?.remoteTranscription;
  return <section className="min-h-0 flex-1 overflow-y-auto px-6 py-6 text-text">
    <div className="mx-auto max-w-3xl">
      <h1 className="mb-5 font-display text-3xl">Shared timeline</h1>
      {backend.kind === "tauri" && config && config.authentication !== "apiKey" ? <>
        <input type="search" value={search} maxLength={200} aria-label="Search shared transcripts" placeholder="Search shared transcripts"
          onChange={(e) => setSearch(e.target.value)} className="w-full rounded-md border border-line bg-sunken px-3 py-2" />
        <LibraryList key={JSON.stringify([config.endpoint, config.issuer, config.clientId, query])} query={query} />
      </> : <>
        <p className="mb-3 text-muted">Choose your Authentik server in the desktop app to see recordings from your laptops.</p>
        <Button variant="secondary" onClick={() => navigate("/settings/transcription")}>Transcription settings</Button>
      </>}
    </div>
  </section>;
}
