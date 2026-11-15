import { useStatus } from "../../state/hooks";
import { queueMessage } from "../../lib/queue";
import { humanDuration } from "../../lib/time";
import { navigate } from "../../lib/router";
import { Button } from "../../components/ui/Button";

export function QueueNotice() {
  const status = useStatus();
  if (!status?.backlog) return null;
  const message = queueMessage(status);
  const q = status.queue;
  return (
    <div className="flex flex-wrap items-center gap-3 border-b border-line bg-surface px-5 py-3 text-sm">
      <div className="min-w-0 flex-1">
        <p className={`font-medium ${q?.fallingBehind || status.transcriber === "error" ? "text-warning" : "text-text"}`}>{message.title} · {status.backlog} clips{q ? ` · ${humanDuration(q.audioMs)} of speech` : ""}</p>
        <p className="mt-1 text-muted">{message.detail} You can play saved audio while transcripts wait.</p>
      </div>
      {q?.oldestEventId && <Button size="sm" variant="secondary" onClick={() => navigate(`/timeline/${q.oldestEventId}`)}>Review oldest</Button>}
      <Button size="sm" variant="ghost" onClick={() => navigate("/settings/transcription")}>Processing settings</Button>
    </div>
  );
}
