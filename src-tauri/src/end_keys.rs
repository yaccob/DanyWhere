//! DanyWhere: end shortcuts (idea 3; user, 2026-11-19, F3 to F7).
//!
//! A recording ends with one of several end shortcuts. Each says what
//! follows: what is appended (the "Append" syntax of `append_text`), whether
//! the text is post-processed (with the profile's prompts), and whether the
//! post-processing gets earlier dictations as context. The list is a profile
//! value (`ProfileValues::end_keys`); every entry keeps its `id`, so the
//! profiles can share entries one by one.
//!
//! Out of the box there are two, "Plain" and "Post-process". Every start
//! shortcut has a default end from the list (`ProfileValues::default_ends`),
//! used by its second press and by its release when held: the dictation
//! shortcut ends "Plain", the post-processing one "Post-process", as both did
//! before. Further end shortcuts carry a key combination of their own, which
//! DanyWhere registers only while a recording runs, like Escape
//! (`shortcut::profile_sync`).
//!
//! "Send Context" as a profile value and the "Append" field went into these:
//! the first read of an older store gives every profile the two end
//! shortcuts with the Append text and the context switch it had
//! ([`migrate`]).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Emitter};

use crate::append_text::KeyPlatform;
use crate::profiles::ProfileValues;
use crate::settings::{self, AppSettings, ShortcutBinding};

/// The end shortcut without post-processing every profile starts with.
pub const PLAIN_ID: &str = "plain";
/// The end shortcut with post-processing every profile starts with.
pub const POST_PROCESS_ID: &str = "post_process";
/// The prefix of an end shortcut's binding id while it is registered.
pub const BINDING_PREFIX: &str = "end_key:";
/// The start shortcuts, each with a default end.
pub const START_BINDINGS: [&str; 2] = ["transcribe", "transcribe_with_post_process"];

/// One end shortcut.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type, Default)]
#[serde(default)]
pub struct EndKey {
    /// Stable, so profiles can share the entry; `plain` and `post_process`
    /// for the two every profile starts with.
    pub id: String,
    /// Empty for the two built-in ones, which show their translated name.
    pub name: String,
    /// The key combination that ends a recording this way; empty: none (it
    /// ends a recording only as a start shortcut's default end).
    pub shortcut: String,
    /// What is appended, in the "Append" syntax (`append_text`).
    pub append_text: String,
    /// Whether the text is post-processed (if the profile allows it).
    pub post_process: bool,
    /// Whether post-processing gets earlier dictations as context.
    pub send_context: bool,
}

/// The two end shortcuts every profile starts with, with `append_text` and
/// `send_context`: those of the profile before end shortcuts existed, or
/// none for a new installation.
pub fn builtin(append_text: &str, send_context: bool) -> Vec<EndKey> {
    [(PLAIN_ID, false), (POST_PROCESS_ID, true)]
        .into_iter()
        .map(|(id, post_process)| EndKey {
            id: id.to_string(),
            name: String::new(),
            shortcut: String::new(),
            append_text: append_text.to_string(),
            post_process,
            send_context,
        })
        .collect()
}

/// A new installation's end shortcuts (serde default of the settings).
pub fn default_end_keys() -> Vec<EndKey> {
    builtin("", false)
}

/// Whether `id` names one of the two built-in end shortcuts.
pub fn is_builtin(id: &str) -> bool {
    id == PLAIN_ID || id == POST_PROCESS_ID
}

/// The end shortcut a start shortcut ends with out of the box.
pub fn default_end_id(start_binding: &str) -> &'static str {
    if start_binding == START_BINDINGS[1] {
        POST_PROCESS_ID
    } else {
        PLAIN_ID
    }
}

/// A new installation's default ends (serde default of the settings).
pub fn default_ends() -> HashMap<String, String> {
    START_BINDINGS
        .into_iter()
        .map(|binding| (binding.to_string(), default_end_id(binding).to_string()))
        .collect()
}

/// How a recording ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndChoice {
    /// With the default end of this start shortcut (its second press, its
    /// release when held, its service).
    DefaultOf(String),
    /// With the end shortcut of this id.
    Key(String),
}

