//! DanyWhere: instruction shortcuts (user, 2026-11-26; sidecar notes
//! `2026-11-26_1720_nachbearbeitung-gesprochene-anweisung` and
//! `2026-11-26_1841_nachbearbeitung-nur-ueber-anweisungstaste`).
//!
//! The fourth list of the same form as the hold and end shortcuts
//! (`ProfileValues::instruction_keys`, an [`EndKey`] each: id, name,
//! combination, "Append", "Send Context"). During a recording an
//! instruction shortcut switches from the text to a spoken instruction for
//! post-processing (`dictation_flow::instruction`, the coordinator's stage
//! "Recording: instruction"): tapped, an end shortcut sends the dictation
//! off with its own values; held, its release sends it off with the
//! instruction shortcut's values (in History `instruction:<id>`). A
//! dictation post-processes when an instruction shortcut was pressed, unless
//! its profile never post-processes (`settings::PostProcessMode`).
//!
//! Instruction shortcuts are registered only during a recording, like the
//! end shortcuts, and only while the profile that applies now may
//! post-process, set up or not ([`taken_now`]; a press
//! cuts by the same profile, [`is_taken_now`]); never on Linux,
//! where shortcuts registered only during a recording are unstable
//! (`shortcut::profile_sync`). Each is
//! also registered with the modifiers of each hold shortcut, which are
//! still down when it is pressed during a held recording
//! ([`bindings_while_holding_on`]). One
//! with exactly a start or hold shortcut's combination is not registered on
//! its own: that key's press during a recording is the instruction
//! shortcut's (`start_keys::Roles::instruction`). That is how Linux has
//! them (user, 2026-11-28): its factory instruction shortcut is the second
//! factory combination, a hold shortcut registered all along, and one of
//! one's own needs a start or hold shortcut's combination there. The same
//! combination as an end shortcut is a conflict (both are taken only during
//! a recording); an empty list is fine.

use handy_keys::{Hotkey, Modifiers};
use tauri::{AppHandle, Emitter};

use crate::append_text::KeyPlatform;
use crate::end_keys::EndKey;
use crate::profiles::ProfileValues;
use crate::settings::{AppSettings, ShortcutBinding};

/// The prefix of an instruction shortcut's binding id while it is registered.
pub const BINDING_PREFIX: &str = "instruction_key:";
/// The prefix of the id an end by holding an instruction shortcut takes in
/// History (`instruction:<id>`).
pub const END_PREFIX: &str = "instruction:";
/// A new installation's instruction shortcut, ⇥.
pub const FACTORY_ID: &str = "instruction_tab";
/// A new installation's instruction shortcut on Linux, the second factory
/// combination (`start_keys::factory_combinations_on`).
pub const LINUX_FACTORY_ID: &str = "instruction_second";

/// A new installation's instruction shortcuts on `platform` (recommendation
/// 2 i of the sidecar note `2026-11-26_1841`): ⇥ with "Send Context" on
/// (recommendation 3 i), appending a space on macOS as its hold shortcuts
/// do, nothing on Windows. Linux (user, 2026-11-28) can't take ⇥ only
/// during a recording, so its instruction shortcut is the second factory
/// combination, Ctrl+Alt+Shift+Space, which holds without a recording;
/// without "Append" and "Send Context", as Linux' hold and end shortcuts.
pub fn factory_instruction_keys_on(platform: KeyPlatform) -> Vec<EndKey> {
    let append_text = match platform {
        KeyPlatform::Linux => {
            let (_, second) = crate::start_keys::factory_combinations_on(platform);
            return vec![EndKey {
                id: LINUX_FACTORY_ID.to_string(),
                name: String::new(),
                shortcut: second.to_string(),
                append_text: String::new(),
                send_context: false,
            }];
        }
        KeyPlatform::Macos => " ",
        KeyPlatform::Windows => "",
    };
    vec![EndKey {
        id: FACTORY_ID.to_string(),
        name: String::new(),
        shortcut: "tab".to_string(),
        append_text: append_text.to_string(),
        send_context: true,
    }]
}

/// A new installation's instruction shortcuts on this system.
pub fn default_instruction_keys() -> Vec<EndKey> {
    factory_instruction_keys_on(KeyPlatform::current())
}

/// The binding id instruction shortcut `id` is registered under.
pub fn binding_id(id: &str) -> String {
    format!("{BINDING_PREFIX}{id}")
}

/// What follows an instruction shortcut's binding id in the binding of its
/// combination with a hold shortcut's modifiers
/// (`instruction_key:<id>#held:<modifiers>`, [`bindings_while_holding_on`]).
/// Ids never hold it: they are `instruction_<number>` or [`FACTORY_ID`].
const HELD_MARKER: &str = "#held:";

