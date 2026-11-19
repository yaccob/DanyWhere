//! DanyWhere: the defaults and the profiles that override them ("defaults
//! for everything, a profile overrides what it needs"; user, 2026-11-19,
//! decisions P1–P11 in the sidecar note `2026-11-19_1711`).
//!
//! The defaults (the "Grundeinstellungen", `AppSettings::default_values`)
//! hold a value for every field of [`ProfileValues`]. A profile holds each
//! field either **coupled** (it follows the default) or as its **own** value
//! (it stays when the default changes, even while it happens to be equal).
//!
//! The model is *materialized*: `Profile::values` always holds a profile's
//! complete effective values, and `Profile::own` names the fields it holds
//! as its own. A coupled field always equals the default; when a default
//! changes, [`propagate`] copies it into every profile's coupled field. So
//! the code that reads a profile's values (`app_profiles::with_values`,
//! `dictation_settings`, `profile_redo`, the export) stays as it is.
//!
//! Everything here works on the serde names of the fields of
//! [`ProfileValues`], through `serde_json`, with no code per field: a field
//! added to `ProfileValues` (and mirrored in `from_settings`/`apply_to`)
//! couples without touching this file.
//!
//! **Lists entry by entry (P10).** Fields registered in [`LIST_FIELDS`] are
//! never coupled or own as a whole: each entry is. A profile can change an
//! entry, add one, or switch one of the defaults' entries off
//! ([`OwnEntries`]). A list is either an array of objects told apart by a
//! property (`word_lists` by `id`) or a map told apart by its keys (the
//! model and effort per provider).
//!
//! **In use (P2).** `active_profile_id == DEFAULT_PROFILE_ID` means the
//! defaults are in use; no profile has that id. The flat fields of
//! `AppSettings` keep holding the effective values of whatever is in use.
//!
//! **Old write paths.** Handy's `change_*_setting` commands, the tray and
//! the shortcuts keep writing the flat fields. [`route_flat`] (called by
//! `settings::write_settings`) sends every changed field where its value
//! came from:
//!
//! - the defaults are in use → the default (and on to every coupled profile);
//! - a profile is in use and holds the field as its own → that profile;
//! - a profile is in use and the field is coupled → the default (and on).
//!
//! In a list, each entry counts as a value of its own: a changed or removed
//! entry goes where it came from (a profile's own entry → the profile, where
//! removing one of the defaults' entries switches it off; a coupled entry →
//! the defaults); a new entry has no source yet and counts as coupled, so
//! it goes to the defaults, unless the profile had switched that id off,
//! which then becomes its own entry. This rule holds until the interface
//! names a target itself (strand E2), through the commands below, which say
//! where a value goes ([`Scope`]).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;

use crate::profiles::{Profile, ProfileValues};
use crate::settings::{self, AppSettings};

/// Where a command sets or removes a value: in the defaults or in a profile.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Scope {
    /// The defaults ("Grundeinstellungen").
    Defaults,
    /// The profile with this id.
    Profile { id: String },
}

/// A profile's own entries of one list field (P10).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default, Type)]
pub struct OwnEntries {
    /// Entries the profile changed or added, by id: they stay as they are in
    /// the profile when the defaults change.
    #[serde(default)]
    pub own: BTreeSet<String>,
    /// Entries of the defaults the profile switched off, by id.
    #[serde(default)]
    pub off: BTreeSet<String>,
}

impl OwnEntries {
    /// Whether the profile follows the defaults in every entry.
    pub fn is_empty(&self) -> bool {
        self.own.is_empty() && self.off.is_empty()
    }
}

/// How the entries of a list field are told apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKey {
    /// An array of objects, each told apart by this property.
    Property(&'static str),
    /// A map, each entry told apart by its key.
    MapKey,
}

/// A field of [`ProfileValues`] that deviates entry by entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListField {
    /// The serde name of the field.
    pub field: &'static str,
    pub key: EntryKey,
}

