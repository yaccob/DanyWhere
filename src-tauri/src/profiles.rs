//! DanyWhere: profiles. A profile holds everything that concerns a
//! dictation (the user's decision P6 of 2026-11-19): speech model, languages,
//! word lists, post-processing with its providers, keys and which prompts run
//! in what order, context, filler words, microphone, sounds, overlay and how
//! the text is inserted, and since 2026-11-19 the shortcuts with the end
//! shortcuts (`end_keys`; user, T1: the start shortcut is a value of the
//! profile and doesn't choose it). What concerns the machine, the app or the
//! interface stays global (`setting_classes`), and so do the prompts
//! themselves, one library for all (`prompt_library`).
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
use crate::settings::{
    self, AppSettings, ChineseScript, ClipboardHandling, LanguageDetectionMode, OverlayPosition,
    OverlayStyle, PasteMethod, PostProcessProvider, SecretMap, ShortcutActivation, SoundTheme,
    TypingTool,
};
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
/// same names and types; the context settings mirror `dictation_context`.
/// New fields go at the end; `setting_classes` checks that every setting is
/// a profile value or classified otherwise.
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
    pub filler_word_removal_enabled: bool,
    pub custom_filler_words: Option<Vec<String>>,
    /// How the dictation shortcut's presses drive a recording. Read when a
    /// key event comes, so from the profile in use.
    pub shortcut_activation: ShortcutActivation,
    /// The feedback sounds, their volume and theme, the done sound.
    pub audio_feedback: bool,
    pub audio_feedback_volume: f32,
    pub sound_theme: SoundTheme,
    pub done_sound: bool,
    /// Mute the computer's sound while recording.
    pub mute_while_recording: bool,
    /// The microphone stays open between dictations; follows the profile in
    /// use (`profile_effects`).
    pub always_on_microphone: bool,
    /// The microphone a dictation records with (`dictation_microphone`).
    pub selected_microphone: Option<String>,
    pub selected_channel: Option<u16>,
    pub clamshell_microphone: Option<String>,
    /// Where the feedback sounds play.
    pub selected_output_device: Option<String>,
    pub airpods_mode: bool,
    /// Where the recording overlay sits and how it shows (`overlay_look`).
    pub overlay_position: OverlayPosition,
    pub overlay_style: OverlayStyle,
    /// How the text gets into the application.
    pub paste_method: PasteMethod,
    pub clipboard_handling: ClipboardHandling,
    pub clipboard_notice_ms: u64,
    pub typing_tool: TypingTool,
    pub external_script_path: Option<String>,
    /// The word list every dictation with this profile gets, after its own
    /// (`profile_words`); the same in every profile until one differs.
    pub custom_words: Vec<String>,
    /// `dictation_context.excluded_apps`
    pub context_excluded_apps: Vec<String>,
    /// The post-processing providers with their endpoints, their API keys
    /// (never exported, see `profile_transfer`) and the time limit.
    pub post_process_providers: Vec<PostProcessProvider>,
    pub post_process_api_keys: SecretMap,
    pub post_process_timeout_secs: u64,
    /// Drop silence from recordings; decode long audio in chunks.
    pub vad_enabled: bool,
    pub long_form_chunking: bool,
    /// The shortcuts (`bindings`), entry by binding id: the start shortcuts
    /// apply only while this profile does (`shortcut::profile_sync`).
    pub bindings: HashMap<String, crate::settings::ShortcutBinding>,
    /// The end shortcuts (`end_keys`), each with a stable id.
    pub end_keys: Vec<crate::end_keys::EndKey>,
    /// The default end per start shortcut (`default_ends`), entry by binding
    /// id.
    pub default_ends: HashMap<String, String>,
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
            filler_word_removal_enabled: settings.filler_word_removal_enabled,
            custom_filler_words: settings.custom_filler_words.clone(),
            shortcut_activation: settings.shortcut_activation,
            audio_feedback: settings.audio_feedback,
            audio_feedback_volume: settings.audio_feedback_volume,
            sound_theme: settings.sound_theme,
            done_sound: settings.done_sound,
            mute_while_recording: settings.mute_while_recording,
            always_on_microphone: settings.always_on_microphone,
            selected_microphone: settings.selected_microphone.clone(),
            selected_channel: settings.selected_channel,
            clamshell_microphone: settings.clamshell_microphone.clone(),
            selected_output_device: settings.selected_output_device.clone(),
            airpods_mode: settings.airpods_mode,
            overlay_position: settings.overlay_position,
            overlay_style: settings.overlay_style,
            paste_method: settings.paste_method,
            clipboard_handling: settings.clipboard_handling,
            clipboard_notice_ms: settings.clipboard_notice_ms,
            typing_tool: settings.typing_tool,
            external_script_path: settings.external_script_path.clone(),
            custom_words: settings.custom_words.clone(),
            context_excluded_apps: settings.dictation_context.excluded_apps.clone(),
            post_process_providers: settings.post_process_providers.clone(),
            post_process_api_keys: settings.post_process_api_keys.clone(),
            post_process_timeout_secs: settings.post_process_timeout_secs,
            vad_enabled: settings.vad_enabled,
            long_form_chunking: settings.long_form_chunking,
            bindings: settings.bindings.clone(),
            end_keys: settings.end_keys.clone(),
            default_ends: settings.default_ends.clone(),
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
        settings.filler_word_removal_enabled = self.filler_word_removal_enabled;
        settings.custom_filler_words = self.custom_filler_words.clone();
        settings.bindings = self.bindings.clone();
        settings.end_keys = self.end_keys.clone();
        settings.default_ends = self.default_ends.clone();
        settings.shortcut_activation = self.shortcut_activation;
        settings.audio_feedback = self.audio_feedback;
        settings.audio_feedback_volume = self.audio_feedback_volume;
        settings.sound_theme = self.sound_theme;
        settings.done_sound = self.done_sound;
        settings.mute_while_recording = self.mute_while_recording;
        settings.always_on_microphone = self.always_on_microphone;
        settings.selected_microphone = self.selected_microphone.clone();
        settings.selected_channel = self.selected_channel;
        settings.clamshell_microphone = self.clamshell_microphone.clone();
        settings.selected_output_device = self.selected_output_device.clone();
        settings.airpods_mode = self.airpods_mode;
        settings.overlay_position = self.overlay_position;
        settings.overlay_style = self.overlay_style;
        settings.paste_method = self.paste_method;
        settings.clipboard_handling = self.clipboard_handling;
        settings.clipboard_notice_ms = self.clipboard_notice_ms;
        settings.typing_tool = self.typing_tool;
        settings.external_script_path = self.external_script_path.clone();
        settings.custom_words = self.custom_words.clone();
        settings.dictation_context.excluded_apps = self.context_excluded_apps.clone();
        settings.post_process_providers = self.post_process_providers.clone();
        settings.post_process_api_keys = self.post_process_api_keys.clone();
        settings.post_process_timeout_secs = self.post_process_timeout_secs;
        settings.vad_enabled = self.vad_enabled;
        settings.long_form_chunking = self.long_form_chunking;
    }
}

