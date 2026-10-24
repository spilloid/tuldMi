# IPC contract (webview <-> Tauri shell)

Types are defined in `crates/engine/src/types.rs` and mirrored in `src/lib/ipc/types.ts`
(camelCase JSON). Commands are Tauri `invoke` calls with camelCase argument names. Every command
returns `Result<T, String>`; the error string is user-presentable.

## Commands

| Command | Args | Returns | Notes |
|---|---|---|---|
| `get_status` | — | `CaptureStatus` | |
| `set_paused` | `paused: bool` | `CaptureStatus` | Pause = devices closed, nothing captured. Resume starts a new session. |
| `set_monitoring` | `enabled: bool` | `CaptureStatus` | Onboarding mic test: devices open, `levels` flow, **nothing is stored**. Ignored while recording. |
| `list_devices` | — | `DeviceInfo[]` | Inputs and loopback-capable outputs. |
| `get_settings` | — | `Settings` | |
| `update_settings` | `settings: Settings` | `Settings` | Whole object. Engine re-opens devices if sources changed; registers new shortcut; applies autostart. |
| `list_events` | `before?: number, beforeId?: string, limit: number` | `SpeechEvent[]` | Newest first, keyset cursor on `(startedAt, id)` of the last event of the previous page, so equal start times are never skipped. |
| `get_event` | `id: string` | `SpeechEvent \| null` | |
| `search` | `query: SearchQuery` | `SearchHit[]` | Snippet highlights are `\u0002`…`\u0003`. Never render as HTML. |
| `retry_event` | `id: string` | `boolean` | |
| `delete_event` | `id: string` | `void` | Transcript and audio. |
| `delete_event_audio` | `id: string` | `void` | Keeps transcript. |
| `delete_all_history` | — | `void` | Everything except settings and models. UI must confirm. |
| `list_models` | — | `ModelInfo[]` | Catalog + installed flag. |
| `download_model` | `modelId: string` | `void` | Returns immediately; progress via `model-progress`. Only on explicit user action. |
| `cancel_model_download` | `modelId: string` | `void` | |
| `delete_model` | `modelId: string` | `void` | |
| `storage_stats` | — | `StorageStats` | |
| `open_data_dir` | — | `void` | Opens the folder in the OS file manager. |
| `export_transcripts` | `format: "markdown" \| "json", from?: number, to?: number` | `string` | Writes to `Documents/tuldMi Exports/`, returns the absolute path. |
| `reveal_path` | `path: string` | `void` | Show a file (e.g. an export) in the file manager. |
| `quit_app` | — | `void` | Ends the session cleanly, flushes in-progress speech, exits. |

Audio playback: `convertFileSrc(event.audio.path)` (asset protocol, scoped to the data dir).

## Events (`listen`)

| Event | Payload |
|---|---|
| `tuldmi://status` | `CaptureStatus` — on every state, device, backlog, or transcriber change |
| `tuldmi://levels` | `LevelFrame[]` — ~30 Hz while capturing or monitoring; one frame per active source |
| `tuldmi://event-upserted` | `SpeechEvent` — new event, or status/transcript change |
| `tuldmi://event-deleted` | `string` (event id) |
| `tuldmi://model-progress` | `ModelProgress` |
| `tuldmi://notice` | `{ level: "info" \| "warning" \| "error", message: string }` |
| `tuldmi://navigate` | `{ view: "timeline" \| "settings" \| "search", section?: string }` — from the tray menu. Settings sections: `audio`, `transcription`, `recording`, `retention`, `privacy`, `appearance`, `shortcuts`, `startup`, `storage`, `about`. |

## Window / tray behaviour

- Closing the main window hides it; capture continues. The first time, the UI shows a notice
  explaining tuldMi keeps listening in the tray (with a "Pause recording" action).
- Tray menu: status line, Pause/Resume, Open tuldMi, Mic/System status lines, backlog line,
  Settings…, Quit tuldMi. Tray icon reflects recording / paused / error.
- Global shortcut (setting `pauseShortcut`) toggles pause from anywhere.

## Contract notes (2026-10-24)

- `CaptureStatus.monitoring` is true while devices are open for the level test only.
- `Settings.consentAcknowledgedAt` records when the user acknowledged the recording-consent
  notice in onboarding (epoch ms); onboarding requires it.
- App version: use `getVersion()` from `@tauri-apps/api/app` (no custom command).
- Deferred to v0.1.x: listing in-flight downloads on reload, search offset paging, a source filter
  on export, per-model language lists.
