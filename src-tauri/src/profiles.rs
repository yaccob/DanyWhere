//! DanyWhere: profiles. A profile holds everything that decides how a
//! dictation is processed: speech model, languages, word lists, post-processing
//! and which of its prompts run in what order, context limits and "Send
//! Context", filler words. Shortcuts, microphone, API keys, the interface and
//! the prompts themselves (one library for all, `prompt_library`) stay global.
//!
//! The flat fields of [`AppSettings`] always hold the active profile's values,
//! so the code that reads settings stays as it is. [`sync_active_profile`]
//! copies them into the active profile whenever settings are stored, so every
//! change lands in the active profile; switching loads another profile's
//! values into the flat fields.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Emitter};

use crate::prompt_library::PromptOrderEntry;
use crate::settings::{self, AppSettings, ChineseScript, LanguageDetectionMode};
use crate::word_lists::WordList;

/// The id of the profile every installation starts with.
pub const DEFAULT_PROFILE_ID: &str = "default";

/// How long the overlay shows which profile was switched to.
const SWITCH_NOTICE: std::time::Duration = std::time::Duration::from_millis(2000);

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type)]
pub struct Profile {
    pub id: String,
    /// Empty = the default profile's name, shown translated ("Standard").
    pub name: String,
    pub values: ProfileValues,
    /// Applications this profile is used for automatically: bundle ids on
    /// macOS, executable names on Windows. Not used yet.
    #[serde(default)]
    pub apps: Vec<String>,
}

/// Everything that is per profile. Mirrors fields of [`AppSettings`] with the
/// same names and types; the context limits mirror `dictation_context`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type)]
#[serde(default)]
pub struct ProfileValues {
    pub selected_model: String,
    pub selected_language: String,
    pub spoken_languages: Vec<String>,
    pub language_detection: LanguageDetectionMode,
    pub language_detection_window_secs: u32,
    pub translate_to_english: bool,
    pub chinese_script: ChineseScript,
    pub profile_words: Vec<String>,
    pub word_lists: Vec<WordList>,
    pub active_word_list_id: Option<String>,
    pub post_process_enabled: bool,
    pub post_process_provider_id: String,
    pub post_process_models: HashMap<String, String>,
    /// The effort per provider (`post_process_effort`).
    pub post_process_efforts: HashMap<String, String>,
    /// Which prompts of the library run, and the order of the whole list
    /// (`prompt_library`). The prompts themselves are shared by all profiles.
    pub post_process_prompt_order: Vec<PromptOrderEntry>,
    /// `dictation_context.age_limit_minutes`
    pub context_age_limit_minutes: u32,
    /// `dictation_context.max_entries`
    pub context_max_entries: u32,
    /// `dictation_context.char_budget`
    pub context_char_budget: u32,
    /// The switch "Send Context" (`send_context`).
    pub send_context: bool,
    pub filler_word_removal_enabled: bool,
    pub custom_filler_words: Option<Vec<String>>,
    /// The "Append" field (`append_text`), which replaced the trailing space;
    /// stored profiles got it from theirs (`append_text::migrate`).
    pub append_text: String,
}

impl Default for ProfileValues {
    /// A new installation's values, for fields a stored profile lacks.
    fn default() -> Self {
        Self::from_settings(&settings::get_default_settings())
    }
}

impl ProfileValues {
    /// The profile values in the flat fields of `settings`.
    pub fn from_settings(settings: &AppSettings) -> Self {
        Self {
            selected_model: settings.selected_model.clone(),
            selected_language: settings.selected_language.clone(),
            spoken_languages: settings.spoken_languages.clone(),
            language_detection: settings.language_detection,
            language_detection_window_secs: settings.language_detection_window_secs,
            translate_to_english: settings.translate_to_english,
            chinese_script: settings.chinese_script,
            profile_words: settings.profile_words.clone(),
            word_lists: settings.word_lists.clone(),
            active_word_list_id: settings.active_word_list_id.clone(),
            post_process_enabled: settings.post_process_enabled,
            post_process_provider_id: settings.post_process_provider_id.clone(),
            post_process_models: settings.post_process_models.clone(),
            post_process_efforts: settings.post_process_efforts.clone(),
            post_process_prompt_order: settings.post_process_prompt_order.clone(),
            context_age_limit_minutes: settings.dictation_context.age_limit_minutes,
            context_max_entries: settings.dictation_context.max_entries,
            context_char_budget: settings.dictation_context.char_budget,
            send_context: settings.send_context,
            filler_word_removal_enabled: settings.filler_word_removal_enabled,
            custom_filler_words: settings.custom_filler_words.clone(),
            append_text: settings.append_text.clone(),
        }
    }