/// The fields of [`ProfileValues`] that deviate entry by entry (P10); every
/// other field is coupled or own as a whole. A new list plugs in with one
/// line; strand D's two fields, for example:
///
/// ```text
/// ListField { field: "bindings", key: EntryKey::MapKey },     // HashMap<String, ShortcutBinding>
/// ListField { field: "end_keys", key: EntryKey::Property("id") }, // Vec<EndKey>
/// ```
///
/// The model and the effort of post-processing are kept per provider, so a
/// profile that picks its own model for one provider still follows the
/// defaults for the others (the user's table counts "model (Anthropic)" as
/// one value).
pub const LIST_FIELDS: &[ListField] = &[
    ListField {
        field: "word_lists",
        key: EntryKey::Property("id"),
    },
    ListField {
        field: "post_process_models",
        key: EntryKey::MapKey,
    },
    ListField {
        field: "post_process_efforts",
        key: EntryKey::MapKey,
    },
];

/// How a field of [`ProfileValues`] couples, for the interface.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FieldKind {
    /// One value, coupled or own as a whole.
    Value,
    /// A list of objects told apart by the property `key`, entry by entry.
    List { key: String },
    /// A map whose entries are told apart by their keys, entry by entry.
    Map,
}

/// A field of [`ProfileValues`]: its serde name and how it couples.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct ProfileField {
    pub name: String,
    pub kind: FieldKind,
}

const NOT_BUILT: &str = "not built yet";

/// The serde names of the fields of [`ProfileValues`], in their order.
pub fn field_names() -> Vec<String> {
    Vec::new()
}

/// Every field of [`ProfileValues`] with how it couples.
pub fn fields() -> Vec<ProfileField> {
    Vec::new()
}

/// How many own values `profile` has (P5): its own fields plus, per list,
/// its own and its switched-off entries.
pub fn own_count(_profile: &Profile) -> usize {
    0
}

/// The one-time switch from the profile "Standard" to the defaults (P1).
/// Runs while `default_values` is `None` (a store from before them, or a
/// new installation); returns whether it changed anything.
pub fn migrate_to_defaults(_settings: &mut AppSettings) -> bool {
    false
}

/// Couples the fields and entries of `profile` that equal `defaults` and
/// makes the others its own (the migration and an import, P1).
pub fn couple_equal(_profile: &mut Profile, _defaults: &ProfileValues) {}

/// Copies the defaults into every profile's coupled fields and entries and
/// the values in use into the flat fields. Returns whether anything changed.
pub fn propagate(_settings: &mut AppSettings) -> bool {
    false
}

/// The old write paths: sends every flat field that differs from what is in
/// use where its value came from (rule in the module comment), then
/// [`propagate`]s. Returns whether anything changed.
pub fn route_flat(_settings: &mut AppSettings) -> bool {
    false
}

/// [`route_flat`] for profile `id` (or the defaults) instead of what is in
/// use: `values` are its values after a change through an old write path.
pub fn route(_settings: &mut AppSettings, _id: &str, _values: &ProfileValues) -> bool {
    false
}

/// Sets `field` in `scope` to `value`. In a profile this makes it the
/// profile's own (P7.3); a list's changed and added entries become its own,
/// removed entries of the defaults are switched off.
pub fn set_value(
    _settings: &mut AppSettings,
    _scope: &Scope,
    _field: &str,
    _value: serde_json::Value,
) -> Result<(), String> {
    Err(NOT_BUILT.to_string())
}

/// Holds `field` in profile `id` as its own without changing it (P4); a
/// list's entries all become its own, the defaults' missing ones off.
pub fn hold_value(_settings: &mut AppSettings, _id: &str, _field: &str) -> Result<(), String> {
    Err(NOT_BUILT.to_string())
}

/// Couples `fields` of profile `id` to the defaults again (P8: one value,
/// or the values of a page), entries of lists included.
pub fn reset_values(
    _settings: &mut AppSettings,
    _id: &str,
    _fields: &[String],
) -> Result<(), String> {
    Err(NOT_BUILT.to_string())
}

/// Couples every value of profile `id` to the defaults again (P8); its name
/// and applications stay.
pub fn reset_profile(_settings: &mut AppSettings, _id: &str) -> Result<(), String> {
    Err(NOT_BUILT.to_string())
}

/// "Change the default instead" (P7.3): the default takes profile `id`'s
/// value of `field`, and the profile is coupled to it again.
pub fn adopt_as_default(
    _settings: &mut AppSettings,
    _id: &str,
    _field: &str,
) -> Result<(), String> {
    Err(NOT_BUILT.to_string())
}

/// Sets entry `entry_id` of list `field` in `scope` to `entry`, adding it
/// when there is none; in a profile the entry becomes its own.
pub fn set_entry(
    _settings: &mut AppSettings,
    _scope: &Scope,
    _field: &str,
    _entry_id: &str,
    _entry: serde_json::Value,
) -> Result<(), String> {
    Err(NOT_BUILT.to_string())
}

