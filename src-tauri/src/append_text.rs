//! DanyWhere: the "Append" field, what a dictation gets appended (user,
//! 2026-11-15: "dass man jede beliebige Zeichenkette anhängen kann, und auch
//! Steuerzeichen"). Since 2026-11-19 every end shortcut has one (`end_keys`):
//! the dictation gets the field of the end shortcut it ended with; the
//! profile's own field went into its end shortcuts.
//!
//! It replaces Handy's "Append Trailing Space" and "Auto Submit". Their
//! fields (`append_trailing_space`, `auto_submit`, `auto_submit_key`) stay in
//! the settings for Handy merges and for the migration, but only this field
//! decides what happens.
//!
//! The field has two controls: a text field and a selection of the key press
//! after the text (user, 2026-11-15: "da soll eine Auswahl angeboten werden,
//! die für dieses Betriebssystem passt. Und da klickt man eins an davon und
//! alles steht"). Neither can show an error:
//! - in the text field `\s` is a space, `\n` a new line, `\t` a tab, `\\` a
//!   backslash; braces and every other backslash are text as written;
//! - the selection offers what fits this system (`KeyPlatform::offered_keys`).
//!
//! Both are stored as one string, as before, so nothing migrates again: the
//! text with its braces written `\{` and `\}`, then the key press in braces,
//! `{Enter}`, `{Tab}`, `{Shift+Enter}`, `{Cmd+Enter}` (`{Win+Enter}`,
//! `{Super+Enter}`), `{Ctrl+Enter}`, as this system writes it (`compose`,
//! `split`). Reading it never fails either: braces anywhere but in the run of
//! key presses at the end, or around anything that names no key press, are
//! text, and so is a backslash before anything but `s`, `n`, `t`, `\`, `{`,
//! `}`. A key press is read by any system's name of it; a value written on
//! another system is still rewritten to this system's names when settings are
//! read and when a profile is imported (`localize`).
//!
//! The text is appended wherever the trailing space was: to the pasted text,
//! to the clipboard copy after a paste, and to the reply of the "Insert Last
//! Dictation" service; since 2026-11-16 also to what "Copy Last Transcript"
//! puts on the clipboard, during a dictation or after it, and to a dictation
//! left on the clipboard because the app in front didn't know the shortcut
//! (both stand in for the service). The key presses follow the paste where auto submit
//! was sent; none with paste method "None" (as auto submit) and none without
//! Accessibility, where they would go nowhere.

use enigo::{Direction, Enigo, Key, Keyboard};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::settings::{AutoSubmitKey, PasteMethod};

/// The system whose key names and key presses the field uses.
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

    /// The key presses the selection offers here, in its order. No
    /// Win+Enter on Windows, where it turns Narrator on or off, and no
    /// Super+Enter on Linux, where Super combinations belong to the window
    /// manager.
    pub fn offered_keys(self) -> &'static [AppendKey] {
        use AppendKey::*;
        match self {
            Self::Macos => &[Enter, ShiftEnter, CmdEnter, CtrlEnter, Tab],
            Self::Windows | Self::Linux => &[Enter, ShiftEnter, CtrlEnter, Tab],
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
/// are one key, so a name of another system's is read as that key, and
/// rewritten when a value moves.
const META_NAMES: [&str; 5] = ["cmd", "command", "win", "super", "meta"];

/// A key press the field can end with.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum AppendKey {
    Enter,
    Tab,
    ShiftEnter,
    /// Cmd+Enter on macOS, Win+Enter on Windows, Super+Enter on Linux, as
    /// Handy's `AutoSubmitKey::CmdEnter`. Offered only on the Mac; elsewhere
    /// only a stored value can hold it.
    CmdEnter,
    CtrlEnter,
}

impl AppendKey {
    /// The key press as `platform` writes it in the stored value, without
    /// the braces.
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
    Key {
        key: AppendKey,
    },
}

/// The stored value as the field's two controls show it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct AppendChoice {
    /// The text as the text field shows it: `\s`, `\n`, `\t` and `\\` as
    /// written, braces and every other character as they are.
    pub text: String,
    /// The key presses after the text, in their order; none is empty.
    pub keys: Vec<AppendKey>,
    /// Whether the selection has no entry for `keys`: several key presses,
    /// or one this system doesn't offer, as an older version or a hand-edited
    /// settings file may have stored. The selection then shows them as an
    /// extra entry, and they stay, and are sent, until another is chosen.
    pub kept: bool,
}

/// A value to store, made from the field's two controls.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct AppendComposed {
    /// The stored string.
    pub value: String,
    /// Its parts, for the preview in words under the field.
    pub parts: Vec<AppendPart>,
}

/// One piece of the stored value.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    /// `\s`, `\n`, `\t` or `\\`, by the character after the backslash; the
    /// text field shows them as written.
    Escape(char),
    /// A character of text as it is.
    Char(char),
    /// A key press in the run at the end.
    Key(AppendKey),
}