    /// Puts these values into the flat fields of `settings`; every other
    /// field stays as it is.
    pub fn apply_to(&self, settings: &mut AppSettings) {
        settings.selected_model = self.selected_model.clone();
        settings.selected_language = self.selected_language.clone();
        settings.spoken_languages = self.spoken_languages.clone();
        settings.language_detection = self.language_detection;
        settings.language_detection_window_secs = self.language_detection_window_secs;
        settings.translate_to_english = self.translate_to_english;
        settings.chinese_script = self.chinese_script;
        settings.profile_words = self.profile_words.clone();
        settings.word_lists = self.word_lists.clone();
        settings.active_word_list_id = self.active_word_list_id.clone();
        settings.post_process_enabled = self.post_process_enabled;
        settings.post_process_provider_id = self.post_process_provider_id.clone();
        settings.post_process_models = self.post_process_models.clone();
        settings.post_process_efforts = self.post_process_efforts.clone();
        settings.post_process_prompt_order = self.post_process_prompt_order.clone();
        settings.dictation_context.age_limit_minutes = self.context_age_limit_minutes;
        settings.dictation_context.max_entries = self.context_max_entries;
        settings.dictation_context.char_budget = self.context_char_budget;
        settings.send_context = self.send_context;
        settings.filler_word_removal_enabled = self.filler_word_removal_enabled;
        settings.custom_filler_words = self.custom_filler_words.clone();
        settings.append_text = self.append_text.clone();
    }
}

/// What a switch changed that needs more than storing the settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SwitchChanges {
    /// The speech model differs, so it has to be loaded.
    pub model_changed: bool,
    /// Post-processing went on or off, so its shortcuts are (un)registered.
    pub post_process_enabled_changed: bool,
}

/// The profile whose values the flat fields hold.
pub fn active_profile(settings: &AppSettings) -> Option<&Profile> {
    settings
        .profiles
        .iter()
        .find(|profile| profile.id == settings.active_profile_id)
}

/// The name to show for `profile`: its own, or `default_name` (translated
/// "Standard") while the default profile has none.
pub fn display_name(profile: &Profile, default_name: &str) -> String {
    if profile.name.trim().is_empty() {
        default_name.to_string()
    } else {
        profile.name.clone()
    }
}

/// Makes the profiles consistent, e.g. after a read from the store:
/// creates the default profile from the flat values when there is none (a
/// new installation, or an update from a version without profiles), makes
/// the first profile active when the active id names none, makes every
/// profile's prompt order cover the library, and copies the flat values into
/// the active profile. Returns whether anything changed.
pub fn ensure_profiles(settings: &mut AppSettings) -> bool {
    let mut changed = false;
    if settings.profiles.is_empty() {
        log::info!("No profiles yet; the settings in use become the default profile");
        settings.profiles.push(Profile {
            id: DEFAULT_PROFILE_ID.to_string(),
            name: String::new(),
            values: ProfileValues::from_settings(settings),
            apps: Vec::new(),
        });
        settings.active_profile_id = DEFAULT_PROFILE_ID.to_string();
        changed = true;
    }
    if active_profile(settings).is_none() {
        // The flat values belong to no profile (a hand-edited store); the
        // first profile's own values win.
        let first = settings.profiles[0].clone();
        log::warn!(
            "Active profile '{}' not found; using '{}'",
            settings.active_profile_id,
            first.id
        );
        first.values.apply_to(settings);
        settings.active_profile_id = first.id;
        changed = true;
    }
    if crate::prompt_library::normalize_orders(settings) {
        changed = true;
    }
    if sync_active_profile(settings) {
        changed = true;
    }
    changed
}

