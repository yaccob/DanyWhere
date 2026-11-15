//! DanyWhere: the "Append" field, what every dictation of a profile gets
//! appended (user, 2026-11-15: "dass man jede beliebige Zeichenkette anhängen
//! kann, und auch Steuerzeichen").
//!
//! It replaces Handy's "Append Trailing Space" and "Auto Submit". Their
//! fields (`append_trailing_space`, `auto_submit`, `auto_submit_key`) stay in
//! the settings for Handy merges and for the migration, but only this field
//! decides what happens.
//!
//! The stored string:
//! - plain text is appended as written;
//! - `\n` new line, `\t` tab, `\s` space (visible, unlike a typed trailing
//!   space), `\\` backslash, `\{` and `\}` braces, all as text;
//! - key presses in braces, `{Enter}`, `{Tab}`, `{Shift+Enter}`,
//!   `{Cmd+Enter}` (Super/Win+Enter off macOS, as Handy's auto submit),
//!   `{Ctrl+Enter}`, only at the end: once a key press appears, only key
//!   presses may follow.
//!
//! The text is appended wherever the trailing space was: to the pasted text,
//! to the clipboard copy after a paste, and to the reply of the "Insert Last
//! Dictation" service. The key presses follow the paste where auto submit
//! was sent; none with paste method "None" (as auto submit) and none without
//! Accessibility, where they would go nowhere.

use enigo::{Direction, Enigo, Key, Keyboard};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;

use crate::profiles::ProfileValues;
use crate::settings::{get_settings, write_settings, AppSettings, AutoSubmitKey, PasteMethod};

/// A key press the field can end with.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum AppendKey {
    Enter,
    Tab,
    ShiftEnter,
    /// Cmd+Enter on macOS, Super (Windows key)+Enter elsewhere, as Handy's
    /// `AutoSubmitKey::CmdEnter`.
    CmdEnter,
    CtrlEnter,
}

/// One part of the field, in order; what the preview names.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AppendPart {
    /// Text as written. Spaces at its ends are parts of their own.
    Text {
        text: String,
    },
    Space,
    NewLine,
    Tab,
    Key {
        key: AppendKey,
    },
}

/// Why a value is invalid, as a code the frontend translates.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum AppendTextErrorCode {
    /// A backslash before a character that has no meaning after one.
    UnknownEscape,
    /// A backslash at the very end.
    TrailingBackslash,
    /// Braces around something that is no known key press.
    UnknownKey,
    /// A `{` without its `}`.
    UnclosedKey,
    /// A `}` without its `{`.
    UnmatchedBrace,
    /// Text after a key press.
    TextAfterKey,
}

/// An invalid value: the code and what it is about, the escape (`\x`) for
/// `UnknownEscape`, the name between the braces for `UnknownKey`, else empty.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct AppendTextError {
    pub code: AppendTextErrorCode,
    pub detail: String,
}

impl AppendTextError {
    fn new(code: AppendTextErrorCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for AppendTextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} {:?}", self.code, self.detail)
    }
}

/// Reads the field into its parts, or says why it is invalid.
pub fn parse(value: &str) -> Result<Vec<AppendPart>, AppendTextError> {
    use AppendTextErrorCode::*;

    let mut parts = Vec::new();
    // Text as written, not yet a part.
    let mut run = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        let part = match c {
            '\\' => match chars.next() {
                None => return Err(AppendTextError::new(TrailingBackslash, "")),
                Some('s') => Some(AppendPart::Space),
                Some('n') => Some(AppendPart::NewLine),
                Some('t') => Some(AppendPart::Tab),
                Some(escaped @ ('\\' | '{' | '}')) => {
                    run.push(escaped);
                    None
                }
                Some(other) => {
                    return Err(AppendTextError::new(UnknownEscape, format!("\\{other}")))
                }
            },
            '{' => {
                let mut name = String::new();
                loop {
                    match chars.next() {
                        None => return Err(AppendTextError::new(UnclosedKey, "")),
                        Some('}') => break,
                        Some(c) => name.push(c),
                    }
                }
                let key = key_named(&name).ok_or_else(|| AppendTextError::new(UnknownKey, name))?;
                Some(AppendPart::Key { key })
            }
            '}' => return Err(AppendTextError::new(UnmatchedBrace, "")),
            c => {
                run.push(c);
                None
            }
        };
        if let Some(part) = part {
            push_text(&mut parts, &mut run);
            if ends_with_key(&parts) && !matches!(part, AppendPart::Key { .. }) {
                return Err(AppendTextError::new(TextAfterKey, ""));
            }
            parts.push(part);
        } else if ends_with_key(&parts) {
            return Err(AppendTextError::new(TextAfterKey, ""));
        }
    }
    push_text(&mut parts, &mut run);
    Ok(parts)
}