/// The stored value's pieces. `\s`, `\n`, `\t` and `\\` are escapes, `\{`
/// and `\}` braces; a backslash before anything else, or at the end, is
/// itself. A key press, by any system's name of it, counts only in the run
/// at the end; any other braces are text as written.
fn tokens(value: &str) -> Vec<Token> {
    let chars: Vec<char> = value.chars().collect();
    // Each key press with the text it stands for, should it not be at the end.
    let mut tokens: Vec<(Token, String)> = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        let next = chars.get(index + 1).copied();
        if c == '\\' {
            match next {
                Some(escaped @ ('s' | 'n' | 't' | '\\')) => {
                    tokens.push((Token::Escape(escaped), String::new()));
                    index += 2;
                }
                Some(brace @ ('{' | '}')) => {
                    tokens.push((Token::Char(brace), String::new()));
                    index += 2;
                }
                _ => {
                    tokens.push((Token::Char(c), String::new()));
                    index += 1;
                }
            }
            continue;
        }
        if c == '{' {
            // To the next brace; a key press if that one closes it.
            let rest = &chars[index + 1..];
            let close = rest
                .iter()
                .position(|&c| c == '{' || c == '}')
                .filter(|&at| rest[at] == '}');
            if let Some(at) = close {
                let name: String = rest[..at].iter().collect();
                if let Some(key) = key_of(&name) {
                    tokens.push((Token::Key(key), format!("{{{name}}}")));
                    index += at + 2;
                    continue;
                }
            }
        }
        tokens.push((Token::Char(c), String::new()));
        index += 1;
    }
    let keys_from = tokens
        .iter()
        .rposition(|(token, _)| !matches!(token, Token::Key(_)))
        .map_or(0, |last_text| last_text + 1);
    let mut result = Vec::with_capacity(tokens.len());
    for (index, (token, written)) in tokens.into_iter().enumerate() {
        match token {
            Token::Key(_) if index < keys_from => result.extend(written.chars().map(Token::Char)),
            token => result.push(token),
        }
    }
    result
}

/// Reads the stored value into its parts. Never fails: see the module
/// documentation for what counts as text.
pub fn parse(value: &str) -> Vec<AppendPart> {
    let mut parts = Vec::new();
    // Text as written, not yet a part.
    let mut run = String::new();
    for token in tokens(value) {
        let part = match token {
            Token::Escape('s') => AppendPart::Space,
            Token::Escape('n') => AppendPart::NewLine,
            Token::Escape('t') => AppendPart::Tab,
            Token::Escape(c) | Token::Char(c) => {
                run.push(c);
                continue;
            }
            Token::Key(key) => AppendPart::Key { key },
        };
        push_text(&mut parts, &mut run);
        parts.push(part);
    }
    push_text(&mut parts, &mut run);
    parts
}

/// The stored value as the text field and the selection show it.
pub fn split(value: &str, platform: KeyPlatform) -> AppendChoice {
    let mut text = String::new();
    let mut keys = Vec::new();
    for token in tokens(value) {
        match token {
            Token::Escape(c) => {
                text.push('\\');
                text.push(c);
            }
            Token::Char(c) => text.push(c),
            Token::Key(key) => keys.push(key),
        }
    }
    let shown = match keys.as_slice() {
        [] => true,
        [key] => platform.offered_keys().contains(key),
        _ => false,
    };
    AppendChoice {
        text,
        keys,
        kept: !shown,
    }
}

/// The value to store for the text field's `text` and the selection's
/// `keys`, with `platform`'s key names: the text's braces written `\{` and
/// `\}`, then the key presses in braces. A lone backslash stays as it is,
/// unless a brace or a key press follows, with which it would be read as
/// `\{`; then it is written `\\`, the same character.
pub fn compose(text: &str, keys: &[AppendKey], platform: KeyPlatform) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut value = String::with_capacity(text.len() + 16);
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        let next = chars.get(index + 1).copied();
        match c {
            '\\' => match next {
                Some(escaped @ ('s' | 'n' | 't' | '\\')) => {
                    value.push(c);
                    value.push(escaped);
                    index += 1;
                }
                Some('{' | '}') => value.push_str(r"\\"),
                None if !keys.is_empty() => value.push_str(r"\\"),
                _ => value.push(c),
            },
            '{' => value.push_str(r"\{"),
            '}' => value.push_str(r"\}"),
            c => value.push(c),
        }
        index += 1;
    }
    for key in keys {
        value.push('{');
        value.push_str(&key.name(platform));
        value.push('}');
    }
    value
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
    /// A key press with a name of another system's.
    Foreign(AppendKey),
    Unknown,
}

/// A name between braces as its modifier and base, lower case and without
/// spaces: `{ Shift + Enter }` → `(Some("shift"), "enter")`.
fn name_parts(name: &str) -> (Option<String>, String) {
    let name: String = name
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase();
    match name.split_once('+') {
        Some((modifier, base)) => (Some(modifier.to_string()), base.to_string()),
        None => (None, name),
    }
}

