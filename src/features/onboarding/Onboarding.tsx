import { AnimatePresence, motion } from "motion/react";
import { ArrowLeft, ArrowRight, Check, HardDrive, Lock, Mic, Monitor, Pause, ShieldCheck, WifiOff } from "lucide-react";
import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import type { DeviceInfo, Retention, Settings } from "../../lib/ipc/types";
import { errorMessage } from "../../lib/ipc/backend";
import { navigate } from "../../lib/router";
import { transition, useReducedMotion } from "../../lib/motion";
import { useBackend, useModels, useSettings, useStatus } from "../../state/hooks";
import { Titlebar } from "../../components/shell/Titlebar";
import { Logo } from "../../components/shell/Logo";
import { Button } from "../../components/ui/Button";
import { Keys } from "../../components/ui/Kbd";
import { useToast } from "../../components/ui/Toast";
import { SourceCard } from "../shared/SourceCard";
import { ModelList } from "../shared/ModelList";
import { MonitoringNote } from "../shared/MonitoringNote";
import { consentPatch, isAcknowledged } from "./consent";

const STEPS = ["Welcome", "What's captured", "Privacy & storage", "Consent", "Devices", "Transcription", "Ready"] as const;

export function Onboarding({ step: initialStep }: { step: number }) {
  const backend = useBackend();
  const { settings, patch } = useSettings();
  const toast = useToast();
  const reduced = useReducedMotion();
  const [step, setStep] = useState(Math.min(initialStep, STEPS.length - 1));
  const [dir, setDir] = useState(1);

  useEffect(() => setStep(Math.min(initialStep, STEPS.length - 1)), [initialStep]);

  const ack = settings ? isAcknowledged(settings) : false;

  const go = (n: number) => {
    const next = Math.max(0, Math.min(STEPS.length - 1, n));
    setDir(next >= step ? 1 : -1);
    setStep(next);
    navigate(`/onboarding/${next}`, { replace: true });
  };

  const save = async (p: Partial<Settings>) => {
    try {
      await patch(p);
    } catch (e) {
      toast.show({ level: "error", title: "Couldn't save", message: errorMessage(e) });
    }
  };

  const finish = async (startRecording: boolean) => {
    if (!ack) {
      go(3);
      toast.show({ level: "warning", message: "Please confirm the consent acknowledgement first.", key: "ack" });
      return;
    }
    try {
      await patch({ onboardingComplete: true, consentAcknowledgedAt: settings!.consentAcknowledgedAt, captureOnLaunch: startRecording ? true : settings!.captureOnLaunch });
      await backend.setPaused(!startRecording);
      navigate("/timeline", { replace: true });
    } catch (e) {
      toast.show({ level: "error", title: "Couldn't finish setup", message: errorMessage(e) });
    }
  };

  if (!settings) return null;
  const canContinue = step !== 3 || ack;

  return (
    <div className="flex h-full flex-col bg-canvas">
      <Titlebar demo={backend.kind === "demo"} title="Setting up tuldMi" />
      <div className="flex min-h-0 flex-1">
        {/* Step rail */}
        <aside className="flex w-[248px] shrink-0 flex-col border-r border-line-soft px-6 py-8 max-[1100px]:hidden">
          <ol className="space-y-0.5" aria-label="Setup steps">
            {STEPS.map((label, i) => {
              const done = i < step;
              const cur = i === step;
              return (
                <li key={label}>
                  <button
                    type="button"
                    disabled={i > step && !(i <= 3 || ack)}
                    onClick={() => i <= step && go(i)}
                    aria-current={cur ? "step" : undefined}
                    className={`flex h-8 w-full items-center gap-3 rounded-md px-2 text-left text-base transition-colors ${cur ? "text-text" : done ? "text-muted hover:text-text" : "text-faint"}`}
                  >
                    <span
                      className={`inline-flex h-[18px] w-[18px] shrink-0 items-center justify-center rounded-full border text-[10px] font-semibold transition-colors duration-[220ms] ${
                        cur ? "border-accent bg-accent text-on-accent" : done ? "border-accent-line bg-accent-soft text-accent-text" : "border-line-strong text-faint"
                      }`}
                    >
                      {done ? <Check size={10} strokeWidth={3} /> : i + 1}
                    </span>
                    <span className={cur ? "font-semibold" : ""}>{label}</span>
                  </button>
                </li>
              );
            })}
          </ol>
          <div className="mt-auto text-sm text-faint">
            <div className="mb-1 flex items-center gap-1.5 text-muted">
              <WifiOff size={13} aria-hidden /> Works offline
            </div>
            No account, no sign-in, no telemetry.
          </div>
        </aside>

        {/* Step body */}
        <main className="relative flex min-w-0 flex-1 flex-col">
          <div className="h-[3px] bg-line-soft min-[1101px]:hidden" aria-hidden>
            <div className="h-full bg-accent transition-[width] duration-[320ms] ease-out" style={{ width: `${((step + 1) / STEPS.length) * 100}%` }} />
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto">
            <AnimatePresence mode="wait" initial={false} custom={dir}>
              <motion.div
                key={step}
                initial={reduced ? { opacity: 0 } : { opacity: 0, x: 18 * dir }}
                animate={{ opacity: 1, x: 0 }}
                exit={reduced ? { opacity: 0 } : { opacity: 0, x: -12 * dir, transition: { duration: 0.14 } }}
                transition={transition.view}
                className="mx-auto w-full max-w-[600px] px-8 pb-10 pt-[min(9vh,72px)]"
              >
                {step === 0 && <Welcome />}
                {step === 1 && <Captured />}
                {step === 2 && <PrivacyStep retention={settings.retention} onRetention={(r) => void save({ retention: r })} />}
                {step === 3 && <Consent ack={ack} onAck={(v) => void save(consentPatch(v))} />}
                {step === 4 && <Devices settings={settings} onChange={(p) => void save(p)} />}
                {step === 5 && <ModelStep onSkip={() => go(6)} />}
                {step === 6 && <Ready settings={settings} />}
              </motion.div>
            </AnimatePresence>
          </div>
          <footer className="flex h-16 shrink-0 items-center gap-3 border-t border-line-soft px-8">
            <span className="text-sm text-faint tnum">
              Step {step + 1} of {STEPS.length}
            </span>
            <div className="flex-1" />
            {step > 0 && (
              <Button variant="ghost" icon={<ArrowLeft size={14} />} onClick={() => go(step - 1)}>
                Back
              </Button>
            )}
            {step < STEPS.length - 1 ? (
              <Button variant="primary" size="lg" disabled={!canContinue} trailing={<ArrowRight size={14} />} onClick={() => go(step + 1)}>
                {step === 0 ? "Get started" : step === 5 ? "Continue" : "Continue"}
              </Button>
            ) : (
              <>
                <Button variant="secondary" size="lg" onClick={() => void finish(false)}>
                  Open paused
                </Button>
                <Button variant="primary" size="lg" onClick={() => void finish(true)}>
                  Start listening
                </Button>
              </>
            )}
          </footer>
        </main>
      </div>
    </div>
  );
}

