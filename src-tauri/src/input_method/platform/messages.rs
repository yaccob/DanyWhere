//! DanyWhere: the messages between the app and its input method
//! (PROTOCOL.md, "Messages"): one JSON object per line in UTF-8, each with a
//! `"type"`. Unknown types and unknown fields are ignored, so either side can
//! be newer.

use serde::{Deserialize, Serialize};

/// A message from the input method.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FromInputMethod {
    /// First, right after connecting.
    Hello {
        #[serde(default)]
        version: String,
        #[serde(default)]
        pid: Option<i32>,
    },
    /// A text field of `client` now has the input method.
    Active {
        #[serde(default)]
        client: Option<String>,
    },
    /// `client`'s text field no longer has it.
    Inactive {
        #[serde(default)]
        client: Option<String>,
    },
    /// The answer to `insert` with this `id`.
    Inserted {
        id: u64,
        #[serde(default)]
        ok: bool,
        #[serde(default)]
        client: Option<String>,
        /// `no_client` or `error` when `ok` is false.
        #[serde(default)]
        reason: Option<String>,
    },
    /// Stage 2: a key it was told to catch was pressed.
    Caught {
        id: u64,
        code: u16,
        #[serde(default)]
        mods: u64,
    },
    /// A type this app doesn't know (a newer input method).
    #[serde(other)]
    Unknown,
}

/// A key for stage 2's `catch`: key code and device-independent modifier
/// flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CatchKey {
    pub code: u16,
    pub mods: u64,
}

/// A message to the input method.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToInputMethod {
    /// Insert `text` into the text field that has the input method.
    Insert { id: u64, text: String },
    /// The keyboard layout for typing while its source is selected; `None`
    /// clears it.
    Layout { source: Option<String> },
    /// The source to select if the link breaks while its own is the current
    /// one; `None` clears it.
    Restore { source: Option<String> },
    /// Stage 2: catch the next key-down matching `keys` (none disarms).
    /// The input method knows it already; the app sends it once stage 2
    /// ("insert and send" with Enter) is built.
    #[allow(dead_code)]
    Catch {
        id: u64,
        keys: Vec<CatchKey>,
        #[serde(skip_serializing_if = "Option::is_none")]
        hold_ms: Option<u64>,
    },
    /// Stage 2: insert `text`, then let the held key through.
    #[allow(dead_code)]
    Release { id: u64, text: String },
    /// Exit (the app replaces the bundle).
    Quit,
}

/// One received line as a message; `None` for an empty line or one that
/// isn't a message (never logged: it is the peer's business).
pub fn parse_line(line: &str) -> Option<FromInputMethod> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    serde_json::from_str(line).ok()
}