/// The key press named between braces, by any system's names. Case and
/// spaces don't count; `Control` is Ctrl everywhere.
fn key_of(name: &str) -> Option<AppendKey> {
    let (modifier, base) = name_parts(name);
    let enter = matches!(base.as_str(), "enter" | "return");
    match modifier.as_deref() {
        None if enter => Some(AppendKey::Enter),
        None if base == "tab" => Some(AppendKey::Tab),
        Some("shift") if enter => Some(AppendKey::ShiftEnter),
        Some(meta) if enter && META_NAMES.contains(&meta) => Some(AppendKey::CmdEnter),
        Some("ctrl" | "control") if enter => Some(AppendKey::CtrlEnter),
        _ => None,
    }
}

/// The key press named between braces, and whether `platform` names it so.
fn key_named(name: &str, platform: KeyPlatform) -> KeyName {
    let Some(key) = key_of(name) else {
        return KeyName::Unknown;
    };
    let (modifier, base) = name_parts(name);
    let foreign_meta = modifier
        .as_deref()
        .is_some_and(|meta| META_NAMES.contains(&meta) && !platform.meta_names().contains(&meta));
    let foreign_return = base == "return" && !platform.knows_return();
    if foreign_meta || foreign_return {
        KeyName::Foreign(key)
    } else {
        KeyName::Local(key)
    }
}

/// `value` with every key press that uses another system's name of a key
/// written as `platform` writes it (`{Win+Enter}` → `{Cmd+Enter}` on the
/// Mac, `{Return}` → `{Enter}` elsewhere). Everything else stays as written,
/// this system's own other names (`{Command+Enter}`) and text in braces too.
pub fn localize(value: &str, platform: KeyPlatform) -> String {
    // Read as the old syntax did: a backslash takes the next character with
    // it, a `{` runs to the next `}`.
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
                    KeyName::Foreign(key) if closed => {
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
                AppendPart::Key { key } => append.keys.push(*key),
            }
        }
        append
    }
}

/// What an "Append" value (an end shortcut's, `end_keys`) appends, with the
/// key presses as stored, also those the selection doesn't offer here.
pub fn for_value(value: &str) -> Append {
    Append::from_parts(&parse(value))
}

