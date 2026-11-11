import { Download, Loader2, Users, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import type { ModelInfo, SeparationState } from "../../lib/ipc/types";
import { errorMessage } from "../../lib/ipc/backend";
import { formatBytes } from "../../lib/platform";
import { useBackend, useModels, useSettings, useStatus } from "../../state/hooks";
import { Button } from "../../components/ui/Button";
import { Pill, ProgressBar, Switch } from "../../components/ui/controls";
import { useToast } from "../../components/ui/Toast";
import { Group, Row } from "./parts";

const STATE: Record<SeparationState, { tone: "neutral" | "success" | "warning" | "danger"; text: string }> = {
  off: { tone: "neutral", text: "Off" },
  needsModels: { tone: "warning", text: "Models needed" },
  unavailable: { tone: "danger", text: "Unavailable" },
  ready: { tone: "success", text: "Ready" },
  working: { tone: "success", text: "Separating…" },
};

/**
 * Settings › Transcription › Speakers (docs/design/speakers.md): separate the voices on the
 * other side of calls. Off by default; needs two models, downloaded only when the user asks.
 */
export function SpeakersGroup() {
  const backend = useBackend();
  const toast = useToast();
  const status = useStatus();
  const { settings, patch } = useSettings();
  const { progress, store } = useModels();
  const [models, setModels] = useState<ModelInfo[] | null>(null);
  const refresh = useCallback(() => backend.speakerModels().then(setModels, () => setModels([])), [backend]);
  useEffect(() => void refresh(), [refresh]);
  // A finished download changes what's installed.
  const finished = models?.filter((m) => progress?.[m.id]?.state === "done").map((m) => m.id).join(",");
  useEffect(() => {
    if (finished) void refresh();
  }, [finished, refresh]);

  if (!settings || !models) return null;
  if (models.length === 0) return null;
  const installed = models.every((m) => m.installed);
  const missing = models.filter((m) => !m.installed);
  const active = missing.map((m) => progress?.[m.id]).filter((p) => p && (p.state === "downloading" || p.state === "verifying"));
  const failed = missing.map((m) => progress?.[m.id]).find((p) => p?.state === "failed");
  const total = missing.reduce((n, m) => n + m.sizeBytes, 0);
  const done = active.reduce((n, p) => n + (p?.downloaded ?? 0), 0);
  const separation = status?.separation;
  const state = STATE[separation?.state ?? "off"];
  const backfilling = separation?.backfillRemaining != null;

  const download = async () => {
    for (const m of missing) {
      store.clearProgress(m.id);
      try {
        await backend.downloadModel(m.id);
      } catch (e) {
        toast.show({ level: "error", title: `Couldn't start downloading ${m.name}`, message: errorMessage(e) });
        return;
      }
    }
  };
  const cancel = () => missing.forEach((m) => void backend.cancelModelDownload(m.id).catch(() => {}));
  const setEnabled = async (on: boolean) => {
    try {
      await patch({ speakersEnabled: on });
    } catch (e) {
      toast.show({ level: "error", title: "Couldn't save setting", message: errorMessage(e) });
    }
  };
  const separatePast = async () => {
    try {
      const n = await backend.separatePastRecordings();
      toast.show({
        level: n ? "info" : "success",
        message: n ? `Separating speakers in ${n} past ${n === 1 ? "recording" : "recordings"}. New speech is transcribed first.` : "No past recordings need their speakers separated.",
        key: "separate-past",
      });
    } catch (e) {
      toast.show({ level: "error", title: "Couldn't separate past recordings", message: errorMessage(e) });
    }
  };

  return (
    <Group
      title="Speakers"
      footer={
        <>
          Speaker separation runs on this computer. Voice fingerprints exist only in memory, for the current conversation, and are never saved. Names you give speakers are stored with your transcripts. Model licenses:{" "}
          {models.map((m, i) => (
            <span key={m.id}>
              {i > 0 && "; "}
              {m.license}
            </span>
          ))}
          .
        </>
      }
    >
      <Row
        label="Separate remote speakers"
        htmlFor="speakers-enabled"
        description="Labels the other side of calls by voice (Speaker 1, Speaker 2…) within each conversation. Click a name in the timeline to rename that speaker. Me is never split: it's your microphone."
      >
        <div className="flex items-center gap-2.5">
          {settings.speakersEnabled && <Pill tone={state.tone}>{state.text}</Pill>}
          <Switch id="speakers-enabled" label="Separate remote speakers" checked={settings.speakersEnabled} disabled={!installed && !settings.speakersEnabled} onCheckedChange={(v) => void setEnabled(v)} />
        </div>
      </Row>
      {!installed && (
        <Row
          label="Speaker models"
          align="start"
          description={
            active.length ? (
              <span className="mt-1.5 block max-w-[360px]">
                <ProgressBar value={total ? done / total : 0} label="Downloading speaker models" />
                <span className="mt-1 block text-xs tnum">
                  {formatBytes(done)} of {formatBytes(total)}
                </span>
              </span>
            ) : failed ? (
              <span className="text-danger">Download failed: {failed.error ?? "try again"}</span>
            ) : (
              `Two models, ${formatBytes(total)}, downloaded once from Hugging Face and checked against pinned hashes.`
            )
          }
        >
          {active.length ? (
            <Button size="sm" variant="ghost" icon={<X size={13} />} onClick={cancel}>
              Cancel
            </Button>
          ) : (
            <Button size="sm" variant="secondary" icon={<Download size={13} />} onClick={() => void download()}>
              Download ({formatBytes(total)})
            </Button>
          )}
        </Row>
      )}
      {separation?.state === "unavailable" && settings.speakersEnabled && (
        <Row label="Not available" description="The speaker worker is missing from this install, or it kept failing this session. Transcripts are unaffected; restart tuldMi to try again." />
      )}
      <Row
        label="Separate past recordings"
        description="Conversations whose audio is still kept. Their transcripts keep their wording; each passage goes to the voice that says most of it."
      >
        {backfilling ? (
          <span className="inline-flex items-center gap-2 text-sm text-muted tnum">
            <Loader2 size={13} className="animate-spin motion-reduce:animate-none" aria-hidden />
            {separation?.backfillRemaining} left
          </span>
        ) : (
          <Button size="sm" variant="secondary" icon={<Users size={13} />} disabled={!settings.speakersEnabled || !installed} onClick={() => void separatePast()}>
            Separate
          </Button>
        )}
      </Row>
    </Group>
  );
}
