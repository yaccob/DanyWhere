//! DanyWhere: which settings belong to a profile and which to the app (the
//! user's decisions P6 and P11 of 2026-11-19).
//!
//! Every setting that concerns a dictation is a profile value with a
//! default: a field of [`ProfileValues`], mirrored by the flat field of
//! [`AppSettings`] with the same name. What concerns the machine, the app or
//! the state of the interface is an application setting, on one of the pages
//! of "Application Settings". Every field of [`AppSettings`] is in exactly
//! one class; a test fails when a field is in none or in two, e.g. after a
//! Handy merge adds one, so it is classified on purpose.
//!
//! The profile values are not listed here: they are the serde fields of
//! [`ProfileValues`], so a field moves into a profile by being added there.

// The lists are for the settings pages to come (strands E1 and E2); until
// then only the tests read them.
#![cfg_attr(not(test), allow(dead_code))]

#[cfg(test)]
use crate::profiles::ProfileValues;
#[cfg(test)]
use crate::settings::AppSettings;

/// What a field of [`AppSettings`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingClass {
    /// A profile value: a field of [`ProfileValues`] with the same name, or
    /// a struct whose parts are profile values ([`NESTED_PROFILE_FIELDS`]).
    Profile,
    /// An application setting, on its page.
    App(AppPage),
    /// Concerns a dictation, but is still the same for every profile: it
    /// becomes a profile value with other work (the shortcuts with strand D).
    Global,
    /// Shared by all profiles, which choose from it (e.g. a collection of
    /// prompts), not a profile value of its own.
    Shared,
    /// The profiles themselves and how one is chosen.
    Profiles,
    /// State the app keeps for itself and never shows as a setting.
    Internal,
    /// Read only by a migration; another field replaced it.
    Legacy,
}

/// The pages of "Application Settings" (P11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppPage {
    General,
    Updates,
    Storage,
    Accessibility,
    Debug,
}

/// Application settings by page.
pub const APP_SETTINGS: &[(&str, AppPage)] = &[
    ("autostart_enabled", AppPage::General),
    ("start_hidden", AppPage::General),
    ("show_tray_icon", AppPage::General),
    ("app_language", AppPage::General),
    ("theme", AppPage::General),
    ("update_checks_enabled", AppPage::Updates),
    ("show_whats_new_on_update", AppPage::Updates),
    ("recording_storage_limit", AppPage::Storage),
    ("model_unload_timeout", AppPage::Storage),
    ("accessibility_skipped", AppPage::Accessibility),
    ("keyboard_implementation", AppPage::Accessibility),
    ("dictation_key_inserts", AppPage::Accessibility),
    ("dictation_service_when_idle", AppPage::Accessibility),
    ("debug_mode", AppPage::Debug),
    ("log_level", AppPage::Debug),
    ("experimental_enabled", AppPage::Debug),
    ("hold_threshold_ms", AppPage::Debug),
    ("paste_delay_ms", AppPage::Debug),
    ("paste_delay_after_ms", AppPage::Debug),
    ("reliable_paste", AppPage::Debug),
    ("word_correction_threshold", AppPage::Debug),
    ("extra_recording_buffer_ms", AppPage::Debug),
    ("lazy_stream_close", AppPage::Debug),
    ("transcribe_accelerator", AppPage::Debug),
    ("transcribe_gpu_device", AppPage::Debug),
    ("vad_backend", AppPage::Debug),
    ("model_list_timeout_secs", AppPage::Debug),
];

/// Fields that concern a dictation but stay the same for every profile
/// until other work makes them profile values.
pub const GLOBAL: &[&str] = &[
    // Strand D makes the shortcuts profile values (a map by id).
    "bindings",
];

/// Fields shared by all profiles, which choose from them.
pub const SHARED: &[&str] = &[
    // The prompt library; each profile's `post_process_prompt_order` says
    // which of them run, in what order (`prompt_library`).
    "post_process_prompts",
];

/// The profiles and how one is chosen.
pub const PROFILES: &[&str] = &["profiles", "active_profile_id", "profile_auto_by_app"];

/// State the app keeps for itself.
pub const INTERNAL: &[&str] = &[
    "settings_schema_version",
    "onboarding_completed",
    "insert_service_shortcut_set",
    "dictation_service_shortcuts_written_at",
    "whats_new_last_seen_version",
    "history_box_height_newest",
    "history_box_height_other",
];

/// Fields only a migration reads: the storage limit replaced the first two,
/// the "Append" field (`append_text`) the next three, the prompt order
/// (`prompt_library`) Handy's selected prompt.
pub const LEGACY: &[&str] = &[
    "history_limit",
    "recording_retention_period",
    "auto_submit",
    "auto_submit_key",
    "append_trailing_space",
    "post_process_selected_prompt_id",
];

/// Flat fields that are a struct whose parts are profile values of other
/// names, with those names. `profiles::ProfileValues` mirrors them by hand.
pub const NESTED_PROFILE_FIELDS: &[(&str, &[&str])] = &[(
    "dictation_context",
    &[
        "context_age_limit_minutes",
        "context_max_entries",
        "context_char_budget",
        "context_excluded_apps",
    ],
)];