/// `text` as it is pasted: with the text of the "Append" value `value`.
pub fn text_to_paste(text: String, value: &str) -> String {
    text + &for_value(value).text
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
pub fn stored_trailing_space(stored: Option<&serde_json::Value>) -> bool {
    stored
        .and_then(|values| values.get("append_trailing_space"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

/// The "Append" text of a stored object (the settings or a profile's
/// values, `stored`): its field, or for one from before the field the
/// trailing space (`trailing_space`) and auto submit; with `platform`'s key
/// names. The end shortcuts take it over (`end_keys::migrate`).
pub fn stored_value(
    stored: Option<&serde_json::Value>,
    trailing_space: bool,
    auto_submit: bool,
    key: AutoSubmitKey,
    platform: KeyPlatform,
) -> String {
    let value = match stored
        .and_then(|values| values.get("append_text"))
        .and_then(|value| value.as_str())
    {
        Some(value) => value.to_string(),
        None => legacy_value(trailing_space, auto_submit, key, platform),
    };
    localize(&value, platform)
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// The key presses the selection offers on this system, in its order.
#[tauri::command]
#[specta::specta]
pub fn append_key_options() -> Vec<AppendKey> {
    KeyPlatform::current().offered_keys().to_vec()
}

/// The stored value as the text field and the selection show it.
#[tauri::command]
#[specta::specta]
pub fn split_append_text(value: String) -> AppendChoice {
    split(&value, KeyPlatform::current())
}

/// The value to store for the text field's `text` and the selection's
/// `keys`, and its parts for the preview.
#[tauri::command]
#[specta::specta]
pub fn compose_append_text(text: String, keys: Vec<AppendKey>) -> AppendComposed {
    let value = compose(&text, &keys, KeyPlatform::current());
    let parts = parse(&value);
    AppendComposed { value, parts }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::ProfileValues;
    use crate::settings::{get_default_settings, AppSettings};
    use AppendKey::*;

    fn text(text: &str) -> AppendPart {
        AppendPart::Text {
            text: text.to_string(),
        }
    }

    fn key(key: AppendKey) -> AppendPart {
        AppendPart::Key { key }
    }

    const MAC: KeyPlatform = KeyPlatform::Macos;
    const WINDOWS: KeyPlatform = KeyPlatform::Windows;
    const LINUX: KeyPlatform = KeyPlatform::Linux;
    const ALL: [KeyPlatform; 3] = [MAC, WINDOWS, LINUX];
    const EVERY_KEY: [AppendKey; 5] = [Enter, Tab, ShiftEnter, CmdEnter, CtrlEnter];

    /// The keys of a stored value.
    fn keys_of(value: &str) -> Vec<AppendKey> {
        Append::from_parts(&parse(value)).keys
    }

    fn choice(text: &str, keys: &[AppendKey], kept: bool) -> AppendChoice {
        AppendChoice {
            text: text.to_string(),
            keys: keys.to_vec(),
            kept,
        }
    }

    /// What the text field's `text` appends, read by its own rules: `\s`,
    /// `\n`, `\t`, `\\`, everything else as it is. An oracle independent of
    /// `compose` and `parse`.
    fn meaning(text: &str) -> String {
        let mut meant = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' {
                let escaped = match chars.peek() {
                    Some('s') => Some(' '),
                    Some('n') => Some('\n'),
                    Some('t') => Some('\t'),
                    Some('\\') => Some('\\'),
                    _ => None,
                };
                if let Some(escaped) = escaped {
                    chars.next();
                    meant.push(escaped);
                    continue;
                }
            }
            meant.push(c);
        }
        meant
    }

    /// `text` as the text field shows it again after storing: a lone
    /// backslash right before a brace, or at the end with a key press after
    /// it, comes back as `\\`, which means the same.
    fn shown_again(text: &str, keys: &[AppendKey]) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut shown = String::new();
        let mut index = 0;
        while index < chars.len() {
            let c = chars[index];
            let next = chars.get(index + 1).copied();
            if c == '\\' && matches!(next, Some('s' | 'n' | 't' | '\\')) {
                shown.push(c);
                shown.extend(next);
                index += 2;
                continue;
            }
            let lone_before_brace = c == '\\' && matches!(next, Some('{' | '}'));
            let lone_before_key = c == '\\' && next.is_none() && !keys.is_empty();
            if lone_before_brace || lone_before_key {
                shown.push_str(r"\\");
            } else {
                shown.push(c);
            }
            index += 1;
        }
        shown
    }

    // ---- The selection ------------------------------------------------------

    #[test]
    fn the_mac_offers_enter_shift_cmd_and_ctrl_enter_and_tab() {
        assert_eq!(
            MAC.offered_keys(),
            [Enter, ShiftEnter, CmdEnter, CtrlEnter, Tab]
        );
    }

    #[test]
    fn windows_and_linux_offer_no_key_press_with_the_windows_or_super_key() {
        for platform in [WINDOWS, LINUX] {
            assert_eq!(
                platform.offered_keys(),
                [Enter, ShiftEnter, CtrlEnter, Tab],
                "{platform:?}"
            );
        }
    }

    #[test]
    fn the_command_offers_this_systems_key_presses() {
        assert_eq!(
            append_key_options(),
            KeyPlatform::current().offered_keys().to_vec()
        );
    }

    #[test]
    fn the_selection_shows_none_or_one_offered_key_press_as_its_entry() {
        for platform in ALL {
            assert_eq!(split("", platform), choice("", &[], false));
            for &offered in platform.offered_keys() {
                let value = compose("", &[offered], platform);
                assert_eq!(
                    split(&value, platform),
                    choice("", &[offered], false),
                    "{value} on {platform:?}"
                );
            }
        }
    }

    #[test]
    fn a_stored_key_press_the_selection_cannot_show_is_kept_as_an_extra_entry() {
        for platform in ALL {
            assert_eq!(
                split(r"\s{Tab}{Enter}", platform),
                choice(r"\s", &[Tab, Enter], true),
                "{platform:?}"
            );
        }
        assert_eq!(split("{Cmd+Enter}", MAC), choice("", &[CmdEnter], false));
        assert_eq!(split("{Win+Enter}", WINDOWS), choice("", &[CmdEnter], true));
        assert_eq!(
            split("MfG{Super+Enter}", LINUX),
            choice("MfG", &[CmdEnter], true)
        );
    }

    #[test]
    fn a_kept_key_press_is_still_sent_and_stays_when_the_text_changes() {
        for (value, keys) in [
            ("{Tab}{Enter}", vec![Tab, Enter]),
            (r"\s{Win+Enter}", vec![CmdEnter]),
            ("{Super+Enter}", vec![CmdEnter]),
        ] {
            let append = for_value(value);
            assert_eq!(
                keys_after_paste(&append, PasteMethod::CtrlV, false),
                keys,
                "{value}"
            );
            for platform in ALL {
                let kept = split(value, platform);
                let changed = compose("Neu", &kept.keys, platform);
                assert_eq!(keys_of(&changed), keys, "{value} on {platform:?}");
                assert_eq!(Append::from_parts(&parse(&changed)).text, "Neu");
            }
        }
    }

    // ---- Composing and splitting the stored value ---------------------------

    #[test]
    fn composing_writes_the_texts_braces_escaped_and_the_key_press_at_the_end() {
        for platform in ALL {
            assert_eq!(compose("", &[], platform), "");
            assert_eq!(compose("", &[Tab], platform), "{Tab}");
            assert_eq!(compose(r"\s", &[Enter], platform), r"\s{Enter}");
            assert_eq!(compose("{x}", &[], platform), r"\{x\}");
            assert_eq!(
                compose("Gruß {Name}", &[ShiftEnter], platform),
                r"Gruß \{Name\}{Shift+Enter}"
            );
            assert_eq!(compose("{Enter}", &[], platform), r"\{Enter\}");
            assert_eq!(compose(r"a\x\\b", &[], platform), r"a\x\\b");
        }
    }

    #[test]
    fn each_key_press_is_stored_as_the_system_writes_it() {
        for (platform, cmd_enter) in [
            (MAC, "{Cmd+Enter}"),
            (WINDOWS, "{Win+Enter}"),
            (LINUX, "{Super+Enter}"),
        ] {
            assert_eq!(compose("", &[CmdEnter], platform), cmd_enter);
            assert_eq!(compose("", &[Enter], platform), "{Enter}");
            assert_eq!(compose("", &[ShiftEnter], platform), "{Shift+Enter}");
            assert_eq!(compose("", &[CtrlEnter], platform), "{Ctrl+Enter}");
            assert_eq!(compose("", &[Tab], platform), "{Tab}");
        }
    }

    #[test]
    fn a_lone_backslash_before_a_brace_or_a_key_press_is_stored_as_two() {
        for platform in ALL {
            assert_eq!(compose(r"\{", &[], platform), r"\\\{");
            assert_eq!(compose(r"a\}", &[], platform), r"a\\\}");
            assert_eq!(compose(r"a\", &[Enter], platform), r"a\\{Enter}");
            assert_eq!(compose(r"a\", &[], platform), r"a\");
        }
    }

    #[test]
    fn splitting_gives_the_text_as_the_field_shows_it() {
        assert_eq!(split(r"\s{Enter}", MAC), choice(r"\s", &[Enter], false));
        assert_eq!(split(r"a\{b\}c\\", MAC), choice(r"a{b}c\\", &[], false));
        assert_eq!(split(r"\n\t\x", MAC), choice(r"\n\t\x", &[], false));
        assert_eq!(
            split(r"MfG\n{Cmd+Enter}", MAC),
            choice(r"MfG\n", &[CmdEnter], false)
        );
        assert_eq!(split("{Foo}", MAC), choice("{Foo}", &[], false));
    }

    #[test]
    fn every_text_and_key_press_survives_storing_and_reading() {
        let alphabet = ['a', ' ', '\\', '{', '}', 's', 'n', 'x'];
        let mut texts = vec![String::new()];
        let mut previous = vec![String::new()];
        for _ in 0..4 {
            let longer: Vec<String> = previous
                .iter()
                .flat_map(|text| alphabet.iter().map(move |c| format!("{text}{c}")))
                .collect();
            texts.extend(longer.iter().cloned());
            previous = longer;
        }
        let mut key_choices: Vec<Vec<AppendKey>> = vec![vec![], vec![Tab, Enter]];
        key_choices.extend(EVERY_KEY.iter().map(|&key| vec![key]));
        for platform in ALL {
            for text in &texts {
                for keys in &key_choices {
                    let value = compose(text, keys, platform);
                    let label = format!("{text:?} {keys:?} on {platform:?} as {value:?}");
                    // A dictation gets the text's meaning and the key presses.
                    assert_eq!(
                        Append::from_parts(&parse(&value)),
                        Append {
                            text: meaning(text),
                            keys: keys.clone(),
                        },
                        "{label}"
                    );
                    // The field shows it again as typed, but for a lone
                    // backslash that had to be stored as two.
                    let back = split(&value, platform);
                    assert_eq!(back.keys, *keys, "{label}");
                    assert_eq!(back.text, shown_again(text, keys), "{label}");
                    // Storing what is shown changes nothing.
                    assert_eq!(compose(&back.text, &back.keys, platform), value, "{label}");
                }
            }
        }
    }

    #[test]
    fn the_command_gives_the_value_and_its_parts() {
        let composed = compose_append_text(r"\s{x}".to_string(), vec![Enter]);
        assert_eq!(composed.value, r"\s\{x\}{Enter}");
        assert_eq!(
            composed.parts,
            vec![AppendPart::Space, text("{x}"), key(Enter)]
        );
    }

    // ---- Reading the stored value -------------------------------------------

    #[test]
    fn an_empty_field_appends_nothing() {
        assert_eq!(parse(""), vec![]);
        assert_eq!(Append::from_parts(&[]), Append::default());
    }

    #[test]
    fn plain_text_is_appended_as_written() {
        assert_eq!(parse("Gruß, J."), vec![text("Gruß, J.")]);
    }

    #[test]
    fn escapes_stand_for_space_new_line_and_tab() {
        assert_eq!(parse(r"\s"), vec![AppendPart::Space]);
        assert_eq!(parse(r"\n"), vec![AppendPart::NewLine]);
        assert_eq!(parse(r"\t"), vec![AppendPart::Tab]);
        assert_eq!(
            parse(r"\n\n"),
            vec![AppendPart::NewLine, AppendPart::NewLine]
        );
    }

    #[test]
    fn escaped_backslash_and_braces_are_text() {
        assert_eq!(parse(r"a\{b\}c\\"), vec![text(r"a{b}c\")]);
    }

    #[test]
    fn every_key_press_in_braces() {
        assert_eq!(parse("{Enter}"), vec![key(Enter)]);
        assert_eq!(parse("{Tab}"), vec![key(Tab)]);
        assert_eq!(parse("{Shift+Enter}"), vec![key(ShiftEnter)]);
        assert_eq!(parse("{Ctrl+Enter}"), vec![key(CtrlEnter)]);
        for platform in ALL {
            assert_eq!(
                parse(&format!("{{{}}}", CmdEnter.name(platform))),
                vec![key(CmdEnter)]
            );
        }
    }

    #[test]
    fn key_names_ignore_case_and_spaces() {
        assert_eq!(keys_of("{enter}"), vec![Enter]);
        assert_eq!(keys_of("{TAB}"), vec![Tab]);
        assert_eq!(keys_of("{ shift + enter }"), vec![ShiftEnter]);
        assert_eq!(keys_of("{CTRL+enter}"), vec![CtrlEnter]);
        assert_eq!(keys_of("{Control+Enter}"), vec![CtrlEnter]);
    }

    /// A safeguard behind `localize`: whatever system wrote a key press, it
    /// is read as that key press.
    #[test]
    fn any_systems_name_of_a_key_is_read_as_that_key() {
        for value in [
            "{Cmd+Enter}",
            "{Command+Return}",
            "{Win+Enter}",
            "{Super+Enter}",
            "{meta+enter}",
        ] {
            assert_eq!(keys_of(value), vec![CmdEnter], "{value}");
        }
        assert_eq!(keys_of("{Return}"), vec![Enter]);
        assert_eq!(keys_of("{Shift+Return}"), vec![ShiftEnter]);
    }

    #[test]
    fn text_then_key_presses_in_their_order() {
        assert_eq!(parse(r"\s{Enter}"), vec![AppendPart::Space, key(Enter)]);
        assert_eq!(
            parse(r"OK\n{Tab}{Enter}"),
            vec![text("OK"), AppendPart::NewLine, key(Tab), key(Enter)]
        );
    }

    #[test]
    fn typed_spaces_at_the_ends_of_text_are_parts_of_their_own() {
        assert_eq!(parse("foo bar "), vec![text("foo bar"), AppendPart::Space]);
        assert_eq!(
            parse(" – "),
            vec![AppendPart::Space, text("–"), AppendPart::Space]
        );
        assert_eq!(parse("  "), vec![AppendPart::Space, AppendPart::Space]);
        assert_eq!(
            parse(r"a\sb"),
            vec![text("a"), AppendPart::Space, text("b")]
        );
    }

    #[test]
    fn any_other_backslash_is_text_as_written() {
        assert_eq!(parse(r"\x"), vec![text(r"\x")]);
        assert_eq!(parse(r"ab\r"), vec![text(r"ab\r")]);
        assert_eq!(parse(r"C:\Users"), vec![text(r"C:\Users")]);
        assert_eq!(parse(r"abc\"), vec![text(r"abc\")]);
    }

    #[test]
    fn braces_that_are_no_key_press_at_the_end_are_text() {
        assert_eq!(parse("{Foo}"), vec![text("{Foo}")]);
        assert_eq!(parse("{}"), vec![text("{}")]);
        assert_eq!(parse("{Shift+Tab}"), vec![text("{Shift+Tab}")]);
        assert_eq!(parse("{Enter"), vec![text("{Enter")]);
        assert_eq!(parse("a}"), vec![text("a}")]);
        assert_eq!(parse("{Enter}x"), vec![text("{Enter}x")]);
        assert_eq!(parse("{Enter} "), vec![text("{Enter}"), AppendPart::Space]);
        assert_eq!(
            parse(r"{Tab}\n{Enter}"),
            vec![text("{Tab}"), AppendPart::NewLine, key(Enter)]
        );
        assert_eq!(parse("{a{Enter}"), vec![text("{a"), key(Enter)]);
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
    fn localize_leaves_text_and_braces_without_a_key_press_as_written() {
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
    fn every_value_stored_on_one_system_means_the_same_on_every_other() {
        let keys = EVERY_KEY.to_vec();
        for from in ALL {
            let written = compose(r"\s", &keys, from);
            for to in ALL {
                let moved = localize(&written, to);
                assert_eq!(keys_of(&moved), keys, "{written} moved to {to:?}");
                // Moving again changes nothing.
                assert_eq!(localize(&moved, to), moved);
            }
        }
    }

    // ---- What a dictation gets ----------------------------------------------

    #[test]
    fn text_parts_are_appended_and_keys_follow() {
        assert_eq!(
            Append::from_parts(&parse(r"\s\n\t\\{Tab}{Enter}")),
            Append {
                text: " \n\t\\".to_string(),
                keys: vec![Tab, Enter],
            }
        );
    }

    #[test]
    fn the_pasted_text_gets_the_fields_text() {
        assert_eq!(text_to_paste("Hallo".into(), r"\s{Enter}"), "Hallo ");
        assert_eq!(text_to_paste("Hallo".into(), r"\nMfG"), "Hallo\nMfG");
    }

    #[test]
    fn an_empty_value_appends_nothing() {
        assert_eq!(text_to_paste("Hallo".into(), ""), "Hallo");
        assert_eq!(for_value(""), Append::default());
    }

    #[test]
    fn a_hand_edited_value_appends_its_braces_as_text() {
        assert_eq!(text_to_paste("Hallo".into(), "{Foo}"), "Hallo{Foo}");
        assert_eq!(for_value("{Enter}x").keys, vec![]);
        assert_eq!(text_to_paste("Hallo".into(), "{Enter}x"), "Hallo{Enter}x");
    }

    #[test]
    fn key_presses_follow_every_paste_method_but_none() {
        let append = for_value(r"\s{Shift+Enter}{Enter}");
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
        let append = for_value(r"\s{Enter}");
        assert!(keys_after_paste(&append, PasteMethod::CtrlV, true).is_empty());
        assert_eq!(text_to_paste("Hallo".into(), r"\s{Enter}"), "Hallo ");
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
                assert_eq!(Append::from_parts(&parse(&value)), expected, "{value}");
            }
        }
    }

    /// A stored store as written before the field (the end shortcuts took it
    /// over, `end_keys::migrate`): the old settings and two
    /// profiles, "default" with the trailing space and "work" without.
    fn stored_before_the_field(
        trailing_space: bool,
        auto_submit: bool,
        key: &str,
    ) -> serde_json::Value {
        let mut settings = get_default_settings();
        settings.append_trailing_space = trailing_space;
        settings.auto_submit = auto_submit;
        // The shape of a store from before the defaults, when "Standard"
        // was a profile.
        let values = crate::profiles::ProfileValues::from_settings(&settings);
        settings.profiles = vec![
            crate::profiles::Profile::new(crate::profiles::DEFAULT_PROFILE_ID, "", values.clone()),
            crate::profiles::Profile::new("work", "Work", values),
        ];
        settings.active_profile_id = crate::profiles::DEFAULT_PROFILE_ID.to_string();
        let mut stored = serde_json::to_value(&settings).expect("serializes");
        let object = stored.as_object_mut().expect("an object");
        object.remove("end_keys");
        object.insert("auto_submit_key".into(), serde_json::json!(key));
        for (index, profile) in object["profiles"]
            .as_array_mut()
            .expect("profiles")
            .iter_mut()
            .enumerate()
        {
            let values = profile["values"].as_object_mut().expect("values");
            values.remove("end_keys");
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
            assert!(crate::end_keys::migrate_on(
                &mut settings,
                &stored,
                platform
            ));
            let with_space = format!(r"\s{cmd_enter}");
            assert_eq!(settings.end_keys[0].append_text, with_space);
            assert_eq!(
                settings.profiles[0].values.end_keys[0].append_text,
                with_space
            );
            // auto_submit was global, so every profile gets its key.
            assert_eq!(
                settings.profiles[1].values.end_keys[0].append_text,
                cmd_enter
            );

            // Once stored, nothing changes any more.
            let stored = serde_json::to_value(&settings).expect("serializes");
            let mut again = read(&stored);
            assert!(!crate::end_keys::migrate_on(&mut again, &stored, platform));
            assert_eq!(again.end_keys[0].append_text, with_space);
        }
    }

    #[test]
    fn the_trailing_space_alone_becomes_a_space() {
        let stored = stored_before_the_field(true, false, "enter");
        let mut settings = read(&stored);
        assert!(crate::end_keys::migrate(&mut settings, &stored));
        assert_eq!(settings.end_keys[0].append_text, r"\s");
        assert_eq!(settings.profiles[0].values.end_keys[0].append_text, r"\s");
        assert_eq!(settings.profiles[1].values.end_keys[0].append_text, "");
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
                assert!(crate::end_keys::migrate_on(
                    &mut settings,
                    &stored,
                    platform
                ));
                assert_eq!(settings.end_keys[0].append_text, value);
                assert_eq!(settings.profiles[0].values.end_keys[0].append_text, value);
                assert_eq!(settings.profiles[1].values.end_keys[0].append_text, value);
            }
        }
    }

    #[test]
    fn nothing_to_carry_over_needs_no_write() {
        let stored = stored_before_the_field(false, false, "enter");
        let mut settings = read(&stored);
        assert!(!crate::end_keys::migrate(&mut settings, &stored));
        assert_eq!(settings.end_keys[0].append_text, "");
    }

    #[test]
    fn a_stored_value_stays_even_with_the_old_settings_on() {
        let mut stored = stored_before_the_field(true, true, "enter");
        stored["append_text"] = serde_json::json!("MfG");
        stored["profiles"][1]["values"]["append_text"] = serde_json::json!("{Tab}");
        let mut settings = read(&stored);
        assert!(crate::end_keys::migrate(&mut settings, &stored));
        assert_eq!(settings.end_keys[0].append_text, "MfG");
        assert_eq!(settings.profiles[1].values.end_keys[0].append_text, "{Tab}");
        // Only "default" lacked the field.
        assert_eq!(
            settings.profiles[0].values.end_keys[0].append_text,
            r"\s{Enter}"
        );
    }

    /// A current store whose values (in the end shortcuts) were written on
    /// another system.
    fn stored_with(settings_value: &str, profile_values: [&str; 2]) -> serde_json::Value {
        let end_keys = |value: &str| {
            serde_json::to_value(crate::end_keys::builtin(value, false)).expect("serializes")
        };
        let mut stored = stored_before_the_field(false, false, "enter");
        stored["end_keys"] = end_keys(settings_value);
        for (index, value) in profile_values.iter().enumerate() {
            stored["profiles"][index]["values"]["end_keys"] = end_keys(value);
        }
        stored
    }

    #[test]
    fn reading_the_store_gives_every_value_the_systems_key_names() {
        let stored = stored_with("{Cmd+Enter}", [r"\s{Return}", "{Super+Enter}"]);
        let mut settings = read(&stored);
        assert!(crate::end_keys::migrate_on(&mut settings, &stored, WINDOWS));
        assert_eq!(settings.end_keys[0].append_text, "{Win+Enter}");
        assert_eq!(
            settings.profiles[0].values.end_keys[0].append_text,
            r"\s{Enter}"
        );
        assert_eq!(
            settings.profiles[1].values.end_keys[0].append_text,
            "{Win+Enter}"
        );

        // Once stored, nothing changes any more.
        let stored = serde_json::to_value(&settings).expect("serializes");
        let mut again = read(&stored);
        assert!(!crate::end_keys::migrate_on(&mut again, &stored, WINDOWS));

        let stored = stored_with("{Win+Enter}", ["{Meta+Enter}", "{Return}"]);
        let mut settings = read(&stored);
        assert!(crate::end_keys::migrate_on(&mut settings, &stored, MAC));
        assert_eq!(settings.end_keys[0].append_text, "{Cmd+Enter}");
        assert_eq!(
            settings.profiles[0].values.end_keys[0].append_text,
            "{Cmd+Enter}"
        );
        assert_eq!(
            settings.profiles[1].values.end_keys[0].append_text,
            "{Return}"
        );
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
                !crate::end_keys::migrate_on(&mut settings, &stored, platform),
                "{platform:?}"
            );
            assert_eq!(settings.end_keys[0].append_text, values[0]);
        }
    }

    // ---- Imports (`end_keys::prepare_import`) --------------------------------

    #[test]
    fn a_profile_file_from_before_the_field_gets_it_from_its_trailing_space() {
        let older = "format: danywhere-profile\nversion: 1\nprofile:\n  id: x\n  name: Old\n  values:\n    append_trailing_space: true\n";
        let mut values = ProfileValues::default();
        crate::end_keys::prepare_import(older, &mut values);
        assert_eq!(values.end_keys[0].append_text, r"\s");

        let without_space = older.replace("true", "false");
        let mut values = ProfileValues::default();
        crate::end_keys::prepare_import(&without_space, &mut values);
        assert_eq!(values.end_keys[0].append_text, "");
    }

    #[test]
    fn a_profile_file_with_the_field_keeps_it() {
        let newer = "format: danywhere-profile\nversion: 1\nprofile:\n  id: x\n  name: New\n  values:\n    append_trailing_space: true\n    append_text: '{Enter}'\n";
        let mut values = ProfileValues::default();
        crate::end_keys::prepare_import(newer, &mut values);
        assert_eq!(values.end_keys[0].append_text, "{Enter}");
        assert_eq!(values.end_keys[1].append_text, "{Enter}");
    }

    #[test]
    fn an_imported_profile_gets_the_systems_key_names() {
        let file = "format: danywhere-profile\nversion: 1\nprofile:\n  id: x\n  name: New\n  values:\n    end_keys: []\n";
        for (platform, written, expected) in [
            (MAC, r"\s{Win+Enter}", r"\s{Cmd+Enter}"),
            (WINDOWS, r"\s{Cmd+Enter}", r"\s{Win+Enter}"),
            (WINDOWS, "{Return}", "{Enter}"),
            (LINUX, "{Command+Return}", "{Super+Enter}"),
            (LINUX, "{Meta+Enter}", "{Meta+Enter}"),
        ] {
            let mut values = ProfileValues {
                end_keys: crate::end_keys::builtin(written, false),
                ..ProfileValues::default()
            };
            crate::end_keys::prepare_import_on(file, &mut values, platform);
            assert_eq!(
                values.end_keys[0].append_text, expected,
                "{written} on {platform:?}"
            );
        }
    }
}
