//! DanyWhere: the switch "Send Context". A dictation with post-processing
//! sends the earlier dictations of the same window along as context
//! (`dictation_context`) while its profile's switch is on; a dictation
//! without post-processing never does (user, 2026-11-15: "Ohne
//! Nachbearbeitung: nie Kontext"). It replaces the shortcut "Transcribe with
//! Context" and its macOS service "End Dictation with Context and Insert".
//!
//! The switch is a profile value (`AppSettings::send_context`, mirrored in
//! `ProfileValues`), so a dictation into an application with a profile of
//! its own follows that profile's switch. It is turned over like "Choose
//! Profile by Application" (`app_profiles`): in the tray, in the sidebar, on
//! the Context settings, and with its own shortcut; each turns over the
//! active profile's value.

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::settings::{self, AppSettings};
use crate::tray::TrayIconState;

/// The shortcut that turns the active profile's switch over.
pub const TOGGLE_BINDING: &str = "toggle_send_context";

/// The shortcut that dictated with context up to 0.1.6; dropped from stored
/// settings.
pub const RETIRED_BINDING: &str = "transcribe_with_context";

/// The message of the macOS service that ended a dictation with context up
/// to 0.1.6.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const RETIRED_SERVICE_MESSAGE: &str = "endDictationWithContextAndInsert";

/// How long the overlay shows the switch's new state.
const TOGGLE_NOTICE: std::time::Duration = std::time::Duration::from_millis(2000);

/// Whether a dictation sends earlier dictations as context: only with
/// post-processing, and only while the switch of its profile (`settings`
/// hold its values) is on.
pub fn sends_context(post_process: bool, settings: &AppSettings) -> bool {
    post_process && settings.send_context
}

/// Turns the active profile's switch over; returns the new state.
pub fn toggle(settings: &mut AppSettings) -> bool {
    settings.send_context = !settings.send_context;
    settings.send_context
}

/// Drops the retired context shortcut from stored settings; returns whether
/// it was there.
pub fn drop_retired_binding(settings: &mut AppSettings) -> bool {
    settings.bindings.remove(RETIRED_BINDING).is_some()
}

/// The tray's check item: the active profile's state while it
/// post-processes, otherwise none (the switch would do nothing).
pub fn tray_check(settings: &AppSettings) -> Option<bool> {
    settings
        .post_process_enabled
        .then_some(settings.send_context)
}

/// What the overlay shows after the switch was turned over: the profile
/// whose switch it is and its new state.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct ContextNotice {
    pub name: String,
    pub sends: bool,
}

/// The notice after turning the switch over (`settings` hold its new state):
/// the active profile's name; `default_name` is the translated name of the
/// default profile.
pub fn toggle_notice(settings: &AppSettings, default_name: &str) -> Option<ContextNotice> {
    let profile = crate::profiles::active_profile(settings)?;
    Some(ContextNotice {
        name: crate::profiles::display_name(profile, default_name),
        sends: settings.send_context,
    })
}

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

/// Whether the recording in progress was started to post-process, so that
/// turning the switch over while it records shows in its pill.
static RECORDING_POST_PROCESS: AtomicBool = AtomicBool::new(false);

/// Whether the dictation that starts now, `post_process` or not, would send
/// context with its profile's switch as it stands.
fn dictation_sends_context(app: &AppHandle, post_process: bool) -> bool {
    let post_process = crate::app_profiles::effective_post_process(
        post_process,
        crate::app_profiles::dictation_values().as_ref(),
    );
    sends_context(post_process, &crate::app_profiles::dictation_settings(app))
}

/// A recording starts (after `app_profiles::begin_dictation`): its pill
/// shows the bubble while it would send context.
pub fn begin_dictation(app: &AppHandle, post_process: bool) {
    RECORDING_POST_PROCESS.store(post_process, Ordering::SeqCst);
    crate::overlay::set_dictation_context(app, dictation_sends_context(app, post_process));
}

/// The recording ends with or without post-processing (`post_process`, as
/// the dictation's profile decided it): whether it sends context, decided
/// now, so turning the switch over while recording still counts. The pills
/// that follow show the decision.
pub fn decide_at_stop(app: &AppHandle, post_process: bool) -> bool {
    let sends = sends_context(post_process, &crate::app_profiles::dictation_settings(app));
    if sends {
        log::info!("This dictation sends earlier dictations as context");
    }
    crate::overlay::set_dictation_context(app, sends);
    sends
}