fn ends_with_key(parts: &[AppendPart]) -> bool {
    matches!(parts.last(), Some(AppendPart::Key { .. }))
}

/// Ends the text run: its text, with the spaces at its ends as parts of
/// their own, since a typed space can't be seen in the preview.
fn push_text(parts: &mut Vec<AppendPart>, run: &mut String) {
    let text = std::mem::take(run);
    let inner = text.trim_matches(' ');
    let leading = text.len() - text.trim_start_matches(' ').len();
    let trailing = if inner.is_empty() {
        0
    } else {
        text.len() - text.trim_end_matches(' ').len()
    };
    parts.extend(std::iter::repeat_n(AppendPart::Space, leading));
    if !inner.is_empty() {
        parts.push(AppendPart::Text {
            text: inner.to_string(),
        });
    }
    parts.extend(std::iter::repeat_n(AppendPart::Space, trailing));
}

/// The key press named between braces. Case and spaces don't count;
/// "Return" is Enter, "Command", "Super", "Win" and "Meta" are Cmd,
/// "Control" is Ctrl.
fn key_named(name: &str) -> Option<AppendKey> {
    let name: String = name
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase();
    let (modifier, base) = match name.split_once('+') {
        Some((modifier, base)) => (Some(modifier), base),
        None => (None, name.as_str()),
    };
    let enter = matches!(base, "enter" | "return");
    match modifier {
        None if enter => Some(AppendKey::Enter),
        None if base == "tab" => Some(AppendKey::Tab),
        Some("shift") if enter => Some(AppendKey::ShiftEnter),
        Some("cmd" | "command" | "super" | "win" | "meta") if enter => Some(AppendKey::CmdEnter),
        Some("ctrl" | "control") if enter => Some(AppendKey::CtrlEnter),
        _ => None,
    }
}

/// What a dictation gets: the text appended to it and the key presses after
/// it is pasted, in their order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Append {
    pub text: String,
    pub keys: Vec<AppendKey>,
}

impl Append {
    pub fn from_parts(parts: &[AppendPart]) -> Self {
        let mut append = Self::default();
        for part in parts {
            match part {
                AppendPart::Text { text } => append.text.push_str(text),
                AppendPart::Space => append.text.push(' '),
                AppendPart::NewLine => append.text.push('\n'),
                AppendPart::Tab => append.text.push('\t'),
                AppendPart::Key { key } => append.keys.push(*key),
            }
        }
        append
    }
}

/// The "Append" field of `settings`. A stored value that doesn't parse,
/// which only a hand-edited settings file can hold, appends nothing.
pub fn for_settings(settings: &AppSettings) -> Append {
    match parse(&settings.append_text) {
        Ok(parts) => Append::from_parts(&parts),
        Err(error) => {
            log::warn!("The stored \"Append\" value is invalid ({error}); appending nothing");
            Append::default()
        }
    }
}

/// `text` as it is pasted: with the field's text appended.
pub fn text_to_paste(text: String, settings: &AppSettings) -> String {
    text + &for_settings(settings).text
}

/// A finished dictation's `text` with the "Append" text of its profile,
/// whose `values` were taken while it ran (`None`: the active profile's),
/// for the service call waiting for it.
pub fn with_dictation_text(app: &AppHandle, text: &str, values: Option<&ProfileValues>) -> String {
    let settings = crate::app_profiles::with_values(&get_settings(app), values);
    text_to_paste(text.to_string(), &settings)
}

/// The key presses to send after a paste: none with paste method "None", as
/// Handy's auto submit, and none without Accessibility, where they can't be
/// sent (dropped silently, no notice per dictation).
pub fn keys_after_paste(
    append: &Append,
    paste_method: PasteMethod,
    accessibility_skipped: bool,
) -> Vec<AppendKey> {
    if paste_method == PasteMethod::None || accessibility_skipped {
        return Vec::new();
    }
    append.keys.clone()
}

