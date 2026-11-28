//! DanyWhere: start and hold shortcuts of one's own choosing (user,
//! 2026-11-26; sidecar notes `2026-11-26_1713_start-und-haltetasten-frei-waehlbar`
//! and `2026-11-26_1810_tasten-und-anweisung-umsetzung`).
//!
//! Every layer (the defaults and each profile) has three independent lists
//! of key combinations:
//! - **start shortcuts** (`ProfileValues::start_keys`, a [`StartKey`] each,
//!   only a combination): a tap starts a recording that runs until an end
//!   shortcut ends it;
//! - **hold shortcuts** (`ProfileValues::hold_keys`, an [`EndKey`] each): a
//!   press starts a recording, and its release from the hold threshold on
//!   ends it with the hold shortcut's own values (what is appended, whether
//!   earlier dictations go along when it post-processes); a shorter press
//!   discards it, as a cancel would;
//! - **end shortcuts** (`end_keys`);
//! - **instruction shortcuts** (`instruction_keys`), during a recording
//!   only.
//!
//! What a combination does follows from the lists it is in ([`Roles`],
//! resolved on the coordinator's thread from the profile that applies
//! now): only start shortcuts: a tap locks the recording; only hold
//! shortcuts: holding records; both: short locks, long holds (Handy's
//! "Auto"). During a recording a press of a start or hold shortcut ends it
//! only as the end shortcut with exactly its combination.
//!
//! Start and hold shortcuts are taken always, each combination once with
//! the system: as `start_key:<id>` when a start shortcut has it, else as
//! `hold_key:<id>` ([`bindings_to_register`]). Since variant b (user,
//! 2026-11-26) no hold shortcut post-processes of its own; an instruction
//! shortcut, or the profile's mode, says it (`settings::PostProcessMode`).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Emitter};

use crate::append_text::KeyPlatform;
use crate::end_keys::{EndChoice, EndKey};
use crate::key_sides::identical_press;
use crate::profiles::ProfileValues;
use crate::settings::{AppSettings, ShortcutBinding};

/// The prefix of a start shortcut's binding id while it is registered.
pub const START_BINDING_PREFIX: &str = "start_key:";
/// The prefix of a hold shortcut's binding id while it is registered.
pub const HOLD_BINDING_PREFIX: &str = "hold_key:";
/// The prefix of the id an end by holding takes in History (`hold:<id>`).
pub const HOLD_END_PREFIX: &str = "hold:";
/// A new installation's start shortcut.
pub const FACTORY_START_ID: &str = "start";
/// The first hold shortcut an older store's start shortcuts become
/// ([`from_old`]), out of the box from 2026-11-26 until variant b.
pub const HOLD_DIRECT_ID: &str = "hold_direct";
/// The second one, which post-processed until variant b.
pub const HOLD_POST_ID: &str = "hold_post";
/// A new installation's hold shortcut that appends a space (on macOS).
pub const HOLD_SPACE_ID: &str = "hold_space";
/// A new installation's hold shortcut that appends nothing.
pub const HOLD_NO_SPACE_ID: &str = "hold_no_space";
/// The start shortcut an older store's second start shortcut becomes.
pub const SECOND_START_ID: &str = "start_second";
/// The start shortcuts up to 2026-11-26, bindings of their own: the plain
/// one and Handy's post-processing one ("Second Start Shortcut").
pub const LEGACY_START_BINDINGS: [&str; 2] = ["transcribe", "transcribe_with_post_process"];

/// One start shortcut: only a key combination; a start shortcut has no
/// values, since how a recording ends is up to the end shortcuts.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type, Default)]
#[serde(default)]
pub struct StartKey {
    /// Stable, so profiles can share the entry.
    pub id: String,
    pub shortcut: String,
}

/// The two factory combinations on `platform`: the start one and the one
/// with ⇧ (until variant b: the one that held with post-processing).
pub fn factory_combinations_on(platform: KeyPlatform) -> (&'static str, &'static str) {
    match platform {
        KeyPlatform::Linux => ("ctrl+alt+space", "ctrl+alt+shift+space"),
        KeyPlatform::Macos | KeyPlatform::Windows => ("ctrl+space", "ctrl+shift+space"),
    }
}

/// A new installation's start shortcuts on `platform` (question 6 a).
pub fn factory_start_keys_on(platform: KeyPlatform) -> Vec<StartKey> {
    let (start, _) = factory_combinations_on(platform);
    vec![StartKey {
        id: FACTORY_START_ID.to_string(),
        shortcut: start.to_string(),
    }]
}

/// A new installation's hold shortcuts on `platform` (recommendation 2 i of
/// the sidecar note `2026-11-26_1841`), both unnamed (they follow their
/// combination): on macOS ⌃Space appends a space and ⌃⇧Space nothing ("⇧
/// picks the smaller variant"), both with "Send Context" (recommendation 3
/// i); elsewhere neither appends anything nor sends context, as before.
pub fn factory_hold_keys_on(platform: KeyPlatform) -> Vec<EndKey> {
    let (first, second) = factory_combinations_on(platform);
    let macos = platform == KeyPlatform::Macos;
    let hold = |id: &str, shortcut: &str, append_text: &str| EndKey {
        id: id.to_string(),
        name: String::new(),
        shortcut: shortcut.to_string(),
        append_text: if macos { append_text } else { "" }.to_string(),
        send_context: macos,
    };
    vec![
        hold(HOLD_SPACE_ID, first, " "),
        hold(HOLD_NO_SPACE_ID, second, ""),
    ]
}

/// The hold shortcuts a new installation had on `platform` from 2026-11-26
/// until variant b, without their box "Post-process": an unchanged list of
/// them becomes the new factory one ([`replace_old_factory_holds`]).
fn old_factory_hold_keys_on(platform: KeyPlatform) -> Vec<EndKey> {
    let (direct, post) = factory_combinations_on(platform);
    let macos = platform == KeyPlatform::Macos;
    let hold = |id: &str, shortcut: &str, send_context: bool| EndKey {
        id: id.to_string(),
        name: String::new(),
        shortcut: shortcut.to_string(),
        append_text: if macos { " " } else { "" }.to_string(),
        send_context,
    };
    vec![
        hold(HOLD_DIRECT_ID, direct, false),
        hold(HOLD_POST_ID, post, macos),
    ]
}

/// A layer's hold shortcuts that are still the factory ones of before
/// variant b on `platform` become the new factory ones; a list of one's own
/// stays. Returns whether they changed.
pub fn replace_old_factory_holds(hold_keys: &mut Vec<EndKey>, platform: KeyPlatform) -> bool {
    if *hold_keys != old_factory_hold_keys_on(platform) {
        return false;
    }
    log::info!("The factory hold shortcuts take variant b's: ⇧ appends nothing");
    *hold_keys = factory_hold_keys_on(platform);
    true
}

/// A new installation's start shortcuts on this system.
pub fn default_start_keys() -> Vec<StartKey> {
    factory_start_keys_on(KeyPlatform::current())
}

/// A new installation's hold shortcuts on this system.
pub fn default_hold_keys() -> Vec<EndKey> {
    factory_hold_keys_on(KeyPlatform::current())
}

/// The binding id start shortcut `id` is registered under.
pub fn start_binding_id(id: &str) -> String {
    format!("{START_BINDING_PREFIX}{id}")
}

/// The binding id hold shortcut `id` is registered under.
pub fn hold_binding_id(id: &str) -> String {
    format!("{HOLD_BINDING_PREFIX}{id}")
}

/// The start shortcut a registered binding id stands for.
pub fn start_id_of(binding_id: &str) -> Option<&str> {
    binding_id.strip_prefix(START_BINDING_PREFIX)
}

/// The hold shortcut a registered binding id stands for.
pub fn hold_id_of(binding_id: &str) -> Option<&str> {
    binding_id.strip_prefix(HOLD_BINDING_PREFIX)
}

/// Whether `binding_id` is a start or hold shortcut's, which the
/// transcription coordinator handles.
pub fn is_dictation_binding(binding_id: &str) -> bool {
    start_id_of(binding_id).is_some() || hold_id_of(binding_id).is_some()
}

/// A start or hold shortcut as a binding to register.
fn registered(id: String, shortcut: &str) -> ShortcutBinding {
    ShortcutBinding {
        id,
        name: String::new(),
        description: String::new(),
        default_binding: shortcut.to_string(),
        current_binding: shortcut.to_string(),
    }
}

/// The start and hold shortcuts to register, each combination once: under
/// `start_key:<id>` when a start shortcut has it, else under
/// `hold_key:<id>`.
pub fn bindings_to_register(start_keys: &[StartKey], hold_keys: &[EndKey]) -> Vec<ShortcutBinding> {
    let mut wanted: Vec<ShortcutBinding> = Vec::new();
    let starts = start_keys
        .iter()
        .map(|key| (start_binding_id(&key.id), key.shortcut.as_str()));
    let holds = hold_keys
        .iter()
        .map(|hold| (hold_binding_id(&hold.id), hold.shortcut.as_str()));
    for (id, shortcut) in starts.chain(holds) {
        let shortcut = shortcut.trim();
        let taken = wanted
            .iter()
            .any(|binding| identical_press(&binding.current_binding, shortcut));
        if !shortcut.is_empty() && !taken {
            wanted.push(registered(id, shortcut));
        }
    }
    wanted
}

