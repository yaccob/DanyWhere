use crate::managers::{
    history::{HistoryEntry, HistoryManager, PaginatedHistory, RecordingStorageUsage},
    transcription_jobs::{ReprocessMode, TranscriptionJobManager},
};
use crate::settings::RecordingStorageLimit;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, State};

#[tauri::command]
#[specta::specta]
pub async fn get_history_entries(
    _app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    cursor: Option<i64>,
    limit: Option<usize>,
) -> Result<PaginatedHistory, String> {
    history_manager
        .get_history_entries(cursor, limit)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn toggle_history_entry_saved(
    _app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    id: i64,
) -> Result<(), String> {
    history_manager
        .toggle_saved_status(id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn get_audio_file_path(
    _app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    file_name: String,
) -> Result<String, String> {
    let path = history_manager.get_audio_file_path(&file_name);
    path.to_str()
        .ok_or_else(|| "Invalid file path".to_string())
        .map(|s| s.to_string())
}

/// A WAV of the recording for the history player: Opus recordings are
/// decoded into a short-lived copy, because the webview of older macOS
/// versions can't play Ogg Opus.
#[tauri::command]
#[specta::specta]
pub async fn get_recording_playback_path(
    history_manager: State<'_, Arc<HistoryManager>>,
    file_name: String,
) -> Result<String, String> {
    let history_manager = Arc::clone(&history_manager);
    let path =
        tauri::async_runtime::spawn_blocking(move || history_manager.playback_wav_path(&file_name))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
    path.to_str()
        .ok_or_else(|| "Invalid file path".to_string())
        .map(|s| s.to_string())
}

/// What History's recordings take, for the storage setting.
#[tauri::command]
#[specta::specta]
pub async fn get_recording_storage_usage(
    history_manager: State<'_, Arc<HistoryManager>>,
) -> Result<RecordingStorageUsage, String> {
    history_manager.storage_usage().map_err(|e| e.to_string())
}

/// Set how much space recordings may take, and apply it right away.
#[tauri::command]
#[specta::specta]
pub async fn update_recording_storage_limit(
    app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    limit: RecordingStorageLimit,
) -> Result<(), String> {
    let mut settings = crate::settings::get_settings(&app);
    settings.recording_storage_limit = limit;
    crate::settings::write_settings(&app, settings);

    history_manager
        .cleanup_old_entries()
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn delete_history_entry(
    _app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    jobs: State<'_, Arc<TranscriptionJobManager>>,
    id: i64,
) -> Result<(), String> {
    jobs.cancel(id);
    history_manager
        .delete_entry(id)
        .await
        .map_err(|e| e.to_string())
}

/// Queues the entry to be transcribed again with the current model. Its
/// progress and outcome arrive as `TranscriptionJob` events.
#[tauri::command]
#[specta::specta]
pub fn retry_history_entry_transcription(jobs: State<'_, Arc<TranscriptionJobManager>>, id: i64) {
    jobs.retry(id);
}

/// Queues the entry to be transcribed or post-processed again, as `mode`
/// says. Its progress and outcome arrive as `TranscriptionJob` events.
#[tauri::command]
#[specta::specta]
pub fn reprocess_history_entry(
    jobs: State<'_, Arc<TranscriptionJobManager>>,
    id: i64,
    mode: ReprocessMode,
) {
    jobs.reprocess(id, mode);
}

/// Stops the entry's background transcription: a queued one is dropped, a
/// running one stops after the chunk it is on. The entry keeps the text it
/// had before.
#[tauri::command]
#[specta::specta]
pub fn cancel_history_entry_transcription(jobs: State<'_, Arc<TranscriptionJobManager>>, id: i64) {
    jobs.cancel(id);
}

/// Adds an audio or video file to History and queues its transcription.
/// Fails without adding anything if the file has no audio Handy can read.
#[tauri::command]
#[specta::specta]
pub async fn transcribe_file(
    jobs: State<'_, Arc<TranscriptionJobManager>>,
    path: String,
) -> Result<HistoryEntry, String> {
    let jobs = Arc::clone(&jobs);
    tauri::async_runtime::spawn_blocking(move || jobs.transcribe_file(PathBuf::from(path)))
        .await
        .map_err(|e| format!("Failed to read the file: {}", e))?
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn update_history_limit(
    app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    limit: usize,
) -> Result<(), String> {
    let mut settings = crate::settings::get_settings(&app);
    settings.history_limit = limit;
    crate::settings::write_settings(&app, settings);

    history_manager
        .cleanup_old_entries()
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn update_recording_retention_period(
    app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    period: String,
) -> Result<(), String> {
    use crate::settings::RecordingRetentionPeriod;

    let retention_period = match period.as_str() {
        "never" => RecordingRetentionPeriod::Never,
        "preserve_limit" => RecordingRetentionPeriod::PreserveLimit,
        "days3" => RecordingRetentionPeriod::Days3,
        "weeks2" => RecordingRetentionPeriod::Weeks2,
        "months3" => RecordingRetentionPeriod::Months3,
        _ => return Err(format!("Invalid retention period: {}", period)),
    };

    let mut settings = crate::settings::get_settings(&app);
    settings.recording_retention_period = retention_period;
    crate::settings::write_settings(&app, settings);

    history_manager
        .cleanup_old_entries()
        .map_err(|e| e.to_string())?;

    Ok(())
}
