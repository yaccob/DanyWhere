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
//!   `{Cmd+Enter}`, `{Ctrl+Enter}`, only at the end: once a key press
//!   appears, only key presses may follow.
//!
//! Key names are the system's own (user, 2026-11-15: "das sollte
//! betriebssystemabhängig sein, weil unter Mac jetzt eine Win-Taste zu haben,
//! ist nicht sehr sinnvoll"): the ⌘ key is `Cmd` or `Command` on the Mac,
//! the Windows key `Win` on Windows, the Super key `Super` or `Meta` on
//! Linux; `Return` is Enter only on the Mac. A name of another system's key
//! is an error that names this system's one. A value written on another
//! system is rewritten to this system's names when settings are read and
//! when a profile is imported (`localize`), so it never turns invalid by
//! moving.
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

/// The system whose key names the field uses.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum KeyPlatform {
    Macos,
    Windows,
    Linux,
}

impl KeyPlatform {
    /// This build's system; every other one names keys as Linux does.
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Linux
        }
    }

    /// This system's names of the ⌘ / Windows / Super key, lower case.
    fn meta_names(self) -> &'static [&'static str] {
        match self {
            Self::Macos => &["cmd", "command"],
            Self::Windows => &["win"],
            Self::Linux => &["super", "meta"],
        }
    }

    /// The name this system writes for that key.
    fn meta_name(self) -> &'static str {
        match self {
            Self::Macos => "Cmd",
            Self::Windows => "Win",
            Self::Linux => "Super",
        }
    }

    /// Whether `Return` names Enter: only on the Mac, whose key says so.
    fn knows_return(self) -> bool {
        self == Self::Macos
    }
}

/// Every system's names of the ⌘ / Windows / Super key, lower case. They
/// are one key, so a name of another system's is reported as such, and
/// rewritten when a value moves, instead of being unknown.
const META_NAMES: [&str; 5] = ["cmd", "command", "win", "super", "meta"];

/// A key press the field can end with.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum AppendKey {
    Enter,
    Tab,
    ShiftEnter,
    /// Cmd+Enter on macOS, Win+Enter on Windows, Super+Enter on Linux, as
    /// Handy's `AutoSubmitKey::CmdEnter`.
    CmdEnter,
    CtrlEnter,
}

impl AppendKey {
    /// The key press as `platform` writes it, without the braces.
    pub fn name(self, platform: KeyPlatform) -> String {
        match self {
            Self::Enter => "Enter".to_string(),
            Self::Tab => "Tab".to_string(),
            Self::ShiftEnter => "Shift+Enter".to_string(),
            Self::CmdEnter => format!("{}+Enter", platform.meta_name()),
            Self::CtrlEnter => "Ctrl+Enter".to_string(),
        }
    }
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
    /// A key press, with its `name` as this system writes it (`Win+Enter`),
    /// whichever of its names was typed.
    Key {
        key: AppendKey,
        name: String,
    },
}

/// What the frontend needs to name the field's keys on this system.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct AppendKeyNames {
    /// The system; picks the wording of `ForeignKeyName`.
    pub platform: KeyPlatform,
    /// `AppendKey::CmdEnter` as this system writes it, for the field's
    /// description: `Cmd+Enter`, `Win+Enter` or `Super+Enter`.
    pub cmd_enter: String,
}

/// The names `append_key_names` gives the frontend on `platform`.
pub fn key_names(platform: KeyPlatform) -> AppendKeyNames {
    AppendKeyNames {
        platform,
        cmd_enter: AppendKey::CmdEnter.name(platform),
    }
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
    /// A known key press written with another system's name of a key, such
    /// as `{Win+Enter}` on the Mac or `{Return}` on Windows.
    ForeignKeyName,
}

/// An invalid value: the code and what it is about, the escape (`\x`) for
/// `UnknownEscape`, the name between the braces for `UnknownKey`, this
/// system's name of the key for `ForeignKeyName` (`Cmd`), else empty.
/// `suggestion` is, for `ForeignKeyName` only, the key press as this system
/// writes it, without the braces (`Cmd+Enter`).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct AppendTextError {
    pub code: AppendTextErrorCode,
    pub detail: String,
    pub suggestion: String,
}