/// Removes entry `entry_id` of list `field` in `scope`: from the defaults
/// (and every profile that follows them in it), or from a profile, where
/// one of the defaults' entries is switched off.
pub fn remove_entry(
    _settings: &mut AppSettings,
    _scope: &Scope,
    _field: &str,
    _entry_id: &str,
) -> Result<(), String> {
    Err(NOT_BUILT.to_string())
}

/// Couples entry `entry_id` of list `field` in profile `id` to the defaults
/// again: a changed or switched-off entry follows them, an added one goes.
pub fn reset_entry(
    _settings: &mut AppSettings,
    _id: &str,
    _field: &str,
    _entry_id: &str,
) -> Result<(), String> {
    Err(NOT_BUILT.to_string())
}

/// Runs `change` on the stored settings, stores them and applies what
/// changed for the values in use (`profile_effects::apply`).
fn change<T>(
    app: &AppHandle,
    change: impl FnOnce(&mut AppSettings) -> Result<T, String>,
) -> Result<T, String> {
    let mut settings = settings::get_settings(app);
    let before = settings.clone();
    let out = change(&mut settings)?;
    settings::write_settings(app, settings.clone());
    crate::profile_effects::apply(app, &before, &settings);
    Ok(out)
}

/// Every field of a profile's values with how it couples: the interface
/// shows chains and counts by it.
#[tauri::command]
#[specta::specta]
pub fn get_profile_fields() -> Vec<ProfileField> {
    fields()
}

/// Sets a value in the defaults or in a profile (P7.3: in a profile it
/// becomes the profile's own). `value` is checked against the field's type
/// and validated like the field's own change command validates it.
#[tauri::command]
#[specta::specta]
pub fn set_profile_value(
    app: AppHandle,
    scope: Scope,
    field: String,
    value: serde_json::Value,
) -> Result<(), String> {
    change(&app, |settings| set_value(settings, &scope, &field, value))
}

/// Holds a value of profile `id` as its own without changing it (P4).
#[tauri::command]
#[specta::specta]
pub fn hold_profile_value(app: AppHandle, id: String, field: String) -> Result<(), String> {
    change(&app, |settings| hold_value(settings, &id, &field))
}

/// Couples values of profile `id` to the defaults again: one, or the values
/// of a page (P8).
#[tauri::command]
#[specta::specta]
pub fn reset_profile_values(app: AppHandle, id: String, fields: Vec<String>) -> Result<(), String> {
    change(&app, |settings| reset_values(settings, &id, &fields))
}

/// Couples every value of profile `id` to the defaults again (P8); name and
/// applications stay.
#[tauri::command]
#[specta::specta]
pub fn reset_whole_profile(app: AppHandle, id: String) -> Result<(), String> {
    change(&app, |settings| reset_profile(settings, &id))
}

/// "Change the default instead" (P7.3).
#[tauri::command]
#[specta::specta]
pub fn use_profile_value_as_default(
    app: AppHandle,
    id: String,
    field: String,
) -> Result<(), String> {
    change(&app, |settings| adopt_as_default(settings, &id, &field))
}

/// Sets or adds an entry of a list in the defaults or in a profile (P10).
#[tauri::command]
#[specta::specta]
pub fn set_profile_entry(
    app: AppHandle,
    scope: Scope,
    field: String,
    entry_id: String,
    entry: serde_json::Value,
) -> Result<(), String> {
    change(&app, |settings| {
        set_entry(settings, &scope, &field, &entry_id, entry)
    })
}

/// Removes an entry of a list from the defaults, or switches it off in a
/// profile (P10).
#[tauri::command]
#[specta::specta]
pub fn remove_profile_entry(
    app: AppHandle,
    scope: Scope,
    field: String,
    entry_id: String,
) -> Result<(), String> {
    change(&app, |settings| {
        remove_entry(settings, &scope, &field, &entry_id)
    })
}

/// Couples an entry of a list in profile `id` to the defaults again (P10).
#[tauri::command]
#[specta::specta]
pub fn reset_profile_entry(
    app: AppHandle,
    id: String,
    field: String,
    entry_id: String,
) -> Result<(), String> {
    change(&app, |settings| {
        reset_entry(settings, &id, &field, &entry_id)
    })
}
