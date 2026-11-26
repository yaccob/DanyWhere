//! DanyWhere: the profile's mode of post-processing (user, 2026-11-26,
//! variant b of the sidecar note
//! `2026-11-26_1841_nachbearbeitung-nur-ueber-anweisungstaste`):
//! `settings::PostProcessMode` (never / with an instruction key / always)
//! instead of the switch "Post Processing" (`post_process_enabled`) and the
//! box "Post-process" of every end and hold shortcut.
//!
//! The step from a store before it is cheap (user, 2026-11-26: "es gibt
//! praktisch noch keine User"): every layer's switch on becomes "with an
//! instruction key", off becomes "never"; a profile that held the switch as
//! its own holds the mode as its own. The boxes of the ends are ignored on
//! read (`end_keys::EndKey` has none any more).

use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::profiles::ProfileValues;
use crate::settings::{AppSettings, PostProcessMode};

/// The serde name of the mode.
const MODE: &str = "post_process_mode";
/// The serde name of the switch it replaces.
const SWITCH: &str = "post_process_enabled";

/// The mode of a switch: on → with an instruction key, off → never.
fn of_switch(enabled: bool) -> PostProcessMode {
    if enabled {
        PostProcessMode::WithInstructionKey
    } else {
        PostProcessMode::Never
    }
}

/// The mode a layer's stored values (`stored`) hold: their own mode, else
/// that of their switch, else "never", as Handy's switch was off out of the
/// box. `None` without stored values.
pub fn stored_mode(stored: Option<&Value>) -> Option<PostProcessMode> {
    let stored = stored?;
    if let Some(mode) = stored
        .get(MODE)
        .and_then(|mode| serde_json::from_value(mode.clone()).ok())
    {
        return Some(mode);
    }
    Some(of_switch(
        stored.get(SWITCH).and_then(Value::as_bool).unwrap_or(false),
    ))
}

/// The mode stored values from before it carry over: only those with the
/// switch and without the mode; others keep what the read gave them (a
/// stored profile lacking both takes the flat values', contract V5).
fn carried_over(stored: Option<&Value>) -> Option<PostProcessMode> {
    let stored = stored?;
    if stored.get(MODE).is_some() {
        return None;
    }
    stored.get(SWITCH).and_then(Value::as_bool).map(of_switch)
}

/// The flat values' step (`stored` is the store's JSON), before the
/// profiles and the defaults are filled from them. Returns whether the
/// store was from before the mode.
pub fn migrate_flat(settings: &mut AppSettings, stored: &Value) -> bool {
    if stored.get(MODE).is_some() {
        return false;
    }
    if let Some(mode) = stored_mode(Some(stored)) {
        log::info!("Post-processing: the switch becomes the mode {mode:?}");
        settings.post_process_mode = mode;
    }
    true
}

/// The defaults' and every profile's step, after the filling of missing
/// values: each takes its own switch; a profile that held the switch as
/// its own holds the mode as its own. Returns whether anything changed.
pub fn migrate_layers(settings: &mut AppSettings, stored: &Value) -> bool {
    let mut changed = false;
    let stored_defaults = stored.get("default_values").filter(|v| !v.is_null());
    if let (Some(defaults), Some(mode)) = (
        settings.default_values.as_mut(),
        carried_over(stored_defaults),
    ) {
        changed |= defaults.post_process_mode != mode;
        defaults.post_process_mode = mode;
    }
    let stored_profiles = stored.get("profiles").and_then(Value::as_array);
    for profile in settings.profiles.iter_mut() {
        let stored_values = stored_profiles
            .and_then(|profiles| {
                profiles
                    .iter()
                    .find(|stored| stored.get("id").and_then(Value::as_str) == Some(&profile.id))
            })
            .and_then(|stored| stored.get("values"));
        if let Some(mode) = carried_over(stored_values) {
            changed |= profile.values.post_process_mode != mode;
            profile.values.post_process_mode = mode;
        }
        if profile.own.remove(SWITCH) {
            profile.own.insert(MODE.to_string());
            changed = true;
        }
    }
    changed
}

/// For an import (`stored` is the file's profile values): a file from
/// before the mode takes its switch's. Run after the values the file lacks
/// were filled from the defaults (`profile_transfer::prepare_import_values`).
pub fn prepare_import(stored: &serde_json::Map<String, Value>, values: &mut ProfileValues) {
    if stored.contains_key(MODE) {
        return;
    }
    if let Some(enabled) = stored.get(SWITCH).and_then(Value::as_bool) {
        values.post_process_mode = of_switch(enabled);
    }
}

