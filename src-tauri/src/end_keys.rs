//! DanyWhere: end shortcuts (idea 3; user, 2026-11-19, rethought
//! 2026-11-22 and 2026-11-26).
//!
//! A recording ends with one of the end shortcuts, a free list
//! (`ProfileValues::end_keys`) in which no entry is distinguished. Each has
//! a key combination of its own, which DanyWhere registers only while a
//! recording runs, like Escape (`shortcut::profile_sync`), and says what
//! follows: what is appended (the "Append" syntax of `append_text`) and
//! whether a post-processing gets earlier dictations as context. Whether the
//! text is post-processed is no end's business since variant b (user,
//! 2026-11-26): an instruction shortcut pressed during the recording, or the
//! profile's mode, says it (`settings::PostProcessMode`). Every entry keeps its
//! `id`, so the profiles can share entries one by one. Its name is free;
//! empty means it has none of its own and is shown by its combination.
//!
//! Start and hold shortcuts are lists of their own (`start_keys`). An end
//! may have exactly a start or hold shortcut's combination: pressing that
//! one during a recording is then a press of that end. A hold shortcut has
//! an end shortcut's form; releasing it ends with its own values
//! ([`EndChoice::Held`]).
//!
//! Out of the box macOS has six ends, named by what they do, Windows and
//! Linux two, `plain` and `post_process`, with the factory start and hold
//! combinations. The first read of an older store carries over what it had
//! ([`migrate`]).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Emitter};

use crate::append_text::KeyPlatform;
use crate::profiles::ProfileValues;
use crate::settings::{self, AppSettings, ShortcutBinding};
use crate::start_keys::{OldHold, LEGACY_START_BINDINGS};

/// The end with the start combination on Windows and Linux out of the box.
pub const PLAIN_ID: &str = "plain";
/// The end with the second factory combination (⇧) on Windows and Linux
/// out of the box; it post-processed until variant b, hence its id.
pub const POST_PROCESS_ID: &str = "post_process";
/// The prefix of an end shortcut's binding id while it is registered.
pub const BINDING_PREFIX: &str = "end_key:";
/// The prefix of the id an end by holding took in History up to
/// 2026-11-26 (`held:<start binding>`).
const LEGACY_HELD_PREFIX: &str = "held:";

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
    /// Whether post-processing, when it runs, gets earlier dictations as
    /// context. (A stored box "Post-process" of before variant b is ignored.)
    pub send_context: bool,
}

/// The two ends Windows and Linux start with, `plain` and `post_process`,
/// on combinations `keys`, with `append_text` and `send_context`.
pub fn builtin_for_keys(keys: (&str, &str), append_text: &str, send_context: bool) -> Vec<EndKey> {
    [(PLAIN_ID, keys.0), (POST_PROCESS_ID, keys.1)]
        .into_iter()
        .map(|(id, shortcut)| EndKey {
            id: id.to_string(),
            name: String::new(),
            shortcut: shortcut.to_string(),
            append_text: append_text.to_string(),
            send_context,
        })
        .collect()
}

/// This system's factory combinations.
fn factory_keys() -> (&'static str, &'static str) {
    crate::start_keys::factory_combinations_on(KeyPlatform::current())
}

/// [`builtin_for_keys`] with this system's factory combinations (tests).
#[cfg(test)]
pub fn builtin(append_text: &str, send_context: bool) -> Vec<EndKey> {
    builtin_for_keys(factory_keys(), append_text, send_context)
}

/// This system's factory hold shortcuts with `append_text` and
/// `send_context` (tests).
#[cfg(test)]
pub fn builtin_hold_keys(append_text: &str, send_context: bool) -> Vec<EndKey> {
    crate::start_keys::default_hold_keys()
        .into_iter()
        .map(|hold| EndKey {
            append_text: append_text.to_string(),
            send_context,
            ..hold
        })
        .collect()
}

/// Test fixtures elsewhere: `settings` with the two built-in ends and hold
/// shortcuts without text or context, the factory ones before 2026-11-24
/// and still on Windows and Linux. The dictation tests are written against
/// them (two ends, `plain` and `post_process`), whatever this system's
/// factory ends are.
#[cfg(test)]
pub fn with_builtin_ends(mut settings: AppSettings) -> AppSettings {
    settings.end_keys = builtin("", false);
    settings.hold_keys = builtin_hold_keys("", false);
    settings
}

/// The hold ends of an older store without them, with `append_text` and
/// `send_context`.
fn builtin_holds(append_text: &str, send_context: bool) -> [OldHold; 2] {
    [(); 2].map(|()| OldHold {
        append_text: append_text.to_string(),
        send_context,
    })
}

/// A new installation's end shortcuts on this system.
pub fn default_end_keys() -> Vec<EndKey> {
    settings::get_default_settings().end_keys
}

/// What a store without end shortcuts (0.2.5 and before) reads as, the serde
/// default of the settings and of a stored profile: the two built-in ends
/// with a new installation's combinations, without text or context, as
/// before the factory ends of 2026-11-24. [`migrate`] then carries the
/// store's own values over; kept apart from the factory ends so that a store
/// with nothing to carry over still needs no write.
pub fn stored_before_end_keys() -> Vec<EndKey> {
    builtin_for_keys(factory_keys(), "", false)
}

/// The words the names of macOS's factory ends are, in one language, e.g.
/// "Absatz" (until variant b "{mode} · {effect}", e.g. "Nachbearbeitet ·
/// Absatz"). The source is `settings.endKeys.factoryNames` of the locale
/// files, which build.rs carries into the tray's strings
/// (`factory_end_name_*`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FactoryEndNames {
    pub space: String,
    pub no_space: String,
    pub paragraph: String,
    pub line: String,
    pub enter: String,
    pub shift_enter: String,
}

impl FactoryEndNames {
    /// The words in the language of `locale` (e.g. the system's `de-AT`),
    /// found as the tray finds its strings: the locale, its language, else
    /// English.
    pub fn for_locale(locale: &str) -> Self {
        let strings = crate::tray_i18n::get_tray_translations(Some(locale.to_string()));
        Self {
            space: strings.factory_end_name_space,
            no_space: strings.factory_end_name_no_space,
            paragraph: strings.factory_end_name_paragraph,
            line: strings.factory_end_name_line,
            enter: strings.factory_end_name_enter,
            shift_enter: strings.factory_end_name_shift_enter,
        }
    }
}

/// One factory end: named by what it does, with "Send Context" (it counts
/// only where the dictation post-processes; recommendation 3 i).
fn factory_end(id: &str, shortcut: &str, append_text: &str, name: &str) -> EndKey {
    EndKey {
        id: id.to_string(),
        name: name.to_string(),
        shortcut: shortcut.to_string(),
        append_text: append_text.to_string(),
        send_context: true,
    }
}

/// macOS's factory ends of variant b, as (id, combination, Append): the six
/// direct ones of the user's twelve of 2026-11-24, with their ids.
const MACOS_SIX: [(&str, &str, &str); 6] = [
    ("direct_space", "ctrl+space", " "),
    ("direct_no_space", "ctrl+shift+space", ""),
    ("direct_paragraph", "return", r"\n\n"),
    ("direct_line", "shift+return", r"\n"),
    ("direct_enter", "ctrl+return", "{Enter}"),
    ("direct_shift_enter", "ctrl+shift+return", "{Shift+Enter}"),
];

/// The combinations the six had with ⌥ until variant b, which then
/// post-processed (the user's rule of 2026-11-23: "⌥ post-processes").
const MACOS_OLD_POST: [(&str, &str); 6] = [
    ("post_space", "ctrl+option+space"),
    ("post_no_space", "ctrl+option+shift+space"),
    ("post_paragraph", "option+return"),
    ("post_line", "option+shift+return"),
    ("post_enter", "ctrl+option+return"),
    ("post_shift_enter", "ctrl+option+shift+return"),
];