/// The instruction shortcut a registered binding id stands for: its own
/// binding (`instruction_key:<id>`) or one with a hold shortcut's modifiers
/// (`instruction_key:<id>#held:<modifiers>`).
pub fn instruction_id_of(binding_id: &str) -> Option<&str> {
    let id = binding_id.strip_prefix(BINDING_PREFIX)?;
    Some(id.split_once(HELD_MARKER).map_or(id, |(id, _)| id))
}

/// Whether a registered binding id is an instruction shortcut's with a hold
/// shortcut's modifiers ([`bindings_while_holding_on`]).
pub fn is_while_holding(binding_id: &str) -> bool {
    binding_id.starts_with(BINDING_PREFIX) && binding_id.contains(HELD_MARKER)
}

/// Whether the instruction shortcuts of `settings_now` (the settings with
/// the values of the profile that applies now) are taken during a
/// recording: the profile may post-process (not "never"). Also on Linux
/// (user, 2026-11-28), there only through a start or hold shortcut with the
/// instruction shortcut's combination, as nothing is registered there
/// during a recording (`shortcut::profile_sync::desired_on`). Also before
/// post-processing is set up (the post-processing
/// setup, sidecar note `2026-11-26_2250_nachbearbeitung-einstieg-umsetzung`,
/// section 2): such a dictation goes out unprocessed, and its notice says
/// what is missing (`post_process_setup::post_process_ready`).
pub fn taken(settings_now: &AppSettings) -> bool {
    settings_now.post_process_mode.allows()
}

/// The instruction shortcuts taken during a recording now: those of
/// `settings_now` (the settings with the values of the profile that applies
/// now, `app_profiles::settings_now`) while [`taken`], else none. The
/// registration (`shortcut::profile_sync::desired_on`) and the cut
/// ([`is_taken_now`]) both ask it, so a press that reaches DanyWhere is one
/// that cuts (review finding 5, 2026-11-26), as the end shortcuts follow
/// the profile now too.
pub fn taken_now(settings_now: &AppSettings) -> &[EndKey] {
    if taken(settings_now) {
        &settings_now.instruction_keys
    } else {
        &[]
    }
}

/// Whether a press of instruction shortcut `instruction_id` cuts the
/// recording now: the profile that applies now (`settings` as stored, with
/// `frontmost_app` as `app_profiles::frontmost_if_choosing` looks it up)
/// takes it ([`taken_now`]), not the profile the dictation began with.
/// Whether the dictation then post-processes is still for the profile at
/// its end to decide.
pub fn is_taken_now(
    settings: &AppSettings,
    frontmost_app: Option<&str>,
    instruction_id: &str,
) -> bool {
    let now = crate::app_profiles::settings_now(settings, frontmost_app);
    taken_now(&now).iter().any(|key| key.id == instruction_id)
}

/// The instruction shortcuts of `instruction_keys` that have a combination,
/// as bindings to register while a recording runs.
pub fn bindings_to_register(instruction_keys: &[EndKey]) -> Vec<ShortcutBinding> {
    instruction_keys
        .iter()
        .filter(|key| !key.shortcut.trim().is_empty())
        .map(|key| ShortcutBinding {
            id: binding_id(&key.id),
            name: key.name.clone(),
            description: String::new(),
            default_binding: key.shortcut.trim().to_string(),
            current_binding: key.shortcut.trim().to_string(),
        })
        .collect()
}

/// The instruction shortcuts of `instruction_keys` as pressed while a hold
/// shortcut of `hold_keys` holds the recording (found in Dev, 2026-11-26):
/// the hold shortcut's modifiers are still down, so ⇥ comes as ⌃⌥⇥ or ⌃⇥,
/// which a binding of `tab` alone doesn't match (handy-keys and Carbon
/// match the modifiers exactly), and the ⇥ reached the application. So each
/// instruction shortcut with a combination is also registered with each set
/// of modifiers a hold shortcut adds to it, with a main key without a side
/// as the recorder stores one (`key_sides::recorded`), under
/// `instruction_key:<id>#held:<modifiers>`, which [`instruction_id_of`]
/// maps back to `<id>`: to the coordinator a press of either is the
/// instruction shortcut's. A hold shortcut that adds no modifier adds no
/// binding, and combinations that don't parse are left out.
///
/// The hold shortcut's modifiers released before ⇥ end that binding's
/// press: handy-keys releases a hotkey whose modifiers no longer match, so
/// its release comes with the hold shortcut's, just before or after it. To
/// the coordinator ⇥ came up with the hold shortcut, which is how the user
/// sees it ("hold key released: sent").
pub fn bindings_while_holding_on(
    instruction_keys: &[EndKey],
    hold_keys: &[EndKey],
    platform: KeyPlatform,
) -> Vec<ShortcutBinding> {
    let held: Vec<Modifiers> = hold_keys
        .iter()
        .filter_map(|key| key.shortcut.trim().parse::<Hotkey>().ok())
        .map(|hotkey| hotkey.modifiers)
        .filter(|modifiers| !modifiers.is_empty())
        .collect();
    let mut bindings: Vec<ShortcutBinding> = Vec::new();
    for key in instruction_keys {
        let own = key.shortcut.trim();
        let Ok(plain) = own.parse::<Hotkey>() else {
            continue;
        };
        for &modifiers in &held {
            let combined = crate::key_sides::recorded(
                Hotkey {
                    modifiers: plain.modifiers | modifiers,
                    key: plain.key,
                },
                platform,
            );
            let combination = combined.to_handy_string();
            if crate::key_sides::identical_press(&combination, own) {
                continue;
            }
            let held_modifiers = Hotkey {
                modifiers: combined.modifiers,
                key: None,
            }
            .to_handy_string();
            let id = format!("{}{HELD_MARKER}{held_modifiers}", binding_id(&key.id));
            if bindings.iter().any(|binding| binding.id == id) {
                continue;
            }
            bindings.push(ShortcutBinding {
                id,
                name: key.name.clone(),
                description: String::new(),
                default_binding: combination.clone(),
                current_binding: combination,
            });
        }
    }
    bindings
}