/// The end shortcut `id` in `values`.
fn find<'a>(values: &'a ProfileValues, id: &str) -> Option<&'a EndKey> {
    values.end_keys.iter().find(|end| end.id == id)
}

/// The end shortcut a dictation with `values` ends with by `choice`.
/// A pressed end shortcut the profile doesn't have (it changed meanwhile)
/// ends as the dictation shortcut's default end; a default end pointing at a
/// deleted entry falls back to the built-in default, and a profile without
/// that ends as the start shortcut did before end shortcuts existed.
pub fn resolve(values: &ProfileValues, choice: &EndChoice) -> EndKey {
    let start_binding = match choice {
        EndChoice::Key(id) => {
            if let Some(end) = find(values, id) {
                return end.clone();
            }
            log::warn!("End shortcut '{id}' not in the profile; ending as the dictation shortcut");
            START_BINDINGS[0]
        }
        EndChoice::DefaultOf(binding) => binding.as_str(),
    };
    let builtin_id = default_end_id(start_binding);
    values
        .default_ends
        .get(start_binding)
        .and_then(|id| find(values, id))
        .or_else(|| find(values, builtin_id))
        .cloned()
        .unwrap_or_else(|| EndKey {
            id: builtin_id.to_string(),
            post_process: builtin_id == POST_PROCESS_ID,
            ..EndKey::default()
        })
}

/// The default end of `start_binding` in `values`.
pub fn default_end(values: &ProfileValues, start_binding: &str) -> EndKey {
    resolve(values, &EndChoice::DefaultOf(start_binding.to_string()))
}

/// The end a text gets that no end shortcut chose ("Paste Last
/// Transcript", "Insert Last Dictation" and "Copy Last Transcript" outside a
/// dictation): the default end of the dictation shortcut.
pub fn plain_end(values: &ProfileValues) -> EndKey {
    default_end(values, START_BINDINGS[0])
}

/// The rank of start shortcut `start_binding`'s default end in the profile
/// that applies now (`app_profiles::profile_now`), for the coordinator.
pub fn default_end_rank_now(app: &AppHandle, start_binding: &str) -> u8 {
    let settings = settings::get_settings(app);
    let frontmost = crate::app_profiles::frontmost_if_choosing(&settings);
    let (_, values) = crate::app_profiles::profile_now(&settings, frontmost.as_deref());
    rank(&default_end(&values, start_binding))
}

/// How much an end does after the recording, to decide whether a start
/// shortcut's press ends a locked recording begun with another one: only one
/// that does more does (post-processing chosen up front is never lost).
pub fn rank(end: &EndKey) -> u8 {
    u8::from(end.post_process)
}

/// The binding id end shortcut `end_id` is registered under.
pub fn binding_id(end_id: &str) -> String {
    format!("{BINDING_PREFIX}{end_id}")
}

/// The end shortcut a registered binding id stands for.
pub fn end_id_of(binding_id: &str) -> Option<&str> {
    binding_id.strip_prefix(BINDING_PREFIX)
}

/// The end shortcuts of `end_keys` that have a key combination, as bindings
/// to register while a recording runs.
pub fn bindings_to_register(end_keys: &[EndKey]) -> Vec<ShortcutBinding> {
    end_keys
        .iter()
        .filter(|end| !end.shortcut.trim().is_empty())
        .map(|end| ShortcutBinding {
            id: binding_id(&end.id),
            name: end.name.clone(),
            description: String::new(),
            default_binding: end.shortcut.trim().to_string(),
            current_binding: end.shortcut.trim().to_string(),
        })
        .collect()
}

