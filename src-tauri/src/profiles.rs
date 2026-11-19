//! DanyWhere: profiles. The values that concern a dictation (the user's
//! decision P6 of 2026-11-19; [`ProfileValues`]: speech model, languages,
//! word lists, post-processing with its providers, keys and which prompts
//! run in what order, context, filler words, microphone, sounds, overlay,
//! how the text is inserted, and the shortcuts with the end shortcuts,
//! `end_keys`; user, T1) have defaults (`AppSettings::default_values`, the
//! "Grundeinstellungen"), and a profile overrides what it needs: each of its
//! values is coupled to the default or its own (`profile_layers`, decisions
//! P1–P11). What concerns the machine, the app or the interface stays
//! global (`setting_classes`), and so do the prompts themselves, one library
//! for all (`prompt_library`).
//!
//! What is in use is a profile or the defaults (`active_profile_id ==
//! DEFAULT_PROFILE_ID`). The flat fields of [`AppSettings`] always hold its
//! effective values, so the code that reads settings stays as it is;
//! `settings::write_settings` sends a change of them where its value comes
//! from (`profile_layers::route_flat`), and switching loads the values of
//! another profile, or the defaults, into the flat fields.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Emitter};

use crate::profile_layers::OwnEntries;
use crate::prompt_library::PromptOrderEntry;
use crate::settings::{
    self, AppSettings, ChineseScript, ClipboardHandling, LanguageDetectionMode, OverlayPosition,
    OverlayStyle, PasteMethod, PostProcessProvider, SecretMap, ShortcutActivation, SoundTheme,
    TypingTool,
};
use crate::word_lists::WordList;

/// The id that names the defaults where a profile id is expected (in use,
/// switching, the tray); no profile has it. Until the switch to the
/// defaults it was the id of the profile "Standard".
pub const DEFAULT_PROFILE_ID: &str = "default";

/// How long the overlay shows which profile was switched to.
const SWITCH_NOTICE: std::time::Duration = std::time::Duration::from_millis(2000);

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type)]
pub struct Profile {
    pub id: String,
    /// Empty only in a store from before the defaults, where it was the
    /// profile "Standard", shown translated.
    pub name: String,
    pub values: ProfileValues,
    /// Applications this profile is used for automatically: bundle ids on
    /// macOS, executable names on Windows. Not used yet.
    #[serde(default)]
    pub apps: Vec<String>,
    /// The serde names of the fields of [`ProfileValues`] this profile holds
    /// as its own values; every other field is coupled to the defaults
    /// (`profile_layers`). Lists that deviate entry by entry are never named
    /// here: their entries are in `own_entries`.
    #[serde(default)]
    pub own: BTreeSet<String>,
    /// Per list field (`profile_layers::LIST_FIELDS`), the entries this
    /// profile changed or added and the defaults' entries it switched off.
    #[serde(default)]
    pub own_entries: BTreeMap<String, OwnEntries>,
}

impl Profile {
    /// A profile with `values` that follows the defaults in everything and
    /// is used for no application.
    pub fn new(id: impl Into<String>, name: impl Into<String>, values: ProfileValues) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            values,
            apps: Vec::new(),
            own: BTreeSet::new(),
            own_entries: BTreeMap::new(),
        }
    }
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
/// whose values are the flat ones, and on the defaults while they aren't in
/// use. Returns whether a profile or the defaults changed.
pub fn ensure_in_every_profile(
    settings: &mut AppSettings,
    ensure: impl Fn(&mut AppSettings) -> bool,
) -> bool {
    let mut profiles = std::mem::take(&mut settings.profiles);
    let mut defaults = settings.default_values.take();
    // Without the profiles and the defaults, so it's a small copy.
    let mut scratch = settings.clone();
    let mut changed = false;
    let in_use = settings.active_profile_id.as_str();
    let values = profiles
        .iter_mut()
        .filter(|profile| profile.id != in_use)
        .map(|profile| &mut profile.values)
        .chain(defaults.iter_mut().filter(|_| in_use != DEFAULT_PROFILE_ID));
    for values in values {
        values.apply_to(&mut scratch);
        if ensure(&mut scratch) {
            *values = ProfileValues::from_settings(&scratch);
            changed = true;
        }
    }
    settings.profiles = profiles;
    settings.default_values = defaults;
    changed
}

