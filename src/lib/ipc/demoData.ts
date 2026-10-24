// Sample content for the demo backend. Fictional people and companies. Everything built from
// this file is shown under a persistent "Demo data" marker in the UI.
//
// Demo events carry no audio (`audio: null`) — we never synthesize fake recordings. Events
// older than the demo retention window are marked `audioExpired: true`.

import type { DeviceInfo, EventStatus, ModelInfo, Source, Speaker, SpeechEvent } from "./types";

type Turn = [Source, string | string[], Partial<Pick<SpeechEvent, "status" | "error">>?];

interface ScriptedConversation {
  /** Days before today (0 = today). */
  daysAgo: number;
  /** Either a fixed "HH:MM" clock time or minutes before now (for today). */
  at: string | { minutesAgo: number };
  turns: Turn[];
}

const ME: Speaker = { id: "00000000-0000-4000-8000-00000000000a", label: "Me", kind: "self" };
const REMOTE: Speaker = { id: "00000000-0000-4000-8000-00000000000b", label: "Remote", kind: "remote" };

export const DEMO_MODEL_ID = "whisper-base.en";
export const DEMO_RETENTION_DAYS = 7;

const conversations: ScriptedConversation[] = [
  {
    daysAgo: 9,
    at: "16:00",
    turns: [
      ["system", [
        "Welcome everyone to the spring release webinar. Today we're walking through offline sync, the new sharing model, and what changed in pricing.",
        "If you have questions, drop them in the chat and we'll take as many as we can at the end.",
      ]],
      ["system", "Offline sync is the big one. Edits now queue locally and replay in order when you reconnect, with conflict markers instead of silent overwrites."],
      ["system", "Sharing moves from per-document links to workspaces, so access follows the team instead of the file."],
      ["mic", "Okay, the conflict markers part is what we need for the field team."],
      ["system", "Pricing for existing customers doesn't change until your renewal date, and annual plans keep their current rate for one more term."],
    ],
  },
  {
    daysAgo: 3,
    at: "10:30",
    turns: [
      ["system", "Can everyone see the Figma file? I'm on the onboarding flow, frame twelve."],
      ["mic", "Yes, I've got it. The step indicator feels heavier than the content now."],
      ["system", [
        "Agreed. I tried a thinner progress rail but it disappeared on the dark theme.",
        "Maybe we keep the rail and drop the step numbers entirely.",
      ]],
      ["mic", "", { status: "failed", error: "Transcription failed: the model process exited unexpectedly (code 3). Audio was kept, so you can retry." }],
      ["system", "Marco, can you take the copy pass on the consent screen? Legal wants the wording about recording laws to be explicit."],
      ["mic", "I'll do it this afternoon and send it to Dana before Thursday."],
      ["system", "", { status: "empty" }],
      ["system", "Great. Last thing, the empty state for search still says lorem ipsum in staging."],
      ["mic", "Ha, yes, I'll fix that today too."],
    ],
  },
  {
    daysAgo: 3,
    at: "15:10",
    turns: [
      ["mic", [
        "Note to self: the contractor quote for the kitchen came in at eleven thousand four hundred, which is above what we planned.",
        "Ask about splitting the countertop into a second phase.",
      ]],
    ],
  },
  {
    daysAgo: 1,
    at: "09:30",
    turns: [
      ["system", "Morning! Quick one today, I just want to check where we are on the Henderson proposal."],
      ["mic", "The draft is done. I'm waiting on the final numbers from finance, they promised them by noon."],
      ["system", "Okay. If it slips, send it with the ranges and mark the totals as estimates."],
      ["mic", "That works. I'll also attach the timeline we discussed, the twelve week version."],
      ["system", "Perfect. And remind me, did Henderson want the on-site training or the remote option?"],
      ["mic", "On-site for the first two sessions, then remote. They were clear about that."],
      ["system", "Good. Let's talk Thursday after it's out."],
    ],
  },
  {
    daysAgo: 1,
    at: "13:05",
    turns: [
      ["system", "Hi, this is Lakeside Dental calling to confirm your appointment on Tuesday the twenty-eighth at three fifteen."],
      ["mic", "Yes, that still works. Is it the cleaning or the follow-up for the crown?"],
      ["system", "Both, actually. Dr. Okafor wanted to look at the crown while you're in. Please plan for about an hour."],
      ["mic", "Okay, perfect. Thank you."],
    ],
  },
  {
    daysAgo: 1,
    at: "17:40",
    turns: [
      ["system", [
        "On today's episode we're talking about memory: why we forget most conversations within a day, and what that means for how we take notes.",
        "The research here is fascinating. Within twenty-four hours people lose the majority of the specifics, but keep the gist.",
      ]],
      ["system", "Which is exactly why writing things down right after a meeting matters so much more than writing them during."],
    ],
  },
  {
    daysAgo: 0,
    at: { minutesAgo: 310 },
    turns: [
      ["system", "Okay, let's start. Standup, quick round. Jae, you want to go first?"],
      ["mic", "Sure. Yesterday I finished the export flow, Markdown and JSON both work. Today I'm on the settings storage page."],
      ["system", "Nice. Any blockers?"],
      ["mic", "Not really. I need someone to review the retention copy, it has to be very clear that transcripts are kept."],
      ["system", "I can take that review after lunch."],
      ["system", "From my side, the installer is signed now. I'm testing the upgrade path from the beta build today."],
      ["mic", "Does the upgrade keep the database in place?"],
      ["system", "It does. The data folder isn't touched, only the app binaries."],
    ],
  },
  {
    daysAgo: 0,
    at: { minutesAgo: 140 },
    turns: [
      ["mic", "Reminder: pick up the prescription before six, and call Mom back about the weekend."],
    ],
  },
  {
    daysAgo: 0,
    at: { minutesAgo: 24 },
    turns: [
      ["system", "Thanks for jumping on. I wanted to go through the Q4 roadmap before it goes to the board."],
      ["mic", "Of course. I read the doc this morning. My main question is about the mobile timeline."],
      ["system", [
        "Right. We pushed mobile to January. The sync layer has to be solid first, and honestly we don't have the people for both.",
        "The board will ask about it, so I want us aligned on the reasoning.",
      ]],
      ["mic", "I agree with the call. I'd frame it as protecting the reliability work rather than delaying mobile."],
      ["system", "", { status: "empty" }],
      ["system", "That's a good framing. Can you write two sentences on that for the deck?"],
      ["mic", "Yes, I'll send them by end of day, along with the hiring numbers."],
      ["system", "Great. And the budget line for contractors, are we still at one hundred and twenty thousand?", { status: "transcribing" }],
      ["mic", "", { status: "queued" }],
    ],
  },
];