/// Pause between two key presses, so the app in front handles each one.
const KEY_GAP: std::time::Duration = std::time::Duration::from_millis(30);

/// Sends `keys` in their order.
pub fn send_keys(enigo: &mut Enigo, keys: &[AppendKey]) -> Result<(), String> {
    for (index, key) in keys.iter().enumerate() {
        if index > 0 {
            std::thread::sleep(KEY_GAP);
        }
        match key {
            AppendKey::Enter => crate::clipboard::send_return_key(enigo, AutoSubmitKey::Enter)?,
            AppendKey::CtrlEnter => {
                crate::clipboard::send_return_key(enigo, AutoSubmitKey::CtrlEnter)?
            }
            AppendKey::CmdEnter => {
                crate::clipboard::send_return_key(enigo, AutoSubmitKey::CmdEnter)?
            }
            AppendKey::Tab => tap(enigo, Key::Tab)?,
            AppendKey::ShiftEnter => {
                enigo
                    .key(Key::Shift, Direction::Press)
                    .map_err(|e| format!("Failed to press Shift key: {e}"))?;
                let tapped = tap(enigo, Key::Return);
                enigo
                    .key(Key::Shift, Direction::Release)
                    .map_err(|e| format!("Failed to release Shift key: {e}"))?;
                tapped?
            }
        }
    }
    Ok(())
}

fn tap(enigo: &mut Enigo, key: Key) -> Result<(), String> {
    enigo
        .key(key, Direction::Press)
        .map_err(|e| format!("Failed to press {key:?}: {e}"))?;
    enigo
        .key(key, Direction::Release)
        .map_err(|e| format!("Failed to release {key:?}: {e}"))
}

// ---------------------------------------------------------------------------
// Migration from Handy's two settings
// ---------------------------------------------------------------------------

/// The field's value for Handy's old settings: `\s` for the trailing space,
/// then the auto submit key.
pub fn legacy_value(trailing_space: bool, auto_submit: bool, key: AutoSubmitKey) -> String {
    let mut value = String::new();
    if trailing_space {
        value.push_str(r"\s");
    }
    if auto_submit {
        value.push_str(match key {
            AutoSubmitKey::Enter => "{Enter}",
            AutoSubmitKey::CmdEnter => "{Cmd+Enter}",
            AutoSubmitKey::CtrlEnter => "{Ctrl+Enter}",
        });
    }
    value
}

