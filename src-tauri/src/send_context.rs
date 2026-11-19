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
//! Profile by Application" (`app_profiles`): in the tray, in the sidebar and
//! on the Context settings, which turn over the active profile's value, and
//! with its own shortcut, which turns over the value of the profile a
//! dictation into the application in front would use (user, 2026-11-15:
//! "the alternative").

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::settings::{self, AppSettings};
use crate::tray::TrayIconState;

/// The shortcut that turns over the switch of the profile a dictation into
/// the application in front would use.
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

/// The state of profile `id`'s switch (or the defaults'): for what is in
/// use the flat field, which holds its latest values; `None` without such a
/// profile.
pub fn switch_of(settings: &AppSettings, id: &str) -> Option<bool> {
    crate::profiles::values_of(settings, id).map(|values| values.send_context)
}

/// Sets profile `id`'s switch (or the defaults') to what `change` makes of
/// its state: the flat field for what is in use (storing sends it where it
/// belongs), else it goes where that profile's value comes from
/// (`profile_layers::route`). Returns the new state; `None` without such a
/// profile.
fn change_switch(
    settings: &mut AppSettings,
    id: &str,
    change: impl FnOnce(bool) -> bool,
) -> Option<bool> {
    if id == settings.active_profile_id {
        settings.send_context = change(settings.send_context);
        return Some(settings.send_context);
    }
    let mut values = crate::profiles::values_of(settings, id)?;
    values.send_context = change(values.send_context);
    crate::profile_layers::route(settings, id, &values);
    Some(values.send_context)
}

/// Turns profile `id`'s switch over; returns the new state, `None` without
/// such a profile.
pub fn toggle_profile(settings: &mut AppSettings, id: &str) -> Option<bool> {
    change_switch(settings, id, |on| !on)
}