/// `message` as one line, newline included. JSON escapes line breaks in
/// strings, so a text never splits a message.
pub fn to_line(message: &ToInputMethod) -> String {
    let mut line = serde_json::to_string(message).unwrap_or_else(|e| {
        // Only strings and numbers: serializing can't fail.
        log::error!("Input method message not serialized: {e}");
        String::new()
    });
    line.push('\n');
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    // The examples of PROTOCOL.md, exactly as written there.

    #[test]
    fn hello_from_the_protocol() {
        assert_eq!(
            parse_line(r#"{"type":"hello","version":"0.3.3","pid":123}"#),
            Some(FromInputMethod::Hello {
                version: "0.3.3".into(),
                pid: Some(123)
            })
        );
    }

    #[test]
    fn active_and_inactive_from_the_protocol() {
        assert_eq!(
            parse_line(r#"{"type":"active","client":"com.apple.TextEdit"}"#),
            Some(FromInputMethod::Active {
                client: Some("com.apple.TextEdit".into())
            })
        );
        assert_eq!(
            parse_line(r#"{"type":"inactive","client":"com.apple.TextEdit"}"#),
            Some(FromInputMethod::Inactive {
                client: Some("com.apple.TextEdit".into())
            })
        );
    }

    #[test]
    fn inserted_ok_from_the_protocol() {
        assert_eq!(
            parse_line(r#"{"type":"inserted","id":7,"ok":true,"client":"com.apple.TextEdit"}"#),
            Some(FromInputMethod::Inserted {
                id: 7,
                ok: true,
                client: Some("com.apple.TextEdit".into()),
                reason: None
            })
        );
    }

    #[test]
    fn inserted_failed_from_the_protocol() {
        assert_eq!(
            parse_line(r#"{"type":"inserted","id":7,"ok":false,"reason":"no_client"}"#),
            Some(FromInputMethod::Inserted {
                id: 7,
                ok: false,
                client: None,
                reason: Some("no_client".into())
            })
        );
    }

    #[test]
    fn caught_from_the_protocol() {
        assert_eq!(
            parse_line(r#"{"type":"caught","id":9,"code":36,"mods":0}"#),
            Some(FromInputMethod::Caught {
                id: 9,
                code: 36,
                mods: 0
            })
        );
    }

    #[test]
    fn a_newer_input_methods_unknown_type_is_ignored() {
        assert_eq!(
            parse_line(r#"{"type":"stretch","id":3,"what":[1,2]}"#),
            Some(FromInputMethod::Unknown)
        );
    }

    #[test]
    fn unknown_fields_of_a_known_message_are_ignored() {
        assert_eq!(
            parse_line(r#"{"type":"hello","version":"0.4.0","pid":42,"features":["catch"]}"#),
            Some(FromInputMethod::Hello {
                version: "0.4.0".into(),
                pid: Some(42)
            })
        );
    }

    #[test]
    fn what_isnt_a_message_is_nothing() {
        assert_eq!(parse_line(""), None);
        assert_eq!(parse_line("   "), None);
        assert_eq!(parse_line("not json"), None);
        assert_eq!(parse_line(r#"{"no":"type"}"#), None);
        assert_eq!(parse_line(r#"{"type":"inserted"}"#), None, "no id");
    }

    #[test]
    fn a_line_ending_is_allowed() {
        assert_eq!(
            parse_line("{\"type\":\"inactive\",\"client\":\"x\"}\r\n"),
            Some(FromInputMethod::Inactive {
                client: Some("x".into())
            })
        );
    }

    #[test]
    fn insert_as_in_the_protocol() {
        assert_eq!(
            to_line(&ToInputMethod::Insert {
                id: 7,
                text: "…".into()
            }),
            "{\"type\":\"insert\",\"id\":7,\"text\":\"…\"}\n"
        );
    }

    #[test]
    fn layout_and_restore_as_in_the_protocol() {
        assert_eq!(
            to_line(&ToInputMethod::Layout {
                source: Some("com.apple.keylayout.German".into())
            }),
            "{\"type\":\"layout\",\"source\":\"com.apple.keylayout.German\"}\n"
        );
        assert_eq!(
            to_line(&ToInputMethod::Restore {
                source: Some("com.apple.keylayout.German".into())
            }),
            "{\"type\":\"restore\",\"source\":\"com.apple.keylayout.German\"}\n"
        );
    }

    #[test]
    fn cleared_layout_and_restore_say_null() {
        assert_eq!(
            to_line(&ToInputMethod::Layout { source: None }),
            "{\"type\":\"layout\",\"source\":null}\n"
        );
        assert_eq!(
            to_line(&ToInputMethod::Restore { source: None }),
            "{\"type\":\"restore\",\"source\":null}\n"
        );
    }

    #[test]
    fn catch_release_and_quit_as_in_the_protocol() {
        assert_eq!(
            to_line(&ToInputMethod::Catch {
                id: 9,
                keys: vec![CatchKey { code: 36, mods: 0 }],
                hold_ms: Some(1500)
            }),
            "{\"type\":\"catch\",\"id\":9,\"keys\":[{\"code\":36,\"mods\":0}],\"hold_ms\":1500}\n"
        );
        assert_eq!(
            to_line(&ToInputMethod::Catch {
                id: 9,
                keys: vec![],
                hold_ms: None
            }),
            "{\"type\":\"catch\",\"id\":9,\"keys\":[]}\n"
        );
        assert_eq!(
            to_line(&ToInputMethod::Release {
                id: 9,
                text: "…".into()
            }),
            "{\"type\":\"release\",\"id\":9,\"text\":\"…\"}\n"
        );
        assert_eq!(to_line(&ToInputMethod::Quit), "{\"type\":\"quit\"}\n");
    }

    #[test]
    fn a_text_with_line_breaks_stays_one_line() {
        let line = to_line(&ToInputMethod::Insert {
            id: 1,
            text: "first\nsecond\r\nthird\u{2028}".into(),
        });
        assert_eq!(line.matches('\n').count(), 1);
        assert!(line.ends_with('\n'));
        let back: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(back["text"], "first\nsecond\r\nthird\u{2028}");
    }
}
