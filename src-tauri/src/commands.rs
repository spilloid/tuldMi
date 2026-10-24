//! Tauri commands. Every command is `async` so it runs off the main (UI) thread; errors are
//! user-presentable strings. Contract: `docs/IPC.md`.

use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;
use tuldmi_engine::engine::ExportFormat;
use tuldmi_engine::types::*;
use uuid::Uuid;

use crate::AppState;

type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn id(s: &str) -> Res<Uuid> {
    Uuid::parse_str(s).map_err(|_| "Unknown item.".to_string())
}

#[tauri::command]
pub async fn get_status(state: State<'_, AppState>) -> Res<CaptureStatus> {
    Ok(state.engine.status())
}

#[tauri::command]
pub async fn set_paused(state: State<'_, AppState>, paused: bool) -> Res<CaptureStatus> {
    state.engine.set_paused(paused).map_err(err)
}

#[tauri::command]
pub async fn set_monitoring(state: State<'_, AppState>, enabled: bool) -> Res<CaptureStatus> {
    state.engine.set_monitoring(enabled).map_err(err)
}

#[tauri::command]
pub async fn list_devices(state: State<'_, AppState>) -> Res<Vec<DeviceInfo>> {
    state.engine.list_devices().map_err(err)
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Res<Settings> {
    Ok(state.engine.settings())
}

#[tauri::command]
pub async fn update_settings(app: AppHandle, state: State<'_, AppState>, settings: Settings) -> Res<Settings> {
    let old = state.engine.settings();
    let saved = state.engine.update_settings(settings).map_err(err)?;
    if old.launch_at_login != saved.launch_at_login
        && let Err(e) = crate::sync_autostart(&app, saved.launch_at_login)
    {
        // Don't let the setting claim something Windows didn't do.
        let mut reverted = saved.clone();
        reverted.launch_at_login = old.launch_at_login;
        let _ = state.engine.update_settings(reverted);
        return Err(e);
    }
    if old.pause_shortcut != saved.pause_shortcut
        && let Err(e) = crate::register_shortcut(&app, Some(&old.pause_shortcut), &saved.pause_shortcut)
    {
        // Restore the previous working shortcut so the user isn't left without one.
        let _ = crate::register_shortcut(&app, None, &old.pause_shortcut);
        let mut reverted = saved.clone();
        reverted.pause_shortcut = old.pause_shortcut.clone();
        let _ = state.engine.update_settings(reverted);
        return Err(e);
    }
    Ok(saved)
}

#[tauri::command]
pub async fn list_events(state: State<'_, AppState>, before: Option<i64>, before_id: Option<String>, limit: u32) -> Res<Vec<SpeechEvent>> {
    let cursor = match (before, before_id) {
        (Some(at), Some(id)) => Some((at, self::id(&id)?)),
        // A timestamp alone means "older than this instant"; the nil UUID sorts first, so
        // every event at exactly `at` is excluded and nothing before it is skipped.
        (Some(at), None) => Some((at, Uuid::nil())),
        _ => None,
    };
    state.engine.store().list_events_page(cursor, limit.clamp(1, 500)).map_err(err)
}

#[tauri::command]
pub async fn get_event(state: State<'_, AppState>, id: String) -> Res<Option<SpeechEvent>> {
    state.engine.store().get_event(self::id(&id)?).map_err(err)
}

#[tauri::command]
pub async fn search(state: State<'_, AppState>, query: SearchQuery) -> Res<Vec<SearchHit>> {
    state.engine.search(&query).map_err(err)
}

#[tauri::command]
pub async fn retry_event(state: State<'_, AppState>, id: String) -> Res<bool> {
    state.engine.retry_event(self::id(&id)?).map_err(err)
}

#[tauri::command]
pub async fn delete_event(state: State<'_, AppState>, id: String) -> Res<()> {
    state.engine.delete_event(self::id(&id)?).map_err(err)
}

#[tauri::command]
pub async fn delete_event_audio(state: State<'_, AppState>, id: String) -> Res<()> {
    state.engine.delete_event_audio(self::id(&id)?).map_err(err)
}

#[tauri::command]
pub async fn delete_all_history(state: State<'_, AppState>) -> Res<()> {
    state.engine.delete_all_history().map_err(err)
}

#[tauri::command]
pub async fn list_models(state: State<'_, AppState>) -> Res<Vec<ModelInfo>> {
    state.engine.list_models().map_err(err)
}

#[tauri::command]
pub async fn download_model(state: State<'_, AppState>, model_id: String) -> Res<()> {
    state.engine.download_model(&model_id).map_err(err)
}

#[tauri::command]
pub async fn cancel_model_download(state: State<'_, AppState>, model_id: String) -> Res<()> {
    state.engine.cancel_model_download(&model_id);
    Ok(())
}

#[tauri::command]
pub async fn delete_model(state: State<'_, AppState>, model_id: String) -> Res<()> {
    state.engine.delete_model(&model_id).map_err(err)
}

#[tauri::command]
pub async fn storage_stats(state: State<'_, AppState>) -> Res<StorageStats> {
    state.engine.storage_stats().map_err(err)
}

#[tauri::command]
pub async fn open_data_dir(app: AppHandle, state: State<'_, AppState>) -> Res<()> {
    app.opener().open_path(state.data_dir.to_string_lossy(), None::<&str>).map_err(err)
}

#[tauri::command]
pub async fn export_transcripts(app: AppHandle, state: State<'_, AppState>, format: String, from: Option<i64>, to: Option<i64>) -> Res<String> {
    let format = match format.as_str() {
        "markdown" => ExportFormat::Markdown,
        "json" => ExportFormat::Json,
        _ => return Err("Unknown export format.".into()),
    };
    let docs = app.path().document_dir().map_err(err)?;
    let path = state.engine.export(format, from, to, &docs.join("tuldMi Exports")).map_err(err)?;
    Ok(path.to_string_lossy().into_owned())
}

/// Only paths inside the data dir or the exports folder may be revealed.
#[tauri::command]
pub async fn reveal_path(app: AppHandle, state: State<'_, AppState>, path: String) -> Res<()> {
    let p = PathBuf::from(&path);
    let exports = app.path().document_dir().map_err(err)?.join("tuldMi Exports");
    let canon = p.canonicalize().map_err(|_| "That file no longer exists.".to_string())?;
    let allowed = [state.data_dir.clone(), exports].iter().filter_map(|d| d.canonicalize().ok()).any(|d| canon.starts_with(d));
    if !allowed {
        return Err("Not allowed.".into());
    }
    app.opener().reveal_item_in_dir(canon).map_err(err)
}

#[tauri::command]
pub async fn quit_app(app: AppHandle) -> Res<()> {
    app.exit(0);
    Ok(())
}