/// The name of the factory end `index` of [`MACOS_SIX`] in `names`.
fn six_name(names: &FactoryEndNames, index: usize) -> &str {
    [
        &names.space,
        &names.no_space,
        &names.paragraph,
        &names.line,
        &names.enter,
        &names.shift_enter,
    ][index]
}

/// A new installation's end shortcuts on `platform`.
///
/// macOS (variant b, user, 2026-11-26; recommendation 2 i): the six ends
/// without post-processing of the user's twelve of 2026-11-24 (qa note
/// `2026-11-23_2135_endetasten-grundeinstellungen`): ⇧ picks the smaller
/// variant, ⌃ on Enter presses a key instead of appending text; named only
/// by what they do, all with "Send Context". The ⌥ variants went with the
/// box "Post-process": an instruction shortcut says it now. ⌃Space is the
/// factory start shortcut, so its second press ends with a space.
///
/// Windows and Linux keep the two ends with the factory start and hold
/// combinations: Linux takes no end's own combination
/// (`shortcut::profile_sync`), and on Windows Ctrl+Alt is AltGr.
///
/// The macOS ends get names from `names` (user, 2026-11-24, "Variante 2"):
/// written once, in the language of a new installation, and from then on
/// names like typed ones, which follow neither a change of language nor a
/// changed end. Windows' and Linux' two ends stay unnamed.
pub fn factory_end_keys_on(platform: KeyPlatform, names: &FactoryEndNames) -> Vec<EndKey> {
    if platform != KeyPlatform::Macos {
        return builtin_for_keys(
            crate::start_keys::factory_combinations_on(platform),
            "",
            false,
        );
    }
    MACOS_SIX
        .iter()
        .enumerate()
        .map(|(index, (id, shortcut, append_text))| {
            factory_end(id, shortcut, append_text, six_name(names, index))
        })
        .collect()
}

/// macOS's twelve factory ends from 2026-11-24 until variant b, without
/// their names and their box "Post-process", which a read drops: the six,
/// without context, then the same six with ⌥, with context.
fn old_macos_twelve() -> Vec<EndKey> {
    let direct = MACOS_SIX.iter().map(|(id, shortcut, append_text)| EndKey {
        send_context: false,
        ..factory_end(id, shortcut, append_text, "")
    });
    let post = MACOS_OLD_POST
        .iter()
        .zip(MACOS_SIX.iter())
        .map(|((id, shortcut), (_, _, append_text))| factory_end(id, shortcut, append_text, ""));
    direct.chain(post).collect()
}

/// The step to variant b for one layer's end shortcuts on macOS: a list
/// that is still the twelve factory ends of before (but for their names)
/// becomes the six of [`factory_end_keys_on`], named in the language of
/// `names` (migration may stay cheap: a name of one's own goes with it); a
/// list of one's own keeps its ends, only without the box. Returns whether
/// it changed.
fn twelve_to_six(end_keys: &mut Vec<EndKey>, names: &FactoryEndNames) -> bool {
    let unnamed: Vec<EndKey> = end_keys
        .iter()
        .map(|end| EndKey {
            name: String::new(),
            ..end.clone()
        })
        .collect();
    if unnamed != old_macos_twelve() {
        return false;
    }
    log::info!("The twelve factory ends become the six of variant b");
    *end_keys = factory_end_keys_on(KeyPlatform::Macos, names);
    true
}

/// Without Accessibility DanyWhere can't press keys, not even with its
/// input method, so the factory ends whose "Append" presses one go; the
/// others stay in their order. A list that isn't this system's factory list
/// is the user's and stays as it is; the names don't count, as they are
/// written in the language of the first start and a name changes nothing an
/// end does. Returns whether any went.
pub fn drop_factory_key_presses(end_keys: &mut Vec<EndKey>) -> bool {
    let unnamed = |ends: &[EndKey]| -> Vec<EndKey> {
        ends.iter()
            .map(|end| EndKey {
                name: String::new(),
                ..end.clone()
            })
            .collect()
    };
    if unnamed(end_keys) != unnamed(&default_end_keys()) {
        return false;
    }
    let before = end_keys.len();
    end_keys.retain(|end| {
        crate::append_text::for_value(&end.append_text)
            .keys
            .is_empty()
    });
    end_keys.len() != before
}

/// How a recording ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndChoice {
    /// With the end shortcut of this id: pressed, or a start or hold
    /// shortcut with exactly its combination was.
    Key(String),
    /// With the values of the hold shortcut of this id: it was held and
    /// released (in History `hold:<id>`).
    Held(String),
    /// As a press of the start shortcut of this id would: with the end
    /// shortcut of its combination, else the hold shortcut of its
    /// combination, else the profile's first end shortcut. For the
    /// services, remote control and Handy's `stop`, which must always end.
    Start(String),
    /// With the values of the instruction shortcut of this id: it was held
    /// and released (in History `instruction:<id>`), after the recording
    /// went on to a spoken instruction (`instruction_keys`).
    Instruction(String),
    /// As the inner choice, after an instruction shortcut was pressed
    /// without a spoken instruction: remote control's and the service's
    /// post-processing toggle (`start_keys::remote_toggle`). The dictation
    /// post-processes unless its profile never does.
    AfterInstructionKey(Box<EndChoice>),
}

impl EndChoice {
    /// Whether an instruction shortcut came before this end, so the
    /// dictation post-processes unless its profile never does
    /// (`dictation_snapshot::wants_post_processing`).
    pub fn after_instruction_key(&self) -> bool {
        matches!(self, Self::Instruction(_) | Self::AfterInstructionKey(_))
    }

    /// This end, after an instruction shortcut: unchanged when one came
    /// before it already.
    pub fn with_instruction_key(self) -> EndChoice {
        if self.after_instruction_key() {
            self
        } else {
            Self::AfterInstructionKey(Box::new(self))
        }
    }
}

/// The end shortcut `id` in `values`.
fn find<'a>(values: &'a ProfileValues, id: &str) -> Option<&'a EndKey> {
    values.end_keys.iter().find(|end| end.id == id)
}

/// The end of hold shortcut `hold_id` in `values`: its values, under its
/// History id (`hold:<id>`).
pub fn hold_end(values: &ProfileValues, hold_id: &str) -> Option<EndKey> {
    values
        .hold_keys
        .iter()
        .find(|hold| hold.id == hold_id)
        .map(|hold| EndKey {
            id: format!("{}{}", crate::start_keys::HOLD_END_PREFIX, hold.id),
            ..hold.clone()
        })
}

/// The profile's first end shortcut; a list never is empty (`validate`).
fn first_end(values: &ProfileValues) -> EndKey {
    values.end_keys.first().cloned().unwrap_or_else(|| {
        log::warn!("A profile without end shortcuts; ending without one");
        EndKey::default()
    })
}

/// The end shortcut with exactly combination `keys` in `values`
/// (`key_sides::identical_press`): one that fires on fewer or more presses
/// is another combination.
pub fn end_with_combination<'a>(values: &'a ProfileValues, keys: &str) -> Option<&'a EndKey> {
    if keys.trim().is_empty() {
        return None;
    }
    values
        .end_keys
        .iter()
        .find(|end| crate::key_sides::identical_press(&end.shortcut, keys))
}

/// [`EndChoice::Start`]: the end of the start shortcut's combination, else
/// the hold shortcut of its combination, else the first end.
fn as_start(values: &ProfileValues, start_id: &str) -> EndKey {
    let Some(key) = values.start_keys.iter().find(|key| key.id == start_id) else {
        return first_end(values);
    };
    end_with_combination(values, &key.shortcut)
        .cloned()
        .or_else(|| {
            values
                .hold_keys
                .iter()
                .find(|hold| crate::key_sides::identical_press(&hold.shortcut, &key.shortcut))
                .and_then(|hold| hold_end(values, &hold.id))
        })
        .unwrap_or_else(|| first_end(values))
}

