//! DanyWhere: context from earlier dictations. A dictation with
//! post-processing sends the earlier dictations of the same window along as
//! context (`dictation_context`) when the end shortcut it ended with says so
//! (`end_keys`, user, 2026-11-19, F6); a dictation without post-processing
//! never does (user, 2026-11-15: "Ohne Nachbearbeitung: nie Kontext").
//!
//! History: up to 0.1.6 the shortcut "Transcribe with Context" and its macOS
//! service "End Dictation with Context and Insert" sent context; from 0.2.0
//! to 0.2.5 the profile's switch "Send Context" did, turned over with its own
//! shortcut, in the tray, in the sidebar and on the post-processing page.
//! Both shortcuts are dropped from stored settings (`end_keys::migrate`); the
//! old service's entry in the services settings is cleared
//! (`insert_service`). What stays here is the speech bubble the overlay
//! shows while a dictation would send context.

/// The shortcut that dictated with context up to 0.1.6; dropped from stored
/// settings.
pub const RETIRED_BINDING: &str = "transcribe_with_context";

/// The shortcut that turned the switch over from 0.2.0 to 0.2.5; dropped
/// from stored settings (`end_keys::migrate`), as the context hangs on the
/// end shortcut now.
pub const RETIRED_TOGGLE_BINDING: &str = "toggle_send_context";

/// The message of the macOS service that ended a dictation with context up
/// to 0.1.6.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const RETIRED_SERVICE_MESSAGE: &str = "endDictationWithContextAndInsert";

/// The menu title "Insert Last Dictation" ends with in every build.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const INSERT_TITLE_SUFFIX: &str = "Insert Last Dictation";

/// The menu title the retired context service had after the build's prefix.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const RETIRED_SERVICE_TITLE: &str = "End Dictation with Context and Insert";

/// The `NSServicesStatus` key of the retired context service, from the
/// menu title of "Insert Last Dictation" (`insert_title`), which carries the
/// same prefix in every build ("DanyWhere: ", "DanyWhere Dev: "). Only macOS
/// has services.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn retired_service_status_key(bundle_id: &str, insert_title: &str) -> Option<String> {
    let prefix = insert_title.strip_suffix(INSERT_TITLE_SUFFIX)?;
    Some(crate::insert_service::status_key(
        bundle_id,
        &format!("{prefix}{RETIRED_SERVICE_TITLE}"),
        RETIRED_SERVICE_MESSAGE,
    ))
}

/// A recording starts: its pill shows the speech bubble while the end it
/// would take now (its start shortcut's default end) sends context
/// (`sends`).
pub fn begin_dictation(app: &tauri::AppHandle, sends: bool) {
    crate::overlay::set_dictation_context(app, sends);
}

/// The recording ends: whether it sends context (`sends`, decided by the
/// end shortcut and post-processing) goes into the log and the pills that
/// follow. Returns `sends`.
pub fn decide_at_stop(app: &tauri::AppHandle, sends: bool) -> bool {
    if sends {
        log::info!("This dictation sends earlier dictations as context");
    }
    crate::overlay::set_dictation_context(app, sends);
    sends
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::get_default_settings;

    #[test]
    fn neither_retired_shortcut_is_a_default_any_more() {
        let defaults = get_default_settings().bindings;
        for id in [RETIRED_BINDING, RETIRED_TOGGLE_BINDING] {
            assert!(!defaults.contains_key(id), "{id}");
            assert!(!crate::actions::ACTION_MAP.contains_key(id), "{id}");
        }
    }

    #[test]
    fn the_retired_services_entry_is_found_by_the_insert_services_title() {
        assert_eq!(
            retired_service_status_key(
                "com.pais.handy.dev",
                "DanyWhere Dev: Insert Last Dictation"
            )
            .as_deref(),
            Some(
                "com.pais.handy.dev - DanyWhere Dev: End Dictation with Context and Insert - endDictationWithContextAndInsert"
            )
        );
        assert_eq!(
            retired_service_status_key("com.yaccob.danywhere", "DanyWhere: Insert Last Dictation")
                .as_deref(),
            Some(
                "com.yaccob.danywhere - DanyWhere: End Dictation with Context and Insert - endDictationWithContextAndInsert"
            )
        );
        assert_eq!(
            retired_service_status_key("com.yaccob.danywhere", "Something Else"),
            None
        );
    }
}
