//! tuldMi desktop shell: owns the engine, bridges its events to the webview, and provides the
//! tray, the global pause shortcut, close-to-tray behaviour, and launch-at-login.

mod commands;
mod events;
mod icons;
mod tray;
mod window;

use std::path::PathBuf;
use std::sync::Arc;

use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager, RunEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as _};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tuldmi_engine::engine::{Engine, EngineConfig};
use tuldmi_engine::types::CaptureState;

pub const VAD_MODEL: &str = "ggml-silero-v6.2.0.bin";

pub struct AppState {
    pub engine: Arc<Engine>,
    pub data_dir: PathBuf,
    /// Keeps the non-blocking log writer alive for the process lifetime.
    _log_guard: tracing_appender::non_blocking::WorkerGuard,
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| window::show_main(app)))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--minimized"])))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        toggle_pause(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            if let Err(e) = setup(app.handle()) {
                let msg = format!("tuldMi could not start.\n\n{e}");
                app.dialog().message(msg).title("tuldMi").kind(MessageDialogKind::Error).blocking_show();
                std::process::exit(1);
            }
            Ok(())
        })
        .on_window_event(window::on_event)
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::set_paused,
            commands::set_monitoring,
            commands::list_devices,
            commands::get_settings,
            commands::update_settings,
            commands::list_events,
            commands::get_event,
            commands::search,
            commands::retry_event,
            commands::delete_event,
            commands::delete_event_audio,
            commands::delete_all_history,
            commands::list_models,
            commands::download_model,
            commands::cancel_model_download,
            commands::delete_model,
            commands::storage_stats,
            commands::open_data_dir,
            commands::export_transcripts,
            commands::reveal_path,
            commands::quit_app,
        ])
        .build(tauri::generate_context!())
        .unwrap_or_else(|e| {
            eprintln!("tuldMi failed to build: {e}");
            std::process::exit(1);
        });

    app.run(|app, event| match event {
        // Windows are hidden, never closed, so this only fires for an explicit exit; keep the
        // process alive otherwise (e.g. if a webview crashes) — capture runs in the tray.
        RunEvent::ExitRequested { code: None, api, .. } => api.prevent_exit(),
        RunEvent::Exit => shutdown(app),
        _ => {}
    });
}

fn setup(app: &AppHandle) -> Result<(), String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&data_dir).map_err(|e| format!("cannot create {}: {e}", data_dir.display()))?;

    let log_dir = data_dir.join("logs");
    let appender = tracing_appender::rolling::daily(&log_dir, "tuldmi.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter = tracing_subscriber::EnvFilter::try_from_env("TULDMI_LOG").unwrap_or_else(|_| "info".into());
    let _ = tracing_subscriber::fmt().with_env_filter(filter).with_writer(writer).with_ansi(false).try_init();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "tuldMi starting");

    let vad_model_path = app.path().resolve(VAD_MODEL, BaseDirectory::Resource).ok().filter(|p| p.exists());
    if vad_model_path.is_none() {
        tracing::warn!("bundled Silero VAD model not found; using energy VAD");
    }
    let sink = Arc::new(events::TauriSink::new(app.clone()));
    let engine = Engine::start(EngineConfig { data_dir: data_dir.clone(), vad_model_path, suppress_autostart: false }, sink).map_err(|e| e.to_string())?;
    let engine = Arc::new(engine);
    let settings = engine.settings();
    app.manage(AppState { engine: engine.clone(), data_dir, _log_guard: guard });

    tray::create(app).map_err(|e| e.to_string())?;
    tray::update(app, &engine.status());

    if let Err(e) = register_shortcut(app, None, &settings.pause_shortcut) {
        tracing::warn!(error = %e, "pause shortcut not registered");
    }
    sync_autostart(app, settings.launch_at_login);

    let minimized = std::env::args().any(|a| a == "--minimized");
    if !(minimized && settings.onboarding_complete) {
        window::show_main(app);
    }
    Ok(())
}

pub fn engine(app: &AppHandle) -> Arc<Engine> {
    app.state::<AppState>().engine.clone()
}

/// Global shortcut / tray action. Before onboarding is done there is nothing to pause, so it
/// just brings the window forward.
pub fn toggle_pause(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else { return };
    let engine = state.engine.clone();
    if !engine.settings().onboarding_complete {
        window::show_main(app);
        return;
    }
    let recording = matches!(engine.status().state, CaptureState::Recording | CaptureState::Starting | CaptureState::Error);
    if let Err(e) = engine.set_paused(recording) {
        tracing::warn!(error = %e, "toggle pause failed");
    }
}

pub fn register_shortcut(app: &AppHandle, old: Option<&str>, new: &str) -> Result<(), String> {
    let gs = app.global_shortcut();
    if let Some(old) = old.filter(|o| !o.is_empty()) {
        let _ = gs.unregister(old);
    }
    if new.trim().is_empty() {
        return Ok(());
    }
    gs.register(new).map_err(|e| format!("Could not register shortcut {new}: {e}"))
}

pub fn sync_autostart(app: &AppHandle, enabled: bool) {
    let al = app.autolaunch();
    let current = al.is_enabled().unwrap_or(false);
    let result = match (enabled, current) {
        (true, false) => al.enable(),
        (false, true) => al.disable(),
        _ => Ok(()),
    };
    if let Err(e) = result {
        tracing::warn!(error = %e, "launch-at-login change failed");
    }
}

fn shutdown(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        state.engine.shutdown();
    }
    tracing::info!("tuldMi stopped");
}