/// How a recording registered as `binding_id` ends as a press of its key:
/// a start shortcut's as [`EndChoice::Start`], a hold shortcut's with its
/// values, anything else as [`fallback_end`].
fn as_press(values: &ProfileValues, binding_id: &str) -> EndKey {
    if let Some(id) = crate::start_keys::start_id_of(binding_id) {
        return as_start(values, id);
    }
    crate::start_keys::hold_id_of(binding_id)
        .and_then(|id| hold_end(values, id))
        .unwrap_or_else(|| fallback_end(values))
}

/// The end a dictation with `values`, started with `recording_binding`,
/// ends with by `choice`. A pressed end shortcut or a held instruction
/// shortcut the profile doesn't have (it changed meanwhile) ends as a press
/// of the recording's key, a hold shortcut it doesn't have as
/// [`fallback_end`].
pub fn resolve(values: &ProfileValues, choice: &EndChoice, recording_binding: &str) -> EndKey {
    match choice {
        EndChoice::Key(id) => find(values, id).cloned().unwrap_or_else(|| {
            log::warn!("End shortcut '{id}' not in the profile; ending as its key's press");
            as_press(values, recording_binding)
        }),
        EndChoice::Held(id) => hold_end(values, id).unwrap_or_else(|| {
            log::warn!("Hold shortcut '{id}' not in the profile; ending as no end chose");
            fallback_end(values)
        }),
        EndChoice::Start(id) => as_start(values, id),
        EndChoice::Instruction(id) => crate::instruction_keys::instruction_end(values, id)
            .unwrap_or_else(|| {
                log::warn!(
                    "Instruction shortcut '{id}' not in the profile; ending as its key's press"
                );
                as_press(values, recording_binding)
            }),
        EndChoice::AfterInstructionKey(inner) => resolve(values, inner, recording_binding),
    }
}

/// The end of id `id` in `values`: an end shortcut, a hold shortcut's end
/// by its History id (`hold:<id>`), or a held instruction shortcut's
/// (`instruction:<id>`); one of before hold shortcuts (`held:transcribe`,
/// `held:transcribe_with_post_process`) is the first, or the second, hold
/// shortcut, where the start shortcuts' hold ends went
/// (`start_keys::from_old`). For redoing a dictation for another profile
/// (`profile_redo`).
pub fn find_end(values: &ProfileValues, id: &str) -> Option<EndKey> {
    if let Some(hold) = id.strip_prefix(crate::start_keys::HOLD_END_PREFIX) {
        return hold_end(values, hold);
    }
    if let Some(key) = id.strip_prefix(crate::instruction_keys::END_PREFIX) {
        return crate::instruction_keys::instruction_end(values, key);
    }
    if let Some(binding) = id.strip_prefix(LEGACY_HELD_PREFIX) {
        let index = LEGACY_START_BINDINGS
            .iter()
            .position(|legacy| *legacy == binding)?;
        return values
            .hold_keys
            .get(index)
            .and_then(|hold| hold_end(values, &hold.id));
    }
    find(values, id).cloned()
}

/// The end a text gets that no end chose ("Paste Last Transcript", "Insert
/// Last Dictation" and "Copy Last Transcript" outside a dictation, before
/// DanyWhere knows how the last one ended): as a press of the first start
/// shortcut; without one as the first hold shortcut; else the first end.
pub fn fallback_end(values: &ProfileValues) -> EndKey {
    if let Some(start) = values.start_keys.first() {
        return as_start(values, &start.id);
    }
    values
        .hold_keys
        .first()
        .and_then(|hold| hold_end(values, &hold.id))
        .unwrap_or_else(|| first_end(values))
}