/// Copies the flat profile fields into the active profile. Called whenever
/// settings are stored, so a change always lands in the active profile.
/// Returns whether the active profile changed.
pub fn sync_active_profile(settings: &mut AppSettings) -> bool {
    let values = ProfileValues::from_settings(settings);
    let active_id = settings.active_profile_id.clone();
    match settings
        .profiles
        .iter_mut()
        .find(|profile| profile.id == active_id)
    {
        Some(profile) if profile.values != values => {
            profile.values = values;
            true
        }
        _ => false,
    }
}

fn profile_index(settings: &AppSettings, id: &str) -> Result<usize, String> {
    settings
        .profiles
        .iter()
        .position(|profile| profile.id == id)
        .ok_or_else(|| format!("Profile '{id}' not found"))
}

fn clean_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("A profile needs a name".to_string());
    }
    Ok(name.to_string())
}

/// Makes profile `id` the active one: the flat values go into the profile
/// that was active, profile `id`'s values into the flat fields. Refused while
/// a dictation runs (`busy`), so no dictation mixes two profiles' values.
pub fn switch_to(
    settings: &mut AppSettings,
    id: &str,
    busy: bool,
) -> Result<SwitchChanges, String> {
    if busy {
        return Err("A profile can't be switched during a dictation".to_string());
    }
    let target = settings.profiles[profile_index(settings, id)?]
        .values
        .clone();
    sync_active_profile(settings);
    let changes = SwitchChanges {
        model_changed: target.selected_model != settings.selected_model,
        post_process_enabled_changed: target.post_process_enabled != settings.post_process_enabled,
    };
    target.apply_to(settings);
    settings.active_profile_id = id.to_string();
    Ok(changes)
}

/// A copy of profile `source_id` named `name`, with a new id and no
/// applications. It isn't switched to.
pub fn duplicate(
    settings: &mut AppSettings,
    source_id: &str,
    name: &str,
    now_millis: i64,
) -> Result<Profile, String> {
    let name = clean_name(name)?;
    let index = profile_index(settings, source_id)?;
    // The active profile's latest values are the flat ones.
    sync_active_profile(settings);
    let base = format!("profile_{now_millis}");
    let mut id = base.clone();
    let mut suffix = 1;
    while settings.profiles.iter().any(|profile| profile.id == id) {
        suffix += 1;
        id = format!("{base}_{suffix}");
    }
    let profile = Profile {
        id,
        name,
        values: settings.profiles[index].values.clone(),
        apps: Vec::new(),
    };
    settings.profiles.push(profile.clone());
    Ok(profile)
}

/// Renames profile `id`. Only the default profile may have an empty name,
/// which shows the translated default name again.
pub fn rename(settings: &mut AppSettings, id: &str, name: &str) -> Result<(), String> {
    let index = profile_index(settings, id)?;
    let name = if id == DEFAULT_PROFILE_ID {
        name.trim().to_string()
    } else {
        clean_name(name)?
    };
    settings.profiles[index].name = name;
    Ok(())
}

/// Deletes profile `id`. The last profile can't be deleted; the active one is
/// switched away from first, to the first other profile, which is refused
/// while a dictation runs. Returns the changes of that switch, if any.
pub fn delete(
    settings: &mut AppSettings,
    id: &str,
    busy: bool,
) -> Result<Option<SwitchChanges>, String> {
    profile_index(settings, id)?;
    if settings.profiles.len() == 1 {
        return Err("The last profile can't be deleted".to_string());
    }
    let mut switched = None;
    if settings.active_profile_id == id {
        let other = settings
            .profiles
            .iter()
            .find(|profile| profile.id != id)
            .map(|profile| profile.id.clone())
            .expect("a second profile");
        switched = Some(switch_to(settings, &other, busy)?);
    }
    settings.profiles.retain(|profile| profile.id != id);
    Ok(switched)
}

