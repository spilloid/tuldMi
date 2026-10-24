import { AudioLines, HardDrive, Info, Keyboard, Languages, Palette, Power, ShieldCheck, SlidersHorizontal, Timer } from "lucide-react";
import type { ReactNode } from "react";
import { navigate } from "../../lib/router";
import { useSettings } from "../../state/hooks";
import {
  AboutSection,
  AppearanceSection,
  AudioSection,
  PrivacySection,
  RecordingSection,
  RetentionSection,
  ShortcutsSection,
  StartupSection,
  StorageSection,
  TranscriptionSection,
} from "./sections";

const SECTIONS: { id: string; label: string; icon: ReactNode; render: () => ReactNode }[] = [
  { id: "audio", label: "Audio sources", icon: <AudioLines size={15} />, render: () => <AudioSection /> },
  { id: "transcription", label: "Transcription", icon: <Languages size={15} />, render: () => <TranscriptionSection /> },
  { id: "recording", label: "Recording", icon: <SlidersHorizontal size={15} />, render: () => <RecordingSection /> },
  { id: "retention", label: "Retention", icon: <Timer size={15} />, render: () => <RetentionSection /> },
  { id: "privacy", label: "Privacy & data", icon: <ShieldCheck size={15} />, render: () => <PrivacySection /> },
  { id: "appearance", label: "Appearance", icon: <Palette size={15} />, render: () => <AppearanceSection /> },
  { id: "shortcuts", label: "Shortcuts", icon: <Keyboard size={15} />, render: () => <ShortcutsSection /> },
  { id: "startup", label: "Startup", icon: <Power size={15} />, render: () => <StartupSection /> },
  { id: "storage", label: "Storage", icon: <HardDrive size={15} />, render: () => <StorageSection /> },
  { id: "about", label: "About", icon: <Info size={15} />, render: () => <AboutSection /> },
];

export function SettingsView({ section }: { section: string }) {
  const { settings } = useSettings();
  const current = SECTIONS.find((s) => s.id === section) ?? SECTIONS[0];
  return (
    <div className="flex min-h-0 flex-1">
      <nav aria-label="Settings sections" className="w-[212px] shrink-0 overflow-y-auto border-r border-line-soft px-3 py-6 max-[1100px]:w-[184px]">
        <div className="caps mb-2 px-2.5 text-faint">Settings</div>
        <ul className="space-y-px">
          {SECTIONS.map((s) => {
            const on = s.id === current.id;
            return (
              <li key={s.id}>
                <button
                  type="button"
                  aria-current={on ? "page" : undefined}
                  onClick={() => navigate(`/settings/${s.id}`, { replace: true })}
                  className={`flex h-8 w-full items-center gap-2.5 rounded-md px-2.5 text-left text-base transition-colors duration-[140ms] ${
                    on ? "bg-selected font-medium text-text" : "text-muted hover:bg-hover hover:text-text"
                  }`}
                >
                  <span className={on ? "text-accent-text" : "text-faint"}>{s.icon}</span>
                  {s.label}
                </button>
              </li>
            );
          })}
        </ul>
      </nav>
      <div className="min-w-0 flex-1 overflow-y-auto [scrollbar-gutter:stable]">
        <div key={current.id} className="mx-auto max-w-[720px] px-8 pb-16 pt-8 max-[1100px]:px-6">
          {settings ? current.render() : null}
        </div>
      </div>
    </div>
  );
}
