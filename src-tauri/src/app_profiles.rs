//! DanyWhere: profiles per application. A profile can be assigned
//! applications; while "choose the profile by application" is on, a dictation
//! into one of them uses that profile's values, without switching what is in
//! use. What is chosen in the sidebar and the tray (a profile, or the
//! defaults, which take no applications) applies to every other application.
//!
//! The values of such a dictation are held here from the start of its
//! recording until its pipeline finishes or is cancelled; the code on the
//! dictation path reads them through [`dictation_settings`]. The profile is
//! chosen when the recording starts, so its model loads and the pills name
//! it, and again when it ends ([`settle_dictation`]), by the application in
//! front then, which is where its text goes (user, 2026-11-16). Where the
//! text is inserted later, `profile_redo` looks once more.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Emitter};

use crate::profiles::{Profile, ProfileValues};
use crate::settings::{self, AppSettings};

/// The shortcut that turns choosing the profile by application on and off.
pub const TOGGLE_BINDING: &str = "toggle_profile_auto";

/// How long the overlay shows the switch's new state and profile.
const TOGGLE_NOTICE: std::time::Duration = std::time::Duration::from_millis(2000);

/// An application that can be assigned to a profile.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct ApplicationInfo {
    /// Bundle id on macOS, executable name on Windows.
    pub id: String,
    pub name: String,
    pub running: bool,
}

/// The id of the profile a dictation into `frontmost_app` uses: the profile
/// the application is assigned to while `auto_by_app` is on, otherwise the
/// active one.
pub fn profile_for_dictation<'a>(
    profiles: &'a [Profile],
    active_id: &'a str,
    auto_by_app: bool,
    frontmost_app: Option<&str>,
) -> &'a str {
    if auto_by_app {
        if let Some(app) = frontmost_app {
            if let Some(profile) = profiles
                .iter()
                .find(|profile| profile.apps.iter().any(|assigned| assigned == app))
            {
                return &profile.id;
            }
        }
    }
    active_id
}

/// Whether any profile has applications assigned.
pub fn has_assignments(settings: &AppSettings) -> bool {
    settings
        .profiles
        .iter()
        .any(|profile| !profile.apps.is_empty())
}

/// Whether choosing by application is switched off while it would do
/// something: the tray marks its idle icon then.
pub fn manual_mode(settings: &AppSettings) -> bool {
    !settings.profile_auto_by_app && has_assignments(settings)
}

/// Whether some dictation can post-process: the active profile does, or,
/// while choosing by application is on, a profile with applications does.
/// The post-processing shortcuts are registered only then.
pub fn post_processing_reachable(settings: &AppSettings) -> bool {
    // The flat fields hold the active profile's latest values.
    settings.post_process_enabled
        || (settings.profile_auto_by_app
            && settings.profiles.iter().any(|profile| {
                profile.id != settings.active_profile_id
                    && !profile.apps.is_empty()
                    && profile.values.post_process_enabled
            }))
}

/// What the overlay shows about choosing the profile by application, as a
/// glyph before the profile's name: nothing while no application is assigned
/// (the switch does nothing then), a window while it is on, the tray's padlock
/// while it is off.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProfileMode {
    None,
    Auto,
    Lock,
}

/// A profile as the overlay shows it: its name and the state of the switch.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct OverlayProfile {
    pub name: String,
    pub mode: ProfileMode,
}

/// The state of the switch as the overlay shows it; the padlock under the
/// same condition as the tray's ([`manual_mode`]).
pub fn profile_mode(settings: &AppSettings) -> ProfileMode {
    if !has_assignments(settings) {
        ProfileMode::None
    } else if settings.profile_auto_by_app {
        ProfileMode::Auto
    } else {
        ProfileMode::Lock
    }
}

/// Whether a dictation's pills name the defaults while they apply. Open
/// question 2 of the build plan, recommendation a: they don't, the pill
/// stays short and names a profile only when one applies; the tray lists
/// the defaults first and a switch to them names them. One line to change.
pub const PILLS_NAME_THE_DEFAULTS: bool = false;