/// What a pressed combination is in the profile that applies now: the
/// start shortcut, the hold shortcut, the end shortcut and the instruction
/// shortcut with exactly its combination, if any. Filled in on the
/// coordinator's thread.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Roles {
    pub start: Option<String>,
    pub hold: Option<String>,
    pub end: Option<String>,
    /// The instruction shortcut with exactly its combination, while
    /// instruction shortcuts are taken (`instruction_keys::taken`).
    pub instruction: Option<String>,
}

/// The combination of the start or hold shortcut a registered binding id
/// stands for, in `values`.
fn combination_of<'a>(values: &'a ProfileValues, binding_id: &str) -> Option<&'a str> {
    if let Some(id) = start_id_of(binding_id) {
        return values
            .start_keys
            .iter()
            .find(|key| key.id == id)
            .map(|key| key.shortcut.as_str());
    }
    let id = hold_id_of(binding_id)?;
    values
        .hold_keys
        .iter()
        .find(|key| key.id == id)
        .map(|key| key.shortcut.as_str())
}

/// The [`Roles`] of the combination registered as `binding_id`, in `values`
/// (`instruction_taken`: whether instruction shortcuts are taken now,
/// `instruction_keys::taken`). None when `values` no longer has that start
/// or hold shortcut.
pub fn roles_of(values: &ProfileValues, binding_id: &str, instruction_taken: bool) -> Roles {
    match combination_of(values, binding_id)
        .map(str::trim)
        .filter(|keys| !keys.is_empty())
    {
        Some(keys) => roles_for(values, keys, instruction_taken),
        None => Roles::default(),
    }
}

/// The [`Roles`] of combination `keys` in `values`.
pub fn roles_for(values: &ProfileValues, keys: &str, instruction_taken: bool) -> Roles {
    let same = |shortcut: &str| identical_press(shortcut, keys);
    Roles {
        start: values
            .start_keys
            .iter()
            .find(|key| same(&key.shortcut))
            .map(|key| key.id.clone()),
        hold: values
            .hold_keys
            .iter()
            .find(|key| same(&key.shortcut))
            .map(|key| key.id.clone()),
        end: values
            .end_keys
            .iter()
            .find(|end| same(&end.shortcut))
            .map(|end| end.id.clone()),
        instruction: values
            .instruction_keys
            .iter()
            .find(|key| instruction_taken && same(&key.shortcut))
            .map(|key| key.id.clone()),
    }
}

/// [`roles_of`] in the profile that applies now (`app_profiles::profile_now`).
pub fn roles_now(app: &AppHandle, binding_id: &str) -> Roles {
    let settings = crate::settings::get_settings(app);
    let frontmost = crate::app_profiles::frontmost_if_choosing(&settings);
    let now = crate::app_profiles::settings_now(&settings, frontmost.as_deref());
    roles_of(
        &ProfileValues::from_settings(&now),
        binding_id,
        crate::instruction_keys::taken(&now),
    )
}

/// What remote control toggles (`--toggle-transcription` and SIGUSR2, or
/// with `post_process` `--toggle-post-process` and SIGUSR1): the binding a
/// recording it starts runs under, and how its second toggle ends it: as a
/// tap of the first start shortcut (without one: of the first hold
/// shortcut, locked). With `post_process` it ends as after an instruction
/// shortcut without an instruction (variant b, user, 2026-11-26): the
/// profile's prompts, unless it never post-processes.
pub fn remote_toggle(values: &ProfileValues, post_process: bool) -> Option<(String, EndChoice)> {
    let as_hold = |hold: &EndKey| (hold_binding_id(&hold.id), EndChoice::Held(hold.id.clone()));
    let (binding_id, end) = match values.start_keys.first() {
        Some(start) => (
            start_binding_id(&start.id),
            EndChoice::Start(start.id.clone()),
        ),
        None => values.hold_keys.first().map(as_hold)?,
    };
    Some((binding_id, after_instruction_key_if(end, post_process)))
}

/// `end`, as after an instruction shortcut when `post_process`.
fn after_instruction_key_if(end: EndChoice, post_process: bool) -> EndChoice {
    if post_process {
        end.with_instruction_key()
    } else {
        end
    }
}

/// The hold shortcut the post-processing service stands for: the first one
/// whose combination no start shortcut has, as the post-processing hold
/// shortcut was out of the box until variant b (later for NoAX).
fn post_service_hold(values: &ProfileValues) -> Option<&EndKey> {
    values.hold_keys.iter().find(|hold| {
        !values
            .start_keys
            .iter()
            .any(|start| identical_press(&start.shortcut, &hold.shortcut))
    })
}

/// The combination of the key a service stands for: the first start
/// shortcut, or with `post_process` [`post_service_hold`].
fn service_key(values: &ProfileValues, post_process: bool) -> Option<&str> {
    if post_process {
        post_service_hold(values).map(|hold| hold.shortcut.as_str())
    } else {
        values.start_keys.first().map(|key| key.shortcut.as_str())
    }
}

/// The binding one of the two "end dictation" services stands for (macOS
/// without Accessibility, `insert_service`): the first start shortcut's for
/// the plain one, [`post_service_hold`]'s with `post_process`; as
/// registered ([`bindings_to_register`]), so releasing it releases what
/// DanyWhere took.
pub fn service_binding(values: &ProfileValues, post_process: bool) -> Option<ShortcutBinding> {
    let keys = service_key(values, post_process)?;
    bindings_to_register(&values.start_keys, &values.hold_keys)
        .into_iter()
        .find(|binding| identical_press(&binding.current_binding, keys))
}

/// How a service ends a recording: as a press of the first start shortcut
/// (without one, as the first hold shortcut held), with `post_process` as
/// after an instruction shortcut without an instruction.
pub fn service_end(values: &ProfileValues, post_process: bool) -> Option<EndChoice> {
    let end = match values.start_keys.first() {
        Some(key) => EndChoice::Start(key.id.clone()),
        None => EndChoice::Held(values.hold_keys.first()?.id.clone()),
    };
    Some(after_instruction_key_if(end, post_process))
}

/// The binding a recording a service starts runs under: the first start
/// shortcut's, or with `post_process` [`post_service_hold`]'s.
pub fn service_start_binding(values: &ProfileValues, post_process: bool) -> Option<String> {
    if post_process {
        post_service_hold(values).map(|hold| hold_binding_id(&hold.id))
    } else {
        values
            .start_keys
            .first()
            .map(|key| start_binding_id(&key.id))
    }
}

/// How a recording started with `binding_id` ends "as a press of its key":
/// a start shortcut's as [`EndChoice::Start`], a hold shortcut's as
/// [`EndChoice::Held`].
pub fn end_as_press(binding_id: &str) -> Option<EndChoice> {
    if let Some(id) = start_id_of(binding_id) {
        return Some(EndChoice::Start(id.to_string()));
    }
    hold_id_of(binding_id).map(|id| EndChoice::Held(id.to_string()))
}

/// Which list a key combination is in, for [`check_combination`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyList {
    Start,
    Hold,
    End,
    Instruction,
}

impl KeyList {
    /// The prefix of its entries' binding ids, as conflicts name them.
    fn prefix(self) -> &'static str {
        match self {
            Self::Start => START_BINDING_PREFIX,
            Self::Hold => HOLD_BINDING_PREFIX,
            Self::End => crate::end_keys::BINDING_PREFIX,
            Self::Instruction => crate::instruction_keys::BINDING_PREFIX,
        }
    }
}

/// The shared check of a list: `what` is `start`, `hold` or `instruction`
/// (`instruction_keys`).
pub(crate) fn validate_list<'a>(
    entries: impl Iterator<Item = (&'a str, &'a str)>,
    what: &str,
) -> Result<(), String> {
    let mut ids = std::collections::HashSet::new();
    let mut taken: Vec<&str> = Vec::new();
    for (id, shortcut) in entries {
        if id.trim().is_empty() {
            return Err(format!("A {what} shortcut needs an id"));
        }
        if !ids.insert(id) {
            return Err(format!("The {what} shortcut id '{id}' is there twice"));
        }
        let shortcut = shortcut.trim();
        if shortcut.is_empty() {
            return Err(format!("{what}-key-needs-shortcut:{id}"));
        }
        if taken
            .iter()
            .any(|other| crate::key_sides::same_press(other, shortcut))
        {
            return Err(format!("{what}-key-shortcut-twice:{shortcut}"));
        }
        taken.push(shortcut);
    }
    Ok(())
}

/// Checks a list of start shortcuts: ids present and unique, a combination
/// for every entry (`start-key-needs-shortcut:<id>`), none twice
/// (`start-key-shortcut-twice:<combination>`, `key_sides::same_press`). An
/// empty list is fine while the layer has a hold shortcut ([`still_starts`]).
pub fn validate_start_keys(start_keys: &[StartKey]) -> Result<(), String> {
    validate_list(
        start_keys
            .iter()
            .map(|key| (key.id.as_str(), key.shortcut.as_str())),
        "start",
    )
}