/// The serde names of the profile values (contract V1): what the coupling
/// of defaults and the filling of missing values work over.
pub fn value_names() -> &'static [String] {
    static NAMES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    NAMES.get_or_init(|| match serde_json::to_value(ProfileValues::default()) {
        Ok(serde_json::Value::Object(map)) => map.keys().cloned().collect(),
        _ => Vec::new(),
    })
}

/// Contract V5: gives `values` the value of `flat` for every profile value
/// that `stored` (the profile's values as stored, a JSON object) lacks.
/// Returns whether one was missing.
///
/// Generic over the serde names, so a new profile value needs no code here.
pub fn fill_missing_values(
    values: &mut ProfileValues,
    stored: &serde_json::Map<String, serde_json::Value>,
    flat: &ProfileValues,
) -> bool {
    let missing = missing_names(stored);
    if missing.is_empty() {
        return false;
    }
    let (Ok(serde_json::Value::Object(flat)), Ok(serde_json::Value::Object(mut filled))) =
        (serde_json::to_value(flat), serde_json::to_value(&*values))
    else {
        log::warn!("Profile values could not be written as JSON; nothing filled in");
        return false;
    };
    for name in missing {
        if let Some(value) = flat.get(name) {
            filled.insert(name.clone(), value.clone());
        }
    }
    match serde_json::from_value(serde_json::Value::Object(filled)) {
        Ok(filled) => {
            *values = filled;
            true
        }
        Err(e) => {
            log::warn!("Profile values could not be filled in: {e}");
            false
        }
    }
}

