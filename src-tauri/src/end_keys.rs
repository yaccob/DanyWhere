//! DanyWhere: start and end shortcuts (idea 3; user, 2026-11-19, rethought
//! 2026-11-22).
//!
//! A start shortcut only starts a recording. A recording ends with one of
//! the end shortcuts, a free list (`ProfileValues::end_keys`) in which no
//! entry is distinguished. Each has a key combination of its own, which
//! DanyWhere registers only while a recording runs, like Escape
//! (`shortcut::profile_sync`), and says what follows: what is appended (the
//! "Append" syntax of `append_text`), whether the text is post-processed
//! (with the profile's prompts) and whether the post-processing gets earlier
//! dictations as context. Every entry keeps its `id`, so the profiles can
//! share entries one by one. Its name is free; empty means it has none of
//! its own and is shown by its combination.
//!
//! Start and end shortcuts are independent. An end may have a start
//! shortcut's combination: pressing that start shortcut during a recording
//! is then a press of that end. A start shortcut whose combination no end
//! has does nothing during a recording. The one exception is holding:
//! releasing a held start shortcut ends with its hold end
//! (`ProfileValues::hold_ends`), which has neither a name nor a combination.
//!
//! Out of the box there are two ends, `plain` and `post_process`, with the
//! combinations of the two start shortcuts, and holding ends without and
//! with post-processing, so in use nothing changed. The first read of an
//! older store carries over what it had ([`migrate`]).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Emitter};

use crate::append_text::KeyPlatform;
use crate::profiles::ProfileValues;
use crate::settings::{self, AppSettings, ShortcutActivation, ShortcutBinding};

/// The end that has the first start shortcut's combination out of the box.
pub const PLAIN_ID: &str = "plain";
/// The end that has the second start shortcut's combination out of the box.
pub const POST_PROCESS_ID: &str = "post_process";
/// The prefix of an end shortcut's binding id while it is registered.
pub const BINDING_PREFIX: &str = "end_key:";
/// The prefix of the id a hold end takes in History (`held:<binding>`).
pub const HELD_PREFIX: &str = "held:";
/// The start shortcuts.
pub const START_BINDINGS: [&str; 2] = ["transcribe", "transcribe_with_post_process"];

/// One end shortcut.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type, Default)]
#[serde(default)]
pub struct EndKey {
    /// Stable, so profiles can share the entry.
    pub id: String,
    /// Its name of its own; empty: none, it is shown by its combination.
    pub name: String,
    /// The key combination that ends a recording this way; every end has one.
    pub shortcut: String,
    /// What is appended, in the "Append" syntax (`append_text`).
    pub append_text: String,
    /// Whether the text is post-processed (if the profile allows it).
    pub post_process: bool,
    /// Whether post-processing gets earlier dictations as context.
    pub send_context: bool,
}

/// How releasing a held start shortcut ends the recording.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type, Default)]
#[serde(default)]
pub struct HoldEnd {
    /// What is appended, in the "Append" syntax (`append_text`).
    pub append_text: String,
    /// Whether the text is post-processed (if the profile allows it).
    pub post_process: bool,
    /// Whether post-processing gets earlier dictations as context.
    pub send_context: bool,
}

impl HoldEnd {
    /// Start shortcut `binding`'s hold end out of the box: the second one
    /// post-processes, as Handy's post-processing shortcut did.
    fn factory(binding: &str) -> Self {
        Self {
            post_process: binding == START_BINDINGS[1],
            ..Self::default()
        }
    }

    /// As an end of start shortcut `binding`, under its History id.
    fn as_end(&self, binding: &str) -> EndKey {
        EndKey {
            id: held_id(binding),
            name: String::new(),
            shortcut: String::new(),
            append_text: self.append_text.clone(),
            post_process: self.post_process,
            send_context: self.send_context,
        }
    }
}

/// The id start shortcut `binding`'s hold end has in History.
pub fn held_id(binding: &str) -> String {
    format!("{HELD_PREFIX}{binding}")
}

/// The end that has start shortcut `binding`'s combination out of the box.
fn builtin_id(binding: &str) -> &'static str {
    if binding == START_BINDINGS[1] {
        POST_PROCESS_ID
    } else {
        PLAIN_ID
    }
}

/// The key combination of start shortcut `binding` in `bindings`, or a new
/// installation's.
fn combination(bindings: &HashMap<String, ShortcutBinding>, binding: &str) -> String {
    bindings
        .get(binding)
        .map(|found| found.current_binding.trim().to_string())
        .filter(|keys| !keys.is_empty())
        .unwrap_or_else(|| {
            settings::get_default_settings()
                .bindings
                .get(binding)
                .map(|found| found.current_binding.clone())
                .unwrap_or_default()
        })
}

/// The two ends every layer starts with, each with the combination of its
/// start shortcut in `bindings` and with `append_text` and `send_context`:
/// those the layer had before end shortcuts existed, or none.
pub fn builtin_for(
    bindings: &HashMap<String, ShortcutBinding>,
    append_text: &str,
    send_context: bool,
) -> Vec<EndKey> {
    START_BINDINGS
        .into_iter()
        .map(|binding| EndKey {
            id: builtin_id(binding).to_string(),
            name: String::new(),
            shortcut: combination(bindings, binding),
            append_text: append_text.to_string(),
            post_process: HoldEnd::factory(binding).post_process,
            send_context,
        })
        .collect()
}

/// [`builtin_for`] with a new installation's start shortcuts (tests).
#[cfg(test)]
pub fn builtin(append_text: &str, send_context: bool) -> Vec<EndKey> {
    builtin_for(
        &settings::get_default_settings().bindings,
        append_text,
        send_context,
    )
}

/// The hold ends every layer starts with: without and with post-processing,
/// with `append_text` and `send_context`.
pub fn builtin_holds(append_text: &str, send_context: bool) -> HashMap<String, HoldEnd> {
    START_BINDINGS
        .into_iter()
        .map(|binding| {
            let hold = HoldEnd {
                append_text: append_text.to_string(),
                send_context,
                ..HoldEnd::factory(binding)
            };
            (binding.to_string(), hold)
        })
        .collect()
}