/// [`validate_start_keys`] for hold shortcuts (`hold-key-…`).
pub fn validate_hold_keys(hold_keys: &[EndKey]) -> Result<(), String> {
    validate_list(
        hold_keys
            .iter()
            .map(|key| (key.id.as_str(), key.shortcut.as_str())),
        "hold",
    )
}

/// Checks `shortcut` of an entry of `list` against the rest of `values`:
/// another shortcut's combination (paste, copy, …, cancel; one press firing
/// both, `key_sides::same_press`) is `conflict:<binding id>`; a start, hold,
/// end or instruction shortcut of another list may have exactly the same
/// combination (`key_sides::identical_press`), but one that overlaps it
/// only partly (`ctrl+space` against `ctrl_left+space`) is
/// `conflict:start_key:<id>`, `conflict:hold_key:<id>`,
/// `conflict:end_key:<id>` or `conflict:instruction_key:<id>`. Instruction
/// and end shortcuts are both taken only during a recording, so between
/// them even exactly the same combination is a conflict.
pub fn check_combination(
    shortcut: &str,
    list: KeyList,
    values: &ProfileValues,
) -> Result<(), String> {
    let shortcut = shortcut.trim();
    crate::end_keys::check_against_bindings(shortcut, &values.bindings)?;
    let starts = values
        .start_keys
        .iter()
        .map(|key| (KeyList::Start, key.id.as_str(), key.shortcut.as_str()));
    let holds = values
        .hold_keys
        .iter()
        .map(|key| (KeyList::Hold, key.id.as_str(), key.shortcut.as_str()));
    let ends = values
        .end_keys
        .iter()
        .map(|end| (KeyList::End, end.id.as_str(), end.shortcut.as_str()));
    let instructions = values
        .instruction_keys
        .iter()
        .map(|key| (KeyList::Instruction, key.id.as_str(), key.shortcut.as_str()));
    let during_a_recording = |a: KeyList, b: KeyList| {
        matches!(
            (a, b),
            (KeyList::End, KeyList::Instruction) | (KeyList::Instruction, KeyList::End)
        )
    };
    for (other, id, keys) in starts.chain(holds).chain(ends).chain(instructions) {
        if other == list || !crate::key_sides::same_press(keys, shortcut) {
            continue;
        }
        if during_a_recording(list, other) || !identical_press(keys, shortcut) {
            return Err(format!("conflict:{}{id}", other.prefix()));
        }
    }
    Ok(())
}

/// Checks another shortcut's combination (`bindings`) against the start
/// and hold shortcuts of `values`, which are always taken, and its
/// instruction shortcuts, taken during a recording, where the other
/// shortcut would take their press (review finding 8, 2026-11-26):
/// `conflict:start_key:<id>`, `conflict:hold_key:<id>` or
/// `conflict:instruction_key:<id>`, also for a combination that overlaps
/// one only partly. End shortcuts aren't checked here (yet).
pub fn check_binding_against_keys(shortcut: &str, values: &ProfileValues) -> Result<(), String> {
    let shortcut = shortcut.trim();
    let starts = values
        .start_keys
        .iter()
        .map(|key| (KeyList::Start, key.id.as_str(), key.shortcut.as_str()));
    let holds = values
        .hold_keys
        .iter()
        .map(|key| (KeyList::Hold, key.id.as_str(), key.shortcut.as_str()));
    let instructions = values
        .instruction_keys
        .iter()
        .map(|key| (KeyList::Instruction, key.id.as_str(), key.shortcut.as_str()));
    for (list, id, keys) in starts.chain(holds).chain(instructions) {
        if crate::key_sides::same_press(keys, shortcut) {
            return Err(format!("conflict:{}{id}", list.prefix()));
        }
    }
    Ok(())
}

/// Checks the start shortcuts of `values` as their change command does:
/// the list, and each combination valid for `implementation` and free.
pub fn check_start_keys(
    values: &ProfileValues,
    implementation: crate::settings::KeyboardImplementation,
) -> Result<(), String> {
    validate_start_keys(&values.start_keys)?;
    for key in &values.start_keys {
        crate::shortcut::validate_shortcut_for_implementation(&key.shortcut, implementation)?;
        check_combination(&key.shortcut, KeyList::Start, values)?;
    }
    Ok(())
}

/// [`check_start_keys`] for the hold shortcuts, whose "Append" texts take
/// this system's key names.
pub fn check_hold_keys(
    values: &mut ProfileValues,
    implementation: crate::settings::KeyboardImplementation,
) -> Result<(), String> {
    validate_hold_keys(&values.hold_keys)?;
    for hold in values.hold_keys.iter_mut() {
        hold.append_text = crate::append_text::localize(&hold.append_text, KeyPlatform::current());
    }
    for hold in &values.hold_keys {
        crate::shortcut::validate_shortcut_for_implementation(&hold.shortcut, implementation)?;
        check_combination(&hold.shortcut, KeyList::Hold, values)?;
    }
    Ok(())
}

/// Whether a locked recording started with `key` can end where the end
/// shortcuts' own combinations aren't taken (Linux): an end shortcut has
/// exactly its combination.
fn can_end(values: &ProfileValues, key: &StartKey) -> bool {
    values
        .end_keys
        .iter()
        .any(|end| identical_press(&end.shortcut, &key.shortcut))
}

/// A layer that could always start a recording before a change must still
/// after it: it keeps a start or a hold shortcut (`start-key-last`). On
/// Linux (`linux`), where the end shortcuts' own combinations aren't taken,
/// every start shortcut needs an end shortcut with exactly its combination,
/// or a locked recording could never end (`start-key-cannot-end:<id>`); a
/// start shortcut that couldn't end before isn't held to it.
pub fn still_starts(
    before: &ProfileValues,
    after: &ProfileValues,
    linux: bool,
) -> Result<(), String> {
    let starts =
        |values: &ProfileValues| !values.start_keys.is_empty() || !values.hold_keys.is_empty();
    if starts(before) && !starts(after) {
        return Err("start-key-last".to_string());
    }
    if linux {
        for key in &after.start_keys {
            if can_end(after, key) {
                continue;
            }
            let held_to_it = before
                .start_keys
                .iter()
                .find(|old| old.id == key.id)
                .is_none_or(|old| can_end(before, old));
            if held_to_it {
                return Err(format!("start-key-cannot-end:{}", key.id));
            }
        }
    }
    Ok(())
}

/// [`still_starts`] for the defaults and every profile (`profile_layers::commit`).
pub fn check_layers(before: &AppSettings, after: &AppSettings) -> Result<(), String> {
    let linux = cfg!(target_os = "linux");
    still_starts(
        &crate::profiles::default_values(before),
        &crate::profiles::default_values(after),
        linux,
    )?;
    for profile in &after.profiles {
        if let Some(old) = before.profiles.iter().find(|old| old.id == profile.id) {
            still_starts(&old.values, &profile.values, linux)?;
        }
    }
    Ok(())
}

/// Start or hold shortcut `binding_id` of the flat values of `settings`
/// back to the factory one with its id, when `valid` takes that one (a
/// switch of the shortcut backend). Returns the combination it got.
pub fn reset_to_factory(
    settings: &mut AppSettings,
    binding_id: &str,
    valid: impl Fn(&str) -> bool,
) -> Option<String> {
    if let Some(id) = start_id_of(binding_id) {
        let factory = default_start_keys().into_iter().find(|key| key.id == id)?;
        let key = settings.start_keys.iter_mut().find(|key| key.id == id)?;
        valid(&factory.shortcut).then(|| {
            key.shortcut = factory.shortcut.clone();
            factory.shortcut
        })
    } else {
        let id = hold_id_of(binding_id)?;
        let factory = default_hold_keys().into_iter().find(|key| key.id == id)?;
        let key = settings.hold_keys.iter_mut().find(|key| key.id == id)?;
        valid(&factory.shortcut).then(|| {
            key.shortcut = factory.shortcut.clone();
            factory.shortcut
        })
    }
}

/// Every start and hold shortcut of the flat values of `settings` that
/// `valid` refuses, back to its factory combination where there is one it
/// takes ([`reset_to_factory`]). Returns the binding ids reset.
pub fn reset_invalid_keys(settings: &mut AppSettings, valid: impl Fn(&str) -> bool) -> Vec<String> {
    let refused: Vec<String> = settings
        .start_keys
        .iter()
        .filter(|key| !valid(&key.shortcut))
        .map(|key| start_binding_id(&key.id))
        .chain(
            settings
                .hold_keys
                .iter()
                .filter(|key| !valid(&key.shortcut))
                .map(|key| hold_binding_id(&key.id)),
        )
        .collect();
    refused
        .into_iter()
        .filter(|id| reset_to_factory(settings, id, &valid).is_some())
        .collect()
}

/// Stores the active profile's start shortcuts (the whole list, as the
/// editor holds it).
#[tauri::command]
#[specta::specta]
pub fn change_start_keys_setting(app: AppHandle, start_keys: Vec<StartKey>) -> Result<(), String> {
    let mut settings = crate::settings::get_settings(&app);
    let before = ProfileValues::from_settings(&settings);
    let mut after = before.clone();
    after.start_keys = start_keys;
    check_start_keys(&after, settings.keyboard_implementation)?;
    still_starts(&before, &after, cfg!(target_os = "linux"))?;
    settings.start_keys = after.start_keys;
    crate::settings::write_settings(&app, settings);
    announce(&app, "start_keys");
    Ok(())
}

