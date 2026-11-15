//! Searching History (DanyWhere): a full-text search over an entry's texts
//! and a filter by the window it was dictated in, over the whole history
//! rather than the page History has loaded.
//!
//! The text is searched in every version of the entry (`history_versions`),
//! from the transcription up to the one History shows; the window in the
//! app's name, the window's title and the line History shows for them ("App
//! – Title"), and in the name of the file a transcribed file's entry shows
//! instead. Both are substrings by default, ignoring case. The text can also
//! match case and whole words only, and both can be regular expressions (the
//! `regex` crate's syntax, which runs in linear time whatever is typed). The
//! two fields combine with each other and with History's "same window"
//! filter.

use std::borrow::Cow;
use std::sync::Arc;

use anyhow::Result;
use regex::{Regex, RegexBuilder};
use rusqlite::{types::ValueRef, Connection, Row};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::managers::history::{HistoryEntry, HistoryManager, PaginatedHistory};

/// What History is searched for. An empty field asks for nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct HistorySearchQuery {
    /// Searched in the entry's texts.
    pub text: String,
    /// Searched in the window it was dictated in, or in the name of the
    /// file it was transcribed from; an entry with neither never matches it.
    pub window: String,
    /// Both fields are regular expressions.
    pub regex: bool,
    /// The text matches case; the window always ignores it.
    pub case_sensitive: bool,
    /// The text matches whole words only.
    pub whole_word: bool,
    /// Only entries dictated in exactly this window (History's "same
    /// window" filter).
    pub same_window: Option<SameWindow>,
}

/// One window, as History's "same window" filter names it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SameWindow {
    pub app_id: String,
    pub title: String,
}

/// A field whose regular expression doesn't compile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidField {
    Text,
    Window,
}

impl InvalidField {
    /// The error the commands return, for History to show under the field.
    pub fn code(self) -> &'static str {
        match self {
            InvalidField::Text => "invalid_text_pattern",
            InvalidField::Window => "invalid_window_pattern",
        }
    }
}

/// The parts of an entry a search looks at.
#[derive(Clone, Debug, Default)]
pub struct Searched<'a> {
    /// The texts of the entry's versions, the transcription first.
    pub texts: Vec<Cow<'a, str>>,
    pub app_id: Option<&'a str>,
    pub app_name: Option<&'a str>,
    pub title: Option<&'a str>,
    /// The name of the file a transcribed file's entry was made from (its
    /// `source_name`, which History shows in place of a window).
    pub file: Option<&'a str>,
}

impl<'a> Searched<'a> {
    pub fn of_entry(entry: &'a HistoryEntry) -> Self {
        let window = &entry.window_context;
        Self {
            texts: entry
                .versions
                .iter()
                .map(|version| Cow::Borrowed(version.text.as_str()))
                .collect(),
            app_id: window.window_app_id.as_deref(),
            app_name: window.window_app_name.as_deref(),
            title: window.window_title.as_deref(),
            file: entry.source_name.as_deref(),
        }
    }
}

/// A query, compiled.
#[derive(Debug)]
pub struct Search {
    text: Option<Regex>,
    whole_word: bool,
    window: Option<Regex>,
    same_window: Option<SameWindow>,
}