impl AppendTextError {
    fn new(code: AppendTextErrorCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
            suggestion: String::new(),
        }
    }
}

impl std::fmt::Display for AppendTextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} {:?}", self.code, self.detail)
    }
}

/// Reads the field into its parts, with `platform`'s key names, or says why
/// it is invalid.
pub fn parse(value: &str, platform: KeyPlatform) -> Result<Vec<AppendPart>, AppendTextError> {
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
                let key = match key_named(&name, platform) {
                    KeyName::Local(key) => key,
                    KeyName::Foreign { key, local } => {
                        return Err(AppendTextError {
                            suggestion: key.name(platform),
                            ..AppendTextError::new(ForeignKeyName, local)
                        })
                    }
                    KeyName::Unknown => return Err(AppendTextError::new(UnknownKey, name)),
                };
                Some(AppendPart::Key {
                    key,
                    name: key.name(platform),
                })
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

/// What a name between braces is on one system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyName {
    /// A key press, with this system's names.
    Local(AppendKey),
    /// A key press with a name of another system's: `local` is this
    /// system's name of that key (the first one, if two are foreign).
    Foreign {
        key: AppendKey,
        local: &'static str,
    },
    Unknown,
}

/// The key press named between braces, with `platform`'s names. Case and
/// spaces don't count; `Control` is Ctrl everywhere.
fn key_named(name: &str, platform: KeyPlatform) -> KeyName {
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
    let key = match modifier {
        None if enter => AppendKey::Enter,
        None if base == "tab" => AppendKey::Tab,
        Some("shift") if enter => AppendKey::ShiftEnter,
        Some(meta) if enter && META_NAMES.contains(&meta) => AppendKey::CmdEnter,
        Some("ctrl" | "control") if enter => AppendKey::CtrlEnter,
        _ => return KeyName::Unknown,
    };
    if let Some(meta) = modifier.filter(|modifier| META_NAMES.contains(modifier)) {
        if !platform.meta_names().contains(&meta) {
            return KeyName::Foreign {
                key,
                local: platform.meta_name(),
            };
        }
    }
    if base == "return" && !platform.knows_return() {
        return KeyName::Foreign {
            key,
            local: "Enter",
        };
    }
    KeyName::Local(key)
}

/// `value` with every key press that uses another system's name of a key
/// written as `platform` writes it (`{Win+Enter}` → `{Cmd+Enter}` on the
/// Mac, `{Return}` → `{Enter}` elsewhere). Everything else stays as written,
/// this system's own other names (`{Command+Enter}`) and invalid parts too.
pub fn localize(value: &str, platform: KeyPlatform) -> String {
    // Read as `parse` does: a backslash takes the next character with it, a
    // `{` runs to the next `}`.
    let mut localized = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                localized.push(c);
                localized.extend(chars.next());
            }
            '{' => {
                let mut name = String::new();
                let mut closed = false;
                for c in chars.by_ref() {
                    if c == '}' {
                        closed = true;
                        break;
                    }
                    name.push(c);
                }
                localized.push('{');
                match key_named(&name, platform) {
                    KeyName::Foreign { key, .. } if closed => {
                        localized.push_str(&key.name(platform));
                    }
                    _ => localized.push_str(&name),
                }
                if closed {
                    localized.push('}');
                }
            }
            c => localized.push(c),
        }
    }
    localized
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
                AppendPart::Key { key, .. } => append.keys.push(*key),
            }
        }
        append
    }
}