function StepTitle({ eyebrow, title, children }: { eyebrow?: string; title: string; children?: ReactNode }) {
  return (
    <div className="mb-7">
      {eyebrow && <div className="caps mb-3 text-accent-text">{eyebrow}</div>}
      <h1 className="font-display text-[34px] font-[440] leading-[1.12] tracking-[-0.022em] text-text [text-wrap:balance]">{title}</h1>
      {children && <div className="mt-3 text-md leading-[25px] text-muted [text-wrap:pretty]">{children}</div>}
    </div>
  );
}

function Welcome() {
  return (
    <>
      <div className="mb-9">
        <Logo size={44} />
      </div>
      <StepTitle eyebrow="Welcome" title="Your conversations, remembered.">
        tuldMi quietly listens while you work and turns what's said into a searchable, private timeline — so the details from a call
        last week are a few keystrokes away.
      </StepTitle>
      <ul className="space-y-3">
        <Feature icon={<Lock size={15} />} title="Private by design">Everything is recorded, transcribed and stored on this computer.</Feature>
        <Feature icon={<Pause size={15} />} title="Always in your control">Pause instantly from the tray or a global shortcut. Recording state is always visible.</Feature>
        <Feature icon={<ShieldCheck size={15} />} title="Honest about what it does">Only speech is kept. Your mic is Me; everything you hear is Remote. No guessing who said what.</Feature>
      </ul>
    </>
  );
}

function Feature({ icon, title, children }: { icon: ReactNode; title: string; children: ReactNode }) {
  return (
    <li className="flex gap-3.5">
      <span className="mt-0.5 inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-line bg-surface text-accent-text">{icon}</span>
      <span>
        <span className="block text-base font-semibold text-text">{title}</span>
        <span className="block text-base text-muted">{children}</span>
      </span>
    </li>
  );
}

function Captured() {
  return (
    <>
      <StepTitle title="Two sources, kept apart">
        tuldMi records two separate tracks and labels them by where the sound came from.
      </StepTitle>
      <div className="grid grid-cols-2 gap-3 max-[760px]:grid-cols-1">
        <SourceExplainer icon={<Mic size={18} />} tone="me" title="Microphone" label="Me" text="Your voice — what you say in meetings, calls, or notes to yourself." />
        <SourceExplainer icon={<Monitor size={18} />} tone="remote" title="System audio" label="Remote" text="What plays through your speakers or headphones: the other side of a call, a video, a webinar." />
      </div>
      <div className="mt-5 rounded-xl border border-line-soft bg-sunken px-4 py-3.5 text-base text-muted">
        <span className="font-semibold text-text">About “Remote”.</span> When several people speak through your speakers, they all appear as Remote.
        tuldMi doesn't identify individual voices in this version.
      </div>
      <p className="mt-4 text-sm text-faint">Silence isn't stored. Speech is detected on the fly; a short buffer keeps the first words of every sentence.</p>
    </>
  );
}

