import { Check, CircleAlert, Download, Loader2, Trash2, X } from "lucide-react";

import type { ModelInfo, ModelProgress } from "../../lib/ipc/types";
import { errorMessage } from "../../lib/ipc/backend";
import { formatBytes } from "../../lib/platform";
import { useBackend, useModels, useSettings } from "../../state/hooks";
import { Button } from "../../components/ui/Button";
import { Pill, ProgressBar } from "../../components/ui/controls";
import { useConfirm } from "../../components/ui/Confirm";
import { useToast } from "../../components/ui/Toast";

/** Model catalog with explicit download / cancel / delete / activate. */
export function ModelList({ freeDisk, compact }: { freeDisk?: number | null; compact?: boolean }) {
  const { models, progress, refresh, store } = useModels();
  const { settings, patch } = useSettings();
  const backend = useBackend();
  const toast = useToast();
  const confirm = useConfirm();

  if (!models) {
    return (
      <div className="space-y-2" aria-busy>
        {[0, 1].map((i) => (
          <div key={i} className="skeleton h-[86px] rounded-xl" />
        ))}
      </div>
    );
  }
  if (models.length === 0) {
    return <div className="rounded-xl border border-line bg-surface p-4 text-base text-muted">No models are available in this build.</div>;
  }

  const download = async (m: ModelInfo) => {
    if (freeDisk != null && freeDisk < m.sizeBytes * 1.1) {
      toast.show({ level: "error", title: "Not enough disk space", message: `${m.name} needs ${formatBytes(m.sizeBytes)}; ${formatBytes(freeDisk)} is free.` });
      return;
    }
    store.clearProgress(m.id);
    try {
      await backend.downloadModel(m.id);
    } catch (e) {
      toast.show({ level: "error", title: `Couldn't start downloading ${m.name}`, message: errorMessage(e) });
    }
  };
  const cancel = async (m: ModelInfo) => {
    try {
      await backend.cancelModelDownload(m.id);
    } catch (e) {
      toast.show({ level: "error", message: errorMessage(e) });
    }
  };
  const remove = async (m: ModelInfo) => {
    const ok = await confirm({
      title: `Delete ${m.name}?`,
      message: `This frees ${formatBytes(m.sizeBytes)}. Audio keeps being captured; new speech waits in the queue until a model is installed.`,
      confirmLabel: "Delete model",
      danger: true,
    });
    if (!ok) return;
    try {
      await backend.deleteModel(m.id);
      if (settings?.modelId === m.id) await patch({ modelId: null });
      await refresh();
    } catch (e) {
      toast.show({ level: "error", message: errorMessage(e) });
    }
  };
  const activate = async (m: ModelInfo) => {
    try {
      await patch({ modelId: m.id });
    } catch (e) {
      toast.show({ level: "error", message: errorMessage(e) });
    }
  };

  return (
    <>
    <ul className="space-y-2.5">
      {models.map((m) => (
        <ModelCard
          key={m.id}
          model={m}
          progress={progress[m.id]}
          active={settings?.modelId === m.id && m.installed}
          compact={compact}
          onDownload={() => void download(m)}
          onCancel={() => void cancel(m)}
          onDelete={() => void remove(m)}
          onActivate={() => void activate(m)}
          onDismiss={() => store.clearProgress(m.id)}
        />
      ))}
    </ul>
    {backend.kind === "demo" && (
      <p className="mt-2 px-0.5 text-xs text-faint">Demo mode simulates downloads and progress — nothing is fetched or installed.</p>
    )}
    </>
  );
}

function ModelCard({
  model: m,
  progress: p,
  active,
  compact,
  onDownload,
  onCancel,
  onDelete,
  onActivate,
  onDismiss,
}: {
  model: ModelInfo;
  progress?: ModelProgress;
  active: boolean;
  compact?: boolean;
  onDownload: () => void;
  onCancel: () => void;
  onDelete: () => void;
  onActivate: () => void;
  onDismiss: () => void;
}) {
  const downloading = p && (p.state === "downloading" || p.state === "verifying");
  const failed = p?.state === "failed";
  const frac = p && p.total ? p.downloaded / p.total : 0;

  return (
    <li className={`rounded-xl border bg-surface px-4 py-3.5 shadow-1 transition-colors ${active ? "border-accent-line" : "border-line"}`}>
      <div className="flex items-start gap-4">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-base font-semibold text-text">{m.name}</span>
            {m.recommended && <Pill tone="accent">Recommended</Pill>}
            {active && (
              <Pill tone="success">
                <Check size={11} /> In use
              </Pill>
            )}
            {m.installed && !active && <Pill>Installed</Pill>}
          </div>
          {!compact && <p className="mt-1 text-sm text-muted [text-wrap:pretty]">{m.description}</p>}
          <div className="mt-1.5 flex flex-wrap gap-x-3 gap-y-0.5 text-xs text-faint">
            <span className="tnum">{formatBytes(m.sizeBytes)}</span>
            <span>{m.languages}</span>
            <span>License: {m.license}</span>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1.5 pt-0.5">
          {downloading ? (
            <Button size="sm" variant="secondary" icon={<X size={13} />} onClick={onCancel}>
              Cancel
            </Button>
          ) : m.installed ? (
            <>
              {!active && (
                <Button size="sm" variant="secondary" onClick={onActivate}>
                  Use this model
                </Button>
              )}
              <Button size="sm" variant="ghost" aria-label={`Delete ${m.name}`} icon={<Trash2 size={13} />} onClick={onDelete}>
                {compact ? "" : "Delete"}
              </Button>
            </>
          ) : (
            <Button size="sm" variant={m.recommended ? "primary" : "secondary"} icon={<Download size={13} />} onClick={onDownload}>
              {failed ? "Try again" : "Download"}
            </Button>
          )}
        </div>
      </div>
      {downloading && (
        <div className="mt-3">
          <ProgressBar value={p!.state === "verifying" ? 1 : frac} label={`Downloading ${m.name}`} />
          <div className="mt-1.5 flex justify-between text-xs text-faint tnum">
            <span className="flex items-center gap-1.5">
              <Loader2 size={11} className="animate-spin motion-reduce:animate-none" aria-hidden />
              {p!.state === "verifying" ? "Verifying checksum…" : `Downloading · ${Math.round(frac * 100)}%`}
            </span>
            <span>
              {formatBytes(p!.downloaded)} of {formatBytes(p!.total || m.sizeBytes)}
            </span>
          </div>
        </div>
      )}
      {failed && (
        <div className="mt-3 flex items-start gap-2 rounded-md bg-danger-soft px-3 py-2 text-sm text-text">
          <CircleAlert size={14} className="mt-[2px] shrink-0 text-danger" aria-hidden />
          <span className="flex-1">{p?.error ?? "The download failed."} Your connection may have dropped — nothing partial is kept.</span>
          <button type="button" onClick={onDismiss} className="text-faint hover:text-text" aria-label="Dismiss">
            <X size={13} />
          </button>
        </div>
      )}
      {p?.state === "cancelled" && !m.installed && (
        <div className="mt-2 text-xs text-faint">Download cancelled.</div>
      )}
    </li>
  );
}