/// The translated name of the default profile, in the interface language.
pub fn default_profile_name(settings: &AppSettings) -> String {
    crate::tray_i18n::get_tray_translations(Some(settings.app_language.clone())).default_profile
}

/// What a switch needs beyond storing: load the speech model, (un)register
/// the post-processing shortcuts, tell the window and the tray, and show the
/// name, with the glyph of choosing the profile by application, in the
/// overlay unless a dictation owns it.
fn after_switch(app: &AppHandle, settings: &AppSettings, changes: SwitchChanges) {
    if changes.post_process_enabled_changed {
        crate::shortcut::sync_post_process_shortcuts(app, settings);
    }
    if changes.model_changed && !settings.selected_model.is_empty() {
        let app = app.clone();
        let model_id = settings.selected_model.clone();
        std::thread::spawn(move || {
            if let Err(e) = crate::commands::models::switch_active_model(&app, &model_id) {
                log::warn!("Profile switch: could not load model {model_id}: {e}");
            }
            crate::tray::update_tray_menu(&app);
        });
    }
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({ "setting": "profiles" }),
    );
    crate::tray::rebuild_tray_menu(app);
    crate::prompt_frame::prompts_changed(app);
    if !crate::overlay::is_session_showing() {
        if let Some(shown) = crate::app_profiles::overlay_profile(
            settings,
            &settings.active_profile_id,
            &default_profile_name(settings),
        ) {
            crate::overlay::show_profile_notice(app, shown, SWITCH_NOTICE);
        }
    }
}

/// Store `settings` and tell the settings window and the tray.
fn save(app: &AppHandle, settings: AppSettings) {
    settings::write_settings(app, settings);
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({ "setting": "profiles" }),
    );
    crate::tray::rebuild_tray_menu(app);
}

/// Switches to profile `id` with everything that goes with it.
fn switch_with_effects(app: &AppHandle, id: &str) -> Result<(), String> {
    let busy = crate::tray::dictation_in_progress(app);
    let mut settings = settings::get_settings(app);
    let changes = switch_to(&mut settings, id, busy)?;
    log::info!("Switched to profile {id}");
    settings::write_settings(app, settings.clone());
    after_switch(app, &settings, changes);
    Ok(())
}

/// Switches to profile `id` from the tray.
pub fn select_from_tray(app: &AppHandle, id: &str) -> Result<(), String> {
    switch_with_effects(app, id)
}

#[tauri::command]
#[specta::specta]
pub fn switch_profile(app: AppHandle, id: String) -> Result<(), String> {
    switch_with_effects(&app, &id)
}

#[tauri::command]
#[specta::specta]
pub fn duplicate_profile(
    app: AppHandle,
    source_id: String,
    name: String,
) -> Result<Profile, String> {
    let now = chrono::Utc::now().timestamp_millis();
    let mut settings = settings::get_settings(&app);
    let profile = duplicate(&mut settings, &source_id, &name, now)?;
    save(&app, settings);
    Ok(profile)
}

