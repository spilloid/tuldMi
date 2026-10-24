//! Engine -> webview event bridge. Event names are the contract in `docs/IPC.md`.

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tuldmi_engine::engine::EventSink;
use tuldmi_engine::types::{EngineEvent, NoticeLevel};

pub const STATUS: &str = "tuldmi://status";
pub const LEVELS: &str = "tuldmi://levels";
pub const EVENT_UPSERTED: &str = "tuldmi://event-upserted";
pub const EVENT_DELETED: &str = "tuldmi://event-deleted";
pub const MODEL_PROGRESS: &str = "tuldmi://model-progress";
pub const NOTICE: &str = "tuldmi://notice";
pub const NAVIGATE: &str = "tuldmi://navigate";

#[derive(Clone, Serialize)]
struct Notice<'a> {
    level: NoticeLevel,
    message: &'a str,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Navigate {
    pub view: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<&'static str>,
}

pub struct TauriSink {
    app: AppHandle,
}

impl TauriSink {
    pub fn new(app: AppHandle) -> Self {
        TauriSink { app }
    }
}

impl EventSink for TauriSink {
    fn emit(&self, event: EngineEvent) {
        let result = match &event {
            EngineEvent::Status(s) => {
                // Never call tray APIs from an engine thread: off the main thread they block
                // until the main thread services them, and during exit it never does — that
                // stalled the engine's flush and lost in-progress speech on Quit. Hand the
                // update to the main thread fire-and-forget; drop it once we're exiting.
                if !crate::is_exiting() {
                    let app = self.app.clone();
                    let status = s.clone();
                    let _ = self.app.run_on_main_thread(move || crate::tray::update(&app, &status));
                }
                self.app.emit(STATUS, s)
            }
            EngineEvent::Levels(l) => self.app.emit(LEVELS, l),
            EngineEvent::EventUpserted(e) => self.app.emit(EVENT_UPSERTED, e),
            EngineEvent::EventDeleted(id) => self.app.emit(EVENT_DELETED, id.to_string()),
            EngineEvent::ModelProgress(p) => self.app.emit(MODEL_PROGRESS, p),
            EngineEvent::Notice { level, message } => self.app.emit(NOTICE, Notice { level: *level, message }),
        };
        if let Err(e) = result {
            tracing::debug!(error = %e, "event emit failed");
        }
    }
}