/// Stores the switch as `change` sets it and tells the settings window, the
/// tray and the overlay: a recording's pill shows the new decision, else a
/// notice from the shortcut or the tray (`notice`) names the profile and the
/// state. Returns the new state.
fn set(app: &AppHandle, notice: bool, change: impl FnOnce(&mut AppSettings) -> bool) -> bool {
    let mut settings = settings::get_settings(app);
    let on = change(&mut settings);
    log::info!(
        "Send Context is {} for profile {}",
        if on { "on" } else { "off" },
        settings.active_profile_id
    );
    settings::write_settings(app, settings.clone());
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({ "setting": "send_context" }),
    );
    crate::tray::rebuild_tray_menu(app);
    if crate::tray::dictation_phase(app) == TrayIconState::Recording {
        let post_process = RECORDING_POST_PROCESS.load(Ordering::SeqCst);
        crate::overlay::set_dictation_context(app, dictation_sends_context(app, post_process));
    } else if notice && !crate::overlay::is_session_showing() {
        if let Some(shown) =
            toggle_notice(&settings, &crate::profiles::default_profile_name(&settings))
        {
            crate::overlay::show_context_notice(app, shown, TOGGLE_NOTICE);
        }
    }
    on
}

/// The shortcut's action. Runs off the shortcut thread (see
/// `ToggleSendContextAction`).
pub fn toggle_from_shortcut(app: &AppHandle) {
    set(app, true, toggle);
}

/// The tray's check item.
pub fn toggle_from_tray(app: &AppHandle) {
    set(app, true, toggle);
}