/// A new installation's end shortcuts (serde default of the settings).
pub fn default_end_keys() -> Vec<EndKey> {
    settings::get_default_settings().end_keys
}

/// A new installation's hold ends (serde default of the settings).
pub fn default_hold_ends() -> HashMap<String, HoldEnd> {
    builtin_holds("", false)
}

/// How a recording ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndChoice {
    /// With the end shortcut of this id: pressed, or a start shortcut with
    /// its combination was.
    Key(String),
    /// With the hold end of this start shortcut: it was held and released.
    Held(String),
    /// As a press of this start shortcut would: with the end of its
    /// combination, else as when held. For the services, remote control and
    /// Handy's `stop`, which must always end.
    Start(String),
}

/// The end shortcut `id` in `values`.
fn find<'a>(values: &'a ProfileValues, id: &str) -> Option<&'a EndKey> {
    values.end_keys.iter().find(|end| end.id == id)
}

/// The hold end of start shortcut `binding` in `values`, or the factory one.
pub fn hold_end(values: &ProfileValues, binding: &str) -> EndKey {
    values
        .hold_ends
        .get(binding)
        .cloned()
        .unwrap_or_else(|| HoldEnd::factory(binding))
        .as_end(binding)
}

/// The end shortcut with exactly start shortcut `binding`'s combination in
/// `values` (`key_sides::identical_press`): one that fires on fewer or more
/// presses is another combination.
pub fn end_with_keys<'a>(values: &'a ProfileValues, binding: &str) -> Option<&'a EndKey> {
    let keys = combination(&values.bindings, binding);
    if keys.is_empty() {
        return None;
    }
    values
        .end_keys
        .iter()
        .find(|end| crate::key_sides::identical_press(&end.shortcut, &keys))
}

/// [`EndChoice::Start`]: the end of `binding`'s combination, else its hold
/// end.
fn as_start(values: &ProfileValues, binding: &str) -> EndKey {
    end_with_keys(values, binding)
        .cloned()
        .unwrap_or_else(|| hold_end(values, binding))
}

/// The end a dictation with `values`, started with `recording_binding`,
/// ends with by `choice`. A pressed end shortcut the profile doesn't have
/// (it changed meanwhile) ends as a press of the recording's start shortcut.
pub fn resolve(values: &ProfileValues, choice: &EndChoice, recording_binding: &str) -> EndKey {
    match choice {
        EndChoice::Key(id) => find(values, id).cloned().unwrap_or_else(|| {
            log::warn!("End shortcut '{id}' not in the profile; ending as its start shortcut");
            as_start(values, recording_binding)
        }),
        EndChoice::Held(binding) => hold_end(values, binding),
        EndChoice::Start(binding) => as_start(values, binding),
    }
}

/// The end of id `id` in `values`: an end shortcut, or a hold end by its
/// History id (`held:<binding>`). For redoing a dictation for another
/// profile (`profile_redo`).
pub fn find_end(values: &ProfileValues, id: &str) -> Option<EndKey> {
    if let Some(binding) = id.strip_prefix(HELD_PREFIX) {
        return START_BINDINGS
            .contains(&binding)
            .then(|| hold_end(values, binding));
    }
    find(values, id).cloned()
}

/// The end a text gets that no end chose ("Paste Last Transcript", "Insert
/// Last Dictation" and "Copy Last Transcript" outside a dictation, before
/// DanyWhere knows how the last one ended): as a press of the first start
/// shortcut.
pub fn fallback_end(values: &ProfileValues) -> EndKey {
    as_start(values, START_BINDINGS[0])
}

/// The end a recording started with `binding` is expected to take, for what
/// shows and loads at its start: with "Hold" its hold end, else as a press
/// of `binding` would end it.
pub fn expected_end(
    values: &ProfileValues,
    binding: &str,
    activation: ShortcutActivation,
) -> EndKey {
    match activation {
        ShortcutActivation::PushToTalk => hold_end(values, binding),
        ShortcutActivation::Toggle | ShortcutActivation::HoldOrToggle => as_start(values, binding),
    }
}