/// The profile whose switch the shortcut turns over while nothing records:
/// the one a dictation into `frontmost_app` would use now, chosen by
/// `app_profiles::dictation_profile` just as a dictation's start chooses it
/// (the application's own profile while choosing by application is on,
/// else the active one). The shortcut is pressed where one dictates, so it
/// changes what the next dictation there does.
pub fn shortcut_profile(settings: &AppSettings, frontmost_app: Option<&str>) -> String {
    crate::app_profiles::dictation_profile(settings, frontmost_app).map_or_else(
        || settings.active_profile_id.clone(),
        |profile| profile.id.clone(),
    )
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

/// The notice after turning profile `id`'s switch over (`settings` hold its
/// new state): that profile's name and state; `default_name` is the
/// translated name of the default profile.
pub fn toggle_notice(
    settings: &AppSettings,
    id: &str,
    default_name: &str,
) -> Option<ContextNotice> {
    Some(ContextNotice {
        name: crate::profiles::name_of(settings, id, default_name)?,
        sends: switch_of(settings, id)?,
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
/// now, so turning the switch over while recording still counts (for an
/// application's profile through `app_profiles::carry_send_context`). The
/// pills that follow show the decision.
pub fn decide_at_stop(app: &AppHandle, post_process: bool) -> bool {
    let sends = sends_context(post_process, &crate::app_profiles::dictation_settings(app));
    if sends {
        log::info!("This dictation sends earlier dictations as context");
    }
    crate::overlay::set_dictation_context(app, sends);
    sends
}

/// Stores the switch of profile `profile` (`None`: the active one's) as
/// `change` sets it and tells the settings window, the tray and the overlay:
/// a recording with that profile follows it, and the recording's pill shows
/// its decision; else a notice from the shortcut or the tray (`notice`)
/// names the profile and the state.
fn set(
    app: &AppHandle,
    profile: Option<String>,
    notice: bool,
    change: impl FnOnce(&mut AppSettings, &str) -> Option<bool>,
) {
    let mut settings = settings::get_settings(app);
    let id = profile.unwrap_or_else(|| settings.active_profile_id.clone());
    let Some(on) = change(&mut settings, &id) else {
        log::warn!("Send Context not switched: no profile {id}");
        return;
    };
    log::info!(
        "Send Context is {} for profile {id}",
        if on { "on" } else { "off" }
    );
    settings::write_settings(app, settings.clone());
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({ "setting": "send_context" }),
    );
    crate::tray::rebuild_tray_menu(app);
    if crate::tray::dictation_phase(app) == TrayIconState::Recording {
        // A recording with an application's profile holds that profile's
        // values from its start; with the active profile it reads the stored
        // switch anyway.
        crate::app_profiles::carry_send_context_into_dictation(&id, on);
        let post_process = RECORDING_POST_PROCESS.load(Ordering::SeqCst);
        crate::overlay::set_dictation_context(app, dictation_sends_context(app, post_process));
    } else if notice && !crate::overlay::is_session_showing() {
        if let Some(shown) = toggle_notice(
            &settings,
            &id,
            &crate::profiles::default_profile_name(&settings),
        ) {
            crate::overlay::show_context_notice(app, shown, TOGGLE_NOTICE);
        }
    }
}

/// The shortcut's action, pressed where one dictates: it turns over the
/// switch of the profile a dictation into the application in front would use
/// now (`shortcut_profile`, the application looked up as a dictation's start
/// looks it up), and the notice names that profile. While a recording runs,
/// it turns over the recording's own profile, which the pill shows and the
/// recording follows; that is the profile the application in front got when
/// the recording started. Runs off the shortcut thread (see
/// `ToggleSendContextAction`).
pub fn toggle_from_shortcut(app: &AppHandle) {
    let profile = if crate::tray::dictation_phase(app) == TrayIconState::Recording {
        crate::app_profiles::dictation_profile_id()
    } else {
        let settings = settings::get_settings(app);
        let frontmost = crate::app_profiles::frontmost_if_choosing(&settings);
        Some(shortcut_profile(&settings, frontmost.as_deref()))
    };
    set(app, profile, true, toggle_profile);
}

/// The tray's check item: the active profile's switch, like the sidebar's.
pub fn toggle_from_tray(app: &AppHandle) {
    set(app, None, true, toggle_profile);
}

/// The switch in the sidebar and on the Context settings: the active
/// profile's.
#[tauri::command]
#[specta::specta]
pub fn set_send_context(app: AppHandle, enabled: bool) {
    set(&app, None, false, |settings, id| {
        change_switch(settings, id, |_| enabled)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::{ensure_profiles, switch_to, DEFAULT_PROFILE_ID};
    use crate::settings::{get_default_settings, registers_binding, ShortcutBinding};

    /// The defaults (in use, post-processing on, switch off) and "work"
    /// (switch on, its own value) assigned to Mail.
    fn two_profiles() -> AppSettings {
        let mut s = AppSettings {
            post_process_enabled: true,
            ..Default::default()
        };
        ensure_profiles(&mut s);
        let mut work = crate::profiles::profile_for_test(&s, "work", "Work", |values| {
            values.send_context = true;
        });
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
        assert_eq!(toggle_profile(&mut s, DEFAULT_PROFILE_ID), Some(true));
        assert!(s.send_context);
        crate::profile_layers::route_flat(&mut s);
        assert!(crate::profiles::default_values(&s).send_context);
        assert_eq!(toggle_profile(&mut s, DEFAULT_PROFILE_ID), Some(false));
        crate::profile_layers::route_flat(&mut s);
        assert!(!crate::profiles::default_values(&s).send_context);
        // The other profile keeps its own.
        assert!(s.profiles[0].values.send_context);
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
            "ctrl+shift+x"
        } else {
            "alt+shift+x"
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
        toggle_profile(&mut s, DEFAULT_PROFILE_ID);
        assert_eq!(tray_check(&s), Some(true));
        s.post_process_enabled = false;
        assert_eq!(tray_check(&s), None);
    }

    #[test]
    fn the_overlay_names_the_active_profile_with_the_new_state() {
        let mut s = two_profiles();
        toggle_profile(&mut s, DEFAULT_PROFILE_ID);
        assert_eq!(
            toggle_notice(&s, DEFAULT_PROFILE_ID, "Standard"),
            Some(ContextNotice {
                name: "Standard".to_string(),
                sends: true,
            })
        );
        switch_to(&mut s, "work", false).expect("switched");
        toggle_profile(&mut s, "work");
        assert_eq!(
            toggle_notice(&s, "work", "Standard"),
            Some(ContextNotice {
                name: "Work".to_string(),
                sends: false,
            })
        );
        // With a single profile too.
        s.profiles.retain(|profile| profile.id == "work");
        assert!(toggle_notice(&s, "work", "Standard").is_some());
    }

    #[test]
    fn the_shortcut_turns_over_the_switch_of_the_profile_the_app_in_front_gets() {
        let mut s = two_profiles();
        let id = shortcut_profile(&s, Some("com.apple.mail"));
        assert_eq!(id, "work");
        assert_eq!(toggle_profile(&mut s, &id), Some(false));
        assert!(!s.profiles[0].values.send_context);
        // The active profile keeps its own, also once the settings are stored.
        assert!(!s.send_context);
        crate::profile_layers::route_flat(&mut s);
        assert!(!crate::profiles::default_values(&s).send_context);
        assert!(!s.profiles[0].values.send_context);
        // And the next dictation into Mail follows it.
        let profile =
            crate::app_profiles::dictation_profile(&s, Some("com.apple.mail")).expect("work");
        let effective = crate::app_profiles::with_values(&s, Some(&profile.values));
        assert!(!sends_context(true, &effective));
    }

    #[test]
    fn turning_over_another_profiles_switch_leaves_the_active_ones() {
        let mut s = two_profiles();
        assert_eq!(toggle_profile(&mut s, "work"), Some(false));
        assert!(!s.send_context);
        assert_eq!(toggle_profile(&mut s, "work"), Some(true));
        assert!(s.profiles[0].values.send_context);
        assert!(!s.send_context);
        // The active profile's own is the flat field, which storing copies
        // into the profile.
        assert_eq!(toggle_profile(&mut s, DEFAULT_PROFILE_ID), Some(true));
        assert!(s.send_context);
        crate::profile_layers::route_flat(&mut s);
        assert!(crate::profiles::default_values(&s).send_context);
        // No such profile: nothing changes.
        assert_eq!(toggle_profile(&mut s, "nope"), None);
        assert!(s.send_context);
        assert!(s.profiles[0].values.send_context);
    }

    #[test]
    fn elsewhere_and_while_choosing_by_app_is_off_the_shortcut_turns_over_the_active_profile() {
        let mut s = two_profiles();
        assert_eq!(
            shortcut_profile(&s, Some("com.apple.TextEdit")),
            DEFAULT_PROFILE_ID
        );
        assert_eq!(shortcut_profile(&s, None), DEFAULT_PROFILE_ID);
        crate::app_profiles::toggle_auto(&mut s);
        assert_eq!(
            shortcut_profile(&s, Some("com.apple.mail")),
            DEFAULT_PROFILE_ID
        );
        crate::app_profiles::toggle_auto(&mut s);
        // With the assigned profile active, the active one either way.
        switch_to(&mut s, "work", false).expect("switched");
        assert_eq!(shortcut_profile(&s, Some("com.apple.mail")), "work");
        assert_eq!(shortcut_profile(&s, Some("com.apple.TextEdit")), "work");
        switch_to(&mut s, DEFAULT_PROFILE_ID, false).expect("switched back");
        // Without any assignment.
        crate::app_profiles::set_apps(&mut s, "work", Vec::new()).expect("unassigned");
        assert_eq!(
            shortcut_profile(&s, Some("com.apple.mail")),
            DEFAULT_PROFILE_ID
        );
    }

    #[test]
    fn the_overlay_names_the_profile_whose_switch_was_turned_over() {
        let mut s = two_profiles();
        toggle_profile(&mut s, "work");
        assert_eq!(
            toggle_notice(&s, "work", "Standard"),
            Some(ContextNotice {
                name: "Work".to_string(),
                sends: false,
            })
        );
        // The active profile with its latest state, not yet stored into it.
        s.send_context = true;
        assert_eq!(
            toggle_notice(&s, DEFAULT_PROFILE_ID, "Standard"),
            Some(ContextNotice {
                name: "Standard".to_string(),
                sends: true,
            })
        );
        assert_eq!(toggle_notice(&s, "nope", "Standard"), None);
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