/// `needle` as a regex: as typed with `regex`, else literally.
fn compile(needle: &str, regex: bool, ignore_case: bool) -> Result<Regex, regex::Error> {
    let pattern = if regex {
        needle.to_string()
    } else {
        regex::escape(needle)
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(ignore_case)
        .build()
}

/// A letter, digit or underscore: what a whole word can't be cut inside of
/// (the frontend's highlighting uses the same rule).
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The match `start..end` of `text` cuts no word in two: it doesn't begin
/// right after a word character with one, nor end right before one with
/// one. A match may begin or end with punctuation ("#tag", "etc.").
fn cuts_no_word(text: &str, start: usize, end: usize) -> bool {
    let joined = |a: Option<char>, b: Option<char>| {
        a.is_some_and(is_word_char) && b.is_some_and(is_word_char)
    };
    let before = text[..start].chars().next_back();
    let first = text[start..end].chars().next();
    let last = text[start..end].chars().next_back();
    let after = text[end..].chars().next();
    !joined(before, first) && !joined(last, after)
}

/// Where to look next after the match at `start` cut a word: past the word
/// it starts in, since a match starting anywhere inside it cuts it too.
fn after_cut(text: &str, start: usize) -> usize {
    let rest = &text[start..];
    match rest.chars().next() {
        None => text.len() + 1,
        Some(c) if is_word_char(c) => rest
            .char_indices()
            .find(|(_, c)| !is_word_char(*c))
            .map_or(text.len(), |(i, _)| start + i),
        Some(c) => start + c.len_utf8(),
    }
}

/// Whether `regex` matches a whole word of `text` (see [`cuts_no_word`]).
/// Like an editor's search, it takes the leftmost match at each place and
/// moves on when that one cuts a word.
fn matches_whole_word(regex: &Regex, text: &str) -> bool {
    let mut at = 0;
    while at <= text.len() {
        let Some(found) = regex.find_at(text, at) else {
            return false;
        };
        if !found.is_empty() && cuts_no_word(text, found.start(), found.end()) {
            return true;
        }
        at = after_cut(text, found.start());
    }
    false
}

/// The line History shows for a window ("App – Title"), from the app's name
/// or else its id, as the frontend's `windowLabel` builds it.
fn window_parts<'a>(entry: &Searched<'a>) -> (Option<&'a str>, Option<&'a str>) {
    let app = entry
        .app_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .or(entry.app_id.filter(|id| !id.is_empty()));
    let title = entry.title.map(str::trim).filter(|title| !title.is_empty());
    (app, title)
}

impl Search {
    /// Compiles `query`; fails with the field whose regular expression
    /// doesn't compile.
    pub fn new(query: &HistorySearchQuery) -> Result<Self, InvalidField> {
        let text = if query.text.is_empty() {
            None
        } else {
            Some(
                compile(&query.text, query.regex, !query.case_sensitive)
                    .map_err(|_| InvalidField::Text)?,
            )
        };
        let window = if query.window.is_empty() {
            None
        } else {
            Some(compile(&query.window, query.regex, true).map_err(|_| InvalidField::Window)?)
        };
        Ok(Self {
            text,
            whole_word: query.whole_word,
            window,
            same_window: query.same_window.clone(),
        })
    }

    /// Whether `entry` matches every field of the query.
    pub fn matches(&self, entry: &Searched<'_>) -> bool {
        if let Some(same) = &self.same_window {
            if entry.app_id != Some(same.app_id.as_str())
                || entry.title != Some(same.title.as_str())
            {
                return false;
            }
        }
        if let Some(window) = &self.window {
            if !Self::window_matches(window, entry) {
                return false;
            }
        }
        match &self.text {
            Some(text) => entry.texts.iter().any(|t| self.text_matches(text, t)),
            None => true,
        }
    }

    pub fn matches_entry(&self, entry: &HistoryEntry) -> bool {
        self.matches(&Searched::of_entry(entry))
    }

    fn text_matches(&self, regex: &Regex, text: &str) -> bool {
        if self.whole_word {
            matches_whole_word(regex, text)
        } else {
            regex.is_match(text)
        }
    }

    /// The app, the title, the line History shows for both, or the name of
    /// the file a transcribed file's entry shows instead.
    fn window_matches(regex: &Regex, entry: &Searched<'_>) -> bool {
        let (app, title) = window_parts(entry);
        if app.is_some_and(|app| regex.is_match(app))
            || title.is_some_and(|title| regex.is_match(title))
            || entry.file.is_some_and(|file| regex.is_match(file))
        {
            return true;
        }
        match (app, title) {
            (Some(app), Some(title)) => regex.is_match(&format!("{app} – {title}")),
            _ => false,
        }
    }
}

