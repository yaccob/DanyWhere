use anyhow::{anyhow, Result};
use chrono::{DateTime, Local, Utc};
use log::{debug, error, info, warn};
use rusqlite::{params, Connection, OptionalExtension};
use rusqlite_migration::{MigrationDefinitionError, Migrations, M};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use tauri_specta::Event;

use crate::language_detection::{LanguageDecision, LanguageSource};

/// Database migrations for transcription history.
/// Each migration is applied in order. The library tracks which migrations
/// have been applied using SQLite's user_version pragma.
///
/// Note: For users upgrading from tauri-plugin-sql, migrate_from_tauri_plugin_sql()
/// converts the old _sqlx_migrations table tracking to the user_version pragma,
/// ensuring migrations don't re-run on existing databases.
static MIGRATIONS: &[M] = &[
    M::up(
        "CREATE TABLE IF NOT EXISTS transcription_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file_name TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            saved BOOLEAN NOT NULL DEFAULT 0,
            title TEXT NOT NULL,
            transcription_text TEXT NOT NULL
        );",
    ),
    M::up("ALTER TABLE transcription_history ADD COLUMN post_processed_text TEXT;"),
    M::up("ALTER TABLE transcription_history ADD COLUMN post_process_prompt TEXT;"),
    M::up("ALTER TABLE transcription_history ADD COLUMN post_process_requested BOOLEAN NOT NULL DEFAULT 0;"),
];

/// Nullable columns added outside [`MIGRATIONS`]; see
/// [`HistoryManager::apply_migrations`].
/// `recording_removed` holds when the storage limit deleted the entry's
/// recording (Unix seconds); the transcript stays.
const ADDED_COLUMNS: &[&str] = &[
    "source_name",
    "language",
    "language_source",
    "recording_removed",
];

/// Every column a [`HistoryEntry`] is read from.
const ENTRY_COLUMNS: &str = "id, file_name, timestamp, saved, title, transcription_text, \
    post_processed_text, post_process_prompt, post_process_requested, source_name, language, \
    language_source, recording_removed";

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct PaginatedHistory {
    pub entries: Vec<HistoryEntry>,
    pub has_more: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(tag = "action")]
pub enum HistoryUpdatePayload {
    #[serde(rename = "added")]
    Added { entry: HistoryEntry },
    #[serde(rename = "updated")]
    Updated { entry: HistoryEntry },
    #[serde(rename = "deleted")]
    Deleted { id: i64 },
    #[serde(rename = "toggled")]
    Toggled { id: i64 },
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct HistoryEntry {
    pub id: i64,
    pub file_name: String,
    pub timestamp: i64,
    pub saved: bool,
    pub title: String,
    pub transcription_text: String,
    pub post_processed_text: Option<String>,
    pub post_process_prompt: Option<String>,
    pub post_process_requested: bool,
    /// The name of the file this entry was transcribed from; `None` for a
    /// dictation.
    pub source_name: Option<String>,
    /// The language it was transcribed in, when known.
    pub language: Option<String>,
    /// How that language was chosen; `None` before it was recorded.
    pub language_source: Option<LanguageSource>,
    /// The storage limit deleted the recording; the transcript stays, but
    /// there is nothing to play or transcribe again.
    pub recording_removed: bool,
}

/// One recording as the storage limit sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredRecording {
    pub id: i64,
    pub bytes: u64,
    /// Starred entries are never deleted automatically.
    pub saved: bool,
    /// Transcribed files are never deleted automatically.
    pub is_file: bool,
    pub timestamp: i64,
}

/// What History's recordings take, for the storage setting.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RecordingStorageUsage {
    /// All recordings.
    pub total_bytes: u64,
    /// Starred entries and transcribed files, which are never deleted
    /// automatically.
    pub protected_bytes: u64,
}

/// The recordings to delete so the rest fit in `limit` bytes (`None`:
/// unlimited): the oldest first, never a starred entry or a transcribed
/// file. These still count, so when they alone exceed the limit every
/// other recording goes, except the newest: what was just dictated can
/// always be played and transcribed again, even if that goes over the
/// limit by one recording.
pub fn recordings_to_remove(recordings: &[StoredRecording], limit: Option<u64>) -> Vec<i64> {
    let Some(limit) = limit else {
        return Vec::new();
    };
    let mut total: u64 = recordings.iter().map(|r| r.bytes).sum();
    if total <= limit {
        return Vec::new();
    }

    let mut removable: Vec<&StoredRecording> = recordings
        .iter()
        .filter(|r| !r.saved && !r.is_file)
        .collect();
    removable.sort_by_key(|r| (r.timestamp, r.id));
    // The newest recording is never removed.
    removable.pop();

    let mut remove = Vec::new();
    for recording in removable {
        if total <= limit {
            break;
        }
        total = total.saturating_sub(recording.bytes);
        remove.push(recording.id);
    }
    remove
}

/// How much the recordings take, and how much of it is never deleted.
pub fn storage_usage(recordings: &[StoredRecording]) -> RecordingStorageUsage {
    recordings
        .iter()
        .fold(RecordingStorageUsage::default(), |mut usage, r| {
            usage.total_bytes += r.bytes;
            if r.saved || r.is_file {
                usage.protected_bytes += r.bytes;
            }
            usage
        })
}

pub struct HistoryManager {
    app_handle: AppHandle,
    recordings_dir: PathBuf,
    /// Decoded WAV copies of Opus recordings for the player.
    playback_dir: PathBuf,
    db_path: PathBuf,
}

/// A transcription's language as its two columns.
fn split_language(language: Option<LanguageDecision>) -> (Option<String>, Option<LanguageSource>) {
    match language {
        Some(decision) => (decision.language, Some(decision.source)),
        None => (None, None),
    }
}

impl HistoryManager {
    pub fn new(app_handle: &AppHandle) -> Result<Self> {
        // Create recordings directory in app data dir
        let app_data_dir = crate::portable::app_data_dir(app_handle)?;
        let recordings_dir = app_data_dir.join("recordings");
        let playback_dir = app_data_dir.join("recording-playback");
        let db_path = app_data_dir.join("history.db");

        // Ensure recordings directory exists
        if !recordings_dir.exists() {
            fs::create_dir_all(&recordings_dir)?;
            debug!("Created recordings directory: {:?}", recordings_dir);
        }

        let manager = Self {
            app_handle: app_handle.clone(),
            recordings_dir,
            playback_dir,
            db_path,
        };

        // Initialize database and run migrations synchronously
        manager.init_database()?;

        Ok(manager)
    }