/// Profile `id` (or the defaults) as the overlay shows it; `default_name`
/// is the translated name of the defaults.
pub fn overlay_profile(
    settings: &AppSettings,
    id: &str,
    default_name: &str,
) -> Option<OverlayProfile> {
    Some(OverlayProfile {
        name: crate::profiles::name_of(settings, id, default_name)?,
        mode: profile_mode(settings),
    })
}

/// The profile a dictation's pills name: `id`, the one the dictation runs
/// with, whether an assignment chose it or not. `None` without profiles,
/// where the pills stay as they were, and while the defaults apply
/// ([`PILLS_NAME_THE_DEFAULTS`]).
pub fn dictation_overlay_profile(
    settings: &AppSettings,
    id: &str,
    default_name: &str,
) -> Option<OverlayProfile> {
    if settings.profiles.is_empty()
        || (id == crate::profiles::DEFAULT_PROFILE_ID && !PILLS_NAME_THE_DEFAULTS)
    {
        return None;
    }
    overlay_profile(settings, id, default_name)
}

/// What the overlay says after the switch was turned over (`settings` hold
/// its new state): the profile a dictation into `frontmost_app` gets now,
/// its own while the switch is on, otherwise the active one.
pub fn toggle_notice_profile(
    settings: &AppSettings,
    frontmost_app: Option<&str>,
    default_name: &str,
) -> Option<OverlayProfile> {
    let id = profile_for_dictation(
        &settings.profiles,
        &settings.active_profile_id,
        settings.profile_auto_by_app,
        frontmost_app,
    );
    overlay_profile(settings, id, default_name)
}

/// Assigns `apps` to profile `id`, each at most once and to no other profile.
pub fn set_apps(settings: &mut AppSettings, id: &str, apps: Vec<String>) -> Result<(), String> {
    let index = settings
        .profiles
        .iter()
        .position(|profile| profile.id == id)
        .ok_or_else(|| format!("Profile '{id}' not found"))?;
    let mut clean: Vec<String> = Vec::new();
    for app in apps {
        let app = app.trim();
        if !app.is_empty() && !clean.iter().any(|kept| kept == app) {
            clean.push(app.to_string());
        }
    }
    for (other, profile) in settings.profiles.iter_mut().enumerate() {
        if other != index {
            profile.apps.retain(|app| !clean.contains(app));
        }
    }
    settings.profiles[index].apps = clean;
    Ok(())
}

/// Turns choosing by application on or off; returns the new state.
pub fn toggle_auto(settings: &mut AppSettings) -> bool {
    settings.profile_auto_by_app = !settings.profile_auto_by_app;
    settings.profile_auto_by_app
}

/// The profile a dictation into `frontmost_app` uses when it is not the
/// active one; `None`: the active profile's values (the flat fields) apply.
pub fn dictation_profile<'a>(
    settings: &'a AppSettings,
    frontmost_app: Option<&str>,
) -> Option<&'a Profile> {
    let id = profile_for_dictation(
        &settings.profiles,
        &settings.active_profile_id,
        settings.profile_auto_by_app,
        frontmost_app,
    );
    if id == settings.active_profile_id {
        return None;
    }
    settings.profiles.iter().find(|profile| profile.id == id)
}

/// `settings` with `values` in its profile fields, if any.
pub fn with_values(settings: &AppSettings, values: Option<&ProfileValues>) -> AppSettings {
    let mut settings = settings.clone();
    if let Some(values) = values {
        values.apply_to(&mut settings);
    }
    settings
}

/// Whether a dictation that asked for post-processing (`requested`) does
/// post-process: a dictation profile other than the active one decides with
/// its own switch, since its shortcut may be registered for another profile.
pub fn effective_post_process(requested: bool, dictation_values: Option<&ProfileValues>) -> bool {
    requested && dictation_values.is_none_or(|values| values.post_process_enabled)
}