/// The profile values `stored` lacks.
fn missing_names(stored: &serde_json::Map<String, serde_json::Value>) -> Vec<&'static String> {
    value_names()
        .iter()
        .filter(|name| !stored.contains_key(name.as_str()))
        .collect()
}

/// Contract V5: every stored profile that lacks a profile value gets the
/// flat value it had, not a new installation's. `raw` is the stored
/// settings as JSON. Returns whether a profile got one.
pub fn fill_missing_values_from_flat(settings: &mut AppSettings, raw: &serde_json::Value) -> bool {
    let Some(stored_profiles) = raw.get("profiles").and_then(|profiles| profiles.as_array()) else {
        return false;
    };
    let stored_values = |id: &str| {
        stored_profiles
            .iter()
            .find(|stored| stored.get("id").and_then(|stored_id| stored_id.as_str()) == Some(id))
            .and_then(|stored| stored.get("values"))
            .and_then(|values| values.as_object())
    };
    // The flat values are built only when a profile lacks one, i.e. once
    // after an update.
    let lacking = settings.profiles.iter().any(|profile| {
        stored_values(&profile.id).is_some_and(|stored| !missing_names(stored).is_empty())
    });
    if !lacking {
        return false;
    }
    let flat = ProfileValues::from_settings(settings);
    let mut changed = false;
    for profile in settings.profiles.iter_mut() {
        let Some(stored) = stored_values(&profile.id) else {
            continue;
        };
        let missing = missing_names(stored);
        if fill_missing_values(&mut profile.values, stored, &flat) {
            log::info!(
                "Profile {} takes the settings in use for {missing:?}",
                profile.id
            );
            changed = true;
        }
    }
    changed
}

/// Runs `ensure`, a repair of flat values (e.g. Handy's providers a new
/// version brings), on the values of every profile but the active one,
/// whose values are the flat ones. Returns whether a profile changed.
pub fn ensure_in_every_profile(
    settings: &mut AppSettings,
    ensure: impl Fn(&mut AppSettings) -> bool,
) -> bool {
    let mut profiles = std::mem::take(&mut settings.profiles);
    // Without the profiles, so it's a small copy.
    let mut scratch = settings.clone();
    let mut changed = false;
    for profile in profiles
        .iter_mut()
        .filter(|profile| profile.id != settings.active_profile_id)
    {
        profile.values.apply_to(&mut scratch);
        if ensure(&mut scratch) {
            profile.values = ProfileValues::from_settings(&scratch);
            changed = true;
        }
    }
    settings.profiles = profiles;
    changed
}

