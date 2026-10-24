import { AlertTriangle, Mic, Monitor } from "lucide-react";
import type { DeviceInfo, Settings, Source, SourceStatus } from "../../lib/ipc/types";
import { useLevels } from "../../state/hooks";
import { LiveMeter } from "../../components/viz/LiveMeter";
import { Select, Switch } from "../../components/ui/controls";

const DEFAULT = "__default__";

/** Toggle + device picker + live meter for one capture source. */
export function SourceCard({
  source,
  settings,
  devices,
  status,
  onChange,
  metersExpected,
  demo,
}: {
  source: Source;
  settings: Settings;
  devices: DeviceInfo[] | null;
  status?: SourceStatus;
  onChange: (patch: Partial<Settings>) => void;
  /** Whether levels should be flowing (recording or monitoring). */
  metersExpected: boolean;
  demo: boolean;
}) {
  const { summary } = useLevels();
  const me = source === "mic";
  const enabled = me ? settings.micEnabled : settings.systemEnabled;
  const deviceId = me ? settings.micDeviceId : settings.systemDeviceId;
  const kind = me ? "input" : "output";
  const list = (devices ?? []).filter((d) => d.kind === kind);
  const def = list.find((d) => d.isDefault);
  const options = [
    { value: DEFAULT, label: `System default${def ? ` — ${def.name}` : ""}`, hint: "Follows Windows when you switch devices" },
    ...list.map((d) => ({ value: d.id, label: d.name })),
  ];
  const sum = summary[source];
  const problem = status && enabled && (status.state === "error" || status.state === "unavailable");

  let meterText: string;
  if (!enabled) meterText = "Off — nothing from this source is captured";
  else if (problem) meterText = status?.error ?? "Device unavailable — tuldMi keeps retrying";
  else if (demo) meterText = "No live audio in demo mode";
  else if (!metersExpected) meterText = "Levels appear while recording or testing";
  else if (!sum.live) meterText = "Waiting for audio…";
  else meterText = sum.speaking ? "Speech detected" : me ? "Listening — say something" : "Listening — play something with speech";

  return (
    <div className={`rounded-xl border bg-surface shadow-1 transition-colors ${enabled ? "border-line" : "border-line-soft"}`}>
      <div className="flex items-center gap-3 px-4 pb-3 pt-3.5">
        <span className={`inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-lg ${me ? "bg-me-soft text-me-text" : "bg-remote-soft text-remote-text"}`}>
          {me ? <Mic size={17} /> : <Monitor size={17} />}
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 text-base font-semibold text-text">
            {me ? "Microphone" : "System audio"}
            <span className={`text-sm font-medium ${me ? "text-me-text" : "text-remote-text"}`}>→ {me ? "Me" : "Remote"}</span>
          </div>
          <div className="text-sm text-muted">
            {me ? "Your side of conversations." : "Everything played through your speakers or headphones — calls, meetings, videos."}
          </div>
        </div>
        <Switch
          checked={enabled}
          label={me ? "Capture microphone" : "Capture system audio"}
          onCheckedChange={(v) => onChange(me ? { micEnabled: v } : { systemEnabled: v })}
        />
      </div>
      <div className={`border-t border-line-soft px-4 py-3 ${enabled ? "" : "opacity-50"}`}>
        <div className="flex items-center gap-3">
          <span className="w-[54px] shrink-0 text-sm text-muted">Device</span>
          <Select
            label={me ? "Microphone device" : "Output device to capture"}
            value={deviceId ?? DEFAULT}
            options={options}
            disabled={!enabled || !devices}
            onChange={(v) => onChange(me ? { micDeviceId: v === DEFAULT ? null : v } : { systemDeviceId: v === DEFAULT ? null : v })}
            className="min-w-0 flex-1"
          />
        </div>
        <div className="mt-3 flex items-center gap-3">
          <span className="w-[54px] shrink-0 text-sm text-muted">Level</span>
          <div className="flex min-w-0 flex-1 items-center gap-3 rounded-lg border border-line-soft bg-sunken px-3 py-2">
            <LiveMeter source={source} variant="large" className="h-8 min-w-0 flex-1" label={me ? "Microphone" : "System audio"} />
            <span className={`flex w-[210px] shrink-0 items-center gap-1.5 text-xs ${problem ? "text-warning" : "text-faint"}`}>
              {problem && <AlertTriangle size={12} aria-hidden />}
              <span className="line-clamp-2">{meterText}</span>
            </span>
          </div>
        </div>
        {status?.format && enabled && !problem && <div className="mt-2 pl-[66px] text-xs text-faint">{status.deviceName} · {status.format}</div>}
      </div>
    </div>
  );
}