/// Checks a list of end shortcuts with the default ends that point into it:
/// ids present and unique, a name for every entry but the built-in ones, no
/// key combination twice, and every default end still in the list.
pub fn validate(end_keys: &[EndKey], default_ends: &HashMap<String, String>) -> Result<(), String> {
    let mut ids = std::collections::HashSet::new();
    let mut shortcuts = std::collections::HashSet::new();
    for end in end_keys {
        if end.id.trim().is_empty() {
            return Err("An end shortcut needs an id".to_string());
        }
        if !ids.insert(end.id.as_str()) {
            return Err(format!("End shortcut id '{}' twice", end.id));
        }
        if end.name.trim().is_empty() && !is_builtin(&end.id) {
            return Err(format!("end-key-needs-name:{}", end.id));
        }
        let shortcut = end.shortcut.trim();
        if !shortcut.is_empty() && !shortcuts.insert(shortcut) {
            return Err(format!("end-key-shortcut-twice:{shortcut}"));
        }
    }
    // In the order of the start shortcuts, so the error is always the same.
    for binding in START_BINDINGS {
        if let Some(end_id) = default_ends.get(binding) {
            if !ids.contains(end_id.as_str()) {
                return Err(format!("end-key-in-use:{end_id}"));
            }
        }
    }
    Ok(())
}

/// Run on every read of the store (`settings_value` is the stored JSON),
/// with this system's key names; see [`migrate_on`].
pub fn migrate(settings: &mut AppSettings, settings_value: &serde_json::Value) -> bool {
    migrate_on(settings, settings_value, KeyPlatform::current())
}

/// For the settings and every stored profile:
/// - once, a value without end shortcuts gets the two built-in ones with
///   the "Append" text (or Handy's trailing space and auto submit before
///   that) and the "Send Context" it had;
/// - once, a value without default ends gets the built-in ones, and a
///   profile without shortcuts gets the flat ones (those it used);
/// - the retired shortcuts "Transcribe with Context" and "Send Context" go;
/// - an Append text with another system's key names gets `platform`'s
///   (`append_text::localize`), on every read.
///
/// Returns whether anything changed, so a store with nothing to carry over
/// is never written.
pub fn migrate_on(
    settings: &mut AppSettings,
    settings_value: &serde_json::Value,
    platform: KeyPlatform,
) -> bool {
    let (auto_submit, key) = (settings.auto_submit, settings.auto_submit_key);
    let mut changed = false;
    // The settings: the active profile's values.
    let flat_end_keys = (settings_value.get("end_keys").is_none()).then(|| {
        legacy_end_keys(
            Some(settings_value),
            settings.append_trailing_space,
            auto_submit,
            key,
            platform,
        )
    });
    if let Some(end_keys) = flat_end_keys {
        if settings.end_keys != end_keys {
            log::info!("End shortcuts take over \"Append\" and \"Send Context\"");
            settings.end_keys = end_keys;
            changed = true;
        }
    }
    if settings_value.get("default_ends").is_none() && settings.default_ends != default_ends() {
        settings.default_ends = default_ends();
        changed = true;
    }
    changed |= drop_retired_bindings(&mut settings.bindings);

    let stored_profiles = settings_value
        .get("profiles")
        .and_then(|profiles| profiles.as_array());
    let flat_bindings = settings.bindings.clone();
    for profile in settings.profiles.iter_mut() {
        let stored_values = stored_profiles
            .and_then(|profiles| {
                profiles.iter().find(|stored| {
                    stored.get("id").and_then(|id| id.as_str()) == Some(profile.id.as_str())
                })
            })
            .and_then(|stored| stored.get("values"));
        let lacks = |field: &str| stored_values.is_none_or(|values| values.get(field).is_none());
        if lacks("end_keys") {
            let end_keys = legacy_end_keys(
                stored_values,
                crate::append_text::stored_trailing_space(stored_values),
                auto_submit,
                key,
                platform,
            );
            if profile.values.end_keys != end_keys {
                profile.values.end_keys = end_keys;
                changed = true;
            }
        }
        if lacks("default_ends") && profile.values.default_ends != default_ends() {
            profile.values.default_ends = default_ends();
            changed = true;
        }
        // The shortcuts it used were the flat ones (contract V5).
        if lacks("bindings") && profile.values.bindings != flat_bindings {
            profile.values.bindings = flat_bindings.clone();
            changed = true;
        }
        changed |= drop_retired_bindings(&mut profile.values.bindings);
    }

    let append_texts = settings
        .end_keys
        .iter_mut()
        .chain(
            settings
                .default_values
                .iter_mut()
                .flat_map(|values| values.end_keys.iter_mut()),
        )
        .chain(
            settings
                .profiles
                .iter_mut()
                .flat_map(|profile| profile.values.end_keys.iter_mut()),
        );
    for end in append_texts {
        let local = crate::append_text::localize(&end.append_text, platform);
        if end.append_text != local {
            log::info!(
                "\"Append\" takes this system's key names: {:?} → {local:?}",
                end.append_text
            );
            end.append_text = local;
            changed = true;
        }
    }
    changed
}