/// The id of the end with start shortcut `binding`'s combination in the
/// profile that applies now (`app_profiles::profile_now`), for the
/// coordinator.
pub fn keys_end_now(app: &AppHandle, binding: &str) -> Option<String> {
    let settings = settings::get_settings(app);
    let frontmost = crate::app_profiles::frontmost_if_choosing(&settings);
    let (_, values) = crate::app_profiles::profile_now(&settings, frontmost.as_deref());
    end_with_keys(&values, binding).map(|end| end.id.clone())
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

/// Checks a list of end shortcuts: at least one, ids present and unique, a
/// key combination for every entry, none twice (two that one press fires
/// both count as the same, `key_sides::same_press`). A name is optional.
pub fn validate(end_keys: &[EndKey]) -> Result<(), String> {
    if end_keys.is_empty() {
        return Err("end-key-last".to_string());
    }
    let mut ids = std::collections::HashSet::new();
    let mut shortcuts: Vec<&str> = Vec::new();
    for end in end_keys {
        if end.id.trim().is_empty() {
            return Err("An end shortcut needs an id".to_string());
        }
        if !ids.insert(end.id.as_str()) {
            return Err(format!("End shortcut id '{}' twice", end.id));
        }
        let shortcut = end.shortcut.trim();
        if shortcut.is_empty() {
            return Err(format!("end-key-needs-shortcut:{}", end.id));
        }
        if shortcuts
            .iter()
            .any(|taken| crate::key_sides::same_press(taken, shortcut))
        {
            return Err(format!("end-key-shortcut-twice:{shortcut}"));
        }
        shortcuts.push(shortcut);
    }
    Ok(())
}

/// Whether a recording with `values` can end where end shortcuts' own
/// combinations aren't taken (Linux, `shortcut::profile_sync`): with
/// "Hold" every release ends it; otherwise an end must have a start
/// shortcut's combination.
pub fn can_end_with_start_shortcuts(values: &ProfileValues) -> bool {
    values.shortcut_activation == ShortcutActivation::PushToTalk
        || START_BINDINGS
            .into_iter()
            .any(|binding| end_with_keys(values, binding).is_some())
}

/// A layer that could always end a recording before a change must still
/// after it: it keeps an end shortcut, and on Linux (`linux`) a way that
/// works there ([`can_end_with_start_shortcuts`]). A layer that couldn't
/// before isn't held to it, so an older store never blocks a change.
pub fn still_ends(
    before: &ProfileValues,
    after: &ProfileValues,
    linux: bool,
) -> Result<(), String> {
    if !before.end_keys.is_empty() && after.end_keys.is_empty() {
        return Err("end-key-last".to_string());
    }
    if linux && can_end_with_start_shortcuts(before) && !can_end_with_start_shortcuts(after) {
        return Err("end-key-cannot-end".to_string());
    }
    Ok(())
}

/// [`still_ends`] for the defaults and every profile of `before` and
/// `after` (`profile_layers::commit`), on this system.
pub fn check_layers(before: &AppSettings, after: &AppSettings) -> Result<(), String> {
    let linux = cfg!(target_os = "linux");
    still_ends(
        &crate::profiles::default_values(before),
        &crate::profiles::default_values(after),
        linux,
    )?;
    for profile in &after.profiles {
        if let Some(old) = before.profiles.iter().find(|old| old.id == profile.id) {
            still_ends(&old.values, &profile.values, linux)?;
        }
    }
    Ok(())
}

/// Run on every read of the store (`settings_value` is the stored JSON),
/// with this system's key names; see [`migrate_on`].
pub fn migrate(settings: &mut AppSettings, settings_value: &serde_json::Value) -> bool {
    migrate_on(settings, settings_value, KeyPlatform::current())
}

/// For the settings, the defaults and every stored profile:
/// - once, a value without end shortcuts (0.2.5 and before) gets the two
///   built-in ones with the combinations of its start shortcuts, and hold
///   ends, all with the "Append" text (or Handy's trailing space and auto
///   submit before that) and the "Send Context" it had;
/// - once, a value with end shortcuts but no hold ends (Dev, 2026-11-19 to
///   2026-11-22) turns its default ends into hold ends ([`from_default_ends`]);
///   a profile's entry that then differs from the defaults becomes its own;
/// - a profile without shortcuts gets the flat ones (those it used);
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
    let mut changed = drop_retired_bindings(&mut settings.bindings);
    // The settings: the values in use.
    changed |= migrate_layer(
        Some(settings_value),
        &mut settings.end_keys,
        &mut settings.hold_ends,
        &settings.bindings,
        settings.append_trailing_space,
        (auto_submit, key, platform),
    );
    // The defaults.
    let stored_defaults = settings_value.get("default_values");
    if let Some(defaults) = settings.default_values.as_mut() {
        changed |= drop_retired_bindings(&mut defaults.bindings);
        changed |= migrate_layer(
            stored_defaults,
            &mut defaults.end_keys,
            &mut defaults.hold_ends,
            &defaults.bindings,
            crate::append_text::stored_trailing_space(stored_defaults),
            (auto_submit, key, platform),
        );
    }

    let stored_profiles = settings_value
        .get("profiles")
        .and_then(|profiles| profiles.as_array());
    let flat_bindings = settings.bindings.clone();
    let defaults = settings.default_values.clone();
    for profile in settings.profiles.iter_mut() {
        let stored_values = stored_profiles
            .and_then(|profiles| {
                profiles.iter().find(|stored| {
                    stored.get("id").and_then(|id| id.as_str()) == Some(profile.id.as_str())
                })
            })
            .and_then(|stored| stored.get("values"));
        let lacks = |field: &str| stored_values.is_none_or(|values| values.get(field).is_none());
        // The shortcuts it used were the flat ones (contract V5); its end
        // shortcuts take their combinations.
        if lacks("bindings") && profile.values.bindings != flat_bindings {
            profile.values.bindings = flat_bindings.clone();
            changed = true;
        }
        changed |= drop_retired_bindings(&mut profile.values.bindings);
        let from_dev = !lacks("end_keys") && lacks("hold_ends");
        let values = &mut profile.values;
        changed |= migrate_layer(
            stored_values,
            &mut values.end_keys,
            &mut values.hold_ends,
            &values.bindings,
            crate::append_text::stored_trailing_space(stored_values),
            (auto_submit, key, platform),
        );
        // Its coupled entries now differ where its start shortcuts do: they
        // become its own, so the defaults don't overwrite them.
        if let (true, Some(defaults)) = (from_dev, defaults.as_ref()) {
            changed |= crate::profile_layers::own_where_different(profile, defaults, "end_keys");
            changed |= crate::profile_layers::own_where_different(profile, defaults, "hold_ends");
        }
    }

    for end in settings
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
        )
    {
        changed |= localize_in_place(&mut end.append_text, platform);
    }
    for hold in settings
        .hold_ends
        .values_mut()
        .chain(
            settings
                .default_values
                .iter_mut()
                .flat_map(|values| values.hold_ends.values_mut()),
        )
        .chain(
            settings
                .profiles
                .iter_mut()
                .flat_map(|profile| profile.values.hold_ends.values_mut()),
        )
    {
        changed |= localize_in_place(&mut hold.append_text, platform);
    }
    changed
}

/// `text` with `platform`'s key names; whether it changed.
fn localize_in_place(text: &mut String, platform: KeyPlatform) -> bool {
    let local = crate::append_text::localize(text, platform);
    if *text == local {
        return false;
    }
    log::info!("\"Append\" takes this system's key names: {text:?} → {local:?}");
    *text = local;
    true
}