/// Stores the active profile's hold shortcuts (the whole list).
#[tauri::command]
#[specta::specta]
pub fn change_hold_keys_setting(app: AppHandle, hold_keys: Vec<EndKey>) -> Result<(), String> {
    let mut settings = crate::settings::get_settings(&app);
    let before = ProfileValues::from_settings(&settings);
    let mut after = before.clone();
    after.hold_keys = hold_keys;
    check_hold_keys(&mut after, settings.keyboard_implementation)?;
    still_starts(&before, &after, cfg!(target_os = "linux"))?;
    settings.hold_keys = after.hold_keys;
    crate::settings::write_settings(&app, settings);
    announce(&app, "hold_keys");
    Ok(())
}

/// After a change of `setting`: tells the windows, registers what differs
/// and gives the services their shortcuts.
fn announce(app: &AppHandle, setting: &str) {
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({ "setting": setting }),
    );
    crate::shortcut::profile_sync::sync_now(app);
    crate::insert_service::sync_dictation_service_shortcuts(app);
}

// ---- The step from a store before 2026-11-26 ------------------------------

/// How a layer's start shortcuts drove a recording up to 2026-11-26
/// (`shortcut_activation`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OldActivation {
    /// "Tap".
    Toggle,
    /// "Hold".
    PushToTalk,
    /// "Auto".
    HoldOrToggle,
}

impl OldActivation {
    /// As `stored` (a layer's stored values) held it: `shortcut_activation`,
    /// else Handy's `push_to_talk` switch, else "Auto".
    pub fn stored_in(stored: Option<&serde_json::Value>) -> Self {
        let field = |name: &str| stored.and_then(|values| values.get(name));
        match field("shortcut_activation").and_then(|value| value.as_str()) {
            Some("toggle") => Self::Toggle,
            Some("push_to_talk") => Self::PushToTalk,
            Some(_) => Self::HoldOrToggle,
            None => match field("push_to_talk").and_then(|value| value.as_bool()) {
                Some(true) => Self::PushToTalk,
                Some(false) => Self::Toggle,
                None => Self::HoldOrToggle,
            },
        }
    }
}

/// A start shortcut's end when held, up to 2026-11-26 (`hold_ends`); its
/// box "Post-process" goes with variant b.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct OldHold {
    pub append_text: String,
    pub send_context: bool,
}

/// The combinations of the two start shortcuts in an older store's
/// `bindings`, else a new installation's.
pub fn legacy_combinations(bindings: &HashMap<String, ShortcutBinding>) -> (String, String) {
    let (first, second) = factory_combinations_on(KeyPlatform::current());
    let of = |id: &str, factory: &str| {
        bindings
            .get(id)
            .map(|binding| binding.current_binding.trim().to_string())
            .filter(|keys| !keys.is_empty())
            .unwrap_or_else(|| factory.to_string())
    };
    (
        of(LEGACY_START_BINDINGS[0], first),
        of(LEGACY_START_BINDINGS[1], second),
    )
}

/// The start and hold shortcuts a layer of an older store gets (the table
/// of the implementation note): its start shortcuts K1 and K2 (`bindings`)
/// with their hold ends H1 and H2 (`holds`), by `activation`:
///
/// | before | start shortcuts | hold shortcuts |
/// |---|---|---|
/// | Auto | K1, K2 | K1 with H1, K2 with H2 |
/// | Hold | none | K1 with H1, K2 with H2 |
/// | Tap | K1, K2 | none |
///
/// K2 becomes a start shortcut only where it was taken (`second_taken`:
/// post-processing was reachable, Handy's rule), so the update takes
/// nobody a combination they had.
pub fn from_old(
    bindings: &HashMap<String, ShortcutBinding>,
    activation: OldActivation,
    holds: &[OldHold; 2],
    second_taken: bool,
) -> (Vec<StartKey>, Vec<EndKey>) {
    let (first, second) = legacy_combinations(bindings);
    let has_second = !second.is_empty() && !crate::key_sides::same_press(&first, &second);
    let mut start_keys = Vec::new();
    if activation != OldActivation::PushToTalk {
        start_keys.push(StartKey {
            id: FACTORY_START_ID.to_string(),
            shortcut: first.clone(),
        });
        if has_second && second_taken {
            start_keys.push(StartKey {
                id: SECOND_START_ID.to_string(),
                shortcut: second.clone(),
            });
        }
    }
    let mut hold_keys = Vec::new();
    if activation != OldActivation::Toggle {
        let hold = |id: &str, shortcut: &str, old: &OldHold| EndKey {
            id: id.to_string(),
            name: String::new(),
            shortcut: shortcut.to_string(),
            append_text: old.append_text.clone(),
            send_context: old.send_context,
        };
        hold_keys.push(hold(HOLD_DIRECT_ID, &first, &holds[0]));
        if has_second {
            hold_keys.push(hold(HOLD_POST_ID, &second, &holds[1]));
        }
    }
    (start_keys, hold_keys)
}

/// Whether stored values (a layer's) are from before start shortcuts.
fn lacks_start_keys(stored: Option<&serde_json::Value>) -> bool {
    stored.is_none_or(|values| values.get("start_keys").is_none())
}

/// The step to start and hold shortcuts (2026-11-26) for a store from
/// before them (`stored` is its JSON), after the end shortcuts' steps
/// (`end_keys::migrate_on`), which give each layer's hold ends
/// (`flat_holds`, `default_holds`):
/// - the flat values and the defaults get theirs by [`from_old`];
/// - every profile from before them follows the defaults: what it had of
///   its own there goes (user, 2026-11-26: migration may stay cheap), with
///   a log line;
/// - the old start bindings go everywhere, also from a profile's own
///   entries, and a profile's own names of fields that are gone.
///
/// `reachable` is whether post-processing was reachable before. Returns
/// whether anything changed; a second run changes nothing.
pub fn migrate(
    settings: &mut AppSettings,
    stored: &serde_json::Value,
    reachable: bool,
    flat_holds: &[OldHold; 2],
    default_holds: Option<&[OldHold; 2]>,
) -> bool {
    let mut changed = false;
    let flat_before = lacks_start_keys(Some(stored));
    if flat_before {
        let (start_keys, hold_keys) = from_old(
            &settings.bindings,
            OldActivation::stored_in(Some(stored)),
            flat_holds,
            reachable,
        );
        settings.start_keys = start_keys;
        settings.hold_keys = hold_keys;
        log::info!("Start and hold shortcuts take over the start shortcuts and their hold ends");
        changed = true;
    }
    let stored_defaults = stored
        .get("default_values")
        .filter(|values| !values.is_null());
    if let (Some(defaults), Some(holds)) = (settings.default_values.as_mut(), default_holds) {
        if lacks_start_keys(stored_defaults) {
            let (start_keys, hold_keys) = from_old(
                &defaults.bindings,
                OldActivation::stored_in(stored_defaults),
                holds,
                reachable,
            );
            defaults.start_keys = start_keys;
            defaults.hold_keys = hold_keys;
            changed = true;
        }
    }
    // The profiles follow the defaults (without them, the flat values).
    let (start_keys, hold_keys) = match &settings.default_values {
        Some(defaults) => (defaults.start_keys.clone(), defaults.hold_keys.clone()),
        None => (settings.start_keys.clone(), settings.hold_keys.clone()),
    };
    let stored_profiles = stored
        .get("profiles")
        .and_then(|profiles| profiles.as_array());
    for profile in settings.profiles.iter_mut() {
        let stored_values = stored_profiles
            .and_then(|profiles| {
                profiles.iter().find(|stored| {
                    stored.get("id").and_then(|id| id.as_str()) == Some(profile.id.as_str())
                })
            })
            .and_then(|stored| stored.get("values"));
        if !lacks_start_keys(stored_values) {
            continue;
        }
        let legacy_entry = profile.own_entries.get("bindings").is_some_and(|entries| {
            LEGACY_START_BINDINGS
                .iter()
                .any(|id| entries.own.contains(*id) || entries.off.contains(*id))
        });
        if legacy_entry
            || profile.own.contains("shortcut_activation")
            || profile.own.contains("hold_ends")
        {
            log::info!(
                "Profile '{}' had start shortcuts of its own; it follows the defaults' start and hold shortcuts now",
                profile.id
            );
        }
        profile.values.start_keys = start_keys.clone();
        profile.values.hold_keys = hold_keys.clone();
        profile.own_entries.remove("start_keys");
        profile.own_entries.remove("hold_keys");
        changed = true;
    }
    // With defaults, the flat values are what is in use: the defaults', or
    // a profile's, which follows them.
    if flat_before && settings.default_values.is_some() {
        settings.start_keys = start_keys;
        settings.hold_keys = hold_keys;
    }
    changed |= drop_legacy(settings);
    changed
}

/// The old start bindings out of `bindings`; whether one was there.
pub fn drop_legacy_bindings(bindings: &mut HashMap<String, ShortcutBinding>) -> bool {
    let mut dropped = false;
    for id in LEGACY_START_BINDINGS {
        dropped |= bindings.remove(id).is_some();
    }
    dropped
}

