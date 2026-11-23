//! Versions of a History entry (DanyWhere): a stack of texts. The speech
//! model's transcription is one version; every post-processing (✨) and every
//! edit the user saves as new (✎) puts another one on top. The top version is
//! what the entry "is": what History shows by default, what is copied, pasted
//! again and sent along as context.
//!
//! One rule makes every version (user, 2026-11-19, H1): post-processing any
//! version, saving an edit of any version as new, and transcribing again put
//! their result on top; nothing is deleted on the way. Only the top version
//! can be changed in place. All versions are alike (H3): any can be moved
//! anywhere and deleted, the transcription included, as long as one is left,
//! and nothing asks first (H4). A version doesn't remember which one it was
//! made from (H7), but it keeps its steps (who, when, with which prompt).
//!
//! Storage. While the column `versions` is NULL, the stack is derived from
//! the columns of before (see [`StackColumns`]), so a dictation needs no
//! change to Handy's code: one with post-processing is [transcription, ✨].
//! DanyWhere 0.2 stored the versions above the transcription as a JSON array,
//! the transcription itself in `transcription_text`; that is still read. Once
//! anything changes a stack, the whole stack is stored as `{"stack": [...]}`
//! and Handy's columns follow it, so that a deleted text stays nowhere:
//!
//! - `transcription_text` holds the entry's transcript, the version the
//!   speech model made last ([`transcript_index`]). When the user deleted
//!   every transcription it holds the top version's text instead, so Handy's
//!   checks for an entry with text still hold. Code that needs the transcript
//!   finds it by its step, never by its position ([`transcript`]).
//! - `post_processed_text` and `post_process_prompt` keep the latest
//!   post-processing only while the stack still has its text.
//! - the corrections from before versions are cleared: the stack holds what
//!   is left of them.
//!
//! Two things still replace versions, both outside History's buttons and
//! only while the stack is still the one a dictation made (`profile_redo`,
//! the redo of a dictation for the application it goes into): Handy's
//! `update_transcription` starts the stack anew, and the redo's
//! post-processing replaces the dictation's own ([`PostProcessTarget`]).

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use anyhow::Result;
use log::warn;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::language_detection::{LanguageDecision, LanguageSource};
use crate::managers::history::{HistoryEntry, HistoryManager};

/// Who made a version, or one step of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum VersionAuthor {
    /// The speech model.
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
    /// Unix milliseconds; None where it isn't known (steps from before
    /// versions existed that happened after the entry was made).
    pub at: Option<i64>,
    /// For an AI step: the prompt's name, when known.
    pub prompt_name: Option<String>,
    /// DanyWhere: for a transcription step, the id of the speech model that
    /// made it; `None` where it isn't known (before it was recorded). Left
    /// out of the column when `None`, so stacks stored without it read the
    /// same.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
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
    /// The top version gets the new text and one more step.
    Replace,
    /// The new text is a new version on top.
    New,
}

/// Why a stack can't change as asked. The commands return its
/// [`code`](VersionError::code).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VersionError {
    /// A version can't be empty.
    Blank,
    /// The entry has no version with that number.
    NoSuchVersion,
    /// Only the top version is changed in place; an edit of a version below
    /// it is saved as a new version on top.
    NotTop,
    /// The entry's only version can't be deleted; deleting the entry can.
    LastVersion,
}

impl VersionError {
    pub fn code(self) -> &'static str {
        match self {
            VersionError::Blank => "blank",
            VersionError::NoSuchVersion => "no_such_version",
            VersionError::NotTop => "not_top",
            VersionError::LastVersion => "last_version",
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
            model: None,
        }
    }
}

impl HistoryVersion {
    /// A transcription as the speech model `model` (its id, when known)
    /// wrote it at `at` (Unix milliseconds, when known).
    pub fn transcribed_by(text: &str, at: Option<i64>, model: Option<&str>) -> Self {
        let mut step = VersionStep::new(VersionAuthor::Transcription, at);
        step.model = model.map(str::to_string);
        Self::made_of(text, step)
    }

    /// A transcription by a model not known, for tests.
    #[cfg(test)]
    pub(crate) fn transcribed(text: &str, at: Option<i64>) -> Self {
        Self::transcribed_by(text, at, None)
    }

    /// A transcription without a time, for tests.
    #[cfg(test)]
    pub(crate) fn transcription(text: &str) -> Self {
        Self::transcribed(text, None)
    }

    fn made_of(text: &str, step: VersionStep) -> Self {
        Self {
            text: text.to_string(),
            steps: vec![step],
        }
    }

    /// Whether `by` made it (its first step).
    pub fn made_by(&self, by: VersionAuthor) -> bool {
        self.steps.first().is_some_and(|step| step.by == by)
    }

    /// Whether the user changed it, in any step.
    pub fn edited_by_user(&self) -> bool {
        self.steps.iter().any(|step| step.by == VersionAuthor::User)
    }
}

/// `text` unless it is blank.
fn non_blank(text: Option<&str>) -> Option<&str> {
    text.filter(|text| !text.trim().is_empty())
}

/// The column `versions` as stored.
#[derive(Deserialize)]
#[serde(untagged)]
enum Stored {
    /// The whole stack, bottom to top.
    Whole { stack: Vec<HistoryVersion> },
    /// DanyWhere 0.2's: the versions above `transcription_text`.
    Above(Vec<HistoryVersion>),
}

/// The whole stack as the column stores it.
#[derive(Serialize)]
struct WholeStack<'a> {
    stack: &'a [HistoryVersion],
}