/// The two built-in end shortcuts for a stored object (the settings or a
/// profile's values) from before them: its "Append" text, or before that
/// its trailing space (`trailing_space`) and the global auto submit, and its
/// "Send Context".
fn legacy_end_keys(
    stored: Option<&serde_json::Value>,
    trailing_space: bool,
    auto_submit: bool,
    key: crate::settings::AutoSubmitKey,
    platform: KeyPlatform,
) -> Vec<EndKey> {
    let append =
        crate::append_text::stored_value(stored, trailing_space, auto_submit, key, platform);
    let send_context = stored
        .and_then(|values| values.get("send_context"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    builtin(&append, send_context)
}

/// The shortcuts that are gone: "Transcribe with Context" (0.1.6) and the
/// switch "Send Context" (0.2.0 to 0.2.5). Returns whether one was there.
fn drop_retired_bindings(bindings: &mut HashMap<String, ShortcutBinding>) -> bool {
    let mut dropped = false;
    for id in [
        crate::send_context::RETIRED_BINDING,
        crate::send_context::RETIRED_TOGGLE_BINDING,
    ] {
        dropped |= bindings.remove(id).is_some();
    }
    dropped
}

/// For an import (`file_text` is the file as read), with this system's key
/// names; see [`prepare_import_on`].
pub fn prepare_import(file_text: &str, values: &mut ProfileValues) {
    prepare_import_on(file_text, values, KeyPlatform::current());
}

/// For an import: a profile file from before end shortcuts gets them from its
/// "Append" field (or trailing space) and "Send Context"; a file without
/// shortcuts takes the importing installation's (an empty map, which
/// `profiles::ensure_profiles` fills); Append texts get `platform`'s key
/// names.
pub fn prepare_import_on(file_text: &str, values: &mut ProfileValues, platform: KeyPlatform) {
    if let Ok(file) = serde_saphyr::from_str::<serde_json::Value>(file_text) {
        let stored = file
            .get("profile")
            .and_then(|profile| profile.get("values"));
        let lacks = |field: &str| stored.is_none_or(|values| values.get(field).is_none());
        if lacks("end_keys") {
            // auto_submit was global, so the file holds no key press.
            values.end_keys = legacy_end_keys(
                stored,
                crate::append_text::stored_trailing_space(stored),
                false,
                crate::settings::AutoSubmitKey::Enter,
                platform,
            );
        }
        if lacks("default_ends") {
            values.default_ends = default_ends();
        }
        if lacks("bindings") {
            values.bindings.clear();
        }
    }
    drop_retired_bindings(&mut values.bindings);
    for end in values.end_keys.iter_mut() {
        end.append_text = crate::append_text::localize(&end.append_text, platform);
    }
}

/// Checks a key combination of an end shortcut against every other shortcut
/// in `bindings`; the error names the one it collides with, as
/// `change_binding` does (`conflict:<id>`).
pub fn check_against_bindings(
    shortcut: &str,
    bindings: &HashMap<String, ShortcutBinding>,
) -> Result<(), String> {
    let shortcut = shortcut.trim();
    let mut ids: Vec<&String> = bindings.keys().collect();
    ids.sort();
    match ids.into_iter().find(|id| {
        bindings[*id]
            .current_binding
            .trim()
            .eq_ignore_ascii_case(shortcut)
    }) {
        Some(id) => Err(format!("conflict:{id}")),
        None => Ok(()),
    }
}

/// Stores the active profile's end shortcuts (the whole list, as the editor
/// holds it). A running recording takes the new ones at once.
#[tauri::command]
#[specta::specta]
pub fn change_end_keys_setting(app: AppHandle, end_keys: Vec<EndKey>) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    validate(&end_keys, &settings.default_ends)?;
    for end in &end_keys {
        if end.shortcut.trim().is_empty() {
            continue;
        }
        crate::shortcut::validate_shortcut_for_implementation(
            &end.shortcut,
            settings.keyboard_implementation,
        )?;
        check_against_bindings(&end.shortcut, &settings.bindings)?;
    }
    settings.end_keys = end_keys;
    settings::write_settings(&app, settings);
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({ "setting": "end_keys" }),
    );
    crate::shortcut::profile_sync::sync_now(&app);
    Ok(())
}

