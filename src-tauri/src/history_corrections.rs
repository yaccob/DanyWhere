//! Corrections in History (DanyWhere). The user can correct an entry's
//! transcription and its post-processed text; both originals stay as they
//! were made, each correction is stored beside its original. A post-processing
//! can also be deleted, so that the transcription is the entry's result again.
//!
//! What an entry "is" — what History shows by default, what is copied and
//! pasted again — is the first of: the corrected post-processing, the
//! post-processing, the corrected transcription, the transcription.
//!
//! Correcting the transcription never post-processes again by itself. The
//! entry instead remembers which version of the transcription its
//! post-processing was made from, and History says when that is no longer the
//! current one; the wand post-processes it again on demand.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::managers::history::{HistoryEntry, HistoryManager};

/// Which of an entry's two texts a correction belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionPart {
    /// The transcription as the speech model wrote it.
    Transcription,
    /// The text post-processing made of it.
    PostProcessing,
}

/// Why a correction can't be stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CorrectionError {
    /// A correction can't leave the text empty; delete the entry or its
    /// post-processing instead.
    Blank,
}

/// `text` unless it is blank.
fn non_blank(text: Option<&str>) -> Option<&str> {
    text.filter(|text| !text.trim().is_empty())
}

/// What the entry "is": shown by default, copied and pasted again.
pub fn entry_text(entry: &HistoryEntry) -> &str {
    match non_blank(entry.post_processed_text.as_deref()) {
        Some(post_processed) => {
            non_blank(entry.post_processed_correction.as_deref()).unwrap_or(post_processed)
        }
        None => transcript_for_post_processing(entry),
    }
}

/// The transcription post-processing starts from: the corrected one, if the
/// user corrected it.
pub fn transcript_for_post_processing(entry: &HistoryEntry) -> &str {
    non_blank(entry.transcription_correction.as_deref()).unwrap_or(&entry.transcription_text)
}

/// Whether the post-processed text was made from another version of the
/// transcription than the entry's current one. `from_correction` is the
/// transcription's correction it was made from (`None`: the original).
pub fn post_processing_outdated(
    post_processed_text: Option<&str>,
    transcription_correction: Option<&str>,
    from_correction: Option<&str>,
) -> bool {
    non_blank(post_processed_text).is_some() && transcription_correction != from_correction
}

/// What to store as the correction of `original` when the user saved
/// `edited`: `None` when it equals the original, so saving the original text
/// back removes the correction.
pub fn correction_to_store(
    original: &str,
    edited: &str,
) -> std::result::Result<Option<String>, CorrectionError> {
    let edited = edited.trim();
    if edited.is_empty() {
        return Err(CorrectionError::Blank);
    }
    if edited == original.trim() {
        return Ok(None);
    }
    Ok(Some(edited.to_string()))
}

/// Stores `text` as the correction of the entry's `part`, or removes the
/// correction when `text` is `None`. Returns the entry as it is now.
pub(crate) fn set_correction_with_conn(
    conn: &Connection,
    id: i64,
    part: CorrectionPart,
    text: Option<&str>,
) -> Result<HistoryEntry> {
    let entry = HistoryManager::entry_with_conn(conn, id)?;
    let (original, column) = match part {
        CorrectionPart::Transcription => (
            Some(entry.transcription_text.as_str()),
            "transcription_correction",
        ),
        CorrectionPart::PostProcessing => (
            non_blank(entry.post_processed_text.as_deref()),
            "post_processed_correction",
        ),
    };
    let Some(original) = original else {
        return Err(anyhow!(
            "History entry {} has no post-processing to correct",
            id
        ));
    };
    let stored = match text {
        None => None,
        Some(text) => correction_to_store(original, text)
            .map_err(|CorrectionError::Blank| anyhow!("blank"))?,
    };
    conn.execute(
        &format!("UPDATE transcription_history SET {column} = ?1 WHERE id = ?2"),
        params![stored, id],
    )?;
    HistoryManager::entry_with_conn(conn, id)
}

/// Deletes the entry's post-processing, together with its correction. The
/// entry no longer counts as post-processed, so transcribing it again doesn't
/// post-process either.
pub(crate) fn delete_post_processing_with_conn(conn: &Connection, id: i64) -> Result<HistoryEntry> {
    let updated = conn.execute(
        "UPDATE transcription_history
         SET post_processed_text = NULL,
             post_process_prompt = NULL,
             post_processed_correction = NULL,
             post_processed_from_correction = NULL,
             post_process_requested = 0
         WHERE id = ?1",
        params![id],
    )?;
    if updated == 0 {
        return Err(anyhow!("History entry {} not found", id));
    }
    HistoryManager::entry_with_conn(conn, id)
}