/// One layer (the settings, the defaults or a profile's values, stored as
/// `stored`): see [`migrate_on`]. `legacy` is the global auto submit with
/// its key and this system, for a store from before the "Append" field.
fn migrate_layer(
    stored: Option<&serde_json::Value>,
    end_keys: &mut Vec<EndKey>,
    hold_ends: &mut HashMap<String, HoldEnd>,
    bindings: &HashMap<String, ShortcutBinding>,
    trailing_space: bool,
    legacy: (bool, crate::settings::AutoSubmitKey, KeyPlatform),
) -> bool {
    let lacks = |field: &str| stored.is_none_or(|values| values.get(field).is_none());
    if lacks("end_keys") {
        let (auto_submit, key, platform) = legacy;
        let append =
            crate::append_text::stored_value(stored, trailing_space, auto_submit, key, platform);
        let send_context = stored
            .and_then(|values| values.get("send_context"))
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let ends = builtin_for(bindings, &append, send_context);
        let holds = builtin_holds(&append, send_context);
        let changed = *end_keys != ends || *hold_ends != holds;
        if changed {
            log::info!("End shortcuts take over \"Append\" and \"Send Context\"");
        }
        *end_keys = ends;
        *hold_ends = holds;
        return changed;
    }
    if lacks("hold_ends") {
        let default_ends: HashMap<String, String> = stored
            .and_then(|values| values.get("default_ends"))
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default();
        return from_default_ends(end_keys, hold_ends, bindings, &default_ends);
    }
    false
}