/// [`bindings_while_holding_on`] on this system.
pub fn bindings_while_holding(
    instruction_keys: &[EndKey],
    hold_keys: &[EndKey],
) -> Vec<ShortcutBinding> {
    bindings_while_holding_on(instruction_keys, hold_keys, KeyPlatform::current())
}

/// The end of instruction shortcut `id` in `values`, held and released:
/// its values, under its History id (`instruction:<id>`).
pub fn instruction_end(values: &ProfileValues, id: &str) -> Option<EndKey> {
    values
        .instruction_keys
        .iter()
        .find(|key| key.id == id)
        .map(|key| EndKey {
            id: format!("{END_PREFIX}{}", key.id),
            ..key.clone()
        })
}

/// Checks a list of instruction shortcuts: ids present and unique, a
/// combination for every entry (`instruction-key-needs-shortcut:<id>`),
/// none twice (`instruction-key-shortcut-twice:<combination>`,
/// `key_sides::same_press`). An empty list is fine.
pub fn validate_instruction_keys(instruction_keys: &[EndKey]) -> Result<(), String> {
    crate::start_keys::validate_list(
        instruction_keys
            .iter()
            .map(|key| (key.id.as_str(), key.shortcut.as_str())),
        "instruction",
    )
}

/// Checks the instruction shortcuts of `values` as their change command
/// does: the list, and each combination valid for `implementation` and free
/// (`start_keys::check_combination`), "Append" in this system's key names.
pub fn check_instruction_keys(
    values: &mut ProfileValues,
    implementation: crate::settings::KeyboardImplementation,
) -> Result<(), String> {
    check_instruction_keys_on(values, implementation, cfg!(target_os = "linux"))
}

/// [`check_instruction_keys`] on Linux (`linux`) or elsewhere. Linux
/// registers nothing during a recording, so there each needs exactly a
/// start or hold shortcut's combination
/// (`instruction-key-needs-start-or-hold:<id>`, user, 2026-11-28).
pub fn check_instruction_keys_on(
    values: &mut ProfileValues,
    implementation: crate::settings::KeyboardImplementation,
    linux: bool,
) -> Result<(), String> {
    validate_instruction_keys(&values.instruction_keys)?;
    if linux {
        if let Some(key) = values
            .instruction_keys
            .iter()
            .find(|key| !on_a_start_or_hold_key(values, &key.shortcut))
        {
            return Err(format!("instruction-key-needs-start-or-hold:{}", key.id));
        }
    }
    for key in values.instruction_keys.iter_mut() {
        key.append_text = crate::append_text::localize(&key.append_text, KeyPlatform::current());
    }
    for key in &values.instruction_keys {
        crate::shortcut::validate_shortcut_for_implementation(&key.shortcut, implementation)?;
        crate::start_keys::check_combination(
            &key.shortcut,
            crate::start_keys::KeyList::Instruction,
            values,
        )?;
    }
    Ok(())
}

/// Whether a start or hold shortcut of `values` has exactly combination
/// `keys` (`key_sides::identical_press`): its press during a recording is
/// then an instruction shortcut's (`start_keys::Roles::instruction`), the
/// only way Linux has them.
pub fn on_a_start_or_hold_key(values: &ProfileValues, keys: &str) -> bool {
    let keys = keys.trim();
    let starts = values.start_keys.iter().map(|key| key.shortcut.as_str());
    let holds = values.hold_keys.iter().map(|key| key.shortcut.as_str());
    starts
        .chain(holds)
        .any(|shortcut| crate::key_sides::identical_press(shortcut.trim(), keys))
}

/// The instruction shortcuts of `values` whose combination an end shortcut
/// fires too (`key_sides::same_press`), with that end: both are taken only
/// during a recording, so one press would be both.
fn conflicts(values: &ProfileValues) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for key in &values.instruction_keys {
        for end in &values.end_keys {
            let (a, b) = (key.shortcut.trim(), end.shortcut.trim());
            if !a.is_empty() && !b.is_empty() && crate::key_sides::same_press(a, b) {
                found.push((key.id.clone(), end.id.clone()));
            }
        }
    }
    found
}