/// Sets the default end of start shortcut `binding_id` in the active
/// profile.
#[tauri::command]
#[specta::specta]
pub fn change_default_end_setting(
    app: AppHandle,
    binding_id: String,
    end_id: String,
) -> Result<(), String> {
    if !START_BINDINGS.contains(&binding_id.as_str()) {
        return Err(format!("'{binding_id}' is no start shortcut"));
    }
    let mut settings = settings::get_settings(&app);
    let mut default_ends = settings.default_ends.clone();
    default_ends.insert(binding_id, end_id);
    validate(&settings.end_keys, &default_ends)?;
    settings.default_ends = default_ends;
    settings::write_settings(&app, settings);
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({ "setting": "default_ends" }),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::get_default_settings;

    fn end(id: &str, name: &str, shortcut: &str, append: &str, pp: bool, ctx: bool) -> EndKey {
        EndKey {
            id: id.to_string(),
            name: name.to_string(),
            shortcut: shortcut.to_string(),
            append_text: append.to_string(),
            post_process: pp,
            send_context: ctx,
        }
    }

    fn values_with(end_keys: Vec<EndKey>, ends: &[(&str, &str)]) -> ProfileValues {
        ProfileValues {
            end_keys,
            default_ends: ends
                .iter()
                .map(|(b, e)| (b.to_string(), e.to_string()))
                .collect(),
            ..Default::default()
        }
    }

    // ---- Out of the box --------------------------------------------------

    #[test]
    fn a_new_installation_has_plain_and_post_process() {
        let settings = get_default_settings();
        let ids: Vec<(&str, bool)> = settings
            .end_keys
            .iter()
            .map(|end| (end.id.as_str(), end.post_process))
            .collect();
        assert_eq!(ids, vec![(PLAIN_ID, false), (POST_PROCESS_ID, true)]);
        assert!(settings.end_keys.iter().all(|end| end.name.is_empty()
            && end.shortcut.is_empty()
            && end.append_text.is_empty()
            && !end.send_context));
        assert_eq!(
            settings.default_ends.get("transcribe").map(String::as_str),
            Some(PLAIN_ID)
        );
        assert_eq!(
            settings
                .default_ends
                .get("transcribe_with_post_process")
                .map(String::as_str),
            Some(POST_PROCESS_ID)
        );
    }

    #[test]
    fn the_built_in_ones_carry_the_append_text_and_the_context_switch() {
        let ends = builtin(r"\n\n", true);
        assert_eq!(
            ends,
            vec![
                end(PLAIN_ID, "", "", r"\n\n", false, true),
                end(POST_PROCESS_ID, "", "", r"\n\n", true, true),
            ]
        );
        assert!(is_builtin(PLAIN_ID) && is_builtin(POST_PROCESS_ID));
        assert!(!is_builtin("end_1"));
    }

    // ---- Which end a recording takes ---------------------------------------

    #[test]
    fn each_start_shortcut_ends_with_its_default_end() {
        let values = values_with(builtin(r"\s", true), &[]);
        assert_eq!(default_end_id("transcribe"), PLAIN_ID);
        assert_eq!(
            default_end_id("transcribe_with_post_process"),
            POST_PROCESS_ID
        );
        // Without entries of its own, the built-in default ends apply.
        assert_eq!(default_end(&values, "transcribe").id, PLAIN_ID);
        let pp = default_end(&values, "transcribe_with_post_process");
        assert_eq!(pp.id, POST_PROCESS_ID);
        assert!(pp.post_process && pp.send_context);
        assert_eq!(pp.append_text, r"\s");
    }

    #[test]
    fn a_profile_can_give_a_start_shortcut_another_default_end() {
        let mut keys = builtin("", false);
        keys.push(end("send", "Send", "ctrl+enter", "{Enter}", true, false));
        let values = values_with(keys, &[("transcribe", "send")]);
        let chosen = resolve(&values, &EndChoice::DefaultOf("transcribe".to_string()));
        assert_eq!(chosen.id, "send");
        assert_eq!(chosen.append_text, "{Enter}");
        assert_eq!(plain_end(&values).id, "send");
    }

    #[test]
    fn an_end_shortcut_ends_with_its_own_values() {
        let mut keys = builtin("", false);
        keys.push(end("line", "Line", "shift+enter", r"\n", false, false));
        let values = values_with(keys, &[]);
        let chosen = resolve(&values, &EndChoice::Key("line".to_string()));
        assert_eq!(
            chosen,
            end("line", "Line", "shift+enter", r"\n", false, false)
        );
    }

    #[test]
    fn a_missing_end_falls_back_to_the_built_in_default_then_to_a_plain_one() {
        // A default end pointing at a deleted entry.
        let values = values_with(builtin(r"\s", false), &[("transcribe", "gone")]);
        assert_eq!(default_end(&values, "transcribe").id, PLAIN_ID);
        // An end shortcut the profile doesn't have (another profile's).
        assert_eq!(
            resolve(&values, &EndChoice::Key("gone".to_string())).id,
            PLAIN_ID
        );
        // No end shortcuts at all: an end like before, by the shortcut.
        let empty = values_with(Vec::new(), &[]);
        let pp = default_end(&empty, "transcribe_with_post_process");
        assert_eq!(pp.id, POST_PROCESS_ID);
        assert!(pp.post_process);
        assert!(!default_end(&empty, "transcribe").post_process);
    }

    #[test]
    fn an_end_that_post_processes_ranks_higher() {
        assert!(
            rank(&end("a", "", "", "", true, false)) > rank(&end("b", "", "", "", false, true))
        );
        assert_eq!(
            rank(&end("a", "", "", "", false, false)),
            rank(&end("b", "", "", "", false, true))
        );
    }

    // ---- Registering -------------------------------------------------------

    #[test]
    fn an_end_shortcut_is_registered_under_its_own_binding_id() {
        let id = binding_id("send");
        assert_eq!(id, "end_key:send");
        assert_eq!(end_id_of(&id), Some("send"));
        assert_eq!(end_id_of("transcribe"), None);
    }

    #[test]
    fn only_end_shortcuts_with_a_key_combination_are_registered() {
        let mut keys = builtin("", false);
        keys.push(end("send", "Send", "ctrl+enter", "{Enter}", true, false));
        keys.push(end("blank", "Blank", "  ", "", false, false));
        let bindings = bindings_to_register(&keys);
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].id, "end_key:send");
        assert_eq!(bindings[0].current_binding, "ctrl+enter");
        assert_eq!(bindings[0].name, "Send");
    }

    #[test]
    fn a_key_combination_that_another_shortcut_has_is_refused() {
        let bindings = get_default_settings().bindings;
        let taken = bindings["transcribe"].current_binding.clone();
        let error = check_against_bindings(&taken, &bindings).unwrap_err();
        assert_eq!(error, "conflict:transcribe");
        assert!(check_against_bindings("ctrl+shift+f9", &bindings).is_ok());
        let cancel = bindings["cancel"].current_binding.clone();
        assert_eq!(
            check_against_bindings(&cancel, &bindings).unwrap_err(),
            "conflict:cancel"
        );
    }

    // ---- Checking a list ---------------------------------------------------

    #[test]
    fn a_list_needs_unique_ids_names_and_key_combinations() {
        let ends = default_ends();
        let mut keys = builtin("", false);
        assert!(validate(&keys, &ends).is_ok());
        keys.push(end("send", "Send", "ctrl+enter", "", false, false));
        assert!(validate(&keys, &ends).is_ok());

        let mut twice = keys.clone();
        twice.push(end("send", "Other", "", "", false, false));
        assert!(validate(&twice, &ends).is_err(), "id twice");

        let mut nameless = keys.clone();
        nameless.push(end("x", "  ", "", "", false, false));
        assert!(validate(&nameless, &ends).is_err(), "no name");

        let mut no_id = keys.clone();
        no_id.push(end("", "Named", "", "", false, false));
        assert!(validate(&no_id, &ends).is_err(), "no id");

        let mut same_keys = keys.clone();
        same_keys.push(end("y", "Y", "ctrl+enter", "", false, false));
        assert!(validate(&same_keys, &ends).is_err(), "combination twice");
    }

    #[test]
    fn an_end_shortcut_in_use_as_a_default_end_cannot_go() {
        let keys: Vec<EndKey> = builtin("", false)
            .into_iter()
            .filter(|end| end.id != POST_PROCESS_ID)
            .collect();
        assert_eq!(
            validate(&keys, &default_ends()).unwrap_err(),
            "end-key-in-use:post_process"
        );
        let mut ends = default_ends();
        ends.insert(
            "transcribe_with_post_process".to_string(),
            PLAIN_ID.to_string(),
        );
        assert!(validate(&keys, &ends).is_ok());
    }

    // ---- Migration ---------------------------------------------------------

    /// A store from 0.2.5: the Append text and Send Context per profile.
    fn store_0_2_5() -> serde_json::Value {
        let mut stored = serde_json::to_value(get_default_settings()).unwrap();
        let map = stored.as_object_mut().unwrap();
        for key in ["end_keys", "default_ends"] {
            map.remove(key);
        }
        map.insert("append_text".into(), serde_json::json!(r"\n\n"));
        map.insert("send_context".into(), serde_json::json!(true));
        map.insert("active_profile_id".into(), serde_json::json!("work"));
        let mut bindings = map["bindings"].clone();
        bindings["transcribe"]["current_binding"] = serde_json::json!("f13");
        bindings["toggle_send_context"] = serde_json::json!({
            "id": "toggle_send_context", "name": "Send Context", "description": "",
            "default_binding": "ctrl+shift+x", "current_binding": "ctrl+shift+x"
        });
        map.insert("bindings".into(), bindings);
        map.insert(
            "profiles".into(),
            serde_json::json!([
                { "id": "default", "name": "", "apps": [],
                  "values": { "selected_language": "de", "append_text": r"\s", "send_context": false } },
                { "id": "work", "name": "Arbeit", "apps": ["com.microsoft.VSCode"],
                  "values": { "selected_language": "en", "append_text": r"\n\n", "send_context": true } }
            ]),
        );
        stored
    }

    fn read(stored: &serde_json::Value) -> AppSettings {
        serde_json::from_value(stored.clone()).expect("the store loads")
    }

    #[test]
    fn every_profile_gets_the_two_end_shortcuts_with_what_it_had() {
        let stored = store_0_2_5();
        let mut s = read(&stored);
        assert!(migrate_on(&mut s, &stored, KeyPlatform::Macos));
        assert_eq!(s.end_keys, builtin(r"\n\n", true));
        assert_eq!(s.default_ends, default_ends());
        assert_eq!(s.profiles[0].values.end_keys, builtin(r"\s", false));
        assert_eq!(s.profiles[1].values.end_keys, builtin(r"\n\n", true));
        assert_eq!(s.profiles[1].values.default_ends, default_ends());
    }

    #[test]
    fn a_profile_without_shortcuts_gets_those_it_used() {
        let stored = store_0_2_5();
        let mut s = read(&stored);
        migrate_on(&mut s, &stored, KeyPlatform::Macos);
        for profile in &s.profiles {
            assert_eq!(
                profile.values.bindings["transcribe"].current_binding, "f13",
                "{}",
                profile.id
            );
        }
    }

    #[test]
    fn the_send_context_shortcut_goes_everywhere() {
        let stored = store_0_2_5();
        let mut s = read(&stored);
        migrate_on(&mut s, &stored, KeyPlatform::Macos);
        assert!(!s.bindings.contains_key("toggle_send_context"));
        assert!(!s
            .bindings
            .contains_key(crate::send_context::RETIRED_BINDING));
        for profile in &s.profiles {
            assert!(!profile.values.bindings.contains_key("toggle_send_context"));
        }
        assert!(!get_default_settings()
            .bindings
            .contains_key("toggle_send_context"));
    }

    #[test]
    fn a_store_from_before_the_append_field_takes_the_trailing_space() {
        let mut stored = store_0_2_5();
        let map = stored.as_object_mut().unwrap();
        map.remove("append_text");
        map.insert("append_trailing_space".into(), serde_json::json!(true));
        map["profiles"][0]["values"]
            .as_object_mut()
            .unwrap()
            .remove("append_text");
        map["profiles"][0]["values"]["append_trailing_space"] = serde_json::json!(true);
        let mut s = read(&stored);
        assert!(migrate_on(&mut s, &stored, KeyPlatform::Macos));
        assert_eq!(s.end_keys[0].append_text, r"\s");
        assert_eq!(s.profiles[0].values.end_keys[0].append_text, r"\s");
    }

    #[test]
    fn the_migration_runs_once() {
        let stored = store_0_2_5();
        let mut s = read(&stored);
        assert!(migrate_on(&mut s, &stored, KeyPlatform::Macos));
        let written = serde_json::to_value(&s).unwrap();
        let mut again = read(&written);
        assert!(!migrate_on(&mut again, &written, KeyPlatform::Macos));
        assert_eq!(again.end_keys, s.end_keys);
        assert_eq!(again.profiles, s.profiles);
    }

    #[test]
    fn a_current_store_with_nothing_to_carry_over_is_not_written() {
        let stored = serde_json::to_value(get_default_settings()).unwrap();
        let mut s = read(&stored);
        assert!(!migrate_on(&mut s, &stored, KeyPlatform::Macos));
    }

    #[test]
    fn key_names_from_another_system_are_localized_on_every_read() {
        let mut s = get_default_settings();
        s.end_keys[1].append_text = "{Win+Enter}".to_string();
        let stored = serde_json::to_value(&s).unwrap();
        let mut s = read(&stored);
        assert!(migrate_on(&mut s, &stored, KeyPlatform::Macos));
        assert_eq!(s.end_keys[1].append_text, "{Cmd+Enter}");
    }

    // ---- Import --------------------------------------------------------------

    #[test]
    fn an_import_from_before_end_shortcuts_gets_them_from_its_fields() {
        let file = "version: 1\nprofile:\n  id: work\n  name: Arbeit\n  values:\n    selected_language: en\n    append_text: '\\n\\n'\n    send_context: true\n";
        let mut values = ProfileValues::default();
        prepare_import_on(file, &mut values, KeyPlatform::Macos);
        assert_eq!(values.end_keys, builtin(r"\n\n", true));
        assert_eq!(values.default_ends, default_ends());
        assert!(
            values.bindings.is_empty(),
            "takes the importing installation's shortcuts"
        );
    }

    #[test]
    fn an_import_with_end_shortcuts_keeps_them() {
        let values = ProfileValues {
            end_keys: vec![end("send", "Send", "ctrl+enter", "{Win+Enter}", true, true)],
            ..Default::default()
        };
        let file = serde_saphyr::to_string(&serde_json::json!({
            "version": 1,
            "profile": { "id": "w", "name": "W", "values": values.clone() }
        }))
        .unwrap();
        let mut imported = values.clone();
        prepare_import_on(&file, &mut imported, KeyPlatform::Macos);
        assert_eq!(imported.end_keys[0].id, "send");
        assert_eq!(imported.end_keys[0].append_text, "{Cmd+Enter}");
        assert_eq!(imported.bindings, values.bindings);
    }
}
