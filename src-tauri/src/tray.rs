//! System tray: always-visible recording state plus the controls that matter when the window is
//! hidden. Menu text and icon follow every `CaptureStatus` the engine emits.

use parking_lot::Mutex;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};
use tuldmi_engine::types::{CaptureState, CaptureStatus, Source, SourceState, TranscriberState};

use crate::events::{Navigate, NAVIGATE};
use crate::icons::{self, Glyph};

const TRAY_ID: &str = "main";

pub struct TrayItems {
    status: MenuItem<Wry>,
    toggle: MenuItem<Wry>,
    mic: MenuItem<Wry>,
    system: MenuItem<Wry>,
    backlog: MenuItem<Wry>,
    last: Mutex<Option<Snapshot>>,
}

#[derive(Clone, PartialEq)]
struct Snapshot {
    glyph: Glyph,
    status: String,
    toggle: String,
    mic: String,
    system: String,
    backlog: String,
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let status = MenuItem::with_id(app, "status", "tuldMi", false, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "Start recording", true, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open tuldMi", true, None::<&str>)?;
    let mic = MenuItem::with_id(app, "mic", "Microphone", false, None::<&str>)?;
    let system = MenuItem::with_id(app, "system", "System audio", false, None::<&str>)?;
    let backlog = MenuItem::with_id(app, "backlog", "Transcription", false, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings...", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit tuldMi", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &status,
            &PredefinedMenuItem::separator(app)?,
            &toggle,
            &open,
            &PredefinedMenuItem::separator(app)?,
            &mic,
            &system,
            &backlog,
            &PredefinedMenuItem::separator(app)?,
            &settings,
            &quit,
        ],
    )?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icons::tray(Glyph::Idle))
        .tooltip("tuldMi")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "toggle" => crate::toggle_pause(app),
            "open" => crate::window::show_main(app),
            "settings" => {
                crate::window::show_main(app);
                let _ = app.emit(NAVIGATE, Navigate { view: "settings", section: None });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                crate::window::show_main(tray.app_handle());
            }
        })
        .build(app)?;

    app.manage(TrayItems { status, toggle, mic, system, backlog, last: Mutex::new(None) });
    Ok(())
}

pub fn update(app: &AppHandle, s: &CaptureStatus) {
    let Some(items) = app.try_state::<TrayItems>() else { return };
    let snap = snapshot(s);
    {
        let mut last = items.last.lock();
        if last.as_ref() == Some(&snap) {
            return;
        }
        *last = Some(snap.clone());
    }
    let _ = items.status.set_text(&snap.status);
    let _ = items.toggle.set_text(&snap.toggle);
    let _ = items.mic.set_text(&snap.mic);
    let _ = items.system.set_text(&snap.system);
    let _ = items.backlog.set_text(&snap.backlog);
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_icon(Some(icons::tray(snap.glyph)));
        let _ = tray.set_tooltip(Some(format!("tuldMi - {}", snap.status)));
    }
}

fn snapshot(s: &CaptureStatus) -> Snapshot {
    let (glyph, status, toggle) = match s.state {
        CaptureState::Recording => (Glyph::Recording, "Recording", "Pause recording"),
        CaptureState::Starting => (Glyph::Recording, "Starting...", "Pause recording"),
        CaptureState::Paused => (Glyph::Paused, "Paused", "Resume recording"),
        CaptureState::Stopped => (Glyph::Idle, "Not recording", "Start recording"),
        CaptureState::Error => (Glyph::Error, "Audio devices unavailable", "Pause recording"),
    };
    let source_line = |src: Source, label: &str| {
        let Some(x) = s.sources.iter().find(|x| x.source == src) else { return format!("{label}: off") };
        let state = match x.state {
            SourceState::Active => "on",
            SourceState::Disabled => "off",
            SourceState::Unavailable => "unavailable",
            SourceState::Error => "error",
        };
        match (&x.device_name, x.state) {
            (Some(name), SourceState::Active) => format!("{label}: {}", truncate(name, 34)),
            _ => format!("{label}: {state}"),
        }
    };
    let backlog = match (s.transcriber, s.backlog) {
        (TranscriberState::NoModel, n) if n > 0 => format!("{n} waiting - no model installed"),
        (TranscriberState::NoModel, _) => "No transcription model".to_string(),
        (TranscriberState::Loading, _) => "Loading transcription model...".to_string(),
        (TranscriberState::Error, _) => "Transcription error".to_string(),
        (_, 0) => "Transcripts up to date".to_string(),
        (_, n) => format!("Transcribing - {n} waiting"),
    };
    Snapshot {
        glyph,
        status: status.to_string(),
        toggle: toggle.to_string(),
        mic: source_line(Source::Mic, "Microphone"),
        system: source_line(Source::System, "System audio"),
        backlog,
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(max - 3).collect::<String>())
    }
}