/// The "Append" field of `settings`. A stored value that doesn't parse,
/// which only a hand-edited settings file can hold, appends nothing.
pub fn for_settings(settings: &AppSettings) -> Append {
    match parse(&settings.append_text, KeyPlatform::current()) {
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
/// then the auto submit key, as `platform` writes it.
pub fn legacy_value(
    trailing_space: bool,
    auto_submit: bool,
    key: AutoSubmitKey,
    platform: KeyPlatform,
) -> String {
    let mut value = String::new();
    if trailing_space {
        value.push_str(r"\s");
    }
    if auto_submit {
        let key = match key {
            AutoSubmitKey::Enter => AppendKey::Enter,
            AutoSubmitKey::CmdEnter => AppendKey::CmdEnter,
            AutoSubmitKey::CtrlEnter => AppendKey::CtrlEnter,
        };
        value.push('{');
        value.push_str(&key.name(platform));
        value.push('}');
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

/// Run on every read of the store (`settings_value` is the stored JSON),
/// with this system's key names; see `migrate_on`.
pub fn migrate(settings: &mut AppSettings, settings_value: &serde_json::Value) -> bool {
    migrate_on(settings, settings_value, KeyPlatform::current())
}

/// Two steps, for the settings and every stored profile:
/// - once, a value without the field gets it from its
///   `append_trailing_space` and the global auto submit;
/// - a value with another system's key names gets `platform`'s
///   (`localize`), e.g. from a settings file copied from another system or
///   edited by hand. This one runs on every read too: there is no marker
///   that it ran, and a value can come in at any time.
///
/// Returns whether a value changed, so a store with nothing to carry over
/// and only this system's names is never written. A derived value that
/// equals the default (both old settings off) is no change.
pub fn migrate_on(
    settings: &mut AppSettings,
    settings_value: &serde_json::Value,
    platform: KeyPlatform,
) -> bool {
    let (auto_submit, key) = (settings.auto_submit, settings.auto_submit_key);
    let mut changed = false;
    if !has_field(Some(settings_value)) {
        let value = legacy_value(settings.append_trailing_space, auto_submit, key, platform);
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
        let value = legacy_value(
            stored_trailing_space(stored_values),
            auto_submit,
            key,
            platform,
        );
        if profile.values.append_text != value {
            profile.values.append_text = value;
            changed = true;
        }
    }
    let values = std::iter::once(&mut settings.append_text).chain(
        settings
            .profiles
            .iter_mut()
            .map(|profile| &mut profile.values.append_text),
    );
    for value in values {
        let local = localize(value, platform);
        if *value != local {
            log::info!("\"Append\" takes this system's key names: {value:?} → {local:?}");
            *value = local;
            changed = true;
        }
    }
    changed
}

/// For an import (`file_text` is the file as read), with this system's key
/// names; see `prepare_import_on`.
pub fn prepare_import(file_text: &str, values: &mut ProfileValues) {
    prepare_import_on(file_text, values, KeyPlatform::current());
}

/// For an import: a profile file from before the field gets it from its
/// `append_trailing_space`, and a value written on another system gets
/// `platform`'s key names (`localize`).
pub fn prepare_import_on(file_text: &str, values: &mut ProfileValues, platform: KeyPlatform) {
    if let Ok(file) = serde_saphyr::from_str::<serde_json::Value>(file_text) {
        let stored = file
            .get("profile")
            .and_then(|profile| profile.get("values"));
        if !has_field(stored) {
            // auto_submit was global, so the file holds no key press.
            values.append_text = legacy_value(
                stored_trailing_space(stored),
                false,
                AutoSubmitKey::Enter,
                platform,
            );
        }
    }
    values.append_text = localize(&values.append_text, platform);
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// The field's parts for the preview, or why the value is invalid.
#[tauri::command]
#[specta::specta]
pub fn parse_append_text(value: String) -> Result<Vec<AppendPart>, AppendTextError> {
    parse(&value, KeyPlatform::current())
}

/// This system and its name of the ⌘ / Windows / Super key press, for the
/// field's description and the wording of its errors.
#[tauri::command]
#[specta::specta]
pub fn append_key_names() -> AppendKeyNames {
    key_names(KeyPlatform::current())
}

/// Stores the field in the active profile; an invalid value is refused and
/// the stored one stays.
#[tauri::command]
#[specta::specta]
pub fn change_append_text_setting(app: AppHandle, value: String) -> Result<(), AppendTextError> {
    parse(&value, KeyPlatform::current())?;
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

    const MAC: KeyPlatform = KeyPlatform::Macos;
    const WINDOWS: KeyPlatform = KeyPlatform::Windows;
    const LINUX: KeyPlatform = KeyPlatform::Linux;
    const ALL: [KeyPlatform; 3] = [MAC, WINDOWS, LINUX];

    /// A key press as the Mac names it.
    fn key(key: AppendKey) -> AppendPart {
        key_on(MAC, key)
    }

    fn key_on(platform: KeyPlatform, key: AppendKey) -> AppendPart {
        AppendPart::Key {
            key,
            name: key.name(platform),
        }
    }

    fn error(value: &str) -> (AppendTextErrorCode, String) {
        let error = parse(value, MAC).expect_err(value);
        (error.code, error.detail)
    }

    /// Code, detail and suggestion of `value`'s error on `platform`.
    fn error_on(platform: KeyPlatform, value: &str) -> (AppendTextErrorCode, String, String) {
        let error = parse(value, platform).expect_err(value);
        (error.code, error.detail, error.suggestion)
    }

    /// The keys of a valid value on `platform`.
    fn keys_on(platform: KeyPlatform, value: &str) -> Vec<AppendKey> {
        let parts =
            parse(value, platform).unwrap_or_else(|e| panic!("{value} on {platform:?}: {e}"));
        Append::from_parts(&parts).keys
    }

    // ---- Valid values -------------------------------------------------------

    #[test]
    fn an_empty_field_appends_nothing() {
        assert_eq!(parse("", MAC), Ok(vec![]));
        assert_eq!(Append::from_parts(&[]), Append::default());
    }

    #[test]
    fn plain_text_is_appended_as_written() {
        assert_eq!(parse("Gruß, J.", MAC), Ok(vec![text("Gruß, J.")]));
    }

    #[test]
    fn escapes_stand_for_space_new_line_and_tab() {
        assert_eq!(parse(r"\s", MAC), Ok(vec![AppendPart::Space]));
        assert_eq!(parse(r"\n", MAC), Ok(vec![AppendPart::NewLine]));
        assert_eq!(parse(r"\t", MAC), Ok(vec![AppendPart::Tab]));
        assert_eq!(
            parse(r"\n\n", MAC),
            Ok(vec![AppendPart::NewLine, AppendPart::NewLine])
        );
    }

    #[test]
    fn escaped_backslash_and_braces_are_text() {
        assert_eq!(parse(r"a\{b\}c\\", MAC), Ok(vec![text(r"a{b}c\")]));
    }

    #[test]
    fn every_key_press_in_braces() {
        assert_eq!(parse("{Enter}", MAC), Ok(vec![key(Enter)]));
        assert_eq!(parse("{Tab}", MAC), Ok(vec![key(Tab)]));
        assert_eq!(parse("{Shift+Enter}", MAC), Ok(vec![key(ShiftEnter)]));
        assert_eq!(parse("{Cmd+Enter}", MAC), Ok(vec![key(CmdEnter)]));
        assert_eq!(parse("{Ctrl+Enter}", MAC), Ok(vec![key(CtrlEnter)]));
    }

    #[test]
    fn key_names_ignore_case_and_spaces() {
        for platform in ALL {
            assert_eq!(keys_on(platform, "{enter}"), vec![Enter]);
            assert_eq!(keys_on(platform, "{TAB}"), vec![Tab]);
            assert_eq!(keys_on(platform, "{ shift + enter }"), vec![ShiftEnter]);
            assert_eq!(keys_on(platform, "{CTRL+enter}"), vec![CtrlEnter]);
            assert_eq!(keys_on(platform, "{Control+Enter}"), vec![CtrlEnter]);
        }
    }

    // ---- Key names per system -------------------------------------------------

    #[test]
    fn the_mac_knows_cmd_command_and_return() {
        for value in [
            "{Cmd+Enter}",
            "{Command+Enter}",
            "{ command + enter }",
            "{CMD+Return}",
        ] {
            assert_eq!(keys_on(MAC, value), vec![CmdEnter], "{value}");
        }
        assert_eq!(keys_on(MAC, "{Return}"), vec![Enter]);
        assert_eq!(keys_on(MAC, "{Shift+Return}"), vec![ShiftEnter]);
        assert_eq!(keys_on(MAC, "{Ctrl+Return}"), vec![CtrlEnter]);
    }

    #[test]
    fn windows_knows_win() {
        for value in ["{Win+Enter}", "{win + enter}", "{WIN+ENTER}"] {
            assert_eq!(keys_on(WINDOWS, value), vec![CmdEnter], "{value}");
        }
        assert_eq!(keys_on(WINDOWS, "{Enter}{Tab}"), vec![Enter, Tab]);
    }

    #[test]
    fn linux_knows_super_and_meta() {
        for value in [
            "{Super+Enter}",
            "{Meta+Enter}",
            "{ super + enter }",
            "{META+enter}",
        ] {
            assert_eq!(keys_on(LINUX, value), vec![CmdEnter], "{value}");
        }
    }

    #[test]
    fn the_mac_names_the_cmd_key_for_another_systems_name() {
        for value in [
            "{Win+Enter}",
            "{Super+Enter}",
            "{meta+enter}",
            "{Win+Return}",
        ] {
            assert_eq!(
                error_on(MAC, value),
                (ForeignKeyName, "Cmd".to_string(), "Cmd+Enter".to_string()),
                "{value}"
            );
        }
    }

    #[test]
    fn windows_names_the_win_key_and_enter_for_another_systems_name() {
        for value in [
            "{Cmd+Enter}",
            "{Command+Enter}",
            "{Super+Enter}",
            "{Meta+Enter}",
        ] {
            assert_eq!(
                error_on(WINDOWS, value),
                (ForeignKeyName, "Win".to_string(), "Win+Enter".to_string()),
                "{value}"
            );
        }
        // Both names foreign: the first is named, the suggestion fixes both.
        assert_eq!(
            error_on(WINDOWS, "{Cmd+Return}"),
            (ForeignKeyName, "Win".to_string(), "Win+Enter".to_string())
        );
        for (value, suggestion) in [
            ("{Return}", "Enter"),
            ("{Shift+Return}", "Shift+Enter"),
            ("{Ctrl+Return}", "Ctrl+Enter"),
            ("{Win+Return}", "Win+Enter"),
        ] {
            assert_eq!(
                error_on(WINDOWS, value),
                (ForeignKeyName, "Enter".to_string(), suggestion.to_string()),
                "{value}"
            );
        }
    }

    #[test]
    fn linux_names_the_super_key_and_enter_for_another_systems_name() {
        for value in ["{Cmd+Enter}", "{Command+Enter}", "{Win+Enter}"] {
            assert_eq!(
                error_on(LINUX, value),
                (
                    ForeignKeyName,
                    "Super".to_string(),
                    "Super+Enter".to_string()
                ),
                "{value}"
            );
        }
        assert_eq!(
            error_on(LINUX, "{return}"),
            (ForeignKeyName, "Enter".to_string(), "Enter".to_string())
        );
    }

    #[test]
    fn a_foreign_name_in_no_known_key_press_is_unknown() {
        for platform in ALL {
            assert_eq!(
                error_on(platform, "{Win+Tab}").0,
                UnknownKey,
                "{platform:?}"
            );
            assert_eq!(error_on(platform, "{Cmd}").0, UnknownKey, "{platform:?}");
        }
    }

    #[test]
    fn a_foreign_name_is_reported_where_it_stands() {
        // An error before it comes first; text after it is its own error.
        assert_eq!(error_on(MAC, r"\x{Win+Enter}").0, UnknownEscape);
        assert_eq!(error_on(MAC, "{Enter}{Win+Enter}").0, ForeignKeyName);
    }

    #[test]
    fn the_preview_names_each_key_press_as_the_system_writes_it() {
        assert_eq!(
            parse("{ command + return }", MAC),
            Ok(vec![key_on(MAC, CmdEnter)])
        );
        assert_eq!(
            parse("{win+enter}", WINDOWS),
            Ok(vec![key_on(WINDOWS, CmdEnter)])
        );
        assert_eq!(
            parse("{meta+enter}", LINUX),
            Ok(vec![key_on(LINUX, CmdEnter)])
        );
        assert_eq!(parse("{return}", MAC), Ok(vec![key_on(MAC, Enter)]));
        let names = |platform| {
            [Enter, Tab, ShiftEnter, CmdEnter, CtrlEnter].map(|key: AppendKey| key.name(platform))
        };
        assert_eq!(
            names(MAC),
            ["Enter", "Tab", "Shift+Enter", "Cmd+Enter", "Ctrl+Enter"]
        );
        assert_eq!(
            names(WINDOWS),
            ["Enter", "Tab", "Shift+Enter", "Win+Enter", "Ctrl+Enter"]
        );
        assert_eq!(
            names(LINUX),
            ["Enter", "Tab", "Shift+Enter", "Super+Enter", "Ctrl+Enter"]
        );
    }

    #[test]
    fn the_description_gets_the_systems_name_of_the_cmd_key_press() {
        assert_eq!(
            key_names(MAC),
            AppendKeyNames {
                platform: MAC,
                cmd_enter: "Cmd+Enter".to_string()
            }
        );
        assert_eq!(key_names(WINDOWS).cmd_enter, "Win+Enter");
        assert_eq!(key_names(LINUX).cmd_enter, "Super+Enter");
        assert_eq!(
            key_names(KeyPlatform::current()).platform,
            KeyPlatform::current()
        );
    }

    // ---- Moving a value between systems ---------------------------------------

    #[test]
    fn localize_rewrites_only_another_systems_names() {
        for (platform, value, expected) in [
            (MAC, "{Win+Enter}", "{Cmd+Enter}"),
            (MAC, r"\s{ super + enter }", r"\s{Cmd+Enter}"),
            (MAC, "{Meta+Return}", "{Cmd+Enter}"),
            (MAC, "{Command+Enter}", "{Command+Enter}"),
            (MAC, "{Return}", "{Return}"),
            (WINDOWS, "{Cmd+Enter}", "{Win+Enter}"),
            (WINDOWS, "{Super+Enter}", "{Win+Enter}"),
            (WINDOWS, "{Return}", "{Enter}"),
            (
                WINDOWS,
                r"MfG\n{Shift+Return}{Tab}",
                r"MfG\n{Shift+Enter}{Tab}",
            ),
            (WINDOWS, "{win+enter}", "{win+enter}"),
            (LINUX, "{Win+Enter}", "{Super+Enter}"),
            (LINUX, "{Command+Return}", "{Super+Enter}"),
            (LINUX, "{Meta+Enter}", "{Meta+Enter}"),
            (LINUX, "{Ctrl+Return}", "{Ctrl+Enter}"),
        ] {
            assert_eq!(
                localize(value, platform),
                expected,
                "{value} on {platform:?}"
            );
        }
    }

    #[test]
    fn localize_leaves_text_and_invalid_parts_as_written() {
        for platform in ALL {
            for value in [
                "",
                "Gruß",
                r"\{Win+Enter\}",
                r"a\\{Tab}",
                "{Foo}",
                "{Win+Tab}",
                "{Win+Enter",
                "a}",
                r"\x",
                "abc\\",
            ] {
                assert_eq!(localize(value, platform), value, "{value} on {platform:?}");
            }
        }
    }

    #[test]
    fn every_value_written_on_one_system_is_valid_on_every_other() {
        let keys = [Enter, Tab, ShiftEnter, CmdEnter, CtrlEnter];
        for from in ALL {
            let value: String = keys
                .iter()
                .map(|key| format!("{{{}}}", key.name(from)))
                .collect();
            let written = format!(r"\s{value}");
            assert_eq!(keys_on(from, &written), keys, "{written} on {from:?}");
            for to in ALL {
                let moved = localize(&written, to);
                assert_eq!(keys_on(to, &moved), keys, "{written} moved to {to:?}");
                // Moving again changes nothing.
                assert_eq!(localize(&moved, to), moved);
            }
        }
    }

    #[test]
    fn text_then_key_presses_in_their_order() {
        assert_eq!(
            parse(r"\s{Enter}", MAC),
            Ok(vec![AppendPart::Space, key(Enter)])
        );
        assert_eq!(
            parse(r"OK\n{Tab}{Enter}", MAC),
            Ok(vec![text("OK"), AppendPart::NewLine, key(Tab), key(Enter)])
        );
    }

    #[test]
    fn typed_spaces_at_the_ends_of_text_are_parts_of_their_own() {
        assert_eq!(
            parse("foo bar ", MAC),
            Ok(vec![text("foo bar"), AppendPart::Space])
        );
        assert_eq!(
            parse(" – ", MAC),
            Ok(vec![AppendPart::Space, text("–"), AppendPart::Space])
        );
        assert_eq!(
            parse("  ", MAC),
            Ok(vec![AppendPart::Space, AppendPart::Space])
        );
        assert_eq!(
            parse(r"a\sb", MAC),
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
        let parts = parse(r"\s\n\t\\{Tab}{Enter}", MAC).expect("valid");
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
        for platform in ALL {
            assert_eq!(legacy_value(false, false, K::Enter, platform), "");
            assert_eq!(legacy_value(false, false, K::CmdEnter, platform), "");
            assert_eq!(legacy_value(true, false, K::Enter, platform), r"\s");
            assert_eq!(legacy_value(false, true, K::Enter, platform), "{Enter}");
            assert_eq!(
                legacy_value(false, true, K::CtrlEnter, platform),
                "{Ctrl+Enter}"
            );
            assert_eq!(legacy_value(true, true, K::Enter, platform), r"\s{Enter}");
            assert_eq!(
                legacy_value(true, true, K::CtrlEnter, platform),
                r"\s{Ctrl+Enter}"
            );
        }
    }

    #[test]
    fn auto_submit_with_cmd_becomes_the_systems_name_of_the_key() {
        use AutoSubmitKey as K;
        assert_eq!(legacy_value(false, true, K::CmdEnter, MAC), "{Cmd+Enter}");
        assert_eq!(
            legacy_value(false, true, K::CmdEnter, WINDOWS),
            "{Win+Enter}"
        );
        assert_eq!(
            legacy_value(true, true, K::CmdEnter, LINUX),
            r"\s{Super+Enter}"
        );
    }

    #[test]
    fn every_old_value_means_what_the_old_settings_did() {
        use AutoSubmitKey as K;
        for platform in ALL {
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
                let value = legacy_value(trailing, auto, key, platform);
                let parts = parse(&value, platform)
                    .unwrap_or_else(|e| panic!("{value} on {platform:?}: {e}"));
                assert_eq!(Append::from_parts(&parts), expected, "{value}");
            }
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
        for (platform, cmd_enter) in [
            (MAC, "{Cmd+Enter}"),
            (WINDOWS, "{Win+Enter}"),
            (LINUX, "{Super+Enter}"),
        ] {
            let stored = stored_before_the_field(true, true, "cmd_enter");
            let mut settings = read(&stored);
            assert!(migrate_on(&mut settings, &stored, platform));
            let with_space = format!(r"\s{cmd_enter}");
            assert_eq!(settings.append_text, with_space);
            assert_eq!(settings.profiles[0].values.append_text, with_space);
            // auto_submit was global, so every profile gets its key.
            assert_eq!(settings.profiles[1].values.append_text, cmd_enter);

            // Once stored, nothing changes any more.
            let stored = serde_json::to_value(&settings).expect("serializes");
            let mut again = read(&stored);
            assert!(!migrate_on(&mut again, &stored, platform));
            assert_eq!(again.append_text, with_space);
        }
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
        for platform in ALL {
            for (key, value) in [
                ("enter", "{Enter}".to_string()),
                ("ctrl_enter", "{Ctrl+Enter}".to_string()),
                ("cmd_enter", format!("{{{}}}", CmdEnter.name(platform))),
            ] {
                let stored = stored_before_the_field(false, true, key);
                let mut settings = read(&stored);
                assert!(migrate_on(&mut settings, &stored, platform));
                assert_eq!(settings.append_text, value);
                assert_eq!(settings.profiles[0].values.append_text, value);
                assert_eq!(settings.profiles[1].values.append_text, value);
            }
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

    /// A current store whose values were written on another system.
    fn stored_with(settings_value: &str, profile_values: [&str; 2]) -> serde_json::Value {
        let mut stored = stored_before_the_field(false, false, "enter");
        stored["append_text"] = serde_json::json!(settings_value);
        for (index, value) in profile_values.iter().enumerate() {
            stored["profiles"][index]["values"]["append_text"] = serde_json::json!(value);
        }
        stored
    }

    #[test]
    fn reading_the_store_gives_every_value_the_systems_key_names() {
        let stored = stored_with("{Cmd+Enter}", [r"\s{Return}", "{Super+Enter}"]);
        let mut settings = read(&stored);
        assert!(migrate_on(&mut settings, &stored, WINDOWS));
        assert_eq!(settings.append_text, "{Win+Enter}");
        assert_eq!(settings.profiles[0].values.append_text, r"\s{Enter}");
        assert_eq!(settings.profiles[1].values.append_text, "{Win+Enter}");

        // Once stored, nothing changes any more.
        let stored = serde_json::to_value(&settings).expect("serializes");
        let mut again = read(&stored);
        assert!(!migrate_on(&mut again, &stored, WINDOWS));

        let stored = stored_with("{Win+Enter}", ["{Meta+Enter}", "{Return}"]);
        let mut settings = read(&stored);
        assert!(migrate_on(&mut settings, &stored, MAC));
        assert_eq!(settings.append_text, "{Cmd+Enter}");
        assert_eq!(settings.profiles[0].values.append_text, "{Cmd+Enter}");
        assert_eq!(settings.profiles[1].values.append_text, "{Return}");
    }

    #[test]
    fn a_store_with_the_systems_own_names_needs_no_write() {
        for (platform, values) in [
            (MAC, ["{Command+Return}", "{Cmd+Enter}", "{Return}"]),
            (WINDOWS, ["{Win+Enter}", "{win + enter}", r"\s{Tab}"]),
            (LINUX, ["{Meta+Enter}", "{Super+Enter}", "{Foo}"]),
        ] {
            let stored = stored_with(values[0], [values[1], values[2]]);
            let mut settings = read(&stored);
            assert!(
                !migrate_on(&mut settings, &stored, platform),
                "{platform:?}"
            );
            assert_eq!(settings.append_text, values[0]);
        }
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
        prepare_import(older, &mut values);
        assert_eq!(values.append_text, r"\s");

        let without_space = older.replace("true", "false");
        let mut values = ProfileValues::default();
        prepare_import(&without_space, &mut values);
        assert_eq!(values.append_text, "");
    }

    #[test]
    fn a_profile_file_with_the_field_keeps_it() {
        let newer = "format: danywhere-profile\nversion: 1\nprofile:\n  id: x\n  name: New\n  values:\n    append_trailing_space: true\n    append_text: '{Enter}'\n";
        let mut values = ProfileValues {
            append_text: "{Enter}".to_string(),
            ..ProfileValues::default()
        };
        prepare_import(newer, &mut values);
        assert_eq!(values.append_text, "{Enter}");
    }

    #[test]
    fn an_imported_profile_gets_the_systems_key_names() {
        let file = "format: danywhere-profile\nversion: 1\nprofile:\n  id: x\n  name: New\n  values:\n    append_text: 'x'\n";
        for (platform, written, expected) in [
            (MAC, r"\s{Win+Enter}", r"\s{Cmd+Enter}"),
            (WINDOWS, r"\s{Cmd+Enter}", r"\s{Win+Enter}"),
            (WINDOWS, "{Return}", "{Enter}"),
            (LINUX, "{Command+Return}", "{Super+Enter}"),
            (LINUX, "{Meta+Enter}", "{Meta+Enter}"),
        ] {
            let mut values = ProfileValues {
                append_text: written.to_string(),
                ..ProfileValues::default()
            };
            prepare_import_on(file, &mut values, platform);
            assert_eq!(values.append_text, expected, "{written} on {platform:?}");
        }
    }
}