/// The applications to offer: the running ones first, then the installed
/// ones, each once, by name, without DanyWhere itself (`own_id`).
pub fn merge_applications(
    running: Vec<(String, String)>,
    installed: Vec<(String, String)>,
    own_id: &str,
) -> Vec<ApplicationInfo> {
    let mut apps: Vec<ApplicationInfo> = Vec::new();
    let mut add = |(id, name): (String, String), running: bool| {
        if id.is_empty() || id == own_id || apps.iter().any(|app| app.id == id) {
            return;
        }
        apps.push(ApplicationInfo { id, name, running });
    };
    for app in running {
        add(app, true);
    }
    for app in installed {
        add(app, false);
    }
    apps.sort_by(|a, b| {
        b.running
            .cmp(&a.running)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    apps
}

/// The profile of the dictation in progress, when it isn't the active one.
struct DictationProfile {
    id: String,
    values: ProfileValues,
}

static DICTATION_PROFILE: Mutex<Option<DictationProfile>> = Mutex::new(None);

fn dictation_lock() -> std::sync::MutexGuard<'static, Option<DictationProfile>> {
    DICTATION_PROFILE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The application in front, looked up only while it can choose the profile.
/// A dictation's start and the shortcut "Send Context" (`send_context`) both
/// look it up here and pass it to [`dictation_profile`], so the shortcut
/// turns over the switch of the profile the next dictation gets.
pub fn frontmost_if_choosing(settings: &AppSettings) -> Option<String> {
    if settings.profile_auto_by_app && has_assignments(settings) {
        crate::window_capture::frontmost_app().map(|(id, _)| id)
    } else {
        None
    }
}

/// Decides the profile of a dictation that starts now, from the application
/// in front. Returns what the dictation's pills show of it (from two
/// profiles on).
pub fn begin_dictation(app: &AppHandle) -> Option<OverlayProfile> {
    let settings = settings::get_settings(app);
    let frontmost = frontmost_if_choosing(&settings);
    let chosen = dictation_profile(&settings, frontmost.as_deref());
    let shown = dictation_overlay_profile(
        &settings,
        chosen.map_or(settings.active_profile_id.as_str(), |profile| {
            profile.id.as_str()
        }),
        &crate::profiles::default_profile_name(&settings),
    );
    let chosen = chosen.map(|profile| DictationProfile {
        id: profile.id.clone(),
        values: profile.values.clone(),
    });
    if let Some(profile) = &chosen {
        log::info!(
            "Dictation into {} uses profile {}",
            frontmost.as_deref().unwrap_or("?"),
            profile.id
        );
    }
    *dictation_lock() = chosen;
    shown
}

/// The profile a dictation that started with profile `start_id` ends with,
/// chosen by the application in front when its recording ends
/// (`frontmost_at_end`), as the user decided on 2026-11-16: that is where
/// its text goes. `None` when it stays with `start_id`: choosing by
/// application is off or does nothing, the application is unknown, or it
/// gets the same profile.
pub fn profile_at_end(
    settings: &AppSettings,
    start_id: &str,
    frontmost_at_end: Option<&str>,
) -> Option<String> {
    if !(settings.profile_auto_by_app && has_assignments(settings)) {
        return None;
    }
    let end_id = profile_for_dictation(
        &settings.profiles,
        &settings.active_profile_id,
        true,
        Some(frontmost_at_end?),
    );
    (end_id != start_id).then(|| end_id.to_string())
}

/// What changed when the recording ended in an application with another
/// profile than the one it started in.
pub struct EndChange {
    /// What the dictation's pills show of its profile from now on.
    pub shown: Option<OverlayProfile>,
    /// The transcription runs with other values (model, languages, word
    /// lists, filler words): a live decode of the recording no longer fits.
    pub transcription_differs: bool,
}

/// Chooses the dictation's profile again when its recording ends, from the
/// application in front then (the one a service call came from, without
/// Accessibility). The start's profile had its model preloaded and its
/// pills shown; from here on the end's profile applies. `None` when it
/// stays.
pub fn settle_dictation(app: &AppHandle) -> Option<EndChange> {
    let settings = settings::get_settings(app);
    if !(settings.profile_auto_by_app && has_assignments(&settings)) {
        return None;
    }
    let frontmost = crate::window_capture::frontmost_app().map(|(id, _)| id);
    let start_id = dictation_profile_id().unwrap_or_else(|| settings.active_profile_id.clone());
    let end_id = profile_at_end(&settings, &start_id, frontmost.as_deref())?;
    let end_values = crate::profile_redo::values_of(&settings, &end_id)?;
    let start_values =
        dictation_values().unwrap_or_else(|| ProfileValues::from_settings(&settings));
    let transcription_differs =
        crate::profile_redo::transcription_differs(&settings, &start_values, &end_values);
    log::info!(
        "The recording ends in {}: profile {end_id} instead of {start_id}{}",
        frontmost.as_deref().unwrap_or("?"),
        if transcription_differs {
            ", transcribed with its values"
        } else {
            ""
        }
    );
    *dictation_lock() = (end_id != settings.active_profile_id).then(|| DictationProfile {
        id: end_id.clone(),
        values: end_values,
    });
    Some(EndChange {
        shown: dictation_overlay_profile(
            &settings,
            &end_id,
            &crate::profiles::default_profile_name(&settings),
        ),
        transcription_differs,
    })
}

thread_local! {
    /// Profile values a redo (`profile_redo`) runs with on its own thread,
    /// over whatever dictation is in progress.
    static VALUES_HERE: std::cell::RefCell<Option<ProfileValues>> =
        const { std::cell::RefCell::new(None) };
}

/// Runs `run` with `values` as the dictation's values on this thread only
/// (see [`dictation_values`]): a redo for another profile reads them through
/// the same code a dictation does, while a dictation on another thread keeps
/// its own.
pub fn with_values_here<R>(values: ProfileValues, run: impl FnOnce() -> R) -> R {
    /// Puts back what this thread had before, also when `run` panics.
    struct Restore(Option<ProfileValues>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            VALUES_HERE.with(|here| *here.borrow_mut() = previous);
        }
    }
    let previous = VALUES_HERE.with(|here| here.borrow_mut().replace(values));
    let _restore = Restore(previous);
    run()
}

/// The profile the dictation in progress runs with and its values, as they
/// are now (its "Send Context" may have been turned over while recording).
pub fn dictation_profile_and_values(app: &AppHandle) -> (String, ProfileValues) {
    if let Some(profile) = dictation_lock().as_ref() {
        return (profile.id.clone(), profile.values.clone());
    }
    let settings = settings::get_settings(app);
    (
        settings.active_profile_id.clone(),
        ProfileValues::from_settings(&settings),
    )
}

/// The id of the profile the dictation in progress uses, when it isn't the
/// active one.
pub fn dictation_profile_id() -> Option<String> {
    dictation_lock().as_ref().map(|profile| profile.id.clone())
}

/// Carries profile `id`'s "Send Context", just turned over to `on`, into the
/// dictation in progress (`dictation`) when that runs with the profile, so
/// its recording follows the switch as one with the active profile does
/// (`send_context` decides when the recording stops). Its other values stay
/// as they were at its start. Returns whether it runs with that profile.
fn carry_send_context(dictation: &mut Option<DictationProfile>, id: &str, on: bool) -> bool {
    match dictation {
        Some(profile) if profile.id == id => {
            profile.values.send_context = on;
            true
        }
        _ => false,
    }
}

/// [`carry_send_context`] for the dictation in progress.
pub fn carry_send_context_into_dictation(id: &str, on: bool) {
    if carry_send_context(&mut dictation_lock(), id, on) {
        log::info!("The recording with profile {id} follows Send Context");
    }
}

/// Ends the dictation's own profile: the active profile applies again.
pub fn end_dictation() {
    *dictation_lock() = None;
}

/// The values of the dictation in progress, when its profile isn't the active
/// one; on a thread that redoes a dictation, the values it redoes it with
/// ([`with_values_here`]).
pub fn dictation_values() -> Option<ProfileValues> {
    if let Some(values) = VALUES_HERE.with(|here| here.borrow().clone()) {
        return Some(values);
    }
    dictation_lock()
        .as_ref()
        .map(|profile| profile.values.clone())
}

/// The settings a dictation reads: the stored ones, with the values of the
/// dictation's own profile over them while one applies. Never store these.
pub fn dictation_settings(app: &AppHandle) -> AppSettings {
    with_values(&settings::get_settings(app), dictation_values().as_ref())
}

/// Everything that follows a change of the assignments or of the switch:
/// shortcuts, the window, the tray menu and icon.
fn after_change(app: &AppHandle, settings: &AppSettings) {
    crate::shortcut::sync_post_process_shortcuts(app, settings);
    crate::shortcut::sync_profile_toggle_shortcut(app, settings);
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({ "setting": "profiles" }),
    );
    crate::tray::rebuild_tray_menu(app);
}

