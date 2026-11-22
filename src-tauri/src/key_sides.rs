//! DanyWhere: key combinations without a side (user, 2026-11-22, design
//! question 3a after ideas 1 to 3).
//!
//! handy-keys records the side of every modifier held (`ctrl_left+return`)
//! and then fires only for that side, so the right Ctrl never triggered a
//! combination recorded with the left one. Nobody chose a side on purpose:
//! the recorder takes whichever key was pressed, and the app never showed
//! it. So a combination with a main key is recorded and stored without a
//! side (`ctrl+return`, which handy-keys parses as either side). A
//! combination of modifiers only keeps its side, which is its whole point:
//! a bare `option` would fire on every Option press, e.g. typing @ or €.
//!
//! Off macOS Alt keeps its side as well: the right Alt is AltGr there, and
//! a sideless `alt+q` would catch AltGr+Q, which types @ on a German
//! keyboard.

use std::collections::HashMap;

use handy_keys::{Hotkey, Modifiers};

use crate::append_text::KeyPlatform;
use crate::end_keys::EndKey;
use crate::profiles::ProfileValues;
use crate::settings::{AppSettings, ShortcutBinding};

/// The modifier groups, each as (left, right).
const GROUPS: [(Modifiers, Modifiers); 4] = [
    (Modifiers::CTRL_LEFT, Modifiers::CTRL_RIGHT),
    (Modifiers::OPT_LEFT, Modifiers::OPT_RIGHT),
    (Modifiers::SHIFT_LEFT, Modifiers::SHIFT_RIGHT),
    (Modifiers::CMD_LEFT, Modifiers::CMD_RIGHT),
];

/// Whether modifier group `group` (both sides) loses its side on
/// `platform`: every group on macOS, every one but Alt elsewhere.
fn loses_side(group: Modifiers, platform: KeyPlatform) -> bool {
    group != Modifiers::FN && (group != Modifiers::OPT || platform == KeyPlatform::Macos)
}

/// `hotkey` with both sides of every held group that `loses` names.
fn widened(hotkey: Hotkey, loses: impl Fn(Modifiers) -> bool) -> Hotkey {
    let mut modifiers = hotkey.modifiers;
    for (left, right) in GROUPS {
        let both = left | right;
        if modifiers.intersects(both) && loses(both) {
            modifiers |= both;
        }
    }
    Hotkey {
        modifiers,
        key: hotkey.key,
    }
}

/// The combination as the recorder stores it on `platform`: with a main
/// key, either side of each modifier held (Alt off macOS excepted);
/// modifiers alone as they were pressed.
pub fn recorded(hotkey: Hotkey, platform: KeyPlatform) -> Hotkey {
    if hotkey.key.is_none() {
        return hotkey;
    }
    widened(hotkey, |group| loses_side(group, platform))
}

/// The string the recorder stores for `hotkey` on this system.
pub fn recorded_string(hotkey: Hotkey) -> String {
    recorded(hotkey, KeyPlatform::current()).to_handy_string()
}

/// `binding` without the sides it would lose on `platform`, or `None` when
/// nothing changes (no side, modifiers only, or not a combination). The
/// other names stay as they are written.
pub fn without_sides(binding: &str, platform: KeyPlatform) -> Option<String> {
    let hotkey: Hotkey = binding.trim().parse().ok()?;
    hotkey.key?;
    let mut changed = false;
    let mut parts: Vec<String> = Vec::new();
    for part in binding.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        let lower = part.to_ascii_lowercase();
        let side_off = lower
            .strip_suffix("_left")
            .or_else(|| lower.strip_suffix("_right"))
            .filter(|base| {
                base.parse::<Modifiers>()
                    .is_ok_and(|group| loses_side(group, platform))
            });
        let part = match side_off {
            Some(base) => {
                changed = true;
                base.to_string()
            }
            None => part.to_string(),
        };
        if parts.iter().any(|kept| kept.eq_ignore_ascii_case(&part)) {
            changed = true;
        } else {
            parts.push(part);
        }
    }
    changed.then(|| parts.join("+"))
}

const NO_KEY: u8 = 1;
const LEFT: u8 = 2;
const RIGHT: u8 = 4;