/// Stores the active profile's mode of post-processing. The instruction
/// shortcuts follow it (`instruction_keys::taken`).
#[tauri::command]
#[specta::specta]
pub fn change_post_process_mode_setting(
    app: AppHandle,
    mode: PostProcessMode,
) -> Result<(), String> {
    let mut settings = crate::settings::get_settings(&app);
    settings.post_process_mode = mode;
    crate::settings::write_settings(&app, settings);
    let _ = app.emit("settings-changed", serde_json::json!({ "setting": MODE }));
    crate::shortcut::profile_sync::sync_now(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::get_default_settings;

    /// A store from before the mode: the switch, flat and in the defaults
    /// and a profile "work" that held it as its own.
    fn store(flat: bool, defaults: bool, work: bool) -> serde_json::Value {
        let mut stored = serde_json::to_value(get_default_settings()).unwrap();
        let map = stored.as_object_mut().unwrap();
        map.remove("post_process_mode");
        map.insert("post_process_enabled".into(), serde_json::json!(flat));
        let mut values = serde_json::to_value(ProfileValues::default()).unwrap();
        let values_map = values.as_object_mut().unwrap();
        values_map.remove("post_process_mode");
        values_map.insert("post_process_enabled".into(), serde_json::json!(defaults));
        let mut work_values = values.clone();
        work_values["post_process_enabled"] = serde_json::json!(work);
        map.insert("default_values".into(), values);
        map.insert("active_profile_id".into(), serde_json::json!("default"));
        map.insert(
            "profiles".into(),
            serde_json::json!([{
                "id": "work", "name": "Arbeit", "apps": [], "values": work_values,
                "own": ["post_process_enabled"], "own_entries": {}
            }]),
        );
        stored
    }

    /// `stored` read and taken through the steps as the store's read does.
    fn migrated(stored: &serde_json::Value) -> (AppSettings, bool) {
        let mut settings: AppSettings = serde_json::from_value(stored.clone()).unwrap();
        let mut changed = migrate_flat(&mut settings, stored);
        crate::profiles::fill_missing_values_from_flat(&mut settings, stored);
        crate::profile_layers::fill_missing_defaults_from_flat(&mut settings, stored);
        changed |= migrate_layers(&mut settings, stored);
        (settings, changed)
    }

    #[test]
    fn the_switch_on_is_with_instruction_key_off_is_never() {
        let on = serde_json::json!({ "post_process_enabled": true });
        let off = serde_json::json!({ "post_process_enabled": false });
        assert_eq!(
            stored_mode(Some(&on)),
            Some(PostProcessMode::WithInstructionKey)
        );
        assert_eq!(stored_mode(Some(&off)), Some(PostProcessMode::Never));
        let both =
            serde_json::json!({ "post_process_enabled": false, "post_process_mode": "always" });
        assert_eq!(
            stored_mode(Some(&both)),
            Some(PostProcessMode::Always),
            "a stored mode wins"
        );
        assert_eq!(
            stored_mode(Some(&serde_json::json!({}))),
            Some(PostProcessMode::Never),
            "a store without the switch had it off"
        );
    }

    #[test]
    fn every_layer_takes_its_own_switch() {
        let (s, changed) = migrated(&store(true, false, true));
        assert!(changed);
        assert_eq!(s.post_process_mode, PostProcessMode::WithInstructionKey);
        assert_eq!(
            s.default_values
                .as_ref()
                .map(|values| values.post_process_mode),
            Some(PostProcessMode::Never)
        );
        assert_eq!(
            s.profiles[0].values.post_process_mode,
            PostProcessMode::WithInstructionKey
        );
        let (s, _) = migrated(&store(false, true, false));
        assert_eq!(s.post_process_mode, PostProcessMode::Never);
        assert_eq!(
            s.default_values
                .as_ref()
                .map(|values| values.post_process_mode),
            Some(PostProcessMode::WithInstructionKey)
        );
        assert_eq!(
            s.profiles[0].values.post_process_mode,
            PostProcessMode::Never
        );
    }

    #[test]
    fn a_profile_that_held_the_switch_as_its_own_holds_the_mode() {
        let (s, _) = migrated(&store(true, true, false));
        let own = &s.profiles[0].own;
        assert!(own.contains("post_process_mode"), "{own:?}");
        assert!(!own.contains("post_process_enabled"), "{own:?}");
    }

    #[test]
    fn a_second_read_changes_nothing() {
        let (s, _) = migrated(&store(true, false, true));
        let written = serde_json::to_value(&s).unwrap();
        let (again, changed) = migrated(&written);
        assert!(!changed);
        assert_eq!(again.post_process_mode, s.post_process_mode);
        assert_eq!(again.profiles, s.profiles);
    }

    #[test]
    fn an_imported_profile_takes_its_switch() {
        let file = serde_json::json!({ "post_process_enabled": false });
        let mut values = ProfileValues::default();
        assert_eq!(
            values.post_process_mode,
            PostProcessMode::WithInstructionKey
        );
        prepare_import(file.as_object().unwrap(), &mut values);
        assert_eq!(values.post_process_mode, PostProcessMode::Never);
        let mut kept = ProfileValues::default();
        prepare_import(
            serde_json::json!({ "post_process_mode": "always" })
                .as_object()
                .unwrap(),
            &mut kept,
        );
        assert_eq!(
            kept.post_process_mode,
            PostProcessMode::WithInstructionKey,
            "a file with the mode keeps what serde read"
        );
    }
}