/// The stored `append_trailing_space` of a stored object, false if missing.
fn stored_trailing_space(stored: Option<&serde_json::Value>) -> bool {
    stored
        .and_then(|values| values.get("append_trailing_space"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

/// Whether a stored object (settings or a profile's values) has the field.
fn has_field(stored: Option<&serde_json::Value>) -> bool {
    stored.is_some_and(|values| values.get("append_text").is_some())
}

/// One-time migration, run on every read of the store (`settings_value` is
/// the stored JSON): the settings and every stored profile without the field
/// get it from their `append_trailing_space` and the global auto submit.
/// Returns whether a value changed; a derived value that equals the default
/// (both old settings off) needs no write.
pub fn migrate(settings: &mut AppSettings, settings_value: &serde_json::Value) -> bool {
    let (auto_submit, key) = (settings.auto_submit, settings.auto_submit_key);
    let mut changed = false;
    if !has_field(Some(settings_value)) {
        let value = legacy_value(settings.append_trailing_space, auto_submit, key);
        if settings.append_text != value {
            log::info!("\"Append\" takes over the trailing space and auto submit: {value:?}");
            settings.append_text = value;
            changed = true;
        }
    }
    let stored_profiles = settings_value
        .get("profiles")
        .and_then(|profiles| profiles.as_array());
    for profile in settings.profiles.iter_mut() {
        let stored_values = stored_profiles
            .and_then(|profiles| {
                profiles.iter().find(|stored| {
                    stored.get("id").and_then(|id| id.as_str()) == Some(profile.id.as_str())
                })
            })
            .and_then(|stored| stored.get("values"));
        if has_field(stored_values) {
            continue;
        }
        let value = legacy_value(stored_trailing_space(stored_values), auto_submit, key);
        if profile.values.append_text != value {
            profile.values.append_text = value;
            changed = true;
        }
    }
    changed
}

/// For an import: a profile file from before the field gets it from its
/// `append_trailing_space`. `file_text` is the file as read.
pub fn fill_from_older_export(file_text: &str, values: &mut ProfileValues) {
    let Ok(file) = serde_saphyr::from_str::<serde_json::Value>(file_text) else {
        return;
    };
    let stored = file
        .get("profile")
        .and_then(|profile| profile.get("values"));
    if has_field(stored) {
        return;
    }
    // auto_submit was global, so the file holds no key press.
    values.append_text = legacy_value(stored_trailing_space(stored), false, AutoSubmitKey::Enter);
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// The field's parts for the preview, or why the value is invalid.
#[tauri::command]
#[specta::specta]
pub fn parse_append_text(value: String) -> Result<Vec<AppendPart>, AppendTextError> {
    parse(&value)
}

/// Stores the field in the active profile; an invalid value is refused and
/// the stored one stays.
#[tauri::command]
#[specta::specta]
pub fn change_append_text_setting(app: AppHandle, value: String) -> Result<(), AppendTextError> {
    parse(&value)?;
    let mut settings = get_settings(&app);
    settings.append_text = value;
    write_settings(&app, settings);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::get_default_settings;
    use AppendKey::*;
    use AppendTextErrorCode::*;

    fn text(text: &str) -> AppendPart {
        AppendPart::Text {
            text: text.to_string(),
        }
    }

    fn key(key: AppendKey) -> AppendPart {
        AppendPart::Key { key }
    }

    fn error(value: &str) -> (AppendTextErrorCode, String) {
        let error = parse(value).expect_err(value);
        (error.code, error.detail)
    }

    // ---- Valid values -------------------------------------------------------

    #[test]
    fn an_empty_field_appends_nothing() {
        assert_eq!(parse(""), Ok(vec![]));
        assert_eq!(Append::from_parts(&[]), Append::default());
    }

    #[test]
    fn plain_text_is_appended_as_written() {
        assert_eq!(parse("Gruß, J."), Ok(vec![text("Gruß, J.")]));
    }

    #[test]
    fn escapes_stand_for_space_new_line_and_tab() {
        assert_eq!(parse(r"\s"), Ok(vec![AppendPart::Space]));
        assert_eq!(parse(r"\n"), Ok(vec![AppendPart::NewLine]));
        assert_eq!(parse(r"\t"), Ok(vec![AppendPart::Tab]));
        assert_eq!(
            parse(r"\n\n"),
            Ok(vec![AppendPart::NewLine, AppendPart::NewLine])
        );
    }

    #[test]
    fn escaped_backslash_and_braces_are_text() {
        assert_eq!(parse(r"a\{b\}c\\"), Ok(vec![text(r"a{b}c\")]));
    }

    #[test]
    fn every_key_press_in_braces() {
        assert_eq!(parse("{Enter}"), Ok(vec![key(Enter)]));
        assert_eq!(parse("{Tab}"), Ok(vec![key(Tab)]));
        assert_eq!(parse("{Shift+Enter}"), Ok(vec![key(ShiftEnter)]));
        assert_eq!(parse("{Cmd+Enter}"), Ok(vec![key(CmdEnter)]));
        assert_eq!(parse("{Ctrl+Enter}"), Ok(vec![key(CtrlEnter)]));
    }

    #[test]
    fn key_names_ignore_case_and_spaces_and_know_other_names() {
        assert_eq!(parse("{enter}"), Ok(vec![key(Enter)]));
        assert_eq!(parse("{RETURN}"), Ok(vec![key(Enter)]));
        assert_eq!(parse("{ shift + enter }"), Ok(vec![key(ShiftEnter)]));
        assert_eq!(parse("{Control+Enter}"), Ok(vec![key(CtrlEnter)]));
        for name in ["Command", "Super", "Win", "Meta"] {
            assert_eq!(
                parse(&format!("{{{name}+Enter}}")),
                Ok(vec![key(CmdEnter)]),
                "{name}"
            );
        }
    }

    #[test]
    fn text_then_key_presses_in_their_order() {
        assert_eq!(parse(r"\s{Enter}"), Ok(vec![AppendPart::Space, key(Enter)]));
        assert_eq!(
            parse(r"OK\n{Tab}{Enter}"),
            Ok(vec![text("OK"), AppendPart::NewLine, key(Tab), key(Enter)])
        );
    }

    #[test]
    fn typed_spaces_at_the_ends_of_text_are_parts_of_their_own() {
        assert_eq!(
            parse("foo bar "),
            Ok(vec![text("foo bar"), AppendPart::Space])
        );
        assert_eq!(
            parse(" – "),
            Ok(vec![AppendPart::Space, text("–"), AppendPart::Space])
        );
        assert_eq!(parse("  "), Ok(vec![AppendPart::Space, AppendPart::Space]));
        assert_eq!(
            parse(r"a\sb"),
            Ok(vec![text("a"), AppendPart::Space, text("b")])
        );
    }

    // ---- Invalid values -----------------------------------------------------

    #[test]
    fn an_unknown_escape_is_invalid() {
        assert_eq!(error(r"\x"), (UnknownEscape, r"\x".to_string()));
        assert_eq!(error(r"ab\r"), (UnknownEscape, r"\r".to_string()));
    }

    #[test]
    fn a_backslash_at_the_end_is_invalid() {
        assert_eq!(error(r"abc\"), (TrailingBackslash, String::new()));
    }

    #[test]
    fn an_unknown_key_is_invalid() {
        assert_eq!(error("{Foo}"), (UnknownKey, "Foo".to_string()));
        assert_eq!(error("{}"), (UnknownKey, String::new()));
        assert_eq!(error("{Shift+Tab}"), (UnknownKey, "Shift+Tab".to_string()));
        assert_eq!(
            error("{Ctrl+Shift+Enter}"),
            (UnknownKey, "Ctrl+Shift+Enter".to_string())
        );
    }

    #[test]
    fn an_unclosed_key_is_invalid() {
        assert_eq!(error("{Enter"), (UnclosedKey, String::new()));
        assert_eq!(error(r"\s{"), (UnclosedKey, String::new()));
    }

    #[test]
    fn a_lone_closing_brace_is_invalid() {
        assert_eq!(error("a}"), (UnmatchedBrace, String::new()));
    }

    #[test]
    fn text_after_a_key_press_is_invalid() {
        assert_eq!(error("{Enter}x").0, TextAfterKey);
        assert_eq!(error(r"{Enter}\s").0, TextAfterKey);
        assert_eq!(error("{Enter} ").0, TextAfterKey);
        assert_eq!(error(r"{Tab}\n{Enter}").0, TextAfterKey);
    }

    // ---- What a dictation gets ----------------------------------------------

    #[test]
    fn text_parts_are_appended_and_keys_follow() {
        let parts = parse(r"\s\n\t\\{Tab}{Enter}").expect("valid");
        assert_eq!(
            Append::from_parts(&parts),
            Append {
                text: " \n\t\\".to_string(),
                keys: vec![Tab, Enter],
            }
        );
    }

    fn settings_with(append_text: &str) -> AppSettings {
        AppSettings {
            append_text: append_text.to_string(),
            paste_method: PasteMethod::CtrlV,
            ..get_default_settings()
        }
    }

    #[test]
    fn the_pasted_text_gets_the_fields_text() {
        let settings = settings_with(r"\s{Enter}");
        assert_eq!(text_to_paste("Hallo".into(), &settings), "Hallo ");
        let settings = settings_with(r"\nMfG");
        assert_eq!(text_to_paste("Hallo".into(), &settings), "Hallo\nMfG");
    }

    #[test]
    fn only_the_new_field_decides() {
        // The old switches no longer do anything.
        let mut settings = settings_with("");
        settings.append_trailing_space = true;
        settings.auto_submit = true;
        assert_eq!(text_to_paste("Hallo".into(), &settings), "Hallo");
        assert_eq!(for_settings(&settings), Append::default());
    }

    #[test]
    fn a_hand_edited_invalid_value_appends_nothing() {
        let settings = settings_with("{Foo}");
        assert_eq!(for_settings(&settings), Append::default());
        assert_eq!(text_to_paste("Hallo".into(), &settings), "Hallo");
    }

    #[test]
    fn key_presses_follow_every_paste_method_but_none() {
        let append = for_settings(&settings_with(r"\s{Shift+Enter}{Enter}"));
        for method in [
            PasteMethod::CtrlV,
            PasteMethod::Direct,
            PasteMethod::CtrlShiftV,
            PasteMethod::ShiftInsert,
            PasteMethod::ExternalScript,
        ] {
            assert_eq!(
                keys_after_paste(&append, method, false),
                vec![ShiftEnter, Enter],
                "{method:?}"
            );
        }
        assert!(keys_after_paste(&append, PasteMethod::None, false).is_empty());
    }

    #[test]
    fn key_presses_are_dropped_without_accessibility_but_the_text_stays() {
        let settings = AppSettings {
            accessibility_skipped: true,
            ..settings_with(r"\s{Enter}")
        };
        let append = for_settings(&settings);
        assert!(keys_after_paste(&append, PasteMethod::CtrlV, true).is_empty());
        assert_eq!(text_to_paste("Hallo".into(), &settings), "Hallo ");
    }

    // ---- Migration ----------------------------------------------------------

    #[test]
    fn the_old_settings_become_a_value() {
        use AutoSubmitKey as K;
        assert_eq!(legacy_value(false, false, K::Enter), "");
        assert_eq!(legacy_value(false, false, K::CmdEnter), "");
        assert_eq!(legacy_value(true, false, K::Enter), r"\s");
        assert_eq!(legacy_value(false, true, K::Enter), "{Enter}");
        assert_eq!(legacy_value(false, true, K::CmdEnter), "{Cmd+Enter}");
        assert_eq!(legacy_value(false, true, K::CtrlEnter), "{Ctrl+Enter}");
        assert_eq!(legacy_value(true, true, K::Enter), r"\s{Enter}");
        assert_eq!(legacy_value(true, true, K::CtrlEnter), r"\s{Ctrl+Enter}");
    }

    #[test]
    fn every_old_value_means_what_the_old_settings_did() {
        use AutoSubmitKey as K;
        for (trailing, auto, key, expected) in [
            (
                true,
                true,
                K::Enter,
                Append {
                    text: " ".into(),
                    keys: vec![Enter],
                },
            ),
            (
                false,
                true,
                K::CmdEnter,
                Append {
                    text: "".into(),
                    keys: vec![CmdEnter],
                },
            ),
            (
                true,
                false,
                K::CtrlEnter,
                Append {
                    text: " ".into(),
                    keys: vec![],
                },
            ),
            (
                false,
                true,
                K::CtrlEnter,
                Append {
                    text: "".into(),
                    keys: vec![CtrlEnter],
                },
            ),
        ] {
            let value = legacy_value(trailing, auto, key);
            let parts = parse(&value).expect(&value);
            assert_eq!(Append::from_parts(&parts), expected, "{value}");
        }
    }

    /// A stored store as written before the field: the old settings and two
    /// profiles, "default" with the trailing space and "work" without.
    fn stored_before_the_field(
        trailing_space: bool,
        auto_submit: bool,
        key: &str,
    ) -> serde_json::Value {
        let mut settings = get_default_settings();
        settings.append_trailing_space = trailing_space;
        settings.auto_submit = auto_submit;
        crate::profiles::ensure_profiles(&mut settings);
        let mut work = settings.profiles[0].clone();
        work.id = "work".to_string();
        work.name = "Work".to_string();
        settings.profiles.push(work);
        let mut stored = serde_json::to_value(&settings).expect("serializes");
        let object = stored.as_object_mut().expect("an object");
        object.remove("append_text");
        object.insert("auto_submit_key".into(), serde_json::json!(key));
        for (index, profile) in object["profiles"]
            .as_array_mut()
            .expect("profiles")
            .iter_mut()
            .enumerate()
        {
            let values = profile["values"].as_object_mut().expect("values");
            values.remove("append_text");
            values.insert(
                "append_trailing_space".into(),
                serde_json::json!(index == 0 && trailing_space),
            );
        }
        stored
    }

    fn read(stored: &serde_json::Value) -> AppSettings {
        serde_json::from_value(stored.clone()).expect("parses")
    }

    #[test]
    fn the_settings_and_each_profile_get_the_field_once() {
        let stored = stored_before_the_field(true, true, "cmd_enter");
        let mut settings = read(&stored);
        assert!(migrate(&mut settings, &stored));
        assert_eq!(settings.append_text, r"\s{Cmd+Enter}");
        assert_eq!(settings.profiles[0].values.append_text, r"\s{Cmd+Enter}");
        // auto_submit was global, so every profile gets its key.
        assert_eq!(settings.profiles[1].values.append_text, "{Cmd+Enter}");

        // Once stored, nothing changes any more.
        let stored = serde_json::to_value(&settings).expect("serializes");
        let mut again = read(&stored);
        assert!(!migrate(&mut again, &stored));
        assert_eq!(again.append_text, r"\s{Cmd+Enter}");
    }

    #[test]
    fn the_trailing_space_alone_becomes_a_space() {
        let stored = stored_before_the_field(true, false, "enter");
        let mut settings = read(&stored);
        assert!(migrate(&mut settings, &stored));
        assert_eq!(settings.append_text, r"\s");
        assert_eq!(settings.profiles[0].values.append_text, r"\s");
        assert_eq!(settings.profiles[1].values.append_text, "");
    }

    #[test]
    fn auto_submit_alone_becomes_its_key_in_every_profile() {
        for (key, value) in [
            ("enter", "{Enter}"),
            ("ctrl_enter", "{Ctrl+Enter}"),
            ("cmd_enter", "{Cmd+Enter}"),
        ] {
            let stored = stored_before_the_field(false, true, key);
            let mut settings = read(&stored);
            assert!(migrate(&mut settings, &stored));
            assert_eq!(settings.append_text, value);
            assert_eq!(settings.profiles[0].values.append_text, value);
            assert_eq!(settings.profiles[1].values.append_text, value);
        }
    }

    #[test]
    fn nothing_to_carry_over_needs_no_write() {
        let stored = stored_before_the_field(false, false, "enter");
        let mut settings = read(&stored);
        assert!(!migrate(&mut settings, &stored));
        assert_eq!(settings.append_text, "");
    }

    #[test]
    fn a_stored_value_stays_even_with_the_old_settings_on() {
        let mut stored = stored_before_the_field(true, true, "enter");
        stored["append_text"] = serde_json::json!("MfG");
        stored["profiles"][1]["values"]["append_text"] = serde_json::json!("{Tab}");
        let mut settings = read(&stored);
        assert!(migrate(&mut settings, &stored));
        assert_eq!(settings.append_text, "MfG");
        assert_eq!(settings.profiles[1].values.append_text, "{Tab}");
        // Only "default" lacked the field.
        assert_eq!(settings.profiles[0].values.append_text, r"\s{Enter}");
    }

    // ---- Profiles -------------------------------------------------------------

    #[test]
    fn the_field_is_a_profile_value() {
        let mut settings = settings_with(r"\s{Enter}");
        let values = ProfileValues::from_settings(&settings);
        assert_eq!(values.append_text, r"\s{Enter}");
        let other = ProfileValues {
            append_text: "{Tab}".to_string(),
            ..values
        };
        other.apply_to(&mut settings);
        assert_eq!(settings.append_text, "{Tab}");
    }

    #[test]
    fn a_profile_file_from_before_the_field_gets_it_from_its_trailing_space() {
        let older = "format: danywhere-profile\nversion: 1\nprofile:\n  id: x\n  name: Old\n  values:\n    append_trailing_space: true\n";
        let mut values = ProfileValues {
            append_text: String::new(),
            ..ProfileValues::default()
        };
        fill_from_older_export(older, &mut values);
        assert_eq!(values.append_text, r"\s");

        let without_space = older.replace("true", "false");
        let mut values = ProfileValues::default();
        fill_from_older_export(&without_space, &mut values);
        assert_eq!(values.append_text, "");
    }

    #[test]
    fn a_profile_file_with_the_field_keeps_it() {
        let newer = "format: danywhere-profile\nversion: 1\nprofile:\n  id: x\n  name: New\n  values:\n    append_trailing_space: true\n    append_text: '{Enter}'\n";
        let mut values = ProfileValues {
            append_text: "{Enter}".to_string(),
            ..ProfileValues::default()
        };
        fill_from_older_export(newer, &mut values);
        assert_eq!(values.append_text, "{Enter}");
    }
}