/// The old start bindings out of every layer and every profile's own
/// entries, and a profile's own names of fields that are gone
/// (`shortcut_activation`, `hold_ends`). Returns whether anything changed.
fn drop_legacy(settings: &mut AppSettings) -> bool {
    let mut changed = drop_legacy_bindings(&mut settings.bindings);
    if let Some(defaults) = settings.default_values.as_mut() {
        changed |= drop_legacy_bindings(&mut defaults.bindings);
    }
    let known = crate::profiles::value_names();
    for profile in settings.profiles.iter_mut() {
        changed |= drop_legacy_bindings(&mut profile.values.bindings);
        if let Some(entries) = profile.own_entries.get_mut("bindings") {
            for id in LEGACY_START_BINDINGS {
                changed |= entries.own.remove(id);
                changed |= entries.off.remove(id);
            }
            if entries.is_empty() {
                profile.own_entries.remove("bindings");
            }
        }
        let before = profile.own.len();
        profile.own.retain(|field| known.contains(field));
        changed |= profile.own.len() != before;
    }
    changed
}

/// The start shortcut ids of `values`, for tests.
#[cfg(test)]
fn start_ids(values: &ProfileValues) -> Vec<&str> {
    values
        .start_keys
        .iter()
        .map(|key| key.id.as_str())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::get_default_settings;

    const MAC: KeyPlatform = KeyPlatform::Macos;

    fn start(id: &str, shortcut: &str) -> StartKey {
        StartKey {
            id: id.to_string(),
            shortcut: shortcut.to_string(),
        }
    }

    fn key(id: &str, shortcut: &str) -> EndKey {
        EndKey {
            id: id.to_string(),
            name: String::new(),
            shortcut: shortcut.to_string(),
            append_text: String::new(),
            send_context: false,
        }
    }

    fn values(starts: Vec<StartKey>, holds: Vec<EndKey>, ends: Vec<EndKey>) -> ProfileValues {
        ProfileValues {
            start_keys: starts,
            hold_keys: holds,
            end_keys: ends,
            ..ProfileValues::default()
        }
    }

    /// ⌃Space start, hold and end; ⌃⇧Space holds; ↩ ends.
    fn usual() -> ProfileValues {
        values(
            vec![start("s", "ctrl+space")],
            vec![key("h1", "ctrl+space"), key("h2", "ctrl+shift+space")],
            vec![key("e1", "ctrl+space"), key("e2", "return")],
        )
    }

    fn binding_ids(bindings: &[ShortcutBinding]) -> Vec<(&str, &str)> {
        bindings
            .iter()
            .map(|b| (b.id.as_str(), b.current_binding.as_str()))
            .collect()
    }

    // ---- Out of the box ----------------------------------------------------

    #[test]
    fn out_of_the_box_one_start_and_two_hold_shortcuts_on_macos_and_windows() {
        for platform in [MAC, KeyPlatform::Windows] {
            assert_eq!(
                factory_start_keys_on(platform),
                vec![start(FACTORY_START_ID, "ctrl+space")],
                "{platform:?}"
            );
            let holds = factory_hold_keys_on(platform);
            let ids: Vec<(&str, &str)> = holds
                .iter()
                .map(|h| (h.id.as_str(), h.shortcut.as_str()))
                .collect();
            assert_eq!(
                ids,
                [
                    (HOLD_SPACE_ID, "ctrl+space"),
                    (HOLD_NO_SPACE_ID, "ctrl+shift+space")
                ],
                "{platform:?}"
            );
            assert!(holds.iter().all(|h| h.name.is_empty()), "{platform:?}");
        }
    }

    #[test]
    fn on_macos_the_first_hold_shortcut_appends_a_space_the_second_nothing() {
        let holds = factory_hold_keys_on(MAC);
        assert_eq!(holds.len(), 2);
        assert_eq!(
            (holds[0].append_text.as_str(), holds[0].send_context),
            (" ", true)
        );
        assert_eq!(
            (holds[1].append_text.as_str(), holds[1].send_context),
            ("", true)
        );
        for platform in [KeyPlatform::Windows, KeyPlatform::Linux] {
            let holds = factory_hold_keys_on(platform);
            assert_eq!(holds.len(), 2, "{platform:?}");
            assert!(
                holds
                    .iter()
                    .all(|h| h.append_text.is_empty() && !h.send_context),
                "{platform:?}"
            );
        }
    }

    #[test]
    fn on_linux_the_factory_shortcuts_add_alt() {
        assert_eq!(
            factory_start_keys_on(KeyPlatform::Linux),
            vec![start(FACTORY_START_ID, "ctrl+alt+space")]
        );
        let holds = factory_hold_keys_on(KeyPlatform::Linux);
        let keys: Vec<&str> = holds.iter().map(|h| h.shortcut.as_str()).collect();
        assert_eq!(keys, ["ctrl+alt+space", "ctrl+alt+shift+space"]);
    }

    #[test]
    fn a_new_installation_has_this_systems_factory_lists_and_no_start_bindings() {
        let settings = get_default_settings();
        assert_eq!(
            settings.start_keys,
            factory_start_keys_on(KeyPlatform::current())
        );
        assert_eq!(
            settings.hold_keys,
            factory_hold_keys_on(KeyPlatform::current())
        );
        for legacy in LEGACY_START_BINDINGS {
            assert!(!settings.bindings.contains_key(legacy), "{legacy}");
        }
    }

    #[test]
    fn the_factory_lists_are_valid_and_collide_with_nothing() {
        let settings = get_default_settings();
        let values = ProfileValues::from_settings(&settings);
        assert!(!values.start_keys.is_empty() && !values.hold_keys.is_empty());
        assert_eq!(validate_start_keys(&values.start_keys), Ok(()));
        assert_eq!(validate_hold_keys(&values.hold_keys), Ok(()));
        for key in &values.start_keys {
            assert_eq!(
                check_combination(&key.shortcut, KeyList::Start, &values),
                Ok(())
            );
        }
        for key in &values.hold_keys {
            assert_eq!(
                check_combination(&key.shortcut, KeyList::Hold, &values),
                Ok(())
            );
        }
        for end in &values.end_keys {
            assert_eq!(
                check_combination(&end.shortcut, KeyList::End, &values),
                Ok(()),
                "{}",
                end.shortcut
            );
        }
    }

    // ---- Registering -------------------------------------------------------

    #[test]
    fn start_and_hold_shortcuts_have_binding_ids_of_their_own() {
        assert_eq!(start_binding_id("start"), "start_key:start");
        assert_eq!(hold_binding_id("hold_post"), "hold_key:hold_post");
        assert_eq!(start_id_of("start_key:start"), Some("start"));
        assert_eq!(hold_id_of("hold_key:hold_post"), Some("hold_post"));
        assert!(is_dictation_binding("start_key:x"));
        assert!(is_dictation_binding("hold_key:x"));
        assert!(!is_dictation_binding("cancel"));
        assert!(!is_dictation_binding("end_key:x"));
    }

    #[test]
    fn a_combination_in_both_lists_is_registered_once_as_the_start_shortcut() {
        let v = usual();
        assert_eq!(
            binding_ids(&bindings_to_register(&v.start_keys, &v.hold_keys)),
            [
                ("start_key:s", "ctrl+space"),
                ("hold_key:h2", "ctrl+shift+space")
            ]
        );
    }

    #[test]
    fn hold_shortcuts_alone_are_registered_under_their_own_ids() {
        let v = usual();
        assert_eq!(
            binding_ids(&bindings_to_register(&[], &v.hold_keys)),
            [
                ("hold_key:h1", "ctrl+space"),
                ("hold_key:h2", "ctrl+shift+space")
            ]
        );
    }

    // ---- What a press is ---------------------------------------------------

    fn roles(start: Option<&str>, hold: Option<&str>, end: Option<&str>) -> Roles {
        Roles {
            start: start.map(str::to_string),
            hold: hold.map(str::to_string),
            end: end.map(str::to_string),
            instruction: None,
        }
    }

    #[test]
    fn a_combination_has_the_roles_of_the_lists_it_is_in() {
        let v = usual();
        assert_eq!(
            roles_of(&v, "start_key:s", true),
            roles(Some("s"), Some("h1"), Some("e1"))
        );
        assert_eq!(
            roles_of(&v, "hold_key:h2", true),
            roles(None, Some("h2"), None)
        );
    }

    #[test]
    fn a_shortcut_the_profile_no_longer_has_has_no_roles() {
        let v = usual();
        assert_eq!(roles_of(&v, "start_key:gone", true), Roles::default());
        assert_eq!(roles_of(&v, "cancel", true), Roles::default());
    }

    // ---- Remote control and services --------------------------------------

    #[test]
    fn remote_control_toggles_as_the_first_start_shortcut_else_the_first_hold_shortcut() {
        let mut v = usual();
        assert_eq!(
            remote_toggle(&v, false),
            Some(("start_key:s".to_string(), EndChoice::Start("s".to_string())))
        );
        v.start_keys.clear();
        assert_eq!(
            remote_toggle(&v, false),
            Some(("hold_key:h1".to_string(), EndChoice::Held("h1".to_string())))
        );
    }

    #[test]
    fn the_services_stand_for_the_first_start_and_hold_shortcut_of_its_own() {
        let mut v = usual();
        let id = |b: Option<ShortcutBinding>| b.map(|b| (b.id, b.current_binding));
        assert_eq!(
            id(service_binding(&v, false)),
            Some(("start_key:s".to_string(), "ctrl+space".to_string()))
        );
        assert_eq!(
            id(service_binding(&v, true)),
            Some(("hold_key:h2".to_string(), "ctrl+shift+space".to_string()))
        );
        assert_eq!(service_end(&v, false), Some(EndChoice::Start("s".into())));
        assert_eq!(
            service_end(&v, true),
            Some(EndChoice::Start("s".into()).with_instruction_key())
        );
        assert_eq!(
            service_start_binding(&v, true).as_deref(),
            Some("hold_key:h2")
        );
        // Without a hold shortcut of its own the post-processing service has
        // no key.
        v.hold_keys[1].shortcut = "ctrl+space".to_string();
        v.hold_keys.remove(0);
        assert_eq!(service_binding(&v, true), None);
    }

    #[test]
    fn a_recording_ends_as_a_press_of_its_own_key() {
        assert_eq!(
            end_as_press("start_key:s"),
            Some(EndChoice::Start("s".into()))
        );
        assert_eq!(
            end_as_press("hold_key:h"),
            Some(EndChoice::Held("h".into()))
        );
        assert_eq!(end_as_press("cancel"), None);
    }

    // ---- Checking ----------------------------------------------------------

    #[test]
    fn every_start_and_hold_shortcut_needs_a_combination() {
        assert_eq!(
            validate_start_keys(&[start("a", "ctrl+space"), start("b", " ")]),
            Err("start-key-needs-shortcut:b".to_string())
        );
        assert_eq!(
            validate_hold_keys(&[key("h", "")]),
            Err("hold-key-needs-shortcut:h".to_string())
        );
    }

    #[test]
    fn a_list_takes_a_combination_once() {
        assert_eq!(
            validate_start_keys(&[start("a", "ctrl+space"), start("b", "ctrl_left+space")]),
            Err("start-key-shortcut-twice:ctrl_left+space".to_string())
        );
        assert_eq!(
            validate_hold_keys(&[key("a", "f13"), key("b", "F13")]),
            Err("hold-key-shortcut-twice:F13".to_string())
        );
        assert!(
            validate_start_keys(&[start("l", "ctrl_left+x"), start("r", "ctrl_right+x")]).is_ok()
        );
    }

    #[test]
    fn a_list_needs_ids_once() {
        assert!(validate_start_keys(&[start("", "f13")]).is_err());
        assert!(validate_start_keys(&[start("a", "f13"), start("a", "f14")]).is_err());
        assert!(validate_hold_keys(&[key("a", "f13"), key("a", "f14")]).is_err());
        assert!(
            validate_start_keys(&[]).is_ok(),
            "a hold shortcut may start"
        );
    }

    #[test]
    fn another_shortcuts_combination_is_a_conflict() {
        let v = ProfileValues::from_settings(&get_default_settings());
        for id in ["cancel", "paste_last_transcript", "cycle_word_list"] {
            let taken = v.bindings[id].current_binding.clone();
            for list in [KeyList::Start, KeyList::Hold, KeyList::End] {
                assert_eq!(
                    check_combination(&taken, list, &v),
                    Err(format!("conflict:{id}")),
                    "{list:?}"
                );
            }
        }
    }

    #[test]
    fn the_same_combination_in_another_list_is_fine_but_an_overlap_is_not() {
        let v = usual();
        for list in [KeyList::Start, KeyList::Hold, KeyList::End] {
            assert_eq!(check_combination("ctrl+space", list, &v), Ok(()));
        }
        assert_eq!(
            check_combination("ctrl_left+space", KeyList::End, &v),
            Err("conflict:start_key:s".to_string())
        );
        assert_eq!(
            check_combination("ctrl_left+shift+space", KeyList::Start, &v),
            Err("conflict:hold_key:h2".to_string())
        );
        let mut only_end = v.clone();
        only_end.start_keys.clear();
        only_end.hold_keys.clear();
        assert_eq!(
            check_combination("ctrl_left+space", KeyList::Hold, &only_end),
            Err("conflict:end_key:e1".to_string())
        );
    }

    #[test]
    fn another_shortcut_may_not_take_a_start_or_hold_shortcuts_combination() {
        let v = usual();
        assert_eq!(
            check_binding_against_keys("ctrl+space", &v),
            Err("conflict:start_key:s".to_string())
        );
        assert_eq!(
            check_binding_against_keys("ctrl+shift+space", &v),
            Err("conflict:hold_key:h2".to_string())
        );
        assert_eq!(check_binding_against_keys("return", &v), Ok(()));
    }

    #[test]
    fn another_shortcut_may_not_take_an_instruction_keys_combination() {
        // Review finding 8 (2026-11-26): "Cycle Word List" on ⇥ would
        // swallow the instruction key, or the other way round.
        let mut v = usual();
        v.instruction_keys = vec![key("tab", "tab"), key("send", "ctrl+return")];
        assert_eq!(
            check_binding_against_keys("tab", &v),
            Err("conflict:instruction_key:tab".to_string())
        );
        assert_eq!(
            check_binding_against_keys("ctrl_left+return", &v),
            Err("conflict:instruction_key:send".to_string()),
            "overlapping it only partly too"
        );
        assert_eq!(check_binding_against_keys("shift+tab", &v), Ok(()));
    }

    // ---- There is always a way to start ------------------------------------

    #[test]
    fn a_layer_keeps_a_start_or_a_hold_shortcut() {
        let before = usual();
        let mut no_start = before.clone();
        no_start.start_keys.clear();
        assert!(still_starts(&before, &no_start, false).is_ok());
        let mut nothing = no_start.clone();
        nothing.hold_keys.clear();
        assert_eq!(
            still_starts(&before, &nothing, false),
            Err("start-key-last".to_string())
        );
        // A layer that couldn't start before isn't blocked by it.
        assert!(still_starts(&nothing, &nothing, false).is_ok());
    }

    #[test]
    fn on_linux_every_start_shortcut_needs_an_end_with_its_combination() {
        let before = usual();
        assert!(still_starts(&before, &before, true).is_ok());
        let mut added = before.clone();
        added.start_keys.push(start("f", "f13"));
        assert_eq!(
            still_starts(&before, &added, true),
            Err("start-key-cannot-end:f".to_string())
        );
        assert!(still_starts(&before, &added, false).is_ok(), "elsewhere");
        let mut moved = before.clone();
        moved.end_keys[0].shortcut = "f14".to_string();
        assert_eq!(
            still_starts(&before, &moved, true),
            Err("start-key-cannot-end:s".to_string())
        );
        // One that couldn't end before isn't held to it.
        assert!(still_starts(&added, &added, true).is_ok());
    }

    #[test]
    fn the_layers_are_checked_one_by_one() {
        let before = crate::end_keys::with_builtin_ends(get_default_settings());
        let mut after = before.clone();
        let mut defaults = ProfileValues::from_settings(&after);
        defaults.start_keys.clear();
        defaults.hold_keys.clear();
        after.default_values = Some(defaults);
        let mut was = before.clone();
        was.default_values = Some(ProfileValues::from_settings(&before));
        assert_eq!(
            check_layers(&was, &after),
            Err("start-key-last".to_string())
        );
    }

    // ---- Through the layers (`profile_layers`) -----------------------------

    use crate::profile_layers::{remove_entry, set_entry, set_value, Scope};

    /// A new installation's settings with a profile "work" that follows
    /// the defaults.
    fn layered() -> AppSettings {
        let mut s = get_default_settings();
        crate::profiles::ensure_profiles(&mut s);
        let work = crate::profiles::profile_for_test(&s, "work", "Arbeit", |_| {});
        s.profiles.push(work);
        s
    }

    fn defaults_of(s: &AppSettings) -> ProfileValues {
        crate::profiles::default_values(s)
    }

    #[test]
    fn the_last_way_to_start_cannot_go_from_a_layer() {
        let mut s = layered();
        for hold in defaults_of(&s).hold_keys {
            remove_entry(&mut s, &Scope::Defaults, "hold_keys", &hold.id).expect("a start remains");
        }
        assert!(defaults_of(&s).hold_keys.is_empty());
        assert_eq!(
            remove_entry(&mut s, &Scope::Defaults, "start_keys", FACTORY_START_ID),
            Err("start-key-last".to_string())
        );
        // Switching it off in the profile, which follows the defaults.
        let work = Scope::Profile {
            id: "work".to_string(),
        };
        assert_eq!(
            remove_entry(&mut s, &work, "start_keys", FACTORY_START_ID),
            Err("start-key-last".to_string())
        );
        assert_eq!(
            set_value(
                &mut s,
                &Scope::Defaults,
                "start_keys",
                serde_json::json!([])
            ),
            Err("start-key-last".to_string())
        );
        assert_eq!(defaults_of(&s).start_keys.len(), 1);
    }

    #[test]
    fn the_layers_check_start_and_hold_shortcuts_as_their_commands_do() {
        let mut s = layered();
        let twice = serde_json::json!([
            { "id": "a", "shortcut": "f13" },
            { "id": "b", "shortcut": "f13" }
        ]);
        assert_eq!(
            set_value(&mut s, &Scope::Defaults, "start_keys", twice),
            Err("start-key-shortcut-twice:f13".to_string())
        );
        let paste = defaults_of(&s).bindings["paste_last_transcript"]
            .current_binding
            .clone();
        assert_eq!(
            set_entry(
                &mut s,
                &Scope::Defaults,
                "hold_keys",
                "x",
                serde_json::json!({ "id": "x", "shortcut": paste })
            ),
            Err("conflict:paste_last_transcript".to_string())
        );
        // Another shortcut on a start shortcut's combination.
        let mut cycle = defaults_of(&s).bindings["cycle_word_list"].clone();
        cycle.current_binding = defaults_of(&s).start_keys[0].shortcut.clone();
        assert_eq!(
            set_entry(
                &mut s,
                &Scope::Defaults,
                "bindings",
                "cycle_word_list",
                serde_json::to_value(cycle).unwrap()
            ),
            Err(format!("conflict:start_key:{FACTORY_START_ID}"))
        );
        // Accepted entries are entries of their own in a profile (not on
        // Linux, where F15 would need an end shortcut of its own).
        if cfg!(target_os = "linux") {
            return;
        }
        let work = Scope::Profile {
            id: "work".to_string(),
        };
        set_entry(
            &mut s,
            &work,
            "start_keys",
            "mine",
            serde_json::json!({ "id": "mine", "shortcut": "f15" }),
        )
        .expect("a free combination");
        let profile = s.profiles.iter().find(|p| p.id == "work").unwrap();
        assert!(profile.own_entries["start_keys"].own.contains("mine"));
        assert!(defaults_of(&s).start_keys.iter().all(|k| k.id != "mine"));
    }

    #[test]
    fn the_start_ids_helper_lists_in_order() {
        assert_eq!(start_ids(&usual()), ["s"]);
    }

    // ---- How a recording ends ----------------------------------------------

    use crate::end_keys::{fallback_end, find_end, resolve};

    /// [`usual`] with the second hold shortcut pressing Enter and sending
    /// context.
    fn with_send() -> ProfileValues {
        let mut v = usual();
        v.hold_keys[1].append_text = "{Enter}".to_string();
        v.hold_keys[1].send_context = true;
        v
    }

    #[test]
    fn releasing_a_hold_shortcut_ends_with_its_values_under_its_history_id() {
        let v = with_send();
        let end = resolve(&v, &EndChoice::Held("h2".into()), "hold_key:h2");
        assert_eq!(end.id, "hold:h2");
        assert_eq!(end.append_text, "{Enter}");
        assert!(end.send_context);
    }

    #[test]
    fn as_a_start_shortcut_ends_with_its_end_else_its_hold_else_the_first_end() {
        let mut v = usual();
        let start = EndChoice::Start("s".into());
        assert_eq!(resolve(&v, &start, "start_key:s").id, "e1");
        v.end_keys.remove(0);
        assert_eq!(resolve(&v, &start, "start_key:s").id, "hold:h1");
        v.hold_keys.remove(0);
        assert_eq!(resolve(&v, &start, "start_key:s").id, "e2");
        let gone = EndChoice::Start("gone".into());
        assert_eq!(resolve(&usual(), &gone, "start_key:s").id, "e1");
    }

    #[test]
    fn an_end_the_profile_lacks_ends_as_a_press_of_the_recordings_key() {
        let v = usual();
        let gone = EndChoice::Key("gone".into());
        assert_eq!(resolve(&v, &gone, "hold_key:h2").id, "hold:h2");
        assert_eq!(resolve(&v, &gone, "start_key:s").id, "e1");
        let gone_hold = EndChoice::Held("gone".into());
        assert_eq!(resolve(&v, &gone_hold, "hold_key:gone").id, "e1");
    }

    #[test]
    fn an_end_is_found_by_its_history_id_also_from_before_hold_shortcuts() {
        let v = with_send();
        assert_eq!(
            find_end(&v, "hold:h2").map(|e| e.append_text),
            Some("{Enter}".into())
        );
        assert_eq!(find_end(&v, "e2").map(|e| e.id), Some("e2".into()));
        assert_eq!(
            find_end(&v, "held:transcribe").map(|e| e.id),
            Some("hold:h1".into())
        );
        assert_eq!(
            find_end(&v, "held:transcribe_with_post_process").map(|e| e.id),
            Some("hold:h2".into())
        );
        assert_eq!(find_end(&v, "held:other"), None);
        assert_eq!(find_end(&v, "hold:gone"), None);
    }

    #[test]
    fn without_a_known_end_a_text_ends_as_the_first_start_shortcut() {
        let mut v = usual();
        assert_eq!(fallback_end(&v).id, "e1");
        v.start_keys.clear();
        assert_eq!(fallback_end(&v).id, "hold:h1");
        v.hold_keys.clear();
        assert_eq!(fallback_end(&v).id, "e1");
    }

    // ---- Migration -------------------------------------------------------

    /// The combinations of the start shortcuts in [`old_store`].
    const K1: &str = "ctrl+option+d";
    const K2: &str = "ctrl+option+shift+d";

    /// The values of a layer as 0.5.0 stored them, with `activation`.
    fn old_layer(mut layer: serde_json::Value, activation: &str) -> serde_json::Value {
        let map = layer.as_object_mut().unwrap();
        map.remove("start_keys");
        map.remove("hold_keys");
        let bindings = map["bindings"].as_object_mut().unwrap();
        for (id, keys) in [
            (LEGACY_START_BINDINGS[0], K1),
            (LEGACY_START_BINDINGS[1], K2),
        ] {
            bindings.insert(
                id.to_string(),
                serde_json::json!({
                    "id": id, "name": "", "description": "",
                    "default_binding": keys, "current_binding": keys
                }),
            );
        }
        map.insert("shortcut_activation".into(), serde_json::json!(activation));
        map.insert(
            "hold_ends".into(),
            serde_json::json!({
                "transcribe": { "append_text": " ", "post_process": false, "send_context": false },
                "transcribe_with_post_process": { "append_text": "\\n", "post_process": true, "send_context": true }
            }),
        );
        layer
    }

    /// A store of 0.5.0: the flat values and the defaults with `activation`,
    /// post-processing `post_process`, and a profile "work" with its own
    /// "Toggle" and its own start shortcut.
    fn old_store(activation: &str, post_process: bool) -> serde_json::Value {
        let mut settings = get_default_settings();
        settings.post_process_mode = if post_process {
            crate::settings::PostProcessMode::WithInstructionKey
        } else {
            crate::settings::PostProcessMode::Never
        };
        let defaults = serde_json::to_value(ProfileValues::from_settings(&settings)).unwrap();
        let mut stored = old_layer(serde_json::to_value(&settings).unwrap(), activation);
        let map = stored.as_object_mut().unwrap();
        map.insert(
            "default_values".into(),
            old_layer(defaults.clone(), activation),
        );
        map.insert("active_profile_id".into(), serde_json::json!("default"));
        let mut work = old_layer(defaults, "toggle");
        work["bindings"][LEGACY_START_BINDINGS[0]]["current_binding"] = serde_json::json!("f13");
        map.insert(
            "profiles".into(),
            serde_json::json!([{
                "id": "work", "name": "Arbeit", "apps": [],
                "values": work,
                "own": ["shortcut_activation"],
                "own_entries": { "bindings": { "own": ["transcribe"], "off": [] } }
            }]),
        );
        stored
    }

    fn migrated(stored: &serde_json::Value) -> AppSettings {
        let mut s: AppSettings = serde_json::from_value(stored.clone()).expect("the store loads");
        assert!(crate::end_keys::migrate_on(&mut s, stored, MAC));
        s
    }

    fn hold(id: &str, shortcut: &str, append: &str, ctx: bool) -> EndKey {
        EndKey {
            append_text: append.to_string(),
            send_context: ctx,
            ..key(id, shortcut)
        }
    }

    fn old_holds() -> Vec<EndKey> {
        vec![
            hold(HOLD_DIRECT_ID, K1, " ", false),
            hold(HOLD_POST_ID, K2, "\\n", true),
        ]
    }

    #[test]
    fn from_auto_both_start_shortcuts_and_both_hold_ends_carry_over() {
        let s = migrated(&old_store("hold_or_toggle", true));
        let starts = vec![start(FACTORY_START_ID, K1), start(SECOND_START_ID, K2)];
        assert_eq!(s.start_keys, starts);
        assert_eq!(s.hold_keys, old_holds());
        let defaults = s.default_values.as_ref().unwrap();
        assert_eq!(defaults.start_keys, starts);
        assert_eq!(defaults.hold_keys, old_holds());
    }

    #[test]
    fn from_auto_a_second_start_shortcut_that_was_not_taken_does_not_start() {
        let s = migrated(&old_store("hold_or_toggle", false));
        assert_eq!(s.start_keys, vec![start(FACTORY_START_ID, K1)]);
        assert_eq!(s.hold_keys, old_holds());
    }

    #[test]
    fn from_hold_only_hold_shortcuts() {
        let s = migrated(&old_store("push_to_talk", true));
        assert!(s.start_keys.is_empty());
        assert_eq!(s.hold_keys, old_holds());
    }

    #[test]
    fn from_toggle_only_start_shortcuts() {
        let s = migrated(&old_store("toggle", true));
        assert_eq!(
            s.start_keys,
            vec![start(FACTORY_START_ID, K1), start(SECOND_START_ID, K2)]
        );
        assert!(s.hold_keys.is_empty());
    }

    #[test]
    fn handys_push_to_talk_switch_counts_as_hold() {
        let mut stored = old_store("push_to_talk", true);
        let map = stored.as_object_mut().unwrap();
        map.remove("shortcut_activation");
        map.insert("push_to_talk".into(), serde_json::json!(true));
        map.insert("default_values".into(), serde_json::Value::Null);
        map.insert("profiles".into(), serde_json::json!([]));
        let s = migrated(&stored);
        assert!(s.start_keys.is_empty());
        assert_eq!(s.hold_keys, old_holds());
    }

    #[test]
    fn the_old_start_bindings_go_everywhere() {
        let s = migrated(&old_store("hold_or_toggle", true));
        let all = std::iter::once(&s.bindings)
            .chain(s.default_values.iter().map(|v| &v.bindings))
            .chain(s.profiles.iter().map(|p| &p.values.bindings));
        for bindings in all {
            for legacy in LEGACY_START_BINDINGS {
                assert!(!bindings.contains_key(legacy), "{legacy}");
            }
        }
        let work = &s.profiles[0];
        assert!(
            work.own_entries
                .get("bindings")
                .is_none_or(|e| !e.own.contains("transcribe")),
            "{:?}",
            work.own_entries
        );
    }

    #[test]
    fn a_profile_with_its_own_start_values_follows_the_defaults() {
        let s = migrated(&old_store("hold_or_toggle", true));
        let work = &s.profiles[0];
        let defaults = s.default_values.as_ref().unwrap();
        assert_eq!(work.values.start_keys, defaults.start_keys);
        assert_eq!(work.values.hold_keys, defaults.hold_keys);
        assert!(!work.own.contains("shortcut_activation"));
        assert!(!work.own_entries.contains_key("start_keys"));
        assert!(!work.own_entries.contains_key("hold_keys"));
    }

    #[test]
    fn the_step_to_start_and_hold_shortcuts_runs_once() {
        for activation in ["hold_or_toggle", "push_to_talk", "toggle"] {
            let s = migrated(&old_store(activation, true));
            let written = serde_json::to_value(&s).unwrap();
            let mut again: AppSettings = serde_json::from_value(written.clone()).unwrap();
            assert!(
                !crate::end_keys::migrate_on(&mut again, &written, MAC),
                "{activation}"
            );
            assert_eq!(again.start_keys, s.start_keys);
            assert_eq!(again.hold_keys, s.hold_keys);
            assert_eq!(again.profiles, s.profiles);
        }
    }

    #[test]
    fn an_old_profile_file_imports_with_the_defaults_start_and_hold_shortcuts() {
        let mut settings = get_default_settings();
        crate::profiles::ensure_profiles(&mut settings);
        let mut file_values = old_layer(
            serde_json::to_value(ProfileValues::from_settings(&settings)).unwrap(),
            "toggle",
        );
        file_values["bindings"][LEGACY_START_BINDINGS[0]]["current_binding"] =
            serde_json::json!("f13");
        let file = serde_saphyr::to_string(&serde_json::json!({
            "version": 1,
            "profile": { "id": "w", "name": "W", "values": file_values }
        }))
        .unwrap();
        let mut values: ProfileValues =
            serde_json::from_value(file_values.clone()).expect("the file's values load");
        crate::end_keys::prepare_import_on(&file, &mut values, MAC);
        crate::profile_transfer::prepare_import_values(&file, &mut values, &settings);
        for legacy in LEGACY_START_BINDINGS {
            assert!(!values.bindings.contains_key(legacy), "{legacy}");
        }
        let defaults = crate::profiles::default_values(&settings);
        assert_eq!(values.start_keys, defaults.start_keys);
        assert_eq!(values.hold_keys, defaults.hold_keys);
    }

    // ---- Variant b (user, 2026-11-26) ----------------------------------------

    /// A hold shortcut of variant b: no box, `append` and `ctx`.
    fn hold_b(id: &str, shortcut: &str, append: &str, ctx: bool) -> EndKey {
        EndKey {
            id: id.to_string(),
            shortcut: shortcut.to_string(),
            append_text: append.to_string(),
            send_context: ctx,
            ..Default::default()
        }
    }

    #[test]
    fn out_of_the_box_the_hold_shortcuts_are_named_by_what_they_append() {
        assert_eq!(
            factory_hold_keys_on(MAC),
            vec![
                hold_b(HOLD_SPACE_ID, "ctrl+space", " ", true),
                hold_b(HOLD_NO_SPACE_ID, "ctrl+shift+space", "", true),
            ]
        );
        assert_eq!(
            factory_hold_keys_on(KeyPlatform::Windows),
            vec![
                hold_b(HOLD_SPACE_ID, "ctrl+space", "", false),
                hold_b(HOLD_NO_SPACE_ID, "ctrl+shift+space", "", false),
            ]
        );
        assert_eq!(
            factory_hold_keys_on(KeyPlatform::Linux),
            vec![
                hold_b(HOLD_SPACE_ID, "ctrl+alt+space", "", false),
                hold_b(HOLD_NO_SPACE_ID, "ctrl+alt+shift+space", "", false),
            ]
        );
    }

    /// ⌃Space start and hold, ⌃⇧Space hold, an end on ⌃Space, ⇥ instructs.
    fn variant_b() -> ProfileValues {
        ProfileValues {
            start_keys: vec![start("s", "ctrl+space")],
            hold_keys: vec![
                hold_b("h1", "ctrl+space", " ", true),
                hold_b("h2", "ctrl+shift+space", "", true),
            ],
            end_keys: vec![hold_b("e1", "ctrl+space", " ", true)],
            instruction_keys: vec![hold_b("tab", "tab", " ", true)],
            ..ProfileValues::default()
        }
    }

    #[test]
    fn remote_post_processing_toggles_as_an_instruction_key_without_an_instruction() {
        let values = variant_b();
        assert_eq!(
            remote_toggle(&values, false),
            Some(("start_key:s".to_string(), EndChoice::Start("s".to_string())))
        );
        assert_eq!(
            remote_toggle(&values, true),
            Some((
                "start_key:s".to_string(),
                EndChoice::Start("s".to_string()).with_instruction_key()
            ))
        );
        let holds_only = ProfileValues {
            start_keys: Vec::new(),
            ..variant_b()
        };
        assert_eq!(
            remote_toggle(&holds_only, true),
            Some((
                "hold_key:h1".to_string(),
                EndChoice::Held("h1".to_string()).with_instruction_key()
            ))
        );
    }

    #[test]
    fn the_post_processing_service_ends_as_an_instruction_key_without_an_instruction() {
        let values = variant_b();
        assert_eq!(
            service_end(&values, true),
            Some(EndChoice::Start("s".to_string()).with_instruction_key())
        );
        assert_eq!(
            service_end(&values, false),
            Some(EndChoice::Start("s".to_string()))
        );
        // Its key: the first hold shortcut with a combination of its own,
        // as the post-processing hold shortcut was out of the box.
        assert_eq!(
            service_start_binding(&values, true).as_deref(),
            Some("hold_key:h2")
        );
    }

    #[test]
    fn a_start_shortcuts_combination_is_an_instruction_key_while_they_are_taken() {
        let mut values = variant_b();
        values.instruction_keys[0].shortcut = "ctrl+shift+space".to_string();
        let taken = roles_of(&values, "hold_key:h2", true);
        assert_eq!(taken.instruction.as_deref(), Some("tab"));
        assert_eq!(taken.hold.as_deref(), Some("h2"));
        assert_eq!(roles_of(&values, "hold_key:h2", false).instruction, None);
        assert_eq!(roles_of(&values, "start_key:s", true).instruction, None);
    }

    /// Linux (user, 2026-11-28): a new installation's Ctrl+Alt+Shift+Space
    /// holds without a recording and instructs during one, as instruction
    /// shortcuts are taken there too (`instruction_keys::taken`).
    #[test]
    fn on_linux_the_second_factory_combination_instructs_during_a_recording() {
        let linux = KeyPlatform::Linux;
        let mut settings = crate::settings::get_default_settings();
        settings.start_keys = factory_start_keys_on(linux);
        settings.hold_keys = factory_hold_keys_on(linux);
        settings.instruction_keys = crate::instruction_keys::factory_instruction_keys_on(linux);
        let values = ProfileValues::from_settings(&settings);
        let taken = crate::instruction_keys::taken(&settings);
        let second = roles_of(&values, &hold_binding_id(HOLD_NO_SPACE_ID), taken);
        assert_eq!(
            second.instruction.as_deref(),
            Some(crate::instruction_keys::LINUX_FACTORY_ID)
        );
        assert_eq!(second.hold.as_deref(), Some(HOLD_NO_SPACE_ID));
        assert_eq!(second.start, None);
        let first = roles_of(&values, &start_binding_id(FACTORY_START_ID), taken);
        assert_eq!(first.instruction, None);
        assert_eq!(first.start.as_deref(), Some(FACTORY_START_ID));
    }
}
