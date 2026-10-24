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
    let shortcut = state.engine.settings().pause_shortcut.replace("CommandOrControl", "Ctrl").replace("Backquote", "`");
    let engine = state.engine.clone();
    let win = window.clone();
    app.dialog()
        .message(format!(
            "tuldMi keeps recording in the background after you close this window.\n\n\
             The tray icon shows when it is listening. Pause any time from the tray or with {shortcut}."
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