fn set_auto(app: &AppHandle, change: impl FnOnce(&mut AppSettings) -> bool) -> bool {
    let mut settings = settings::get_settings(app);
    let on = change(&mut settings);
    log::info!(
        "Choosing the profile by application is {}",
        if on { "on" } else { "off" }
    );
    settings::write_settings(app, settings.clone());
    after_change(app, &settings);
    on
}

/// The shortcut's action: switch choosing by application over and show in
/// the overlay the new state and the profile a dictation into the
/// application in front gets now. Runs off the shortcut thread (see
/// `ToggleProfileAutoAction`), and the application in front is looked up
/// outside the settings store.
pub fn toggle_from_shortcut(app: &AppHandle) {
    set_auto(app, toggle_auto);
    if crate::overlay::is_session_showing() {
        return;
    }
    let settings = settings::get_settings(app);
    let frontmost = frontmost_if_choosing(&settings);
    if let Some(shown) = toggle_notice_profile(
        &settings,
        frontmost.as_deref(),
        &crate::profiles::default_profile_name(&settings),
    ) {
        crate::overlay::show_profile_notice(app, shown, TOGGLE_NOTICE);
    }
}

/// The tray's check item.
pub fn toggle_from_tray(app: &AppHandle) {
    set_auto(app, toggle_auto);
}

