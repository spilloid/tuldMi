# tuldMi frontend

React 19 + TypeScript + Vite 8 + Tailwind CSS 4, rendered in the Tauri 2 webview. Everything
lives under `src/`; the Rust side is reached only through `src/lib/ipc/`.

## Running

```sh
npm install
npm run dev          # http://localhost:1420 — demo backend in a browser
npm run typecheck
npm test             # vitest
npm run build        # dist/, consumed by the Tauri shell
```

In a plain browser the app runs on the **demo backend** (sample data, always labelled with a
"Demo data" pill in the titlebar). Inside Tauri it uses the real backend unless the URL has `?demo`.

Demo / screenshot URL flags (query string, demo backend only):

| Flag | Effect |
|---|---|
| `demo` | Force the demo backend inside Tauri |
| `fresh` | First-run state (onboarding, no model installed) |
| `empty` | No history (empty-state timeline) |
| `paused` | Start paused |
| `still` | No scripted live arrivals (deterministic screenshots) |
| `clock=HH:MM` | Pretend the wall clock reads HH:MM today (demo only) |
| `theme=light\|dark` | Override the theme setting |
| `fixture` | **Dev server only**: playback fixture with a synthesized test tone, waveform computed from the same samples |

Every surface has a hash route: `#/timeline`, `#/timeline/<eventId>` (or `@N` = Nth newest),
`#/search?q=…&src=mic&range=7d`, `#/settings/<section>`, `#/onboarding/<step>`. Append
`?palette=<query>` or `?keys` to a hash route to open the command palette / shortcut sheet.

## Layout

```
src/
  app/            App (providers, boot, theme), Shell (layout, global keys, notices)
  styles/         tokens.css (design tokens, paper + ink themes), index.css (Tailwind @theme, base)
  lib/
    ipc/          types.ts (mirror of crates/engine/src/types.rs), backend.ts (Backend interface),
                  tauri.ts (invoke/listen), demo.ts + demoData.ts + demoSearch.ts (DemoBackend)
    motion.ts     duration / easing tokens (MISSION §2), reduced-motion hook
    time.ts       formatting, day/conversation grouping (3-min gap clustering), event merge
    snippet.ts    \u0002/\u0003 highlight splitting (never HTML)
    peaks.ts      peak-envelope resampling, dB meter mapping
  state/          AppStore (observables + live event merge), hooks (useStatus, useLevels,
                  useTimeline, useSearch, useSettings, useModels), ui (selection/overlays), player
  components/
    viz/          scheduler (one shared rAF loop), LiveMeter, RecordingIndicator, Waveform
    ui/           Button, Tooltip, Kbd, controls (Switch, Slider, Select, Segmented, Pill,
                  ProgressBar), Dialog, Confirm, Toast
    shell/        Titlebar (frameless, window controls), NavRail, CaptureBar, Logo
  features/       timeline, player, search, settings, onboarding, shortcuts, shared
  dev/            Fixture (dev-only, tree-shaken from production builds)
```

## Canvas rendering

All animated canvases register with one scheduler (`components/viz/scheduler.ts`). The rAF loop
only exists while at least one task is awake **and** on screen (IntersectionObserver +
`visibilitychange`); tasks pick 60 or 30 fps and go dormant (returning 0) when nothing changes.
Canvases are DPR-aware via ResizeObserver and a `resolution` media query, redraw on theme
change, are `aria-hidden`, and always have a text equivalent nearby. Reduced motion renders a
static frame.

- **LiveMeter** draws a scrolling RMS/peak history from real `tuldmi://levels` frames only. With
  no frames it shows a dotted baseline and "No live audio" and requests no frames.
- **RecordingIndicator** morphs between recording (breathing ring at 30 fps), paused, stopped
  and error glyphs.
- **Waveform** renders `audio.peaks` (50/s, 0..255) with max-downsampling; the player variant is
  an accessible `role="slider"` with pointer scrubbing and arrow/Home/End/PageUp/PageDown keys.
  Missing audio never gets a placeholder waveform — the UI shows the unavailable state.

## Dependencies (and why)

Runtime:

| Package | License | Why |
|---|---|---|
| react, react-dom 19 | MIT | UI |
| @tauri-apps/api 2 | MIT / Apache-2.0 | `invoke`, `listen`, `convertFileSrc`, window controls |
| @radix-ui/react-dialog | MIT | Accessible modal focus trap / scroll lock (dialogs, palette, confirms) |
| @radix-ui/react-context-menu | MIT | Event right-click menu with keyboard support and collision handling |
| @radix-ui/react-dropdown-menu | MIT | Select-style pickers and the speed menu (radio items, typeahead) |
| @radix-ui/react-tooltip | MIT | Delay-grouped, accessible tooltips |
| @radix-ui/react-switch | MIT | Toggle semantics (`role=switch`) |
| @radix-ui/react-slider | MIT | Keyboard/pointer slider for VAD settings |
| motion 14 | MIT | Height-auto expansion, exit animations (AnimatePresence), toasts, view transitions |
| lucide-react | ISC | Icons |
| @fontsource-variable/inter, fraunces, jetbrains-mono | OFL-1.1 | Bundled fonts — no network at runtime |

Dev: vite, @vitejs/plugin-react, tailwindcss + @tailwindcss/vite, typescript 7, vitest, jsdom,
@testing-library/react, @types/react(-dom).

## Notes for the shell

- The titlebar uses `data-tauri-drag-region`; the window needs `decorations: false` and the
  `core:window:allow-start-dragging`, `allow-minimize`, `allow-toggle-maximize`, `allow-close`,
  `allow-is-maximized` permissions. Close calls `getCurrentWindow().close()`; hide-to-tray is the
  shell's decision.
- Toasts are available to shell-driven flows as `window.tuldmiToast.show({ level, title, message,
  action })`, and every `tuldmi://notice` becomes a toast.
- Audio playback uses `convertFileSrc(audio.path)`, so the asset protocol must be enabled and
  scoped to the data directory.