    fn init_database(&self) -> Result<()> {
        info!("Initializing database at {:?}", self.db_path);

        let mut conn = Connection::open(&self.db_path)?;

        // Handle migration from tauri-plugin-sql to rusqlite_migration
        // tauri-plugin-sql used _sqlx_migrations table, rusqlite_migration uses user_version pragma
        self.migrate_from_tauri_plugin_sql(&conn)?;

        // Validate migrations in debug builds
        #[cfg(debug_assertions)]
        Migrations::new(MIGRATIONS.to_vec())
            .validate()
            .expect("Invalid migrations");

        // Get current version before migration
        let version_before: i32 =
            conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        debug!("Database version before migration: {}", version_before);

        Self::apply_migrations(&mut conn)?;

        // Get version after migration
        let version_after: i32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

        if version_after > version_before {
            info!(
                "Database migrated from version {} to {}",
                version_before, version_after
            );
        } else {
            debug!("Database already at latest version {}", version_after);
        }

        Ok(())
    }

    /// Bring the schema up to date. A newer Handy's database opens as is:
    /// migrations only add columns, and every query names its columns.
    fn apply_migrations(conn: &mut Connection) -> Result<()> {
        match Migrations::new(MIGRATIONS.to_vec()).to_latest(conn) {
            Err(rusqlite_migration::Error::MigrationDefinition(
                MigrationDefinitionError::DatabaseTooFarAhead,
            )) => {
                warn!("history.db is from a newer version of Handy; opening it without migrating")
            }
            other => other?,
        }

        // Not a migration: a higher user_version makes released builds refuse
        // the database, while an extra nullable column is invisible to them.
        for column in ADDED_COLUMNS {
            if !Self::has_column(conn, column)? {
                conn.execute(
                    &format!("ALTER TABLE transcription_history ADD COLUMN {column} TEXT"),
                    [],
                )?;
                info!("Added the {column} column to transcription history");
            }
        }

        Ok(())
    }