/// The columns an entry's stack is read from.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct StackColumns<'a> {
    pub transcription: &'a str,
    /// The stack as stored; `None` (NULL): derived from the rest.
    pub versions: Option<&'a str>,
    /// The user's correction of the transcription, from before versions.
    pub transcription_correction: Option<&'a str>,
    pub post_processed: Option<&'a str>,
    /// The user's correction of the post-processing, from before versions.
    pub post_processed_correction: Option<&'a str>,
    /// The transcription's correction the post-processing was made from.
    pub from_correction: Option<&'a str>,
    /// When the entry was made (Unix milliseconds): the time of the steps
    /// derived without one that happened then, the transcription and a
    /// dictation's own post-processing.
    pub made_at: Option<i64>,
    /// A transcribed file's entry, whose post-processing may have come later.
    pub is_file: bool,
    /// The id of the speech model that made `transcription`, when known.
    pub transcription_model: Option<&'a str>,
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
            made_at: row
                .get::<_, Option<i64>>("timestamp")?
                .map(|seconds| seconds.saturating_mul(1000)),
            is_file: text_ref(row, "source_name")?.is_some(),
            transcription_model: text_ref(row, "transcription_model")?,
        })
    }

    /// The transcription of `transcription_text`.
    fn transcription_version(&self) -> HistoryVersion {
        HistoryVersion::transcribed_by(self.transcription, self.made_at, self.transcription_model)
    }

    /// The versions above the transcription of an entry from before
    /// versions existed, oldest first, with when they were made where that
    /// is known: with a post-processing, the correction it was made from (if
    /// any), the post-processing (✨) and its correction (if any); without
    /// one, the transcription's correction (if any). A correction of the
    /// transcription the post-processing was not made from fits no stack,
    /// so it is left out. Blank texts count as none. A dictation's own
    /// post-processing ran when it was made; one made from a correction came
    /// later, as may a file's.
    fn derived(&self) -> Vec<(VersionAuthor, &'a str, Option<i64>)> {
        let mut versions = Vec::new();
        match non_blank(self.post_processed) {
            Some(post_processed) => {
                let from = non_blank(self.from_correction);
                if let Some(from) = from {
                    versions.push((VersionAuthor::User, from, None));
                }
                let at = if from.is_none() && !self.is_file {
                    self.made_at
                } else {
                    None
                };
                versions.push((VersionAuthor::Ai, post_processed, at));
                if let Some(correction) = non_blank(self.post_processed_correction) {
                    versions.push((VersionAuthor::User, correction, None));
                }
            }
            None => {
                if let Some(correction) = non_blank(self.transcription_correction) {
                    versions.push((VersionAuthor::User, correction, None));
                }
            }
        }
        versions
    }

    /// The column `versions` as stored; `None` when it is NULL or can't be
    /// read.
    fn stored(&self) -> Option<Stored> {
        let json = self.versions?;
        match serde_json::from_str(json) {
            Ok(stored) => Some(stored),
            Err(e) => {
                warn!("Ignoring the stored versions of a History entry: {e}");
                None
            }
        }
    }

    /// The whole stack, bottom to top.
    pub fn versions(&self) -> Vec<HistoryVersion> {
        match self.stored() {
            Some(Stored::Whole { stack }) if !stack.is_empty() => stack,
            Some(Stored::Whole { .. }) => vec![self.transcription_version()],
            Some(Stored::Above(above)) => std::iter::once(self.transcription_version())
                .chain(above)
                .collect(),
            None => {
                std::iter::once(self.transcription_version())
                    .chain(self.derived().into_iter().map(|(by, text, at)| {
                        HistoryVersion::made_of(text, VersionStep::new(by, at))
                    }))
                    .collect()
            }
        }
    }

    /// The texts of [`Self::versions`], copied only when they are stored.
    pub fn texts(&self) -> Vec<Cow<'a, str>> {
        match self.stored() {
            Some(Stored::Whole { stack }) if !stack.is_empty() => {
                stack.into_iter().map(|v| Cow::Owned(v.text)).collect()
            }
            Some(Stored::Whole { .. }) => vec![Cow::Borrowed(self.transcription)],
            Some(Stored::Above(above)) => std::iter::once(Cow::Borrowed(self.transcription))
                .chain(above.into_iter().map(|v| Cow::Owned(v.text)))
                .collect(),
            None => std::iter::once(Cow::Borrowed(self.transcription))
                .chain(self.derived().into_iter().map(|(_, t, _)| Cow::Borrowed(t)))
                .collect(),
        }
    }
}

/// The stack of an entry just made at `made_at` (Unix milliseconds) by the
/// speech model `model`, as reading it back gives it.
pub fn versions_of_new_entry(
    transcription: &str,
    post_processed: Option<&str>,
    made_at: Option<i64>,
    model: Option<&str>,
) -> Vec<HistoryVersion> {
    StackColumns {
        transcription,
        post_processed,
        made_at,
        transcription_model: model,
        ..Default::default()
    }
    .versions()
}

/// DanyWhere: names the prompt `prompt_name` in the ✨ steps of a new
/// entry's `versions`, made by its own post-processing, as a
/// post-processing made later in History names it.
pub(crate) fn name_the_prompt(versions: &mut [HistoryVersion], prompt_name: Option<&str>) {
    let Some(name) = prompt_name.filter(|name| !name.trim().is_empty()) else {
        return;
    };
    for step in versions
        .iter_mut()
        .flat_map(|version| version.steps.iter_mut())
        .filter(|step| step.by == VersionAuthor::Ai)
    {
        step.prompt_name = Some(name.to_string());
    }
}

/// DanyWhere: what the column `versions` stores for a new `entry`: nothing
/// (NULL) while its other columns give its stack back, as for every entry
/// made before; else the whole stack, such as one whose ✨ step names its
/// prompt ([`name_the_prompt`]), which those columns don't hold.
pub(crate) fn column_of_new_entry(entry: &HistoryEntry) -> Result<Option<String>> {
    let derived = StackColumns {
        transcription: &entry.transcription_text,
        post_processed: entry.post_processed_text.as_deref(),
        made_at: Some(entry.timestamp.saturating_mul(1000)),
        is_file: entry.source_name.is_some(),
        transcription_model: entry.transcription_model.as_deref(),
        ..Default::default()
    }
    .versions();
    if derived == entry.versions {
        Ok(None)
    } else {
        stored_json(&entry.versions).map(Some)
    }
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

/// The index of the entry's transcript: of the versions the speech model
/// made, the one it made last (a step without a time counts as the oldest;
/// of equal times, the higher one wins). `None` once the user deleted every
/// transcription.
pub fn transcript_index(versions: &[HistoryVersion]) -> Option<usize> {
    versions
        .iter()
        .enumerate()
        .filter(|(_, version)| version.made_by(VersionAuthor::Transcription))
        .max_by_key(|(index, version)| (version.steps.first().and_then(|step| step.at), *index))
        .map(|(index, _)| index)
}

/// The entry's transcript (see [`transcript_index`]).
pub fn transcript(versions: &[HistoryVersion]) -> Option<&HistoryVersion> {
    transcript_index(versions).map(|index| &versions[index])
}

/// What `transcription_text` holds for a stack: its transcript, or the top
/// version's text once no transcription is left.
fn transcript_column(versions: &[HistoryVersion]) -> &str {
    transcript(versions)
        .or_else(|| versions.last())
        .map_or("", |version| version.text.as_str())
}

/// The stack after the user saved `text` (trimmed), an edit of version
/// `base`, at `at` (Unix milliseconds); `None`: nothing changes. `New` puts
/// it on top as a new version, whichever version it was made from;
/// `Replace` gives the top version the text and a ✎ step, and only the top.
/// A text equal to the one of `base` or of the top changes nothing.
pub fn save_version(
    versions: &[HistoryVersion],
    base: u32,
    text: &str,
    mode: SaveVersionMode,
    at: i64,
) -> std::result::Result<Option<Vec<HistoryVersion>>, VersionError> {
    let base = base as usize;
    let (Some(base_version), Some(top)) = (versions.get(base), versions.last()) else {
        return Err(VersionError::NoSuchVersion);
    };
    let text = text.trim();
    if text.is_empty() {
        return Err(VersionError::Blank);
    }
    if mode == SaveVersionMode::Replace && base + 1 != versions.len() {
        return Err(VersionError::NotTop);
    }
    if text == base_version.text.trim() || text == top.text.trim() {
        return Ok(None);
    }
    let step = VersionStep::new(VersionAuthor::User, Some(at));
    let mut saved = versions.to_vec();
    match (mode, saved.last_mut()) {
        (SaveVersionMode::Replace, Some(top)) => {
            top.text = text.to_string();
            top.steps.push(step);
        }
        _ => saved.push(HistoryVersion::made_of(text, step)),
    }
    Ok(Some(saved))
}

/// The stack without version `index`. Any version can go, the
/// transcription too, as long as one is left.
pub fn delete_version(
    versions: &[HistoryVersion],
    index: u32,
) -> std::result::Result<Vec<HistoryVersion>, VersionError> {
    let index = index as usize;
    if index >= versions.len() {
        return Err(VersionError::NoSuchVersion);
    }
    if versions.len() == 1 {
        return Err(VersionError::LastVersion);
    }
    let mut left = versions.to_vec();
    left.remove(index);
    Ok(left)
}

/// The stack with version `from` moved to place `to` (both counted from
/// the bottom, `to` as it is afterwards); `None`: it is there already.
pub fn move_version(
    versions: &[HistoryVersion],
    from: u32,
    to: u32,
) -> std::result::Result<Option<Vec<HistoryVersion>>, VersionError> {
    let (from, to) = (from as usize, to as usize);
    if from >= versions.len() || to >= versions.len() {
        return Err(VersionError::NoSuchVersion);
    }
    if from == to {
        return Ok(None);
    }
    let mut moved = versions.to_vec();
    let version = moved.remove(from);
    moved.insert(to, version);
    Ok(Some(moved))
}

/// Where a post-processing's result goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostProcessTarget {
    /// On top, as a new version: every post-processing from History.
    OnTop,
    /// In place of the top version while a post-processing made it and the
    /// user didn't change it, else on top: the redo of a dictation for the
    /// application it goes into, which replaces the dictation's own
    /// post-processing (`profile_redo`).
    InPlaceOfAiTop,
}