impl HistoryManager {
    /// See [`set_correction_with_conn`]; tells the frontend.
    pub fn set_correction(
        &self,
        id: i64,
        part: CorrectionPart,
        text: Option<&str>,
    ) -> Result<HistoryEntry> {
        let conn = self.get_connection()?;
        let entry = set_correction_with_conn(&conn, id, part, text)?;
        self.emit_entry(&entry);
        Ok(entry)
    }

    /// See [`delete_post_processing_with_conn`]; tells the frontend.
    pub fn delete_post_processing(&self, id: i64) -> Result<HistoryEntry> {
        let conn = self.get_connection()?;
        let entry = delete_post_processing_with_conn(&conn, id)?;
        self.emit_entry(&entry);
        Ok(entry)
    }
}

/// Corrects the entry's `part` to `text`, or removes its correction when
/// `text` is `None`. Errors: `blank`, or the database's message.
#[tauri::command]
#[specta::specta]
pub fn correct_history_entry(
    history: State<'_, Arc<HistoryManager>>,
    id: i64,
    part: CorrectionPart,
    text: Option<String>,
) -> std::result::Result<HistoryEntry, String> {
    history
        .set_correction(id, part, text.as_deref())
        .map_err(|e| e.to_string())
}

/// Deletes the entry's post-processing, so its (corrected) transcription is
/// its result again.
#[tauri::command]
#[specta::specta]
pub fn delete_history_entry_post_processing(
    history: State<'_, Arc<HistoryManager>>,
    id: i64,
) -> std::result::Result<HistoryEntry, String> {
    history
        .delete_post_processing(id)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        transcription: &str,
        post_processed: Option<&str>,
        transcription_correction: Option<&str>,
        post_processed_correction: Option<&str>,
    ) -> HistoryEntry {
        HistoryEntry {
            id: 1,
            file_name: "handy-1.opus".to_string(),
            timestamp: 0,
            saved: false,
            title: String::new(),
            transcription_text: transcription.to_string(),
            post_processed_text: post_processed.map(str::to_string),
            post_process_prompt: None,
            post_process_requested: post_processed.is_some(),
            source_name: None,
            language: None,
            language_source: None,
            recording_removed: false,
            transcription_correction: transcription_correction.map(str::to_string),
            post_processed_correction: post_processed_correction.map(str::to_string),
            post_processing_outdated: false,
            window_context: Default::default(),
        }
    }

    // ---- What an entry is ----------------------------------------------

    #[test]
    fn the_corrected_post_processing_comes_first() {
        let e = entry(
            "raw",
            Some("Clean."),
            Some("raw fixed"),
            Some("Clean, fixed."),
        );
        assert_eq!(entry_text(&e), "Clean, fixed.");
    }

    #[test]
    fn then_the_post_processing() {
        let e = entry("raw", Some("Clean."), Some("raw fixed"), None);
        assert_eq!(entry_text(&e), "Clean.");
    }

    #[test]
    fn then_the_corrected_transcription() {
        let e = entry("raw", None, Some("raw fixed"), None);
        assert_eq!(entry_text(&e), "raw fixed");
    }

    #[test]
    fn then_the_transcription() {
        let e = entry("raw", None, None, None);
        assert_eq!(entry_text(&e), "raw");
    }

    #[test]
    fn a_blank_post_processing_counts_as_none() {
        let e = entry("raw", Some("  \n"), Some("raw fixed"), None);
        assert_eq!(entry_text(&e), "raw fixed");
    }

    #[test]
    fn post_processing_starts_from_the_corrected_transcription() {
        assert_eq!(
            transcript_for_post_processing(&entry("raw", None, Some("raw fixed"), None)),
            "raw fixed"
        );
        assert_eq!(
            transcript_for_post_processing(&entry("raw", None, None, None)),
            "raw"
        );
    }

    // ---- Outdated post-processing --------------------------------------

    #[test]
    fn post_processing_from_the_current_transcription_is_up_to_date() {
        assert!(!post_processing_outdated(Some("Clean."), None, None));
        assert!(!post_processing_outdated(
            Some("Clean."),
            Some("raw fixed"),
            Some("raw fixed")
        ));
    }

    #[test]
    fn correcting_the_transcription_outdates_the_post_processing() {
        assert!(post_processing_outdated(
            Some("Clean."),
            Some("raw fixed"),
            None
        ));
    }

    #[test]
    fn changing_or_reverting_the_correction_outdates_it_too() {
        assert!(post_processing_outdated(
            Some("Clean."),
            Some("raw fixed again"),
            Some("raw fixed")
        ));
        assert!(post_processing_outdated(
            Some("Clean."),
            None,
            Some("raw fixed")
        ));
    }

    #[test]
    fn without_post_processing_nothing_is_outdated() {
        assert!(!post_processing_outdated(None, Some("raw fixed"), None));
        assert!(!post_processing_outdated(
            Some(" "),
            Some("raw fixed"),
            None
        ));
    }

    // ---- What gets stored ----------------------------------------------

    #[test]
    fn an_edited_text_is_stored_as_the_correction() {
        assert_eq!(
            correction_to_store("raw", "raw fixed"),
            Ok(Some("raw fixed".to_string()))
        );
    }

    #[test]
    fn saving_the_original_removes_the_correction() {
        assert_eq!(correction_to_store("raw words", "raw words"), Ok(None));
        assert_eq!(correction_to_store("raw words", " raw words\n"), Ok(None));
    }

    #[test]
    fn a_correction_keeps_the_users_line_breaks_but_not_outer_blanks() {
        assert_eq!(
            correction_to_store("raw", "  first\nsecond \n"),
            Ok(Some("first\nsecond".to_string()))
        );
    }

    #[test]
    fn a_blank_correction_is_refused() {
        assert_eq!(
            correction_to_store("raw", " \n "),
            Err(CorrectionError::Blank)
        );
    }

    // ---- In the database -----------------------------------------------

    fn database() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        HistoryManager::apply_migrations(&mut conn).expect("migrate");
        conn
    }

    fn insert(conn: &Connection, transcription: &str, post_processed: Option<&str>) -> i64 {
        conn.execute(
            "INSERT INTO transcription_history (
                file_name, timestamp, saved, title, transcription_text,
                post_processed_text, post_process_prompt, post_process_requested
            ) VALUES ('handy-1.opus', 1, 0, 't', ?1, ?2, ?3, ?4)",
            params![
                transcription,
                post_processed,
                post_processed.map(|_| "Fix it"),
                post_processed.is_some()
            ],
        )
        .expect("insert entry");
        conn.last_insert_rowid()
    }

    fn get(conn: &Connection, id: i64) -> HistoryEntry {
        HistoryManager::entry_with_conn(conn, id).expect("entry exists")
    }

    #[test]
    fn an_old_database_gets_the_correction_columns() {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
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
                post_process_requested BOOLEAN NOT NULL DEFAULT 0
            );
            PRAGMA user_version = 4;
            INSERT INTO transcription_history (file_name, timestamp, title, transcription_text, post_processed_text)
            VALUES ('a.wav', 1, 'a', 'old words', 'Old words.');",
        )
        .expect("old schema");
        HistoryManager::apply_migrations(&mut conn).expect("migrate");

        let e = get(&conn, 1);
        assert_eq!(e.transcription_text, "old words");
        assert_eq!(e.transcription_correction, None);
        assert_eq!(e.post_processed_correction, None);
        assert!(!e.post_processing_outdated);
    }

    #[test]
    fn correcting_the_transcription_keeps_the_original() {
        let conn = database();
        let id = insert(&conn, "das Build hängt schief", None);

        let e = set_correction_with_conn(
            &conn,
            id,
            CorrectionPart::Transcription,
            Some("das Bild hängt schief"),
        )
        .expect("correct");

        assert_eq!(e.transcription_text, "das Build hängt schief");
        assert_eq!(
            e.transcription_correction.as_deref(),
            Some("das Bild hängt schief")
        );
        assert_eq!(entry_text(&get(&conn, id)), "das Bild hängt schief");
    }

    #[test]
    fn correcting_the_post_processing_keeps_its_original() {
        let conn = database();
        let id = insert(&conn, "raw", Some("Clean."));

        set_correction_with_conn(&conn, id, CorrectionPart::PostProcessing, Some("Cleaner."))
            .expect("correct");

        let e = get(&conn, id);
        assert_eq!(e.post_processed_text.as_deref(), Some("Clean."));
        assert_eq!(e.post_processed_correction.as_deref(), Some("Cleaner."));
        assert_eq!(entry_text(&e), "Cleaner.");
    }

    #[test]
    fn a_post_processing_that_isnt_there_cant_be_corrected() {
        let conn = database();
        let id = insert(&conn, "raw", None);
        assert!(
            set_correction_with_conn(&conn, id, CorrectionPart::PostProcessing, Some("x")).is_err()
        );
    }

    #[test]
    fn a_blank_correction_is_refused_by_the_database_call() {
        let conn = database();
        let id = insert(&conn, "raw", None);
        assert!(
            set_correction_with_conn(&conn, id, CorrectionPart::Transcription, Some("  ")).is_err()
        );
        assert_eq!(get(&conn, id).transcription_correction, None);
    }

    #[test]
    fn reverting_removes_the_correction() {
        let conn = database();
        let id = insert(&conn, "raw", None);
        set_correction_with_conn(&conn, id, CorrectionPart::Transcription, Some("fixed"))
            .expect("correct");

        set_correction_with_conn(&conn, id, CorrectionPart::Transcription, None).expect("revert");

        assert_eq!(get(&conn, id).transcription_correction, None);
    }

    #[test]
    fn saving_the_original_text_removes_the_correction_too() {
        let conn = database();
        let id = insert(&conn, "raw", None);
        set_correction_with_conn(&conn, id, CorrectionPart::Transcription, Some("fixed"))
            .expect("correct");

        set_correction_with_conn(&conn, id, CorrectionPart::Transcription, Some("raw"))
            .expect("save the original");

        assert_eq!(get(&conn, id).transcription_correction, None);
    }

    #[test]
    fn correcting_the_transcription_marks_the_post_processing_outdated() {
        let conn = database();
        let id = insert(&conn, "raw", Some("Clean."));
        assert!(!get(&conn, id).post_processing_outdated);

        set_correction_with_conn(&conn, id, CorrectionPart::Transcription, Some("raw fixed"))
            .expect("correct");

        assert!(get(&conn, id).post_processing_outdated);
    }

    #[test]
    fn post_processing_the_corrected_transcription_is_up_to_date_and_drops_its_correction() {
        let conn = database();
        let id = insert(&conn, "raw", Some("Clean."));
        set_correction_with_conn(&conn, id, CorrectionPart::Transcription, Some("raw fixed"))
            .expect("correct transcription");
        set_correction_with_conn(&conn, id, CorrectionPart::PostProcessing, Some("Cleaner."))
            .expect("correct post-processing");

        HistoryManager::update_post_processing_with_conn(
            &conn,
            id,
            "Clean, fixed.".into(),
            Some("Fix it".into()),
            Some("raw fixed".into()),
        )
        .expect("post-process again");

        let e = get(&conn, id);
        assert_eq!(e.post_processed_text.as_deref(), Some("Clean, fixed."));
        assert_eq!(e.post_processed_correction, None);
        assert!(!e.post_processing_outdated);
        assert_eq!(e.transcription_correction.as_deref(), Some("raw fixed"));
    }

    #[test]
    fn transcribing_again_drops_both_corrections() {
        let conn = database();
        let id = insert(&conn, "raw", Some("Clean."));
        set_correction_with_conn(&conn, id, CorrectionPart::Transcription, Some("raw fixed"))
            .expect("correct transcription");
        set_correction_with_conn(&conn, id, CorrectionPart::PostProcessing, Some("Cleaner."))
            .expect("correct post-processing");

        HistoryManager::update_transcription_with_conn(
            &conn,
            id,
            "new words".into(),
            Some("New words.".into()),
            None,
            None,
            true,
        )
        .expect("transcribe again");

        let e = get(&conn, id);
        assert_eq!(e.transcription_correction, None);
        assert_eq!(e.post_processed_correction, None);
        assert!(!e.post_processing_outdated);
    }

    #[test]
    fn deleting_the_post_processing_makes_the_transcription_the_result() {
        let conn = database();
        let id = insert(&conn, "raw", Some("Clean."));
        set_correction_with_conn(&conn, id, CorrectionPart::Transcription, Some("raw fixed"))
            .expect("correct transcription");
        set_correction_with_conn(&conn, id, CorrectionPart::PostProcessing, Some("Cleaner."))
            .expect("correct post-processing");

        let e = delete_post_processing_with_conn(&conn, id).expect("delete");

        assert_eq!(e.post_processed_text, None);
        assert_eq!(e.post_processed_correction, None);
        assert_eq!(e.post_process_prompt, None);
        assert!(!e.post_process_requested);
        assert!(!e.post_processing_outdated);
        assert_eq!(e.transcription_correction.as_deref(), Some("raw fixed"));
        assert_eq!(entry_text(&get(&conn, id)), "raw fixed");
    }

    #[test]
    fn a_missing_entry_fails() {
        let conn = database();
        assert!(
            set_correction_with_conn(&conn, 42, CorrectionPart::Transcription, Some("x")).is_err()
        );
        assert!(delete_post_processing_with_conn(&conn, 42).is_err());
    }
}