/// A layer whose instruction and end shortcuts were apart before a change
/// keeps them apart (`conflict:instruction_key:<id>`); a conflict it had
/// already doesn't block it, so an older store never blocks a change.
fn still_apart(before: &ProfileValues, after: &ProfileValues) -> Result<(), String> {
    let had = conflicts(before);
    match conflicts(after)
        .into_iter()
        .find(|pair| !had.contains(pair))
    {
        Some((key, _)) => Err(format!("conflict:{BINDING_PREFIX}{key}")),
        None => Ok(()),
    }
}

/// [`still_apart`] for the defaults and every profile
/// (`profile_layers::commit`): a change of the defaults reaches the
/// profiles that follow them, next to their own entries.
pub fn check_layers(before: &AppSettings, after: &AppSettings) -> Result<(), String> {
    still_apart(
        &crate::profiles::default_values(before),
        &crate::profiles::default_values(after),
    )?;
    for profile in &after.profiles {
        if let Some(old) = before.profiles.iter().find(|old| old.id == profile.id) {
            still_apart(&old.values, &profile.values)?;
        }
    }
    Ok(())
}

/// Stores the active profile's instruction shortcuts (the whole list, as
/// the editor holds it). A running recording takes the new ones at once.
#[tauri::command]
#[specta::specta]
pub fn change_instruction_keys_setting(
    app: AppHandle,
    instruction_keys: Vec<EndKey>,
) -> Result<(), String> {
    let mut settings = crate::settings::get_settings(&app);
    let mut after = ProfileValues::from_settings(&settings);
    after.instruction_keys = instruction_keys;
    check_instruction_keys(&mut after, settings.keyboard_implementation)?;
    settings.instruction_keys = after.instruction_keys;
    crate::settings::write_settings(&app, settings);
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({ "setting": "instruction_keys" }),
    );
    crate::shortcut::profile_sync::sync_now(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{get_default_settings, PostProcessMode};
    use crate::start_keys::{check_combination, KeyList, StartKey};

    fn key(id: &str, shortcut: &str, append: &str, ctx: bool) -> EndKey {
        EndKey {
            id: id.to_string(),
            name: String::new(),
            shortcut: shortcut.to_string(),
            append_text: append.to_string(),
            send_context: ctx,
        }
    }

    /// Settings whose post-processing has a provider with a model, with
    /// `mode`.
    fn set_up(mode: PostProcessMode) -> AppSettings {
        let mut settings = get_default_settings();
        settings.post_process_mode = mode;
        settings.post_process_provider_id = "anthropic".to_string();
        settings
            .post_process_models
            .insert("anthropic".to_string(), "claude".to_string());
        settings
    }

    // ---- Out of the box ----------------------------------------------------

    #[test]
    fn on_macos_the_factory_instruction_key_is_tab_with_a_space_and_context() {
        assert_eq!(
            factory_instruction_keys_on(KeyPlatform::Macos),
            vec![key(FACTORY_ID, "tab", " ", true)]
        );
    }

    #[test]
    fn on_windows_it_is_tab_without_append_text_with_context() {
        assert_eq!(
            factory_instruction_keys_on(KeyPlatform::Windows),
            vec![key(FACTORY_ID, "tab", "", true)]
        );
    }

    /// Linux (user, 2026-11-28): the second factory combination, which is a
    /// hold shortcut there and isn't taken apart during a recording; its
    /// press then switches to the instruction. Nothing appended and no
    /// context, as Linux' hold and end shortcuts (Linux has no window
    /// titles, `window_capture`).
    #[test]
    fn on_linux_it_is_the_second_factory_combination_without_append_text_or_context() {
        let (_, second) = crate::start_keys::factory_combinations_on(KeyPlatform::Linux);
        assert_eq!(second, "ctrl+alt+shift+space");
        assert_eq!(
            factory_instruction_keys_on(KeyPlatform::Linux),
            vec![key(LINUX_FACTORY_ID, second, "", false)]
        );
        assert!(crate::start_keys::factory_hold_keys_on(KeyPlatform::Linux)
            .iter()
            .any(|hold| crate::key_sides::identical_press(&hold.shortcut, second)));
    }

    /// Every system's factory start, hold, end and instruction shortcuts get
    /// on with each other; on Linux the instruction shortcut has a hold
    /// shortcut's combination and no end's (user, 2026-11-28).
    #[test]
    fn every_systems_factory_lists_collide_with_nothing() {
        let names = crate::end_keys::FactoryEndNames::for_locale("en");
        for platform in [KeyPlatform::Macos, KeyPlatform::Windows, KeyPlatform::Linux] {
            let mut values = ProfileValues::from_settings(&get_default_settings());
            values.start_keys = crate::start_keys::factory_start_keys_on(platform);
            values.hold_keys = crate::start_keys::factory_hold_keys_on(platform);
            values.end_keys = crate::end_keys::factory_end_keys_on(platform, &names);
            values.instruction_keys = factory_instruction_keys_on(platform);
            assert!(!values.instruction_keys.is_empty(), "{platform:?}");
            for key in &values.instruction_keys {
                assert_eq!(
                    check_combination(&key.shortcut, KeyList::Instruction, &values),
                    Ok(()),
                    "{platform:?}"
                );
            }
            for end in &values.end_keys {
                assert_eq!(
                    check_combination(&end.shortcut, KeyList::End, &values),
                    Ok(()),
                    "{platform:?}: {}",
                    end.shortcut
                );
            }
            assert_eq!(
                check_instruction_keys_on(
                    &mut values,
                    crate::settings::KeyboardImplementation::Tauri,
                    platform == KeyPlatform::Linux
                ),
                Ok(()),
                "{platform:?}"
            );
        }
    }

    #[test]
    fn a_new_installation_gets_this_systems_factory_instruction_keys() {
        let settings = get_default_settings();
        assert_eq!(
            settings.instruction_keys,
            factory_instruction_keys_on(KeyPlatform::current())
        );
        assert_eq!(
            settings.post_process_mode,
            PostProcessMode::WithInstructionKey
        );
        assert!(!settings.instruction_keys.is_empty());
    }

    #[test]
    fn the_factory_instruction_keys_are_valid_ones() {
        for platform in [KeyPlatform::Macos, KeyPlatform::Windows, KeyPlatform::Linux] {
            assert_eq!(
                validate_instruction_keys(&factory_instruction_keys_on(platform)),
                Ok(())
            );
        }
        let mut values = ProfileValues::from_settings(&get_default_settings());
        assert_eq!(
            check_instruction_keys(
                &mut values,
                crate::settings::KeyboardImplementation::HandyKeys
            ),
            Ok(())
        );
    }

    // ---- Registering -------------------------------------------------------

    #[test]
    fn an_instruction_key_is_registered_under_its_own_binding_id() {
        assert_eq!(binding_id("tab"), "instruction_key:tab");
        assert_eq!(instruction_id_of("instruction_key:tab"), Some("tab"));
        assert_eq!(instruction_id_of("end_key:tab"), None);
    }

    #[test]
    fn only_instruction_keys_with_a_combination_are_registered() {
        let bindings =
            bindings_to_register(&[key("tab", "tab", "", true), key("blank", "  ", "", false)]);
        let ids: Vec<(&str, &str)> = bindings
            .iter()
            .map(|b| (b.id.as_str(), b.current_binding.as_str()))
            .collect();
        assert_eq!(ids, vec![("instruction_key:tab", "tab")]);
    }

    // ---- While a hold shortcut is held (Dev check, 2026-11-26) ---------------
    //
    // The hold shortcut's modifiers are still down when ⇥ is pressed, so the
    // press is ⌃⌥⇥ or ⌃⇥, which a binding of `tab` alone doesn't match: the
    // ⇥ reached the application in front. So each instruction shortcut is
    // also taken with each hold shortcut's modifiers, under a binding id that
    // maps back to it.

    fn hold(id: &str, shortcut: &str) -> EndKey {
        key(id, shortcut, "", false)
    }

    /// Each binding's combination, with the instruction shortcut its id
    /// maps back to.
    fn held_combinations(bindings: &[ShortcutBinding]) -> Vec<(Option<&str>, &str)> {
        bindings
            .iter()
            .map(|b| (instruction_id_of(&b.id), b.current_binding.as_str()))
            .collect()
    }

    /// Whether `found` holds a binding of instruction shortcut `id` that
    /// fires on exactly the presses of `keys`.
    fn holds_combination(found: &[(Option<&str>, &str)], id: &str, keys: &str) -> bool {
        found.iter().any(|(of, combination)| {
            *of == Some(id) && crate::key_sides::identical_press(combination, keys)
        })
    }

    #[test]
    fn an_instruction_key_taken_with_a_hold_keys_modifiers_maps_back_to_it() {
        assert_eq!(
            instruction_id_of("instruction_key:tab#held:ctrl+option"),
            Some("tab")
        );
        assert_eq!(
            instruction_id_of("instruction_key:instruction_tab#held:ctrl"),
            Some("instruction_tab")
        );
        assert_eq!(instruction_id_of("instruction_key:tab"), Some("tab"));
        assert_eq!(instruction_id_of("end_key:tab#held:ctrl"), None);
        assert!(is_while_holding("instruction_key:tab#held:ctrl"));
        assert!(!is_while_holding("instruction_key:tab"));
        assert!(!is_while_holding("end_key:tab#held:ctrl"));
    }

    #[test]
    fn while_a_hold_key_is_held_an_instruction_key_is_taken_with_its_modifiers() {
        let found = bindings_while_holding_on(
            &[key("tab", "tab", " ", true)],
            &[
                hold("space", "ctrl+space"),
                hold("no_space", "ctrl+shift+space"),
                hold("dev", "ctrl+option+d"),
            ],
            KeyPlatform::Macos,
        );
        let found = held_combinations(&found);
        assert_eq!(found.len(), 3, "{found:?}");
        for keys in ["ctrl+tab", "ctrl+shift+tab", "ctrl+option+tab"] {
            assert!(holds_combination(&found, "tab", keys), "{keys}: {found:?}");
        }
    }

    #[test]
    fn a_hold_key_adds_only_modifiers_the_instruction_key_lacks_and_each_set_once() {
        // F13 holds no modifier down; ⌃Space and ⌃D add nothing to ⌃⇥; ⌃⇧Space
        // and ⌃⇧D add ⇧, once.
        let found = bindings_while_holding_on(
            &[key("ctab", "ctrl+tab", "", false)],
            &[
                hold("f", "f13"),
                hold("a", "ctrl+space"),
                hold("b", "ctrl+d"),
                hold("c", "ctrl+shift+space"),
                hold("d", "ctrl+shift+d"),
            ],
            KeyPlatform::Macos,
        );
        let found = held_combinations(&found);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            holds_combination(&found, "ctab", "ctrl+shift+tab"),
            "{found:?}"
        );
    }

    #[test]
    fn a_hold_key_of_modifiers_alone_adds_them_without_a_side_as_the_recorder_does() {
        // With a main key the recorder stores no side (`key_sides`), so the
        // left or the right ⌃⌥ held with ⇥ both count.
        let found = bindings_while_holding_on(
            &[key("tab", "tab", "", true)],
            &[hold("mods", "ctrl_left+option_left")],
            KeyPlatform::Macos,
        );
        let found = held_combinations(&found);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            holds_combination(&found, "tab", "ctrl+option+tab"),
            "{found:?}"
        );
    }

    #[test]
    fn nothing_held_or_nothing_to_hold_adds_nothing() {
        let tab = [key("tab", "tab", "", true)];
        assert!(bindings_while_holding_on(&tab, &[], KeyPlatform::Macos).is_empty());
        assert!(bindings_while_holding_on(
            &[key("blank", " ", "", false)],
            &[hold("a", "ctrl+space")],
            KeyPlatform::Macos
        )
        .is_empty());
        assert!(
            bindings_while_holding_on(&tab, &[hold("x", "no such key")], KeyPlatform::Macos)
                .is_empty()
        );
    }

    /// Also on Linux (user, 2026-11-28): there only through a start or hold
    /// shortcut with the instruction key's combination, as nothing is
    /// registered there during a recording (`shortcut::profile_sync`).
    #[test]
    fn instruction_keys_are_taken_unless_never_also_on_linux() {
        assert!(taken(&set_up(PostProcessMode::WithInstructionKey)));
        assert!(taken(&set_up(PostProcessMode::Always)));
        assert!(!taken(&set_up(PostProcessMode::Never)));
    }

    /// The post-processing setup (sidecar note
    /// `2026-11-26_2250_nachbearbeitung-einstieg-umsetzung`, section 2): ⇥
    /// cuts also before post-processing is set up; the dictation's end
    /// inserts the text unprocessed and its notice offers the setup.
    #[test]
    fn instruction_keys_are_taken_before_post_processing_is_set_up() {
        let mut no_model = set_up(PostProcessMode::Always);
        no_model.post_process_models.clear();
        assert!(taken(&no_model));
        let mut no_provider = set_up(PostProcessMode::WithInstructionKey);
        no_provider.post_process_provider_id = "gone".to_string();
        assert!(taken(&no_provider));
        // A new installation: a provider without a model or key.
        assert!(taken(&get_default_settings()));
        let mut never = get_default_settings();
        never.post_process_mode = PostProcessMode::Never;
        assert!(!taken(&never));
    }

    // ---- The profile that applies now (review finding 5, 2026-11-26) ---------
    //
    // The registration (`shortcut::profile_sync`) and the cut (the
    // coordinator's executor) both go by the profile that applies now, as
    // the end shortcuts do, not by the dictation's.

    #[test]
    fn the_instruction_keys_taken_now_are_the_profiles_while_it_takes_them() {
        let tab = vec![key("tab", "tab", " ", true)];
        let with = |mode: PostProcessMode| {
            let mut settings = set_up(mode);
            settings.instruction_keys = tab.clone();
            settings
        };
        assert_eq!(
            taken_now(&with(PostProcessMode::WithInstructionKey)),
            tab.as_slice()
        );
        assert_eq!(taken_now(&with(PostProcessMode::Always)), tab.as_slice());
        assert!(taken_now(&with(PostProcessMode::Never)).is_empty());
        // Also before post-processing is set up.
        let mut no_model = with(PostProcessMode::Always);
        no_model.post_process_models.clear();
        assert_eq!(taken_now(&no_model), tab.as_slice());
    }

    /// Settings whose active profile has mode `here`, with a profile "Mail"
    /// for Mail with mode `in_mail`, both with ⇥, choosing by application on.
    fn here_and_in_mail(here: PostProcessMode, in_mail: PostProcessMode) -> AppSettings {
        let tab = vec![key("tab", "tab", " ", true)];
        let mut settings = set_up(here);
        settings.instruction_keys = tab.clone();
        let mut mail_settings = set_up(in_mail);
        mail_settings.instruction_keys = tab;
        let mut mail = crate::profiles::Profile::new(
            "mail",
            "Mail",
            ProfileValues::from_settings(&mail_settings),
        );
        mail.apps = vec!["com.apple.mail".to_string()];
        settings.profiles.push(mail);
        settings.profile_auto_by_app = true;
        settings
    }

    #[test]
    fn a_press_cuts_in_the_app_in_front_that_takes_instruction_keys() {
        // The dictation began in an app whose profile never post-processes;
        // Mail, in front now, takes ⇥: it is registered there, so it cuts.
        let s = here_and_in_mail(PostProcessMode::Never, PostProcessMode::WithInstructionKey);
        assert!(is_taken_now(&s, Some("com.apple.mail"), "tab"));
        assert!(!is_taken_now(&s, Some("com.apple.TextEdit"), "tab"));
    }

    #[test]
    fn a_press_cuts_nothing_in_the_app_in_front_that_never_post_processes() {
        let s = here_and_in_mail(PostProcessMode::WithInstructionKey, PostProcessMode::Never);
        assert!(!is_taken_now(&s, Some("com.apple.mail"), "tab"));
        assert!(is_taken_now(&s, Some("com.apple.TextEdit"), "tab"));
    }

    #[test]
    fn a_press_of_an_instruction_key_the_profile_now_lacks_cuts_nothing() {
        let s = here_and_in_mail(
            PostProcessMode::WithInstructionKey,
            PostProcessMode::WithInstructionKey,
        );
        assert!(is_taken_now(&s, None, "tab"));
        assert!(!is_taken_now(&s, None, "instruction_1795300000001"));
    }

    // ---- Its end -------------------------------------------------------------

    #[test]
    fn a_held_instruction_key_ends_with_its_own_values_under_its_history_id() {
        let values = ProfileValues {
            instruction_keys: vec![key("tab", "tab", r"\n", true)],
            ..Default::default()
        };
        assert_eq!(
            instruction_end(&values, "tab"),
            Some(key("instruction:tab", "tab", r"\n", true))
        );
        assert_eq!(instruction_end(&values, "gone"), None);
    }

    // ---- Checking a list -----------------------------------------------------

    #[test]
    fn every_instruction_key_needs_a_combination_and_none_twice() {
        assert_eq!(validate_instruction_keys(&[]), Ok(()), "none is fine");
        assert_eq!(
            validate_instruction_keys(&[key("x", " ", "", false)]),
            Err("instruction-key-needs-shortcut:x".to_string())
        );
        assert_eq!(
            validate_instruction_keys(&[
                key("a", "ctrl+tab", "", false),
                key("b", "ctrl_left+tab", "", false),
            ]),
            Err("instruction-key-shortcut-twice:ctrl_left+tab".to_string())
        );
        assert!(validate_instruction_keys(&[
            key("a", "tab", "", false),
            key("a", "f13", "", false),
        ])
        .is_err());
        assert!(validate_instruction_keys(&[key(" ", "tab", "", false)]).is_err());
    }

    /// The factory start shortcut, a hold shortcut on F13, an end on ↩ and
    /// the instruction key ⇥.
    fn layer() -> ProfileValues {
        ProfileValues {
            start_keys: vec![StartKey {
                id: "start".to_string(),
                shortcut: "ctrl+space".to_string(),
            }],
            hold_keys: vec![key("hold", "f13", "", false)],
            end_keys: vec![key("para", "return", r"\n\n", true)],
            instruction_keys: vec![key("tab", "tab", "", true)],
            ..Default::default()
        }
    }

    #[test]
    fn an_instruction_key_on_an_end_keys_combination_is_a_conflict() {
        let values = layer();
        assert_eq!(
            check_combination("return", KeyList::Instruction, &values),
            Err("conflict:end_key:para".to_string())
        );
        assert_eq!(
            check_combination("tab", KeyList::End, &values),
            Err("conflict:instruction_key:tab".to_string())
        );
        assert_eq!(
            check_combination("ctrl_left+return", KeyList::Instruction, &values),
            Ok(()),
            "↩ alone and ⌃↩ are two combinations"
        );
    }

    #[test]
    fn but_a_start_or_hold_keys_combination_is_fine() {
        let values = layer();
        for keys in ["ctrl+space", "f13"] {
            assert_eq!(
                check_combination(keys, KeyList::Instruction, &values),
                Ok(()),
                "{keys}"
            );
        }
        assert_eq!(
            check_combination("tab", KeyList::Start, &values),
            Ok(()),
            "a start shortcut on the instruction key's combination"
        );
        assert_eq!(
            check_combination("ctrl_left+space", KeyList::Instruction, &values),
            Err("conflict:start_key:start".to_string()),
            "overlapping it only partly is not"
        );
    }

    #[test]
    fn another_shortcuts_combination_is_refused_for_an_instruction_key() {
        let values = ProfileValues::from_settings(&get_default_settings());
        let cancel = values.bindings["cancel"].current_binding.clone();
        assert_eq!(
            check_combination(&cancel, KeyList::Instruction, &values),
            Err("conflict:cancel".to_string())
        );
    }

    #[test]
    fn checking_the_list_checks_every_combination_against_the_layer() {
        let mut values = layer();
        values
            .instruction_keys
            .push(key("enter", "return", "", false));
        // Elsewhere than on Linux, where ⇥ has no start or hold shortcut's
        // combination and is refused for that first.
        assert_eq!(
            check_instruction_keys_on(
                &mut values,
                crate::settings::KeyboardImplementation::HandyKeys,
                false
            ),
            Err("conflict:end_key:para".to_string())
        );
        let mut windows = layer();
        windows.instruction_keys[0].append_text = "{Win+Enter}".to_string();
        assert_eq!(
            check_instruction_keys_on(
                &mut windows,
                crate::settings::KeyboardImplementation::HandyKeys,
                false
            ),
            Ok(())
        );
        assert_eq!(
            windows.instruction_keys[0].append_text,
            crate::append_text::localize("{Win+Enter}", KeyPlatform::current())
        );
    }

    /// Linux (user, 2026-11-28): nothing is registered there during a
    /// recording, so an instruction shortcut works only with exactly a
    /// start or hold shortcut's combination; another is refused instead of
    /// being ignored without a word.
    #[test]
    fn on_linux_an_instruction_key_needs_a_start_or_hold_keys_combination() {
        let handy = crate::settings::KeyboardImplementation::HandyKeys;
        let mut values = layer();
        assert_eq!(
            check_instruction_keys_on(&mut values, handy, true),
            Err("instruction-key-needs-start-or-hold:tab".to_string())
        );
        assert_eq!(
            check_instruction_keys_on(&mut values, handy, false),
            Ok(()),
            "elsewhere ⇥ is taken during a recording"
        );
        for keys in ["ctrl+space", "f13"] {
            let mut on_start_or_hold = layer();
            on_start_or_hold.instruction_keys[0].shortcut = keys.to_string();
            assert_eq!(
                check_instruction_keys_on(&mut on_start_or_hold, handy, true),
                Ok(()),
                "{keys}"
            );
        }
        let mut none = layer();
        none.instruction_keys.clear();
        assert_eq!(check_instruction_keys_on(&mut none, handy, true), Ok(()));
    }

    #[test]
    fn a_profile_entry_goes_through_the_same_check() {
        let mut settings = get_default_settings();
        crate::profiles::ensure_profiles(&mut settings);
        let refused = crate::profile_layers::set_value(
            &mut settings,
            &crate::profile_layers::Scope::Defaults,
            "instruction_keys",
            serde_json::json!([
                { "id": "a", "shortcut": "f14" },
                { "id": "b", "shortcut": "f14" }
            ]),
        );
        assert_eq!(
            refused,
            Err("instruction-key-shortcut-twice:f14".to_string())
        );
    }

    // ---- Every layer ---------------------------------------------------------

    #[test]
    fn a_layer_gets_no_conflict_between_its_instruction_and_end_keys_it_did_not_have() {
        let mut before = get_default_settings();
        before.end_keys = vec![key("para", "return", "", false)];
        before.instruction_keys = vec![key("tab", "tab", "", true)];
        crate::profiles::ensure_profiles(&mut before);
        let mut work = crate::profiles::profile_for_test(&before, "work", "Work", |values| {
            values.instruction_keys = vec![key("f15", "f15", "", true)];
        });
        work.own_entries.insert(
            "instruction_keys".to_string(),
            crate::profile_layers::OwnEntries {
                own: ["f15".to_string()].into(),
                off: ["tab".to_string()].into(),
            },
        );
        before.profiles.push(work);
        // The defaults get an end on F15, which the profile follows.
        let mut after = before.clone();
        if let Some(defaults) = after.default_values.as_mut() {
            defaults.end_keys.push(key("f15end", "f15", "", false));
        }
        crate::profile_layers::propagate(&mut after);
        assert_eq!(
            check_layers(&before, &after),
            Err("conflict:instruction_key:f15".to_string())
        );
        // A layer that had the conflict already isn't held to it.
        assert_eq!(check_layers(&after, &after), Ok(()));
    }
}