/// What a switch changed that needs more than storing the settings. The
/// shortcuts follow every switch (`shortcut::profile_sync`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SwitchChanges {
    /// The speech model differs, so it has to be loaded.
    pub model_changed: bool,
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
    // Every profile has every shortcut: one added since a profile was
    // stored (a new default binding, or an imported profile without
    // shortcuts) comes from the ones in use.
    let flat_bindings = settings.bindings.clone();
    for profile in settings.profiles.iter_mut() {
        for (id, binding) in &flat_bindings {
            if !profile.values.bindings.contains_key(id) {
                profile.values.bindings.insert(id.clone(), binding.clone());
                changed = true;
            }
        }
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

/// What a switch needs beyond storing: load the speech model, register the
/// shortcuts of the profile that applies now (`shortcut::profile_sync`),
/// what the other values need (microphone, overlay: `profile_effects`, from
/// the settings `before` it), tell the window and the tray, and show the
/// name, with the glyph of choosing the profile by application, in the
/// overlay unless a dictation owns it.
fn after_switch(
    app: &AppHandle,
    before: &AppSettings,
    settings: &AppSettings,
    changes: SwitchChanges,
) {
    crate::profile_effects::apply_profile_value_effects(app, before, settings);
    crate::shortcut::profile_sync::sync_profile_shortcuts(app, before, settings);
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
    let before = settings.clone();
    let changes = switch_to(&mut settings, id, busy)?;
    log::info!("Switched to profile {id}");
    settings::write_settings(app, settings.clone());
    after_switch(app, &before, &settings, changes);
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
    let before = settings.clone();
    let switched = delete(&mut settings, &id, busy)?;
    settings::write_settings(&app, settings.clone());
    match switched {
        Some(changes) => after_switch(&app, &before, &settings, changes),
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

/// Helpers for tests elsewhere that set profile values by their serde
/// names.
#[cfg(test)]
pub(crate) mod testing {
    use super::ProfileValues;

    /// `base` with the values of `extra` (a JSON object of serde names)
    /// over it.
    pub fn values_with(base: &ProfileValues, extra: serde_json::Value) -> ProfileValues {
        let mut values = serde_json::to_value(base).expect("serializes");
        for (name, value) in extra.as_object().expect("an object") {
            values[name] = value.clone();
        }
        serde_json::from_value(values).expect("profile values")
    }

    /// The value named `name` of `values`, as JSON.
    pub fn value_of(values: &ProfileValues, name: &str) -> serde_json::Value {
        serde_json::to_value(values).expect("serializes")[name].clone()
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{value_of, values_with};
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

    /// Whether `actual` holds `expected`: equal, or for objects every entry
    /// of `expected` (a map with more providers holds one with fewer).
    fn holds(actual: &serde_json::Value, expected: &serde_json::Value) -> bool {
        match (actual, expected) {
            (serde_json::Value::Object(actual), serde_json::Value::Object(expected)) => expected
                .iter()
                .all(|(key, value)| actual.get(key).is_some_and(|got| holds(got, value))),
            _ => actual == expected,
        }
    }

    /// Values that concern a dictation, other than a new installation's.
    fn dictation_extra() -> serde_json::Value {
        serde_json::json!({
            "shortcut_activation": "push_to_talk",
            "audio_feedback": true,
            "audio_feedback_volume": 0.25,
            "sound_theme": "pop",
            "done_sound": false,
            "mute_while_recording": true,
            "always_on_microphone": true,
            "selected_microphone": "USB Mic",
            "selected_channel": 1,
            "clamshell_microphone": "Desk Mic",
            "selected_output_device": "Speakers",
            "airpods_mode": true,
            "overlay_position": "top",
            "overlay_style": "minimal",
            "paste_method": "direct",
            "clipboard_handling": "copy_to_clipboard",
            "clipboard_notice_ms": 900,
            "typing_tool": "xdotool",
            "external_script_path": "/usr/local/bin/paste-it",
            "custom_words": ["Kubernetes"],
            "context_excluded_apps": ["com.apple.mail"],
            "post_process_api_keys": { "openai": "sk-work" },
            "post_process_timeout_secs": 15,
            "vad_enabled": false,
            "long_form_chunking": false,
        })
    }

    /// The flat value of profile value `name` in `settings`, as JSON.
    fn flat_value(settings: &AppSettings, name: &str) -> serde_json::Value {
        let flat = serde_json::to_value(settings).expect("serializes");
        match name.strip_prefix("context_") {
            Some("excluded_apps") => flat["dictation_context"]["excluded_apps"].clone(),
            _ => flat[name].clone(),
        }
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
            filler_word_removal_enabled: false,
            custom_filler_words: Some(strings(&["äh"])),
            shortcut_activation: ShortcutActivation::PushToTalk,
            audio_feedback: true,
            audio_feedback_volume: 0.25,
            sound_theme: SoundTheme::Pop,
            done_sound: false,
            mute_while_recording: true,
            always_on_microphone: true,
            selected_microphone: Some("USB Mic".to_string()),
            selected_channel: Some(1),
            clamshell_microphone: Some("Desk Mic".to_string()),
            selected_output_device: Some("Speakers".to_string()),
            airpods_mode: true,
            overlay_position: OverlayPosition::Top,
            overlay_style: OverlayStyle::Minimal,
            paste_method: PasteMethod::Direct,
            clipboard_handling: ClipboardHandling::CopyToClipboard,
            clipboard_notice_ms: 900,
            typing_tool: TypingTool::Xdotool,
            external_script_path: Some("/usr/local/bin/paste-it".to_string()),
            custom_words: strings(&["Kubernetes"]),
            context_excluded_apps: strings(&["com.apple.mail"]),
            post_process_providers: Vec::new(),
            post_process_api_keys: serde_json::from_value(serde_json::json!({
                "openai": "sk-work"
            }))
            .expect("keys"),
            post_process_timeout_secs: 15,
            vad_enabled: false,
            long_form_chunking: false,
            bindings: {
                let mut bindings = crate::settings::get_default_settings().bindings;
                bindings
                    .get_mut("transcribe")
                    .expect("a dictation shortcut")
                    .current_binding = "option+space".to_string();
                bindings
            },
            end_keys: crate::end_keys::builtin(r"\n\n", true),
            default_ends: HashMap::from([(
                "transcribe".to_string(),
                crate::end_keys::POST_PROCESS_ID.to_string(),
            )]),
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

    // ---- Everything that concerns a dictation is a profile value (P6) -----

    #[test]
    fn switching_brings_every_value_that_concerns_a_dictation() {
        let mut s = two_profiles();
        let extra = dictation_extra();
        s.profiles[1].values = values_with(&other_values(), extra.clone());
        let before = s.clone();
        switch_to(&mut s, "work", false).expect("switched");
        for (name, value) in extra.as_object().unwrap() {
            assert!(
                holds(&flat_value(&s, name), value),
                "{name}: {} instead of {value}",
                flat_value(&s, name)
            );
        }
        switch_to(&mut s, DEFAULT_PROFILE_ID, false).expect("switched back");
        for name in extra.as_object().unwrap().keys() {
            assert_eq!(flat_value(&s, name), flat_value(&before, name), "{name}");
        }
    }

    #[test]
    fn a_changed_microphone_lands_in_the_active_profile_only() {
        let mut s = two_profiles();
        s.selected_microphone = Some("AirPods".to_string());
        s.custom_words = strings(&["Tauri"]);
        sync_active_profile(&mut s);
        let active = &active_profile(&s).expect("an active profile").values;
        assert_eq!(value_of(active, "selected_microphone"), "AirPods");
        assert_eq!(
            value_of(active, "custom_words"),
            serde_json::json!(["Tauri"])
        );
        assert_eq!(s.profiles[1].values, other_values());
    }

    // ---- Contract V5: a stored profile without a value gets the flat one --

    /// Stored settings with the flat microphone "USB Mic" and two profiles
    /// as an older version wrote them, without the newer values; "work"
    /// stores its microphone as none.
    fn stored_without_new_values() -> serde_json::Value {
        serde_json::json!({
            "selected_language": "de",
            "selected_microphone": "USB Mic",
            "custom_words": ["Sayuri"],
            "post_process_api_keys": { "anthropic": "sk-ant", "openai": "" },
            "dictation_context": { "excluded_apps": ["1Password"] },
            "active_profile_id": "default",
            "profiles": [
                { "id": "default", "name": "", "values": { "selected_language": "de" } },
                { "id": "old", "name": "Old", "values": { "selected_language": "fr" } },
                { "id": "work", "name": "Work",
                  "values": { "selected_language": "en", "selected_microphone": null } }
            ]
        })
    }

    #[test]
    fn a_stored_profile_without_a_value_gets_the_flat_one() {
        let raw = stored_without_new_values();
        let mut s: AppSettings = serde_json::from_value(raw.clone()).expect("settings");
        assert!(fill_missing_values_from_flat(&mut s, &raw));
        let old = &s.profiles[1].values;
        assert_eq!(old.selected_language, "fr", "a stored value stays");
        assert_eq!(value_of(old, "selected_microphone"), "USB Mic");
        assert_eq!(value_of(old, "custom_words"), serde_json::json!(["Sayuri"]));
        assert_eq!(
            value_of(old, "context_excluded_apps"),
            serde_json::json!(["1Password"])
        );
        assert_eq!(
            value_of(old, "post_process_api_keys")["anthropic"],
            "sk-ant"
        );
    }

    #[test]
    fn a_stored_value_stays_even_where_it_is_empty() {
        let raw = stored_without_new_values();
        let mut s: AppSettings = serde_json::from_value(raw.clone()).expect("settings");
        fill_missing_values_from_flat(&mut s, &raw);
        let work = &s.profiles[2].values;
        assert_eq!(
            value_of(work, "selected_microphone"),
            serde_json::Value::Null
        );
        assert_eq!(work.selected_language, "en");
        // Values it lacks still come from the flat ones.
        assert_eq!(
            value_of(work, "custom_words"),
            serde_json::json!(["Sayuri"])
        );
    }

    #[test]
    fn profiles_with_every_value_are_left_alone() {
        let mut s = two_profiles();
        let raw = serde_json::to_value(&s).expect("serializes");
        let before = s.clone();
        assert!(!fill_missing_values_from_flat(&mut s, &raw));
        assert_eq!(s.profiles, before.profiles);
        // Nothing stored at all (a new installation): nothing to fill.
        assert!(!fill_missing_values_from_flat(
            &mut s,
            &serde_json::json!({})
        ));
    }

    #[test]
    fn filling_works_on_any_profile_value_without_code_per_value() {
        let flat = other_values();
        let mut values = ProfileValues::default();
        let mut stored = serde_json::to_value(&values).expect("serializes");
        let stored = stored.as_object_mut().expect("an object");
        stored.remove("selected_model");
        stored.remove("context_char_budget");
        assert!(fill_missing_values(&mut values, stored, &flat));
        assert_eq!(values.selected_model, flat.selected_model);
        assert_eq!(values.context_char_budget, flat.context_char_budget);
        assert_eq!(
            values.selected_language,
            ProfileValues::default().selected_language
        );
        assert!(!fill_missing_values(
            &mut values,
            serde_json::to_value(&flat).unwrap().as_object().unwrap(),
            &ProfileValues::default()
        ));
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
        // Application settings stay.
        assert_eq!(
            s.word_correction_threshold,
            before.word_correction_threshold
        );
        assert_eq!(s.app_language, before.app_language);
        assert_eq!(s.debug_mode, before.debug_mode);
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
    fn switching_reports_a_new_model() {
        let mut s = two_profiles();
        let changes = switch_to(&mut s, "work", false).expect("switched");
        assert_eq!(
            changes,
            SwitchChanges {
                model_changed: true,
            }
        );
        let mut s = two_profiles();
        s.profiles[1].values.selected_model = s.selected_model.clone();
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

    // ---- Shortcuts and end shortcuts (idea 3) ------------------------------

    #[test]
    fn shortcuts_and_end_shortcuts_are_profile_values() {
        let mut s = two_profiles();
        let own = s.bindings["transcribe"].current_binding.clone();
        switch_to(&mut s, "work", false).expect("switched");
        assert_eq!(s.bindings["transcribe"].current_binding, "option+space");
        assert_eq!(s.end_keys, crate::end_keys::builtin(r"\n\n", true));
        assert_eq!(
            s.default_ends.get("transcribe").map(String::as_str),
            Some(crate::end_keys::POST_PROCESS_ID)
        );
        switch_to(&mut s, DEFAULT_PROFILE_ID, false).expect("switched back");
        assert_eq!(s.bindings["transcribe"].current_binding, own);
        assert_eq!(s.end_keys, crate::end_keys::default_end_keys());
    }

    #[test]
    fn append_text_and_send_context_are_no_profile_values_any_more() {
        let stored = serde_json::to_value(ProfileValues::default()).unwrap();
        for gone in ["append_text", "send_context"] {
            assert!(
                stored.get(gone).is_none(),
                "{gone} is still a profile value"
            );
        }
        for kept in ["bindings", "end_keys", "default_ends"] {
            assert!(stored.get(kept).is_some(), "{kept} is no profile value");
        }
    }

    #[test]
    fn every_profile_gets_every_shortcut_the_settings_have() {
        let mut s = two_profiles();
        s.profiles[1].values.bindings.remove("cycle_word_list");
        assert!(ensure_profiles(&mut s));
        assert_eq!(
            s.profiles[1].values.bindings.get("cycle_word_list"),
            s.bindings.get("cycle_word_list")
        );
        // Its own ones stay.
        assert_eq!(
            s.profiles[1].values.bindings["transcribe"].current_binding,
            "option+space"
        );
        assert!(!ensure_profiles(&mut s));
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
