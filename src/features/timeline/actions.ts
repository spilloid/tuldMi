import { useCallback } from "react";
import { errorMessage } from "../../lib/ipc/backend";
import type { SpeechEvent } from "../../lib/ipc/types";
import { eventText, transcriptText } from "../../lib/time";
import { useBackend, useStore } from "../../state/hooks";
import { useConfirm } from "../../components/ui/Confirm";
import { useToast } from "../../components/ui/Toast";

/** One event as "[time] Speaker: text" lines (one per speaker run). */
export function textWithTimestamp(ev: SpeechEvent): string {
  return transcriptText([ev]);
}

export function useEventActions() {
  const backend = useBackend();
  const store = useStore();
  const toast = useToast();
  const confirm = useConfirm();

  const copy = useCallback(
    async (text: string, what: string) => {
      try {
        await navigator.clipboard.writeText(text);
        toast.show({ level: "success", message: `${what} copied`, duration: 2200, key: "copy" });
      } catch {
        toast.show({ level: "error", message: "Couldn't access the clipboard." });
      }
    },
    [toast],
  );

  return {
    copyText: (ev: SpeechEvent) => copy(eventText(ev), "Text"),
    copyWithTimestamp: (ev: SpeechEvent) => copy(textWithTimestamp(ev), "Text with timestamp"),
    /** Several events (a range or a whole conversation) as timestamped transcript lines. */
    copyTranscript: (events: SpeechEvent[], what: string) => copy(transcriptText(events), what),
    transcribeAnyway: async (ev: SpeechEvent) => {
      try {
        const ok = await backend.transcribeAnyway(ev.id);
        if (!ok) toast.show({ level: "warning", message: "This recording's audio is gone, so it can't be transcribed." });
      } catch (e) {
        toast.show({ level: "error", title: "Couldn't transcribe", message: errorMessage(e) });
      }
    },
    retry: async (ev: SpeechEvent) => {
      try {
        const ok = await backend.retryEvent(ev.id);
        if (!ok) toast.show({ level: "warning", message: "This event can't be retried right now." });
      } catch (e) {
        toast.show({ level: "error", title: "Retry failed", message: errorMessage(e) });
      }
    },
    deleteAudio: async (ev: SpeechEvent) => {
      const ok = await confirm({
        title: "Delete this recording?",
        message: "The audio file is removed permanently. The transcript stays in your timeline and search.",
        confirmLabel: "Delete audio",
        danger: true,
      });
      if (!ok) return;
      try {
        await backend.deleteEventAudio(ev.id);
      } catch (e) {
        toast.show({ level: "error", title: "Couldn't delete audio", message: errorMessage(e) });
      }
    },
    remove: async (ev: SpeechEvent) => {
      const ok = await confirm({
        title: "Delete this event?",
        message: "Its transcript and any retained audio are removed permanently. This can't be undone.",
        confirmLabel: "Delete event",
        danger: true,
      });
      if (!ok) return;
      try {
        await backend.deleteEvent(ev.id);
        store.removeLocal(ev.id);
      } catch (e) {
        toast.show({ level: "error", title: "Couldn't delete event", message: errorMessage(e) });
      }
    },
  };
}