/// The number of the version a post-processing starts from: `version`, or
/// the top when it is `None`.
pub fn post_processing_input(
    versions: &[HistoryVersion],
    version: Option<u32>,
) -> std::result::Result<usize, VersionError> {
    let input = match version {
        Some(version) => version as usize,
        None => versions
            .len()
            .checked_sub(1)
            .ok_or(VersionError::NoSuchVersion)?,
    };
    if input < versions.len() {
        Ok(input)
    } else {
        Err(VersionError::NoSuchVersion)
    }
}

/// The stack with a post-processing's `text` (trimmed) applied as `target`
/// says, to the stack as it is when the post-processing ends; `None` when
/// it stays as it is. A result equal to the top's text adds nothing, which
/// History tells the user, since nothing visible happens then.
pub fn apply_post_processing(
    versions: &[HistoryVersion],
    target: PostProcessTarget,
    text: &str,
    step: VersionStep,
) -> Option<Vec<HistoryVersion>> {
    let text = text.trim();
    let mut applied = versions.to_vec();
    let replaces_top = target == PostProcessTarget::InPlaceOfAiTop
        && applied
            .last()
            .is_some_and(|top| top.made_by(VersionAuthor::Ai) && !top.edited_by_user());
    if replaces_top {
        applied.pop();
    }
    if applied.last().is_none_or(|top| top.text.trim() != text) {
        applied.push(HistoryVersion::made_of(text, step));
    }
    (applied != versions).then_some(applied)
}

/// The whole stack as the column `versions` stores it.
fn stored_json(versions: &[HistoryVersion]) -> Result<String> {
    Ok(serde_json::to_string(&WholeStack { stack: versions })?)
}

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Stores `versions` as the stack of `entry`, with Handy's columns following
/// it (see the module's doc): `transcription_text` its transcript;
/// `post_processed` (text and prompt) a new post-processing, else the latest
/// one stays only while the stack has its text; the corrections from before
/// versions cleared. `requested`: whether the entry counts as post-processed
/// from now on, `None` leaves it.
fn store_stack(
    conn: &Connection,
    entry: &HistoryEntry,
    versions: &[HistoryVersion],
    post_processed: Option<(&str, Option<&str>)>,
    requested: Option<bool>,
) -> Result<()> {
    let (post_processed_text, post_process_prompt) = match post_processed {
        Some((text, prompt)) => (Some(text), prompt),
        None => {
            let kept = entry
                .post_processed_text
                .as_deref()
                .filter(|text| versions.iter().any(|v| v.text.trim() == text.trim()));
            (kept, kept.and(entry.post_process_prompt.as_deref()))
        }
    };
    conn.execute(
        "UPDATE transcription_history
         SET versions = ?1,
             transcription_text = ?2,
             post_processed_text = ?3,
             post_process_prompt = ?4,
             post_process_requested = COALESCE(?5, post_process_requested),
             transcription_correction = NULL,
             post_processed_correction = NULL,
             post_processed_from_correction = NULL
         WHERE id = ?6",
        params![
            stored_json(versions)?,
            transcript_column(versions),
            post_processed_text,
            post_process_prompt,
            requested,
            entry.id
        ],
    )?;
    Ok(())
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
    store_stack(conn, &entry, &versions, None, None)?;
    HistoryManager::entry_with_conn(conn, id)
}

/// See [`delete_version`]. When the version deleted had a ✨ step, the
/// entry counts as post-processed only while another version with one
/// stays, so transcribing it again post-processes only then.
pub(crate) fn delete_version_with_conn(
    conn: &Connection,
    id: i64,
    index: u32,
) -> Result<HistoryEntry> {
    let entry = HistoryManager::entry_with_conn(conn, id)?;
    let versions = delete_version(&entry.versions, index)?;
    let deleted = entry.versions.get(index as usize);
    let requested = deleted
        .is_some_and(|version| has_ai_step(std::slice::from_ref(version)))
        .then(|| has_ai_step(&versions));
    store_stack(conn, &entry, &versions, None, requested)?;
    HistoryManager::entry_with_conn(conn, id)
}

/// See [`move_version`].
pub(crate) fn move_version_with_conn(
    conn: &Connection,
    id: i64,
    from: u32,
    to: u32,
) -> Result<HistoryEntry> {
    let entry = HistoryManager::entry_with_conn(conn, id)?;
    let Some(versions) = move_version(&entry.versions, from, to)? else {
        return Ok(entry);
    };
    store_stack(conn, &entry, &versions, None, None)?;
    HistoryManager::entry_with_conn(conn, id)
}

/// What a post-processing came back with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PostProcessed {
    pub text: String,
    /// The prompt's text, for Handy's `post_process_prompt`.
    pub prompt: Option<String>,
    /// The prompt's name, for the ✨ step.
    pub prompt_name: Option<String>,
}

/// What storing a post-processing did.
#[derive(Clone, Debug)]
pub(crate) struct StoredPostProcessing {
    /// The entry as it is now.
    pub entry: HistoryEntry,
    /// Whether the stack stayed as it was: the result equals the top's text
    /// ([`apply_post_processing`]). History says so, since no version came.
    pub unchanged: bool,
}

/// Applies a post-processing to the entry's stack as it is now (see
/// [`apply_post_processing`]) at `at` (Unix milliseconds). Handy's
/// `post_processed_text` and `post_process_prompt` get the result, and the
/// entry counts as post-processed from now on, even when the stack stays.
pub(crate) fn store_post_processing_with_conn(
    conn: &Connection,
    id: i64,
    target: PostProcessTarget,
    result: PostProcessed,
    at: i64,
) -> Result<StoredPostProcessing> {
    let entry = HistoryManager::entry_with_conn(conn, id)?;
    let step = VersionStep {
        by: VersionAuthor::Ai,
        at: Some(at),
        prompt_name: result.prompt_name,
        model: None,
    };
    let applied = apply_post_processing(&entry.versions, target, &result.text, step);
    let unchanged = applied.is_none();
    let versions = applied.unwrap_or_else(|| entry.versions.clone());
    store_stack(
        conn,
        &entry,
        &versions,
        Some((&result.text, result.prompt.as_deref())),
        Some(true),
    )?;
    Ok(StoredPostProcessing {
        entry: HistoryManager::entry_with_conn(conn, id)?,
        unchanged,
    })
}

/// A transcription made again from History ("Transcribe Again"), or a
/// file's first one.
#[derive(Clone, Debug, Default)]
pub(crate) struct NewTranscription {
    pub text: String,
    pub language: Option<LanguageDecision>,
    /// Its post-processing, when one ran and came back with text.
    pub post_processed: Option<PostProcessed>,
    /// Whether the entry counts as post-processed from now on.
    pub post_process_requested: bool,
    /// The id of the speech model that made it.
    pub model: Option<String>,
}