#[tauri::command]
#[specta::specta]
pub fn rename_profile(app: AppHandle, id: String, name: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    rename(&mut settings, &id, &name)?;
    save(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn delete_profile(app: AppHandle, id: String) -> Result<(), String> {
    let busy = crate::tray::dictation_in_progress(&app);
    let mut settings = settings::get_settings(&app);
    let switched = delete(&mut settings, &id, busy)?;
    settings::write_settings(&app, settings.clone());
    match switched {
        Some(changes) => after_switch(&app, &settings, changes),
        None => {
            let _ = app.emit(
                "settings-changed",
                serde_json::json!({ "setting": "profiles" }),
            );
            crate::tray::rebuild_tray_menu(&app);
            crate::prompt_frame::prompts_changed(&app);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::settings::LLMPrompt;

    fn prompt(id: &str, name: &str, text: &str) -> LLMPrompt {
        LLMPrompt {
            id: id.to_string(),
            name: name.to_string(),
            prompt: text.to_string(),
        }
    }

    fn list(id: &str, words: &[&str]) -> WordList {
        WordList {
            id: id.to_string(),
            name: id.to_uppercase(),
            words: words.iter().map(|w| w.to_string()).collect(),
        }
    }

    fn strings(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    fn entry(id: &str, active: bool) -> PromptOrderEntry {
        PromptOrderEntry::new(id, active)
    }

    /// Settings as an update from a version without profiles finds them.
    fn settings_in_use() -> AppSettings {
        AppSettings {
            selected_model: "parakeet".to_string(),
            selected_language: "de".to_string(),
            spoken_languages: strings(&["de", "en"]),
            post_process_enabled: true,
            post_process_provider_id: "anthropic".to_string(),
            post_process_prompts: vec![
                prompt("p1", "Diktat (DE/EN)", "Clean it up."),
                prompt("p9", "Email", "Make an email out of it."),
            ],
            post_process_prompt_order: vec![entry("p1", true), entry("p9", false)],
            word_lists: vec![list("code", &["Rust", "Tauri"])],
            active_word_list_id: Some("code".to_string()),
            custom_words: strings(&["Sayuri"]),
            ..Default::default()
        }
    }

    /// Values that differ from `settings_in_use` in every field.
    fn other_values() -> ProfileValues {
        ProfileValues {
            selected_model: "whisper-large".to_string(),
            selected_language: "en".to_string(),
            spoken_languages: strings(&["en"]),
            language_detection: LanguageDetectionMode::Always,
            language_detection_window_secs: 7,
            translate_to_english: true,
            chinese_script: ChineseScript::Simplified,
            profile_words: strings(&["Kubernetes"]),
            word_lists: vec![list("med", &["Aspirin"])],
            active_word_list_id: Some("med".to_string()),
            post_process_enabled: false,
            post_process_provider_id: "openai".to_string(),
            post_process_models: HashMap::from([("openai".to_string(), "gpt-x".to_string())]),
            post_process_efforts: HashMap::from([("openai".to_string(), "high".to_string())]),
            post_process_prompt_order: vec![entry("p9", true), entry("p1", false)],
            context_age_limit_minutes: 11,
            context_max_entries: 2,
            context_char_budget: 999,
            send_context: true,
            filler_word_removal_enabled: false,
            custom_filler_words: Some(strings(&["äh"])),
            append_text: r"\s{Enter}".to_string(),
        }
    }

    /// `settings_in_use` with its default profile and a second one, "work",
    /// holding `other_values`.
    fn two_profiles() -> AppSettings {
        let mut s = settings_in_use();
        ensure_profiles(&mut s);
        s.profiles.push(Profile {
            id: "work".to_string(),
            name: "Work".to_string(),
            values: other_values(),
            apps: Vec::new(),
        });
        s
    }

    // ---- Migration ------------------------------------------------------

    #[test]
    fn a_new_installation_gets_one_default_profile_with_the_default_values() {
        let mut s = AppSettings::default();
        assert!(ensure_profiles(&mut s));
        assert_eq!(s.profiles.len(), 1);
        assert_eq!(s.profiles[0].id, DEFAULT_PROFILE_ID);
        assert_eq!(s.profiles[0].name, "");
        assert!(s.profiles[0].apps.is_empty());
        assert_eq!(s.active_profile_id, DEFAULT_PROFILE_ID);
        assert_eq!(
            s.profiles[0].values,
            ProfileValues::from_settings(&AppSettings::default())
        );
    }

    #[test]
    fn an_update_keeps_the_settings_in_use_as_the_default_profile() {
        let mut s = settings_in_use();
        assert!(ensure_profiles(&mut s));
        let values = &active_profile(&s).expect("an active profile").values;
        assert_eq!(values.selected_model, "parakeet");
        assert_eq!(values.selected_language, "de");
        assert_eq!(
            values.post_process_prompt_order,
            vec![entry("p1", true), entry("p9", false)]
        );
        assert_eq!(values.word_lists, vec![list("code", &["Rust", "Tauri"])]);
        assert_eq!(values.active_word_list_id.as_deref(), Some("code"));
    }

    #[test]
    fn making_the_profiles_consistent_twice_changes_nothing_the_second_time() {
        let mut s = settings_in_use();
        assert!(ensure_profiles(&mut s));
        let once = s.profiles.clone();
        assert!(!ensure_profiles(&mut s));
        assert_eq!(s.profiles, once);
    }

    #[test]
    fn an_unknown_active_profile_means_the_first() {
        let mut s = two_profiles();
        s.active_profile_id = "gone".to_string();
        assert!(ensure_profiles(&mut s));
        assert_eq!(s.active_profile_id, DEFAULT_PROFILE_ID);
    }

    #[test]
    fn every_profiles_prompt_order_covers_the_library() {
        let mut s = two_profiles();
        s.profiles[1].values.post_process_prompt_order = vec![entry("gone", true)];
        assert!(ensure_profiles(&mut s));
        assert_eq!(
            s.profiles[1].values.post_process_prompt_order,
            vec![entry("p1", false), entry("p9", false)]
        );
    }

    #[test]
    fn a_stored_profile_without_some_values_gets_the_defaults_for_them() {
        let json = serde_json::json!({
            "id": "old",
            "name": "Old",
            "values": { "selected_language": "fr" }
        });
        let profile: Profile = serde_json::from_value(json).expect("a profile");
        assert_eq!(profile.values.selected_language, "fr");
        assert!(profile.apps.is_empty());
        assert_eq!(
            profile.values.context_char_budget,
            AppSettings::default().dictation_context.char_budget
        );
    }

    // ---- Changes go into the active profile -------------------------------

    #[test]
    fn a_change_lands_in_the_active_profile_only() {
        let mut s = two_profiles();
        s.selected_language = "it".to_string();
        s.post_process_prompt_order = vec![entry("p9", true), entry("p1", true)];
        sync_active_profile(&mut s);
        let active = active_profile(&s).expect("an active profile");
        assert_eq!(active.values.selected_language, "it");
        assert_eq!(
            active.values.post_process_prompt_order,
            vec![entry("p9", true), entry("p1", true)]
        );
        assert_eq!(s.profiles[1].values, other_values());
    }

    // ---- Switching --------------------------------------------------------

    #[test]
    fn switching_swaps_every_profile_value_and_nothing_else() {
        let mut s = two_profiles();
        // A change not yet synced belongs to the profile being left.
        s.selected_language = "it".to_string();
        let before = s.clone();
        switch_to(&mut s, "work", false).expect("switched");
        assert_eq!(s.active_profile_id, "work");
        assert_eq!(ProfileValues::from_settings(&s), other_values());
        let left = &s.profiles[0].values;
        assert_eq!(left.selected_language, "it");
        assert_eq!(left.selected_model, "parakeet");
        // Global settings stay.
        assert_eq!(s.custom_words, before.custom_words);
        assert_eq!(s.bindings.len(), before.bindings.len());
        assert_eq!(
            s.dictation_context.excluded_apps,
            before.dictation_context.excluded_apps
        );
        assert_eq!(
            s.word_correction_threshold,
            before.word_correction_threshold
        );
        assert_eq!(s.app_language, before.app_language);
        assert_eq!(
            s.post_process_providers.len(),
            before.post_process_providers.len()
        );
        // The prompt library is shared, not a profile value.
        assert_eq!(s.post_process_prompts, before.post_process_prompts);
        // And back again.
        switch_to(&mut s, DEFAULT_PROFILE_ID, false).expect("switched back");
        assert_eq!(s.selected_language, "it");
        assert_eq!(s.selected_model, "parakeet");
        assert_eq!(s.profiles[1].values, other_values());
    }

    #[test]
    fn the_effort_belongs_to_the_profile() {
        let mut s = two_profiles();
        s.post_process_efforts
            .insert("anthropic".to_string(), "xhigh".to_string());
        sync_active_profile(&mut s);
        assert_eq!(
            s.profiles[0].values.post_process_efforts.get("anthropic"),
            Some(&"xhigh".to_string())
        );
        switch_to(&mut s, "work", false).expect("switched");
        assert_eq!(
            s.post_process_efforts,
            HashMap::from([("openai".to_string(), "high".to_string())])
        );
        switch_to(&mut s, DEFAULT_PROFILE_ID, false).expect("switched back");
        assert_eq!(
            s.post_process_efforts.get("anthropic"),
            Some(&"xhigh".to_string())
        );
    }

    #[test]
    fn a_stored_profile_without_an_effort_loads_with_the_models_default() {
        let mut stored = serde_json::to_value(other_values()).unwrap();
        stored
            .as_object_mut()
            .unwrap()
            .remove("post_process_efforts");
        let values: ProfileValues = serde_json::from_value(stored).unwrap();
        assert!(values.post_process_efforts.is_empty());
        assert_eq!(values.selected_model, "whisper-large");
    }

    #[test]
    fn switching_reports_a_new_model_and_post_processing_going_off() {
        let mut s = two_profiles();
        let changes = switch_to(&mut s, "work", false).expect("switched");
        assert_eq!(
            changes,
            SwitchChanges {
                model_changed: true,
                post_process_enabled_changed: true,
            }
        );
        let mut s = two_profiles();
        s.profiles[1].values.selected_model = s.selected_model.clone();
        s.profiles[1].values.post_process_enabled = s.post_process_enabled;
        let changes = switch_to(&mut s, "work", false).expect("switched");
        assert_eq!(changes, SwitchChanges::default());
    }

    #[test]
    fn switching_during_a_dictation_is_refused_and_changes_nothing() {
        let mut s = two_profiles();
        let before = s.clone();
        assert!(switch_to(&mut s, "work", true).is_err());
        assert_eq!(s.active_profile_id, before.active_profile_id);
        assert_eq!(s.selected_model, before.selected_model);
    }

    #[test]
    fn switching_to_an_unknown_profile_fails() {
        let mut s = two_profiles();
        assert!(switch_to(&mut s, "gone", false).is_err());
        assert_eq!(s.active_profile_id, DEFAULT_PROFILE_ID);
    }

    // ---- Duplicating, renaming, deleting ----------------------------------

    #[test]
    fn a_duplicate_copies_the_values_and_then_drifts_apart() {
        let mut s = two_profiles();
        let copy = duplicate(&mut s, DEFAULT_PROFILE_ID, "  Arbeit  ", 42).expect("a copy");
        assert_eq!(copy.name, "Arbeit");
        assert!(copy.apps.is_empty());
        assert_ne!(copy.id, DEFAULT_PROFILE_ID);
        assert_eq!(s.profiles.len(), 3);
        assert_eq!(s.active_profile_id, DEFAULT_PROFILE_ID);
        assert_eq!(copy.values, ProfileValues::from_settings(&s));

        switch_to(&mut s, &copy.id, false).expect("switched");
        s.post_process_prompt_order = vec![entry("p9", true), entry("p1", false)];
        sync_active_profile(&mut s);
        assert_eq!(
            s.profiles[0].values.post_process_prompt_order,
            vec![entry("p1", true), entry("p9", false)]
        );
    }

    #[test]
    fn a_prompts_text_is_the_same_in_every_profile() {
        let mut s = two_profiles();
        s.post_process_prompts[1].prompt = "Write a short email.".to_string();
        switch_to(&mut s, "work", false).expect("switched");
        assert_eq!(s.post_process_prompts[1].prompt, "Write a short email.");
        let request = crate::prompt_library::requested(&s);
        assert_eq!(
            crate::prompt_library::request_instructions(&request),
            "Write a short email."
        );
    }

    #[test]
    fn a_duplicate_of_the_active_profile_has_its_latest_values() {
        let mut s = two_profiles();
        s.selected_language = "it".to_string();
        let copy = duplicate(&mut s, DEFAULT_PROFILE_ID, "Copy", 1).expect("a copy");
        assert_eq!(copy.values.selected_language, "it");
    }

    #[test]
    fn duplicates_made_in_the_same_millisecond_get_their_own_ids() {
        let mut s = two_profiles();
        let a = duplicate(&mut s, "work", "A", 7).expect("a copy");
        let b = duplicate(&mut s, "work", "B", 7).expect("a copy");
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn a_duplicate_needs_a_name_and_an_existing_source() {
        let mut s = two_profiles();
        assert!(duplicate(&mut s, "work", "   ", 1).is_err());
        assert!(duplicate(&mut s, "gone", "Name", 1).is_err());
        assert_eq!(s.profiles.len(), 2);
    }

    #[test]
    fn renaming_trims_and_only_the_default_profile_may_lose_its_name() {
        let mut s = two_profiles();
        rename(&mut s, "work", "  Büro ").expect("renamed");
        assert_eq!(s.profiles[1].name, "Büro");
        assert!(rename(&mut s, "work", "  ").is_err());
        assert_eq!(s.profiles[1].name, "Büro");
        rename(&mut s, DEFAULT_PROFILE_ID, "Privat").expect("renamed");
        rename(&mut s, DEFAULT_PROFILE_ID, "").expect("back to the default name");
        assert_eq!(s.profiles[0].name, "");
        assert!(rename(&mut s, "gone", "Name").is_err());
    }

    #[test]
    fn the_last_profile_cannot_be_deleted() {
        let mut s = settings_in_use();
        ensure_profiles(&mut s);
        assert!(delete(&mut s, DEFAULT_PROFILE_ID, false).is_err());
        assert_eq!(s.profiles.len(), 1);
    }

    #[test]
    fn deleting_the_active_profile_switches_to_the_first_other_one_first() {
        let mut s = two_profiles();
        let mut third = s.profiles[1].clone();
        third.id = "third".to_string();
        s.profiles.push(third);
        switch_to(&mut s, "work", false).expect("switched");
        let changes = delete(&mut s, "work", false).expect("deleted");
        assert!(changes.is_some());
        assert_eq!(s.active_profile_id, DEFAULT_PROFILE_ID);
        assert_eq!(s.selected_model, "parakeet");
        assert_eq!(
            s.profiles.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec![DEFAULT_PROFILE_ID, "third"]
        );
    }

    #[test]
    fn deleting_another_profile_keeps_the_active_one() {
        let mut s = two_profiles();
        let changes = delete(&mut s, "work", false).expect("deleted");
        assert_eq!(changes, None);
        assert_eq!(s.active_profile_id, DEFAULT_PROFILE_ID);
        assert_eq!(s.profiles.len(), 1);
        assert!(delete(&mut s, "gone", false).is_err());
    }

    #[test]
    fn deleting_the_active_profile_during_a_dictation_is_refused() {
        let mut s = two_profiles();
        assert!(delete(&mut s, DEFAULT_PROFILE_ID, true).is_err());
        assert_eq!(s.profiles.len(), 2);
        // Another profile may go.
        assert!(delete(&mut s, "work", true).is_ok());
    }

    // ---- Names ------------------------------------------------------------

    #[test]
    fn the_default_profile_shows_the_translated_name_until_renamed() {
        let mut s = two_profiles();
        assert_eq!(display_name(&s.profiles[0], "Standard"), "Standard");
        assert_eq!(display_name(&s.profiles[1], "Standard"), "Work");
        s.profiles[0].name = "Privat".to_string();
        assert_eq!(display_name(&s.profiles[0], "Standard"), "Privat");
    }

    #[test]
    fn the_default_profiles_name_follows_the_interface_language() {
        let mut s = AppSettings {
            app_language: "de".to_string(),
            ..Default::default()
        };
        assert_eq!(default_profile_name(&s), "Standard");
        s.app_language = "en".to_string();
        assert_eq!(default_profile_name(&s), "Default");
    }
}