/// Which keys of group (`left`, `right`) a press may hold for `modifiers`
/// to fire, holding at most one: none, the left, the right, or either.
fn held(modifiers: Modifiers, left: Modifiers, right: Modifiers) -> u8 {
    match (modifiers.contains(left), modifiers.contains(right)) {
        (true, true) => LEFT | RIGHT,
        (true, false) => LEFT,
        (false, true) => RIGHT,
        (false, false) => NO_KEY,
    }
}

/// Whether every press that fires `inner` fires `outer` too.
fn covers(outer: &Hotkey, inner: &Hotkey) -> bool {
    outer.key == inner.key
        && outer.modifiers.contains(Modifiers::FN) == inner.modifiers.contains(Modifiers::FN)
        && GROUPS.iter().all(|&(left, right)| {
            held(inner.modifiers, left, right) & !held(outer.modifiers, left, right) == 0
        })
}

/// Whether one press fires both `a` and `b`, holding one key of each
/// modifier: `ctrl+return` and `ctrl_left+return` do (the left Ctrl), the
/// left and the right Ctrl with Return don't. What doesn't parse counts as
/// the same only when written the same.
pub fn same_press(a: &str, b: &str) -> bool {
    match (a.trim().parse::<Hotkey>(), b.trim().parse::<Hotkey>()) {
        (Ok(a), Ok(b)) => {
            a.key == b.key
                && a.modifiers.contains(Modifiers::FN) == b.modifiers.contains(Modifiers::FN)
                && GROUPS.iter().all(|&(left, right)| {
                    held(a.modifiers, left, right) & held(b.modifiers, left, right) != 0
                })
        }
        _ => a.trim().eq_ignore_ascii_case(b.trim()),
    }
}

/// Run on every read of the store, with this system's rule for Alt; see
/// [`migrate_on`].
pub fn migrate(settings: &mut AppSettings) -> bool {
    migrate_on(settings, KeyPlatform::current())
}

/// The shortcuts and end shortcuts of the settings, the defaults and every
/// profile without their sides ([`resolve`]). Returns whether anything
/// changed, so a store without sides is never written.
pub fn migrate_on(settings: &mut AppSettings, platform: KeyPlatform) -> bool {
    let mut changed = resolve(&mut settings.bindings, &mut settings.end_keys, platform);
    if let Some(values) = settings.default_values.as_mut() {
        changed |= resolve(&mut values.bindings, &mut values.end_keys, platform);
    }
    for profile in settings.profiles.iter_mut() {
        changed |= resolve(
            &mut profile.values.bindings,
            &mut profile.values.end_keys,
            platform,
        );
    }
    changed
}

/// For an import: the profile's shortcuts and end shortcuts without sides.
pub fn prepare_import(values: &mut ProfileValues) {
    resolve(
        &mut values.bindings,
        &mut values.end_keys,
        KeyPlatform::current(),
    );
}

/// Where a key combination is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Slot {
    Binding(String),
    End(usize),
}

/// A key combination with a main key, as stored and as it will be.
struct Item {
    slot: Slot,
    original: String,
    hotkey: Hotkey,
    target: String,
    /// The combination without any side: what it can collide with.
    group: Hotkey,
}

