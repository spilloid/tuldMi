import { useRef, useState } from "react";
import type { FormEvent, SyntheticEvent } from "react";
import type { Speaker } from "../../lib/ipc/types";
import { errorMessage } from "../../lib/ipc/backend";
import { Dialog } from "../../components/ui/Dialog";
import { Button } from "../../components/ui/Button";
import { useToast } from "../../components/ui/Toast";
import { useBackend } from "../../state/hooks";

/** Hues for telling separated speakers apart at a glance (a dot next to the name). */
const HUES = [168, 250, 30, 310, 95, 205];

function hueFor(id: string): number {
  let h = 0;
  for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) >>> 0;
  return HUES[h % HUES.length];
}

/**
 * A speaker's name in the timeline. Separated remote speakers ("person") can be renamed:
 * clicking the name opens a small dialog, and the name applies to that speaker everywhere in
 * the conversation. Me and Remote are sources, not people, so they are plain text.
 */
export function SpeakerName({ speaker, label, className = "" }: { speaker: Speaker | null; label: string; className?: string }) {
  const [open, setOpen] = useState(false);
  if (!speaker || speaker.kind !== "person") return <span className={className}>{label}</span>;
  return (
    <>
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          setOpen(true);
        }}
        title={`Rename ${label}`}
        className={`inline-flex items-center gap-1.5 rounded-sm decoration-dotted underline-offset-2 hover:underline focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-line ${className}`}
      >
        <span className="h-1.5 w-1.5 shrink-0 rounded-full" style={{ background: `oklch(0.64 0.1 ${hueFor(speaker.id)})` }} aria-hidden />
        {label}
      </button>
      {open && <RenameDialog speaker={speaker} label={label} onClose={() => setOpen(false)} />}
    </>
  );
}

function RenameDialog({ speaker, label, onClose }: { speaker: Speaker; label: string; onClose: () => void }) {
  const backend = useBackend();
  const toast = useToast();
  const input = useRef<HTMLInputElement>(null);
  const [value, setValue] = useState(label);
  const [saving, setSaving] = useState(false);
  const name = value.trim();
  const valid = name.length > 0 && name.length <= 80;

  const save = async (e: FormEvent) => {
    e.preventDefault();
    if (!valid || saving) return;
    if (name === label) return onClose();
    setSaving(true);
    try {
      await backend.renameSpeaker(speaker.id, name);
      onClose();
    } catch (err) {
      toast.show({ level: "error", title: "Couldn't rename speaker", message: errorMessage(err) });
      setSaving(false);
    }
  };
  // The dialog renders through a portal, but React still bubbles its events to the timeline
  // row it was opened from; keep clicks and keys (Enter!) from toggling that row.
  const stop = (e: SyntheticEvent) => e.stopPropagation();

  return (
    <Dialog
      open
      onOpenChange={(v) => !v && onClose()}
      title="Name this speaker"
      description="The name replaces this label everywhere in the conversation: timeline, search, copied text and AI assistants."
      width={420}
      initialFocus={() => {
        input.current?.select();
        return input.current;
      }}
    >
      <form onSubmit={save} onClick={stop} onKeyDown={stop} onContextMenu={stop} className="mt-4 flex flex-col gap-4">
        <input
          ref={input}
          value={value}
          onChange={(e) => setValue(e.target.value)}
          maxLength={80}
          aria-label="Speaker name"
          className="h-9 rounded-md border border-line bg-surface px-3 text-base text-text outline-none focus:border-accent-line"
        />
        <div className="flex justify-end gap-2">
          <Button type="button" variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" disabled={!valid || saving}>
            Save name
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