/// The text of `column`, without copying it; `None` for NULL or a text that
/// isn't UTF-8.
pub(crate) fn text_ref<'a>(row: &'a Row<'_>, column: &str) -> rusqlite::Result<Option<&'a str>> {
    Ok(match row.get_ref(column)? {
        ValueRef::Text(bytes) => std::str::from_utf8(bytes).ok(),
        _ => None,
    })
}

/// What a search looks at in a row read with the entry's columns.
fn searched_row<'a>(row: &'a Row<'_>) -> rusqlite::Result<Searched<'a>> {
    Ok(Searched {
        // The same versions the entry read from this row has.
        texts: crate::history_versions::StackColumns::from_row(row)?.texts(),
        app_id: text_ref(row, "window_app_id")?,
        app_name: text_ref(row, "window_app_name")?,
        title: text_ref(row, "window_title")?,
        file: text_ref(row, "source_name")?,
    })
}

/// One page of the entries `search` matches, newest first: at most `limit`
/// with an id below `cursor` (`None`: from the newest), and whether more
/// follow. Reads the rows one by one and stops at the first match past the
/// page, so a page costs as many rows as it takes to fill it.
pub(crate) fn search_entries(
    conn: &Connection,
    search: &Search,
    cursor: Option<i64>,
    limit: usize,
) -> Result<PaginatedHistory> {
    let mut conditions: Vec<&str> = Vec::new();
    let mut values: Vec<rusqlite::types::Value> = Vec::new();
    if let Some(cursor) = cursor {
        conditions.push("id < ?");
        values.push(cursor.into());
    }
    // The "same window" filter is exact, so SQLite applies it.
    if let Some(same) = &search.same_window {
        conditions.push("window_app_id = ? AND window_title = ?");
        values.push(same.app_id.clone().into());
        values.push(same.title.clone().into());
    }
    let filter = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };
    let mut statement = conn.prepare(&format!(
        "SELECT {} FROM transcription_history {filter} ORDER BY id DESC",
        HistoryManager::entry_columns()
    ))?;
    let mut rows = statement.query(rusqlite::params_from_iter(values))?;
    let mut entries = Vec::new();
    let mut has_more = false;
    while let Some(row) = rows.next()? {
        let matched = search.matches(&searched_row(row)?);
        if !matched {
            continue;
        }
        if entries.len() >= limit {
            has_more = true;
            break;
        }
        entries.push(HistoryManager::map_entry_row(row)?);
    }
    Ok(PaginatedHistory { entries, has_more })
}

/// Whether the entry `id` matches `search`, for an entry that changed while
/// History shows a search.
pub(crate) fn entry_matches(conn: &Connection, id: i64, search: &Search) -> Result<bool> {
    let entry = HistoryManager::entry_with_conn(conn, id)?;
    Ok(search.matches_entry(&entry))
}

/// The most entries one page of results holds.
const MAX_PAGE: usize = 100;