/// The end a recording started with `binding_id` is expected to take, for
/// what shows and loads at its start: as a press of its key would end it
/// (a hold shortcut's own values).
pub fn expected_end(values: &ProfileValues, binding_id: &str) -> EndKey {
    as_press(values, binding_id)
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

/// A layer that could always end a recording before a change must still
/// after it: it keeps an end shortcut. A layer that couldn't before isn't
/// held to it, so an older store never blocks a change. (Linux' rule, that
/// every start shortcut needs an end with its combination, is
/// `start_keys::still_starts`.)
pub fn still_ends(before: &ProfileValues, after: &ProfileValues) -> Result<(), String> {
    if !before.end_keys.is_empty() && after.end_keys.is_empty() {
        return Err("end-key-last".to_string());
    }
    Ok(())
}

/// [`still_ends`] for the defaults and every profile of `before` and
/// `after` (`profile_layers::commit`).
pub fn check_layers(before: &AppSettings, after: &AppSettings) -> Result<(), String> {
    still_ends(
        &crate::profiles::default_values(before),
        &crate::profiles::default_values(after),
    )?;
    for profile in &after.profiles {
        if let Some(old) = before.profiles.iter().find(|old| old.id == profile.id) {
            still_ends(&old.values, &profile.values)?;
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
///   built-in ones with the combinations of its start shortcuts, all with
///   the "Append" text (or Handy's trailing space and auto submit before
///   that) and the "Send Context" it had;
/// - once, a value with end shortcuts but no hold ends (Dev, 2026-11-19 to
///   2026-11-22) turns its default ends into hold ends ([`from_default_ends`]);
///   a profile's entry that then differs from the defaults becomes its own;
/// - a profile without shortcuts gets the flat ones (those it used);
/// - the retired shortcuts "Transcribe with Context" and "Send Context" go;
/// - once, a value from before start and hold shortcuts (2026-11-26) gets
///   them from its start shortcuts, their behavior and their hold ends
///   (`start_keys::migrate`);
/// - once, variant b (2026-11-26): the twelve factory ends of macOS become
///   the six, the factory hold shortcuts of before the new ones
///   ([`to_variant_b`]); every end's box "Post-process" went when the store
///   was read;
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
    // Whether the second start shortcut was taken, before anything changes.
    let reachable = crate::app_profiles::post_processing_reachable(settings);
    let mut changed = drop_retired_bindings(&mut settings.bindings);
    // The settings: the values in use.
    let (flat_changed, flat_holds) = migrate_layer(
        Some(settings_value),
        &mut settings.end_keys,
        &settings.bindings,
        settings.append_trailing_space,
        (auto_submit, key, platform),
    );
    changed |= flat_changed;
    // The defaults.
    let stored_defaults = settings_value.get("default_values");
    let mut default_holds = None;
    if let Some(defaults) = settings.default_values.as_mut() {
        changed |= drop_retired_bindings(&mut defaults.bindings);
        let (defaults_changed, holds) = migrate_layer(
            stored_defaults,
            &mut defaults.end_keys,
            &defaults.bindings,
            crate::append_text::stored_trailing_space(stored_defaults),
            (auto_submit, key, platform),
        );
        changed |= defaults_changed;
        default_holds = Some(holds);
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
        let from_dev = !lacks("end_keys") && lacks("hold_ends") && lacks("start_keys");
        let values = &mut profile.values;
        let (profile_changed, _) = migrate_layer(
            stored_values,
            &mut values.end_keys,
            &values.bindings,
            crate::append_text::stored_trailing_space(stored_values),
            (auto_submit, key, platform),
        );
        changed |= profile_changed;
        // Its coupled entries now differ where its start shortcuts do: they
        // become its own, so the defaults don't overwrite them.
        if let (true, Some(defaults)) = (from_dev, defaults.as_ref()) {
            changed |= crate::profile_layers::own_where_different(profile, defaults, "end_keys");
        }
    }

    // DanyWhere: the start and hold shortcuts (2026-11-26).
    changed |= crate::start_keys::migrate(
        settings,
        settings_value,
        reachable,
        &flat_holds,
        default_holds.as_ref(),
    );

    // DanyWhere: variant b (2026-11-26).
    changed |= to_variant_b(settings, platform);

    let ends = settings
        .end_keys
        .iter_mut()
        .chain(settings.hold_keys.iter_mut())
        .chain(settings.default_values.iter_mut().flat_map(|values| {
            values
                .end_keys
                .iter_mut()
                .chain(values.hold_keys.iter_mut())
        }))
        .chain(settings.profiles.iter_mut().flat_map(|profile| {
            profile
                .values
                .end_keys
                .iter_mut()
                .chain(profile.values.hold_keys.iter_mut())
        }));
    for end in ends {
        changed |= localize_in_place(&mut end.append_text, platform);
    }
    changed
}

/// The step to variant b (user, 2026-11-26) for every layer: on macOS a
/// list of the twelve factory ends of before becomes the six, named in the
/// interface's language ([`twelve_to_six`]); factory hold shortcuts of
/// before become the new ones (`start_keys::replace_old_factory_holds`).
/// A profile whose list changed loses its own entries and switched-off ones
/// that are gone with it. Returns whether anything changed; a second run
/// changes nothing.
fn to_variant_b(settings: &mut AppSettings, platform: KeyPlatform) -> bool {
    let names = FactoryEndNames::for_locale(&settings.app_language);
    let step = |end_keys: &mut Vec<EndKey>, hold_keys: &mut Vec<EndKey>| {
        let ends = platform == KeyPlatform::Macos && twelve_to_six(end_keys, &names);
        let holds = crate::start_keys::replace_old_factory_holds(hold_keys, platform);
        (ends, holds)
    };
    let (ends, holds) = step(&mut settings.end_keys, &mut settings.hold_keys);
    let mut changed = ends || holds;
    if let Some(defaults) = settings.default_values.as_mut() {
        let (ends, holds) = step(&mut defaults.end_keys, &mut defaults.hold_keys);
        changed |= ends || holds;
    }
    let defaults = settings.default_values.clone();
    for profile in settings.profiles.iter_mut() {
        let (ends, holds) = step(&mut profile.values.end_keys, &mut profile.values.hold_keys);
        for (field, stepped) in [("end_keys", ends), ("hold_keys", holds)] {
            if stepped {
                changed = true;
                prune_own_entries(profile, defaults.as_ref(), field);
            }
        }
    }
    changed
}

/// Drops the own entries of list `field` (`end_keys` or `hold_keys`) of
/// `profile` that its list no longer has, and the switched-off ones the
/// defaults no longer have.
fn prune_own_entries(
    profile: &mut crate::profiles::Profile,
    defaults: Option<&ProfileValues>,
    field: &str,
) {
    let ids = |values: &ProfileValues| -> Vec<String> {
        let list = if field == "end_keys" {
            &values.end_keys
        } else {
            &values.hold_keys
        };
        list.iter().map(|end| end.id.clone()).collect()
    };
    let mine = ids(&profile.values);
    let theirs = defaults.map(ids).unwrap_or_default();
    if let Some(entries) = profile.own_entries.get_mut(field) {
        entries.own.retain(|id| mine.contains(id));
        entries.off.retain(|id| theirs.contains(id));
        if entries.is_empty() {
            profile.own_entries.remove(field);
        }
    }
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
/// Returns whether its end shortcuts changed, and its hold ends as a store
/// from before start and hold shortcuts had them (`start_keys::migrate`
/// reads them).
fn migrate_layer(
    stored: Option<&serde_json::Value>,
    end_keys: &mut Vec<EndKey>,
    bindings: &HashMap<String, ShortcutBinding>,
    trailing_space: bool,
    legacy: (bool, crate::settings::AutoSubmitKey, KeyPlatform),
) -> (bool, [OldHold; 2]) {
    let lacks = |field: &str| stored.is_none_or(|values| values.get(field).is_none());
    if lacks("end_keys") {
        let (auto_submit, key, platform) = legacy;
        let append =
            crate::append_text::stored_value(stored, trailing_space, auto_submit, key, platform);
        let send_context = stored
            .and_then(|values| values.get("send_context"))
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let (first, second) = crate::start_keys::legacy_combinations(bindings);
        let ends = builtin_for_keys((&first, &second), &append, send_context);
        let changed = *end_keys != ends;
        if changed {
            log::info!("End shortcuts take over \"Append\" and \"Send Context\"");
        }
        *end_keys = ends;
        return (changed, builtin_holds(&append, send_context));
    }
    if lacks("start_keys") && lacks("hold_ends") {
        let default_ends: HashMap<String, String> = stored
            .and_then(|values| values.get("default_ends"))
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default();
        return from_default_ends(end_keys, bindings, &default_ends);
    }
    (false, stored_holds(stored))
}

/// The hold ends stored values from before start and hold shortcuts held
/// (`hold_ends`, by start shortcut); a missing one without text or context.
fn stored_holds(stored: Option<&serde_json::Value>) -> [OldHold; 2] {
    let holds: HashMap<String, OldHold> = stored
        .and_then(|values| values.get("hold_ends"))
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default();
    [0, 1].map(|index| {
        holds
            .get(LEGACY_START_BINDINGS[index])
            .cloned()
            .unwrap_or_default()
    })
}

/// The end that has an old start shortcut's combination out of the box.
fn builtin_id(index: usize) -> &'static str {
    if index == 1 {
        POST_PROCESS_ID
    } else {
        PLAIN_ID
    }
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
/// Returns whether the end shortcuts changed, and the hold ends.
fn from_default_ends(
    end_keys: &mut Vec<EndKey>,
    bindings: &HashMap<String, ShortcutBinding>,
    default_ends: &HashMap<String, String>,
) -> (bool, [OldHold; 2]) {
    let before = end_keys.clone();
    let position = |ends: &[EndKey], id: &str| ends.iter().position(|end| end.id == id);
    let default_of = |ends: &[EndKey], index: usize| {
        default_ends
            .get(LEGACY_START_BINDINGS[index])
            .and_then(|id| position(ends, id))
            .or_else(|| position(ends, builtin_id(index)))
    };
    let holds = [0, 1].map(|index| match default_of(end_keys, index) {
        Some(at) => OldHold {
            append_text: end_keys[at].append_text.clone(),
            send_context: end_keys[at].send_context,
        },
        None => OldHold::default(),
    });
    let take = |ends: &mut Vec<EndKey>, index: Option<usize>, keys: &str| {
        let Some(index) = index else { return };
        let free = !ends
            .iter()
            .any(|end| crate::key_sides::same_press(&end.shortcut, keys));
        if ends[index].shortcut.trim().is_empty() && free {
            ends[index].shortcut = keys.to_string();
        }
    };
    let (first, second) = crate::start_keys::legacy_combinations(bindings);
    let keys = [first, second];
    for (index, combination) in keys.iter().enumerate() {
        let at = default_of(end_keys, index);
        take(end_keys, at, combination);
    }
    for (index, combination) in keys.iter().enumerate() {
        let at = position(end_keys, builtin_id(index));
        take(end_keys, at, combination);
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
    log::info!("Default ends became hold ends");
    (*end_keys != before, holds)
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
/// space) and "Send Context", one from Dev its ends from its default ends;
/// a file without shortcuts takes the importing installation's (an empty
/// map, which `profiles::ensure_profiles` fills); the old start shortcuts
/// go: a file from before start and hold shortcuts takes the defaults'
/// (`profile_transfer::prepare_import_values` fills them in); Append texts
/// get `platform`'s key names.
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
            &bindings,
            crate::append_text::stored_trailing_space(stored),
            (false, crate::settings::AutoSubmitKey::Enter, platform),
        );
        let own_start_values = !lacks("shortcut_activation")
            || !lacks("hold_ends")
            || LEGACY_START_BINDINGS
                .iter()
                .any(|id| bindings.contains_key(*id));
        if lacks("start_keys") && own_start_values {
            log::info!("The imported profile's start shortcuts give way to the defaults' start and hold shortcuts");
        }
        if lacks("bindings") {
            values.bindings.clear();
        }
    }
    drop_retired_bindings(&mut values.bindings);
    crate::start_keys::drop_legacy_bindings(&mut values.bindings);
    for end in values
        .end_keys
        .iter_mut()
        .chain(values.hold_keys.iter_mut())
    {
        end.append_text = crate::append_text::localize(&end.append_text, platform);
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
    let shortcut = shortcut.trim();
    let mut ids: Vec<&String> = bindings.keys().collect();
    ids.sort();
    match ids.into_iter().find(|id| {
        let keys = bindings[*id].current_binding.trim();
        !keys.is_empty() && crate::key_sides::same_press(keys, shortcut)
    }) {
        Some(id) => Err(format!("conflict:{id}")),
        None => Ok(()),
    }
}

/// Checks the end shortcuts of `values` as their change command does: the
/// list, and each combination valid for `implementation` and free
/// (`start_keys::check_combination`), "Append" in this system's key names.
pub fn check_end_keys(
    values: &mut ProfileValues,
    implementation: crate::settings::KeyboardImplementation,
) -> Result<(), String> {
    validate(&values.end_keys)?;
    for end in values.end_keys.iter_mut() {
        end.append_text = crate::append_text::localize(&end.append_text, KeyPlatform::current());
    }
    for end in &values.end_keys {
        crate::shortcut::validate_shortcut_for_implementation(&end.shortcut, implementation)?;
        crate::start_keys::check_combination(
            &end.shortcut,
            crate::start_keys::KeyList::End,
            values,
        )?;
    }
    Ok(())
}

/// Stores the active profile's end shortcuts (the whole list, as the editor
/// holds it). A running recording takes the new ones at once.
#[tauri::command]
#[specta::specta]
pub fn change_end_keys_setting(app: AppHandle, end_keys: Vec<EndKey>) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    let before = ProfileValues::from_settings(&settings);
    let mut after = before.clone();
    after.end_keys = end_keys;
    check_end_keys(&mut after, settings.keyboard_implementation)?;
    still_ends(&before, &after)?;
    crate::start_keys::still_starts(&before, &after, cfg!(target_os = "linux"))?;
    settings.end_keys = after.end_keys;
    settings::write_settings(&app, settings);
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({ "setting": "end_keys" }),
    );
    crate::shortcut::profile_sync::sync_now(&app);
    Ok(())
}

/// The twelve ends macOS had out of the box until 2026-11-26, as a store
/// holds them, named in German.
#[cfg(test)]
pub fn twelve_stored() -> serde_json::Value {
    let six = [
        (
            "space",
            "Leerzeichen",
            "ctrl+space",
            "ctrl+option+space",
            " ",
        ),
        (
            "no_space",
            "ohne Leerzeichen",
            "ctrl+shift+space",
            "ctrl+option+shift+space",
            "",
        ),
        ("paragraph", "Absatz", "return", "option+return", r"\n\n"),
        (
            "line",
            "Zeile",
            "shift+return",
            "option+shift+return",
            r"\n",
        ),
        (
            "enter",
            "Enter senden",
            "ctrl+return",
            "ctrl+option+return",
            "{Enter}",
        ),
        (
            "shift_enter",
            "Shift-Enter senden",
            "ctrl+shift+return",
            "ctrl+option+shift+return",
            "{Shift+Enter}",
        ),
    ];
    let mut ends = Vec::new();
    for post in [false, true] {
        for (id, effect, direct_keys, post_keys, append) in six {
            let (prefix, mode, keys) = if post {
                ("post", "Nachbearbeitet", post_keys)
            } else {
                ("direct", "Direkt", direct_keys)
            };
            ends.push(serde_json::json!({
                "id": format!("{prefix}_{id}"),
                "name": format!("{mode} · {effect}"),
                "shortcut": keys,
                "append_text": append,
                "post_process": post,
                "send_context": post,
            }));
        }
    }
    serde_json::Value::Array(ends)
}

/// The hold shortcuts macOS had out of the box from 2026-11-26 until
/// variant b, as a store holds them.
#[cfg(test)]
pub fn r_holds_stored() -> serde_json::Value {
    serde_json::json!([
        { "id": "hold_direct", "name": "", "shortcut": "ctrl+space",
          "append_text": " ", "post_process": false, "send_context": false },
        { "id": "hold_post", "name": "", "shortcut": "ctrl+shift+space",
          "append_text": " ", "post_process": true, "send_context": true },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::get_default_settings;
    use crate::start_keys::{KeyList, StartKey, HOLD_DIRECT_ID, HOLD_POST_ID};

    fn end(id: &str, name: &str, shortcut: &str, append: &str, ctx: bool) -> EndKey {
        EndKey {
            id: id.to_string(),
            name: name.to_string(),
            shortcut: shortcut.to_string(),
            append_text: append.to_string(),
            send_context: ctx,
        }
    }

    /// This system's factory combinations: the start one and the one that
    /// holds with post-processing.
    fn start_keys() -> (String, String) {
        let (first, second) = factory_keys();
        (first.to_string(), second.to_string())
    }

    /// Values with `end_keys` and nothing else of their own.
    fn values_with(end_keys: Vec<EndKey>) -> ProfileValues {
        ProfileValues {
            end_keys,
            ..Default::default()
        }
    }

    // ---- Out of the box --------------------------------------------------

    /// The names a list of ends carries, in its order.
    fn names_of(ends: &[EndKey]) -> Vec<&str> {
        ends.iter().map(|end| end.name.as_str()).collect()
    }

    /// The macOS factory ends named in `locale`.
    fn macos_ends_in(locale: &str) -> Vec<EndKey> {
        factory_end_keys_on(KeyPlatform::Macos, &FactoryEndNames::for_locale(locale))
    }

    #[test]
    fn a_new_installation_gets_this_systems_factory_ends() {
        let settings = get_default_settings();
        assert_eq!(
            settings.end_keys,
            factory_end_keys_on(
                KeyPlatform::current(),
                &FactoryEndNames::for_locale(&settings.app_language)
            )
        );
    }

    /// Variante 2 (user, 2026-11-24): a new installation writes the names
    /// once, in its language (the system's, as `app_language` starts).
    #[cfg(target_os = "macos")]
    #[test]
    fn on_macos_a_new_installation_names_its_ends_in_its_language() {
        let settings = get_default_settings();
        assert_eq!(
            names_of(&settings.end_keys),
            names_of(&macos_ends_in(&settings.app_language))
        );
        assert!(
            settings.end_keys.iter().all(|end| !end.name.is_empty()),
            "{:?}",
            names_of(&settings.end_keys)
        );
    }

    /// A regional locale (the system's, e.g. `de-AT`) takes its language's
    /// names, as the tray does.
    #[test]
    fn a_regional_locale_takes_its_languages_names() {
        let german = FactoryEndNames::for_locale("de");
        assert_eq!(german.space, "Leerzeichen");
        assert_eq!(FactoryEndNames::for_locale("de-AT"), german);
        assert_eq!(FactoryEndNames::for_locale("de_CH"), german);
    }

    #[test]
    fn an_unsupported_locale_takes_the_english_names() {
        let english = FactoryEndNames::for_locale("en");
        assert_eq!(english.no_space, "No space");
        assert_eq!(FactoryEndNames::for_locale("xx-YY"), english);
    }

    /// The first two macOS ends are what a second press of the factory
    /// start and hold combinations does, so a recording started with
    /// ⌃Space ends with it.
    #[cfg(target_os = "macos")]
    #[test]
    fn on_macos_the_factory_combinations_end_without_post_processing() {
        let values = ProfileValues::from_settings(&get_default_settings());
        let (start, post) = start_keys();
        assert_eq!(
            end_with_combination(&values, &start).map(|end| end.id.as_str()),
            Some("direct_space")
        );
        assert_eq!(
            end_with_combination(&values, &post).map(|end| end.id.as_str()),
            Some("direct_no_space")
        );
    }

    #[test]
    fn every_factory_end_is_a_valid_one() {
        let english = FactoryEndNames::for_locale("en");
        for platform in [KeyPlatform::Macos, KeyPlatform::Windows, KeyPlatform::Linux] {
            let ends = factory_end_keys_on(platform, &english);
            assert_eq!(validate(&ends), Ok(()), "{platform:?}");
        }
        // This system's factory ends against its factory shortcuts and the
        // backend that takes them with Accessibility.
        let values = ProfileValues::from_settings(&get_default_settings());
        for end in &values.end_keys {
            assert_eq!(
                crate::start_keys::check_combination(&end.shortcut, KeyList::End, &values),
                Ok(()),
                "{}",
                end.shortcut
            );
            assert_eq!(
                crate::shortcut::validate_shortcut_for_implementation(
                    &end.shortcut,
                    crate::settings::KeyboardImplementation::HandyKeys,
                ),
                Ok(()),
                "{}",
                end.shortcut
            );
        }
    }

    #[test]
    fn the_built_in_ones_carry_the_append_text_and_the_context_switch() {
        let (_, post_keys) = start_keys();
        assert_eq!(
            builtin_for_keys(("f13", &post_keys), r"\n\n", true),
            vec![
                end(PLAIN_ID, "", "f13", r"\n\n", true),
                end(POST_PROCESS_ID, "", &post_keys, r"\n\n", true),
            ]
        );
    }

    // ---- Which end a recording takes ---------------------------------------

    #[test]
    fn an_end_shortcut_ends_with_its_own_values() {
        let mut keys = builtin("", false);
        keys.push(end("line", "Line", "shift+enter", r"\n", false));
        let values = values_with(keys);
        let chosen = resolve(
            &values,
            &EndChoice::Key("line".to_string()),
            "start_key:start",
        );
        assert_eq!(chosen, end("line", "Line", "shift+enter", r"\n", false));
    }

    #[test]
    fn an_end_has_a_combination_only_when_it_is_exactly_the_same() {
        let mut values = values_with(builtin("", false));
        let (plain_keys, _) = start_keys();
        assert_eq!(
            end_with_combination(&values, &plain_keys).map(|end| end.id.as_str()),
            Some(PLAIN_ID)
        );
        values.end_keys[0].shortcut = plain_keys.to_uppercase();
        assert_eq!(
            end_with_combination(&values, &plain_keys).map(|end| end.id.as_str()),
            Some(PLAIN_ID),
            "written in capitals, still the same press"
        );
        values.end_keys[0].shortcut = "ctrl_left+return".to_string();
        assert!(
            end_with_combination(&values, "ctrl+return").is_none(),
            "one side only is another combination: it fires on fewer presses"
        );
        assert!(end_with_combination(&values, "").is_none());
    }

    #[test]
    fn an_end_is_found_by_its_id() {
        let values = values_with(builtin("", false));
        assert_eq!(
            find_end(&values, PLAIN_ID).map(|end| end.id),
            Some(PLAIN_ID.to_string())
        );
        assert!(find_end(&values, "gone").is_none());
    }

    // ---- Registering -------------------------------------------------------

    #[test]
    fn an_end_shortcut_is_registered_under_its_own_binding_id() {
        let id = binding_id("send");
        assert_eq!(id, "end_key:send");
        assert_eq!(end_id_of(&id), Some("send"));
        assert_eq!(end_id_of("start_key:start"), None);
    }

    #[test]
    fn only_end_shortcuts_with_a_key_combination_are_registered() {
        let keys = vec![
            end("send", "Send", "ctrl+enter", "{Enter}", false),
            end("blank", "Blank", "  ", "", false),
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
        keys.push(end("x", "X", "  ", "", false));
        assert_eq!(validate(&keys).unwrap_err(), "end-key-needs-shortcut:x");
    }

    #[test]
    fn a_list_needs_unique_ids_and_combinations_but_no_names() {
        let mut keys = builtin("", false);
        assert!(validate(&keys).is_ok());
        keys.push(end("send", "", "ctrl+enter", "", false));
        assert!(validate(&keys).is_ok(), "a name of its own is optional");

        let mut twice = keys.clone();
        twice.push(end("send", "Other", "f14", "", false));
        assert!(validate(&twice).is_err(), "id twice");

        let mut no_id = keys.clone();
        no_id.push(end("", "Named", "f14", "", false));
        assert!(validate(&no_id).is_err(), "no id");

        let mut same_keys = keys.clone();
        same_keys.push(end("y", "Y", "ctrl+enter", "", false));
        assert_eq!(
            validate(&same_keys).unwrap_err(),
            "end-key-shortcut-twice:ctrl+enter"
        );
    }

    #[test]
    fn a_combination_counts_twice_when_one_press_fires_both() {
        let mut keys = builtin("", false);
        keys.push(end("send", "Send", "ctrl+return", "", false));
        keys.push(end("left", "Left", "ctrl_left+return", "", false));
        assert_eq!(
            validate(&keys).unwrap_err(),
            "end-key-shortcut-twice:ctrl_left+return"
        );
        let mut apart = builtin("", false);
        apart.push(end("left", "Left", "ctrl_left+return", "", false));
        apart.push(end("right", "Right", "ctrl_right+return", "", false));
        assert!(validate(&apart).is_ok(), "the left and the right Ctrl");
    }

    #[test]
    fn the_list_is_never_empty() {
        assert_eq!(validate(&[]).unwrap_err(), "end-key-last");
    }

    /// The factory start shortcut and a hold shortcut on F13.
    fn with_start_and_hold() -> ProfileValues {
        let (start, _) = start_keys();
        ProfileValues {
            start_keys: vec![StartKey {
                id: "start".to_string(),
                shortcut: start,
            }],
            hold_keys: vec![end("hold", "", "f13", "", false)],
            ..values_with(builtin("", false))
        }
    }

    #[test]
    fn an_end_may_have_a_start_or_hold_shortcuts_combination() {
        let values = with_start_and_hold();
        let (start, _) = start_keys();
        for keys in [start.as_str(), "f13"] {
            assert_eq!(
                crate::start_keys::check_combination(keys, KeyList::End, &values),
                Ok(()),
                "{keys}"
            );
        }
    }

    #[test]
    fn but_not_one_that_shares_only_some_presses_with_it() {
        let mut values = with_start_and_hold();
        values.start_keys[0].shortcut = "ctrl+space".to_string();
        assert_eq!(
            crate::start_keys::check_combination("ctrl_left+space", KeyList::End, &values),
            Err("conflict:start_key:start".to_string())
        );
        values.start_keys[0].shortcut = "ctrl_right+x".to_string();
        assert!(crate::start_keys::check_combination("ctrl_left+x", KeyList::End, &values).is_ok());
    }

    #[test]
    fn another_shortcuts_combination_is_refused() {
        let values = with_start_and_hold();
        for id in ["cancel", "paste_last_transcript", "cycle_word_list"] {
            let taken = values.bindings[id].current_binding.clone();
            assert_eq!(
                crate::start_keys::check_combination(&taken, KeyList::End, &values),
                Err(format!("conflict:{id}"))
            );
        }
        assert!(
            crate::start_keys::check_combination("ctrl+shift+f9", KeyList::End, &values).is_ok()
        );
    }

    // ---- There is always a way to end ----------------------------------------

    #[test]
    fn a_layer_that_could_end_must_still_end_after_a_change() {
        let before = values_with(builtin("", false));
        let mut emptied = before.clone();
        emptied.end_keys.clear();
        assert_eq!(still_ends(&before, &emptied).unwrap_err(), "end-key-last");
        assert!(still_ends(&before, &before).is_ok());
        let mut moved = before.clone();
        moved.end_keys[0].shortcut = "f14".to_string();
        assert!(
            still_ends(&before, &moved).is_ok(),
            "its own combinations end it"
        );
        // A layer that couldn't end before isn't blocked by it.
        assert!(still_ends(&emptied, &emptied).is_ok());
    }

    // ---- Migration ---------------------------------------------------------

    /// A binding of the old start shortcuts, as a store held it.
    fn legacy_binding(id: &str, keys: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id, "name": "", "description": "",
            "default_binding": keys, "current_binding": keys
        })
    }

    /// A store from 0.2.5: the Append text and Send Context per profile.
    fn store_0_2_5() -> serde_json::Value {
        let mut stored = serde_json::to_value(get_default_settings()).unwrap();
        let map = stored.as_object_mut().unwrap();
        for key in [
            "end_keys",
            "default_ends",
            "hold_ends",
            "start_keys",
            "hold_keys",
        ] {
            map.remove(key);
        }
        map.insert("append_text".into(), serde_json::json!(r"\n\n"));
        map.insert("send_context".into(), serde_json::json!(true));
        map.insert("active_profile_id".into(), serde_json::json!("work"));
        let (_, post_keys) = start_keys();
        let mut bindings = map["bindings"].clone();
        bindings[LEGACY_START_BINDINGS[0]] = legacy_binding(LEGACY_START_BINDINGS[0], "f13");
        bindings[LEGACY_START_BINDINGS[1]] = legacy_binding(LEGACY_START_BINDINGS[1], &post_keys);
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
                end(PLAIN_ID, "", "f13", r"\n\n", true),
                end(POST_PROCESS_ID, "", &post_keys, r"\n\n", true),
            ]
        );
        // Handy's "Auto" (no activation stored): it holds with both.
        assert_eq!(
            s.hold_keys,
            vec![
                end(HOLD_DIRECT_ID, "", "f13", r"\n\n", true),
                end(HOLD_POST_ID, "", &post_keys, r"\n\n", true),
            ]
        );
        assert_eq!(s.start_keys[0].shortcut, "f13");
        // The profiles used the flat shortcuts (f13).
        assert_eq!(
            s.profiles[0].values.end_keys,
            vec![
                end(PLAIN_ID, "", "f13", r"\s", false),
                end(POST_PROCESS_ID, "", &post_keys, r"\s", false),
            ]
        );
        // Their start and hold shortcuts follow the flat ones.
        for profile in &s.profiles {
            assert_eq!(profile.values.hold_keys, s.hold_keys, "{}", profile.id);
            assert_eq!(profile.values.start_keys, s.start_keys, "{}", profile.id);
        }
    }

    #[test]
    fn a_profile_without_shortcuts_ends_with_those_it_used() {
        let stored = store_0_2_5();
        let mut s = read(&stored);
        migrate_on(&mut s, &stored, KeyPlatform::Macos);
        for profile in &s.profiles {
            assert_eq!(profile.values.end_keys[0].shortcut, "f13", "{}", profile.id);
            assert!(!profile
                .values
                .bindings
                .contains_key(LEGACY_START_BINDINGS[0]));
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
        assert_eq!(s.hold_keys[0].append_text, r"\s");
    }

    /// Profile values from Dev (2026-11-19 to 2026-11-22): end shortcuts
    /// without a combination of their own, ended by their start shortcut as
    /// its default end, a "send" with ⌃↩ and a "spare" that ended nothing.
    fn dev_values() -> serde_json::Value {
        let mut values = serde_json::to_value(ProfileValues::default()).unwrap();
        let map = values.as_object_mut().unwrap();
        map.remove("start_keys");
        map.remove("hold_keys");
        let old = vec![
            end(PLAIN_ID, "", "", r"\n", true),
            end(POST_PROCESS_ID, "", "", r"\s", false),
            end("send", "Senden", "ctrl+return", "{Enter}", false),
            end("spare", "Reserve", "", "", false),
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
        if let serde_json::Value::Object(values) = dev_values() {
            map.extend(values);
        }
        let map = stored.as_object_mut().unwrap();
        map.remove("start_keys");
        map.remove("hold_keys");
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
                end(PLAIN_ID, "", &plain_keys, r"\n", true),
                end(POST_PROCESS_ID, "", &post_keys, r"\s", false),
                end("send", "Senden", "ctrl+return", "{Enter}", false),
            ],
            "an end without a combination that ended nothing goes"
        );
        assert_eq!(
            s.hold_keys,
            vec![
                end(HOLD_DIRECT_ID, "", &plain_keys, r"\n", true),
                end(HOLD_POST_ID, "", &post_keys, r"\s", false),
            ]
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
        assert_eq!(
            s.hold_keys[0],
            end(HOLD_DIRECT_ID, "", &plain_keys, "", false)
        );
    }

    #[test]
    fn from_dev_a_profiles_entry_that_now_differs_from_the_defaults_is_its_own() {
        let mut stored = store_dev();
        let defaults = dev_values();
        let mut profile_values = dev_values();
        profile_values["bindings"][LEGACY_START_BINDINGS[0]] =
            legacy_binding(LEGACY_START_BINDINGS[0], "f13");
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
            assert_eq!(again.hold_keys, s.hold_keys);
            assert_eq!(again.start_keys, s.start_keys);
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
        s.hold_keys[1].append_text = "{Win+Enter}".to_string();
        let stored = serde_json::to_value(&s).unwrap();
        let mut s = read(&stored);
        assert!(migrate_on(&mut s, &stored, KeyPlatform::Macos));
        assert_eq!(s.end_keys[1].append_text, "{Cmd+Enter}");
        assert_eq!(s.hold_keys[1].append_text, "{Cmd+Enter}");
    }

    // ---- Import --------------------------------------------------------------

    #[test]
    fn an_import_from_before_end_shortcuts_gets_them_from_its_fields() {
        let file = "version: 1\nprofile:\n  id: work\n  name: Arbeit\n  values:\n    selected_language: en\n    append_text: '\\n\\n'\n    send_context: true\n";
        let mut values = ProfileValues::default();
        prepare_import_on(file, &mut values, KeyPlatform::Macos);
        assert_eq!(values.end_keys, builtin(r"\n\n", true));
        assert!(
            values.bindings.is_empty(),
            "takes the importing installation's shortcuts"
        );
    }

    #[test]
    fn an_import_from_dev_gives_its_ends_the_start_combinations() {
        let dev = values_with(vec![
            end(PLAIN_ID, "", "", r"\s", false),
            end(POST_PROCESS_ID, "", "", "{Enter}", true),
        ]);
        let mut file_values = serde_json::to_value(&dev).unwrap();
        let map = file_values.as_object_mut().unwrap();
        map.remove("start_keys");
        map.remove("hold_keys");
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
    }

    #[test]
    fn an_import_with_end_shortcuts_keeps_them() {
        let values = ProfileValues {
            end_keys: vec![end("send", "Send", "ctrl+enter", "{Win+Enter}", true)],
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
        assert_eq!(imported.start_keys, values.start_keys);
        assert_eq!(imported.hold_keys, values.hold_keys);
    }

    // ---- Variant b (user, 2026-11-26): no "Post-process" box ------------------

    /// An end without the box: what an end of variant b is.
    fn plain(id: &str, name: &str, shortcut: &str, append: &str, ctx: bool) -> EndKey {
        end(id, name, shortcut, append, ctx)
    }

    /// The six ends of macOS out of the box, named in German.
    fn six_in_german() -> Vec<EndKey> {
        [
            ("direct_space", "Leerzeichen", "ctrl+space", " "),
            (
                "direct_no_space",
                "Ohne Leerzeichen",
                "ctrl+shift+space",
                "",
            ),
            ("direct_paragraph", "Absatz", "return", r"\n\n"),
            ("direct_line", "Zeile", "shift+return", r"\n"),
            ("direct_enter", "Enter senden", "ctrl+return", "{Enter}"),
            (
                "direct_shift_enter",
                "Shift-Enter senden",
                "ctrl+shift+return",
                "{Shift+Enter}",
            ),
        ]
        .into_iter()
        .map(|(id, name, shortcut, append)| plain(id, name, shortcut, append, true))
        .collect()
    }

    #[test]
    fn on_macos_six_ends_named_only_by_what_they_do_all_with_context() {
        assert_eq!(macos_ends_in("de"), six_in_german());
        assert_eq!(
            names_of(&macos_ends_in("en")),
            [
                "Space",
                "No space",
                "Paragraph",
                "Line",
                "Send Enter",
                "Send Shift+Enter"
            ]
        );
    }

    #[test]
    fn every_language_names_the_six_apart() {
        for locale in crate::tray_i18n::TRANSLATIONS.keys() {
            let ends = macos_ends_in(locale);
            assert_eq!(ends.len(), 6, "{locale}");
            let names = names_of(&ends);
            assert!(
                names
                    .iter()
                    .all(|name| !name.trim().is_empty() && !name.contains("{{")),
                "{locale}: {names:?}"
            );
            let apart: std::collections::HashSet<_> = names.iter().collect();
            assert_eq!(apart.len(), 6, "{locale}: {names:?}");
        }
    }

    #[test]
    fn on_windows_and_linux_the_two_ends_stay() {
        for platform in [KeyPlatform::Windows, KeyPlatform::Linux] {
            let (first, second) = crate::start_keys::factory_combinations_on(platform);
            assert_eq!(
                factory_end_keys_on(platform, &FactoryEndNames::for_locale("de")),
                vec![
                    plain(PLAIN_ID, "", first, "", false),
                    plain(POST_PROCESS_ID, "", second, "", false),
                ],
                "{platform:?}"
            );
        }
    }

    // ---- How it ends after an instruction key ----------------------------------

    fn with_instruction_key() -> ProfileValues {
        let mut values = values_with(builtin("", false));
        values.instruction_keys = vec![plain("tab", "", "tab", " ", true)];
        values
    }

    #[test]
    fn a_held_instruction_key_ends_with_its_values() {
        let values = with_instruction_key();
        assert_eq!(
            resolve(
                &values,
                &EndChoice::Instruction("tab".to_string()),
                "start_key:start"
            ),
            plain("instruction:tab", "", "tab", " ", true)
        );
        assert_eq!(
            find_end(&values, "instruction:tab"),
            Some(plain("instruction:tab", "", "tab", " ", true)),
            "a redo finds it by its History id"
        );
    }

    #[test]
    fn an_end_after_an_instruction_key_is_the_inner_end() {
        let values = with_instruction_key();
        let inner = EndChoice::Key(POST_PROCESS_ID.to_string());
        assert_eq!(
            resolve(
                &values,
                &EndChoice::AfterInstructionKey(Box::new(inner.clone())),
                "start_key:start"
            ),
            resolve(&values, &inner, "start_key:start")
        );
    }

    #[test]
    fn an_end_knows_whether_an_instruction_key_came_before_it() {
        let key = EndChoice::Key("x".to_string());
        assert!(!key.after_instruction_key());
        assert!(!EndChoice::Start("s".to_string()).after_instruction_key());
        assert!(!EndChoice::Held("h".to_string()).after_instruction_key());
        assert!(EndChoice::Instruction("tab".to_string()).after_instruction_key());
        let after = key.clone().with_instruction_key();
        assert_eq!(after, EndChoice::AfterInstructionKey(Box::new(key)));
        assert!(after.after_instruction_key());
        assert_eq!(after.clone().with_instruction_key(), after, "not twice");
        let held = EndChoice::Instruction("tab".to_string());
        assert_eq!(held.clone().with_instruction_key(), held);
    }

    #[test]
    fn an_end_by_holding_from_before_hold_shortcuts_is_the_hold_key_in_its_place() {
        let mut values = values_with(builtin("", false));
        values.hold_keys = vec![
            plain("first", "", "f13", "a", false),
            plain("second", "", "f14", "b", false),
        ];
        assert_eq!(
            find_end(&values, "held:transcribe").map(|end| end.id),
            Some("hold:first".to_string())
        );
        assert_eq!(
            find_end(&values, "held:transcribe_with_post_process").map(|end| end.id),
            Some("hold:second".to_string())
        );
    }

    // ---- The step to variant b ------------------------------------------------

    /// A current store whose ends and hold shortcuts are the factory ones of
    /// before variant b.
    fn store_before_variant_b() -> serde_json::Value {
        let mut stored = serde_json::to_value(get_default_settings()).unwrap();
        stored["end_keys"] = twelve_stored();
        stored["hold_keys"] = r_holds_stored();
        stored["app_language"] = serde_json::json!("de");
        stored
    }

    #[test]
    fn the_twelve_factory_ends_become_the_six_named_by_what_they_do() {
        let stored = store_before_variant_b();
        let mut s = read(&stored);
        assert!(migrate_on(&mut s, &stored, KeyPlatform::Macos));
        assert_eq!(s.end_keys, six_in_german());
    }

    /// The names don't count, as they were written in the language of the
    /// first start; the six take the new ones (migration may stay cheap).
    #[test]
    fn a_factory_list_with_names_of_its_own_takes_the_new_names() {
        let mut stored = store_before_variant_b();
        stored["end_keys"][0]["name"] = serde_json::json!("Mein Leerzeichen");
        let mut s = read(&stored);
        migrate_on(&mut s, &stored, KeyPlatform::Macos);
        assert_eq!(s.end_keys, six_in_german());
    }

    #[test]
    fn a_list_of_ones_own_keeps_its_ends_without_the_box() {
        let mut stored = store_before_variant_b();
        stored["end_keys"][3]["append_text"] = serde_json::json!(r"\n\n\n");
        let mut s = read(&stored);
        migrate_on(&mut s, &stored, KeyPlatform::Macos);
        assert_eq!(s.end_keys.len(), 12);
        assert_eq!(s.end_keys[3].append_text, r"\n\n\n");
        assert_eq!(s.end_keys[6].id, "post_space");
    }

    #[test]
    fn the_factory_hold_shortcuts_of_before_become_the_new_ones() {
        let stored = store_before_variant_b();
        let mut s = read(&stored);
        migrate_on(&mut s, &stored, KeyPlatform::Macos);
        assert_eq!(
            s.hold_keys,
            crate::start_keys::factory_hold_keys_on(KeyPlatform::Macos)
        );
        let mut own = store_before_variant_b();
        own["hold_keys"][1]["append_text"] = serde_json::json!(r"\n");
        let mut s = read(&own);
        migrate_on(&mut s, &own, KeyPlatform::Macos);
        assert_eq!(
            s.hold_keys
                .iter()
                .map(|hold| (hold.id.as_str(), hold.append_text.as_str()))
                .collect::<Vec<_>>(),
            vec![("hold_direct", " "), ("hold_post", r"\n")],
            "a list of one's own stays"
        );
    }

    #[test]
    fn every_layer_takes_the_step_and_a_second_read_writes_nothing() {
        let mut stored = store_before_variant_b();
        let mut defaults = serde_json::to_value(ProfileValues::default()).unwrap();
        defaults["end_keys"] = twelve_stored();
        defaults["hold_keys"] = r_holds_stored();
        let mut work = defaults.clone();
        work["selected_language"] = serde_json::json!("en");
        stored["default_values"] = defaults;
        stored["active_profile_id"] = serde_json::json!("default");
        stored["profiles"] = serde_json::json!([{
            "id": "work", "name": "Arbeit", "apps": [], "values": work,
            "own": ["selected_language"], "own_entries": {}
        }]);
        let mut s = read(&stored);
        assert!(migrate_on(&mut s, &stored, KeyPlatform::Macos));
        let defaults = s.default_values.clone().unwrap();
        assert_eq!(defaults.end_keys, six_in_german());
        assert_eq!(s.profiles[0].values.end_keys, six_in_german());
        assert_eq!(
            s.profiles[0].values.hold_keys,
            crate::start_keys::factory_hold_keys_on(KeyPlatform::Macos)
        );
        let written = serde_json::to_value(&s).unwrap();
        let mut again = read(&written);
        assert!(!migrate_on(&mut again, &written, KeyPlatform::Macos));
        assert_eq!(again.end_keys, s.end_keys);
        assert_eq!(again.profiles, s.profiles);
    }
}