#[tauri::command]
#[specta::specta]
pub fn set_profile_auto_by_app(app: AppHandle, enabled: bool) -> Result<(), String> {
    set_auto(&app, |settings| {
        settings.profile_auto_by_app = enabled;
        enabled
    });
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn set_profile_apps(app: AppHandle, id: String, apps: Vec<String>) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    set_apps(&mut settings, &id, apps)?;
    settings::write_settings(&app, settings.clone());
    after_change(&app, &settings);
    Ok(())
}

/// The applications that can be assigned, running ones first. Only macOS
/// lists them; elsewhere the list is empty and ids are typed in.
#[tauri::command]
#[specta::specta]
pub fn list_applications(app: AppHandle) -> Vec<ApplicationInfo> {
    let own_id = app.config().identifier.clone();
    merge_applications(
        crate::window_capture::running_applications(),
        crate::window_capture::installed_applications(),
        &own_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::{ensure_profiles, DEFAULT_PROFILE_ID};

    fn strings(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    /// The defaults (in use, Parakeet, no post-processing) and "mail"
    /// (Whisper, post-processing on) assigned to Mail.
    fn with_mail_profile() -> AppSettings {
        let mut settings = AppSettings {
            selected_model: "parakeet".to_string(),
            post_process_enabled: false,
            custom_words: strings(&["Sayuri"]),
            ..Default::default()
        };
        ensure_profiles(&mut settings);
        let mut mail = crate::profiles::profile_for_test(&settings, "mail", "Mail", |values| {
            values.selected_model = "whisper".to_string();
            values.post_process_enabled = true;
            values.profile_words = strings(&["Grüezi"]);
        });
        mail.apps = strings(&["com.apple.mail"]);
        settings.profiles.push(mail);
        settings
    }

    #[test]
    fn an_assigned_app_picks_its_profile_while_choosing_by_app_is_on() {
        let settings = with_mail_profile();
        assert_eq!(
            profile_for_dictation(
                &settings.profiles,
                DEFAULT_PROFILE_ID,
                true,
                Some("com.apple.mail")
            ),
            "mail"
        );
    }

    #[test]
    fn the_active_profile_applies_elsewhere_and_while_off() {
        let settings = with_mail_profile();
        let profiles = &settings.profiles;
        assert_eq!(
            profile_for_dictation(
                profiles,
                DEFAULT_PROFILE_ID,
                true,
                Some("com.apple.TextEdit")
            ),
            DEFAULT_PROFILE_ID
        );
        assert_eq!(
            profile_for_dictation(profiles, DEFAULT_PROFILE_ID, true, None),
            DEFAULT_PROFILE_ID
        );
        assert_eq!(
            profile_for_dictation(profiles, DEFAULT_PROFILE_ID, false, Some("com.apple.mail")),
            DEFAULT_PROFILE_ID
        );
    }

    #[test]
    fn assigning_moves_an_app_away_from_its_other_profile() {
        let mut settings = with_mail_profile();
        let notes = crate::profiles::profile_for_test(&settings, "notes", "Notes", |_| {});
        settings.profiles.push(notes);
        set_apps(
            &mut settings,
            "notes",
            strings(&[" com.apple.mail ", "com.apple.Notes", "com.apple.Notes", ""]),
        )
        .unwrap();
        assert_eq!(
            settings.profiles[1].apps,
            strings(&["com.apple.mail", "com.apple.Notes"])
        );
        assert!(settings.profiles[0].apps.is_empty());
        assert!(set_apps(&mut settings, "nope", Vec::new()).is_err());
        // The defaults are no profile: no application can be assigned to them.
        assert!(set_apps(&mut settings, DEFAULT_PROFILE_ID, strings(&["x"])).is_err());
    }

    #[test]
    fn manual_mode_needs_the_switch_off_and_an_assignment() {
        let mut settings = with_mail_profile();
        assert!(has_assignments(&settings));
        assert!(settings.profile_auto_by_app);
        assert!(!manual_mode(&settings));
        assert!(!toggle_auto(&mut settings));
        assert!(manual_mode(&settings));
        set_apps(&mut settings, "mail", Vec::new()).unwrap();
        assert!(!has_assignments(&settings));
        assert!(!manual_mode(&settings));
        assert!(toggle_auto(&mut settings));
    }

    #[test]
    fn post_processing_is_reachable_through_an_app_profile() {
        let mut settings = with_mail_profile();
        assert!(!settings.post_process_enabled);
        assert!(post_processing_reachable(&settings));
        toggle_auto(&mut settings);
        assert!(!post_processing_reachable(&settings));
        toggle_auto(&mut settings);
        set_apps(&mut settings, "mail", Vec::new()).unwrap();
        assert!(!post_processing_reachable(&settings));
    }

    #[test]
    fn a_dictation_into_an_assigned_app_reads_that_profiles_values() {
        let settings = with_mail_profile();
        let profile = dictation_profile(&settings, Some("com.apple.mail")).expect("mail");
        assert_eq!(profile.id, "mail");
        let effective = with_values(&settings, Some(&profile.values));
        assert_eq!(effective.selected_model, "whisper");
        assert!(effective.post_process_enabled);
        assert_eq!(effective.profile_words, strings(&["Grüezi"]));
        // Global values stay, and the active profile isn't switched.
        assert_eq!(effective.custom_words, strings(&["Sayuri"]));
        assert_eq!(effective.active_profile_id, DEFAULT_PROFILE_ID);
        assert!(dictation_profile(&settings, Some("com.apple.TextEdit")).is_none());
    }

    #[test]
    fn an_app_profile_without_post_processing_does_not_post_process() {
        let mut settings = with_mail_profile();
        settings.profiles[0].values.post_process_enabled = false;
        assert!(!effective_post_process(
            true,
            Some(&settings.profiles[0].values)
        ));
        assert!(effective_post_process(true, None));
        assert!(!effective_post_process(false, None));
    }

    #[test]
    fn a_turned_over_send_context_reaches_the_recording_of_that_profile_only() {
        let settings = with_mail_profile();
        let mut dictation = Some(DictationProfile {
            id: "mail".to_string(),
            values: settings.profiles[0].values.clone(),
        });
        let values = |dictation: &Option<DictationProfile>| {
            dictation.as_ref().expect("a profile").values.clone()
        };
        assert!(!values(&dictation).send_context);
        assert!(carry_send_context(&mut dictation, "mail", true));
        assert!(values(&dictation).send_context);
        // Another profile's switch leaves the recording's values.
        assert!(!carry_send_context(
            &mut dictation,
            DEFAULT_PROFILE_ID,
            false
        ));
        assert!(values(&dictation).send_context);
        // Its other values stay as they were at its start.
        assert_eq!(values(&dictation).selected_model, "whisper");
        assert!(carry_send_context(&mut dictation, "mail", false));
        assert!(!values(&dictation).send_context);
        // A recording with the active profile holds no values of its own: it
        // reads the stored switch anyway.
        let mut active = None;
        assert!(!carry_send_context(&mut active, "mail", true));
        assert!(active.is_none());
    }

    fn shown(name: &str, mode: ProfileMode) -> Option<OverlayProfile> {
        Some(OverlayProfile {
            name: name.to_string(),
            mode,
        })
    }

    #[test]
    fn the_overlay_shows_no_glyph_without_assignments_a_window_while_on_a_padlock_while_off() {
        let mut settings = with_mail_profile();
        assert_eq!(profile_mode(&settings), ProfileMode::Auto);
        toggle_auto(&mut settings);
        assert_eq!(profile_mode(&settings), ProfileMode::Lock);
        set_apps(&mut settings, "mail", Vec::new()).unwrap();
        assert_eq!(profile_mode(&settings), ProfileMode::None);
        toggle_auto(&mut settings);
        assert_eq!(profile_mode(&settings), ProfileMode::None);
    }

    #[test]
    fn a_switch_names_the_new_profile_with_the_state_of_the_switch() {
        let mut settings = with_mail_profile();
        assert_eq!(
            overlay_profile(&settings, DEFAULT_PROFILE_ID, "Standard"),
            shown("Standard", ProfileMode::Auto)
        );
        toggle_auto(&mut settings);
        assert_eq!(
            overlay_profile(&settings, "mail", "Standard"),
            shown("Mail", ProfileMode::Lock)
        );
        set_apps(&mut settings, "mail", Vec::new()).unwrap();
        assert_eq!(
            overlay_profile(&settings, "mail", "Standard"),
            shown("Mail", ProfileMode::None)
        );
        assert_eq!(overlay_profile(&settings, "nope", "Standard"), None);
    }

    #[test]
    fn a_dictation_names_its_profile_even_the_one_in_use_but_not_the_defaults() {
        let mut settings = with_mail_profile();
        assert_eq!(
            dictation_overlay_profile(&settings, "mail", "Grundeinstellungen"),
            shown("Mail", ProfileMode::Auto)
        );
        // The defaults apply: the pills name no profile (open question 2,
        // recommendation a).
        assert_eq!(
            dictation_overlay_profile(&settings, DEFAULT_PROFILE_ID, "Grundeinstellungen"),
            None
        );
        // A profile in use, the app in front without one of its own: the
        // glyph still shows the switch, not whether an assignment applied.
        let mut work = crate::profiles::profile_for_test(&settings, "work", "Work", |_| {});
        work.apps.clear();
        settings.profiles.push(work);
        settings.active_profile_id = "work".to_string();
        assert_eq!(
            dictation_overlay_profile(&settings, "work", "Grundeinstellungen"),
            shown("Work", ProfileMode::Auto)
        );
        toggle_auto(&mut settings);
        assert_eq!(
            dictation_overlay_profile(&settings, "work", "Grundeinstellungen"),
            shown("Work", ProfileMode::Lock)
        );
        // Without profiles the pills stay as they were.
        settings.profiles.clear();
        assert_eq!(
            dictation_overlay_profile(&settings, DEFAULT_PROFILE_ID, "Grundeinstellungen"),
            None
        );
    }

    #[test]
    fn switching_on_names_the_profile_a_dictation_into_the_app_in_front_gets() {
        let mut settings = with_mail_profile();
        toggle_auto(&mut settings);
        assert!(toggle_auto(&mut settings));
        assert_eq!(
            toggle_notice_profile(&settings, Some("com.apple.mail"), "Standard"),
            shown("Mail", ProfileMode::Auto)
        );
        assert_eq!(
            toggle_notice_profile(&settings, Some("com.apple.TextEdit"), "Standard"),
            shown("Standard", ProfileMode::Auto)
        );
        assert_eq!(
            toggle_notice_profile(&settings, None, "Standard"),
            shown("Standard", ProfileMode::Auto)
        );
    }

    #[test]
    fn switching_off_names_the_active_profile() {
        let mut settings = with_mail_profile();
        assert!(!toggle_auto(&mut settings));
        assert_eq!(
            toggle_notice_profile(&settings, Some("com.apple.mail"), "Standard"),
            shown("Standard", ProfileMode::Lock)
        );
    }

    #[test]
    fn the_overlay_reads_the_mode_in_snake_case() {
        assert_eq!(
            serde_json::to_value(shown("Mail", ProfileMode::Lock)).unwrap(),
            serde_json::json!({ "name": "Mail", "mode": "lock" })
        );
        assert_eq!(
            serde_json::to_value(ProfileMode::Auto).unwrap(),
            serde_json::json!("auto")
        );
        assert_eq!(
            serde_json::to_value(ProfileMode::None).unwrap(),
            serde_json::json!("none")
        );
        assert_eq!(
            serde_json::to_value(None::<OverlayProfile>).unwrap(),
            serde_json::Value::Null
        );
    }

    #[test]
    fn the_app_in_front_at_the_end_chooses_the_profile() {
        let settings = with_mail_profile();
        assert_eq!(
            profile_at_end(&settings, DEFAULT_PROFILE_ID, Some("com.apple.mail")),
            Some("mail".to_string())
        );
        assert_eq!(
            profile_at_end(&settings, "mail", Some("com.apple.TextEdit")),
            Some(DEFAULT_PROFILE_ID.to_string())
        );
    }

    #[test]
    fn the_profile_stays_when_the_end_gives_the_same_or_nothing_to_go_by() {
        let mut settings = with_mail_profile();
        assert_eq!(
            profile_at_end(&settings, "mail", Some("com.apple.mail")),
            None
        );
        assert_eq!(profile_at_end(&settings, "mail", None), None);
        toggle_auto(&mut settings);
        assert_eq!(
            profile_at_end(&settings, DEFAULT_PROFILE_ID, Some("com.apple.mail")),
            None
        );
        toggle_auto(&mut settings);
        set_apps(&mut settings, "mail", Vec::new()).unwrap();
        assert_eq!(
            profile_at_end(&settings, "mail", Some("com.apple.TextEdit")),
            None
        );
    }

    #[test]
    fn values_set_for_a_thread_apply_on_that_thread_only() {
        let settings = with_mail_profile();
        let mail = settings.profiles[0].values.clone();
        assert_eq!(dictation_values(), None);
        let (inside, elsewhere) = with_values_here(mail.clone(), || {
            (
                dictation_values(),
                std::thread::spawn(dictation_values).join().unwrap(),
            )
        });
        assert_eq!(inside, Some(mail.clone()));
        assert_eq!(elsewhere, None);
        assert_eq!(dictation_values(), None);
        // Nested runs restore the outer values.
        let mut other = mail.clone();
        other.selected_model = "moonshine".to_string();
        let (nested, after) = with_values_here(mail.clone(), || {
            (
                with_values_here(other.clone(), dictation_values),
                dictation_values(),
            )
        });
        assert_eq!(nested, Some(other));
        assert_eq!(after, Some(mail));
    }

    #[test]
    fn applications_running_first_each_once_without_danywhere() {
        let running = vec![
            ("com.apple.mail".to_string(), "Mail".to_string()),
            ("com.yaccob.danywhere".to_string(), "DanyWhere".to_string()),
        ];
        let installed = vec![
            ("com.apple.TextEdit".to_string(), "TextEdit".to_string()),
            ("com.apple.mail".to_string(), "Mail".to_string()),
            ("com.apple.Notes".to_string(), "Notes".to_string()),
        ];
        let apps = merge_applications(running, installed, "com.yaccob.danywhere");
        let ids: Vec<(&str, bool)> = apps.iter().map(|a| (a.id.as_str(), a.running)).collect();
        assert_eq!(
            ids,
            vec![
                ("com.apple.mail", true),
                ("com.apple.Notes", false),
                ("com.apple.TextEdit", false),
            ]
        );
    }
}