/** Events that "arrive" while the demo is recording, to show live behaviour. */
export const LIVE_SCRIPT: Turn[] = [
  ["mic", "Yes, one twenty, and that covers the two contractors through March."],
  ["system", "Okay. Then I think we're in good shape. I'll send the deck around tomorrow morning."],
  ["mic", "Sounds good. Talk soon."],
  ["system", "Bye!"],
];

// -------------------------------------------------------------------------------------------

let counter = 0;
function uuid(): string {
  counter += 1;
  const n = counter.toString(16).padStart(12, "0");
  return `d3m0d3m0-0000-4000-8000-${n}`;
}

/** Deterministic, cheap PRNG for spacing only (never for signals). */
function prng(seed: number) {
  let s = seed >>> 0;
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 0x100000000;
  };
}

const WORDS_PER_SECOND = 2.6;

export function makeEvent(
  source: Source,
  sessionId: string,
  startedAt: number,
  texts: string[],
  status: EventStatus,
  error: string | null,
  audioExpired: boolean,
): SpeechEvent {
  const id = uuid();
  let offset = 0;
  const segments = texts
    .filter((t) => t.length > 0)
    .map((text) => {
      const words = text.split(/\s+/).length;
      const dur = Math.round((words / WORDS_PER_SECOND) * 1000);
      const seg = { id: uuid(), eventId: id, startMs: offset, endMs: offset + dur, text, confidence: 0.86 + ((words * 7) % 13) / 100 };
      offset += dur + 420;
      return seg;
    });
  const showText = status === "done";
  const duration = segments.length ? offset - 420 : 1800 + ((texts.join("").length * 37) % 2400);
  return {
    id,
    sessionId,
    source,
    speaker: source === "mic" ? ME : REMOTE,
    startedAt,
    endedAt: startedAt + duration,
    status,
    audio: null,
    audioExpired,
    segments: showText ? segments : [],
    language: showText ? "en" : null,
    modelId: status === "done" || status === "empty" ? DEMO_MODEL_ID : null,
    error,
  };
}