    fn has_column(conn: &Connection, name: &str) -> Result<bool> {
        Ok(conn.query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('transcription_history') WHERE name = ?1",
            [name],
            |row| row.get(0),
        )?)
    }

    /// Migrate from tauri-plugin-sql's migration tracking to rusqlite_migration's.
    /// tauri-plugin-sql used a _sqlx_migrations table, while rusqlite_migration uses
    /// SQLite's user_version pragma. This function checks if the old system was in use
    /// and sets the user_version accordingly so migrations don't re-run.
    fn migrate_from_tauri_plugin_sql(&self, conn: &Connection) -> Result<()> {
        // Check if the old _sqlx_migrations table exists
        let has_sqlx_migrations: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='_sqlx_migrations'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if !has_sqlx_migrations {
            return Ok(());
        }

        // Check current user_version
        let current_version: i32 =
            conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

        if current_version > 0 {
            // Already migrated to rusqlite_migration system
            return Ok(());
        }

        // Get the highest version from the old migrations table
        let old_version: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations WHERE success = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if old_version > 0 {
            info!(
                "Migrating from tauri-plugin-sql (version {}) to rusqlite_migration",
                old_version
            );

            // Set user_version to match the old migration state
            conn.pragma_update(None, "user_version", old_version)?;

            // Optionally drop the old migrations table (keeping it doesn't hurt)
            // conn.execute("DROP TABLE IF EXISTS _sqlx_migrations", [])?;

            info!(
                "Migration tracking converted: user_version set to {}",
                old_version
            );
        }

        Ok(())
    }

    fn get_connection(&self) -> Result<Connection> {
        Ok(Connection::open(&self.db_path)?)
    }

    fn map_history_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
        Ok(HistoryEntry {
            id: row.get("id")?,
            file_name: row.get("file_name")?,
            timestamp: row.get("timestamp")?,
            saved: row.get("saved")?,
            title: row.get("title")?,
            transcription_text: row.get("transcription_text")?,
            post_processed_text: row.get("post_processed_text")?,
            post_process_prompt: row.get("post_process_prompt")?,
            post_process_requested: row.get("post_process_requested")?,
            source_name: row.get("source_name")?,
            language: row.get("language")?,
            language_source: row
                .get::<_, Option<String>>("language_source")?
                .as_deref()
                .and_then(LanguageSource::parse),
            recording_removed: row.get::<_, Option<String>>("recording_removed")?.is_some(),
        })
    }

    pub fn recordings_dir(&self) -> &std::path::Path {
        &self.recordings_dir
    }

    /// Save a new history entry to the database.
    /// The recording should already have been written to the recordings directory.
    pub fn save_entry(
        &self,
        file_name: String,
        transcription_text: String,
        post_process_requested: bool,
        post_processed_text: Option<String>,
        post_process_prompt: Option<String>,
        language: Option<LanguageDecision>,
    ) -> Result<HistoryEntry> {
        let (language, language_source) = split_language(language);
        let entry = self.insert_entry(HistoryEntry {
            id: 0,
            file_name,
            timestamp: Utc::now().timestamp(),
            saved: false,
            title: String::new(),
            transcription_text,
            post_processed_text,
            post_process_prompt,
            post_process_requested,
            source_name: None,
            language,
            language_source,
            recording_removed: false,
        })?;

        self.cleanup_old_entries()?;

        Ok(entry)
    }

    /// Save the entry for a file whose audio is already at `file_name`.
    /// The storage limit never removes it (only deleting it does); never
    /// post-processed.
    pub fn save_file_entry(&self, file_name: String, source_name: String) -> Result<HistoryEntry> {
        let entry = self.insert_entry(HistoryEntry {
            id: 0,
            file_name,
            timestamp: Utc::now().timestamp(),
            saved: false,
            title: String::new(),
            transcription_text: String::new(),
            post_processed_text: None,
            post_process_prompt: None,
            post_process_requested: false,
            source_name: Some(source_name),
            language: None,
            language_source: None,
            recording_removed: false,
        })?;

        // The file counts toward the storage limit, so older dictations may
        // have to make room; the file itself is never removed.
        if let Err(e) = self.cleanup_old_entries() {
            warn!("Failed to apply the storage limit: {}", e);
        }

        Ok(entry)
    }

    /// Insert `entry`, filling in its id and title, and announce it.
    fn insert_entry(&self, mut entry: HistoryEntry) -> Result<HistoryEntry> {
        entry.title = self.format_timestamp_title(entry.timestamp);

        let conn = self.get_connection()?;
        conn.execute(
            "INSERT INTO transcription_history (
                file_name,
                timestamp,
                saved,
                title,
                transcription_text,
                post_processed_text,
                post_process_prompt,
                post_process_requested,
                source_name,
                language,
                language_source
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                &entry.file_name,
                entry.timestamp,
                entry.saved,
                &entry.title,
                &entry.transcription_text,
                &entry.post_processed_text,
                &entry.post_process_prompt,
                entry.post_process_requested,
                &entry.source_name,
                &entry.language,
                entry.language_source.map(LanguageSource::as_str),
            ],
        )?;
        entry.id = conn.last_insert_rowid();

        debug!("Saved history entry with id {}", entry.id);

        // Emit typed event for real-time frontend updates
        if let Err(e) = (HistoryUpdatePayload::Added {
            entry: entry.clone(),
        })
        .emit(&self.app_handle)
        {
            error!("Failed to emit history-updated event: {}", e);
        }

        Ok(entry)
    }

    /// Update an existing history entry with new transcription results (used
    /// by retry). `post_process_requested` is whether the entry counts as
    /// post-processed from now on, so the next retry post-processes too.
    pub fn update_transcription(
        &self,
        id: i64,
        transcription_text: String,
        post_processed_text: Option<String>,
        post_process_prompt: Option<String>,
        language: Option<LanguageDecision>,
        post_process_requested: bool,
    ) -> Result<HistoryEntry> {
        let conn = self.get_connection()?;
        let entry = Self::update_transcription_with_conn(
            &conn,
            id,
            transcription_text,
            post_processed_text,
            post_process_prompt,
            language,
            post_process_requested,
        )?;
        debug!("Updated transcription for history entry {}", id);
        self.emit_entry(&entry);
        Ok(entry)
    }

    fn update_transcription_with_conn(
        conn: &Connection,
        id: i64,
        transcription_text: String,
        post_processed_text: Option<String>,
        post_process_prompt: Option<String>,
        language: Option<LanguageDecision>,
        post_process_requested: bool,
    ) -> Result<HistoryEntry> {
        let (language, language_source) = split_language(language);
        let updated = conn.execute(
            "UPDATE transcription_history
             SET transcription_text = ?1,
                 post_processed_text = ?2,
                 post_process_prompt = ?3,
                 language = ?4,
                 language_source = ?5,
                 post_process_requested = ?6
             WHERE id = ?7",
            params![
                transcription_text,
                post_processed_text,
                post_process_prompt,
                language,
                language_source.map(LanguageSource::as_str),
                post_process_requested,
                id
            ],
        )?;

        if updated == 0 {
            return Err(anyhow!("History entry {} not found", id));
        }
        Self::entry_with_conn(conn, id)
    }

    /// Store a new post-processing of the entry's transcript, which stays as
    /// it is. The entry counts as post-processed from now on.
    pub fn update_post_processing(
        &self,
        id: i64,
        post_processed_text: String,
        post_process_prompt: Option<String>,
    ) -> Result<HistoryEntry> {
        let conn = self.get_connection()?;
        let entry = Self::update_post_processing_with_conn(
            &conn,
            id,
            post_processed_text,
            post_process_prompt,
        )?;
        debug!("Updated post-processing for history entry {}", id);
        self.emit_entry(&entry);
        Ok(entry)
    }

    fn update_post_processing_with_conn(
        conn: &Connection,
        id: i64,
        post_processed_text: String,
        post_process_prompt: Option<String>,
    ) -> Result<HistoryEntry> {
        let updated = conn.execute(
            "UPDATE transcription_history
             SET post_processed_text = ?1,
                 post_process_prompt = ?2,
                 post_process_requested = 1
             WHERE id = ?3",
            params![post_processed_text, post_process_prompt, id],
        )?;
        if updated == 0 {
            return Err(anyhow!("History entry {} not found", id));
        }
        Self::entry_with_conn(conn, id)
    }

    fn entry_with_conn(conn: &Connection, id: i64) -> Result<HistoryEntry> {
        Ok(conn.query_row(
            &format!("SELECT {ENTRY_COLUMNS} FROM transcription_history WHERE id = ?1"),
            params![id],
            Self::map_history_entry,
        )?)
    }

    /// Tell the frontend that `entry` changed.
    fn emit_entry(&self, entry: &HistoryEntry) {
        if let Err(e) = (HistoryUpdatePayload::Updated {
            entry: entry.clone(),
        })
        .emit(&self.app_handle)
        {
            error!("Failed to emit history-updated event: {}", e);
        }
    }

    /// Keep the recordings within the storage limit: the oldest that are
    /// neither starred nor transcribed files go first. Their entries and
    /// transcripts stay; the entries say the recording was removed.
    pub fn cleanup_old_entries(&self) -> Result<()> {
        let limit = crate::settings::get_recording_storage_limit(&self.app_handle).bytes();
        let conn = self.get_connection()?;
        let recordings = Self::stored_recordings_with_conn(&conn, &self.recordings_dir)?;
        let remove = recordings_to_remove(&recordings, limit);
        if remove.is_empty() {
            return Ok(());
        }

        Self::remove_recordings_with_conn(
            &conn,
            &self.recordings_dir,
            &remove,
            Utc::now().timestamp(),
        )?;
        info!(
            "Removed {} recording(s) to stay within the storage limit",
            remove.len()
        );
        for id in remove {
            self.emit_updated(&conn, id);
        }
        Ok(())
    }

    /// What the recordings take now.
    pub fn storage_usage(&self) -> Result<RecordingStorageUsage> {
        let conn = self.get_connection()?;
        Ok(storage_usage(&Self::stored_recordings_with_conn(
            &conn,
            &self.recordings_dir,
        )?))
    }

    /// Convert the WAV recordings from before Opus, then apply the storage
    /// limit to what they take now. Runs once per start, in the background.
    pub fn convert_wav_recordings(&self) -> Result<()> {
        let conn = self.get_connection()?;
        let converted = Self::convert_wav_recordings_with_conn(&conn, &self.recordings_dir)?;
        if !converted.is_empty() {
            info!("Converted {} WAV recording(s) to Opus", converted.len());
        }
        for id in converted {
            self.emit_updated(&conn, id);
        }
        self.cleanup_old_entries()
    }

    /// A WAV of the recording `file_name` for the history player.
    pub fn playback_wav_path(&self, file_name: &str) -> Result<PathBuf> {
        crate::audio_toolkit::write_playback_wav(
            &self.get_audio_file_path(file_name),
            &self.playback_dir,
        )
    }

    /// Tell the frontend that entry `id` changed.
    fn emit_updated(&self, conn: &Connection, id: i64) {
        let entry = conn.query_row(
            &format!("SELECT {ENTRY_COLUMNS} FROM transcription_history WHERE id = ?1"),
            params![id],
            Self::map_history_entry,
        );
        match entry {
            Ok(entry) => {
                if let Err(e) = (HistoryUpdatePayload::Updated { entry }).emit(&self.app_handle) {
                    error!("Failed to emit history-updated event: {}", e);
                }
            }
            Err(e) => warn!("Failed to read history entry {} after a change: {}", id, e),
        }
    }

    pub async fn get_history_entries(
        &self,
        cursor: Option<i64>,
        limit: Option<usize>,
    ) -> Result<PaginatedHistory> {
        let conn = self.get_connection()?;
        let limit = limit.map(|l| l.min(100));

        let mut entries: Vec<HistoryEntry> = match (cursor, limit) {
            (Some(cursor_id), Some(lim)) => {
                let fetch_count = (lim + 1) as i64;
                let mut stmt = conn.prepare(&format!(
                    "SELECT {ENTRY_COLUMNS}
                     FROM transcription_history
                     WHERE id < ?1
                     ORDER BY id DESC
                     LIMIT ?2"
                ))?;
                let result = stmt
                    .query_map(params![cursor_id, fetch_count], Self::map_history_entry)?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                result
            }
            (None, Some(lim)) => {
                let fetch_count = (lim + 1) as i64;
                let mut stmt = conn.prepare(&format!(
                    "SELECT {ENTRY_COLUMNS}
                     FROM transcription_history
                     ORDER BY id DESC
                     LIMIT ?1"
                ))?;
                let result = stmt
                    .query_map(params![fetch_count], Self::map_history_entry)?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                result
            }
            (_, None) => {
                let mut stmt = conn.prepare(&format!(
                    "SELECT {ENTRY_COLUMNS}
                     FROM transcription_history
                     ORDER BY id DESC"
                ))?;
                let result = stmt
                    .query_map([], Self::map_history_entry)?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                result
            }
        };

        let has_more = limit.is_some_and(|lim| entries.len() > lim);
        if has_more {
            entries.pop();
        }

        Ok(PaginatedHistory { entries, has_more })
    }

    #[cfg(test)]
    fn get_latest_entry_with_conn(conn: &Connection) -> Result<Option<HistoryEntry>> {
        let mut stmt = conn.prepare(&format!(
            "SELECT {ENTRY_COLUMNS}
             FROM transcription_history
             ORDER BY timestamp DESC
             LIMIT 1"
        ))?;

        let entry = stmt.query_row([], Self::map_history_entry).optional()?;
        Ok(entry)
    }

    /// Get the latest dictation with non-empty transcription text. Files are
    /// left out: a long file's transcript is never the "last transcript".
    pub fn get_latest_completed_entry(&self) -> Result<Option<HistoryEntry>> {
        let conn = self.get_connection()?;
        Self::get_latest_completed_entry_with_conn(&conn)
    }

    fn get_latest_completed_entry_with_conn(conn: &Connection) -> Result<Option<HistoryEntry>> {
        let mut stmt = conn.prepare(&format!(
            "SELECT {ENTRY_COLUMNS}
             FROM transcription_history
             WHERE transcription_text != '' AND source_name IS NULL
             ORDER BY timestamp DESC
             LIMIT 1"
        ))?;

        let entry = stmt.query_row([], Self::map_history_entry).optional()?;
        Ok(entry)
    }

    pub async fn toggle_saved_status(&self, id: i64) -> Result<()> {
        let conn = self.get_connection()?;

        // Get current saved status
        let current_saved: bool = conn.query_row(
            "SELECT saved FROM transcription_history WHERE id = ?1",
            params![id],
            |row| row.get("saved"),
        )?;

        let new_saved = !current_saved;

        conn.execute(
            "UPDATE transcription_history SET saved = ?1 WHERE id = ?2",
            params![new_saved, id],
        )?;

        debug!("Toggled saved status for entry {}: {}", id, new_saved);

        // Emit history updated event
        if let Err(e) = (HistoryUpdatePayload::Toggled { id }).emit(&self.app_handle) {
            error!("Failed to emit history-updated event: {}", e);
        }

        Ok(())
    }

    pub fn get_audio_file_path(&self, file_name: &str) -> PathBuf {
        self.recordings_dir.join(file_name)
    }

    pub async fn get_entry_by_id(&self, id: i64) -> Result<Option<HistoryEntry>> {
        let conn = self.get_connection()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {ENTRY_COLUMNS}
             FROM transcription_history
             WHERE id = ?1"
        ))?;

        let entry = stmt.query_row([id], Self::map_history_entry).optional()?;

        Ok(entry)
    }

    pub async fn delete_entry(&self, id: i64) -> Result<()> {
        let conn = self.get_connection()?;

        // Get the entry to find the file name
        if let Some(entry) = self.get_entry_by_id(id).await? {
            // Delete the audio file first
            let file_path = self.get_audio_file_path(&entry.file_name);
            if file_path.exists() {
                if let Err(e) = fs::remove_file(&file_path) {
                    error!("Failed to delete audio file {}: {}", entry.file_name, e);
                    // Continue with database deletion even if file deletion fails
                }
            }
        }

        // Delete from database
        conn.execute(
            "DELETE FROM transcription_history WHERE id = ?1",
            params![id],
        )?;

        debug!("Deleted history entry with id: {}", id);

        // Emit history updated event
        if let Err(e) = (HistoryUpdatePayload::Deleted { id }).emit(&self.app_handle) {
            error!("Failed to emit history-updated event: {}", e);
        }

        Ok(())
    }

    /// Every entry whose recording is still there, with its size. A missing
    /// file counts as nothing.
    fn stored_recordings_with_conn(
        conn: &Connection,
        recordings_dir: &Path,
    ) -> Result<Vec<StoredRecording>> {
        let mut stmt = conn.prepare(
            "SELECT id, file_name, saved, source_name, timestamp
             FROM transcription_history
             WHERE recording_removed IS NULL",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>("id")?,
                    row.get::<_, String>("file_name")?,
                    row.get::<_, bool>("saved")?,
                    row.get::<_, Option<String>>("source_name")?,
                    row.get::<_, i64>("timestamp")?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(rows
            .into_iter()
            .filter_map(|(id, file_name, saved, source_name, timestamp)| {
                let metadata = fs::metadata(recordings_dir.join(&file_name)).ok()?;
                metadata.is_file().then(|| StoredRecording {
                    id,
                    bytes: metadata.len(),
                    saved,
                    is_file: source_name.is_some(),
                    timestamp,
                })
            })
            .collect())
    }

    /// Delete the recordings of `ids` and mark them removed at `now`; the
    /// entries and their transcripts stay. A recording that can't be deleted
    /// stays marked as present.
    fn remove_recordings_with_conn(
        conn: &Connection,
        recordings_dir: &Path,
        ids: &[i64],
        now: i64,
    ) -> Result<()> {
        for id in ids {
            let file_name: Option<String> = conn
                .query_row(
                    "SELECT file_name FROM transcription_history WHERE id = ?1",
                    params![id],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(file_name) = file_name else {
                continue;
            };
            let path = recordings_dir.join(&file_name);
            if path.exists() {
                if let Err(e) = fs::remove_file(&path) {
                    error!("Failed to delete recording {}: {}", file_name, e);
                    continue;
                }
            }
            conn.execute(
                "UPDATE transcription_history SET recording_removed = ?1 WHERE id = ?2",
                params![now.to_string(), id],
            )?;
            debug!("Removed recording {} of entry {}", file_name, id);
        }
        Ok(())
    }

    /// Convert every WAV recording to Opus: written aside, checked by
    /// decoding, renamed, the entry pointed at it, and only then the WAV
    /// deleted. A recording that fails stays a WAV. Returns the converted
    /// entries.
    fn convert_wav_recordings_with_conn(
        conn: &Connection,
        recordings_dir: &Path,
    ) -> Result<Vec<i64>> {
        let mut stmt = conn.prepare(
            "SELECT id, file_name FROM transcription_history WHERE recording_removed IS NULL",
        )?;
        let wavs = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>("id")?, row.get::<_, String>("file_name")?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|(_, file_name)| {
                Path::new(file_name)
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
                    && recordings_dir.join(file_name).is_file()
            });

        let mut converted = Vec::new();
        for (id, file_name) in wavs {
            match Self::convert_wav_recording(conn, recordings_dir, id, &file_name) {
                Ok(true) => converted.push(id),
                Ok(false) => {}
                Err(e) => warn!("Kept {} as WAV: {}", file_name, e),
            }
        }
        Ok(converted)
    }

    /// Convert one WAV recording; `false` when its entry went away meanwhile.
    fn convert_wav_recording(
        conn: &Connection,
        recordings_dir: &Path,
        id: i64,
        file_name: &str,
    ) -> Result<bool> {
        let wav = recordings_dir.join(file_name);
        let stem = Path::new(file_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow!("no file name"))?;
        let opus_name = crate::audio_toolkit::recording_file_name(stem);
        let opus = recordings_dir.join(&opus_name);
        if opus.exists() {
            return Err(anyhow!("{} already exists", opus_name));
        }

        let samples = crate::audio_toolkit::read_wav_samples(&wav)?;
        let partial = recordings_dir.join(format!("{opus_name}.partial"));
        let written = crate::audio_toolkit::save_recording(&partial, &samples).and_then(|()| {
            let decoded = crate::audio_toolkit::read_opus_samples(&partial)?.len();
            if decoded != samples.len() {
                return Err(anyhow!(
                    "decoded {} samples instead of {}",
                    decoded,
                    samples.len()
                ));
            }
            Ok(fs::rename(&partial, &opus)?)
        });
        if let Err(e) = written {
            let _ = fs::remove_file(&partial);
            return Err(e);
        }

        let updated = conn.execute(
            "UPDATE transcription_history SET file_name = ?1 WHERE id = ?2 AND file_name = ?3",
            params![opus_name, id, file_name],
        )?;
        if updated == 0 {
            let _ = fs::remove_file(&opus);
            return Ok(false);
        }
        if let Err(e) = fs::remove_file(&wav) {
            warn!("Converted {} but couldn't delete the WAV: {}", file_name, e);
        }
        Ok(true)
    }

    /// The samples of `entry`'s recording, in either format.
    pub fn recording_samples_in(entry: &HistoryEntry, recordings_dir: &Path) -> Result<Vec<f32>> {
        if entry.recording_removed {
            return Err(anyhow!(
                "The recording was removed to stay within the storage limit"
            ));
        }
        crate::audio_toolkit::read_recording_samples(recordings_dir.join(&entry.file_name))
    }

    fn format_timestamp_title(&self, timestamp: i64) -> String {
        if let Some(utc_datetime) = DateTime::from_timestamp(timestamp, 0) {
            // Convert UTC to local timezone
            let local_datetime = utc_datetime.with_timezone(&Local);
            local_datetime.format("%B %e, %Y - %l:%M%p").to_string()
        } else {
            format!("Recording {}", timestamp)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};

    fn setup_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "CREATE TABLE transcription_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_name TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                saved BOOLEAN NOT NULL DEFAULT 0,
                title TEXT NOT NULL,
                transcription_text TEXT NOT NULL,
                post_processed_text TEXT,
                post_process_prompt TEXT,
                post_process_requested BOOLEAN NOT NULL DEFAULT 0,
                source_name TEXT,
                language TEXT,
                language_source TEXT,
                recording_removed TEXT
            );",
        )
        .expect("create transcription_history table");
        conn
    }

    fn insert_entry(conn: &Connection, timestamp: i64, text: &str, post_processed: Option<&str>) {
        conn.execute(
            "INSERT INTO transcription_history (
                file_name,
                timestamp,
                saved,
                title,
                transcription_text,
                post_processed_text,
                post_process_prompt,
                post_process_requested
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                format!("handy-{}.wav", timestamp),
                timestamp,
                false,
                format!("Recording {}", timestamp),
                text,
                post_processed,
                Option::<String>::None,
                false,
            ],
        )
        .expect("insert history entry");
    }

    fn user_version(conn: &Connection) -> i32 {
        conn.pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("read user_version")
    }

    #[test]
    fn migrating_adds_columns_without_raising_the_version() {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        HistoryManager::apply_migrations(&mut conn).expect("migrate");
        for column in ADDED_COLUMNS {
            assert!(HistoryManager::has_column(&conn, column).expect("read table info"));
        }
        assert_eq!(user_version(&conn), MIGRATIONS.len() as i32);

        // Opening it again changes nothing.
        HistoryManager::apply_migrations(&mut conn).expect("migrate again");
        assert_eq!(user_version(&conn), MIGRATIONS.len() as i32);
    }

    #[test]
    fn a_database_from_a_newer_handy_still_opens() {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        HistoryManager::apply_migrations(&mut conn).expect("migrate");
        conn.execute(
            "ALTER TABLE transcription_history ADD COLUMN from_the_future TEXT",
            [],
        )
        .expect("add newer column");
        conn.pragma_update(None, "user_version", MIGRATIONS.len() as i32 + 3)
            .expect("set newer version");

        HistoryManager::apply_migrations(&mut conn).expect("open newer database");
        insert_entry(&conn, 100, "still works", None);
        let entry = HistoryManager::get_latest_entry_with_conn(&conn)
            .expect("fetch latest entry")
            .expect("entry exists");
        assert_eq!(entry.transcription_text, "still works");
    }

    #[test]
    fn reads_the_language_and_ignores_an_unknown_source() {
        let conn = setup_conn();
        let latest = |source: &str| {
            conn.execute(
                "INSERT INTO transcription_history (file_name, timestamp, title, transcription_text, language, language_source)
                 VALUES ('a.wav', (SELECT COUNT(*) FROM transcription_history), 'a', 'hallo', 'de', ?1)",
                [source],
            )
            .expect("insert entry");
            HistoryManager::get_latest_entry_with_conn(&conn)
                .unwrap()
                .unwrap()
        };
        let entry = latest("detected");
        assert_eq!(entry.language.as_deref(), Some("de"));
        assert_eq!(entry.language_source, Some(LanguageSource::Detected));
        assert_eq!(latest("from_the_future").language_source, None);
    }

    #[test]
    fn get_latest_entry_returns_none_when_empty() {
        let conn = setup_conn();
        let entry = HistoryManager::get_latest_entry_with_conn(&conn).expect("fetch latest entry");
        assert!(entry.is_none());
    }

    #[test]
    fn get_latest_entry_returns_newest_entry() {
        let conn = setup_conn();
        insert_entry(&conn, 100, "first", None);
        insert_entry(&conn, 200, "second", Some("processed"));

        let entry = HistoryManager::get_latest_entry_with_conn(&conn)
            .expect("fetch latest entry")
            .expect("entry exists");

        assert_eq!(entry.timestamp, 200);
        assert_eq!(entry.transcription_text, "second");
        assert_eq!(entry.post_processed_text.as_deref(), Some("processed"));
    }

    #[test]
    fn get_latest_completed_entry_skips_empty_entries() {
        let conn = setup_conn();
        insert_entry(&conn, 100, "completed", None);
        insert_entry(&conn, 200, "", None);

        let entry = HistoryManager::get_latest_completed_entry_with_conn(&conn)
            .expect("fetch latest completed entry")
            .expect("completed entry exists");

        assert_eq!(entry.timestamp, 100);
        assert_eq!(entry.transcription_text, "completed");
    }

    #[test]
    fn get_latest_completed_entry_skips_files() {
        let conn = setup_conn();
        insert_entry(&conn, 100, "dictated", None);
        conn.execute(
            "INSERT INTO transcription_history (file_name, timestamp, saved, title, transcription_text, source_name)
             VALUES ('handy-file-1.wav', 200, 1, 'file', 'from a file', 'talk.mp3')",
            [],
        )
        .expect("insert file entry");

        let entry = HistoryManager::get_latest_completed_entry_with_conn(&conn)
            .expect("fetch latest completed entry")
            .expect("completed entry exists");

        assert_eq!(entry.transcription_text, "dictated");
    }

    // ---- Storage limit -------------------------------------------------

    fn rec(id: i64, bytes: u64, timestamp: i64) -> StoredRecording {
        StoredRecording {
            id,
            bytes,
            saved: false,
            is_file: false,
            timestamp,
        }
    }

    #[test]
    fn unlimited_storage_removes_nothing() {
        let recordings = [rec(1, 900, 1), rec(2, 900, 2)];
        assert!(recordings_to_remove(&recordings, None).is_empty());
    }

    #[test]
    fn recordings_within_the_limit_stay() {
        let recordings = [rec(1, 100, 1), rec(2, 150, 2)];
        assert!(recordings_to_remove(&recordings, Some(250)).is_empty());
        assert!(recordings_to_remove(&recordings, Some(1_000)).is_empty());
    }

    #[test]
    fn the_oldest_recordings_go_first_until_the_rest_fits() {
        let recordings = [
            rec(1, 100, 10),
            rec(2, 100, 30),
            rec(3, 100, 20),
            rec(4, 100, 40),
        ];
        assert_eq!(recordings_to_remove(&recordings, Some(250)), vec![1, 3]);
        assert_eq!(recordings_to_remove(&recordings, Some(300)), vec![1]);
    }

    #[test]
    fn recordings_of_the_same_time_go_in_the_order_they_were_made() {
        let recordings = [rec(7, 100, 5), rec(3, 100, 5), rec(9, 100, 6)];
        assert_eq!(recordings_to_remove(&recordings, Some(150)), vec![3, 7]);
    }

    #[test]
    fn starred_recordings_are_never_removed_but_count() {
        let starred = StoredRecording {
            saved: true,
            ..rec(1, 300, 1)
        };
        let recordings = [starred, rec(2, 100, 2), rec(3, 100, 3)];
        assert_eq!(recordings_to_remove(&recordings, Some(450)), vec![2]);
    }

    #[test]
    fn transcribed_files_are_never_removed_but_count() {
        let file = StoredRecording {
            is_file: true,
            ..rec(1, 300, 1)
        };
        let recordings = [file, rec(2, 100, 2), rec(3, 100, 3)];
        assert_eq!(recordings_to_remove(&recordings, Some(450)), vec![2]);
    }

    #[test]
    fn when_protected_recordings_alone_exceed_the_limit_only_the_newest_stays() {
        let starred = StoredRecording {
            saved: true,
            ..rec(1, 600, 1)
        };
        let file = StoredRecording {
            is_file: true,
            ..rec(2, 100, 2)
        };
        let recordings = [rec(4, 50, 4), starred, file, rec(3, 50, 3), rec(5, 50, 5)];
        assert_eq!(recordings_to_remove(&recordings, Some(500)), vec![3, 4]);
    }

    #[test]
    fn the_newest_recording_stays_even_when_it_alone_exceeds_the_limit() {
        let recordings = [rec(1, 100, 1), rec(2, 900, 2)];
        assert_eq!(recordings_to_remove(&recordings, Some(500)), vec![1]);
        assert!(recordings_to_remove(&[rec(3, 900, 3)], Some(500)).is_empty());
    }

    #[test]
    fn the_newest_recording_stays_when_a_newer_one_is_starred() {
        let starred = StoredRecording {
            saved: true,
            ..rec(3, 600, 3)
        };
        let recordings = [rec(1, 50, 1), rec(2, 50, 2), starred];
        assert_eq!(recordings_to_remove(&recordings, Some(500)), vec![1]);
    }

    #[test]
    fn usage_counts_all_recordings_and_the_protected_ones() {
        let starred = StoredRecording {
            saved: true,
            ..rec(1, 300, 1)
        };
        let file = StoredRecording {
            is_file: true,
            ..rec(2, 200, 2)
        };
        let usage = storage_usage(&[starred, file, rec(3, 50, 3)]);
        assert_eq!(
            usage,
            RecordingStorageUsage {
                total_bytes: 550,
                protected_bytes: 500,
            }
        );
        assert_eq!(storage_usage(&[]), RecordingStorageUsage::default());
    }

    // ---- Recordings on disk ----------------------------------------------

    /// Insert an entry whose recording is `file_name`; returns its id.
    fn insert_recording(
        conn: &Connection,
        timestamp: i64,
        file_name: &str,
        saved: bool,
        source_name: Option<&str>,
    ) -> i64 {
        conn.execute(
            "INSERT INTO transcription_history (file_name, timestamp, saved, title, transcription_text, source_name)
             VALUES (?1, ?2, ?3, 'title', ?4, ?5)",
            params![file_name, timestamp, saved, format!("text {timestamp}"), source_name],
        )
        .expect("insert recording entry");
        conn.last_insert_rowid()
    }

    fn entry_by_id(conn: &Connection, id: i64) -> Option<HistoryEntry> {
        conn.query_row(
            &format!("SELECT {ENTRY_COLUMNS} FROM transcription_history WHERE id = ?1"),
            params![id],
            HistoryManager::map_history_entry,
        )
        .optional()
        .expect("read entry")
    }

    fn tone(n: usize) -> Vec<f32> {
        (0..n).map(|i| 0.4 * (i as f32 * 0.05).sin()).collect()
    }

    #[test]
    fn finds_the_recordings_that_are_still_there() {
        let conn = setup_conn();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.opus"), vec![0u8; 1_000]).unwrap();
        std::fs::write(dir.path().join("b.opus"), vec![0u8; 2_000]).unwrap();
        std::fs::write(dir.path().join("f.opus"), vec![0u8; 3_000]).unwrap();
        let a = insert_recording(&conn, 1, "a.opus", false, None);
        let b = insert_recording(&conn, 2, "b.opus", true, None);
        let f = insert_recording(&conn, 3, "f.opus", false, Some("talk.mp3"));
        let removed = insert_recording(&conn, 4, "gone.opus", false, None);
        conn.execute(
            "UPDATE transcription_history SET recording_removed = '5' WHERE id = ?1",
            params![removed],
        )
        .unwrap();
        insert_recording(&conn, 5, "missing.opus", false, None);

        let mut found = HistoryManager::stored_recordings_with_conn(&conn, dir.path())
            .expect("list recordings");
        found.sort_by_key(|r| r.id);
        assert_eq!(
            found,
            vec![
                StoredRecording {
                    id: a,
                    bytes: 1_000,
                    saved: false,
                    is_file: false,
                    timestamp: 1
                },
                StoredRecording {
                    id: b,
                    bytes: 2_000,
                    saved: true,
                    is_file: false,
                    timestamp: 2
                },
                StoredRecording {
                    id: f,
                    bytes: 3_000,
                    saved: false,
                    is_file: true,
                    timestamp: 3
                },
            ]
        );
    }

    #[test]
    fn removing_a_recording_keeps_the_entry_and_its_text() {
        let conn = setup_conn();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.opus"), b"audio").unwrap();
        std::fs::write(dir.path().join("b.opus"), b"audio").unwrap();
        let a = insert_recording(&conn, 1, "a.opus", false, None);
        let b = insert_recording(&conn, 2, "b.opus", false, None);

        HistoryManager::remove_recordings_with_conn(&conn, dir.path(), &[a], 99)
            .expect("remove recording");

        assert!(!dir.path().join("a.opus").exists(), "file deleted");
        let entry = entry_by_id(&conn, a).expect("entry kept");
        assert_eq!(entry.transcription_text, "text 1");
        assert!(entry.recording_removed);
        let other = entry_by_id(&conn, b).expect("other entry kept");
        assert!(!other.recording_removed);
        assert!(dir.path().join("b.opus").exists());
    }

    #[test]
    fn converts_wav_recordings_to_opus() {
        let conn = setup_conn();
        let dir = tempfile::tempdir().unwrap();
        crate::audio_toolkit::save_wav_file(dir.path().join("handy-1.wav"), &tone(16_000)).unwrap();
        let id = insert_recording(&conn, 1, "handy-1.wav", false, None);

        let converted = HistoryManager::convert_wav_recordings_with_conn(&conn, dir.path())
            .expect("convert recordings");

        assert_eq!(converted, vec![id]);
        let entry = entry_by_id(&conn, id).unwrap();
        assert_eq!(entry.file_name, "handy-1.opus");
        assert!(!dir.path().join("handy-1.wav").exists(), "WAV deleted");
        let samples = crate::audio_toolkit::read_opus_samples(dir.path().join("handy-1.opus"))
            .expect("read converted recording");
        assert_eq!(samples.len(), 16_000);
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(leftovers, vec!["handy-1.opus".to_string()]);
    }

    #[test]
    fn a_wav_that_cannot_be_read_stays_as_it_is() {
        let conn = setup_conn();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("broken.wav"), b"not a wav at all").unwrap();
        let id = insert_recording(&conn, 1, "broken.wav", false, None);

        let converted = HistoryManager::convert_wav_recordings_with_conn(&conn, dir.path())
            .expect("conversion runs");

        assert!(converted.is_empty());
        assert_eq!(entry_by_id(&conn, id).unwrap().file_name, "broken.wav");
        let files: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(files, vec!["broken.wav".to_string()]);
    }

    #[test]
    fn conversion_leaves_opus_missing_and_removed_recordings_alone() {
        let conn = setup_conn();
        let dir = tempfile::tempdir().unwrap();
        crate::audio_toolkit::save_opus_file(dir.path().join("new.opus"), &tone(800)).unwrap();
        crate::audio_toolkit::save_wav_file(dir.path().join("old.wav"), &tone(800)).unwrap();
        insert_recording(&conn, 1, "new.opus", false, None);
        insert_recording(&conn, 2, "missing.wav", false, None);
        let removed = insert_recording(&conn, 3, "old.wav", false, None);
        conn.execute(
            "UPDATE transcription_history SET recording_removed = '5' WHERE id = ?1",
            params![removed],
        )
        .unwrap();

        let converted = HistoryManager::convert_wav_recordings_with_conn(&conn, dir.path())
            .expect("conversion runs");

        assert!(converted.is_empty());
        assert_eq!(entry_by_id(&conn, removed).unwrap().file_name, "old.wav");
    }

    #[test]
    fn reads_the_samples_of_opus_and_wav_recordings() {
        let conn = setup_conn();
        let dir = tempfile::tempdir().unwrap();
        crate::audio_toolkit::save_opus_file(dir.path().join("a.opus"), &tone(5_000)).unwrap();
        crate::audio_toolkit::save_wav_file(dir.path().join("b.wav"), &tone(3_000)).unwrap();
        let a = insert_recording(&conn, 1, "a.opus", false, None);
        let b = insert_recording(&conn, 2, "b.wav", false, None);

        let samples = |id| {
            HistoryManager::recording_samples_in(&entry_by_id(&conn, id).unwrap(), dir.path())
                .expect("read samples")
                .len()
        };
        assert_eq!(samples(a), 5_000);
        assert_eq!(samples(b), 3_000);
    }

    #[test]
    fn a_removed_recording_has_no_samples() {
        let conn = setup_conn();
        let dir = tempfile::tempdir().unwrap();
        crate::audio_toolkit::save_wav_file(dir.path().join("a.wav"), &tone(3_000)).unwrap();
        let id = insert_recording(&conn, 1, "a.wav", false, None);
        conn.execute(
            "UPDATE transcription_history SET recording_removed = '5' WHERE id = ?1",
            params![id],
        )
        .unwrap();

        let entry = entry_by_id(&conn, id).unwrap();
        assert!(HistoryManager::recording_samples_in(&entry, dir.path()).is_err());
    }

    // Post-processing an entry again from History.

    fn post_process_requested(conn: &Connection, id: i64) -> bool {
        entry_by_id(conn, id)
            .expect("entry exists")
            .post_process_requested
    }

    #[test]
    fn post_processing_an_entry_keeps_its_transcript() {
        let conn = setup_conn();
        insert_entry(&conn, 100, "raw words", None);

        let entry = HistoryManager::update_post_processing_with_conn(
            &conn,
            1,
            "Clean words.".into(),
            Some("Fix the grammar".into()),
        )
        .expect("update post-processing");

        assert_eq!(entry.transcription_text, "raw words");
        assert_eq!(entry.post_processed_text.as_deref(), Some("Clean words."));
        assert_eq!(
            entry.post_process_prompt.as_deref(),
            Some("Fix the grammar")
        );
        assert_eq!(
            entry_by_id(&conn, 1).unwrap().post_processed_text,
            entry.post_processed_text
        );
    }

    #[test]
    fn post_processing_replaces_an_earlier_result() {
        let conn = setup_conn();
        insert_entry(&conn, 100, "raw words", Some("Old result."));

        HistoryManager::update_post_processing_with_conn(&conn, 1, "New result.".into(), None)
            .expect("update post-processing");

        let entry = entry_by_id(&conn, 1).unwrap();
        assert_eq!(entry.post_processed_text.as_deref(), Some("New result."));
        assert_eq!(entry.post_process_prompt, None);
    }

    #[test]
    fn post_processing_an_entry_marks_it_post_processed() {
        let conn = setup_conn();
        insert_entry(&conn, 100, "raw words", None);
        assert!(!post_process_requested(&conn, 1));

        HistoryManager::update_post_processing_with_conn(&conn, 1, "Clean.".into(), None)
            .expect("update post-processing");

        assert!(post_process_requested(&conn, 1));
    }

    #[test]
    fn post_processing_a_missing_entry_fails() {
        let conn = setup_conn();
        assert!(
            HistoryManager::update_post_processing_with_conn(&conn, 42, "Clean.".into(), None)
                .is_err()
        );
    }

    #[test]
    fn transcribing_again_with_post_processing_marks_the_entry() {
        let conn = setup_conn();
        insert_entry(&conn, 100, "raw words", None);

        HistoryManager::update_transcription_with_conn(
            &conn,
            1,
            "new words".into(),
            Some("New words.".into()),
            None,
            None,
            true,
        )
        .expect("update transcription");

        let entry = entry_by_id(&conn, 1).unwrap();
        assert_eq!(entry.transcription_text, "new words");
        assert_eq!(entry.post_processed_text.as_deref(), Some("New words."));
        assert!(entry.post_process_requested);
    }

    #[test]
    fn transcribing_again_as_dictated_keeps_how_it_was_dictated() {
        let conn = setup_conn();
        insert_entry(&conn, 100, "plain", None);
        insert_entry(&conn, 200, "with post-processing", Some("Processed."));
        conn.execute(
            "UPDATE transcription_history SET post_process_requested = 1 WHERE id = 2",
            [],
        )
        .unwrap();

        HistoryManager::update_transcription_with_conn(
            &conn,
            1,
            "a".into(),
            None,
            None,
            None,
            false,
        )
        .expect("update plain entry");
        HistoryManager::update_transcription_with_conn(
            &conn,
            2,
            "b".into(),
            Some("B.".into()),
            None,
            None,
            true,
        )
        .expect("update post-processed entry");

        assert!(!post_process_requested(&conn, 1));
        assert!(post_process_requested(&conn, 2));
    }
}