function SourceExplainer({ icon, tone, title, label, text }: { icon: ReactNode; tone: "me" | "remote"; title: string; label: string; text: string }) {
  const me = tone === "me";
  return (
    <div className="rounded-xl border border-line bg-surface p-4 shadow-1">
      <div className="flex items-center gap-3">
        <span className={`inline-flex h-10 w-10 items-center justify-center rounded-lg ${me ? "bg-me-soft text-me-text" : "bg-remote-soft text-remote-text"}`}>{icon}</span>
        <div>
          <div className="text-base font-semibold text-text">{title}</div>
          <div className={`text-sm font-semibold ${me ? "text-me-text" : "text-remote-text"}`}>Labelled “{label}”</div>
        </div>
      </div>
      <p className="mt-3 text-base text-muted">{text}</p>
    </div>
  );
}

const RETENTION_OPTIONS: { value: Retention; label: string; hint: string }[] = [
  { value: "deleteAfterTranscription", label: "Don't keep", hint: "Deleted once transcribed" },
  { value: "days7", label: "7 days", hint: "Past week" },
  { value: "days30", label: "30 days", hint: "Recommended" },
  { value: "forever", label: "Indefinitely", hint: "Until you delete" },
];

function PrivacyStep({ retention, onRetention }: { retention: Retention; onRetention: (r: Retention) => void }) {
  const backend = useBackend();
  const [dir, setDir] = useState<string | null>(null);
  useEffect(() => {
    backend.storageStats().then((s) => setDir(s.dataDir), () => setDir(null));
  }, [backend]);
  return (
    <>
      <StepTitle title="Nothing leaves this computer">
        Audio, transcripts and the search index live in tuldMi's data folder on this device. There's no account, no cloud and no
        telemetry. The only time tuldMi uses the network is when you choose to download a transcription model.
      </StepTitle>
      {dir && (
        <div className="-mt-2 mb-7 flex items-center gap-3 rounded-lg border border-line bg-surface px-3.5 py-2.5">
          <Lock size={14} className="shrink-0 text-faint" aria-hidden />
          <span className="text-sm text-muted">Data folder</span>
          <span className="selectable min-w-0 flex-1 truncate font-mono text-[11.5px] text-text" title={dir}>
            {dir}
          </span>
        </div>
      )}
      <div className="mb-3 flex items-center gap-2 text-base font-semibold text-text">
        <HardDrive size={15} className="text-faint" aria-hidden /> How long should audio be kept?
      </div>
      <div role="radiogroup" aria-label="Keep audio for" className="grid grid-cols-4 gap-2 max-[760px]:grid-cols-2">
        {RETENTION_OPTIONS.map((o) => {
          const on = o.value === retention;
          return (
            <button
              key={o.value}
              type="button"
              role="radio"
              aria-checked={on}
              onClick={() => onRetention(o.value)}
              className={`rounded-lg border px-3 py-2.5 text-left transition-colors duration-[140ms] ${on ? "border-accent bg-accent-soft/60 shadow-[0_0_0_1px_var(--tm-accent)]" : "border-line bg-surface hover:border-line-strong"}`}
            >
              <span className="block text-base font-semibold text-text">{o.label}</span>
              <span className="block text-xs text-faint">{o.hint}</span>
            </button>
          );
        })}
      </div>
      <p className="mt-3 text-sm text-muted">Transcripts are always kept until you delete them. You can change this any time in Settings → Retention.</p>
    </>
  );
}

function Consent({ ack, onAck }: { ack: boolean; onAck: (v: boolean) => void }) {
  return (
    <>
      <StepTitle title="Recording is your responsibility">
        Laws about recording conversations differ by country and state. Some require everyone in a conversation to agree, and many
        workplaces have their own policies.
      </StepTitle>
      <ul className="mb-6 space-y-2 text-base text-muted">
        {[
          "Tell the people you talk with that you keep a transcript, where that's required.",
          "Check your employer's policy before recording work calls.",
          "Pause tuldMi whenever a conversation shouldn't be kept.",
        ].map((t) => (
          <li key={t} className="flex gap-2.5">
            <span className="mt-[9px] h-1 w-1 shrink-0 rounded-full bg-faint" aria-hidden />
            {t}
          </li>
        ))}
      </ul>
      <label className={`flex cursor-default items-start gap-3 rounded-xl border px-4 py-3.5 transition-colors ${ack ? "border-accent-line bg-accent-soft/50" : "border-line bg-surface hover:border-line-strong"}`}>
        <input type="checkbox" checked={ack} onChange={(e) => onAck(e.target.checked)} className="peer sr-only" />
        <span
          aria-hidden
          className={`mt-[1px] inline-flex h-[18px] w-[18px] shrink-0 items-center justify-center rounded-[5px] border transition-colors peer-focus-visible:outline-2 peer-focus-visible:outline-offset-2 peer-focus-visible:outline-[var(--tm-focus)] ${
            ack ? "border-accent bg-accent text-on-accent" : "border-line-strong bg-raised"
          }`}
        >
          {ack && <Check size={12} strokeWidth={3} />}
        </span>
        <span className="text-base text-text">
          I understand that I'm responsible for following recording-consent laws and my workplace's policies when using tuldMi.
        </span>
      </label>
    </>
  );
}