/// What a switch changed that needs more than storing the settings. The
/// shortcuts follow every switch (`shortcut::profile_sync`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SwitchChanges {
    /// The speech model differs, so it has to be loaded.
    pub model_changed: bool,
}

/// The profile in use, whose values the flat fields hold; `None` while the
/// defaults are in use (P2).
pub fn active_profile(settings: &AppSettings) -> Option<&Profile> {
    settings
        .profiles
        .iter()
        .find(|profile| profile.id == settings.active_profile_id)
}

/// The defaults ("Grundeinstellungen", contract V3 of the build plan): the
/// values every profile's coupled fields follow and that apply while no
/// profile is in use. Before the switch (`profile_layers::migrate_to_defaults`)
/// they are the values of the profile "Standard".
pub fn default_values(settings: &AppSettings) -> ProfileValues {
    if let Some(values) = &settings.default_values {
        return values.clone();
    }
    match settings
        .profiles
        .iter()
        .find(|profile| profile.id == DEFAULT_PROFILE_ID)
    {
        Some(profile) if profile.id != settings.active_profile_id => profile.values.clone(),
        _ => ProfileValues::from_settings(settings),
    }
}

/// The name to show for `profile`: its own, or `default_name` while it has
/// none (only a store from before the defaults had one, "Standard").
pub fn display_name(profile: &Profile, default_name: &str) -> String {
    if profile.name.trim().is_empty() {
        default_name.to_string()
    } else {
        profile.name.clone()
    }
}

/// The shown name of profile `id`, or `default_name` (the translated name
/// of the defaults) for the defaults; `None` without such a profile.
pub fn name_of(settings: &AppSettings, id: &str, default_name: &str) -> Option<String> {
    if id == DEFAULT_PROFILE_ID {
        return Some(default_name.to_string());
    }
    settings
        .profiles
        .iter()
        .find(|profile| profile.id == id)
        .map(|profile| display_name(profile, default_name))
}

/// The values of profile `id`, or of the defaults: for what is in use the
/// flat fields, which hold its latest values; `None` without such a profile.
pub fn values_of(settings: &AppSettings, id: &str) -> Option<ProfileValues> {
    if id == settings.active_profile_id {
        return Some(ProfileValues::from_settings(settings));
    }
    if id == DEFAULT_PROFILE_ID {
        return Some(default_values(settings));
    }
    settings
        .profiles
        .iter()
        .find(|profile| profile.id == id)
        .map(|profile| profile.values.clone())
}

/// Test fixtures elsewhere: profile `id` named `name` with the defaults'
/// values changed by `change`, holding what then differs as its own (as the
/// switch and an import couple it).
#[cfg(test)]
pub fn profile_for_test(
    settings: &AppSettings,
    id: &str,
    name: &str,
    change: impl FnOnce(&mut ProfileValues),
) -> Profile {
    let defaults = default_values(settings);
    let mut values = defaults.clone();
    change(&mut values);
    let mut profile = Profile::new(id, name, values);
    crate::profile_layers::couple_equal(&mut profile, &defaults);
    profile
}

