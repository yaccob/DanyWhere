//! Versions of a History entry (DanyWhere): a stack of edits. Version 0 is
//! the transcription as the speech model wrote it; every version above it
//! was made from the one directly below, by post-processing (✨) or by the
//! user (✎). The top version is what the entry "is": what History shows by
//! default, what is copied, pasted again and sent along as context.
//!
//! Each version remembers its steps (who, when, with which prompt), also
//! when a later step replaced its text. The transcription itself is never
//! replaced by an edit; transcribing again replaces it and drops every
//! version above it.
//!
//! The column `versions` holds versions 1.. as JSON; version 0 always comes
//! from `transcription_text`. While the column is NULL, the stack is derived
//! from the columns of before (see [`StackColumns`]), so a dictation needs
//! no change to Handy's code: one with post-processing is [transcription,
//! ✨]. Once the user or a post-processing from History changes the stack,
//! the whole stack above the transcription is stored and the old correction
//! columns are no longer read. Handy's `post_processed_text` and
//! `post_process_prompt` keep the latest post-processing's result.

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use anyhow::Result;
use log::warn;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::managers::history::{HistoryEntry, HistoryManager};

/// Who made a version, or one step of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum VersionAuthor {
    /// The speech model; only version 0.
    Transcription,
    /// Post-processing.
    Ai,
    /// The user, editing it in History.
    User,
}

/// One change that made a version what it is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct VersionStep {
    pub by: VersionAuthor,
    /// Unix milliseconds; None for steps migrated from before versions existed.
    pub at: Option<i64>,
    /// For an AI step: the prompt's name, when known.
    pub prompt_name: Option<String>,
}

/// One version of an entry's text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct HistoryVersion {
    pub text: String,
    /// What made it, oldest first; never empty.
    pub steps: Vec<VersionStep>,
}

/// What saving an edited version does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SaveVersionMode {
    /// The version gets the new text and one more step.
    Replace,
    /// The new text is a new version on top of it.
    New,
}

/// Why a version can't be saved or post-processed. The commands return its
/// [`code`](VersionError::code).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VersionError {
    /// A version can't be empty.
    Blank,
    /// The transcription is never replaced, only built upon.
    TranscriptionIsFixed,
    /// The entry has no version with that number.
    NoSuchVersion,
    /// Only a version a post-processing made can be post-processed again
    /// from the one below it.
    NotMadeByAi,
}

impl VersionError {
    pub fn code(self) -> &'static str {
        match self {
            VersionError::Blank => "blank",
            VersionError::TranscriptionIsFixed => "transcription_is_fixed",
            VersionError::NoSuchVersion => "no_such_version",
            VersionError::NotMadeByAi => "not_made_by_ai",
        }
    }
}

impl fmt::Display for VersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for VersionError {}

impl VersionStep {
    fn new(by: VersionAuthor, at: Option<i64>) -> Self {
        Self {
            by,
            at,
            prompt_name: None,
        }
    }
}

impl HistoryVersion {
    /// Version 0: the transcription as the model wrote it.
    pub fn transcription(text: &str) -> Self {
        Self {
            text: text.to_string(),
            steps: vec![VersionStep::new(VersionAuthor::Transcription, None)],
        }
    }

    fn made_of(text: &str, step: VersionStep) -> Self {
        Self {
            text: text.to_string(),
            steps: vec![step],
        }
    }

    /// Whether a post-processing made it (its first step).
    fn made_by_ai(&self) -> bool {
        self.steps
            .first()
            .is_some_and(|step| step.by == VersionAuthor::Ai)
    }
}

/// `text` unless it is blank.
fn non_blank(text: Option<&str>) -> Option<&str> {
    text.filter(|text| !text.trim().is_empty())
}

/// The columns an entry's stack is read from.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct StackColumns<'a> {
    pub transcription: &'a str,
    /// Versions 1.. as stored; `None` (NULL): derived from the rest.
    pub versions: Option<&'a str>,
    /// The user's correction of the transcription, from before versions.
    pub transcription_correction: Option<&'a str>,
    pub post_processed: Option<&'a str>,
    /// The user's correction of the post-processing, from before versions.
    pub post_processed_correction: Option<&'a str>,
    /// The transcription's correction the post-processing was made from.
    pub from_correction: Option<&'a str>,
}

impl<'a> StackColumns<'a> {
    /// The columns of a row read with the entry's columns, without copying
    /// them.
    pub fn from_row(row: &'a Row<'_>) -> rusqlite::Result<Self> {
        use crate::history_search::text_ref;
        Ok(Self {
            transcription: text_ref(row, "transcription_text")?.unwrap_or_default(),
            versions: text_ref(row, "versions")?,
            transcription_correction: text_ref(row, "transcription_correction")?,
            post_processed: text_ref(row, "post_processed_text")?,
            post_processed_correction: text_ref(row, "post_processed_correction")?,
            from_correction: text_ref(row, "post_processed_from_correction")?,
        })
    }

    /// Versions 1.. of an entry from before versions existed, oldest first:
    /// with a post-processing, the correction it was made from (if any),
    /// the post-processing (✨) and its correction (if any); without one,
    /// the transcription's correction (if any). A correction of the
    /// transcription the post-processing was not made from fits no stack,
    /// so it is left out. Blank texts count as none.
    fn derived(&self) -> Vec<(VersionAuthor, &'a str)> {
        let mut versions = Vec::new();
        match non_blank(self.post_processed) {
            Some(post_processed) => {
                if let Some(from) = non_blank(self.from_correction) {
                    versions.push((VersionAuthor::User, from));
                }
                versions.push((VersionAuthor::Ai, post_processed));
                if let Some(correction) = non_blank(self.post_processed_correction) {
                    versions.push((VersionAuthor::User, correction));
                }
            }
            None => {
                if let Some(correction) = non_blank(self.transcription_correction) {
                    versions.push((VersionAuthor::User, correction));
                }
            }
        }
        versions
    }