/// The step from a start shortcut's default end (Dev, 2026-11-19 to
/// 2026-11-22) to start and end shortcuts apart:
/// - each start shortcut's hold end takes the values of its default end,
///   which its release used;
/// - that default end takes the start shortcut's combination, which its
///   second press used, unless it has one or another end has that one;
/// - the built-in ends then take their start shortcuts' combinations where
///   they are still free;
/// - an end left without a combination goes: it ended nothing any more.
///
/// Returns whether anything changed.
fn from_default_ends(
    end_keys: &mut Vec<EndKey>,
    hold_ends: &mut HashMap<String, HoldEnd>,
    bindings: &HashMap<String, ShortcutBinding>,
    default_ends: &HashMap<String, String>,
) -> bool {
    let before = (end_keys.clone(), hold_ends.clone());
    let position = |ends: &[EndKey], id: &str| ends.iter().position(|end| end.id == id);
    let default_of = |ends: &[EndKey], binding: &str| {
        default_ends
            .get(binding)
            .and_then(|id| position(ends, id))
            .or_else(|| position(ends, builtin_id(binding)))
    };
    let holds: HashMap<String, HoldEnd> = START_BINDINGS
        .into_iter()
        .map(|binding| {
            let hold = match default_of(end_keys, binding) {
                Some(index) => HoldEnd {
                    append_text: end_keys[index].append_text.clone(),
                    post_process: end_keys[index].post_process,
                    send_context: end_keys[index].send_context,
                },
                None => HoldEnd::factory(binding),
            };
            (binding.to_string(), hold)
        })
        .collect();
    let take = |ends: &mut Vec<EndKey>, index: Option<usize>, keys: &str| {
        let Some(index) = index else { return };
        let free = !ends
            .iter()
            .any(|end| crate::key_sides::same_press(&end.shortcut, keys));
        if ends[index].shortcut.trim().is_empty() && free {
            ends[index].shortcut = keys.to_string();
        }
    };
    for binding in START_BINDINGS {
        let keys = combination(bindings, binding);
        let index = default_of(end_keys, binding);
        take(end_keys, index, &keys);
    }
    for binding in START_BINDINGS {
        let keys = combination(bindings, binding);
        let index = position(end_keys, builtin_id(binding));
        take(end_keys, index, &keys);
    }
    end_keys.retain(|end| {
        let keep = !end.shortcut.trim().is_empty();
        if !keep {
            log::info!(
                "End shortcut '{}' had no combination and ended nothing any more; it goes",
                end.id
            );
        }
        keep
    });
    *hold_ends = holds;
    log::info!("Default ends became hold ends");
    (end_keys.clone(), hold_ends.clone()) != before
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

/// For an import, as [`migrate_on`] for one layer: a profile file from
/// before end shortcuts gets them from its "Append" field (or trailing
/// space) and "Send Context", one from Dev its hold ends from its default
/// ends; a file without shortcuts takes the importing installation's (an
/// empty map, which `profiles::ensure_profiles` fills), its end shortcuts a
/// new installation's start combinations; Append texts get `platform`'s key
/// names.
pub fn prepare_import_on(file_text: &str, values: &mut ProfileValues, platform: KeyPlatform) {
    if let Ok(file) = serde_saphyr::from_str::<serde_json::Value>(file_text) {
        let stored = file
            .get("profile")
            .and_then(|profile| profile.get("values"));
        let lacks = |field: &str| stored.is_none_or(|values| values.get(field).is_none());
        let bindings = values.bindings.clone();
        // auto_submit was global, so the file holds no key press.
        migrate_layer(
            stored,
            &mut values.end_keys,
            &mut values.hold_ends,
            &bindings,
            crate::append_text::stored_trailing_space(stored),
            (false, crate::settings::AutoSubmitKey::Enter, platform),
        );
        if lacks("bindings") {
            values.bindings.clear();
        }
    }
    drop_retired_bindings(&mut values.bindings);
    for end in values.end_keys.iter_mut() {
        end.append_text = crate::append_text::localize(&end.append_text, platform);
    }
    for hold in values.hold_ends.values_mut() {
        hold.append_text = crate::append_text::localize(&hold.append_text, platform);
    }
}

/// Checks a key combination of a shortcut against every other shortcut in
/// `bindings` (one press firing both counts, `key_sides::same_press`); the
/// error names the one it collides with, as `change_binding` does
/// (`conflict:<id>`).
pub fn check_against_bindings(
    shortcut: &str,
    bindings: &HashMap<String, ShortcutBinding>,
) -> Result<(), String> {
    check_with(shortcut, bindings, |_, _| false)
}

/// [`check_against_bindings`] for an end shortcut, which may have exactly a
/// start shortcut's combination (`key_sides::identical_press`): pressing
/// that start shortcut during a recording then ends this way.
pub fn check_end_against_bindings(
    shortcut: &str,
    bindings: &HashMap<String, ShortcutBinding>,
) -> Result<(), String> {
    check_with(shortcut, bindings, |id, keys| {
        START_BINDINGS.contains(&id) && crate::key_sides::identical_press(keys, shortcut)
    })
}

/// The shared part of the two checks: `allowed` says which binding (by id
/// and keys) may share the press.
fn check_with(
    shortcut: &str,
    bindings: &HashMap<String, ShortcutBinding>,
    allowed: impl Fn(&str, &str) -> bool,
) -> Result<(), String> {
    let shortcut = shortcut.trim();
    let mut ids: Vec<&String> = bindings.keys().collect();
    ids.sort();
    match ids.into_iter().find(|id| {
        let keys = bindings[*id].current_binding.trim();
        !keys.is_empty()
            && crate::key_sides::same_press(keys, shortcut)
            && !allowed(id.as_str(), keys)
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
    validate(&end_keys)?;
    for end in &end_keys {
        crate::shortcut::validate_shortcut_for_implementation(
            &end.shortcut,
            settings.keyboard_implementation,
        )?;
        check_end_against_bindings(&end.shortcut, &settings.bindings)?;
    }
    let before = ProfileValues::from_settings(&settings);
    let mut after = before.clone();
    after.end_keys = end_keys.clone();
    still_ends(&before, &after, cfg!(target_os = "linux"))?;
    settings.end_keys = end_keys;
    settings::write_settings(&app, settings);
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({ "setting": "end_keys" }),
    );
    crate::shortcut::profile_sync::sync_now(&app);
    Ok(())
}

/// Stores the active profile's hold ends, by start shortcut.
#[tauri::command]
#[specta::specta]
pub fn change_hold_ends_setting(
    app: AppHandle,
    hold_ends: HashMap<String, HoldEnd>,
) -> Result<(), String> {
    if let Some(binding) = hold_ends
        .keys()
        .find(|binding| !START_BINDINGS.contains(&binding.as_str()))
    {
        return Err(format!("'{binding}' is no start shortcut"));
    }
    let mut settings = settings::get_settings(&app);
    let platform = KeyPlatform::current();
    settings.hold_ends = hold_ends
        .into_iter()
        .map(|(binding, mut hold)| {
            hold.append_text = crate::append_text::localize(&hold.append_text, platform);
            (binding, hold)
        })
        .collect();
    settings::write_settings(&app, settings);
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({ "setting": "hold_ends" }),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{get_default_settings, ShortcutActivation};

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

    fn hold(append: &str, pp: bool, ctx: bool) -> HoldEnd {
        HoldEnd {
            append_text: append.to_string(),
            post_process: pp,
            send_context: ctx,
        }
    }

    /// The start shortcuts of a new installation.
    fn start_keys() -> (String, String) {
        let bindings = get_default_settings().bindings;
        (
            bindings[START_BINDINGS[0]].current_binding.clone(),
            bindings[START_BINDINGS[1]].current_binding.clone(),
        )
    }

    /// Values with `end_keys`, `hold_ends` and a new installation's start
    /// shortcuts.
    fn values_with(end_keys: Vec<EndKey>, holds: &[(&str, HoldEnd)]) -> ProfileValues {
        ProfileValues {
            end_keys,
            hold_ends: holds
                .iter()
                .map(|(binding, hold)| (binding.to_string(), hold.clone()))
                .collect(),
            ..Default::default()
        }
    }

    // ---- Out of the box --------------------------------------------------

    #[test]
    fn a_new_installation_ends_with_the_start_shortcuts_combinations() {
        let settings = get_default_settings();
        let (plain_keys, post_keys) = start_keys();
        assert_eq!(
            settings.end_keys,
            vec![
                end(PLAIN_ID, "", &plain_keys, "", false, false),
                end(POST_PROCESS_ID, "", &post_keys, "", true, false),
            ]
        );
        assert_eq!(
            settings.hold_ends.get(START_BINDINGS[0]),
            Some(&hold("", false, false))
        );
        assert_eq!(
            settings.hold_ends.get(START_BINDINGS[1]),
            Some(&hold("", true, false))
        );
    }

    #[test]
    fn the_built_in_ones_carry_the_append_text_and_the_context_switch() {
        let mut bindings = get_default_settings().bindings;
        bindings.get_mut(START_BINDINGS[0]).unwrap().current_binding = "f13".to_string();
        let (_, post_keys) = start_keys();
        assert_eq!(
            builtin_for(&bindings, r"\n\n", true),
            vec![
                end(PLAIN_ID, "", "f13", r"\n\n", false, true),
                end(POST_PROCESS_ID, "", &post_keys, r"\n\n", true, true),
            ]
        );
        let holds = builtin_holds(r"\n\n", true);
        assert_eq!(holds[START_BINDINGS[0]], hold(r"\n\n", false, true));
        assert_eq!(holds[START_BINDINGS[1]], hold(r"\n\n", true, true));
    }

    // ---- Which end a recording takes ---------------------------------------

    #[test]
    fn an_end_shortcut_ends_with_its_own_values() {
        let mut keys = builtin("", false);
        keys.push(end("line", "Line", "shift+enter", r"\n", false, false));
        let values = values_with(keys, &[]);
        let chosen = resolve(&values, &EndChoice::Key("line".to_string()), "transcribe");
        assert_eq!(
            chosen,
            end("line", "Line", "shift+enter", r"\n", false, false)
        );
    }

    #[test]
    fn releasing_a_held_start_shortcut_ends_with_its_hold_end() {
        let values = values_with(
            builtin("", false),
            &[
                ("transcribe", hold(r"\s", false, false)),
                ("transcribe_with_post_process", hold("{Enter}", true, true)),
            ],
        );
        let held = resolve(
            &values,
            &EndChoice::Held("transcribe_with_post_process".to_string()),
            "transcribe_with_post_process",
        );
        assert_eq!(held.id, "held:transcribe_with_post_process");
        assert_eq!(held.append_text, "{Enter}");
        assert!(held.post_process && held.send_context);
        assert!(held.shortcut.is_empty() && held.name.is_empty());
        let plain = resolve(
            &values,
            &EndChoice::Held("transcribe".to_string()),
            "transcribe",
        );
        assert_eq!(plain.append_text, r"\s");
        assert!(!plain.post_process);
    }

    #[test]
    fn a_missing_hold_end_takes_the_factory_one() {
        let values = values_with(builtin("", false), &[]);
        let choice = |binding: &str| EndChoice::Held(binding.to_string());
        assert!(!resolve(&values, &choice("transcribe"), "transcribe").post_process);
        assert!(
            resolve(
                &values,
                &choice("transcribe_with_post_process"),
                "transcribe"
            )
            .post_process
        );
    }

    #[test]
    fn as_a_start_shortcut_ends_with_the_end_of_its_combination_else_as_held() {
        let mut values = values_with(
            builtin("", false),
            &[("transcribe", hold(r"\s", false, false))],
        );
        let start = EndChoice::Start("transcribe".to_string());
        assert_eq!(resolve(&values, &start, "transcribe").id, PLAIN_ID);
        let post = EndChoice::Start("transcribe_with_post_process".to_string());
        assert_eq!(resolve(&values, &post, "transcribe").id, POST_PROCESS_ID);
        // No end with its combination any more: as when held.
        values.end_keys[0].shortcut = "ctrl+return".to_string();
        let fallback = resolve(&values, &start, "transcribe");
        assert_eq!(fallback.id, "held:transcribe");
        assert_eq!(fallback.append_text, r"\s");
    }

    #[test]
    fn a_start_shortcut_has_an_end_only_with_exactly_its_combination() {
        let mut values = values_with(builtin("", false), &[]);
        let (plain_keys, _) = start_keys();
        assert_eq!(
            end_with_keys(&values, "transcribe").map(|end| end.id.as_str()),
            Some(PLAIN_ID)
        );
        values.end_keys[0].shortcut = plain_keys.to_uppercase();
        assert_eq!(
            end_with_keys(&values, "transcribe").map(|end| end.id.as_str()),
            Some(PLAIN_ID),
            "written in capitals, still the same press"
        );
        values.end_keys[0].shortcut = "ctrl+return".to_string();
        assert!(end_with_keys(&values, "transcribe").is_none());
        // One side only is another combination: it fires on fewer presses.
        values
            .bindings
            .get_mut("transcribe")
            .unwrap()
            .current_binding = "ctrl+return".to_string();
        values.end_keys[0].shortcut = "ctrl_left+return".to_string();
        assert!(end_with_keys(&values, "transcribe").is_none());
    }

    #[test]
    fn an_end_the_profile_lacks_ends_as_the_recordings_start_shortcut() {
        let values = values_with(builtin("", false), &[]);
        assert_eq!(
            resolve(
                &values,
                &EndChoice::Key("gone".to_string()),
                "transcribe_with_post_process"
            )
            .id,
            POST_PROCESS_ID
        );
    }

    #[test]
    fn an_end_is_found_by_its_id_also_when_held() {
        let values = values_with(
            builtin("", false),
            &[("transcribe", hold(r"\s", true, false))],
        );
        assert_eq!(
            find_end(&values, PLAIN_ID).map(|end| end.id),
            Some(PLAIN_ID.to_string())
        );
        let held = find_end(&values, "held:transcribe").expect("the hold end");
        assert!(held.post_process);
        assert!(find_end(&values, "held:nothing").is_none());
        assert!(find_end(&values, "gone").is_none());
    }

    #[test]
    fn without_a_known_end_the_first_start_shortcuts_end_applies() {
        let mut values = values_with(
            builtin(r"\s", false),
            &[("transcribe", hold("{Tab}", false, false))],
        );
        assert_eq!(fallback_end(&values).id, PLAIN_ID);
        values.end_keys.remove(0);
        assert_eq!(fallback_end(&values).append_text, "{Tab}");
    }

    #[test]
    fn the_end_expected_at_the_start_follows_the_activation() {
        let values = values_with(builtin("", false), &[("transcribe", hold("", true, true))]);
        let hold_end = expected_end(&values, "transcribe", ShortcutActivation::PushToTalk);
        assert_eq!(hold_end.id, "held:transcribe");
        for mode in [ShortcutActivation::Toggle, ShortcutActivation::HoldOrToggle] {
            assert_eq!(expected_end(&values, "transcribe", mode).id, PLAIN_ID);
        }
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
        let keys = vec![
            end("send", "Send", "ctrl+enter", "{Enter}", true, false),
            end("blank", "Blank", "  ", "", false, false),
        ];
        let bindings = bindings_to_register(&keys);
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].id, "end_key:send");
        assert_eq!(bindings[0].current_binding, "ctrl+enter");
    }

    // ---- Checking a list ---------------------------------------------------

    #[test]
    fn every_end_shortcut_needs_a_key_combination() {
        let mut keys = builtin("", false);
        keys.push(end("x", "X", "  ", "", false, false));
        assert_eq!(validate(&keys).unwrap_err(), "end-key-needs-shortcut:x");
    }

    #[test]
    fn a_list_needs_unique_ids_and_combinations_but_no_names() {
        let mut keys = builtin("", false);
        assert!(validate(&keys).is_ok());
        keys.push(end("send", "", "ctrl+enter", "", false, false));
        assert!(validate(&keys).is_ok(), "a name of its own is optional");

        let mut twice = keys.clone();
        twice.push(end("send", "Other", "f14", "", false, false));
        assert!(validate(&twice).is_err(), "id twice");

        let mut no_id = keys.clone();
        no_id.push(end("", "Named", "f14", "", false, false));
        assert!(validate(&no_id).is_err(), "no id");

        let mut same_keys = keys.clone();
        same_keys.push(end("y", "Y", "ctrl+enter", "", false, false));
        assert_eq!(
            validate(&same_keys).unwrap_err(),
            "end-key-shortcut-twice:ctrl+enter"
        );
    }

    #[test]
    fn a_combination_counts_twice_when_one_press_fires_both() {
        let mut keys = builtin("", false);
        keys.push(end("send", "Send", "ctrl+return", "", false, false));
        keys.push(end("left", "Left", "ctrl_left+return", "", false, false));
        assert_eq!(
            validate(&keys).unwrap_err(),
            "end-key-shortcut-twice:ctrl_left+return"
        );
        let mut apart = builtin("", false);
        apart.push(end("left", "Left", "ctrl_left+return", "", false, false));
        apart.push(end("right", "Right", "ctrl_right+return", "", false, false));
        assert!(validate(&apart).is_ok(), "the left and the right Ctrl");
    }

    #[test]
    fn the_list_is_never_empty() {
        assert_eq!(validate(&[]).unwrap_err(), "end-key-last");
    }

    #[test]
    fn an_end_may_have_a_start_shortcuts_combination() {
        let bindings = get_default_settings().bindings;
        for start in START_BINDINGS {
            let keys = bindings[start].current_binding.clone();
            assert!(
                check_end_against_bindings(&keys, &bindings).is_ok(),
                "{start}"
            );
        }
    }

    #[test]
    fn but_not_one_that_shares_only_some_presses_with_it() {
        let mut bindings = get_default_settings().bindings;
        bindings.get_mut("transcribe").unwrap().current_binding = "ctrl+space".to_string();
        assert_eq!(
            check_end_against_bindings("ctrl_left+space", &bindings).unwrap_err(),
            "conflict:transcribe"
        );
        bindings.get_mut("transcribe").unwrap().current_binding = "ctrl_right+x".to_string();
        assert!(check_end_against_bindings("ctrl_left+x", &bindings).is_ok());
    }

    #[test]
    fn another_shortcuts_combination_is_refused() {
        let bindings = get_default_settings().bindings;
        for id in ["cancel", "paste_last_transcript", "cycle_word_list"] {
            let taken = bindings[id].current_binding.clone();
            assert_eq!(
                check_end_against_bindings(&taken, &bindings).unwrap_err(),
                format!("conflict:{id}")
            );
        }
        assert!(check_end_against_bindings("ctrl+shift+f9", &bindings).is_ok());
    }

    // ---- There is always a way to end ----------------------------------------

    #[test]
    fn without_end_shortcuts_of_its_own_only_holding_or_a_start_combination_ends() {
        let mut values = values_with(builtin("", false), &[]);
        values.shortcut_activation = ShortcutActivation::HoldOrToggle;
        assert!(can_end_with_start_shortcuts(&values));
        for end in values.end_keys.iter_mut() {
            end.shortcut = format!("ctrl+{}", end.id.len());
        }
        assert!(!can_end_with_start_shortcuts(&values));
        values.shortcut_activation = ShortcutActivation::Toggle;
        assert!(!can_end_with_start_shortcuts(&values));
        values.shortcut_activation = ShortcutActivation::PushToTalk;
        assert!(
            can_end_with_start_shortcuts(&values),
            "every release ends it"
        );
    }

    #[test]
    fn a_layer_that_could_end_must_still_end_after_a_change() {
        let before = values_with(builtin("", false), &[]);
        let mut emptied = before.clone();
        emptied.end_keys.clear();
        assert_eq!(
            still_ends(&before, &emptied, false).unwrap_err(),
            "end-key-last"
        );
        assert!(still_ends(&before, &before, true).is_ok());
        let mut moved = before.clone();
        moved.end_keys[0].shortcut = "f14".to_string();
        moved.end_keys[1].shortcut = "f15".to_string();
        assert!(
            still_ends(&before, &moved, false).is_ok(),
            "its own combinations end it"
        );
        assert_eq!(
            still_ends(&before, &moved, true).unwrap_err(),
            "end-key-cannot-end",
            "on Linux they aren't taken"
        );
        // A layer that couldn't end before isn't blocked by it.
        assert!(still_ends(&moved, &moved, true).is_ok());
        assert!(still_ends(&emptied, &emptied, false).is_ok());
    }

    // ---- Migration ---------------------------------------------------------

    /// A store from 0.2.5: the Append text and Send Context per profile.
    fn store_0_2_5() -> serde_json::Value {
        let mut stored = serde_json::to_value(get_default_settings()).unwrap();
        let map = stored.as_object_mut().unwrap();
        for key in ["end_keys", "default_ends", "hold_ends"] {
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
    fn from_0_2_5_every_layer_ends_with_its_start_shortcuts_and_holds_as_before() {
        let stored = store_0_2_5();
        let mut s = read(&stored);
        assert!(migrate_on(&mut s, &stored, KeyPlatform::Macos));
        let (_, post_keys) = start_keys();
        assert_eq!(
            s.end_keys,
            vec![
                end(PLAIN_ID, "", "f13", r"\n\n", false, true),
                end(POST_PROCESS_ID, "", &post_keys, r"\n\n", true, true),
            ]
        );
        assert_eq!(s.hold_ends, builtin_holds(r"\n\n", true));
        // The profiles used the flat shortcuts (f13).
        assert_eq!(
            s.profiles[0].values.end_keys,
            vec![
                end(PLAIN_ID, "", "f13", r"\s", false, false),
                end(POST_PROCESS_ID, "", &post_keys, r"\s", true, false),
            ]
        );
        assert_eq!(s.profiles[0].values.hold_ends, builtin_holds(r"\s", false));
        assert_eq!(s.profiles[1].values.hold_ends, builtin_holds(r"\n\n", true));
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
        assert_eq!(s.hold_ends["transcribe"].append_text, r"\s");
    }

    /// Profile values from Dev (2026-11-19 to 2026-11-22): end shortcuts
    /// without a combination of their own, ended by their start shortcut as
    /// its default end, a "send" with ⌃↩ and a "spare" that ended nothing.
    fn dev_values() -> serde_json::Value {
        let mut values = serde_json::to_value(ProfileValues::default()).unwrap();
        let map = values.as_object_mut().unwrap();
        map.remove("hold_ends");
        let old = vec![
            end(PLAIN_ID, "", "", r"\n", false, true),
            end(POST_PROCESS_ID, "", "", r"\s", true, false),
            end("send", "Senden", "ctrl+return", "{Enter}", true, false),
            end("spare", "Reserve", "", "", false, false),
        ];
        map.insert("end_keys".into(), serde_json::json!(old));
        map.insert(
            "default_ends".into(),
            serde_json::json!({ "transcribe": "plain", "transcribe_with_post_process": "post_process" }),
        );
        values
    }

    /// A store from Dev with [`dev_values`] as its flat values.
    fn store_dev() -> serde_json::Value {
        let mut stored = serde_json::to_value(get_default_settings()).unwrap();
        let map = stored.as_object_mut().unwrap();
        map.remove("hold_ends");
        if let serde_json::Value::Object(values) = dev_values() {
            map.extend(values);
        }
        stored
    }

    #[test]
    fn from_dev_the_built_in_ends_take_the_start_combinations_and_become_the_holds() {
        let stored = store_dev();
        let mut s = read(&stored);
        assert!(migrate_on(&mut s, &stored, KeyPlatform::Macos));
        let (plain_keys, post_keys) = start_keys();
        assert_eq!(
            s.end_keys,
            vec![
                end(PLAIN_ID, "", &plain_keys, r"\n", false, true),
                end(POST_PROCESS_ID, "", &post_keys, r"\s", true, false),
                end("send", "Senden", "ctrl+return", "{Enter}", true, false),
            ],
            "an end without a combination that ended nothing goes"
        );
        assert_eq!(s.hold_ends["transcribe"], hold(r"\n", false, true));
        assert_eq!(
            s.hold_ends["transcribe_with_post_process"],
            hold(r"\s", true, false)
        );
    }

    #[test]
    fn from_dev_another_default_end_takes_the_start_combination_and_is_the_hold() {
        let mut stored = store_dev();
        stored["default_ends"]["transcribe"] = serde_json::json!("spare");
        let mut s = read(&stored);
        migrate_on(&mut s, &stored, KeyPlatform::Macos);
        let (plain_keys, _) = start_keys();
        let ids: Vec<(&str, &str)> = s
            .end_keys
            .iter()
            .map(|end| (end.id.as_str(), end.shortcut.as_str()))
            .collect();
        assert!(ids.contains(&("spare", plain_keys.as_str())), "{ids:?}");
        assert!(
            !ids.iter().any(|(id, _)| *id == PLAIN_ID),
            "plain lost its combination to the default end: {ids:?}"
        );
        assert_eq!(s.hold_ends["transcribe"], hold("", false, false));
    }

    #[test]
    fn from_dev_a_profiles_entry_that_now_differs_from_the_defaults_is_its_own() {
        let mut stored = store_dev();
        let defaults = dev_values();
        let mut profile_values = dev_values();
        profile_values["bindings"]["transcribe"]["current_binding"] = serde_json::json!("f13");
        let map = stored.as_object_mut().unwrap();
        map.insert("default_values".into(), defaults);
        map.insert("active_profile_id".into(), serde_json::json!("work"));
        map.insert(
            "profiles".into(),
            serde_json::json!([{
                "id": "work", "name": "Arbeit", "apps": [],
                "values": profile_values,
                "own": [],
                "own_entries": { "bindings": { "own": ["transcribe"], "off": [] } }
            }]),
        );
        let mut s = read(&stored);
        migrate_on(&mut s, &stored, KeyPlatform::Macos);
        let work = &s.profiles[0];
        assert_eq!(work.values.end_keys[0].shortcut, "f13");
        assert!(
            work.own_entries["end_keys"].own.contains(PLAIN_ID),
            "{:?}",
            work.own_entries
        );
        assert!(!work.own.contains("hold_ends"), "the same as the defaults");
        let mut after = s.clone();
        crate::profile_layers::propagate(&mut after);
        assert_eq!(after.profiles[0].values.end_keys[0].shortcut, "f13");
    }

    #[test]
    fn the_migration_runs_once() {
        for stored in [store_0_2_5(), store_dev()] {
            let mut s = read(&stored);
            assert!(migrate_on(&mut s, &stored, KeyPlatform::Macos));
            let written = serde_json::to_value(&s).unwrap();
            let mut again = read(&written);
            assert!(!migrate_on(&mut again, &written, KeyPlatform::Macos));
            assert_eq!(again.end_keys, s.end_keys);
            assert_eq!(again.hold_ends, s.hold_ends);
            assert_eq!(again.profiles, s.profiles);
        }
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
        s.hold_ends
            .get_mut("transcribe_with_post_process")
            .unwrap()
            .append_text = "{Win+Enter}".to_string();
        let stored = serde_json::to_value(&s).unwrap();
        let mut s = read(&stored);
        assert!(migrate_on(&mut s, &stored, KeyPlatform::Macos));
        assert_eq!(s.end_keys[1].append_text, "{Cmd+Enter}");
        assert_eq!(
            s.hold_ends["transcribe_with_post_process"].append_text,
            "{Cmd+Enter}"
        );
    }

    // ---- Import --------------------------------------------------------------

    #[test]
    fn an_import_from_before_end_shortcuts_gets_them_from_its_fields() {
        let file = "version: 1\nprofile:\n  id: work\n  name: Arbeit\n  values:\n    selected_language: en\n    append_text: '\\n\\n'\n    send_context: true\n";
        let mut values = ProfileValues::default();
        prepare_import_on(file, &mut values, KeyPlatform::Macos);
        assert_eq!(values.end_keys, builtin(r"\n\n", true));
        assert_eq!(values.hold_ends, builtin_holds(r"\n\n", true));
        assert!(
            values.bindings.is_empty(),
            "takes the importing installation's shortcuts"
        );
    }

    #[test]
    fn an_import_from_dev_turns_its_default_ends_into_holds() {
        let dev = values_with(
            vec![
                end(PLAIN_ID, "", "", r"\s", false, false),
                end(POST_PROCESS_ID, "", "", "{Enter}", true, true),
            ],
            &[],
        );
        let mut file_values = serde_json::to_value(&dev).unwrap();
        let map = file_values.as_object_mut().unwrap();
        map.remove("hold_ends");
        map.insert(
            "default_ends".into(),
            serde_json::json!({ "transcribe": "plain", "transcribe_with_post_process": "post_process" }),
        );
        let file = serde_saphyr::to_string(&serde_json::json!({
            "version": 1,
            "profile": { "id": "w", "name": "W", "values": file_values }
        }))
        .unwrap();
        let mut imported = dev.clone();
        prepare_import_on(&file, &mut imported, KeyPlatform::Macos);
        let (plain_keys, post_keys) = start_keys();
        assert_eq!(imported.end_keys[0].shortcut, plain_keys);
        assert_eq!(imported.end_keys[1].shortcut, post_keys);
        assert_eq!(
            imported.hold_ends["transcribe_with_post_process"],
            hold("{Enter}", true, true)
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
        assert_eq!(imported.hold_ends, values.hold_ends);
    }
}
