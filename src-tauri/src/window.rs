//! Main-window behaviour: closing hides to the tray (capture continues), and the first time it
//! happens while recording, the user is told so plainly and offered to pause.

use tauri::{AppHandle, Manager, Window, WindowEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tuldmi_engine::types::CaptureState;

use crate::AppState;

const NOTICE_MARKER: &str = ".close-to-tray-explained";

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// "CommandOrControl+Shift+Backquote" as the user reads it: "Ctrl+Shift+`" or "⌘⇧`".
fn shortcut_label(accelerator: &str) -> String {
    let mac = cfg!(target_os = "macos");
    let keys: Vec<&str> = accelerator
        .split('+')
        .map(|k| match k {
            "CommandOrControl" | "CmdOrCtrl" => if mac { "⌘" } else { "Ctrl" },
            "Shift" => if mac { "⇧" } else { "Shift" },
            "Alt" => if mac { "⌥" } else { "Alt" },
            "Control" | "Ctrl" => if mac { "⌃" } else { "Ctrl" },
            "Backquote" => "`",
            "Comma" => ",",
            other => other,
        })
        .collect();
    keys.join(if mac { "" } else { "+" })
}

pub fn on_event(window: &Window, event: &WindowEvent) {
    if window.label() != "main" {
        return;
    }
    let WindowEvent::CloseRequested { api, .. } = event else { return };
    api.prevent_close();
    let app = window.app_handle().clone();
    let Some(state) = app.try_state::<AppState>() else {
        let _ = window.hide();
        return;
    };
    // A device test only makes sense while its screen is visible. Hiding the window never
    // unmounts the test UI, so end monitoring here — otherwise the mic would stay open with
    // the tray showing "Paused". Off the main thread: the engine call waits for a reply.
    if state.engine.status().monitoring {
        let engine = state.engine.clone();
        let _ = std::thread::Builder::new().name("tuldmi-monitor-off".into()).spawn(move || {
            if let Err(e) = engine.set_monitoring(false) {
                tracing::warn!(error = %e, "stop monitoring on hide failed");
            }
        });
    }
    let recording = matches!(state.engine.status().state, CaptureState::Recording | CaptureState::Starting);
    let marker = state.data_dir.join(NOTICE_MARKER);
    if !recording || marker.exists() {
        let _ = window.hide();
        return;
    }
    let _ = std::fs::write(&marker, b"1");
    let shortcut = shortcut_label(&state.engine.settings().pause_shortcut);
    let tray = if cfg!(target_os = "macos") { "menu bar icon" } else { "tray icon" };
    let tray_place = if cfg!(target_os = "macos") { "menu bar" } else { "tray" };
    let engine = state.engine.clone();
    let win = window.clone();
    app.dialog()
        .message(format!(
            "tuldMi keeps recording in the background after you close this window.\n\n\
             The {tray} shows when it is listening. Pause any time from the {tray_place} or with {shortcut}."
        ))
        .title("tuldMi is still listening")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom("Keep recording".into(), "Pause recording".into()))
        .show(move |keep| {
            if !keep {
                let _ = std::thread::Builder::new().name("tuldmi-pause".into()).spawn(move || {
                    if let Err(e) = engine.set_paused(true) {
                        tracing::warn!(error = %e, "pause from close dialog failed");
                    }
                });
            }
            let _ = win.hide();
        });
}

#[cfg(test)]
mod tests {
    use super::shortcut_label;

    /// `setup` builds the main window itself, after the `--pause` exit, so Tauri must never
    /// build it first. A platform file's `windows` array replaces the base one (JSON merge
    /// patch), so every file that declares windows must say `create: false` itself, or that
    /// platform creates `main` twice and never starts.
    #[test]
    fn no_config_file_lets_tauri_create_the_main_window() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut checked = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if !(name.starts_with("tauri.") && name.ends_with(".json")) {
                continue;
            }
            let config: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let Some(windows) = config.pointer("/app/windows").and_then(|w| w.as_array()) else {
                continue;
            };
            for window in windows {
                assert_eq!(window.get("create"), Some(&serde_json::Value::Bool(false)), "{name}: window {:?} must set \"create\": false", window.get("label"));
                checked += 1;
            }
        }
        assert!(checked >= 2, "expected the base and macOS window configs, checked {checked}");
    }

    #[test]
    fn default_pause_shortcut_reads_naturally() {
        let label = shortcut_label("CommandOrControl+Shift+Backquote");
        if cfg!(target_os = "macos") {
            assert_eq!(label, "⌘⇧`");
        } else {
            assert_eq!(label, "Ctrl+Shift+`");
        }
    }
}