    /// Versions 1.. as stored; `None` when none are stored or they can't
    /// be read.
    fn stored(&self) -> Option<Vec<HistoryVersion>> {
        let json = self.versions?;
        match serde_json::from_str(json) {
            Ok(versions) => Some(versions),
            Err(e) => {
                warn!("Ignoring the stored versions of a History entry: {e}");
                None
            }
        }
    }

    /// The whole stack, version 0 included.
    pub fn versions(&self) -> Vec<HistoryVersion> {
        let mut versions = vec![HistoryVersion::transcription(self.transcription)];
        match self.stored() {
            Some(stored) => versions.extend(stored),
            None => versions.extend(
                self.derived()
                    .into_iter()
                    .map(|(by, text)| HistoryVersion::made_of(text, VersionStep::new(by, None))),
            ),
        }
        versions
    }

    /// The texts of [`Self::versions`], copied only when they are stored.
    pub fn texts(&self) -> Vec<Cow<'a, str>> {
        let mut texts = vec![Cow::Borrowed(self.transcription)];
        match self.stored() {
            Some(stored) => texts.extend(stored.into_iter().map(|v| Cow::Owned(v.text))),
            None => texts.extend(self.derived().into_iter().map(|(_, t)| Cow::Borrowed(t))),
        }
        texts
    }
}

/// The stack of an entry just made, as reading it back gives it.
pub fn versions_of_new_entry(
    transcription: &str,
    post_processed: Option<&str>,
) -> Vec<HistoryVersion> {
    StackColumns {
        transcription,
        post_processed,
        ..Default::default()
    }
    .versions()
}

/// What the entry "is": the text of its top version.
pub fn entry_text(entry: &HistoryEntry) -> &str {
    entry
        .versions
        .last()
        .map_or(entry.transcription_text.as_str(), |top| top.text.as_str())
}

/// Whether any version has a step made by post-processing.
pub fn has_ai_step(versions: &[HistoryVersion]) -> bool {
    versions
        .iter()
        .flat_map(|version| &version.steps)
        .any(|step| step.by == VersionAuthor::Ai)
}

/// The stack after the user saved `text` (trimmed) on version `base` at
/// `at` (Unix milliseconds); `None`: nothing changes. The versions above
/// `base` drop, then `Replace` gives `base` the text and a ✎ step, `New`
/// puts a new version on top of it. A text equal to the one of `base`
/// changes nothing, so nothing drops either; a `Replace` that makes `base`
/// equal to the version below drops `base` too.
pub fn save_version(
    versions: &[HistoryVersion],
    base: u32,
    text: &str,
    mode: SaveVersionMode,
    at: i64,
) -> std::result::Result<Option<Vec<HistoryVersion>>, VersionError> {
    let base = base as usize;
    let Some(base_version) = versions.get(base) else {
        return Err(VersionError::NoSuchVersion);
    };
    let text = text.trim();
    if text.is_empty() {
        return Err(VersionError::Blank);
    }
    if mode == SaveVersionMode::Replace && base == 0 {
        return Err(VersionError::TranscriptionIsFixed);
    }
    if text == base_version.text.trim() {
        return Ok(None);
    }
    let step = VersionStep::new(VersionAuthor::User, Some(at));
    let mut saved = versions[..=base].to_vec();
    match mode {
        SaveVersionMode::Replace => replace_top(&mut saved, text, step),
        SaveVersionMode::New => saved.push(HistoryVersion::made_of(text, step)),
    }
    Ok(Some(saved))
}

/// Gives the top version `text` and one more `step`, or drops it when
/// `text` equals the version below. The transcription alone is never
/// replaced.
fn replace_top(versions: &mut Vec<HistoryVersion>, text: &str, step: VersionStep) {
    let [.., below, top] = versions.as_mut_slice() else {
        return;
    };
    if below.text.trim() == text {
        versions.pop();
    } else {
        top.text = text.to_string();
        top.steps.push(step);
    }
}

/// The stack after going back to version `keep`: the versions above it
/// drop. `None`: `keep` is the top or above it, nothing changes.
pub fn revert_to_version(versions: &[HistoryVersion], keep: u32) -> Option<Vec<HistoryVersion>> {
    let keep = keep as usize;
    (keep + 1 < versions.len()).then(|| versions[..=keep].to_vec())
}

/// What a post-processing from History does with its result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostProcessTarget {
    /// Post-process the top version; the result is a new version on top.
    New,
    /// Post-process the top version; the result replaces it, which keeps
    /// its steps and gets one more, a ✨ one. Never the transcription.
    Replace,
    /// Post-process the version below the top again; the result replaces
    /// the top with a single ✨ step. Only when a post-processing made the
    /// top.
    Redo,
}

/// The number of the version a post-processing starts from.
pub fn post_processing_input(
    versions: &[HistoryVersion],
    target: PostProcessTarget,
) -> std::result::Result<usize, VersionError> {
    let top = versions
        .len()
        .checked_sub(1)
        .ok_or(VersionError::NoSuchVersion)?;
    match target {
        PostProcessTarget::New => Ok(top),
        PostProcessTarget::Replace | PostProcessTarget::Redo if top == 0 => {
            Err(VersionError::TranscriptionIsFixed)
        }
        PostProcessTarget::Replace => Ok(top),
        PostProcessTarget::Redo if !versions[top].made_by_ai() => Err(VersionError::NotMadeByAi),
        PostProcessTarget::Redo => Ok(top - 1),
    }
}