/** Build the demo history relative to `now`, oldest first. */
export function buildDemoEvents(now: number): { events: SpeechEvent[]; pendingText: Map<string, string[]> } {
  counter = 0;
  const events: SpeechEvent[] = [];
  const pendingText = new Map<string, string[]>();
  const rand = prng(7);
  for (const conv of conversations) {
    let t: number;
    if (typeof conv.at === "string") {
      const [h, m] = conv.at.split(":").map(Number);
      const d = new Date(now);
      d.setDate(d.getDate() - conv.daysAgo);
      d.setHours(h, m, Math.floor(rand() * 50), 0);
      t = d.getTime();
    } else {
      t = now - conv.at.minutesAgo * 60_000;
    }
    const sessionId = uuid();
    const expired = now - t > DEMO_RETENTION_DAYS * 86_400_000;
    for (const [source, text, opts] of conv.turns) {
      const texts = Array.isArray(text) ? text : [text];
      const status = opts?.status ?? "done";
      const ev = makeEvent(source, sessionId, t, texts, status, opts?.error ?? null, expired);
      if (status === "queued" || status === "transcribing") {
        // The text this event will "resolve" to once the demo finishes transcribing it.
        pendingText.set(ev.id, texts.filter(Boolean).length ? texts : [LIVE_SCRIPT[0][1] as string]);
      }
      if (status === "failed") pendingText.set(ev.id, ["Sorry, go back one frame, I want to compare the two headers side by side."]);
      events.push(ev);
      t = ev.endedAt + Math.round(900 + rand() * 6500);
    }
  }
  // Clamp the newest events so nothing lands in the future.
  const latest = events[events.length - 1];
  if (latest && latest.endedAt > now) {
    const shift = latest.endedAt - now + 4000;
    for (const ev of events) if (now - ev.startedAt < 3_600_000) {
      ev.startedAt -= shift;
      ev.endedAt -= shift;
    }
  }
  return { events, pendingText };
}

export const DEMO_DEVICES: DeviceInfo[] = [
  { id: "in:usb-podcast", name: "Shure MV7 (USB Audio)", kind: "input", isDefault: true },
  { id: "in:array", name: "Microphone Array (Realtek Audio)", kind: "input", isDefault: false },
  { id: "in:headset", name: "Headset Microphone (Jabra Evolve2 65)", kind: "input", isDefault: false },
  { id: "out:speakers", name: "Speakers (Realtek Audio)", kind: "output", isDefault: true },
  { id: "out:headset", name: "Headphones (Jabra Evolve2 65)", kind: "output", isDefault: false },
  { id: "out:hdmi", name: "DELL U2723QE (NVIDIA High Definition Audio)", kind: "output", isDefault: false },
];

// Mirrors crates/engine/src/models.rs CATALOG exactly (ids, names, sizes, license).
const HF_BASE = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/";
const DEMO_LICENSE = "MIT (OpenAI Whisper weights; ggml conversion by whisper.cpp)";

export const DEMO_MODELS: ModelInfo[] = [
  {
    id: "whisper-tiny.en",
    name: "Whisper Tiny (English)",
    description: "Fastest and smallest. Good for older or low-power machines; less accurate.",
    sizeBytes: 77_704_715,
    languages: "English",
    license: DEMO_LICENSE,
    url: HF_BASE + "ggml-tiny.en.bin",
    installed: false,
    recommended: false,
  },
  {
    id: "whisper-base.en",
    name: "Whisper Base (English)",
    description: "Balanced speed and accuracy for English on most CPUs. Recommended.",
    sizeBytes: 147_964_211,
    languages: "English",
    license: DEMO_LICENSE,
    url: HF_BASE + "ggml-base.en.bin",
    installed: true,
    recommended: true,
  },
  {
    id: "whisper-base",
    name: "Whisper Base (Multilingual)",
    description: "Same size as Base English, recognizes ~99 languages.",
    sizeBytes: 147_951_465,
    languages: "Multilingual",
    license: DEMO_LICENSE,
    url: HF_BASE + "ggml-base.bin",
    installed: false,
    recommended: false,
  },
  {
    id: "whisper-small.en",
    name: "Whisper Small (English)",
    description: "Noticeably more accurate English; needs a reasonably fast CPU.",
    sizeBytes: 487_614_201,
    languages: "English",
    license: DEMO_LICENSE,
    url: HF_BASE + "ggml-small.en.bin",
    installed: false,
    recommended: false,
  },
  {
    id: "whisper-large-v3-turbo-q5",
    name: "Whisper Large v3 Turbo (quantized)",
    description: "Highest accuracy, multilingual. Slowest on CPU; best on fast machines.",
    sizeBytes: 574_041_195,
    languages: "Multilingual",
    license: DEMO_LICENSE,
    url: HF_BASE + "ggml-large-v3-turbo-q5_0.bin",
    installed: false,
    recommended: false,
  },
];