function Devices({ settings, onChange }: { settings: Settings; onChange: (p: Partial<Settings>) => void }) {
  const backend = useBackend();
  const status = useStatus();
  const [devices, setDevices] = useState<DeviceInfo[] | null>(null);
  useEffect(() => {
    backend.listDevices().then(setDevices, () => setDevices([]));
    // Live test: devices open, levels flow, nothing is stored. Closed when leaving the step.
    void backend.setMonitoring(true).catch(() => {});
    return () => void backend.setMonitoring(false).catch(() => {});
  }, [backend]);
  return (
    <>
      <StepTitle title="Check your devices">
        Speak normally and play something with speech — the meters respond to the real signal. Nothing is recorded during this test.
      </StepTitle>
      <div className="space-y-3">
        {(["mic", "system"] as const).map((src) => (
          <SourceCard
            key={src}
            source={src}
            settings={settings}
            devices={devices}
            status={status?.sources.find((s) => s.source === src)}
            onChange={onChange}
            metersExpected
            demo={backend.kind === "demo"}
          />
        ))}
      </div>
      <MonitoringNote requested />
      {backend.kind === "demo" && (
        <p className="mt-3 text-sm text-faint">Demo mode has no audio devices, so the meters stay at rest. In the app they move with your voice.</p>
      )}
    </>
  );
}

function ModelStep({ onSkip }: { onSkip: () => void }) {
  const { models } = useModels();
  const anyInstalled = models?.some((m) => m.installed);
  return (
    <>
      <StepTitle title="Choose a transcription model">
        Models turn speech into text on this computer. Pick one to download now — it happens once, and only when you press Download.
      </StepTitle>
      <ModelList compact />
      <div className="mt-5 flex items-center justify-between gap-4 rounded-xl border border-line-soft bg-sunken px-4 py-3">
        <p className="text-sm text-muted">
          {anyInstalled ? "You're set. You can add or switch models later in Settings." : "No rush — audio is still captured, and transcribed once a model is installed."}
        </p>
        {!anyInstalled && (
          <Button size="sm" variant="ghost" onClick={onSkip}>
            Skip for now
          </Button>
        )}
      </div>
    </>
  );
}

function Ready({ settings }: { settings: Settings }) {
  const { models } = useModels();
  const model = models?.find((m) => m.id === settings.modelId && m.installed);
  const ret = RETENTION_OPTIONS.find((r) => r.value === settings.retention)!;
  const sources = [settings.micEnabled && "Microphone (Me)", settings.systemEnabled && "System audio (Remote)"].filter(Boolean).join(" and ") || "No sources — turn one on in Settings";
  return (
    <>
      <StepTitle eyebrow="All set" title="tuldMi is ready to listen">
        It lives in your system tray. Closing the window keeps it listening; the tray icon always shows whether it's recording.
      </StepTitle>
      <div className="mb-6 flex items-center justify-between gap-4 rounded-xl border border-accent-line bg-accent-soft px-5 py-4">
        <div>
          <div className="text-base font-semibold text-text">Pause instantly, from anywhere</div>
          <div className="text-sm text-muted">Press it again to resume. Change it in Settings → Shortcuts.</div>
        </div>
        <Keys combo={settings.pauseShortcut} />
      </div>
      <dl className="divide-y divide-line-soft rounded-xl border border-line bg-surface text-base">
        <Summary k="Capturing" v={sources} />
        <Summary k="Transcription" v={model ? model.name : "No model yet — speech waits in the queue"} />
        <Summary k="Audio kept" v={`${ret.label}${ret.value === "days30" ? "" : ""} · transcripts kept until you delete them`} />
        <Summary k="Stored at" v="This computer only" />
      </dl>
    </>
  );
}

function Summary({ k, v }: { k: string; v: string }) {
  return (
    <div className="flex gap-4 px-4 py-2.5">
      <dt className="w-[120px] shrink-0 text-muted">{k}</dt>
      <dd className="text-text">{v}</dd>
    </div>
  );
}