/// Every class `field` is listed in, in the lists above or as a profile
/// value (`profile_values`: the serde names of [`ProfileValues`]). A sound
/// classification gives exactly one.
pub fn classes_of(field: &str, profile_values: &[String]) -> Vec<SettingClass> {
    let mut classes = Vec::new();
    if profile_values.iter().any(|name| name == field)
        || NESTED_PROFILE_FIELDS.iter().any(|(name, _)| *name == field)
    {
        classes.push(SettingClass::Profile);
    }
    for (name, page) in APP_SETTINGS {
        if *name == field {
            classes.push(SettingClass::App(*page));
        }
    }
    let lists: [(&[&str], SettingClass); 5] = [
        (GLOBAL, SettingClass::Global),
        (SHARED, SettingClass::Shared),
        (PROFILES, SettingClass::Profiles),
        (INTERNAL, SettingClass::Internal),
        (LEGACY, SettingClass::Legacy),
    ];
    for (list, class) in lists {
        if list.contains(&field) {
            classes.push(class);
        }
    }
    classes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The serde names of a struct's fields.
    fn field_names<T: serde::Serialize>(value: &T) -> Vec<String> {
        match serde_json::to_value(value).expect("serializes") {
            serde_json::Value::Object(map) => map.keys().cloned().collect(),
            other => panic!("not a struct: {other}"),
        }
    }

    fn profile_value_names() -> Vec<String> {
        field_names(&ProfileValues::default())
    }

    #[test]
    fn every_setting_is_in_exactly_one_class() {
        let profile_values = profile_value_names();
        let wrong: Vec<String> = field_names(&AppSettings::default())
            .into_iter()
            .filter_map(|field| {
                let classes = classes_of(&field, &profile_values);
                (classes.len() != 1).then(|| format!("{field}: {classes:?}"))
            })
            .collect();
        assert!(
            wrong.is_empty(),
            "Settings in no class or in more than one (classify them in \
             setting_classes.rs, or make them profile values):\n{}",
            wrong.join("\n")
        );
    }

    #[test]
    fn every_listed_name_is_a_setting() {
        let settings = field_names(&AppSettings::default());
        let listed = APP_SETTINGS
            .iter()
            .map(|(name, _)| *name)
            .chain(GLOBAL.iter().copied())
            .chain(SHARED.iter().copied())
            .chain(PROFILES.iter().copied())
            .chain(INTERNAL.iter().copied())
            .chain(LEGACY.iter().copied())
            .chain(NESTED_PROFILE_FIELDS.iter().map(|(name, _)| *name));
        let unknown: Vec<&str> = listed
            .filter(|name| !settings.iter().any(|field| field == name))
            .collect();
        assert!(unknown.is_empty(), "Not settings: {unknown:?}");
    }

    #[test]
    fn every_profile_value_mirrors_a_setting() {
        let settings = field_names(&AppSettings::default());
        let nested: Vec<&str> = NESTED_PROFILE_FIELDS
            .iter()
            .flat_map(|(_, parts)| parts.iter().copied())
            .collect();
        let orphans: Vec<String> = profile_value_names()
            .into_iter()
            .filter(|name| {
                !settings.iter().any(|field| field == name) && !nested.contains(&name.as_str())
            })
            .collect();
        assert!(
            orphans.is_empty(),
            "Profile values without a flat field: {orphans:?}"
        );
    }

    #[test]
    fn every_part_of_the_context_settings_is_a_profile_value() {
        let parts = field_names(&crate::dictation_context::DictationContextSettings::default());
        let (_, mirrored) = NESTED_PROFILE_FIELDS
            .iter()
            .find(|(name, _)| *name == "dictation_context")
            .expect("listed");
        assert_eq!(parts.len(), mirrored.len(), "{parts:?} vs {mirrored:?}");
        let values = profile_value_names();
        for name in mirrored.iter() {
            assert!(values.iter().any(|value| value == name), "{name}");
        }
    }

    /// The settings that became profile values on 2026-11-19 (P6): they
    /// concern a dictation, so a profile may have its own.
    #[test]
    fn what_concerns_a_dictation_is_a_profile_value() {
        let values = profile_value_names();
        let expected = [
            "shortcut_activation",
            "audio_feedback",
            "audio_feedback_volume",
            "sound_theme",
            "done_sound",
            "mute_while_recording",
            "always_on_microphone",
            "selected_microphone",
            "selected_channel",
            "clamshell_microphone",
            "selected_output_device",
            "airpods_mode",
            "overlay_position",
            "overlay_style",
            "paste_method",
            "clipboard_handling",
            "clipboard_notice_ms",
            "typing_tool",
            "external_script_path",
            "custom_words",
            "context_excluded_apps",
            "post_process_providers",
            "post_process_api_keys",
            "post_process_timeout_secs",
            "vad_enabled",
            "long_form_chunking",
        ];
        let missing: Vec<&str> = expected
            .iter()
            .copied()
            .filter(|name| !values.iter().any(|value| value == name))
            .collect();
        assert!(missing.is_empty(), "Not profile values yet: {missing:?}");
    }

    #[test]
    fn a_field_in_two_lists_is_reported_twice() {
        let values = vec!["selected_model".to_string()];
        assert_eq!(
            classes_of("selected_model", &values),
            vec![SettingClass::Profile]
        );
        assert_eq!(
            classes_of("debug_mode", &values),
            vec![SettingClass::App(AppPage::Debug)]
        );
        assert!(classes_of("not_a_setting", &values).is_empty());
        let mut twice = values.clone();
        twice.push("debug_mode".to_string());
        assert_eq!(classes_of("debug_mode", &twice).len(), 2);
    }
}