/// The stack with a post-processing's `text` applied as `target` says,
/// to the stack as it is when the post-processing ends. Where `target` no
/// longer fits that stack, the result goes on top as a new version, so it
/// is never lost. As when the user saves, a result equal to the text it
/// replaces or builds upon changes nothing, and one that would make a
/// version equal to the one below drops that version instead.
pub fn apply_post_processing(
    versions: &[HistoryVersion],
    target: PostProcessTarget,
    text: &str,
    step: VersionStep,
) -> Vec<HistoryVersion> {
    let text = text.trim();
    let target = match post_processing_input(versions, target) {
        Ok(_) => target,
        Err(_) => PostProcessTarget::New,
    };
    let mut applied = versions.to_vec();
    let top_is = |versions: &[HistoryVersion], text: &str| {
        versions.last().is_some_and(|top| top.text.trim() == text)
    };
    match target {
        PostProcessTarget::New => {
            if !top_is(&applied, text) {
                applied.push(HistoryVersion::made_of(text, step));
            }
        }
        PostProcessTarget::Replace => {
            if !top_is(&applied, text) {
                replace_top(&mut applied, text, step);
            }
        }
        PostProcessTarget::Redo => {
            applied.pop();
            if !top_is(&applied, text) {
                applied.push(HistoryVersion::made_of(text, step));
            }
        }
    }
    applied
}

/// Versions 1.. as the column stores them; version 0 comes from
/// `transcription_text`.
fn stored_json(versions: &[HistoryVersion]) -> Result<String> {
    Ok(serde_json::to_string(
        versions.get(1..).unwrap_or_default(),
    )?)
}

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// See [`save_version`]; stores the stack and returns the entry as it is
/// now. Errors: [`VersionError`], or the database's.
pub(crate) fn save_version_with_conn(
    conn: &Connection,
    id: i64,
    base: u32,
    text: &str,
    mode: SaveVersionMode,
    at: i64,
) -> Result<HistoryEntry> {
    let entry = HistoryManager::entry_with_conn(conn, id)?;
    let Some(versions) = save_version(&entry.versions, base, text, mode, at)? else {
        return Ok(entry);
    };
    conn.execute(
        "UPDATE transcription_history SET versions = ?1 WHERE id = ?2",
        params![stored_json(&versions)?, id],
    )?;
    HistoryManager::entry_with_conn(conn, id)
}

/// See [`revert_to_version`]. The entry counts as post-processed only while
/// a version with a ✨ step stays, so transcribing it again post-processes
/// only then.
pub(crate) fn revert_to_version_with_conn(
    conn: &Connection,
    id: i64,
    keep: u32,
) -> Result<HistoryEntry> {
    let entry = HistoryManager::entry_with_conn(conn, id)?;
    let Some(versions) = revert_to_version(&entry.versions, keep) else {
        return Ok(entry);
    };
    conn.execute(
        "UPDATE transcription_history
         SET versions = ?1, post_process_requested = ?2
         WHERE id = ?3",
        params![stored_json(&versions)?, has_ai_step(&versions), id],
    )?;
    HistoryManager::entry_with_conn(conn, id)
}

/// What a post-processing from History came back with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PostProcessed {
    pub text: String,
    /// The prompt's text, for Handy's `post_process_prompt`.
    pub prompt: Option<String>,
    /// The prompt's name, for the ✨ step.
    pub prompt_name: Option<String>,
}

/// Applies a post-processing from History to the entry's stack as it is
/// now (see [`apply_post_processing`]) at `at` (Unix milliseconds). Handy's
/// `post_processed_text` and `post_process_prompt` get the result, and the
/// entry counts as post-processed from now on.
pub(crate) fn store_post_processing_with_conn(
    conn: &Connection,
    id: i64,
    target: PostProcessTarget,
    result: PostProcessed,
    at: i64,
) -> Result<HistoryEntry> {
    let entry = HistoryManager::entry_with_conn(conn, id)?;
    let step = VersionStep {
        by: VersionAuthor::Ai,
        at: Some(at),
        prompt_name: result.prompt_name,
    };
    let versions = apply_post_processing(&entry.versions, target, &result.text, step);
    conn.execute(
        "UPDATE transcription_history
         SET post_processed_text = ?1,
             post_process_prompt = ?2,
             post_process_requested = 1,
             versions = ?3
         WHERE id = ?4",
        params![result.text, result.prompt, stored_json(&versions)?, id],
    )?;
    HistoryManager::entry_with_conn(conn, id)
}

impl HistoryManager {
    /// See [`save_version_with_conn`]; tells the frontend.
    pub fn save_version(
        &self,
        id: i64,
        base: u32,
        text: &str,
        mode: SaveVersionMode,
    ) -> Result<HistoryEntry> {
        let conn = self.get_connection()?;
        let entry = save_version_with_conn(&conn, id, base, text, mode, now_millis())?;
        self.emit_entry(&entry);
        Ok(entry)
    }

    /// See [`revert_to_version_with_conn`]; tells the frontend.
    pub fn revert_to_version(&self, id: i64, keep: u32) -> Result<HistoryEntry> {
        let conn = self.get_connection()?;
        let entry = revert_to_version_with_conn(&conn, id, keep)?;
        self.emit_entry(&entry);
        Ok(entry)
    }

    /// See [`store_post_processing_with_conn`]; tells the frontend.
    pub(crate) fn store_post_processing(
        &self,
        id: i64,
        target: PostProcessTarget,
        result: PostProcessed,
    ) -> Result<HistoryEntry> {
        let conn = self.get_connection()?;
        let entry = store_post_processing_with_conn(&conn, id, target, result, now_millis())?;
        self.emit_entry(&entry);
        Ok(entry)
    }
}

/// Saves the user's `text` on version `base` of the entry: `replace`
/// changes that version, `new` puts a new one on top of it; the versions
/// above `base` drop. Errors: `blank`, `transcription_is_fixed` (replacing
/// version 0), `no_such_version`, or the database's message.
#[tauri::command]
#[specta::specta]
pub fn save_history_version(
    history: State<'_, Arc<HistoryManager>>,
    id: i64,
    base: u32,
    text: String,
    mode: SaveVersionMode,
) -> std::result::Result<HistoryEntry, String> {
    history
        .save_version(id, base, &text, mode)
        .map_err(|e| e.to_string())
}