/// Puts a new transcription on top of the entry's stack at `at` (Unix
/// milliseconds), and its post-processing on top of it; the versions there
/// stay, except the empty transcription of an entry not transcribed yet (a
/// file, a dictation whose transcription failed). A text equal to the top's
/// adds nothing. The entry's language and speech model become the new
/// transcription's.
/// Errors: `blank` for an empty transcription, or the database's.
pub(crate) fn add_transcription_with_conn(
    conn: &Connection,
    id: i64,
    new: NewTranscription,
    at: i64,
) -> Result<HistoryEntry> {
    let text = new.text.trim();
    if text.is_empty() {
        return Err(VersionError::Blank.into());
    }
    let entry = HistoryManager::entry_with_conn(conn, id)?;
    let mut versions: Vec<HistoryVersion> = entry
        .versions
        .iter()
        .filter(|version| !version.text.trim().is_empty())
        .cloned()
        .collect();
    if versions.last().is_none_or(|top| top.text.trim() != text) {
        versions.push(HistoryVersion::transcribed_by(
            text,
            Some(at),
            new.model.as_deref(),
        ));
    }
    if let Some(post_processed) = &new.post_processed {
        let step = VersionStep {
            by: VersionAuthor::Ai,
            at: Some(at),
            prompt_name: post_processed.prompt_name.clone(),
            model: None,
        };
        if let Some(applied) = apply_post_processing(
            &versions,
            PostProcessTarget::OnTop,
            &post_processed.text,
            step,
        ) {
            versions = applied;
        }
    }
    let (language, language_source) = match new.language {
        Some(decision) => (decision.language, Some(decision.source)),
        None => (None, None),
    };
    let transaction = conn.unchecked_transaction()?;
    store_stack(
        &transaction,
        &entry,
        &versions,
        new.post_processed
            .as_ref()
            .map(|p| (p.text.as_str(), p.prompt.as_deref())),
        Some(new.post_process_requested),
    )?;
    transaction.execute(
        "UPDATE transcription_history
         SET language = ?1, language_source = ?2, transcription_model = ?3
         WHERE id = ?4",
        params![
            language,
            language_source.map(LanguageSource::as_str),
            new.model,
            id
        ],
    )?;
    transaction.commit()?;
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

    /// See [`delete_version_with_conn`]; tells the frontend.
    pub fn delete_version(&self, id: i64, index: u32) -> Result<HistoryEntry> {
        let conn = self.get_connection()?;
        let entry = delete_version_with_conn(&conn, id, index)?;
        self.emit_entry(&entry);
        Ok(entry)
    }

    /// See [`move_version_with_conn`]; tells the frontend.
    pub fn move_version(&self, id: i64, from: u32, to: u32) -> Result<HistoryEntry> {
        let conn = self.get_connection()?;
        let entry = move_version_with_conn(&conn, id, from, to)?;
        self.emit_entry(&entry);
        Ok(entry)
    }

    /// See [`store_post_processing_with_conn`]; tells the frontend.
    pub(crate) fn store_post_processing(
        &self,
        id: i64,
        target: PostProcessTarget,
        result: PostProcessed,
    ) -> Result<StoredPostProcessing> {
        let conn = self.get_connection()?;
        let stored = store_post_processing_with_conn(&conn, id, target, result, now_millis())?;
        self.emit_entry(&stored.entry);
        Ok(stored)
    }

    /// See [`add_transcription_with_conn`]; tells the frontend.
    pub(crate) fn add_transcription(&self, id: i64, new: NewTranscription) -> Result<HistoryEntry> {
        let conn = self.get_connection()?;
        let entry = add_transcription_with_conn(&conn, id, new, now_millis())?;
        self.emit_entry(&entry);
        Ok(entry)
    }
}

/// Saves the user's `text`, an edit of version `base` of the entry:
/// `replace` changes the top version (only the top), `new` puts a new
/// version on top. Errors: `blank`, `not_top` (replacing a version below
/// the top), `no_such_version`, or the database's message.
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

/// Deletes version `index` of the entry, without asking: any version, the
/// transcription too. Errors: `last_version` (the only one),
/// `no_such_version`, or the database's message.
#[tauri::command]
#[specta::specta]
pub fn delete_history_version(
    history: State<'_, Arc<HistoryManager>>,
    id: i64,
    index: u32,
) -> std::result::Result<HistoryEntry, String> {
    history.delete_version(id, index).map_err(|e| e.to_string())
}