/// One set of values (the flat settings, the defaults or a profile): its
/// shortcuts and end shortcuts lose their sides. Combinations that would
/// then be the same (`group`) are settled first:
/// - an end shortcut that never fired alone, as every press of it fired a
///   shortcut or an earlier end shortcut too, loses its key combination (the
///   entry stays, e.g. as a default end); a shortcut can't be empty, and end
///   shortcuts are registered only while recording, so they give way;
/// - the others lose their sides if that leaves no two that one press fires
///   both; otherwise each keeps its side, as each fired on its own side.
fn resolve(
    bindings: &mut HashMap<String, ShortcutBinding>,
    end_keys: &mut [EndKey],
    platform: KeyPlatform,
) -> bool {
    let mut ids: Vec<&String> = bindings.keys().collect();
    ids.sort();
    let stored = ids
        .into_iter()
        .map(|id| {
            (
                Slot::Binding(id.clone()),
                bindings[id].current_binding.clone(),
            )
        })
        .chain(
            end_keys
                .iter()
                .enumerate()
                .map(|(i, end)| (Slot::End(i), end.shortcut.clone())),
        );
    let items: Vec<Item> = stored
        .filter_map(|(slot, keys)| {
            let original = keys.trim().to_string();
            let hotkey: Hotkey = original.parse().ok()?;
            hotkey.key?;
            let target = without_sides(&original, platform).unwrap_or_else(|| original.clone());
            Some(Item {
                slot,
                group: widened(hotkey, |_| true),
                original,
                hotkey,
                target,
            })
        })
        .collect();

    let mut changed = false;
    let mut settled = vec![false; items.len()];
    for first in 0..items.len() {
        if settled[first] {
            continue;
        }
        let group: Vec<usize> = (first..items.len())
            .filter(|&i| !settled[i] && items[i].group == items[first].group)
            .collect();
        for &i in &group {
            settled[i] = true;
        }
        if group.iter().all(|&i| items[i].target == items[i].original) {
            continue;
        }
        let gives_way = |end: usize| {
            matches!(items[end].slot, Slot::End(_))
                && group.iter().any(|&other| {
                    other != end
                        && covers(&items[other].hotkey, &items[end].hotkey)
                        && (matches!(items[other].slot, Slot::Binding(_))
                            || !covers(&items[end].hotkey, &items[other].hotkey)
                            || other < end)
                })
        };
        let lost: Vec<usize> = group.iter().copied().filter(|&i| gives_way(i)).collect();
        for &i in &lost {
            if let Slot::End(index) = items[i].slot {
                log::warn!(
                    "End shortcut '{}': {} always fired with another shortcut; its key combination goes",
                    end_keys[index].id,
                    items[i].original
                );
                end_keys[index].shortcut.clear();
                changed = true;
            }
        }
        let rest: Vec<usize> = group.into_iter().filter(|i| !lost.contains(i)).collect();
        let apart = rest.iter().all(|&a| {
            rest.iter()
                .all(|&b| a == b || !same_press(&items[a].target, &items[b].target))
        });
        if !apart {
            log::info!(
                "Key combinations told apart by their side keep it: {}",
                rest.iter()
                    .map(|&i| items[i].original.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            continue;
        }
        for &i in &rest {
            let item = &items[i];
            if item.target == item.original {
                continue;
            }
            log::info!("{} → {} (either side)", item.original, item.target);
            match &item.slot {
                Slot::Binding(id) => {
                    if let Some(binding) = bindings.get_mut(id) {
                        binding.current_binding = item.target.clone();
                    }
                }
                Slot::End(index) => end_keys[*index].shortcut = item.target.clone(),
            }
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use handy_keys::{Key, Modifiers};

    const MAC: KeyPlatform = KeyPlatform::Macos;
    const WINDOWS: KeyPlatform = KeyPlatform::Windows;

    fn hotkey(modifiers: Modifiers, key: Option<Key>) -> Hotkey {
        Hotkey::new(modifiers, key).expect("a hotkey")
    }

    // ---- Recording ---------------------------------------------------------

    #[test]
    fn a_combination_with_a_main_key_is_recorded_without_a_side() {
        let left = recorded(hotkey(Modifiers::CTRL_LEFT, Some(Key::Return)), MAC);
        assert_eq!(left.modifiers, Modifiers::CTRL);
        let right = recorded(hotkey(Modifiers::CTRL_RIGHT, Some(Key::Return)), MAC);
        assert_eq!(right, left);
        let three = recorded(
            hotkey(
                Modifiers::CTRL_LEFT | Modifiers::OPT_RIGHT | Modifiers::SHIFT_LEFT,
                Some(Key::E),
            ),
            MAC,
        );
        assert_eq!(
            three.modifiers,
            Modifiers::CTRL | Modifiers::OPT | Modifiers::SHIFT
        );
        let cmd = recorded(hotkey(Modifiers::CMD_RIGHT, Some(Key::K)), MAC);
        assert_eq!(cmd.modifiers, Modifiers::CMD);
        assert_eq!(cmd.key, Some(Key::K));
    }

    #[test]
    fn the_recorder_stores_the_combination_without_a_side() {
        let stored = recorded_string(hotkey(
            Modifiers::CTRL_LEFT | Modifiers::SHIFT_RIGHT,
            Some(Key::Space),
        ));
        assert_eq!(stored, "ctrl+shift+space");
    }

    #[test]
    fn modifiers_alone_keep_their_side() {
        for modifiers in [
            Modifiers::OPT_RIGHT,
            Modifiers::CTRL_LEFT | Modifiers::OPT_LEFT,
            Modifiers::CMD_RIGHT,
        ] {
            for platform in [MAC, WINDOWS] {
                assert_eq!(
                    recorded(hotkey(modifiers, None), platform).modifiers,
                    modifiers
                );
            }
        }
    }

    #[test]
    fn off_macos_alt_keeps_its_side_because_the_right_one_is_altgr() {
        let left_alt = recorded(hotkey(Modifiers::OPT_LEFT, Some(Key::Q)), WINDOWS);
        assert_eq!(left_alt.modifiers, Modifiers::OPT_LEFT);
        let altgr = recorded(
            hotkey(Modifiers::CTRL_LEFT | Modifiers::OPT_RIGHT, Some(Key::Q)),
            KeyPlatform::Linux,
        );
        assert_eq!(altgr.modifiers, Modifiers::CTRL | Modifiers::OPT_RIGHT);
        let option = recorded(hotkey(Modifiers::OPT_LEFT, Some(Key::Q)), MAC);
        assert_eq!(option.modifiers, Modifiers::OPT);
    }

    #[test]
    fn a_key_alone_stays_as_it_is() {
        let f13 = hotkey(Modifiers::empty(), Some(Key::F13));
        assert_eq!(recorded(f13, MAC), f13);
    }

    // ---- Stored strings ----------------------------------------------------

    #[test]
    fn a_stored_combination_loses_its_sides() {
        assert_eq!(
            without_sides("ctrl_left+return", MAC).as_deref(),
            Some("ctrl+return")
        );
        assert_eq!(
            without_sides("ctrl_left+option_left+e", MAC).as_deref(),
            Some("ctrl+option+e")
        );
        assert_eq!(
            without_sides("ctrl_left+shift_left+space", WINDOWS).as_deref(),
            Some("ctrl+shift+space")
        );
        assert_eq!(
            without_sides("super_right+shift_left+k", KeyPlatform::Linux).as_deref(),
            Some("super+shift+k")
        );
    }

    #[test]
    fn a_stored_combination_without_a_side_or_without_a_main_key_stays() {
        for binding in [
            "ctrl+option+d",
            "option_right",
            "ctrl_left+option_left",
            "f13",
            "escape",
            "",
        ] {
            assert_eq!(without_sides(binding, MAC), None, "{binding}");
        }
        assert_eq!(without_sides("not a combination+", MAC), None);
    }

    #[test]
    fn off_macos_a_stored_alt_keeps_its_side() {
        assert_eq!(without_sides("alt_left+q", WINDOWS), None);
        assert_eq!(
            without_sides("ctrl_left+alt_right+q", WINDOWS).as_deref(),
            Some("ctrl+alt_right+q")
        );
        assert_eq!(
            without_sides("ctrl_left+option_left+e", WINDOWS).as_deref(),
            Some("ctrl+option_left+e")
        );
    }

    // ---- The same press ----------------------------------------------------

    #[test]
    fn combinations_one_press_fires_both_count_as_the_same() {
        assert!(same_press("ctrl+return", "ctrl_left+return"));
        assert!(same_press("ctrl_right+return", "ctrl+return"));
        assert!(same_press("Ctrl+Return", "ctrl+return"));
        assert!(same_press("option", "option_right"));
        assert!(same_press("ctrl+option+e", "ctrl_left+option_right+e"));
    }

    #[test]
    fn combinations_no_single_press_fires_both_are_different() {
        assert!(!same_press("ctrl_left+return", "ctrl_right+return"));
        assert!(!same_press("ctrl+return", "ctrl+shift+return"));
        assert!(!same_press("ctrl+return", "ctrl+space"));
        assert!(!same_press("option_left", "option_right"));
        assert!(!same_press("option", "option+space"));
    }

    // ---- The stored settings -----------------------------------------------

    fn binding(id: &str, keys: &str) -> ShortcutBinding {
        ShortcutBinding {
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
            default_binding: keys.to_string(),
            current_binding: keys.to_string(),
        }
    }

    fn end(id: &str, shortcut: &str) -> EndKey {
        EndKey {
            id: id.to_string(),
            name: id.to_string(),
            shortcut: shortcut.to_string(),
            ..EndKey::default()
        }
    }

    fn bindings(list: &[(&str, &str)]) -> HashMap<String, ShortcutBinding> {
        list.iter()
            .map(|(id, keys)| (id.to_string(), binding(id, keys)))
            .collect()
    }

    fn values(shortcuts: &[(&str, &str)], ends: &[(&str, &str)]) -> ProfileValues {
        ProfileValues {
            bindings: bindings(shortcuts),
            end_keys: ends.iter().map(|(id, keys)| end(id, keys)).collect(),
            ..ProfileValues::default()
        }
    }

    /// Settings whose flat values, defaults and one profile hold `shortcuts`
    /// and `ends`.
    fn settings_with(shortcuts: &[(&str, &str)], ends: &[(&str, &str)]) -> AppSettings {
        let mut settings = crate::settings::get_default_settings();
        settings.bindings = bindings(shortcuts);
        settings.end_keys = ends.iter().map(|(id, keys)| end(id, keys)).collect();
        settings.default_values = Some(values(shortcuts, ends));
        settings.profiles = vec![crate::profiles::Profile::new(
            "work",
            "Arbeit",
            values(shortcuts, ends),
        )];
        settings
    }

    fn keys_of(bindings: &HashMap<String, ShortcutBinding>, id: &str) -> String {
        bindings[id].current_binding.clone()
    }

    /// One set of values: (shortcut id, keys) sorted, end shortcut keys in
    /// order.
    type Stored = (Vec<(String, String)>, Vec<String>);

    /// The flat values, the defaults and the profile.
    fn everywhere(settings: &AppSettings) -> Vec<Stored> {
        let of = |bindings: &HashMap<String, ShortcutBinding>, ends: &[EndKey]| {
            let mut shortcuts: Vec<(String, String)> = bindings
                .iter()
                .map(|(id, b)| (id.clone(), b.current_binding.clone()))
                .collect();
            shortcuts.sort();
            (shortcuts, ends.iter().map(|e| e.shortcut.clone()).collect())
        };
        let defaults = settings.default_values.as_ref().expect("defaults");
        vec![
            of(&settings.bindings, &settings.end_keys),
            of(&defaults.bindings, &defaults.end_keys),
            of(
                &settings.profiles[0].values.bindings,
                &settings.profiles[0].values.end_keys,
            ),
        ]
    }

    #[test]
    fn a_store_of_0_2_5_loses_the_sides_everywhere_once() {
        // The release's settings of 2026-11-22, as 0.2.5 stored them.
        let shortcuts = [
            ("cancel", "escape"),
            ("copy_last_transcript", "ctrl+shift+c"),
            ("cycle_word_list", "ctrl_left+shift_left+l"),
            ("paste_last_transcript", "ctrl_left+shift_left+v"),
            ("transcribe", "ctrl_left+space"),
            ("transcribe_with_post_process", "ctrl_left+shift_left+space"),
        ];
        let mut settings = settings_with(&shortcuts, &[]);
        assert!(migrate_on(&mut settings, MAC));
        for (shortcuts, _) in everywhere(&settings) {
            let keys: HashMap<String, String> = shortcuts.into_iter().collect();
            assert_eq!(keys["cancel"], "escape");
            assert_eq!(keys["copy_last_transcript"], "ctrl+shift+c");
            assert_eq!(keys["cycle_word_list"], "ctrl+shift+l");
            assert_eq!(keys["paste_last_transcript"], "ctrl+shift+v");
            assert_eq!(keys["transcribe"], "ctrl+space");
            assert_eq!(keys["transcribe_with_post_process"], "ctrl+shift+space");
        }
        let written = settings.clone();
        assert!(!migrate_on(&mut settings, MAC), "runs once");
        assert_eq!(settings.bindings, written.bindings);
    }

    #[test]
    fn end_shortcuts_lose_their_sides_too() {
        let mut settings = settings_with(
            &[("transcribe", "ctrl+option+d")],
            &[
                ("plain", ""),
                ("send", "ctrl_left+return"),
                ("hold", "option_right"),
            ],
        );
        assert!(migrate_on(&mut settings, MAC));
        for (_, ends) in everywhere(&settings) {
            assert_eq!(ends, vec!["", "ctrl+return", "option_right"]);
        }
    }

    #[test]
    fn a_store_without_sides_is_not_written() {
        let mut settings = settings_with(
            &[("transcribe", "ctrl+option+d"), ("hold", "option_right")],
            &[("send", "ctrl+return")],
        );
        assert!(!migrate_on(&mut settings, MAC));
    }

    #[test]
    fn an_end_shortcut_that_never_fired_alone_loses_its_combination() {
        // Every press of the left Ctrl + Return fired "sideless" as well.
        let mut settings = settings_with(
            &[("transcribe", "ctrl+option+d")],
            &[("left", "ctrl_left+return"), ("sideless", "ctrl+return")],
        );
        assert!(migrate_on(&mut settings, MAC));
        for (_, ends) in everywhere(&settings) {
            assert_eq!(ends, vec!["", "ctrl+return"]);
        }
        assert_eq!(settings.end_keys.len(), 2, "the entry itself stays");
    }

    #[test]
    fn an_end_shortcut_on_a_shortcuts_keys_loses_its_combination() {
        let mut settings = settings_with(
            &[("transcribe", "ctrl+space")],
            &[("send", "ctrl_left+space")],
        );
        assert!(migrate_on(&mut settings, MAC));
        assert_eq!(settings.end_keys[0].shortcut, "");
        assert_eq!(keys_of(&settings.bindings, "transcribe"), "ctrl+space");
    }

    #[test]
    fn combinations_told_apart_by_their_side_keep_it() {
        // The left and the right Ctrl each did something else; nothing merges.
        let mut settings = settings_with(
            &[
                ("transcribe", "ctrl_left+x"),
                ("copy_last_transcript", "ctrl_right+x"),
            ],
            &[("left", "ctrl_left+return"), ("right", "ctrl_right+return")],
        );
        assert!(!migrate_on(&mut settings, MAC));
        assert_eq!(keys_of(&settings.bindings, "transcribe"), "ctrl_left+x");
        assert_eq!(
            keys_of(&settings.bindings, "copy_last_transcript"),
            "ctrl_right+x"
        );
        assert_eq!(settings.end_keys[0].shortcut, "ctrl_left+return");
        assert_eq!(settings.end_keys[1].shortcut, "ctrl_right+return");
    }

    #[test]
    fn an_end_shortcut_that_also_fired_alone_keeps_its_combination() {
        // "send" fired alone with the right Ctrl, so it stays, and the
        // shortcut keeps its side rather than take that press over.
        let mut settings = settings_with(&[("transcribe", "ctrl_left+x")], &[("send", "ctrl+x")]);
        assert!(!migrate_on(&mut settings, MAC));
        assert_eq!(keys_of(&settings.bindings, "transcribe"), "ctrl_left+x");
        assert_eq!(settings.end_keys[0].shortcut, "ctrl+x");
    }

    #[test]
    fn off_macos_what_stays_apart_by_alt_loses_the_other_sides() {
        let mut settings = settings_with(
            &[
                ("transcribe", "ctrl_left+alt_left+x"),
                ("copy_last_transcript", "ctrl_right+alt_right+x"),
            ],
            &[],
        );
        assert!(migrate_on(&mut settings, WINDOWS));
        assert_eq!(keys_of(&settings.bindings, "transcribe"), "ctrl+alt_left+x");
        assert_eq!(
            keys_of(&settings.bindings, "copy_last_transcript"),
            "ctrl+alt_right+x"
        );
    }

    #[test]
    fn an_imported_profile_loses_the_sides() {
        let mut imported = values(
            &[("transcribe", "ctrl_left+space"), ("hold", "option_right")],
            &[("send", "command_right+return")],
        );
        prepare_import(&mut imported);
        assert_eq!(keys_of(&imported.bindings, "transcribe"), "ctrl+space");
        assert_eq!(keys_of(&imported.bindings, "hold"), "option_right");
        assert!(
            imported.end_keys[0].shortcut == "command+return"
                || imported.end_keys[0].shortcut == "super+return",
            "{}",
            imported.end_keys[0].shortcut
        );
    }
}