/// One page of History's entries that match `query`, newest first, older
/// than the entry `cursor` (`None`: from the newest). Errors:
/// `invalid_text_pattern` / `invalid_window_pattern` when that field's
/// regular expression doesn't compile.
#[tauri::command]
#[specta::specta]
pub async fn search_history_entries(
    history: State<'_, Arc<HistoryManager>>,
    query: HistorySearchQuery,
    cursor: Option<i64>,
    limit: usize,
) -> Result<PaginatedHistory, String> {
    let search = Search::new(&query).map_err(|field| field.code().to_string())?;
    let history = Arc::clone(&history);
    tauri::async_runtime::spawn_blocking(move || {
        let conn = history.get_connection()?;
        search_entries(&conn, &search, cursor, limit.clamp(1, MAX_PAGE))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// Whether the entry `id` matches `query`: a new or changed entry joins a
/// search History shows only if it does. Errors as for
/// [`search_history_entries`].
#[tauri::command]
#[specta::specta]
pub async fn history_entry_matches_search(
    history: State<'_, Arc<HistoryManager>>,
    id: i64,
    query: HistorySearchQuery,
) -> Result<bool, String> {
    let search = Search::new(&query).map_err(|field| field.code().to_string())?;
    let conn = history.get_connection().map_err(|e| e.to_string())?;
    entry_matches(&conn, id, &search).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn query(text: &str) -> HistorySearchQuery {
        HistorySearchQuery {
            text: text.to_string(),
            ..Default::default()
        }
    }

    fn window_query(window: &str) -> HistorySearchQuery {
        HistorySearchQuery {
            window: window.to_string(),
            ..Default::default()
        }
    }

    fn texts<'a>(texts: &[&'a str]) -> Vec<Cow<'a, str>> {
        texts.iter().map(|t| Cow::Borrowed(*t)).collect()
    }

    fn text(text: &str) -> Searched<'_> {
        Searched {
            texts: texts(&[text]),
            ..Default::default()
        }
    }

    fn in_window<'a>(app_id: &'a str, app_name: &'a str, title: &'a str) -> Searched<'a> {
        Searched {
            texts: texts(&["words"]),
            app_id: Some(app_id),
            app_name: Some(app_name),
            title: Some(title),
            file: None,
        }
    }

    fn from_file(name: &str) -> Searched<'_> {
        Searched {
            texts: texts(&["words"]),
            file: Some(name),
            ..Default::default()
        }
    }

    fn matches(query: &HistorySearchQuery, entry: &Searched<'_>) -> bool {
        Search::new(query).expect("query compiles").matches(entry)
    }

    // ---- The text -------------------------------------------------------

    #[test]
    fn an_empty_query_matches_every_entry() {
        let everything = HistorySearchQuery::default();
        assert!(matches(&everything, &text("anything")));
        assert!(matches(&everything, &text("")));
        assert!(matches(&everything, &Searched::default()));
    }

    #[test]
    fn finds_the_text_in_every_version_of_an_entry() {
        let q = query("needle");
        for slot in 0..4 {
            let mut versions = vec!["hay", "more hay", "straw", "chaff"];
            versions[slot] = "a needle in it";
            let entry = Searched {
                texts: texts(&versions),
                ..Default::default()
            };
            assert!(matches(&q, &entry), "version {slot}");
        }
        let none = Searched {
            texts: texts(&["hay", "more hay", "straw"]),
            ..Default::default()
        };
        assert!(!matches(&q, &none));
    }

    #[test]
    fn the_text_ignores_case_by_default_also_beyond_ascii() {
        assert!(matches(&query("hello"), &text("Say HeLLo, world")));
        assert!(matches(&query("über"), &text("ÜBER alles")));
        assert!(!matches(&query("uber"), &text("über alles")));
    }

    #[test]
    fn matching_case_finds_only_the_same_case() {
        let q = HistorySearchQuery {
            case_sensitive: true,
            ..query("Build")
        };
        assert!(matches(&q, &text("the Build hangs")));
        assert!(!matches(&q, &text("the build hangs")));
    }

    #[test]
    fn a_whole_word_is_not_found_inside_a_word() {
        let q = HistorySearchQuery {
            whole_word: true,
            ..query("art")
        };
        assert!(!matches(&q, &text("Start the party")));
        assert!(!matches(&q, &text("Startart")));
        assert!(matches(&q, &text("Start art.")));
        assert!(matches(&q, &text("art")));
        let fuer = HistorySearchQuery {
            whole_word: true,
            ..query("für")
        };
        assert!(matches(&fuer, &text("das ist für mich")));
        assert!(!matches(&fuer, &text("fürs Protokoll")));
    }

    #[test]
    fn a_whole_word_may_begin_or_end_with_punctuation() {
        let whole = |needle: &str| HistorySearchQuery {
            whole_word: true,
            ..query(needle)
        };
        assert!(matches(&whole("foo."), &text("see foo.bar")));
        assert!(matches(&whole("#tag"), &text("x #tag y")));
        assert!(matches(&whole("tag"), &text("x #tag y")));
        assert!(!matches(&whole("fo"), &text("foo.")));
    }

    #[test]
    fn without_regex_the_text_is_literal() {
        assert!(!matches(&query("a.c"), &text("abc")));
        assert!(matches(&query("a.c"), &text("x a.c y")));
        assert!(matches(&query("(1)"), &text("point (1)")));
        assert!(Search::new(&query("(")).is_ok());
    }

    #[test]
    fn a_regex_matches_a_pattern() {
        let digits = HistorySearchQuery {
            regex: true,
            ..query(r"\d{3}")
        };
        assert!(matches(&digits, &text("Zimmer 123")));
        assert!(!matches(&digits, &text("Zimmer 12")));
        let whole = HistorySearchQuery {
            regex: true,
            whole_word: true,
            ..query(r"\d+")
        };
        assert!(!matches(&whole, &text("abc123")));
        assert!(matches(&whole, &text("abc 123")));
        let exact_case = HistorySearchQuery {
            regex: true,
            case_sensitive: true,
            ..query("^Hallo")
        };
        assert!(matches(&exact_case, &text("Hallo du")));
        assert!(!matches(&exact_case, &text("hallo du")));
        assert!(!matches(&exact_case, &text("du Hallo")));
    }

    #[test]
    fn an_invalid_regex_is_an_error_for_its_field() {
        let text = HistorySearchQuery {
            regex: true,
            ..query("(unclosed")
        };
        assert_eq!(Search::new(&text).err(), Some(InvalidField::Text));
        let window = HistorySearchQuery {
            regex: true,
            ..window_query("[a-")
        };
        assert_eq!(Search::new(&window).err(), Some(InvalidField::Window));
        assert_eq!(InvalidField::Text.code(), "invalid_text_pattern");
        assert_eq!(InvalidField::Window.code(), "invalid_window_pattern");
    }

    // ---- The window -----------------------------------------------------

    #[test]
    fn the_window_matches_the_app_the_title_or_the_line_history_shows() {
        let safari = in_window("com.apple.Safari", "Safari", "GitHub – DanyWhere");
        assert!(matches(&window_query("safari"), &safari));
        assert!(matches(&window_query("github"), &safari));
        assert!(matches(&window_query("Safari – GitHub"), &safari));
        assert!(!matches(&window_query("chrome"), &safari));
        assert!(!matches(
            &window_query("safari"),
            &text("dictated in Safari")
        ));
    }

    #[test]
    fn the_window_falls_back_to_the_app_id_as_history_does() {
        let unnamed = in_window("com.apple.mail", "", "Inbox");
        assert!(matches(&window_query("apple.mail"), &unnamed));
        let named = in_window("com.apple.mail", "Mail", "Inbox");
        assert!(!matches(&window_query("apple.mail"), &named));
    }

    #[test]
    fn a_window_regex_can_anchor_the_app_or_the_title() {
        let regex = |pattern: &str| HistorySearchQuery {
            regex: true,
            ..window_query(pattern)
        };
        assert!(matches(&regex("^mail$"), &in_window("m", "Mail", "Inbox")));
        assert!(!matches(
            &regex("^mail$"),
            &in_window("m", "Mailbox", "Inbox")
        ));
        assert!(matches(
            &regex("^Inbox"),
            &in_window("m", "Mail", "Inbox (3)")
        ));
        assert!(matches(
            &regex("^Mail – In"),
            &in_window("m", "Mail", "Inbox")
        ));
    }

    #[test]
    fn the_window_ignores_case_and_whole_word() {
        let q = HistorySearchQuery {
            case_sensitive: true,
            whole_word: true,
            ..window_query("SAF")
        };
        assert!(matches(&q, &in_window("s", "Safari", "Start")));
    }

    #[test]
    fn the_window_matches_a_transcribed_files_name_ignoring_case() {
        let file = from_file("Interview Müller 2026-11-15.m4a");
        assert!(matches(&window_query("interview"), &file));
        assert!(matches(&window_query("MÜLLER"), &file));
        assert!(matches(&window_query(".m4a"), &file));
        assert!(!matches(&window_query("zoom"), &file));
        // Literal without the regex toggle.
        assert!(!matches(&window_query("2026.11"), &file));
        // A dictation has no file name to match.
        assert!(!matches(
            &window_query("m4a"),
            &in_window("m", "Mail", "Inbox")
        ));
    }

    #[test]
    fn a_window_regex_matches_a_transcribed_files_name() {
        let regex = |pattern: &str| HistorySearchQuery {
            regex: true,
            ..window_query(pattern)
        };
        let file = from_file("Interview Müller 2026-11-15.m4a");
        assert!(matches(&regex(r"\.m4a$"), &file));
        assert!(matches(&regex(r"^interview\s"), &file));
        assert!(matches(&regex(r"\d{4}-\d{2}"), &file));
        assert!(!matches(&regex(r"^Müller"), &file));
        assert!(!matches(&regex(r"\.wav$"), &file));
    }

    #[test]
    fn a_files_name_combines_with_the_text() {
        let file = Searched {
            texts: texts(&["the budget for next year"]),
            file: Some("Meeting.m4a"),
            ..Default::default()
        };
        let both = HistorySearchQuery {
            window: "meeting".to_string(),
            ..query("budget")
        };
        assert!(matches(&both, &file));
        let wrong_text = HistorySearchQuery {
            window: "meeting".to_string(),
            ..query("holiday")
        };
        assert!(!matches(&wrong_text, &file));
    }

    #[test]
    fn text_window_and_same_window_combine() {
        let entry = Searched {
            texts: texts(&["the build hangs"]),
            app_id: Some("com.apple.Safari"),
            app_name: Some("Safari"),
            title: Some("CI"),
            file: None,
        };
        let both = HistorySearchQuery {
            window: "safari".to_string(),
            ..query("build")
        };
        assert!(matches(&both, &entry));
        let wrong_window = HistorySearchQuery {
            window: "mail".to_string(),
            ..query("build")
        };
        assert!(!matches(&wrong_window, &entry));
        let same = |app_id: &str, title: &str| HistorySearchQuery {
            same_window: Some(SameWindow {
                app_id: app_id.to_string(),
                title: title.to_string(),
            }),
            ..query("build")
        };
        assert!(matches(&same("com.apple.Safari", "CI"), &entry));
        assert!(!matches(&same("com.apple.Safari", "Other"), &entry));
    }

    // ---- The database ---------------------------------------------------

    fn database() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        HistoryManager::apply_migrations(&mut conn).expect("migrate");
        conn
    }

    fn insert(conn: &Connection, text: &str, window: Option<(&str, &str, &str)>) -> i64 {
        conn.execute(
            "INSERT INTO transcription_history (
                file_name, timestamp, saved, title, transcription_text,
                window_app_id, window_app_name, window_title
            ) VALUES ('a.opus', 1, 0, 't', ?1, ?2, ?3, ?4)",
            params![
                text,
                window.map(|w| w.0),
                window.map(|w| w.1),
                window.map(|w| w.2)
            ],
        )
        .expect("insert entry");
        conn.last_insert_rowid()
    }

    fn compiled(query: &HistorySearchQuery) -> Search {
        Search::new(query).expect("query compiles")
    }

    /// A transcribed file's entry, as `save_file_entry` makes it: no window,
    /// the file's name in `source_name`.
    fn insert_file(conn: &Connection, text: &str, source_name: &str) -> i64 {
        conn.execute(
            "INSERT INTO transcription_history (
                file_name, timestamp, saved, title, transcription_text, source_name
            ) VALUES ('handy-file-1.opus', 1, 0, 't', ?1, ?2)",
            params![text, source_name],
        )
        .expect("insert file entry");
        conn.last_insert_rowid()
    }

    #[test]
    fn a_search_finds_transcribed_files_by_their_name() {
        let conn = database();
        let interview = insert_file(&conn, "questions and answers", "Interview.m4a");
        insert_file(&conn, "a lecture", "Lecture.mp3");
        let dictation = insert(
            &conn,
            "dictated about the interview",
            Some(("com.apple.Notes", "Notes", "Interview prep")),
        );
        let by_name = compiled(&window_query("interview"));
        let page = search_entries(&conn, &by_name, None, 30).expect("search");
        let ids: Vec<i64> = page.entries.iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![dictation, interview]);

        let by_regex = compiled(&HistorySearchQuery {
            regex: true,
            ..window_query(r"\.m4a$")
        });
        let page = search_entries(&conn, &by_regex, None, 30).expect("search");
        let ids: Vec<i64> = page.entries.iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![interview]);
        assert!(entry_matches(&conn, interview, &by_regex).expect("check"));
        assert!(!entry_matches(&conn, dictation, &by_regex).expect("check"));
    }

    #[test]
    fn a_search_pages_through_the_matches_of_the_whole_history() {
        let conn = database();
        let mut expected = Vec::new();
        for i in 0..250 {
            let id = if i % 7 == 0 {
                insert(&conn, &format!("entry {i} with the Needle"), None)
            } else {
                insert(&conn, &format!("entry {i}"), None)
            };
            if i % 7 == 0 {
                expected.push(id);
            }
        }
        expected.reverse();
        let search = compiled(&query("needle"));

        let mut found = Vec::new();
        let mut cursor = None;
        let mut pages = 0;
        loop {
            let page = search_entries(&conn, &search, cursor, 10).expect("search");
            pages += 1;
            assert!(page.entries.len() <= 10);
            found.extend(page.entries.iter().map(|e| e.id));
            if !page.has_more {
                break;
            }
            assert_eq!(page.entries.len(), 10, "a page with more after it is full");
            cursor = page.entries.last().map(|e| e.id);
        }
        assert_eq!(found, expected);
        assert_eq!(pages, 4);
    }

    #[test]
    fn a_search_without_matches_finds_nothing_more() {
        let conn = database();
        for i in 0..50 {
            insert(&conn, &format!("entry {i}"), None);
        }
        let page = search_entries(&conn, &compiled(&query("missing")), None, 30).expect("search");
        assert!(page.entries.is_empty());
        assert!(!page.has_more);
    }

    #[test]
    fn a_search_stays_in_the_same_window() {
        let conn = database();
        let safari = Some(("com.apple.Safari", "Safari", "CI"));
        let other = Some(("com.apple.Safari", "Safari", "Mail"));
        let a = insert(&conn, "build one", safari);
        insert(&conn, "build two", other);
        insert(&conn, "nothing", safari);
        let b = insert(&conn, "build three", safari);
        let q = HistorySearchQuery {
            same_window: Some(SameWindow {
                app_id: "com.apple.Safari".to_string(),
                title: "CI".to_string(),
            }),
            ..query("build")
        };
        let page = search_entries(&conn, &compiled(&q), None, 30).expect("search");
        let ids: Vec<i64> = page.entries.iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![b, a]);
    }

    fn set(conn: &Connection, id: i64, column: &str, value: &str) {
        conn.execute(
            &format!("UPDATE transcription_history SET {column} = ?1 WHERE id = ?2"),
            params![value, id],
        )
        .expect("set column");
    }

    /// The ids `text` finds, newest first, checked against `entry_matches`.
    fn found(conn: &Connection, text: &str, ids: &[i64]) -> Vec<i64> {
        let search = compiled(&query(text));
        let page = search_entries(conn, &search, None, 30).expect("search");
        let found: Vec<i64> = page.entries.iter().map(|e| e.id).collect();
        for id in ids {
            assert_eq!(
                entry_matches(conn, *id, &search).expect("check"),
                found.contains(id),
                "entry {id} for {text:?}"
            );
        }
        found
    }

    #[test]
    fn a_search_finds_every_version_of_a_stored_stack() {
        let conn = database();
        let id = insert(&conn, "raw words", None);
        let other = insert(&conn, "hay", None);
        set(
            &conn,
            id,
            "versions",
            r#"[{"text":"Clean words.","steps":[{"by":"ai","at":1,"prompt_name":null}]},{"text":"Clean words, mine.","steps":[{"by":"user","at":2,"prompt_name":null}]}]"#,
        );
        let ids = [id, other];
        assert_eq!(found(&conn, "raw", &ids), vec![id]);
        assert_eq!(found(&conn, "clean words.", &ids), vec![id]);
        assert_eq!(found(&conn, "mine", &ids), vec![id]);
        assert_eq!(found(&conn, "hay", &ids), vec![other]);
    }

    #[test]
    fn a_search_finds_the_versions_derived_from_the_columns_of_before() {
        let conn = database();
        let id = insert(&conn, "raw words", None);
        set(&conn, id, "post_processed_text", "Clean words.");
        set(&conn, id, "post_processed_correction", "Clean words, mine.");
        assert_eq!(found(&conn, "clean words.", &[id]), vec![id]);
        assert_eq!(found(&conn, "mine", &[id]), vec![id]);
    }

    #[test]
    fn a_correction_left_out_of_the_stack_is_not_found() {
        let conn = database();
        let id = insert(&conn, "raw words", None);
        set(&conn, id, "post_processed_text", "Clean words.");
        set(&conn, id, "transcription_correction", "corrected after it");
        assert_eq!(found(&conn, "corrected", &[id]), Vec::<i64>::new());
        assert_eq!(found(&conn, "clean", &[id]), vec![id]);
    }

    #[test]
    fn once_stored_the_corrections_of_before_are_not_found() {
        let conn = database();
        let id = insert(&conn, "raw words", None);
        set(&conn, id, "transcription_correction", "old correction");
        set(&conn, id, "versions", "[]");
        assert_eq!(found(&conn, "old correction", &[id]), Vec::<i64>::new());
        assert_eq!(found(&conn, "raw", &[id]), vec![id]);
    }

    #[test]
    fn a_changed_entry_is_checked_against_the_search() {
        let conn = database();
        let yes = insert(&conn, "the needle", Some(("m", "Mail", "Inbox")));
        let no = insert(&conn, "hay", Some(("m", "Mail", "Inbox")));
        let search = compiled(&query("needle"));
        assert!(entry_matches(&conn, yes, &search).expect("check"));
        assert!(!entry_matches(&conn, no, &search).expect("check"));
        assert!(entry_matches(&conn, 999, &search).is_err());
    }

    /// How long a search takes that reads every row: 20,000 entries of
    /// about 600 characters each, with a post-processed text. Run with
    /// `cargo test --lib history_search -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn timing_of_a_search_through_a_large_history() {
        let conn = database();
        let words =
            "und dann habe ich gesagt dass wir das morgen machen können wenn der Build durchläuft ";
        let long = words.repeat(7);
        conn.execute_batch("BEGIN").unwrap();
        for i in 0..20_000 {
            conn.execute(
                "INSERT INTO transcription_history (
                    file_name, timestamp, saved, title, transcription_text,
                    post_processed_text, window_app_id, window_app_name, window_title
                ) VALUES ('a.opus', 1, 0, 't', ?1, ?2, 'com.apple.Safari', 'Safari', ?3)",
                params![
                    format!("{i} {long}"),
                    format!("{i} {long}."),
                    format!("Tab {i}")
                ],
            )
            .unwrap();
        }
        conn.execute_batch("COMMIT").unwrap();
        for (name, q) in [
            ("substring, no match", query("zzzz")),
            (
                "whole word, no match",
                HistorySearchQuery {
                    whole_word: true,
                    ..query("Buil")
                },
            ),
            (
                "regex, no match",
                HistorySearchQuery {
                    regex: true,
                    ..query(r"\bBuild\s+\d{4}")
                },
            ),
            ("window, no match", window_query("chrome")),
            ("substring, page of 30", query("build")),
        ] {
            let search = compiled(&q);
            let start = std::time::Instant::now();
            let page = search_entries(&conn, &search, None, 30).unwrap();
            println!(
                "{name}: {} found in {:?}",
                page.entries.len(),
                start.elapsed()
            );
        }
    }
}
