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

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use tauri::AppHandle;

use crate::profiles::{self, Profile, ProfileValues, DEFAULT_PROFILE_ID};
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
/// Post-processing is kept per provider (its settings, model, effort and
/// API key), so a profile that picks its own model or key for one provider
/// still follows the defaults for the others (the user's table counts
/// "model (Anthropic)" as one value).
pub const LIST_FIELDS: &[ListField] = &[
    ListField {
        field: "word_lists",
        key: EntryKey::Property("id"),
    },
    ListField {
        field: "post_process_providers",
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
    ListField {
        field: "post_process_api_keys",
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

/// The values of a profile (or the defaults) as a JSON object, by the serde
/// names of the fields of [`ProfileValues`]. Everything below works on this,
/// so it needs no code per field.
pub type Fields = serde_json::Map<String, Value>;

fn to_fields(values: &ProfileValues) -> Fields {
    match serde_json::to_value(values) {
        Ok(Value::Object(fields)) => fields,
        // A struct of plain data always serializes to an object.
        _ => Fields::new(),
    }
}

fn from_fields(fields: Fields) -> Result<ProfileValues, String> {
    serde_json::from_value(Value::Object(fields)).map_err(|e| format!("Invalid profile value: {e}"))
}

/// How the entries of `field` are told apart, if it deviates entry by entry.
fn list_key(lists: &[ListField], field: &str) -> Option<EntryKey> {
    lists
        .iter()
        .find(|list| list.field == field)
        .map(|list| list.key)
}

/// The serde names of the fields of [`ProfileValues`], sorted
/// (`profiles::value_names`).
pub fn field_names() -> Vec<String> {
    profiles::value_names().to_vec()
}

/// Every field of [`ProfileValues`] with how it couples.
pub fn fields() -> Vec<ProfileField> {
    field_names()
        .into_iter()
        .map(|name| {
            let kind = match list_key(LIST_FIELDS, &name) {
                Some(EntryKey::Property(key)) => FieldKind::List {
                    key: key.to_string(),
                },
                Some(EntryKey::MapKey) => FieldKind::Map,
                None => FieldKind::Value,
            };
            ProfileField { name, kind }
        })
        .collect()
}

/// How `field` couples, or an error for a name `ProfileValues` doesn't have.
fn check_field(field: &str) -> Result<Option<EntryKey>, String> {
    if field_names().iter().any(|name| name == field) {
        Ok(list_key(LIST_FIELDS, field))
    } else {
        Err(format!("Unknown profile value '{field}'"))
    }
}

/// How many own values `profile` has (P5): its own fields plus, per list,
/// its own and its switched-off entries.
pub fn own_count(profile: &Profile) -> usize {
    profile.own.len()
        + profile
            .own_entries
            .values()
            .map(|entries| entries.own.len() + entries.off.len())
            .sum::<usize>()
}

// ---- Lists entry by entry ------------------------------------------------

/// The entries of a list value, in order, by id.
fn entries(key: EntryKey, value: Option<&Value>) -> Vec<(String, Value)> {
    match (key, value) {
        (EntryKey::Property(name), Some(Value::Array(items))) => items
            .iter()
            .filter_map(|item| Some((item.get(name)?.as_str()?.to_string(), item.clone())))
            .collect(),
        (EntryKey::MapKey, Some(Value::Object(map))) => map
            .iter()
            .map(|(id, entry)| (id.clone(), entry.clone()))
            .collect(),
        _ => Vec::new(),
    }
}

/// A list value made of `entries`.
fn assemble(key: EntryKey, entries: Vec<(String, Value)>) -> Value {
    match key {
        EntryKey::Property(_) => {
            Value::Array(entries.into_iter().map(|(_, entry)| entry).collect())
        }
        EntryKey::MapKey => Value::Object(entries.into_iter().collect()),
    }
}

fn find<'a>(entries: &'a [(String, Value)], id: &str) -> Option<&'a Value> {
    entries
        .iter()
        .find(|(entry_id, _)| entry_id == id)
        .map(|(_, entry)| entry)
}

/// `list` with `entry` as entry `id`, in its place or at the end.
fn upsert(list: &mut Vec<(String, Value)>, id: &str, entry: &Value) {
    match list.iter_mut().find(|(entry_id, _)| entry_id == id) {
        Some(slot) => slot.1 = entry.clone(),
        None => list.push((id.to_string(), entry.clone())),
    }
}

/// A profile's effective list: the defaults' entries it follows, its own
/// versions of others, without those it switched off, then the entries it
/// added, in its order.
fn merge_list(key: EntryKey, defaults: &Value, profile: &Value, own: &OwnEntries) -> Value {
    let defaults = entries(key, Some(defaults));
    let mine = entries(key, Some(profile));
    let mut merged = Vec::new();
    for (id, entry) in &defaults {
        if own.off.contains(id) {
            continue;
        }
        let entry = if own.own.contains(id) {
            find(&mine, id).unwrap_or(entry)
        } else {
            entry
        };
        merged.push((id.clone(), entry.clone()));
    }
    for (id, entry) in &mine {
        if own.own.contains(id) && find(&defaults, id).is_none() {
            merged.push((id.clone(), entry.clone()));
        }
    }
    assemble(key, merged)
}

/// The own entries of a list that `profile` holds against `defaults`: an
/// equal entry is coupled, a differing or added one its own, a missing one
/// switched off.
fn compare_list(key: EntryKey, defaults: &Value, profile: &Value) -> OwnEntries {
    let defaults = entries(key, Some(defaults));
    let mine = entries(key, Some(profile));
    OwnEntries {
        own: mine
            .iter()
            .filter(|(id, entry)| find(&defaults, id) != Some(entry))
            .map(|(id, _)| id.clone())
            .collect(),
        off: defaults
            .iter()
            .filter(|(id, _)| find(&mine, id).is_none())
            .map(|(id, _)| id.clone())
            .collect(),
    }
}

/// Drops own ids the profile's list no longer has and switched-off ids the
/// defaults no longer have.
fn tidy_list(key: EntryKey, defaults: &Value, profile: &Value, own: &mut OwnEntries) {
    let defaults = entries(key, Some(defaults));
    let mine = entries(key, Some(profile));
    own.own.retain(|id| find(&mine, id).is_some());
    own.off
        .retain(|id| find(&defaults, id).is_some() && !own.own.contains(id));
}

// ---- Fields --------------------------------------------------------------

/// A profile's effective values from the defaults and what it holds as its
/// own; `lists` say which fields deviate entry by entry.
fn effective_fields(
    lists: &[ListField],
    defaults: &Fields,
    profile: &Fields,
    own: &BTreeSet<String>,
    own_entries: &BTreeMap<String, OwnEntries>,
) -> Fields {
    let mut effective = profile.clone();
    let coupled = OwnEntries::default();
    for (field, default) in defaults {
        let value = match list_key(lists, field) {
            Some(key) => merge_list(
                key,
                default,
                profile.get(field).unwrap_or(&Value::Null),
                own_entries.get(field).unwrap_or(&coupled),
            ),
            None if own.contains(field) && profile.contains_key(field) => continue,
            None => default.clone(),
        };
        effective.insert(field.clone(), value);
    }
    effective
}

/// What `profile` holds as its own against `defaults`: the fields that
/// differ, and per list the entries that differ or are missing.
fn compare_fields(
    lists: &[ListField],
    defaults: &Fields,
    profile: &Fields,
) -> (BTreeSet<String>, BTreeMap<String, OwnEntries>) {
    let mut own = BTreeSet::new();
    let mut own_entries = BTreeMap::new();
    for (field, value) in profile {
        let Some(default) = defaults.get(field) else {
            continue;
        };
        match list_key(lists, field) {
            Some(key) => {
                let entries = compare_list(key, default, value);
                if !entries.is_empty() {
                    own_entries.insert(field.clone(), entries);
                }
            }
            None if value != default => {
                own.insert(field.clone());
            }
            None => {}
        }
    }
    (own, own_entries)
}

/// Sends what changed from `old` to `new` (a profile's values, through an
/// old write path) where it came from: into `profile` (its fields) where it
/// holds the field or entry as its own, else into `defaults` (rule in the
/// module comment). The profile's lists take `new` as they are; merging
/// them with the defaults afterwards keeps only its own entries.
fn route_fields(
    lists: &[ListField],
    defaults: &mut Fields,
    profile: &mut Fields,
    own: &BTreeSet<String>,
    own_entries: &mut BTreeMap<String, OwnEntries>,
    old: &Fields,
    new: &Fields,
) {
    for (field, new_value) in new {
        let old_value = old.get(field).unwrap_or(&Value::Null);
        if old_value == new_value {
            continue;
        }
        let Some(key) = list_key(lists, field) else {
            let target = if own.contains(field) {
                &mut *profile
            } else {
                &mut *defaults
            };
            target.insert(field.clone(), new_value.clone());
            continue;
        };
        let held = own_entries.entry(field.clone()).or_default();
        let olds = entries(key, Some(old_value));
        let news = entries(key, Some(new_value));
        let mut default_list = entries(key, defaults.get(field));
        for (id, entry) in &news {
            match find(&olds, id) {
                Some(previous) if previous == entry => {}
                // A changed entry: the profile's own stays in the profile.
                Some(_) if held.own.contains(id) => {}
                Some(_) => upsert(&mut default_list, id, entry),
                // A new entry: its own again where the profile had
                // switched that id off, else one more of the defaults'.
                None if held.off.remove(id) => {
                    held.own.insert(id.clone());
                }
                None if held.own.contains(id) => {}
                None => upsert(&mut default_list, id, entry),
            }
        }
        for (id, _) in &olds {
            if find(&news, id).is_some() {
                continue;
            }
            if held.own.remove(id) {
                if find(&default_list, id).is_some() {
                    held.off.insert(id.clone());
                }
            } else {
                default_list.retain(|(entry_id, _)| entry_id != id);
            }
        }
        defaults.insert(field.clone(), assemble(key, default_list));
        profile.insert(field.clone(), new_value.clone());
    }
}

// ---- Profiles and defaults -----------------------------------------------

fn defaults_fields(settings: &AppSettings) -> Fields {
    to_fields(&profiles::default_values(settings))
}

/// The values in use: the profile in use's, or the defaults.
fn in_use_values(settings: &AppSettings) -> ProfileValues {
    settings
        .profiles
        .iter()
        .find(|profile| profile.id == settings.active_profile_id)
        .map(|profile| profile.values.clone())
        .unwrap_or_else(|| profiles::default_values(settings))
}

/// Recomputes `profile`'s effective values from `defaults` and tidies what
/// it holds as its own. Returns whether anything changed.
fn recompute(profile: &mut Profile, defaults: &Fields) -> bool {
    let fields = to_fields(&profile.values);
    let before = (profile.own.clone(), profile.own_entries.clone());
    // A field that became a list since: its entries become own instead.
    let lists: Vec<String> = profile
        .own
        .iter()
        .filter(|field| list_key(LIST_FIELDS, field).is_some())
        .cloned()
        .collect();
    for field in lists {
        profile.own.remove(&field);
        if let (Some(key), Some(default)) = (list_key(LIST_FIELDS, &field), defaults.get(&field)) {
            let entries = compare_list(key, default, fields.get(&field).unwrap_or(&Value::Null));
            profile.own_entries.insert(field, entries);
        }
    }
    profile.own.retain(|field| defaults.contains_key(field));
    profile
        .own_entries
        .retain(|field, _| list_key(LIST_FIELDS, field).is_some() && defaults.contains_key(field));
    for (field, held) in profile.own_entries.iter_mut() {
        if let (Some(key), Some(default)) = (list_key(LIST_FIELDS, field), defaults.get(field)) {
            tidy_list(
                key,
                default,
                fields.get(field).unwrap_or(&Value::Null),
                held,
            );
        }
    }
    profile.own_entries.retain(|_, held| !held.is_empty());
    let mut changed = (profile.own.clone(), profile.own_entries.clone()) != before;
    let effective = effective_fields(
        LIST_FIELDS,
        defaults,
        &fields,
        &profile.own,
        &profile.own_entries,
    );
    if effective != fields {
        match from_fields(effective) {
            Ok(values) => {
                profile.values = values;
                changed = true;
            }
            Err(e) => log::warn!("Profile {}: values not recomputed: {e}", profile.id),
        }
    }
    changed
}

/// The one-time switch from the profile "Standard" to the defaults (P1).
/// Runs while `default_values` is `None` (a store from before them, or a
/// new installation); returns whether it changed anything.
///
/// "Standard"'s values (its latest, the flat ones, while it was in use)
/// become the defaults and it leaves the profiles; applications assigned to
/// it, if any, are dropped. Every other profile keeps its values: those
/// equal to the defaults become coupled, the others its own, lists entry by
/// entry. A profile that differs in nothing stays, fully coupled, with its
/// applications (open question 3, recommendation a). Without "Standard"
/// (deleted, or a store from before profiles) the values in use become the
/// defaults. Other strands' migrations of profile values run before this
/// one (`settings::apply_settings_migrations` runs before
/// `profiles::ensure_profiles`).
pub fn migrate_to_defaults(settings: &mut AppSettings) -> bool {
    if settings.default_values.is_some() {
        return false;
    }
    let flat = ProfileValues::from_settings(settings);
    let in_use = settings.active_profile_id.clone();
    let standard = settings
        .profiles
        .iter()
        .position(|profile| profile.id == DEFAULT_PROFILE_ID)
        .map(|index| settings.profiles.remove(index));
    let defaults = match standard {
        Some(standard) => {
            if !standard.apps.is_empty() {
                log::info!(
                    "The defaults take no applications; dropped from \"Standard\": {}",
                    standard.apps.join(", ")
                );
            }
            if in_use == DEFAULT_PROFILE_ID {
                flat.clone()
            } else {
                standard.values
            }
        }
        None => flat.clone(),
    };
    for profile in settings.profiles.iter_mut() {
        if profile.id == in_use {
            // The flat fields hold its latest values.
            profile.values = flat.clone();
        }
        couple_equal(profile, &defaults);
    }
    if !settings.profiles.iter().any(|profile| profile.id == in_use) {
        settings.active_profile_id = DEFAULT_PROFILE_ID.to_string();
    }
    log::info!(
        "Profiles: the defaults hold what \"Standard\" held; own values per profile: {}",
        settings
            .profiles
            .iter()
            .map(|profile| format!("{} {}", profile.id, own_count(profile)))
            .collect::<Vec<_>>()
            .join(", ")
    );
    settings.default_values = Some(defaults);
    propagate(settings);
    true
}

/// Contract V5 for the defaults: a value the stored defaults lack (a field
/// added since they were stored) gets the flat value it had, as a stored
/// profile does (`profiles::fill_missing_values_from_flat`): every field
/// added so far was a setting for everything before. `raw` is the stored
/// settings as JSON. Returns whether one was missing.
pub fn fill_missing_defaults_from_flat(
    settings: &mut AppSettings,
    raw: &serde_json::Value,
) -> bool {
    let Some(stored) = raw
        .get("default_values")
        .and_then(|values| values.as_object())
    else {
        return false;
    };
    let flat = ProfileValues::from_settings(settings);
    match settings.default_values.as_mut() {
        Some(defaults) => profiles::fill_missing_values(defaults, stored, &flat),
        None => false,
    }
}

/// Couples the fields and entries of `profile` that equal `defaults` and
/// makes the others its own (the switch and an import, P1).
pub fn couple_equal(profile: &mut Profile, defaults: &ProfileValues) {
    let (own, own_entries) = compare_fields(
        LIST_FIELDS,
        &to_fields(defaults),
        &to_fields(&profile.values),
    );
    profile.own = own;
    profile.own_entries = own_entries;
}

/// Copies the defaults into every profile's coupled fields and entries and
/// the values in use into the flat fields. Returns whether anything changed.
pub fn propagate(settings: &mut AppSettings) -> bool {
    let defaults = defaults_fields(settings);
    let mut changed = false;
    for profile in settings.profiles.iter_mut() {
        if recompute(profile, &defaults) {
            changed = true;
        }
    }
    let in_use = in_use_values(settings);
    if ProfileValues::from_settings(settings) != in_use {
        in_use.apply_to(settings);
        changed = true;
    }
    changed
}

/// The old write paths: sends every flat field that differs from what is in
/// use where its value came from (rule in the module comment), then
/// [`propagate`]s. Returns whether anything changed.
pub fn route_flat(settings: &mut AppSettings) -> bool {
    let flat = ProfileValues::from_settings(settings);
    let in_use = settings.active_profile_id.clone();
    let routed = route(settings, &in_use, &flat);
    propagate(settings) || routed
}

/// [`route_flat`] for profile `id` (or the defaults) instead of what is in
/// use: `values` are its values after a change through an old write path.
pub fn route(settings: &mut AppSettings, id: &str, values: &ProfileValues) -> bool {
    if id == DEFAULT_PROFILE_ID {
        if settings.default_values.as_ref() == Some(values) {
            return false;
        }
        settings.default_values = Some(values.clone());
        propagate(settings);
        return true;
    }
    let Some(index) = settings
        .profiles
        .iter()
        .position(|profile| profile.id == id)
    else {
        log::warn!("A change for profile {id}, which doesn't exist, is dropped");
        return false;
    };
    let old = to_fields(&settings.profiles[index].values);
    let new = to_fields(values);
    if old == new {
        return false;
    }
    let mut defaults = defaults_fields(settings);
    let profile = &mut settings.profiles[index];
    let mut fields = old.clone();
    route_fields(
        LIST_FIELDS,
        &mut defaults,
        &mut fields,
        &profile.own,
        &mut profile.own_entries,
        &old,
        &new,
    );
    match (from_fields(defaults), from_fields(fields)) {
        (Ok(defaults), Ok(values)) => {
            settings.default_values = Some(defaults);
            settings.profiles[index].values = values;
        }
        (Err(e), _) | (_, Err(e)) => {
            log::warn!("A change for profile {id} is dropped: {e}");
            return false;
        }
    }
    propagate(settings);
    true
}

// ---- Commands with a target ----------------------------------------------

/// The index of profile `id`; the defaults are no profile.
fn index_of(settings: &AppSettings, id: &str) -> Result<usize, String> {
    if id == DEFAULT_PROFILE_ID {
        return Err("The defaults are no profile".to_string());
    }
    settings
        .profiles
        .iter()
        .position(|profile| profile.id == id)
        .ok_or_else(|| format!("Profile '{id}' not found"))
}

/// Runs `change` on a copy of `settings` and keeps it, propagated, only
/// when it succeeds.
fn commit<T>(
    settings: &mut AppSettings,
    change: impl FnOnce(&mut AppSettings) -> Result<T, String>,
) -> Result<T, String> {
    let mut next = settings.clone();
    let out = change(&mut next)?;
    propagate(&mut next);
    *settings = next;
    Ok(out)
}

/// Checks and normalizes `field` of `values` after a change (`entry`: the
/// one entry that changed), as the field's own change command does. Other
/// strands add their fields' checks here.
fn validate(values: &mut ProfileValues, field: &str, entry: Option<&str>) -> Result<(), String> {
    // The providers are profile values too: the ones of the changed layer.
    let providers: Vec<String> = values
        .post_process_providers
        .iter()
        .map(|provider| provider.id.clone())
        .collect();
    let provider = |id: &str| {
        if providers.iter().any(|known| known == id) {
            Ok(())
        } else {
            Err(format!("Provider '{id}' not found"))
        }
    };
    match field {
        "post_process_provider_id" => provider(&values.post_process_provider_id),
        "post_process_models" | "post_process_efforts" | "post_process_api_keys" => {
            let keys: Vec<String> = match field {
                "post_process_models" => values.post_process_models.keys().cloned().collect(),
                "post_process_efforts" => values.post_process_efforts.keys().cloned().collect(),
                _ => values.post_process_api_keys.keys().cloned().collect(),
            };
            match entry {
                Some(id) => provider(id),
                None => keys.iter().try_for_each(|id| provider(id)),
            }
        }
        "language_detection_window_secs" => {
            let (min, max) = settings::LANGUAGE_DETECTION_WINDOW_SECS.into_inner();
            values.language_detection_window_secs =
                values.language_detection_window_secs.clamp(min, max);
            Ok(())
        }
        "active_word_list_id" => match &values.active_word_list_id {
            Some(id) if !values.word_lists.iter().any(|list| &list.id == id) => {
                Err(format!("Word list '{id}' not found"))
            }
            _ => Ok(()),
        },
        "append_text" => {
            values.append_text = crate::append_text::localize(
                &values.append_text,
                crate::append_text::KeyPlatform::current(),
            );
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Sets `field` in `scope` to `value`. In a profile this makes it the
/// profile's own (P7.3); a list's changed and added entries become its own,
/// removed entries of the defaults are switched off.
pub fn set_value(
    settings: &mut AppSettings,
    scope: &Scope,
    field: &str,
    value: Value,
) -> Result<(), String> {
    let key = check_field(field)?;
    commit(settings, |s| match scope {
        Scope::Defaults => {
            let mut fields = defaults_fields(s);
            fields.insert(field.to_string(), value);
            let mut values = from_fields(fields)?;
            validate(&mut values, field, None)?;
            s.default_values = Some(values);
            Ok(())
        }
        Scope::Profile { id } => {
            let index = index_of(s, id)?;
            let defaults = defaults_fields(s);
            let mut fields = to_fields(&s.profiles[index].values);
            let old = fields.get(field).cloned();
            fields.insert(field.to_string(), value);
            let mut values = from_fields(fields)?;
            validate(&mut values, field, None)?;
            let new = to_fields(&values).get(field).cloned();
            let profile = &mut s.profiles[index];
            match key {
                None => {
                    profile.own.insert(field.to_string());
                }
                Some(key) => {
                    let olds = entries(key, old.as_ref());
                    let news = entries(key, new.as_ref());
                    let default_list = entries(key, defaults.get(field));
                    let held = profile.own_entries.entry(field.to_string()).or_default();
                    for (id, entry) in &news {
                        if find(&olds, id) != Some(entry) {
                            held.own.insert(id.clone());
                            held.off.remove(id);
                        }
                    }
                    for (id, _) in &olds {
                        if find(&news, id).is_none() {
                            held.own.remove(id);
                            if find(&default_list, id).is_some() {
                                held.off.insert(id.clone());
                            }
                        }
                    }
                }
            }
            profile.values = values;
            Ok(())
        }
    })
}

/// Holds `field` in profile `id` as its own without changing it (P4); a
/// list's entries all become its own, the defaults' missing ones off.
pub fn hold_value(settings: &mut AppSettings, id: &str, field: &str) -> Result<(), String> {
    let key = check_field(field)?;
    commit(settings, |s| {
        let index = index_of(s, id)?;
        let defaults = defaults_fields(s);
        let profile = &mut s.profiles[index];
        match key {
            None => {
                profile.own.insert(field.to_string());
            }
            Some(key) => {
                let fields = to_fields(&profile.values);
                let mine = entries(key, fields.get(field));
                let default_list = entries(key, defaults.get(field));
                profile.own_entries.insert(
                    field.to_string(),
                    OwnEntries {
                        own: mine.iter().map(|(id, _)| id.clone()).collect(),
                        off: default_list
                            .iter()
                            .filter(|(id, _)| find(&mine, id).is_none())
                            .map(|(id, _)| id.clone())
                            .collect(),
                    },
                );
            }
        }
        Ok(())
    })
}

/// Couples `fields` of profile `id` to the defaults again (P8: one value,
/// or the values of a page), entries of lists included.
pub fn reset_values(settings: &mut AppSettings, id: &str, fields: &[String]) -> Result<(), String> {
    for field in fields {
        check_field(field)?;
    }
    commit(settings, |s| {
        let index = index_of(s, id)?;
        let profile = &mut s.profiles[index];
        for field in fields {
            profile.own.remove(field);
            profile.own_entries.remove(field);
        }
        Ok(())
    })
}

/// Couples every value of profile `id` to the defaults again (P8); its name
/// and applications stay.
pub fn reset_profile(settings: &mut AppSettings, id: &str) -> Result<(), String> {
    commit(settings, |s| {
        let index = index_of(s, id)?;
        let profile = &mut s.profiles[index];
        profile.own.clear();
        profile.own_entries.clear();
        Ok(())
    })
}

/// "Change the default instead" (P7.3): the default takes profile `id`'s
/// value of `field`, and the profile is coupled to it again.
pub fn adopt_as_default(settings: &mut AppSettings, id: &str, field: &str) -> Result<(), String> {
    check_field(field)?;
    commit(settings, |s| {
        let index = index_of(s, id)?;
        let value = to_fields(&s.profiles[index].values)
            .get(field)
            .cloned()
            .unwrap_or(Value::Null);
        let mut defaults = defaults_fields(s);
        defaults.insert(field.to_string(), value);
        s.default_values = Some(from_fields(defaults)?);
        let profile = &mut s.profiles[index];
        profile.own.remove(field);
        profile.own_entries.remove(field);
        Ok(())
    })
}

/// The list field `field`, or an error for a plain value.
fn list_field(field: &str) -> Result<EntryKey, String> {
    check_field(field)?.ok_or_else(|| format!("'{field}' has no entries"))
}

/// Sets entry `entry_id` of list `field` in `scope` to `entry`, adding it
/// when there is none; in a profile the entry becomes its own.
pub fn set_entry(
    settings: &mut AppSettings,
    scope: &Scope,
    field: &str,
    entry_id: &str,
    entry: Value,
) -> Result<(), String> {
    let key = list_field(field)?;
    let mut entry = entry;
    if let EntryKey::Property(name) = key {
        let object = entry
            .as_object_mut()
            .ok_or_else(|| format!("An entry of '{field}' is an object"))?;
        match object.get(name) {
            None => {
                object.insert(name.to_string(), Value::String(entry_id.to_string()));
            }
            Some(Value::String(id)) if id == entry_id => {}
            Some(_) => return Err(format!("The entry's {name} isn't '{entry_id}'")),
        }
    }
    let with_entry = |fields: &mut Fields| {
        let mut list = entries(key, fields.get(field));
        upsert(&mut list, entry_id, &entry);
        fields.insert(field.to_string(), assemble(key, list));
    };
    commit(settings, |s| match scope {
        Scope::Defaults => {
            let mut fields = defaults_fields(s);
            with_entry(&mut fields);
            let mut values = from_fields(fields)?;
            validate(&mut values, field, Some(entry_id))?;
            s.default_values = Some(values);
            Ok(())
        }
        Scope::Profile { id } => {
            let index = index_of(s, id)?;
            let mut fields = to_fields(&s.profiles[index].values);
            with_entry(&mut fields);
            let mut values = from_fields(fields)?;
            validate(&mut values, field, Some(entry_id))?;
            let profile = &mut s.profiles[index];
            profile.values = values;
            let held = profile.own_entries.entry(field.to_string()).or_default();
            held.own.insert(entry_id.to_string());
            held.off.remove(entry_id);
            Ok(())
        }
    })
}

/// Removes entry `entry_id` of list `field` in `scope`: from the defaults
/// (and every profile that follows them in it), or from a profile, where
/// one of the defaults' entries is switched off.
pub fn remove_entry(
    settings: &mut AppSettings,
    scope: &Scope,
    field: &str,
    entry_id: &str,
) -> Result<(), String> {
    let key = list_field(field)?;
    let without = |fields: &mut Fields| -> Result<(), String> {
        let mut list = entries(key, fields.get(field));
        if find(&list, entry_id).is_none() {
            return Err(format!("No entry '{entry_id}' in '{field}'"));
        }
        list.retain(|(id, _)| id != entry_id);
        fields.insert(field.to_string(), assemble(key, list));
        Ok(())
    };
    commit(settings, |s| match scope {
        Scope::Defaults => {
            let mut fields = defaults_fields(s);
            without(&mut fields)?;
            s.default_values = Some(from_fields(fields)?);
            Ok(())
        }
        Scope::Profile { id } => {
            let index = index_of(s, id)?;
            let in_defaults =
                find(&entries(key, defaults_fields(s).get(field)), entry_id).is_some();
            let mut fields = to_fields(&s.profiles[index].values);
            without(&mut fields)?;
            let profile = &mut s.profiles[index];
            profile.values = from_fields(fields)?;
            let held = profile.own_entries.entry(field.to_string()).or_default();
            held.own.remove(entry_id);
            if in_defaults {
                held.off.insert(entry_id.to_string());
            }
            Ok(())
        }
    })
}

/// Couples entry `entry_id` of list `field` in profile `id` to the defaults
/// again: a changed or switched-off entry follows them, an added one goes.
pub fn reset_entry(
    settings: &mut AppSettings,
    id: &str,
    field: &str,
    entry_id: &str,
) -> Result<(), String> {
    list_field(field)?;
    commit(settings, |s| {
        let index = index_of(s, id)?;
        if let Some(held) = s.profiles[index].own_entries.get_mut(field) {
            held.own.remove(entry_id);
            held.off.remove(entry_id);
        }
        Ok(())
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use serde_json::json;

    use crate::profiles::{ensure_profiles, DEFAULT_PROFILE_ID};
    use crate::settings::LLMPrompt;
    use crate::word_lists::WordList;

    const WHISPER: &str = "whisper-large-v3-turbo";
    const CANARY: &str = "canary-1b-v2";

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn list(id: &str, words: &[&str]) -> WordList {
        WordList {
            id: id.to_string(),
            name: id.to_uppercase(),
            words: strings(words),
        }
    }

    /// The user's two prompts, with the dictation prompt's text per profile.
    fn prompts(dictation: &str) -> Vec<LLMPrompt> {
        vec![
            LLMPrompt {
                id: "improve".to_string(),
                name: "Improve Transcriptions".to_string(),
                prompt: "Improve it.".to_string(),
            },
            LLMPrompt {
                id: "diktat_de".to_string(),
                name: "Diktat (DE/EN)".to_string(),
                prompt: dictation.to_string(),
            },
        ]
    }

    // The user's constellation before the switch, as the sidecar note
    // `2026-11-19_1711_grundwerte-und-profile.md` measured it and
    // `qa/skripte/2026-11-19_1921_kopplungsmodell-umsetzung_fixture.py`
    // read it entry by entry from the release's settings (no secrets).

    fn standard_values() -> ProfileValues {
        ProfileValues {
            selected_model: WHISPER.to_string(),
            selected_language: "de".to_string(),
            post_process_enabled: true,
            post_process_provider_id: "anthropic".to_string(),
            post_process_models: map(&[
                ("anthropic", "claude-haiku-5-5"),
                ("custom", ""),
                ("openai", ""),
            ]),
            post_process_efforts: HashMap::new(),
            post_process_prompts: prompts("Dictation."),
            post_process_selected_prompt_id: Some("diktat_de".to_string()),
            word_lists: vec![list("names", &["Sayuri"])],
            active_word_list_id: Some("names".to_string()),
            append_text: String::new(),
            send_context: true,
            ..ProfileValues::default()
        }
    }

    /// Privat: only its own prompt texts.
    fn privat_values() -> ProfileValues {
        ProfileValues {
            post_process_prompts: prompts("Dictation, privately."),
            ..standard_values()
        }
    }

    /// Arbeit: Canary, Sonnet at xhigh, two line breaks appended, own texts.
    fn arbeit_values() -> ProfileValues {
        let mut values = ProfileValues {
            selected_model: CANARY.to_string(),
            post_process_efforts: map(&[("anthropic", "xhigh")]),
            append_text: r"\n\n".to_string(),
            post_process_prompts: prompts("Dictation at work."),
            ..standard_values()
        };
        values
            .post_process_models
            .insert("anthropic".to_string(), "claude-sonnet-5-5".to_string());
        values
    }

    /// Playground: Canary, the custom provider with a model of its own,
    /// Sonnet at xhigh, own texts.
    fn playground_values() -> ProfileValues {
        let mut values = ProfileValues {
            selected_model: CANARY.to_string(),
            post_process_provider_id: "custom".to_string(),
            post_process_efforts: map(&[("anthropic", "xhigh")]),
            post_process_prompts: prompts("Dictation, playing."),
            ..standard_values()
        };
        values
            .post_process_models
            .insert("anthropic".to_string(), "claude-sonnet-5-5".to_string());
        values
            .post_process_models
            .insert("custom".to_string(), "gemma4-12b-qat".to_string());
        values
    }

    /// The store as the release has it: "Standard" and three profiles, the
    /// flat fields holding Playground's values, which is in use.
    fn users_store() -> AppSettings {
        let mut s = AppSettings {
            app_language: "de".to_string(),
            ..Default::default()
        };
        playground_values().apply_to(&mut s);
        s.active_profile_id = "playground".to_string();
        let mut privat = Profile::new("privat", "Privat", privat_values());
        privat.apps = strings(&["com.apple.mail", "com.apple.Notes", "com.apple.TextEdit"]);
        let mut arbeit = Profile::new("arbeit", "Arbeit", arbeit_values());
        arbeit.apps = strings(&["com.microsoft.VSCode", "com.googlecode.iterm2"]);
        s.profiles = vec![
            Profile::new(DEFAULT_PROFILE_ID, "", standard_values()),
            privat,
            arbeit,
            Profile::new("playground", "Playground", playground_values()),
        ];
        s
    }

    /// The user's store after the switch.
    fn migrated() -> AppSettings {
        let mut s = users_store();
        ensure_profiles(&mut s);
        s
    }

    fn profile<'a>(s: &'a AppSettings, id: &str) -> &'a Profile {
        s.profiles
            .iter()
            .find(|profile| profile.id == id)
            .unwrap_or_else(|| panic!("no profile {id}"))
    }

    fn entries_of<'a>(profile: &'a Profile, field: &str) -> Option<&'a OwnEntries> {
        profile.own_entries.get(field)
    }

    fn defaults(s: &AppSettings) -> ProfileValues {
        s.default_values.clone().expect("defaults")
    }

    fn profile_scope(id: &str) -> Scope {
        Scope::Profile { id: id.to_string() }
    }

    // ---- The fields ----------------------------------------------------

    #[test]
    fn every_field_of_profile_values_is_known_by_its_serde_name() {
        let names = field_names();
        let json = serde_json::to_value(ProfileValues::default()).unwrap();
        let expected: Vec<String> = json.as_object().unwrap().keys().cloned().collect();
        let mut sorted = names.clone();
        sorted.sort();
        let mut expected_sorted = expected.clone();
        expected_sorted.sort();
        assert_eq!(sorted, expected_sorted);
        assert!(names.contains(&"selected_model".to_string()));
        assert!(names.contains(&"append_text".to_string()));
    }

    #[test]
    fn the_fields_say_which_deviate_entry_by_entry() {
        let kinds: HashMap<String, FieldKind> = fields()
            .into_iter()
            .map(|field| (field.name, field.kind))
            .collect();
        assert_eq!(kinds.len(), field_names().len());
        assert_eq!(
            kinds.get("word_lists"),
            Some(&FieldKind::List {
                key: "id".to_string()
            })
        );
        assert_eq!(kinds.get("post_process_models"), Some(&FieldKind::Map));
        assert_eq!(kinds.get("post_process_efforts"), Some(&FieldKind::Map));
        assert_eq!(kinds.get("selected_model"), Some(&FieldKind::Value));
        assert_eq!(kinds.get("post_process_prompts"), Some(&FieldKind::Value));
    }

    // ---- The engine works on any fields (test-only registry) ------------

    /// A registry and values no `ProfileValues` has: the engine needs no
    /// code per field, so a field another strand adds couples as these do.
    const TEST_LISTS: &[ListField] = &[
        ListField {
            field: "keys",
            key: EntryKey::MapKey,
        },
        ListField {
            field: "items",
            key: EntryKey::Property("name"),
        },
    ];

    fn fields_of(value: serde_json::Value) -> Fields {
        value.as_object().expect("an object").clone()
    }

    #[test]
    fn a_map_merges_entry_by_entry() {
        let defaults = json!({ "a": 1, "b": 2, "c": 3 });
        let profile = json!({ "a": 10, "b": 99, "x": 5 });
        let own = OwnEntries {
            own: set(&["a", "x"]),
            off: set(&["c"]),
        };
        assert_eq!(
            merge_list(EntryKey::MapKey, &defaults, &profile, &own),
            json!({ "a": 10, "b": 2, "x": 5 })
        );
    }

    #[test]
    fn a_list_merges_in_the_defaults_order_then_the_added_entries() {
        let defaults = json!([
            { "name": "one", "v": 1 },
            { "name": "two", "v": 2 },
            { "name": "three", "v": 3 }
        ]);
        let profile = json!([
            { "name": "mine", "v": 0 },
            { "name": "two", "v": 22 },
            { "name": "one", "v": 1 }
        ]);
        let own = OwnEntries {
            own: set(&["two", "mine"]),
            off: set(&["three"]),
        };
        assert_eq!(
            merge_list(EntryKey::Property("name"), &defaults, &profile, &own),
            json!([
                { "name": "one", "v": 1 },
                { "name": "two", "v": 22 },
                { "name": "mine", "v": 0 }
            ])
        );
    }

    #[test]
    fn comparing_a_list_finds_changed_added_and_missing_entries() {
        let defaults = json!({ "a": 1, "b": 2, "c": 3 });
        let profile = json!({ "a": 1, "b": 20, "x": 5 });
        assert_eq!(
            compare_list(EntryKey::MapKey, &defaults, &profile),
            OwnEntries {
                own: set(&["b", "x"]),
                off: set(&["c"]),
            }
        );
    }

    #[test]
    fn any_field_couples_without_code_of_its_own() {
        let defaults = fields_of(json!({
            "color": "red",
            "size": 3,
            "keys": { "a": "x", "b": "y" },
            "items": [ { "name": "one", "v": 1 } ]
        }));
        let profile = fields_of(json!({
            "color": "blue",
            "size": 3,
            "keys": { "a": "x", "b": "z" },
            "items": [ { "name": "one", "v": 1 }, { "name": "two", "v": 2 } ]
        }));
        let (own, own_entries) = compare_fields(TEST_LISTS, &defaults, &profile);
        assert_eq!(own, set(&["color"]));
        assert_eq!(
            own_entries.get("keys").map(|e| e.own.clone()),
            Some(set(&["b"]))
        );
        assert_eq!(
            own_entries.get("items").map(|e| e.own.clone()),
            Some(set(&["two"]))
        );
        // The defaults change: coupled fields and entries follow, own ones stay.
        let changed = fields_of(json!({
            "color": "green",
            "size": 4,
            "keys": { "a": "q", "b": "y", "c": "new" },
            "items": [ { "name": "one", "v": 100 } ]
        }));
        assert_eq!(
            effective_fields(TEST_LISTS, &changed, &profile, &own, &own_entries),
            fields_of(json!({
                "color": "blue",
                "size": 4,
                "keys": { "a": "q", "b": "z", "c": "new" },
                "items": [ { "name": "one", "v": 100 }, { "name": "two", "v": 2 } ]
            }))
        );
    }

    // ---- The switch (P1) -------------------------------------------------

    #[test]
    fn standard_becomes_the_defaults_and_leaves_the_profiles() {
        let s = migrated();
        assert_eq!(s.default_values, Some(standard_values()));
        assert_eq!(
            s.profiles.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec!["privat", "arbeit", "playground"]
        );
        // What is in use stays in use, with the same values.
        assert_eq!(s.active_profile_id, "playground");
        assert_eq!(ProfileValues::from_settings(&s), playground_values());
        // Every profile keeps its values, name and applications.
        assert_eq!(profile(&s, "privat").values, privat_values());
        assert_eq!(profile(&s, "arbeit").values, arbeit_values());
        assert_eq!(profile(&s, "playground").values, playground_values());
        assert_eq!(profile(&s, "privat").apps.len(), 3);
        assert_eq!(profile(&s, "arbeit").apps.len(), 2);
        assert_eq!(profile(&s, "arbeit").name, "Arbeit");
    }

    #[test]
    fn the_users_profiles_keep_only_what_differs_as_their_own() {
        let s = migrated();
        let privat = profile(&s, "privat");
        assert_eq!(privat.own, set(&["post_process_prompts"]));
        assert!(privat.own_entries.values().all(OwnEntries::is_empty));
        assert_eq!(own_count(privat), 1);

        let arbeit = profile(&s, "arbeit");
        assert_eq!(
            arbeit.own,
            set(&["append_text", "post_process_prompts", "selected_model"])
        );
        assert_eq!(
            entries_of(arbeit, "post_process_models").map(|e| e.own.clone()),
            Some(set(&["anthropic"]))
        );
        assert_eq!(
            entries_of(arbeit, "post_process_efforts").map(|e| e.own.clone()),
            Some(set(&["anthropic"]))
        );
        assert!(entries_of(arbeit, "word_lists").is_none_or(OwnEntries::is_empty));
        assert_eq!(own_count(arbeit), 5);

        // The note's table counted the model map as one value (5); entry by
        // entry Playground's model for its custom provider is its own too.
        let playground = profile(&s, "playground");
        assert_eq!(
            playground.own,
            set(&[
                "post_process_prompts",
                "post_process_provider_id",
                "selected_model"
            ])
        );
        assert_eq!(
            entries_of(playground, "post_process_models").map(|e| e.own.clone()),
            Some(set(&["anthropic", "custom"]))
        );
        assert_eq!(own_count(playground), 6);
    }

    #[test]
    fn the_switch_runs_once() {
        let mut s = users_store();
        assert!(ensure_profiles(&mut s));
        let once = s.clone();
        assert!(!ensure_profiles(&mut s));
        assert_eq!(s.profiles, once.profiles);
        assert_eq!(s.default_values, once.default_values);
        assert_eq!(s.active_profile_id, once.active_profile_id);
    }

    #[test]
    fn standard_in_use_becomes_the_defaults_in_use_with_its_latest_values() {
        let mut s = users_store();
        standard_values().apply_to(&mut s);
        s.active_profile_id = DEFAULT_PROFILE_ID.to_string();
        // A change not yet copied into "Standard" is its latest value.
        s.selected_language = "en".to_string();
        ensure_profiles(&mut s);
        assert_eq!(s.active_profile_id, DEFAULT_PROFILE_ID);
        assert_eq!(defaults(&s).selected_language, "en");
        assert_eq!(s.selected_language, "en");
        // Privat had "de" as its own copy, which the change didn't reach
        // before the switch; it keeps it as its own value.
        assert_eq!(profile(&s, "privat").values.selected_language, "de");
        assert!(profile(&s, "privat").own.contains("selected_language"));
    }

    #[test]
    fn a_profile_that_differs_in_nothing_stays_fully_coupled_with_its_apps() {
        let mut s = users_store();
        s.profiles[1].values = standard_values();
        ensure_profiles(&mut s);
        let privat = profile(&s, "privat");
        assert!(privat.own.is_empty());
        assert_eq!(own_count(privat), 0);
        assert_eq!(privat.apps.len(), 3);
    }

    #[test]
    fn applications_assigned_to_standard_are_dropped() {
        let mut s = users_store();
        s.profiles[0].apps = strings(&["com.apple.Safari"]);
        ensure_profiles(&mut s);
        assert!(s
            .profiles
            .iter()
            .all(|profile| !profile.apps.contains(&"com.apple.Safari".to_string())));
    }

    #[test]
    fn word_lists_couple_entry_by_entry_in_the_switch() {
        let mut s = users_store();
        s.profiles[0].values.word_lists = vec![list("names", &["Sayuri"]), list("code", &["Rust"])];
        // Privat keeps "names", changes "code"'s words, lacks nothing; Arbeit
        // lacks "code" and adds "med".
        s.profiles[1].values.word_lists =
            vec![list("names", &["Sayuri"]), list("code", &["Rust", "Tauri"])];
        s.profiles[2].values.word_lists =
            vec![list("names", &["Sayuri"]), list("med", &["Aspirin"])];
        ensure_profiles(&mut s);
        assert_eq!(
            entries_of(profile(&s, "privat"), "word_lists").cloned(),
            Some(OwnEntries {
                own: set(&["code"]),
                off: BTreeSet::new(),
            })
        );
        assert_eq!(
            entries_of(profile(&s, "arbeit"), "word_lists").cloned(),
            Some(OwnEntries {
                own: set(&["med"]),
                off: set(&["code"]),
            })
        );
        assert!(entries_of(profile(&s, "playground"), "word_lists").is_some());
    }

    #[test]
    fn a_new_installation_gets_the_defaults_and_no_profiles() {
        let mut s = AppSettings::default();
        assert!(ensure_profiles(&mut s));
        assert!(s.profiles.is_empty());
        assert_eq!(s.active_profile_id, DEFAULT_PROFILE_ID);
        assert_eq!(
            s.default_values,
            Some(ProfileValues::from_settings(&AppSettings::default()))
        );
    }

    #[test]
    fn a_store_whose_standard_was_deleted_takes_the_values_in_use_as_defaults() {
        let mut s = users_store();
        s.profiles.remove(0);
        ensure_profiles(&mut s);
        assert_eq!(s.default_values, Some(playground_values()));
        assert_eq!(s.active_profile_id, "playground");
        assert!(profile(&s, "playground").own.is_empty());
        assert!(profile(&s, "arbeit").own.contains("post_process_prompts"));
    }

    // ---- Values added since the defaults were stored (V5) ----------------

    #[test]
    fn a_value_the_stored_defaults_lack_takes_the_flat_one() {
        let mut s = migrated();
        s.selected_microphone = Some("USB Mic".to_string());
        let mut raw = serde_json::to_value(&s).unwrap();
        raw["default_values"]
            .as_object_mut()
            .unwrap()
            .remove("selected_microphone");
        let mut read: AppSettings = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(defaults(&read).selected_microphone, None);
        assert!(fill_missing_defaults_from_flat(&mut read, &raw));
        assert_eq!(
            defaults(&read).selected_microphone.as_deref(),
            Some("USB Mic")
        );
        // Stored with every value: nothing to fill.
        let raw = serde_json::to_value(&read).unwrap();
        assert!(!fill_missing_defaults_from_flat(&mut read, &raw));
    }

    #[test]
    fn a_repair_of_the_flat_values_reaches_the_defaults_not_in_use() {
        let mut s = migrated();
        let repaired = crate::profiles::ensure_in_every_profile(&mut s, |flat| {
            flat.post_process_timeout_secs = 77;
            true
        });
        assert!(repaired);
        assert_eq!(defaults(&s).post_process_timeout_secs, 77);
        // With the defaults in use the flat values are theirs: repaired there.
        crate::profiles::switch_to(&mut s, DEFAULT_PROFILE_ID, false).unwrap();
        let mut d = defaults(&s);
        d.post_process_timeout_secs = 5;
        s.default_values = Some(d);
        crate::profiles::ensure_in_every_profile(&mut s, |flat| {
            flat.post_process_timeout_secs = 88;
            true
        });
        assert_eq!(defaults(&s).post_process_timeout_secs, 5);
    }

    // ---- Coupled and own values ----------------------------------------

    #[test]
    fn a_changed_default_reaches_the_coupled_values_not_the_own_ones() {
        let mut s = migrated();
        set_value(&mut s, &Scope::Defaults, "selected_language", json!("en")).unwrap();
        set_value(
            &mut s,
            &Scope::Defaults,
            "selected_model",
            json!("parakeet"),
        )
        .unwrap();
        for id in ["privat", "arbeit", "playground"] {
            assert_eq!(profile(&s, id).values.selected_language, "en");
        }
        assert_eq!(profile(&s, "privat").values.selected_model, "parakeet");
        assert_eq!(profile(&s, "arbeit").values.selected_model, CANARY);
        // Playground is in use: the flat fields follow.
        assert_eq!(s.selected_language, "en");
        assert_eq!(s.selected_model, CANARY);
    }

    #[test]
    fn an_own_value_that_happens_to_be_equal_stays_when_the_default_changes() {
        let mut s = migrated();
        hold_value(&mut s, "privat", "append_text").unwrap();
        assert!(profile(&s, "privat").own.contains("append_text"));
        assert_eq!(profile(&s, "privat").values.append_text, "");
        set_value(&mut s, &Scope::Defaults, "append_text", json!(r"\s")).unwrap();
        assert_eq!(profile(&s, "privat").values.append_text, "");
        assert_eq!(profile(&s, "playground").values.append_text, r"\s");
    }

    #[test]
    fn a_map_entry_of_the_defaults_reaches_the_profiles_that_follow_it() {
        let mut s = migrated();
        set_entry(
            &mut s,
            &Scope::Defaults,
            "post_process_models",
            "anthropic",
            json!("claude-haiku-6"),
        )
        .unwrap();
        set_entry(
            &mut s,
            &Scope::Defaults,
            "post_process_models",
            "openai",
            json!("gpt-5"),
        )
        .unwrap();
        let models = |id: &str| profile(&s, id).values.post_process_models.clone();
        assert_eq!(models("privat")["anthropic"], "claude-haiku-6");
        assert_eq!(models("arbeit")["anthropic"], "claude-sonnet-5-5");
        assert_eq!(models("arbeit")["openai"], "gpt-5");
        assert_eq!(models("playground")["custom"], "gemma4-12b-qat");
        assert_eq!(s.post_process_models["openai"], "gpt-5");
    }

    #[test]
    fn setting_a_value_in_a_profile_makes_it_its_own() {
        let mut s = migrated();
        set_value(
            &mut s,
            &profile_scope("privat"),
            "selected_language",
            json!("fr"),
        )
        .unwrap();
        assert!(profile(&s, "privat").own.contains("selected_language"));
        assert_eq!(profile(&s, "privat").values.selected_language, "fr");
        assert_eq!(defaults(&s).selected_language, "de");
        assert_eq!(profile(&s, "arbeit").values.selected_language, "de");
        // In the profile in use, the flat fields follow at once.
        set_value(
            &mut s,
            &profile_scope("playground"),
            "translate_to_english",
            json!(true),
        )
        .unwrap();
        assert!(s.translate_to_english);
        assert!(!defaults(&s).translate_to_english);
    }

    #[test]
    fn setting_a_whole_list_in_a_profile_owns_what_changed() {
        let mut s = migrated();
        set_value(
            &mut s,
            &Scope::Defaults,
            "word_lists",
            json!([list("names", &["Sayuri"]), list("code", &["Rust"])]),
        )
        .unwrap();
        set_value(
            &mut s,
            &profile_scope("privat"),
            "word_lists",
            json!([
                list("names", &["Sayuri", "Jakob"]),
                list("med", &["Aspirin"])
            ]),
        )
        .unwrap();
        assert_eq!(
            entries_of(profile(&s, "privat"), "word_lists").cloned(),
            Some(OwnEntries {
                own: set(&["med", "names"]),
                off: set(&["code"]),
            })
        );
        assert_eq!(defaults(&s).word_lists.len(), 2);
        assert_eq!(profile(&s, "arbeit").values.word_lists.len(), 2);
    }

    #[test]
    fn a_wrong_field_type_or_profile_is_refused_and_changes_nothing() {
        let mut s = migrated();
        let before = s.clone();
        assert!(set_value(&mut s, &Scope::Defaults, "no_such_field", json!(1)).is_err());
        assert!(set_value(&mut s, &Scope::Defaults, "selected_language", json!(5)).is_err());
        assert!(set_value(
            &mut s,
            &profile_scope("gone"),
            "selected_language",
            json!("en")
        )
        .is_err());
        assert!(set_value(
            &mut s,
            &profile_scope(DEFAULT_PROFILE_ID),
            "selected_language",
            json!("en")
        )
        .is_err());
        assert!(hold_value(&mut s, "privat", "no_such_field").is_err());
        assert!(reset_values(&mut s, "privat", &strings(&["no_such_field"])).is_err());
        assert_eq!(s.profiles, before.profiles);
        assert_eq!(s.default_values, before.default_values);
    }

    #[test]
    fn values_are_validated_like_their_change_commands_validate_them() {
        let mut s = migrated();
        assert!(set_value(
            &mut s,
            &Scope::Defaults,
            "post_process_provider_id",
            json!("nope")
        )
        .is_err());
        assert!(set_entry(
            &mut s,
            &Scope::Defaults,
            "post_process_models",
            "nope",
            json!("m")
        )
        .is_err());
        assert!(set_value(
            &mut s,
            &Scope::Defaults,
            "active_word_list_id",
            json!("gone")
        )
        .is_err());
        set_value(&mut s, &Scope::Defaults, "active_word_list_id", json!(null)).unwrap();
        set_value(
            &mut s,
            &Scope::Defaults,
            "language_detection_window_secs",
            json!(999),
        )
        .unwrap();
        let (_, max) = crate::settings::LANGUAGE_DETECTION_WINDOW_SECS.into_inner();
        assert_eq!(defaults(&s).language_detection_window_secs, max);
        set_value(
            &mut s,
            &Scope::Defaults,
            "append_text",
            json!("{Win+Enter}"),
        )
        .unwrap();
        assert_eq!(
            defaults(&s).append_text,
            crate::append_text::localize("{Win+Enter}", crate::append_text::KeyPlatform::current())
        );
    }

    // ---- Resetting (P8) and "change the default instead" (P7.3) ----------

    #[test]
    fn resetting_values_couples_them_again() {
        let mut s = migrated();
        reset_values(
            &mut s,
            "arbeit",
            &strings(&["selected_model", "post_process_models"]),
        )
        .unwrap();
        let arbeit = profile(&s, "arbeit");
        assert_eq!(arbeit.values.selected_model, WHISPER);
        assert_eq!(
            arbeit.values.post_process_models["anthropic"],
            "claude-haiku-5-5"
        );
        assert!(!arbeit.own.contains("selected_model"));
        assert!(entries_of(arbeit, "post_process_models").is_none_or(OwnEntries::is_empty));
        assert_eq!(own_count(arbeit), 3);
        assert!(reset_values(&mut s, DEFAULT_PROFILE_ID, &strings(&["selected_model"])).is_err());
    }

    #[test]
    fn resetting_a_whole_profile_keeps_its_name_and_applications() {
        let mut s = migrated();
        reset_profile(&mut s, "arbeit").unwrap();
        let arbeit = profile(&s, "arbeit");
        assert_eq!(own_count(arbeit), 0);
        assert_eq!(arbeit.values, standard_values());
        assert_eq!(arbeit.name, "Arbeit");
        assert_eq!(arbeit.apps.len(), 2);
        // The profile in use: its values go into the flat fields.
        reset_profile(&mut s, "playground").unwrap();
        assert_eq!(ProfileValues::from_settings(&s), standard_values());
        assert!(reset_profile(&mut s, "gone").is_err());
    }

    #[test]
    fn the_default_can_take_a_profiles_value_instead() {
        let mut s = migrated();
        adopt_as_default(&mut s, "arbeit", "append_text").unwrap();
        assert_eq!(defaults(&s).append_text, r"\n\n");
        assert!(!profile(&s, "arbeit").own.contains("append_text"));
        assert_eq!(profile(&s, "privat").values.append_text, r"\n\n");
        assert_eq!(s.append_text, r"\n\n");
        // A list: the profile's whole list becomes the defaults'.
        adopt_as_default(&mut s, "arbeit", "post_process_models").unwrap();
        assert_eq!(
            defaults(&s).post_process_models["anthropic"],
            "claude-sonnet-5-5"
        );
        assert!(entries_of(profile(&s, "arbeit"), "post_process_models")
            .is_none_or(OwnEntries::is_empty));
        assert_eq!(
            profile(&s, "privat").values.post_process_models["anthropic"],
            "claude-sonnet-5-5"
        );
        assert_eq!(
            profile(&s, "playground").values.post_process_models["custom"],
            "gemma4-12b-qat"
        );
    }

    // ---- Entries (P10) ---------------------------------------------------

    #[test]
    fn a_profile_changes_adds_and_switches_off_entries() {
        let mut s = migrated();
        set_entry(
            &mut s,
            &profile_scope("privat"),
            "word_lists",
            "names",
            json!(list("names", &["Jakob"])),
        )
        .unwrap();
        set_entry(
            &mut s,
            &profile_scope("privat"),
            "word_lists",
            "mine",
            json!(list("mine", &["Bethli"])),
        )
        .unwrap();
        let privat = profile(&s, "privat");
        assert_eq!(
            privat.values.word_lists,
            vec![list("names", &["Jakob"]), list("mine", &["Bethli"])]
        );
        assert_eq!(
            entries_of(privat, "word_lists").map(|e| e.own.clone()),
            Some(set(&["mine", "names"]))
        );
        assert_eq!(defaults(&s).word_lists, vec![list("names", &["Sayuri"])]);
        // The defaults' entry changes: the own version stays.
        set_entry(
            &mut s,
            &Scope::Defaults,
            "word_lists",
            "names",
            json!(list("names", &["Sayuri", "Yuki"])),
        )
        .unwrap();
        assert_eq!(
            profile(&s, "privat").values.word_lists[0],
            list("names", &["Jakob"])
        );
        assert_eq!(
            profile(&s, "arbeit").values.word_lists[0],
            list("names", &["Sayuri", "Yuki"])
        );
        // Removing the own one drops it, removing the defaults' one switches it off.
        remove_entry(&mut s, &profile_scope("privat"), "word_lists", "mine").unwrap();
        remove_entry(&mut s, &profile_scope("privat"), "word_lists", "names").unwrap();
        let privat = profile(&s, "privat");
        assert!(privat.values.word_lists.is_empty());
        assert_eq!(
            entries_of(privat, "word_lists").cloned(),
            Some(OwnEntries {
                own: BTreeSet::new(),
                off: set(&["names"]),
            })
        );
        // Resetting it brings the defaults' entry back.
        reset_entry(&mut s, "privat", "word_lists", "names").unwrap();
        assert_eq!(
            profile(&s, "privat").values.word_lists,
            vec![list("names", &["Sayuri", "Yuki"])]
        );
        assert_eq!(own_count(profile(&s, "privat")), 1);
        assert!(remove_entry(&mut s, &profile_scope("privat"), "word_lists", "gone").is_err());
        assert!(set_entry(&mut s, &Scope::Defaults, "selected_model", "x", json!("y")).is_err());
        assert!(set_entry(
            &mut s,
            &profile_scope("privat"),
            "word_lists",
            "a",
            json!(list("b", &[]))
        )
        .is_err());
    }

    #[test]
    fn removing_an_entry_from_the_defaults_leaves_the_own_versions() {
        let mut s = migrated();
        set_entry(
            &mut s,
            &profile_scope("arbeit"),
            "word_lists",
            "names",
            json!(list("names", &["Kunde"])),
        )
        .unwrap();
        remove_entry(&mut s, &Scope::Defaults, "word_lists", "names").unwrap();
        assert!(defaults(&s).word_lists.is_empty());
        assert!(profile(&s, "privat").values.word_lists.is_empty());
        assert_eq!(
            profile(&s, "arbeit").values.word_lists,
            vec![list("names", &["Kunde"])]
        );
    }

    #[test]
    fn a_map_entry_can_be_switched_off_in_a_profile() {
        let mut s = migrated();
        remove_entry(
            &mut s,
            &profile_scope("arbeit"),
            "post_process_models",
            "openai",
        )
        .unwrap();
        assert!(!profile(&s, "arbeit")
            .values
            .post_process_models
            .contains_key("openai"));
        assert!(defaults(&s).post_process_models.contains_key("openai"));
        assert_eq!(own_count(profile(&s, "arbeit")), 6);
    }

    // ---- The old write paths ---------------------------------------------

    #[test]
    fn with_the_defaults_in_use_a_change_goes_into_the_defaults() {
        let mut s = migrated();
        crate::profiles::switch_to(&mut s, DEFAULT_PROFILE_ID, false).unwrap();
        assert_eq!(ProfileValues::from_settings(&s), standard_values());
        s.selected_language = "fr".to_string();
        s.selected_model = "moonshine".to_string();
        route_flat(&mut s);
        assert_eq!(defaults(&s).selected_language, "fr");
        assert_eq!(profile(&s, "privat").values.selected_language, "fr");
        assert_eq!(profile(&s, "privat").values.selected_model, "moonshine");
        assert_eq!(profile(&s, "arbeit").values.selected_model, CANARY);
    }

    #[test]
    fn with_a_profile_in_use_an_own_value_changes_in_the_profile_only() {
        let mut s = migrated();
        s.selected_model = "moonshine".to_string();
        s.post_process_models
            .insert("anthropic".to_string(), "claude-opus-5".to_string());
        route_flat(&mut s);
        assert_eq!(profile(&s, "playground").values.selected_model, "moonshine");
        assert_eq!(
            profile(&s, "playground").values.post_process_models["anthropic"],
            "claude-opus-5"
        );
        assert_eq!(defaults(&s).selected_model, WHISPER);
        assert_eq!(
            defaults(&s).post_process_models["anthropic"],
            "claude-haiku-5-5"
        );
        assert_eq!(s.selected_model, "moonshine");
    }

    #[test]
    fn with_a_profile_in_use_a_coupled_value_changes_the_default_for_all() {
        let mut s = migrated();
        s.selected_language = "it".to_string();
        s.post_process_models
            .insert("openai".to_string(), "gpt-5".to_string());
        route_flat(&mut s);
        assert_eq!(defaults(&s).selected_language, "it");
        assert_eq!(profile(&s, "privat").values.selected_language, "it");
        assert_eq!(
            profile(&s, "arbeit").values.post_process_models["openai"],
            "gpt-5"
        );
        assert!(!profile(&s, "playground").own.contains("selected_language"));
    }

    #[test]
    fn with_a_profile_in_use_list_entries_go_where_they_came_from() {
        let mut s = migrated();
        // A new list counts as coupled: into the defaults.
        s.word_lists.push(list("code", &["Rust"]));
        route_flat(&mut s);
        assert_eq!(defaults(&s).word_lists.len(), 2);
        assert_eq!(profile(&s, "privat").values.word_lists.len(), 2);
        // Arbeit takes its own version of "names".
        set_entry(
            &mut s,
            &profile_scope("arbeit"),
            "word_lists",
            "names",
            json!(list("names", &["Kunde"])),
        )
        .unwrap();
        crate::profiles::switch_to(&mut s, "arbeit", false).unwrap();
        // Its own entry removed: switched off in Arbeit; the coupled one
        // changed: in the defaults.
        s.word_lists.retain(|list| list.id != "names");
        s.word_lists[0].words.push("Tauri".to_string());
        route_flat(&mut s);
        let arbeit = profile(&s, "arbeit");
        assert_eq!(
            entries_of(arbeit, "word_lists").map(|e| e.off.clone()),
            Some(set(&["names"]))
        );
        assert_eq!(defaults(&s).word_lists[0], list("names", &["Sayuri"]));
        assert_eq!(defaults(&s).word_lists[1], list("code", &["Rust", "Tauri"]));
        assert_eq!(
            profile(&s, "privat").values.word_lists[1],
            list("code", &["Rust", "Tauri"])
        );
        // The switched-off entry back in Arbeit: its own again.
        s.word_lists.insert(0, list("names", &["Wieder"]));
        route_flat(&mut s);
        let arbeit = profile(&s, "arbeit");
        assert_eq!(
            entries_of(arbeit, "word_lists").map(|e| e.own.contains("names")),
            Some(true)
        );
        assert_eq!(defaults(&s).word_lists[0], list("names", &["Sayuri"]));
        // A coupled entry removed: gone from the defaults.
        s.word_lists.retain(|list| list.id != "code");
        route_flat(&mut s);
        assert_eq!(defaults(&s).word_lists, vec![list("names", &["Sayuri"])]);
    }

    #[test]
    fn a_change_routed_to_another_profile_follows_the_same_rule() {
        let mut s = migrated();
        let mut values = profile(&s, "arbeit").values.clone();
        values.send_context = false;
        values.append_text = "{Tab}".to_string();
        route(&mut s, "arbeit", &values);
        assert!(!defaults(&s).send_context);
        assert!(!profile(&s, "privat").values.send_context);
        assert_eq!(profile(&s, "arbeit").values.append_text, "{Tab}");
        assert_eq!(defaults(&s).append_text, "");
    }
}