/// Goes back to version `keep` of the entry: every version above it drops.
#[tauri::command]
#[specta::specta]
pub fn revert_history_entry_to_version(
    history: State<'_, Arc<HistoryManager>>,
    id: i64,
    keep: u32,
) -> std::result::Result<HistoryEntry, String> {
    history
        .revert_to_version(id, keep)
        .map_err(|e| e.to_string())
}

/// An entry with `versions` and nothing else of note, for tests.
#[cfg(test)]
pub(crate) fn test_entry(versions: Vec<HistoryVersion>) -> HistoryEntry {
    HistoryEntry {
        id: 1,
        file_name: "handy-1.opus".to_string(),
        timestamp: 0,
        saved: false,
        title: String::new(),
        transcription_text: versions.first().map(|v| v.text.clone()).unwrap_or_default(),
        post_processed_text: None,
        post_process_prompt: None,
        post_process_requested: false,
        source_name: None,
        language: None,
        language_source: None,
        recording_removed: false,
        versions,
        window_context: Default::default(),
    }
}

/// A version made by `by` alone, without a time, for tests.
#[cfg(test)]
pub(crate) fn test_version(text: &str, by: VersionAuthor) -> HistoryVersion {
    HistoryVersion::made_of(text, VersionStep::new(by, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use VersionAuthor::{Ai, Transcription, User};

    fn v(text: &str, by: VersionAuthor) -> HistoryVersion {
        test_version(text, by)
    }

    fn step(by: VersionAuthor, at: i64) -> VersionStep {
        VersionStep {
            by,
            at: Some(at),
            prompt_name: None,
        }
    }

    fn texts(versions: &[HistoryVersion]) -> Vec<&str> {
        versions.iter().map(|v| v.text.as_str()).collect()
    }

    fn authors(versions: &[HistoryVersion]) -> Vec<Vec<VersionAuthor>> {
        versions
            .iter()
            .map(|v| v.steps.iter().map(|s| s.by).collect())
            .collect()
    }

    fn old<'a>(
        transcription: &'a str,
        transcription_correction: Option<&'a str>,
        post_processed: Option<&'a str>,
        from_correction: Option<&'a str>,
        post_processed_correction: Option<&'a str>,
    ) -> Vec<HistoryVersion> {
        StackColumns {
            transcription,
            versions: None,
            transcription_correction,
            post_processed,
            post_processed_correction,
            from_correction,
        }
        .versions()
    }

    // ---- Derived from the columns of before ------------------------------

    #[test]
    fn without_post_processing_or_correction_the_transcription_is_alone() {
        let versions = old("raw", None, None, None, None);
        assert_eq!(versions, vec![HistoryVersion::transcription("raw")]);
        assert_eq!(authors(&versions), vec![vec![Transcription]]);
    }

    #[test]
    fn a_corrected_transcription_is_a_users_version() {
        let versions = old("raw", Some("raw fixed"), None, None, None);
        assert_eq!(texts(&versions), vec!["raw", "raw fixed"]);
        assert_eq!(authors(&versions), vec![vec![Transcription], vec![User]]);
    }

    #[test]
    fn a_post_processing_is_an_ai_version() {
        let versions = old("raw", None, Some("Clean."), None, None);
        assert_eq!(texts(&versions), vec!["raw", "Clean."]);
        assert_eq!(authors(&versions), vec![vec![Transcription], vec![Ai]]);
    }

    #[test]
    fn a_post_processing_made_from_a_correction_sits_on_it() {
        let versions = old(
            "raw",
            Some("raw fixed"),
            Some("Clean."),
            Some("raw fixed"),
            None,
        );
        assert_eq!(texts(&versions), vec!["raw", "raw fixed", "Clean."]);
        assert_eq!(
            authors(&versions),
            vec![vec![Transcription], vec![User], vec![Ai]]
        );
    }

    #[test]
    fn a_correction_the_post_processing_was_not_made_from_is_left_out() {
        // Corrected after the post-processing ("outdated" before versions).
        let after = old("raw", Some("raw fixed"), Some("Clean."), None, None);
        assert_eq!(texts(&after), vec!["raw", "Clean."]);
        // Corrected again after the post-processing was made from the first.
        let again = old(
            "raw",
            Some("raw fixed again"),
            Some("Clean."),
            Some("raw fixed"),
            None,
        );
        assert_eq!(texts(&again), vec!["raw", "raw fixed", "Clean."]);
    }

    #[test]
    fn a_corrected_post_processing_is_a_users_version_on_top() {
        let versions = old("raw", None, Some("Clean."), None, Some("Cleaner."));
        assert_eq!(texts(&versions), vec!["raw", "Clean.", "Cleaner."]);
        assert_eq!(
            authors(&versions),
            vec![vec![Transcription], vec![Ai], vec![User]]
        );
        let all = old(
            "raw",
            Some("raw fixed"),
            Some("Clean."),
            Some("raw fixed"),
            Some("Cleaner."),
        );
        assert_eq!(texts(&all), vec!["raw", "raw fixed", "Clean.", "Cleaner."]);
    }

    #[test]
    fn blank_texts_count_as_none() {
        assert_eq!(
            texts(&old("raw", Some("raw fixed"), Some("  \n"), None, None)),
            vec!["raw", "raw fixed"]
        );
        assert_eq!(texts(&old("raw", Some(" "), None, None, None)), vec!["raw"]);
        assert_eq!(
            texts(&old("raw", None, Some("Clean."), Some(" "), Some("\n"))),
            vec!["raw", "Clean."]
        );
    }

    #[test]
    fn derived_steps_have_no_time_and_no_prompt() {
        let versions = old(
            "raw",
            Some("raw fixed"),
            Some("Clean."),
            Some("raw fixed"),
            Some("Cleaner."),
        );
        assert_eq!(versions.len(), 4);
        for step in versions.iter().flat_map(|v| &v.steps) {
            assert_eq!(step.at, None);
            assert_eq!(step.prompt_name, None);
        }
    }

    #[test]
    fn a_new_entry_has_the_stack_reading_it_back_gives() {
        assert_eq!(texts(&versions_of_new_entry("raw", None)), vec!["raw"]);
        let processed = versions_of_new_entry("raw", Some("Clean."));
        assert_eq!(texts(&processed), vec!["raw", "Clean."]);
        assert_eq!(authors(&processed), vec![vec![Transcription], vec![Ai]]);
    }

    // ---- Stored ------------------------------------------------------------

    fn stored<'a>(transcription: &'a str, json: &'a str) -> StackColumns<'a> {
        StackColumns {
            transcription,
            versions: Some(json),
            transcription_correction: Some("old correction"),
            post_processed: Some("Old post-processing."),
            post_processed_correction: Some("Old correction of it."),
            from_correction: None,
        }
    }

    #[test]
    fn stored_versions_win_over_the_columns_of_before() {
        let json = stored_json(&[
            HistoryVersion::transcription("ignored"),
            v("Clean.", Ai),
            v("Clean, mine.", User),
        ])
        .unwrap();
        let columns = stored("raw", &json);
        let versions = columns.versions();
        assert_eq!(texts(&versions), vec!["raw", "Clean.", "Clean, mine."]);
        assert_eq!(
            authors(&versions),
            vec![vec![Transcription], vec![Ai], vec![User]]
        );
    }

    #[test]
    fn an_empty_stored_stack_is_the_transcription_alone() {
        let versions = stored("raw", "[]").versions();
        assert_eq!(versions, vec![HistoryVersion::transcription("raw")]);
    }

    #[test]
    fn unreadable_stored_versions_fall_back_to_the_columns_of_before() {
        let columns = stored("raw", "{not json");
        assert_eq!(
            texts(&columns.versions()),
            vec!["raw", "Old post-processing.", "Old correction of it."]
        );
    }

    #[test]
    fn the_texts_are_those_of_the_versions() {
        let derived = StackColumns {
            transcription: "raw",
            transcription_correction: Some("raw fixed"),
            post_processed: Some("Clean."),
            from_correction: Some("raw fixed"),
            ..Default::default()
        };
        let json = stored_json(&[
            HistoryVersion::transcription("raw"),
            v("one", User),
            v("two", Ai),
        ])
        .unwrap();
        for columns in [derived, stored("raw", &json)] {
            let versions = columns.versions();
            let texts_only: Vec<String> =
                columns.texts().into_iter().map(Cow::into_owned).collect();
            assert_eq!(texts_only, texts(&versions));
            assert!(versions.len() > 1);
        }
    }

    #[test]
    fn the_column_stores_the_versions_above_the_transcription() {
        let json = stored_json(&[
            HistoryVersion::transcription("raw"),
            HistoryVersion::made_of(
                "Clean.",
                VersionStep {
                    by: Ai,
                    at: Some(1_700_000_000_123),
                    prompt_name: Some("E-Mail".to_string()),
                },
            ),
        ])
        .unwrap();
        assert_eq!(
            json,
            r#"[{"text":"Clean.","steps":[{"by":"ai","at":1700000000123,"prompt_name":"E-Mail"}]}]"#
        );
        assert_eq!(
            stored_json(&[HistoryVersion::transcription("raw")]).unwrap(),
            "[]"
        );
    }

    // ---- What an entry is ------------------------------------------------

    #[test]
    fn the_entry_is_its_top_version() {
        let entry = test_entry(vec![
            HistoryVersion::transcription("raw"),
            v("Clean.", Ai),
            v("Clean, mine.", User),
        ]);
        assert_eq!(entry_text(&entry), "Clean, mine.");
        let alone = test_entry(vec![HistoryVersion::transcription("raw")]);
        assert_eq!(entry_text(&alone), "raw");
    }

    #[test]
    fn has_ai_step_looks_at_every_step() {
        let mut edited = v("Clean.", User);
        edited.steps.push(step(Ai, 1));
        assert!(has_ai_step(&[HistoryVersion::transcription("raw"), edited]));
        assert!(!has_ai_step(&[
            HistoryVersion::transcription("raw"),
            v("mine", User)
        ]));
    }

    // ---- Saving the user's edit ------------------------------------------

    fn stack() -> Vec<HistoryVersion> {
        vec![
            HistoryVersion::transcription("raw"),
            v("Clean.", Ai),
            v("Clean, mine.", User),
        ]
    }

    #[test]
    fn a_new_version_on_the_transcription() {
        let versions = save_version(
            &[HistoryVersion::transcription("raw")],
            0,
            "raw fixed",
            SaveVersionMode::New,
            42,
        )
        .unwrap()
        .expect("changed");
        assert_eq!(texts(&versions), vec!["raw", "raw fixed"]);
        assert_eq!(versions[1].steps, vec![step(User, 42)]);
    }

    #[test]
    fn the_transcription_is_never_replaced() {
        assert_eq!(
            save_version(
                &[HistoryVersion::transcription("raw")],
                0,
                "raw fixed",
                SaveVersionMode::Replace,
                42
            ),
            Err(VersionError::TranscriptionIsFixed)
        );
    }

    #[test]
    fn replacing_a_version_keeps_its_steps_and_adds_one() {
        let versions = save_version(
            &stack()[..2],
            1,
            "Clean, fixed.",
            SaveVersionMode::Replace,
            42,
        )
        .unwrap()
        .expect("changed");
        assert_eq!(texts(&versions), vec!["raw", "Clean, fixed."]);
        assert_eq!(
            versions[1].steps,
            vec![VersionStep::new(Ai, None), step(User, 42)]
        );
    }

    #[test]
    fn a_new_version_on_top() {
        let versions = save_version(&stack(), 2, "Mine again.", SaveVersionMode::New, 42)
            .unwrap()
            .expect("changed");
        assert_eq!(
            texts(&versions),
            vec!["raw", "Clean.", "Clean, mine.", "Mine again."]
        );
    }

    #[test]
    fn saving_below_the_top_drops_the_versions_above() {
        let replaced = save_version(&stack(), 1, "Other.", SaveVersionMode::Replace, 42)
            .unwrap()
            .expect("changed");
        assert_eq!(texts(&replaced), vec!["raw", "Other."]);
        let added = save_version(&stack(), 1, "Other.", SaveVersionMode::New, 42)
            .unwrap()
            .expect("changed");
        assert_eq!(texts(&added), vec!["raw", "Clean.", "Other."]);
        let on_transcription = save_version(&stack(), 0, "raw fixed", SaveVersionMode::New, 42)
            .unwrap()
            .expect("changed");
        assert_eq!(texts(&on_transcription), vec!["raw", "raw fixed"]);
    }

    #[test]
    fn an_unchanged_text_changes_nothing_and_drops_nothing() {
        for mode in [SaveVersionMode::Replace, SaveVersionMode::New] {
            assert_eq!(save_version(&stack(), 1, " Clean.\n", mode, 42), Ok(None));
        }
        assert_eq!(
            save_version(&stack(), 0, "raw", SaveVersionMode::New, 42),
            Ok(None)
        );
    }

    #[test]
    fn replacing_a_version_with_the_text_below_it_removes_it() {
        let versions = save_version(&stack(), 2, "Clean.", SaveVersionMode::Replace, 42)
            .unwrap()
            .expect("changed");
        assert_eq!(texts(&versions), vec!["raw", "Clean."]);
        let below_drops_too = save_version(&stack(), 1, "raw", SaveVersionMode::Replace, 42)
            .unwrap()
            .expect("changed");
        assert_eq!(texts(&below_drops_too), vec!["raw"]);
    }

    #[test]
    fn a_blank_text_is_refused() {
        for mode in [SaveVersionMode::Replace, SaveVersionMode::New] {
            assert_eq!(
                save_version(&stack(), 1, " \n ", mode, 42),
                Err(VersionError::Blank)
            );
        }
    }

    #[test]
    fn an_unknown_version_is_refused() {
        assert_eq!(
            save_version(&stack(), 3, "x", SaveVersionMode::New, 42),
            Err(VersionError::NoSuchVersion)
        );
    }

    #[test]
    fn a_saved_text_keeps_its_line_breaks_but_not_outer_blanks() {
        let versions = save_version(&stack(), 2, "  first\nsecond \n", SaveVersionMode::New, 42)
            .unwrap()
            .expect("changed");
        assert_eq!(versions[3].text, "first\nsecond");
    }

    #[test]
    fn the_errors_reach_the_frontend_as_codes() {
        assert_eq!(VersionError::Blank.to_string(), "blank");
        assert_eq!(
            VersionError::TranscriptionIsFixed.to_string(),
            "transcription_is_fixed"
        );
        assert_eq!(VersionError::NoSuchVersion.to_string(), "no_such_version");
        assert_eq!(VersionError::NotMadeByAi.to_string(), "not_made_by_ai");
        assert_eq!(
            anyhow::Error::from(VersionError::Blank).to_string(),
            "blank"
        );
    }

    #[test]
    fn the_modes_reach_the_frontend_in_snake_case() {
        assert_eq!(
            serde_json::to_string(&SaveVersionMode::Replace).unwrap(),
            "\"replace\""
        );
        assert_eq!(
            serde_json::to_string(&SaveVersionMode::New).unwrap(),
            "\"new\""
        );
        assert_eq!(
            serde_json::to_string(&Transcription).unwrap(),
            "\"transcription\""
        );
    }

    // ---- Going back --------------------------------------------------------

    #[test]
    fn going_back_keeps_the_versions_up_to_the_one_chosen() {
        assert_eq!(
            texts(&revert_to_version(&stack(), 1).unwrap()),
            vec!["raw", "Clean."]
        );
        assert_eq!(texts(&revert_to_version(&stack(), 0).unwrap()), vec!["raw"]);
    }

    #[test]
    fn going_back_to_the_top_or_above_changes_nothing() {
        assert_eq!(revert_to_version(&stack(), 2), None);
        assert_eq!(revert_to_version(&stack(), 7), None);
    }

    // ---- Post-processing from History ------------------------------------

    use PostProcessTarget::{New, Redo, Replace};

    #[test]
    fn post_processing_starts_from_the_top_or_for_redo_below_it() {
        assert_eq!(post_processing_input(&stack(), New), Ok(2));
        assert_eq!(post_processing_input(&stack(), Replace), Ok(2));
        assert_eq!(post_processing_input(&stack()[..2], Redo), Ok(0));
        let transcription = [HistoryVersion::transcription("raw")];
        assert_eq!(post_processing_input(&transcription, New), Ok(0));
    }

    #[test]
    fn the_transcription_is_never_replaced_by_a_post_processing() {
        let transcription = [HistoryVersion::transcription("raw")];
        assert_eq!(
            post_processing_input(&transcription, Replace),
            Err(VersionError::TranscriptionIsFixed)
        );
        assert_eq!(
            post_processing_input(&transcription, Redo),
            Err(VersionError::TranscriptionIsFixed)
        );
    }

    #[test]
    fn only_a_version_a_post_processing_made_is_redone() {
        assert_eq!(
            post_processing_input(&stack(), Redo),
            Err(VersionError::NotMadeByAi)
        );
        // A post-processing the user edited afterwards still counts.
        let mut edited = stack()[..2].to_vec();
        edited[1].steps.push(step(User, 5));
        assert_eq!(post_processing_input(&edited, Redo), Ok(0));
    }

    #[test]
    fn post_processing_on_top_adds_a_version() {
        let versions = apply_post_processing(&stack(), New, "Polished.", step(Ai, 9));
        assert_eq!(
            texts(&versions),
            vec!["raw", "Clean.", "Clean, mine.", "Polished."]
        );
        assert_eq!(versions[3].steps, vec![step(Ai, 9)]);
    }

    #[test]
    fn replacing_the_top_keeps_its_steps_and_adds_the_ai_step() {
        let versions = apply_post_processing(&stack(), Replace, "Polished.", step(Ai, 9));
        assert_eq!(texts(&versions), vec!["raw", "Clean.", "Polished."]);
        assert_eq!(
            versions[2].steps,
            vec![VersionStep::new(User, None), step(Ai, 9)]
        );
    }

    #[test]
    fn redoing_replaces_the_top_with_one_ai_step() {
        let mut edited = stack()[..2].to_vec();
        edited[1].steps.push(step(User, 5));
        let versions = apply_post_processing(&edited, Redo, "Again.", step(Ai, 9));
        assert_eq!(texts(&versions), vec!["raw", "Again."]);
        assert_eq!(versions[1].steps, vec![step(Ai, 9)]);
    }

    #[test]
    fn a_result_equal_to_its_start_changes_nothing() {
        assert_eq!(
            apply_post_processing(&stack(), New, "Clean, mine.", step(Ai, 9)),
            stack()
        );
        assert_eq!(
            apply_post_processing(&stack(), Replace, " Clean, mine. ", step(Ai, 9)),
            stack()
        );
    }

    #[test]
    fn a_result_equal_to_the_version_below_drops_the_top() {
        assert_eq!(
            texts(&apply_post_processing(
                &stack(),
                Replace,
                "Clean.",
                step(Ai, 9)
            )),
            vec!["raw", "Clean."]
        );
        assert_eq!(
            texts(&apply_post_processing(
                &stack()[..2],
                Redo,
                "raw",
                step(Ai, 9)
            )),
            vec!["raw"]
        );
    }

    #[test]
    fn a_target_that_no_longer_fits_puts_the_result_on_top() {
        let transcription = [HistoryVersion::transcription("raw")];
        for target in [Replace, Redo] {
            let versions = apply_post_processing(&transcription, target, "Clean.", step(Ai, 9));
            assert_eq!(texts(&versions), vec!["raw", "Clean."], "{target:?}");
        }
        let versions = apply_post_processing(&stack(), Redo, "Again.", step(Ai, 9));
        assert_eq!(
            texts(&versions),
            vec!["raw", "Clean.", "Clean, mine.", "Again."]
        );
    }

    // ---- In the database ---------------------------------------------------

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

    fn stored_column(conn: &Connection, id: i64) -> Option<String> {
        conn.query_row(
            "SELECT versions FROM transcription_history WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .expect("read versions")
    }

    fn set(conn: &Connection, id: i64, column: &str, value: &str) {
        conn.execute(
            &format!("UPDATE transcription_history SET {column} = ?1 WHERE id = ?2"),
            params![value, id],
        )
        .expect("set column");
    }

    #[test]
    fn an_old_database_gets_the_versions_column_and_keeps_its_stacks() {
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
                post_process_requested BOOLEAN NOT NULL DEFAULT 0,
                transcription_correction TEXT,
                post_processed_correction TEXT,
                post_processed_from_correction TEXT
            );
            PRAGMA user_version = 4;
            INSERT INTO transcription_history (file_name, timestamp, title, transcription_text,
                post_processed_text, post_processed_correction)
            VALUES ('a.wav', 1, 'a', 'old words', 'Old words.', 'Old words, mine.');",
        )
        .expect("old schema");
        HistoryManager::apply_migrations(&mut conn).expect("migrate");

        let e = get(&conn, 1);
        assert_eq!(
            texts(&e.versions),
            vec!["old words", "Old words.", "Old words, mine."]
        );
        assert_eq!(entry_text(&e), "Old words, mine.");
        assert_eq!(stored_column(&conn, 1), None);
    }

    #[test]
    fn a_saved_version_is_stored_and_read_back() {
        let conn = database();
        let id = insert(&conn, "raw", Some("Clean."));
        let e = save_version_with_conn(&conn, id, 1, "Clean, mine.", SaveVersionMode::New, 42)
            .expect("save");
        assert_eq!(texts(&e.versions), vec!["raw", "Clean.", "Clean, mine."]);
        assert_eq!(e.versions[2].steps, vec![step(User, 42)]);
        assert_eq!(get(&conn, id).versions, e.versions);
        let column: Vec<HistoryVersion> =
            serde_json::from_str(&stored_column(&conn, id).expect("stored")).unwrap();
        assert_eq!(column, e.versions[1..]);
        // Handy's columns stay as they were.
        assert_eq!(e.post_processed_text.as_deref(), Some("Clean."));
        assert_eq!(e.transcription_text, "raw");
    }

    #[test]
    fn once_stored_the_columns_of_before_are_no_longer_read() {
        let conn = database();
        let id = insert(&conn, "raw", None);
        set(&conn, id, "transcription_correction", "raw fixed");
        assert_eq!(texts(&get(&conn, id).versions), vec!["raw", "raw fixed"]);

        save_version_with_conn(
            &conn,
            id,
            1,
            "raw fixed, mine",
            SaveVersionMode::Replace,
            42,
        )
        .expect("save");
        set(&conn, id, "transcription_correction", "something else");
        set(&conn, id, "post_processed_correction", "and more");

        let e = get(&conn, id);
        assert_eq!(texts(&e.versions), vec!["raw", "raw fixed, mine"]);
        assert_eq!(
            authors(&e.versions),
            vec![vec![Transcription], vec![User, User]]
        );
    }

    #[test]
    fn an_unchanged_save_writes_nothing() {
        let conn = database();
        let id = insert(&conn, "raw", Some("Clean."));
        let e = save_version_with_conn(&conn, id, 1, "Clean.", SaveVersionMode::Replace, 42)
            .expect("save");
        assert_eq!(texts(&e.versions), vec!["raw", "Clean."]);
        assert_eq!(stored_column(&conn, id), None);
    }

    #[test]
    fn a_refused_save_changes_nothing() {
        let conn = database();
        let id = insert(&conn, "raw", None);
        for (base, text, mode, code) in [
            (0, "x", SaveVersionMode::Replace, "transcription_is_fixed"),
            (0, " ", SaveVersionMode::New, "blank"),
            (4, "x", SaveVersionMode::New, "no_such_version"),
        ] {
            let error =
                save_version_with_conn(&conn, id, base, text, mode, 42).expect_err("refused");
            assert_eq!(error.to_string(), code);
        }
        assert_eq!(stored_column(&conn, id), None);
    }

    #[test]
    fn going_back_to_the_transcription_stays_there() {
        let conn = database();
        let id = insert(&conn, "raw", Some("Clean."));
        assert!(get(&conn, id).post_process_requested);

        let e = revert_to_version_with_conn(&conn, id, 0).expect("revert");

        assert_eq!(texts(&e.versions), vec!["raw"]);
        assert_eq!(stored_column(&conn, id).as_deref(), Some("[]"));
        assert!(!e.post_process_requested);
        // Handy's column keeps the latest post-processing.
        assert_eq!(e.post_processed_text.as_deref(), Some("Clean."));
        assert_eq!(entry_text(&get(&conn, id)), "raw");
    }

    #[test]
    fn going_back_keeps_the_entry_post_processed_while_a_post_processing_stays() {
        let conn = database();
        let id = insert(&conn, "raw", Some("Clean."));
        save_version_with_conn(&conn, id, 1, "Clean, mine.", SaveVersionMode::New, 42)
            .expect("save");

        let e = revert_to_version_with_conn(&conn, id, 1).expect("revert");

        assert_eq!(texts(&e.versions), vec!["raw", "Clean."]);
        assert!(e.post_process_requested);
    }

    #[test]
    fn going_back_to_the_top_changes_nothing() {
        let conn = database();
        let id = insert(&conn, "raw", Some("Clean."));
        let e = revert_to_version_with_conn(&conn, id, 1).expect("revert");
        assert_eq!(texts(&e.versions), vec!["raw", "Clean."]);
        assert_eq!(stored_column(&conn, id), None);
    }

    #[test]
    fn a_post_processing_from_history_becomes_a_version() {
        let conn = database();
        let id = insert(&conn, "raw words", None);
        assert!(!get(&conn, id).post_process_requested);

        let e = store_post_processing_with_conn(
            &conn,
            id,
            New,
            PostProcessed {
                text: "Clean words.".into(),
                prompt: Some("Fix the grammar".into()),
                prompt_name: Some("Grammar".into()),
            },
            77,
        )
        .expect("store");

        assert_eq!(e.transcription_text, "raw words");
        assert_eq!(texts(&e.versions), vec!["raw words", "Clean words."]);
        assert_eq!(
            e.versions[1].steps,
            vec![VersionStep {
                by: Ai,
                at: Some(77),
                prompt_name: Some("Grammar".into())
            }]
        );
        assert_eq!(e.post_processed_text.as_deref(), Some("Clean words."));
        assert_eq!(e.post_process_prompt.as_deref(), Some("Fix the grammar"));
        assert!(e.post_process_requested);
        assert_eq!(get(&conn, id).versions, e.versions);
    }

    #[test]
    fn a_post_processing_replacing_or_redoing_the_top_is_stored() {
        let conn = database();
        let id = insert(&conn, "raw", Some("Clean."));
        let result = |text: &str| PostProcessed {
            text: text.into(),
            ..Default::default()
        };

        let replaced = store_post_processing_with_conn(&conn, id, Replace, result("Cleaner."), 5)
            .expect("replace");
        assert_eq!(texts(&replaced.versions), vec!["raw", "Cleaner."]);
        assert_eq!(
            authors(&replaced.versions),
            vec![vec![Transcription], vec![Ai, Ai]]
        );

        let redone =
            store_post_processing_with_conn(&conn, id, Redo, result("Again."), 6).expect("redo");
        assert_eq!(texts(&redone.versions), vec!["raw", "Again."]);
        assert_eq!(redone.versions[1].steps, vec![step(Ai, 6)]);
        assert_eq!(redone.post_processed_text.as_deref(), Some("Again."));
        assert_eq!(get(&conn, id).versions, redone.versions);
    }

    #[test]
    fn transcribing_again_starts_the_stack_anew() {
        let conn = database();
        let id = insert(&conn, "raw", Some("Clean."));
        save_version_with_conn(&conn, id, 1, "Clean, mine.", SaveVersionMode::New, 42)
            .expect("save");

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
        assert_eq!(texts(&e.versions), vec!["new words", "New words."]);
        assert_eq!(stored_column(&conn, id), None);

        HistoryManager::update_transcription_with_conn(
            &conn,
            id,
            "plain".into(),
            None,
            None,
            None,
            false,
        )
        .expect("transcribe again without post-processing");
        assert_eq!(texts(&get(&conn, id).versions), vec!["plain"]);
    }

    #[test]
    fn a_missing_entry_fails() {
        let conn = database();
        assert!(save_version_with_conn(&conn, 42, 0, "x", SaveVersionMode::New, 1).is_err());
        assert!(revert_to_version_with_conn(&conn, 42, 0).is_err());
        assert!(
            store_post_processing_with_conn(&conn, 42, New, PostProcessed::default(), 1).is_err()
        );
    }
}