/// The switch in the sidebar and on the Context settings.
#[tauri::command]
#[specta::specta]
pub fn set_send_context(app: AppHandle, enabled: bool) {
    set(&app, false, |settings| {
        settings.send_context = enabled;
        enabled
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::{ensure_profiles, switch_to, sync_active_profile, DEFAULT_PROFILE_ID};
    use crate::settings::{get_default_settings, registers_binding, ShortcutBinding};

    /// The default profile (active, post-processing on, switch off) and
    /// "work" (switch on) assigned to Mail.
    fn two_profiles() -> AppSettings {
        let mut s = AppSettings {
            post_process_enabled: true,
            ..Default::default()
        };
        ensure_profiles(&mut s);
        let mut work = s.profiles[0].clone();
        work.id = "work".to_string();
        work.name = "Work".to_string();
        work.values.send_context = true;
        work.apps = vec!["com.apple.mail".to_string()];
        s.profiles.push(work);
        s
    }

    #[test]
    fn context_goes_along_only_with_post_processing_and_the_switch_on() {
        let mut s = AppSettings {
            send_context: true,
            ..Default::default()
        };
        assert!(sends_context(true, &s));
        assert!(!sends_context(false, &s));
        s.send_context = false;
        assert!(!sends_context(true, &s));
        assert!(!sends_context(false, &s));
    }

    #[test]
    fn a_new_installation_starts_with_the_switch_off() {
        assert!(!get_default_settings().send_context);
        assert!(!crate::profiles::ProfileValues::default().send_context);
    }

    #[test]
    fn toggling_turns_over_the_active_profiles_switch_only() {
        let mut s = two_profiles();
        assert!(toggle(&mut s));
        assert!(s.send_context);
        sync_active_profile(&mut s);
        assert!(s.profiles[0].values.send_context);
        assert!(!toggle(&mut s));
        sync_active_profile(&mut s);
        assert!(!s.profiles[0].values.send_context);
        // The other profile keeps its own.
        assert!(s.profiles[1].values.send_context);
    }

    #[test]
    fn switching_profiles_shows_the_new_profiles_switch() {
        let mut s = two_profiles();
        assert!(!s.send_context);
        switch_to(&mut s, "work", false).expect("switched");
        assert!(s.send_context);
        switch_to(&mut s, DEFAULT_PROFILE_ID, false).expect("switched back");
        assert!(!s.send_context);
    }

    #[test]
    fn a_dictation_into_an_apps_profile_follows_that_profiles_switch() {
        let s = two_profiles();
        let profile =
            crate::app_profiles::dictation_profile(&s, Some("com.apple.mail")).expect("work");
        let effective = crate::app_profiles::with_values(&s, Some(&profile.values));
        assert!(sends_context(true, &effective));
        // Elsewhere the active profile's switch, which is off.
        assert!(!sends_context(true, &s));
    }

    fn retired_binding() -> ShortcutBinding {
        ShortcutBinding {
            id: RETIRED_BINDING.to_string(),
            name: "Transcribe with Context".to_string(),
            description: "Converts your speech into text and post-processes it with earlier dictations from the same window as context.".to_string(),
            default_binding: "ctrl+shift+command+space".to_string(),
            current_binding: "ctrl+shift+command+space".to_string(),
        }
    }

    #[test]
    fn stored_settings_with_the_context_shortcut_lose_it() {
        let mut stored = serde_json::to_value(get_default_settings()).unwrap();
        stored["bindings"][RETIRED_BINDING] = serde_json::to_value(retired_binding()).unwrap();
        let mut s: AppSettings = serde_json::from_value(stored).expect("old settings load");
        assert!(drop_retired_binding(&mut s));
        assert!(!s.bindings.contains_key(RETIRED_BINDING));
        assert!(!drop_retired_binding(&mut s));
        assert!(s.bindings.contains_key("transcribe_with_post_process"));
    }

    #[test]
    fn the_context_shortcut_is_no_default_any_more() {
        assert!(!get_default_settings()
            .bindings
            .contains_key(RETIRED_BINDING));
        assert!(!crate::actions::ACTION_MAP.contains_key(RETIRED_BINDING));
    }

    #[test]
    fn the_toggle_has_a_default_shortcut_that_no_other_has() {
        let defaults = get_default_settings();
        let binding = defaults.bindings.get(TOGGLE_BINDING).expect("a default");
        let expected = if cfg!(target_os = "macos") {
            "ctrl+shift+k"
        } else {
            "alt+shift+k"
        };
        assert_eq!(binding.default_binding, expected);
        assert_eq!(binding.current_binding, expected);
        assert_eq!(binding.name, "Send Context");
        let others = defaults
            .bindings
            .values()
            .filter(|b| b.id != TOGGLE_BINDING && b.default_binding == expected)
            .count();
        assert_eq!(others, 0, "{expected} is taken");
        assert!(crate::actions::ACTION_MAP.contains_key(TOGGLE_BINDING));
    }

    #[test]
    fn the_toggle_is_registered_only_while_post_processing_is_reachable() {
        let mut s = two_profiles();
        assert!(registers_binding(TOGGLE_BINDING, &s, true));
        s.post_process_enabled = false;
        // A profile with applications still post-processes.
        assert!(registers_binding(TOGGLE_BINDING, &s, true));
        crate::app_profiles::toggle_auto(&mut s);
        assert!(!registers_binding(TOGGLE_BINDING, &s, true));
    }

    #[test]
    fn the_tray_offers_the_switch_while_the_active_profile_post_processes() {
        let mut s = two_profiles();
        assert_eq!(tray_check(&s), Some(false));
        toggle(&mut s);
        assert_eq!(tray_check(&s), Some(true));
        s.post_process_enabled = false;
        assert_eq!(tray_check(&s), None);
    }

    #[test]
    fn the_overlay_names_the_active_profile_with_the_new_state() {
        let mut s = two_profiles();
        toggle(&mut s);
        assert_eq!(
            toggle_notice(&s, "Standard"),
            Some(ContextNotice {
                name: "Standard".to_string(),
                sends: true,
            })
        );
        switch_to(&mut s, "work", false).expect("switched");
        toggle(&mut s);
        assert_eq!(
            toggle_notice(&s, "Standard"),
            Some(ContextNotice {
                name: "Work".to_string(),
                sends: false,
            })
        );
        // With a single profile too.
        s.profiles.retain(|profile| profile.id == "work");
        assert!(toggle_notice(&s, "Standard").is_some());
    }

    #[test]
    fn the_overlay_reads_the_notice_as_name_and_state() {
        assert_eq!(
            serde_json::to_value(ContextNotice {
                name: "Work".to_string(),
                sends: true,
            })
            .unwrap(),
            serde_json::json!({ "name": "Work", "sends": true })
        );
    }

    #[test]
    fn the_switch_travels_with_an_exported_profile() {
        let mut s = two_profiles();
        // Not yet copied into the active profile.
        s.send_context = true;
        let yaml = crate::profile_transfer::build_export(&s, DEFAULT_PROFILE_ID, "Standard", None)
            .expect("export");
        let (profile, _) = crate::profile_transfer::read_import(&yaml, None).expect("import");
        assert!(profile.values.send_context);
    }

    #[test]
    fn an_export_from_before_the_switch_imports_with_it_off() {
        let s = two_profiles();
        let yaml =
            crate::profile_transfer::build_export(&s, "work", "Standard", None).expect("export");
        let line = yaml
            .lines()
            .find(|line| line.trim_start().starts_with("send_context:"))
            .expect("the export names the switch")
            .to_string();
        let older = yaml.replace(&format!("{line}\n"), "");
        assert!(!older.contains("send_context"), "{older}");
        let (profile, _) = crate::profile_transfer::read_import(&older, None).expect("import");
        assert!(!profile.values.send_context);
        assert_eq!(profile.name, "Work");
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