/// Moves version `from` of the entry to place `to` (counted from the
/// bottom, as it is afterwards); the top one is the entry's result. Errors:
/// `no_such_version`, or the database's message.
#[tauri::command]
#[specta::specta]
pub fn move_history_version(
    history: State<'_, Arc<HistoryManager>>,
    id: i64,
    from: u32,
    to: u32,
) -> std::result::Result<HistoryEntry, String> {
    history
        .move_version(id, from, to)
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
        transcription_text: transcript_column(&versions).to_string(),
        post_processed_text: None,
        post_process_prompt: None,
        post_process_requested: false,
        source_name: None,
        language: None,
        language_source: None,
        transcription_model: None,
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
            model: None,
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

    fn times(versions: &[HistoryVersion]) -> Vec<Vec<Option<i64>>> {
        versions
            .iter()
            .map(|v| v.steps.iter().map(|s| s.at).collect())
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
            ..Default::default()
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
    fn derived_steps_without_the_entrys_time_have_none_and_no_prompt() {
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
    fn derived_steps_that_happened_when_the_entry_was_made_get_its_time() {
        let columns = |is_file| StackColumns {
            transcription: "raw",
            post_processed: Some("Clean."),
            post_processed_correction: Some("Cleaner."),
            made_at: Some(5_000),
            is_file,
            ..Default::default()
        };
        // A dictation post-processed right away.
        assert_eq!(
            times(&columns(false).versions()),
            vec![vec![Some(5_000)], vec![Some(5_000)], vec![None]]
        );
        // A file's post-processing may have come later.
        assert_eq!(
            times(&columns(true).versions()),
            vec![vec![Some(5_000)], vec![None], vec![None]]
        );
        // So did one made from a correction.
        let from_correction = StackColumns {
            transcription_correction: Some("raw fixed"),
            from_correction: Some("raw fixed"),
            ..columns(false)
        };
        assert_eq!(
            times(&from_correction.versions()),
            vec![vec![Some(5_000)], vec![None], vec![None], vec![None]]
        );
    }

    #[test]
    fn a_new_entry_has_the_stack_reading_it_back_gives() {
        assert_eq!(
            texts(&versions_of_new_entry("raw", None, None, None)),
            vec!["raw"]
        );
        let processed = versions_of_new_entry("raw", Some("Clean."), Some(7_000), None);
        assert_eq!(texts(&processed), vec!["raw", "Clean."]);
        assert_eq!(authors(&processed), vec![vec![Transcription], vec![Ai]]);
        assert_eq!(
            times(&processed),
            vec![vec![Some(7_000)], vec![Some(7_000)]]
        );
    }

    // ---- Stored ------------------------------------------------------------

    fn stored<'a>(transcription: &'a str, json: &'a str) -> StackColumns<'a> {
        StackColumns {
            transcription,
            versions: Some(json),
            transcription_correction: Some("old correction"),
            post_processed: Some("Old post-processing."),
            post_processed_correction: Some("Old correction of it."),
            ..Default::default()
        }
    }

    /// The versions above the transcription as DanyWhere 0.2 stored them.
    fn above_json(versions: &[HistoryVersion]) -> String {
        serde_json::to_string(versions).unwrap()
    }

    #[test]
    fn the_versions_above_the_transcription_from_before_are_still_read() {
        let json = above_json(&[v("Clean.", Ai), v("Clean, mine.", User)]);
        let columns = stored("raw", &json);
        let versions = columns.versions();
        assert_eq!(texts(&versions), vec!["raw", "Clean.", "Clean, mine."]);
        assert_eq!(
            authors(&versions),
            vec![vec![Transcription], vec![Ai], vec![User]]
        );
        assert_eq!(texts(&stored("raw", "[]").versions()), vec!["raw"]);
    }

    #[test]
    fn a_whole_stack_is_read_as_stored_without_the_transcription_column() {
        let json = stored_json(&[v("Clean.", Ai), HistoryVersion::transcription("raw")]).unwrap();
        let versions = stored("stale", &json).versions();
        assert_eq!(texts(&versions), vec!["Clean.", "raw"]);
        assert_eq!(authors(&versions), vec![vec![Ai], vec![Transcription]]);
    }

    #[test]
    fn an_empty_whole_stack_is_the_transcription_alone() {
        let versions = stored("raw", r#"{"stack":[]}"#).versions();
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
        let above = above_json(&[v("one", User), v("two", Ai)]);
        let whole = stored_json(&[v("two", Ai), HistoryVersion::transcription("raw")]).unwrap();
        for columns in [derived, stored("raw", &above), stored("stale", &whole)] {
            let versions = columns.versions();
            let texts_only: Vec<String> =
                columns.texts().into_iter().map(Cow::into_owned).collect();
            assert_eq!(texts_only, texts(&versions));
            assert!(versions.len() > 1);
        }
    }

    #[test]
    fn the_column_stores_the_whole_stack() {
        let json = stored_json(&[
            HistoryVersion::transcription("raw"),
            HistoryVersion::made_of(
                "Clean.",
                VersionStep {
                    by: Ai,
                    at: Some(1_700_000_000_123),
                    prompt_name: Some("E-Mail".to_string()),
                    model: None,
                },
            ),
        ])
        .unwrap();
        assert_eq!(
            json,
            r#"{"stack":[{"text":"raw","steps":[{"by":"transcription","at":null,"prompt_name":null}]},{"text":"Clean.","steps":[{"by":"ai","at":1700000000123,"prompt_name":"E-Mail"}]}]}"#
        );
    }

    #[test]
    fn the_column_stores_the_model_of_a_transcription() {
        let json = stored_json(&[HistoryVersion::transcribed_by(
            "raw",
            Some(5),
            Some("canary-1b-v2"),
        )])
        .unwrap();
        assert_eq!(
            json,
            r#"{"stack":[{"text":"raw","steps":[{"by":"transcription","at":5,"prompt_name":null,"model":"canary-1b-v2"}]}]}"#
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

    #[test]
    fn the_transcript_is_found_by_its_step_wherever_it_is() {
        let moved = [v("Clean.", Ai), HistoryVersion::transcription("raw")];
        assert_eq!(transcript_index(&moved), Some(1));
        assert_eq!(transcript(&moved).map(|v| v.text.as_str()), Some("raw"));
        assert_eq!(transcript_index(&[v("Clean.", Ai), v("mine", User)]), None);
        assert_eq!(transcript_index(&[]), None);
    }

    #[test]
    fn the_transcript_is_the_one_the_speech_model_made_last() {
        let again = [
            HistoryVersion::transcribed("old", Some(1)),
            v("Old.", Ai),
            HistoryVersion::transcribed("new", Some(5)),
        ];
        assert_eq!(transcript_index(&again), Some(2));
        // Moved below the old one, it is still the last one made.
        let moved = [
            HistoryVersion::transcribed("new", Some(5)),
            HistoryVersion::transcribed("old", Some(1)),
        ];
        assert_eq!(transcript_index(&moved), Some(0));
        // One without a time is the oldest; of equal times the higher one.
        let untimed = [
            HistoryVersion::transcribed("timed", Some(1)),
            HistoryVersion::transcription("untimed"),
        ];
        assert_eq!(transcript_index(&untimed), Some(0));
        let equal = [
            HistoryVersion::transcription("a"),
            HistoryVersion::transcription("b"),
        ];
        assert_eq!(transcript_index(&equal), Some(1));
    }

    #[test]
    fn handys_transcription_column_holds_the_transcript_else_the_top() {
        assert_eq!(
            transcript_column(&[v("Clean.", Ai), HistoryVersion::transcription("raw")]),
            "raw"
        );
        assert_eq!(
            transcript_column(&[v("mine", User), v("Clean.", Ai)]),
            "Clean."
        );
        assert_eq!(transcript_column(&[]), "");
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
    fn an_edit_of_any_version_saved_as_new_goes_on_top() {
        for base in 0..3 {
            let versions = save_version(&stack(), base, "Other.", SaveVersionMode::New, 42)
                .unwrap()
                .expect("changed");
            assert_eq!(
                texts(&versions),
                vec!["raw", "Clean.", "Clean, mine.", "Other."],
                "base {base}"
            );
        }
    }

    #[test]
    fn replacing_the_top_keeps_its_steps_and_adds_one() {
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
    fn only_the_top_is_replaced() {
        for base in [0, 1] {
            assert_eq!(
                save_version(&stack(), base, "Other.", SaveVersionMode::Replace, 42),
                Err(VersionError::NotTop),
                "base {base}"
            );
        }
    }

    #[test]
    fn a_transcription_on_top_is_replaced_like_any_version() {
        let versions = save_version(
            &[HistoryVersion::transcription("raw")],
            0,
            "raw fixed",
            SaveVersionMode::Replace,
            42,
        )
        .unwrap()
        .expect("changed");
        assert_eq!(texts(&versions), vec!["raw fixed"]);
        assert_eq!(authors(&versions), vec![vec![Transcription, User]]);
    }

    #[test]
    fn replacing_the_top_with_the_text_below_keeps_both() {
        let versions = save_version(&stack(), 2, "Clean.", SaveVersionMode::Replace, 42)
            .unwrap()
            .expect("changed");
        assert_eq!(texts(&versions), vec!["raw", "Clean.", "Clean."]);
    }

    #[test]
    fn an_unchanged_text_changes_nothing() {
        assert_eq!(
            save_version(&stack(), 1, " Clean.\n", SaveVersionMode::New, 42),
            Ok(None)
        );
        assert_eq!(
            save_version(&stack(), 2, "Clean, mine.", SaveVersionMode::Replace, 42),
            Ok(None)
        );
        assert_eq!(
            save_version(&stack(), 0, "raw", SaveVersionMode::New, 42),
            Ok(None)
        );
    }

    #[test]
    fn a_new_version_equal_to_the_top_changes_nothing() {
        assert_eq!(
            save_version(&stack(), 0, "Clean, mine.", SaveVersionMode::New, 42),
            Ok(None)
        );
    }

    #[test]
    fn a_blank_text_is_refused() {
        for mode in [SaveVersionMode::Replace, SaveVersionMode::New] {
            assert_eq!(
                save_version(&stack(), 2, " \n ", mode, 42),
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
        assert_eq!(VersionError::NoSuchVersion.to_string(), "no_such_version");
        assert_eq!(VersionError::NotTop.to_string(), "not_top");
        assert_eq!(VersionError::LastVersion.to_string(), "last_version");
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

    // ---- Deleting and moving ---------------------------------------------

    #[test]
    fn any_version_can_be_deleted_the_transcription_too() {
        let cases = [
            (0, vec!["Clean.", "Clean, mine."]),
            (1, vec!["raw", "Clean, mine."]),
            (2, vec!["raw", "Clean."]),
        ];
        for (index, left) in cases {
            let versions = delete_version(&stack(), index).expect("deleted");
            assert_eq!(texts(&versions), left, "version {index}");
        }
    }

    #[test]
    fn the_last_version_is_never_deleted() {
        assert_eq!(
            delete_version(&[HistoryVersion::transcription("raw")], 0),
            Err(VersionError::LastVersion)
        );
        assert_eq!(
            delete_version(&stack(), 3),
            Err(VersionError::NoSuchVersion)
        );
    }

    #[test]
    fn a_version_moves_to_any_place() {
        let moved =
            |from, to| texts(&move_version(&stack(), from, to).unwrap().unwrap()).join(" | ");
        assert_eq!(moved(0, 2), "Clean. | Clean, mine. | raw");
        assert_eq!(moved(2, 0), "Clean, mine. | raw | Clean.");
        assert_eq!(moved(1, 2), "raw | Clean, mine. | Clean.");
        assert_eq!(moved(1, 0), "Clean. | raw | Clean, mine.");
    }

    #[test]
    fn a_moved_version_keeps_its_steps() {
        let mut versions = stack();
        versions[1].steps.push(step(User, 3));
        let moved = move_version(&versions, 1, 2).unwrap().unwrap();
        assert_eq!(moved[2], versions[1]);
        assert_eq!(moved[1], versions[2]);
    }

    #[test]
    fn moving_a_version_to_its_place_changes_nothing() {
        assert_eq!(move_version(&stack(), 1, 1), Ok(None));
        assert_eq!(
            move_version(&stack(), 3, 0),
            Err(VersionError::NoSuchVersion)
        );
        assert_eq!(
            move_version(&stack(), 0, 3),
            Err(VersionError::NoSuchVersion)
        );
    }

    // ---- Post-processing -----------------------------------------------------

    use PostProcessTarget::{InPlaceOfAiTop, OnTop};

    #[test]
    fn post_processing_starts_from_the_version_chosen_or_the_top() {
        assert_eq!(post_processing_input(&stack(), None), Ok(2));
        assert_eq!(post_processing_input(&stack(), Some(0)), Ok(0));
        assert_eq!(post_processing_input(&stack(), Some(1)), Ok(1));
        assert_eq!(
            post_processing_input(&stack(), Some(3)),
            Err(VersionError::NoSuchVersion)
        );
        assert_eq!(
            post_processing_input(&[], None),
            Err(VersionError::NoSuchVersion)
        );
    }

    #[test]
    fn post_processing_puts_its_result_on_top() {
        let versions = apply_post_processing(&stack(), OnTop, "Polished.", step(Ai, 9))
            .expect("a new version");
        assert_eq!(
            texts(&versions),
            vec!["raw", "Clean.", "Clean, mine.", "Polished."]
        );
        assert_eq!(versions[3].steps, vec![step(Ai, 9)]);
    }

    #[test]
    fn a_result_equal_to_the_top_changes_nothing() {
        assert_eq!(
            apply_post_processing(&stack(), OnTop, " Clean, mine. ", step(Ai, 9)),
            None
        );
        // Equal to a version below the top, it still goes on top.
        let applied = apply_post_processing(&stack(), OnTop, "Clean.", step(Ai, 9));
        assert_eq!(
            applied.as_deref().map(texts),
            Some(vec!["raw", "Clean.", "Clean, mine.", "Clean."])
        );
    }

    #[test]
    fn the_redo_for_another_application_replaces_the_dictations_post_processing() {
        let processed = [HistoryVersion::transcription("raw"), v("Clean.", Ai)];
        let versions = apply_post_processing(&processed, InPlaceOfAiTop, "Again.", step(Ai, 9))
            .expect("in place");
        assert_eq!(texts(&versions), vec!["raw", "Again."]);
        assert_eq!(versions[1].steps, vec![step(Ai, 9)]);
        // Without one, or over the user's text, it goes on top.
        let bare = [HistoryVersion::transcription("raw")];
        let edited = [HistoryVersion::transcription("raw"), v("mine", User)];
        for versions in [&bare[..], &edited[..]] {
            let applied = apply_post_processing(versions, InPlaceOfAiTop, "Again.", step(Ai, 9))
                .expect("on top");
            assert_eq!(applied.len(), versions.len() + 1);
            assert_eq!(applied.last().map(|v| v.text.as_str()), Some("Again."));
        }
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

    fn column(conn: &Connection, id: i64, name: &str) -> Option<String> {
        conn.query_row(
            &format!("SELECT {name} FROM transcription_history WHERE id = ?1"),
            params![id],
            |row| row.get(0),
        )
        .expect("read column")
    }

    fn stored_column(conn: &Connection, id: i64) -> Option<String> {
        column(conn, id, "versions")
    }

    /// The stack as the column stores it, read without the other columns.
    fn stored_stack(conn: &Connection, id: i64) -> Vec<HistoryVersion> {
        let json = stored_column(conn, id).expect("stored");
        StackColumns {
            transcription: "not read",
            versions: Some(&json),
            ..Default::default()
        }
        .versions()
    }

    fn set(conn: &Connection, id: i64, column: &str, value: &str) {
        conn.execute(
            &format!("UPDATE transcription_history SET {column} = ?1 WHERE id = ?2"),
            params![value, id],
        )
        .expect("set column");
    }

    // ---- Which speech model made a transcription ---------------------------

    #[test]
    fn the_derived_transcription_names_its_model() {
        let derived = StackColumns {
            transcription: "raw",
            transcription_model: Some("canary-1b-v2"),
            ..Default::default()
        }
        .versions();
        assert_eq!(derived[0].steps[0].model.as_deref(), Some("canary-1b-v2"));
    }

    #[test]
    fn a_stack_stored_before_models_reads_without_one() {
        let json = r#"{"stack":[{"text":"raw","steps":[{"by":"transcription","at":1000,"prompt_name":null}]}]}"#;
        // The column names the latest transcription's model, which need not
        // be the one that made a stored version.
        let stack = StackColumns {
            transcription: "not read",
            versions: Some(json),
            transcription_model: Some("turbo"),
            ..Default::default()
        }
        .versions();
        assert_eq!(texts(&stack), vec!["raw"]);
        assert_eq!(stack[0].steps[0].model, None);
    }

    #[test]
    fn transcribing_again_puts_its_model_on_the_new_version_and_keeps_the_old_one() {
        let conn = database();
        let id = insert(&conn, "raw", None);
        set(&conn, id, "transcription_model", "canary-1b-v2");
        let mut new = new_transcription("raw again", None);
        new.model = Some("turbo".into());

        let e = add_transcription_with_conn(&conn, id, new, 77).expect("transcribe again");

        assert_eq!(texts(&e.versions), vec!["raw", "raw again"]);
        let models: Vec<Option<&str>> = e
            .versions
            .iter()
            .map(|v| v.steps[0].model.as_deref())
            .collect();
        assert_eq!(models, vec![Some("canary-1b-v2"), Some("turbo")]);
        assert_eq!(e.transcription_model.as_deref(), Some("turbo"));
        let stored: Vec<Option<String>> = stored_stack(&conn, id)
            .into_iter()
            .map(|v| v.steps[0].model.clone())
            .collect();
        assert_eq!(
            stored,
            vec![Some("canary-1b-v2".to_string()), Some("turbo".to_string())]
        );
    }

    #[test]
    fn saving_an_edit_keeps_the_model_of_the_transcription() {
        let conn = database();
        let id = insert(&conn, "raw", None);
        set(&conn, id, "transcription_model", "canary-1b-v2");

        save_version_with_conn(&conn, id, 0, "raw, mine", SaveVersionMode::New, 88)
            .expect("save version");

        let stored = stored_stack(&conn, id);
        assert_eq!(texts(&stored), vec!["raw", "raw, mine"]);
        assert_eq!(stored[0].steps[0].model.as_deref(), Some("canary-1b-v2"));
        // The user's version is no speech model's.
        assert_eq!(stored[1].steps[0].model, None);
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
        assert_eq!(times(&e.versions)[0], vec![Some(1_000)]);
        assert_eq!(stored_column(&conn, 1), None);
    }

    #[test]
    fn a_stack_from_danywhere_0_2_is_read_and_stored_whole_on_its_next_change() {
        let conn = database();
        let id = insert(&conn, "raw", Some("Clean."));
        set(
            &conn,
            id,
            "versions",
            &above_json(&[v("Clean.", Ai), v("Clean, mine.", User)]),
        );
        assert_eq!(
            texts(&get(&conn, id).versions),
            vec!["raw", "Clean.", "Clean, mine."]
        );

        let e = move_version_with_conn(&conn, id, 0, 2).expect("move");

        assert_eq!(texts(&e.versions), vec!["Clean.", "Clean, mine.", "raw"]);
        assert_eq!(stored_stack(&conn, id), e.versions);
        assert_eq!(e.transcription_text, "raw");
    }

    #[test]
    fn a_saved_version_is_stored_with_the_whole_stack() {
        let conn = database();
        let id = insert(&conn, "raw", Some("Clean."));
        let e = save_version_with_conn(&conn, id, 1, "Clean, mine.", SaveVersionMode::New, 42)
            .expect("save");
        assert_eq!(texts(&e.versions), vec!["raw", "Clean.", "Clean, mine."]);
        assert_eq!(e.versions[2].steps, vec![step(User, 42)]);
        assert_eq!(get(&conn, id).versions, e.versions);
        assert_eq!(stored_stack(&conn, id), e.versions);
        // Handy's columns still hold texts the stack has.
        assert_eq!(e.post_processed_text.as_deref(), Some("Clean."));
        assert_eq!(e.post_process_prompt.as_deref(), Some("Fix it"));
        assert_eq!(e.transcription_text, "raw");
    }

    #[test]
    fn saving_an_edit_of_a_lower_version_keeps_every_version() {
        let conn = database();
        let id = insert(&conn, "raw", Some("Clean."));
        let e = save_version_with_conn(&conn, id, 0, "raw fixed", SaveVersionMode::New, 42)
            .expect("save");
        assert_eq!(texts(&e.versions), vec!["raw", "Clean.", "raw fixed"]);
        assert!(e.post_process_requested);
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
    fn storing_a_stack_clears_the_corrections_of_before() {
        let conn = database();
        let id = insert(&conn, "raw", Some("Clean."));
        set(&conn, id, "transcription_correction", "raw fixed");
        set(&conn, id, "post_processed_from_correction", "raw fixed");
        set(&conn, id, "post_processed_correction", "Clean, mine.");
        assert_eq!(
            texts(&get(&conn, id).versions),
            vec!["raw", "raw fixed", "Clean.", "Clean, mine."]
        );

        let e = delete_version_with_conn(&conn, id, 1).expect("delete");

        assert_eq!(texts(&e.versions), vec!["raw", "Clean.", "Clean, mine."]);
        for name in [
            "transcription_correction",
            "post_processed_from_correction",
            "post_processed_correction",
        ] {
            assert_eq!(column(&conn, id, name), None, "{name}");
        }
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
    fn a_refused_change_changes_nothing() {
        let conn = database();
        let id = insert(&conn, "raw", Some("Clean."));
        for (base, text, mode, code) in [
            (0, "x", SaveVersionMode::Replace, "not_top"),
            (1, " ", SaveVersionMode::New, "blank"),
            (4, "x", SaveVersionMode::New, "no_such_version"),
        ] {
            let error =
                save_version_with_conn(&conn, id, base, text, mode, 42).expect_err("refused");
            assert_eq!(error.to_string(), code);
        }
        let alone = insert(&conn, "raw", None);
        let error = delete_version_with_conn(&conn, alone, 0).expect_err("refused");
        assert_eq!(error.to_string(), "last_version");
        let error = move_version_with_conn(&conn, id, 0, 5).expect_err("refused");
        assert_eq!(error.to_string(), "no_such_version");
        assert_eq!(stored_column(&conn, id), None);
        assert_eq!(stored_column(&conn, alone), None);
    }

    #[test]
    fn saving_leaves_the_post_processing_mark_as_it_was() {
        let conn = database();
        // Dictated with post-processing that failed: marked, but no ✨.
        let id = insert(&conn, "raw", None);
        set(&conn, id, "post_process_requested", "1");
        let e = save_version_with_conn(&conn, id, 0, "raw fixed", SaveVersionMode::New, 42)
            .expect("save");
        assert!(e.post_process_requested);
        let plain = insert(&conn, "raw", None);
        let e = save_version_with_conn(&conn, plain, 0, "raw fixed", SaveVersionMode::New, 42)
            .expect("save");
        assert!(!e.post_process_requested);
    }

    // ---- Deleting really deletes ---------------------------------------------

    #[test]
    fn deleting_the_transcription_deletes_it_from_handys_column() {
        let conn = database();
        let id = insert(&conn, "raw words", Some("Clean."));

        let e = delete_version_with_conn(&conn, id, 0).expect("delete");

        assert_eq!(texts(&e.versions), vec!["Clean."]);
        assert_eq!(stored_stack(&conn, id), e.versions);
        // No transcription is left: the column holds the entry's result.
        assert_eq!(e.transcription_text, "Clean.");
        assert_eq!(
            column(&conn, id, "transcription_text").as_deref(),
            Some("Clean.")
        );
        assert_eq!(entry_text(&e), "Clean.");
    }

    #[test]
    fn deleting_a_post_processing_deletes_it_from_handys_columns() {
        let conn = database();
        let id = insert(&conn, "raw", Some("Clean."));

        let e = delete_version_with_conn(&conn, id, 1).expect("delete");

        assert_eq!(texts(&e.versions), vec!["raw"]);
        assert_eq!(e.post_processed_text, None);
        assert_eq!(e.post_process_prompt, None);
        // Nothing post-processed is left: transcribing again doesn't
        // post-process.
        assert!(!e.post_process_requested);
    }

    #[test]
    fn deleting_keeps_the_mark_while_a_post_processing_stays() {
        let conn = database();
        let id = insert(&conn, "raw", Some("Clean."));
        store_post_processing_with_conn(
            &conn,
            id,
            OnTop,
            PostProcessed {
                text: "Cleaner.".into(),
                ..Default::default()
            },
            5,
        )
        .expect("post-process");

        let e = delete_version_with_conn(&conn, id, 2).expect("delete");

        assert_eq!(texts(&e.versions), vec!["raw", "Clean."]);
        assert!(e.post_process_requested);
        // Handy's column held the deleted one.
        assert_eq!(e.post_processed_text, None);
        // Deleting a user's version leaves the mark alone.
        let marked = insert(&conn, "raw", None);
        set(&conn, marked, "post_process_requested", "1");
        save_version_with_conn(&conn, marked, 0, "mine", SaveVersionMode::New, 6).expect("save");
        let e = delete_version_with_conn(&conn, marked, 1).expect("delete");
        assert!(e.post_process_requested);
    }

    #[test]
    fn moving_the_transcription_keeps_it_the_transcript() {
        let conn = database();
        let id = insert(&conn, "raw", Some("Clean."));

        let e = move_version_with_conn(&conn, id, 0, 1).expect("move");

        assert_eq!(texts(&e.versions), vec!["Clean.", "raw"]);
        assert_eq!(entry_text(&e), "raw");
        assert_eq!(e.transcription_text, "raw");
        assert_eq!(e.post_processed_text.as_deref(), Some("Clean."));
        assert!(e.post_process_requested);
    }

    #[test]
    fn a_move_to_its_place_writes_nothing() {
        let conn = database();
        let id = insert(&conn, "raw", Some("Clean."));
        let e = move_version_with_conn(&conn, id, 1, 1).expect("move");
        assert_eq!(texts(&e.versions), vec!["raw", "Clean."]);
        assert_eq!(stored_column(&conn, id), None);
    }

    // ---- Post-processing and transcribing again --------------------------

    #[test]
    fn a_post_processing_from_history_becomes_a_version() {
        let conn = database();
        let id = insert(&conn, "raw words", None);
        assert!(!get(&conn, id).post_process_requested);

        let stored = store_post_processing_with_conn(
            &conn,
            id,
            OnTop,
            PostProcessed {
                text: "Clean words.".into(),
                prompt: Some("Fix the grammar".into()),
                prompt_name: Some("Grammar".into()),
            },
            77,
        )
        .expect("store");

        assert!(!stored.unchanged);
        let e = stored.entry;
        assert_eq!(e.transcription_text, "raw words");
        assert_eq!(texts(&e.versions), vec!["raw words", "Clean words."]);
        assert_eq!(
            e.versions[1].steps,
            vec![VersionStep {
                by: Ai,
                at: Some(77),
                prompt_name: Some("Grammar".into()),
                model: None,
            }]
        );
        assert_eq!(e.post_processed_text.as_deref(), Some("Clean words."));
        assert_eq!(e.post_process_prompt.as_deref(), Some("Fix the grammar"));
        assert!(e.post_process_requested);
        assert_eq!(get(&conn, id).versions, e.versions);
    }

    #[test]
    fn a_post_processing_equal_to_the_top_says_it_added_nothing() {
        let conn = database();
        let id = insert(&conn, "raw words", Some("Clean words."));
        let before = get(&conn, id).versions;

        let stored = store_post_processing_with_conn(
            &conn,
            id,
            OnTop,
            PostProcessed {
                text: " Clean words. ".into(),
                ..Default::default()
            },
            78,
        )
        .expect("store");

        assert!(stored.unchanged);
        assert_eq!(stored.entry.versions, before);
        assert_eq!(get(&conn, id).versions, before);
        // A result below the top still goes on top.
        let stored = store_post_processing_with_conn(
            &conn,
            id,
            OnTop,
            PostProcessed {
                text: "raw words".into(),
                ..Default::default()
            },
            79,
        )
        .expect("store");
        assert!(!stored.unchanged);
        assert_eq!(
            texts(&stored.entry.versions),
            vec!["raw words", "Clean words.", "raw words"]
        );
    }

    #[test]
    fn the_redo_for_another_application_is_stored_in_place_of_the_dictations() {
        let conn = database();
        let id = insert(&conn, "raw", Some("Clean."));
        let redone = store_post_processing_with_conn(
            &conn,
            id,
            InPlaceOfAiTop,
            PostProcessed {
                text: "Again.".into(),
                ..Default::default()
            },
            6,
        )
        .expect("redo")
        .entry;
        assert_eq!(texts(&redone.versions), vec!["raw", "Again."]);
        assert_eq!(redone.versions[1].steps, vec![step(Ai, 6)]);
        assert_eq!(redone.post_processed_text.as_deref(), Some("Again."));
        assert_eq!(get(&conn, id).versions, redone.versions);
    }

    fn new_transcription(text: &str, post_processed: Option<&str>) -> NewTranscription {
        NewTranscription {
            text: text.into(),
            language: Some(LanguageDecision {
                language: Some("de".into()),
                source: LanguageSource::Detected,
            }),
            post_processed: post_processed.map(|text| PostProcessed {
                text: text.into(),
                prompt: Some("Fix it again".into()),
                prompt_name: Some("Again".into()),
            }),
            post_process_requested: post_processed.is_some(),
            model: None,
        }
    }

    /// A time after the entries' (made at second 1).
    const LATER: i64 = 1_800_000_000_000;

    #[test]
    fn transcribing_again_puts_the_new_transcription_on_top() {
        let conn = database();
        let id = insert(&conn, "raw", Some("Clean."));
        save_version_with_conn(&conn, id, 1, "Clean, mine.", SaveVersionMode::New, 42)
            .expect("save");

        let e = add_transcription_with_conn(
            &conn,
            id,
            new_transcription("new words", Some("New words.")),
            LATER,
        )
        .expect("transcribe again");

        assert_eq!(
            texts(&e.versions),
            vec!["raw", "Clean.", "Clean, mine.", "new words", "New words."]
        );
        assert_eq!(
            e.versions[3].steps,
            vec![VersionStep::new(Transcription, Some(LATER))]
        );
        assert_eq!(e.versions[4].steps[0].prompt_name.as_deref(), Some("Again"));
        assert_eq!(e.transcription_text, "new words");
        assert_eq!(e.post_processed_text.as_deref(), Some("New words."));
        assert_eq!(e.post_process_prompt.as_deref(), Some("Fix it again"));
        assert_eq!(e.language.as_deref(), Some("de"));
        assert_eq!(e.language_source, Some(LanguageSource::Detected));
        assert!(e.post_process_requested);
        assert_eq!(get(&conn, id).versions, e.versions);
    }

    #[test]
    fn transcribing_again_without_post_processing_adds_only_the_transcription() {
        let conn = database();
        let id = insert(&conn, "raw", Some("Clean."));

        let e = add_transcription_with_conn(&conn, id, new_transcription("new", None), LATER)
            .expect("transcribe again");

        assert_eq!(texts(&e.versions), vec!["raw", "Clean.", "new"]);
        assert_eq!(e.transcription_text, "new");
        // The old post-processing is still there.
        assert_eq!(e.post_processed_text.as_deref(), Some("Clean."));
        assert!(!e.post_process_requested);
    }

    #[test]
    fn a_first_transcription_takes_the_place_of_the_empty_one() {
        let conn = database();
        let id = insert(&conn, "", None);
        let e = add_transcription_with_conn(&conn, id, new_transcription("words", None), 77)
            .expect("transcribe");
        assert_eq!(texts(&e.versions), vec!["words"]);
        assert_eq!(e.transcription_text, "words");
    }

    #[test]
    fn the_same_transcription_again_adds_nothing() {
        let conn = database();
        let id = insert(&conn, "raw", None);
        let e = add_transcription_with_conn(&conn, id, new_transcription("raw", None), 77)
            .expect("transcribe again");
        assert_eq!(texts(&e.versions), vec!["raw"]);
        assert_eq!(e.language.as_deref(), Some("de"));
    }

    #[test]
    fn the_redo_for_another_application_starts_the_stack_anew() {
        let conn = database();
        let id = insert(&conn, "raw", Some("Clean."));
        move_version_with_conn(&conn, id, 0, 1).expect("move");

        HistoryManager::update_transcription_with_conn(
            &conn,
            id,
            "new words".into(),
            Some("New words.".into()),
            None,
            crate::managers::history::TranscribedWith::default(),
            true,
        )
        .expect("transcribe again");

        let e = get(&conn, id);
        assert_eq!(texts(&e.versions), vec!["new words", "New words."]);
        assert_eq!(stored_column(&conn, id), None);
    }

    #[test]
    fn a_missing_entry_fails() {
        let conn = database();
        assert!(save_version_with_conn(&conn, 42, 0, "x", SaveVersionMode::New, 1).is_err());
        assert!(delete_version_with_conn(&conn, 42, 0).is_err());
        assert!(move_version_with_conn(&conn, 42, 0, 1).is_err());
        assert!(
            store_post_processing_with_conn(&conn, 42, OnTop, PostProcessed::default(), 1).is_err()
        );
        assert!(add_transcription_with_conn(&conn, 42, new_transcription("x", None), 1).is_err());
    }
}