/// Makes the profiles consistent, e.g. after a read from the store: the
/// one-time switch to the defaults (`profile_layers::migrate_to_defaults`),
/// the defaults in use when the id in use names no profile, the flat values
/// (the latest of what is in use) sent where they belong
/// (`profile_layers::route_flat`), and every prompt order (the flat one,
/// the defaults' and every profile's) covering the library
/// (`prompt_library::normalize_orders`). Returns whether anything changed.
pub fn ensure_profiles(settings: &mut AppSettings) -> bool {
    let mut changed = crate::profile_layers::migrate_to_defaults(settings);
    if settings.active_profile_id != DEFAULT_PROFILE_ID && active_profile(settings).is_none() {
        // The flat values belong to no profile (a hand-edited store); the
        // defaults win.
        log::warn!(
            "Profile '{}' in use not found; the defaults are in use",
            settings.active_profile_id
        );
        settings.active_profile_id = DEFAULT_PROFILE_ID.to_string();
        default_values(settings).apply_to(settings);
        changed = true;
    }
    if crate::profile_layers::route_flat(settings) {
        changed = true;
    }
    if crate::prompt_library::normalize_orders(settings) {
        crate::profile_layers::propagate(settings);
        changed = true;
    }
    // Every profile has every shortcut: a binding added to the flat values
    // since (a new default binding) is a new entry, which `route_flat` puts
    // into the defaults and so into every profile that doesn't hold it
    // switched off (`profile_layers`, bindings entry by entry); an imported
    // profile gets the defaults' shortcuts it lacks
    // (`profile_transfer::prepare_import_values`).
    changed
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

/// Puts profile `id` in use, or the defaults for `DEFAULT_PROFILE_ID` (P2):
/// the flat values go where they belong first, then the values of `id` into
/// the flat fields. Refused while a dictation runs (`busy`), so no dictation
/// mixes two profiles' values.
pub fn switch_to(
    settings: &mut AppSettings,
    id: &str,
    busy: bool,
) -> Result<SwitchChanges, String> {
    if busy {
        return Err("A profile can't be switched during a dictation".to_string());
    }
    if id != DEFAULT_PROFILE_ID {
        profile_index(settings, id)?;
    }
    crate::profile_layers::route_flat(settings);
    let target = values_of(settings, id).unwrap_or_else(|| default_values(settings));
    settings.active_profile_id = id.to_string();
    let changes = SwitchChanges {
        model_changed: target.selected_model != settings.selected_model,
    };
    target.apply_to(settings);
    Ok(changes)
}

/// A new profile named `name` with a new id and no applications, not put in
/// use (P9): from the defaults (`DEFAULT_PROFILE_ID`) it follows them in
/// everything; from a profile it keeps that one's own values and coupling.
pub fn duplicate(
    settings: &mut AppSettings,
    source_id: &str,
    name: &str,
    now_millis: i64,
) -> Result<Profile, String> {
    let name = clean_name(name)?;
    let source = if source_id == DEFAULT_PROFILE_ID {
        None
    } else {
        Some(profile_index(settings, source_id)?)
    };
    // What is in use has its latest values in the flat fields.
    crate::profile_layers::route_flat(settings);
    let base = format!("profile_{now_millis}");
    let mut id = base.clone();
    let mut suffix = 1;
    while settings.profiles.iter().any(|profile| profile.id == id) {
        suffix += 1;
        id = format!("{base}_{suffix}");
    }
    let profile = match source {
        None => Profile::new(id, name, default_values(settings)),
        Some(index) => {
            let source = &settings.profiles[index];
            Profile {
                own: source.own.clone(),
                own_entries: source.own_entries.clone(),
                ..Profile::new(id, name, source.values.clone())
            }
        }
    };
    settings.profiles.push(profile.clone());
    Ok(profile)
}

/// Renames profile `id`; the defaults keep their fixed, translated name.
pub fn rename(settings: &mut AppSettings, id: &str, name: &str) -> Result<(), String> {
    if id == DEFAULT_PROFILE_ID {
        return Err("The defaults keep their name".to_string());
    }
    let index = profile_index(settings, id)?;
    settings.profiles[index].name = clean_name(name)?;
    Ok(())
}

/// Deletes profile `id`, the last one too; the defaults can't be deleted.
/// Deleting the profile in use puts the defaults in use first, which is
/// refused while a dictation runs. Returns the changes of that switch, if
/// any.
pub fn delete(
    settings: &mut AppSettings,
    id: &str,
    busy: bool,
) -> Result<Option<SwitchChanges>, String> {
    if id == DEFAULT_PROFILE_ID {
        return Err("The defaults can't be deleted".to_string());
    }
    profile_index(settings, id)?;
    let mut switched = None;
    if settings.active_profile_id == id {
        switched = Some(switch_to(settings, DEFAULT_PROFILE_ID, busy)?);
    }
    settings.profiles.retain(|profile| profile.id != id);
    Ok(switched)
}

/// The translated name of the defaults ("Grundeinstellungen"), in the
/// interface language.
pub fn default_profile_name(settings: &AppSettings) -> String {
    crate::tray_i18n::get_tray_translations(Some(settings.app_language.clone())).default_settings
}

/// What putting a profile (or the defaults) in use needs beyond storing:
/// everything a change of the values in use needs (`profile_effects::apply`:
/// model, post-processing shortcuts, microphone, overlay, window, tray),
/// and the name, with the glyph of choosing the profile by application, in
/// the overlay unless a dictation owns it.
fn after_switch(app: &AppHandle, before: &AppSettings, settings: &AppSettings) {
    crate::profile_effects::apply(app, before, settings);
    if crate::overlay::is_session_showing() {
        return;
    }
    if let Some(shown) = crate::app_profiles::overlay_profile(
        settings,
        &settings.active_profile_id,
        &default_profile_name(settings),
    ) {
        crate::overlay::show_profile_notice(app, shown, SWITCH_NOTICE);
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

/// Puts profile `id` (or the defaults) in use with everything that goes
/// with it.
fn switch_with_effects(app: &AppHandle, id: &str) -> Result<(), String> {
    let busy = crate::tray::dictation_in_progress(app);
    let before = settings::get_settings(app);
    let mut settings = before.clone();
    switch_to(&mut settings, id, busy)?;
    log::info!("Switched to profile {id}");
    settings::write_settings(app, settings.clone());
    after_switch(app, &before, &settings);
    Ok(())
}

/// Puts profile `id` (or the defaults) in use from the tray.
pub fn select_from_tray(app: &AppHandle, id: &str) -> Result<(), String> {
    switch_with_effects(app, id)
}

/// Puts profile `id` in use, or the defaults for `"default"` (P2).
#[tauri::command]
#[specta::specta]
pub fn switch_profile(app: AppHandle, id: String) -> Result<(), String> {
    switch_with_effects(&app, &id)
}

/// A copy of profile `source_id` that keeps its own values and their
/// coupling, or, from `"default"`, a new profile that follows the defaults
/// (P9). It isn't put in use.
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

/// A new profile named `name` that follows the defaults in everything (P9).
/// It isn't put in use.
#[tauri::command]
#[specta::specta]
pub fn create_profile(app: AppHandle, name: String) -> Result<Profile, String> {
    duplicate_profile(app, DEFAULT_PROFILE_ID.to_string(), name)
}

#[tauri::command]
#[specta::specta]
pub fn rename_profile(app: AppHandle, id: String, name: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    rename(&mut settings, &id, &name)?;
    save(&app, settings);
    Ok(())
}

/// Deletes profile `id`; deleting the one in use puts the defaults in use.
#[tauri::command]
#[specta::specta]
pub fn delete_profile(app: AppHandle, id: String) -> Result<(), String> {
    let busy = crate::tray::dictation_in_progress(&app);
    let before = settings::get_settings(&app);
    let mut settings = before.clone();
    let switched = delete(&mut settings, &id, busy)?;
    settings::write_settings(&app, settings.clone());
    if switched.is_some() {
        after_switch(&app, &before, &settings);
    } else {
        crate::profile_effects::apply(&app, &before, &settings);
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
    use crate::profile_layers::couple_equal;

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

    /// `settings_in_use` as the defaults, in use, and a profile "work"
    /// holding `other_values` as its own.
    fn two_profiles() -> AppSettings {
        let mut s = settings_in_use();
        ensure_profiles(&mut s);
        let mut work = Profile::new("work", "Work", other_values());
        couple_equal(&mut work, &default_values(&s));
        s.profiles.push(work);
        s
    }

    fn work(s: &AppSettings) -> &Profile {
        s.profiles
            .iter()
            .find(|profile| profile.id == "work")
            .expect("work")
    }

    // ---- The switch to the defaults ---------------------------------------

    #[test]
    fn a_new_installation_has_the_defaults_in_use_and_no_profiles() {
        let mut s = AppSettings::default();
        assert!(ensure_profiles(&mut s));
        assert!(s.profiles.is_empty());
        assert_eq!(s.active_profile_id, DEFAULT_PROFILE_ID);
        assert_eq!(
            default_values(&s),
            ProfileValues::from_settings(&AppSettings::default())
        );
        assert!(active_profile(&s).is_none());
    }

    #[test]
    fn an_update_keeps_the_settings_in_use_as_the_defaults() {
        let mut s = settings_in_use();
        assert!(ensure_profiles(&mut s));
        let values = default_values(&s);
        assert_eq!(values.selected_model, "parakeet");
        assert_eq!(values.selected_language, "de");
        assert_eq!(
            values.post_process_prompt_order,
            vec![entry("p1", true), entry("p9", false)]
        );
        assert_eq!(values.word_lists, vec![list("code", &["Rust", "Tauri"])]);
        assert_eq!(values.active_word_list_id.as_deref(), Some("code"));
        assert!(s.profiles.is_empty());
    }

    #[test]
    fn making_the_profiles_consistent_twice_changes_nothing_the_second_time() {
        let mut s = two_profiles();
        ensure_profiles(&mut s);
        let once = s.clone();
        assert!(!ensure_profiles(&mut s));
        assert_eq!(s.profiles, once.profiles);
        assert_eq!(s.default_values, once.default_values);
    }

    #[test]
    fn an_unknown_profile_in_use_means_the_defaults() {
        let mut s = two_profiles();
        s.active_profile_id = "gone".to_string();
        s.selected_language = "fr".to_string();
        assert!(ensure_profiles(&mut s));
        assert_eq!(s.active_profile_id, DEFAULT_PROFILE_ID);
        // The flat values belonged to no profile: the defaults win.
        assert_eq!(s.selected_language, "de");
    }

    #[test]
    fn every_prompt_order_covers_the_library() {
        let mut s = two_profiles();
        let index = s.profiles.iter().position(|p| p.id == "work").unwrap();
        s.profiles[index].values.post_process_prompt_order = vec![entry("gone", true)];
        assert!(ensure_profiles(&mut s));
        assert_eq!(
            work(&s).values.post_process_prompt_order,
            vec![entry("p1", false), entry("p9", false)]
        );
        assert_eq!(
            default_values(&s).post_process_prompt_order.len(),
            s.post_process_prompts.len()
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
        assert!(profile.own.is_empty());
        assert!(profile.own_entries.is_empty());
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
        let index = s.profiles.iter().position(|p| p.id == "work").unwrap();
        s.profiles[index].values = values_with(&other_values(), extra.clone());
        let defaults = default_values(&s);
        couple_equal(&mut s.profiles[index], &defaults);
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
    fn a_changed_microphone_goes_where_its_value_comes_from() {
        let mut s = two_profiles();
        let fresh = duplicate(&mut s, DEFAULT_PROFILE_ID, "Fresh", 1).expect("a profile");
        // The defaults in use: into the defaults and on to the profile that
        // follows them; "work" keeps its own microphone ("USB Mic").
        s.selected_microphone = Some("AirPods".to_string());
        s.custom_words = strings(&["Tauri"]);
        crate::profile_layers::route_flat(&mut s);
        let defaults = default_values(&s);
        assert_eq!(value_of(&defaults, "selected_microphone"), "AirPods");
        assert_eq!(
            value_of(&defaults, "custom_words"),
            serde_json::json!(["Tauri"])
        );
        let fresh = s.profiles.iter().find(|p| p.id == fresh.id).unwrap();
        assert_eq!(value_of(&fresh.values, "selected_microphone"), "AirPods");
        assert_eq!(value_of(&work(&s).values, "selected_microphone"), "USB Mic");
        // "work" in use: its own microphone changes there only.
        switch_to(&mut s, "work", false).expect("switched");
        s.selected_microphone = Some("Desk Mic".to_string());
        crate::profile_layers::route_flat(&mut s);
        assert_eq!(
            value_of(&work(&s).values, "selected_microphone"),
            "Desk Mic"
        );
        assert_eq!(
            value_of(&default_values(&s), "selected_microphone"),
            "AirPods"
        );
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

    // ---- Changes go where the value comes from ----------------------------

    #[test]
    fn a_change_of_an_own_value_lands_in_the_profile_in_use_only() {
        let mut s = two_profiles();
        switch_to(&mut s, "work", false).expect("switched");
        s.selected_language = "it".to_string();
        s.post_process_prompt_order = vec![entry("p9", true), entry("p1", true)];
        crate::profile_layers::route_flat(&mut s);
        assert_eq!(work(&s).values.selected_language, "it");
        assert_eq!(
            work(&s).values.post_process_prompt_order,
            vec![entry("p9", true), entry("p1", true)]
        );
        assert_eq!(default_values(&s).selected_language, "de");
    }

    // ---- Switching --------------------------------------------------------

    #[test]
    fn switching_swaps_every_profile_value_and_nothing_else() {
        let mut s = two_profiles();
        // A change not yet stored belongs to what is being left: the defaults.
        s.selected_language = "it".to_string();
        let before = s.clone();
        switch_to(&mut s, "work", false).expect("switched");
        assert_eq!(s.active_profile_id, "work");
        assert_eq!(active_profile(&s).map(|p| p.id.as_str()), Some("work"));
        let mut expected = other_values();
        // Work's own maps follow the defaults in the providers it has no
        // entry of its own for.
        expected.post_process_models = work(&s).values.post_process_models.clone();
        assert_eq!(ProfileValues::from_settings(&s), expected);
        assert_eq!(s.post_process_models["openai"], "gpt-x");
        let left = default_values(&s);
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
        assert_eq!(s.active_profile_id, DEFAULT_PROFILE_ID);
        assert!(active_profile(&s).is_none());
        assert_eq!(s.selected_language, "it");
        assert_eq!(s.selected_model, "parakeet");
        assert_eq!(work(&s).values.selected_language, "en");
    }

    #[test]
    fn the_effort_belongs_to_the_profile() {
        let mut s = two_profiles();
        s.post_process_efforts
            .insert("anthropic".to_string(), "xhigh".to_string());
        crate::profile_layers::route_flat(&mut s);
        assert_eq!(
            default_values(&s).post_process_efforts.get("anthropic"),
            Some(&"xhigh".to_string())
        );
        switch_to(&mut s, "work", false).expect("switched");
        assert_eq!(
            s.post_process_efforts.get("openai"),
            Some(&"high".to_string())
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
        crate::profile_layers::reset_values(&mut s, "work", &strings(&["selected_model"]))
            .expect("reset");
        let changes = switch_to(&mut s, "work", false).expect("switched");
        assert_eq!(changes, SwitchChanges::default());
    }

    #[test]
    fn switching_during_a_dictation_is_refused_and_changes_nothing() {
        let mut s = two_profiles();
        let before = s.clone();
        assert!(switch_to(&mut s, "work", true).is_err());
        assert!(switch_to(&mut s, DEFAULT_PROFILE_ID, true).is_err());
        assert_eq!(s.active_profile_id, before.active_profile_id);
        assert_eq!(s.selected_model, before.selected_model);
    }

    #[test]
    fn switching_to_an_unknown_profile_fails() {
        let mut s = two_profiles();
        assert!(switch_to(&mut s, "gone", false).is_err());
        assert_eq!(s.active_profile_id, DEFAULT_PROFILE_ID);
    }

    // ---- New, duplicating, renaming, deleting -----------------------------

    #[test]
    fn a_new_profile_follows_the_defaults_in_everything() {
        let mut s = two_profiles();
        let copy = duplicate(&mut s, DEFAULT_PROFILE_ID, "  Arbeit  ", 42).expect("a profile");
        assert_eq!(copy.name, "Arbeit");
        assert!(copy.apps.is_empty());
        assert_ne!(copy.id, DEFAULT_PROFILE_ID);
        assert!(copy.own.is_empty());
        assert!(copy.own_entries.is_empty());
        assert_eq!(copy.values, default_values(&s));
        assert_eq!(s.profiles.len(), 2);
        assert_eq!(s.active_profile_id, DEFAULT_PROFILE_ID);

        // A change of a coupled order while the new profile is in use
        // changes the defaults' order, which it follows.
        switch_to(&mut s, &copy.id, false).expect("switched");
        s.post_process_prompt_order = vec![entry("p9", true), entry("p1", false)];
        crate::profile_layers::route_flat(&mut s);
        assert_eq!(
            default_values(&s).post_process_prompt_order,
            vec![entry("p9", true), entry("p1", false)]
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
    fn a_duplicate_keeps_the_own_values_and_their_coupling() {
        let mut s = two_profiles();
        let index = s.profiles.iter().position(|p| p.id == "work").unwrap();
        s.profiles[index].apps = strings(&["com.apple.mail"]);
        let copy = duplicate(&mut s, "work", "Copy", 7).expect("a copy");
        assert_eq!(copy.values, work(&s).values);
        assert_eq!(copy.own, work(&s).own);
        assert_eq!(copy.own_entries, work(&s).own_entries);
        assert!(copy.apps.is_empty());
        // A coupled value of the copy follows the defaults; an own one stays.
        crate::profile_layers::reset_values(&mut s, &copy.id, &strings(&["selected_model"]))
            .expect("reset");
        crate::profile_layers::set_value(
            &mut s,
            &crate::profile_layers::Scope::Defaults,
            "selected_model",
            serde_json::json!("moonshine"),
        )
        .expect("set");
        let copy = s.profiles.iter().find(|p| p.id == copy.id).unwrap();
        assert_eq!(copy.values.selected_model, "moonshine");
        assert_eq!(copy.values.selected_language, "en");
        assert_eq!(work(&s).values.selected_model, "whisper-large");
    }

    #[test]
    fn a_new_profile_from_the_defaults_in_use_has_their_latest_values() {
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
        assert_eq!(s.profiles.len(), 1);
    }

    #[test]
    fn renaming_trims_needs_a_name_and_leaves_the_defaults_alone() {
        let mut s = two_profiles();
        rename(&mut s, "work", "  Büro ").expect("renamed");
        assert_eq!(work(&s).name, "Büro");
        assert!(rename(&mut s, "work", "  ").is_err());
        assert_eq!(work(&s).name, "Büro");
        assert!(rename(&mut s, DEFAULT_PROFILE_ID, "Privat").is_err());
        assert!(rename(&mut s, "gone", "Name").is_err());
    }

    #[test]
    fn the_last_profile_can_be_deleted_the_defaults_cannot() {
        let mut s = two_profiles();
        assert!(delete(&mut s, DEFAULT_PROFILE_ID, false).is_err());
        assert_eq!(delete(&mut s, "work", false), Ok(None));
        assert!(s.profiles.is_empty());
        assert_eq!(s.active_profile_id, DEFAULT_PROFILE_ID);
    }

    #[test]
    fn deleting_the_profile_in_use_falls_back_to_the_defaults() {
        let mut s = two_profiles();
        let mut third = work(&s).clone();
        third.id = "third".to_string();
        s.profiles.push(third);
        switch_to(&mut s, "work", false).expect("switched");
        let changes = delete(&mut s, "work", false).expect("deleted");
        assert!(changes.is_some());
        assert_eq!(s.active_profile_id, DEFAULT_PROFILE_ID);
        assert_eq!(s.selected_model, "parakeet");
        assert_eq!(
            s.profiles.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec!["third"]
        );
    }

    #[test]
    fn deleting_another_profile_keeps_the_one_in_use() {
        let mut s = two_profiles();
        let mut third = work(&s).clone();
        third.id = "third".to_string();
        s.profiles.push(third);
        switch_to(&mut s, "third", false).expect("switched");
        let changes = delete(&mut s, "work", false).expect("deleted");
        assert_eq!(changes, None);
        assert_eq!(s.active_profile_id, "third");
        assert_eq!(s.profiles.len(), 1);
        assert!(delete(&mut s, "gone", false).is_err());
    }

    #[test]
    fn deleting_the_profile_in_use_during_a_dictation_is_refused() {
        let mut s = two_profiles();
        switch_to(&mut s, "work", false).expect("switched");
        assert!(delete(&mut s, "work", true).is_err());
        assert_eq!(s.profiles.len(), 1);
        // A profile not in use may go.
        switch_to(&mut s, DEFAULT_PROFILE_ID, false).expect("switched back");
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
        let index = s.profiles.iter().position(|p| p.id == "work").unwrap();
        s.profiles[index].values.bindings.remove("cycle_word_list");
        assert!(ensure_profiles(&mut s));
        // A shortcut it follows the defaults in comes back from them.
        assert_eq!(
            work(&s).values.bindings.get("cycle_word_list"),
            s.bindings.get("cycle_word_list")
        );
        // Its own ones stay.
        assert_eq!(
            work(&s).values.bindings["transcribe"].current_binding,
            "option+space"
        );
        assert!(!ensure_profiles(&mut s));
    }

    // ---- Names ------------------------------------------------------------

    #[test]
    fn the_defaults_have_a_fixed_translated_name() {
        let s = two_profiles();
        assert_eq!(
            name_of(&s, DEFAULT_PROFILE_ID, "Grundeinstellungen").as_deref(),
            Some("Grundeinstellungen")
        );
        assert_eq!(
            name_of(&s, "work", "Grundeinstellungen").as_deref(),
            Some("Work")
        );
        assert_eq!(name_of(&s, "gone", "Grundeinstellungen"), None);
        assert_eq!(display_name(work(&s), "Grundeinstellungen"), "Work");
    }

    #[test]
    fn the_name_of_the_defaults_follows_the_interface_language() {
        let mut s = AppSettings {
            app_language: "de".to_string(),
            ..Default::default()
        };
        assert_eq!(default_profile_name(&s), "Grundeinstellungen");
        s.app_language = "en".to_string();
        assert_eq!(default_profile_name(&s), "Defaults");
    }

    #[test]
    fn the_values_of_the_defaults_or_a_profile() {
        let mut s = two_profiles();
        assert_eq!(values_of(&s, DEFAULT_PROFILE_ID), Some(default_values(&s)));
        assert_eq!(
            values_of(&s, "work").map(|v| v.selected_language),
            Some("en".to_string())
        );
        assert_eq!(values_of(&s, "gone"), None);
        // What is in use: its latest values are the flat ones.
        s.selected_language = "it".to_string();
        assert_eq!(
            values_of(&s, DEFAULT_PROFILE_ID).map(|v| v.selected_language),
            Some("it".to_string())
        );
    }
}
