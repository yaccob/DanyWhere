//! DanyWhere: one prompt library for all profiles (idea 1, decisions E1–E4
//! of 2026-11-18; sidecar note
//! `qa/2026-11-18_1914_nachbearbeitung-mehrere-prompts.md`).
//!
//! - **The library** is the flat `AppSettings::post_process_prompts`, shared
//!   by all profiles and never part of one. A change to a prompt applies in
//!   every profile; a variant for one profile is a copy under a new name.
//!   Names are unique in the library (H6: History's wand menu lists them).
//! - **Per profile** only which prompts are active and the order of the whole
//!   list: the profile value `post_process_prompt_order` (one
//!   [`PromptOrderEntry`] per prompt of the library), mirrored in the flat
//!   field of the same name like every profile value. A new prompt goes to
//!   the end and is active nowhere.
//! - **One request** carries all active prompts, in their order, each under
//!   its name (`prompt_frame::chained_instructions`); the frame tells the
//!   model that they apply in turn and that a later one wins where two
//!   contradict. One active prompt goes as before, as its text alone. A
//!   prompt that still has `${output}` can't be combined: while it is active
//!   it runs alone, as before, and the other active prompts are left out.
//! - **Cutting the list into several requests later** (E4 "C") needs one
//!   more field on [`PromptOrderEntry`] (e.g. `new_request: bool` with a
//!   serde default), nothing else stored changes.
//! - **Migration** ([`migrate`]): the copies each profile had become one
//!   library, once, on the first read of settings from before it.
//! - **Export and import**: an exported profile carries the texts of its
//!   active prompts; an import reuses a prompt with the same name and text
//!   and adds one with another text under a new name, never overwriting one.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;

use crate::profile_transfer::unique_name;
use crate::profiles::{self, ProfileValues, DEFAULT_PROFILE_ID};
use crate::prompt_frame;
use crate::settings::{self, AppSettings, LLMPrompt};

/// One prompt of the library in a profile's list.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct PromptOrderEntry {
    /// The prompt's id in the library.
    pub id: String,
    /// Whether the prompt runs ("Active").
    pub active: bool,
}

impl PromptOrderEntry {
    pub fn new(id: &str, active: bool) -> Self {
        Self {
            id: id.to_string(),
            active,
        }
    }
}

/// Between a prompt's name and the name of the profile it came from, when
/// two profiles had different texts under one name (migration, import).
pub const NAME_SUFFIX_SEPARATOR: &str = " · ";
/// Between the names of the prompts one post-processing ran with, in the
/// History step that names them (`VersionStep::prompt_name`).
pub const NAMES_SEPARATOR: &str = " → ";
/// A library command refused a name another prompt has.
pub const ERROR_NAME_TAKEN: &str = "prompt_name_taken";
/// A library command got an empty name or text.
pub const ERROR_EMPTY: &str = "prompt_empty";
/// No prompt in the library has the id.
pub const ERROR_NOT_FOUND: &str = "prompt_not_found";
/// The library's last prompt can't be deleted (as in Handy).
pub const ERROR_LAST_PROMPT: &str = "prompt_last";

// ---------------------------------------------------------------------------
// The order a profile keeps
// ---------------------------------------------------------------------------

/// `order` covering the whole library once: its entries for prompts the
/// library has, the first of each, then the library's other prompts,
/// inactive, in the library's order.
pub fn normalized(order: &[PromptOrderEntry], library: &[LLMPrompt]) -> Vec<PromptOrderEntry> {
    let known: HashSet<&str> = library.iter().map(|prompt| prompt.id.as_str()).collect();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut result: Vec<PromptOrderEntry> = order
        .iter()
        .filter(|entry| known.contains(entry.id.as_str()) && seen.insert(entry.id.as_str()))
        .cloned()
        .collect();
    for prompt in library {
        if !seen.contains(prompt.id.as_str()) {
            result.push(PromptOrderEntry::new(&prompt.id, false));
        }
    }
    result
}

/// The order of a profile that has no prompt active: the library's.
pub fn default_order(library: &[LLMPrompt]) -> Vec<PromptOrderEntry> {
    normalized(&[], library)
}

/// Makes the flat order, the defaults' and every profile's cover the
/// library. Returns whether one changed.
pub fn normalize_orders(settings: &mut AppSettings) -> bool {
    let library = &settings.post_process_prompts;
    let mut changed = false;
    let orders = std::iter::once(&mut settings.post_process_prompt_order)
        .chain(
            settings
                .default_values
                .iter_mut()
                .map(|values| &mut values.post_process_prompt_order),
        )
        .chain(
            settings
                .profiles
                .iter_mut()
                .map(|profile| &mut profile.values.post_process_prompt_order),
        );
    for order in orders {
        let whole = normalized(order, library);
        if *order != whole {
            *order = whole;
            changed = true;
        }
    }
    changed
}

// ---------------------------------------------------------------------------
// What one request carries (E4)
// ---------------------------------------------------------------------------

/// The active prompts of `order` that the library has, each once, in order.
pub fn active_prompts<'a>(
    library: &'a [LLMPrompt],
    order: &[PromptOrderEntry],
) -> Vec<&'a LLMPrompt> {
    let mut seen: HashSet<&str> = HashSet::new();
    order
        .iter()
        .filter(|entry| entry.active && seen.insert(entry.id.as_str()))
        .filter_map(|entry| library.iter().find(|prompt| prompt.id == entry.id))
        .collect()
}

/// The active prompts with a text.
fn active_with_text<'a>(
    library: &'a [LLMPrompt],
    order: &[PromptOrderEntry],
) -> Vec<&'a LLMPrompt> {
    active_prompts(library, order)
        .into_iter()
        .filter(|prompt| !prompt.prompt.trim().is_empty())
        .collect()
}

/// The prompts one post-processing request carries: the active ones with a
/// text, in order; or, while one of them still has `${output}`, the first
/// such one alone, as before the frame. Empty: nothing to post-process with.
pub fn request_prompts<'a>(
    library: &'a [LLMPrompt],
    order: &[PromptOrderEntry],
) -> Vec<&'a LLMPrompt> {
    let active = active_with_text(library, order);
    match active
        .iter()
        .find(|prompt| prompt_frame::uses_output_placeholder(&prompt.prompt))
    {
        Some(alone) => vec![*alone],
        None => active,
    }
}

/// [`request_prompts`] of the flat values: the active profile's, or a
/// dictation's own profile's in the settings it reads.
pub fn requested(settings: &AppSettings) -> Vec<&LLMPrompt> {
    request_prompts(
        &settings.post_process_prompts,
        &settings.post_process_prompt_order,
    )
}

/// The instructions for `prompts`: one prompt's text as it is, as before the
/// library; several each under its name (`prompt_frame`).
pub fn request_instructions(prompts: &[&LLMPrompt]) -> String {
    match prompts {
        [] => String::new(),
        [one] => one.prompt.clone(),
        several => prompt_frame::chained_instructions(several),
    }
}

/// The names of `prompts`, in order, for History's step.
pub fn request_names(prompts: &[&LLMPrompt]) -> Option<String> {
    (!prompts.is_empty()).then(|| {
        prompts
            .iter()
            .map(|prompt| prompt.name.as_str())
            .collect::<Vec<_>>()
            .join(NAMES_SEPARATOR)
    })
}

/// `order` with exactly the prompts `ids` active, first and in that order,
/// the rest inactive: for running chosen prompts instead of the profile's
/// (History's wand menu, H6). Ids the library doesn't have are left out.
pub fn with_only(
    order: &[PromptOrderEntry],
    library: &[LLMPrompt],
    ids: &[String],
) -> Vec<PromptOrderEntry> {
    let mut result: Vec<PromptOrderEntry> = Vec::new();
    for id in ids {
        if library.iter().any(|prompt| &prompt.id == id)
            && !result.iter().any(|entry| &entry.id == id)
        {
            result.push(PromptOrderEntry::new(id, true));
        }
    }
    for entry in normalized(order, library) {
        if !result.iter().any(|chosen| chosen.id == entry.id) {
            result.push(PromptOrderEntry::new(&entry.id, false));
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Editing the library
// ---------------------------------------------------------------------------

/// Whether a prompt other than `except` is called `name`, both trimmed.
pub fn name_taken(library: &[LLMPrompt], name: &str, except: Option<&str>) -> bool {
    library
        .iter()
        .any(|prompt| Some(prompt.id.as_str()) != except && prompt.name.trim() == name.trim())
}

/// `base`, or `base_2`, `base_3`, … whichever no prompt has.
fn unique_id(library: &[LLMPrompt], base: &str) -> String {
    let taken = |id: &str| library.iter().any(|prompt| prompt.id == id);
    if !taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}_{n}"))
        .find(|id| !taken(id))
        .expect("an unused number")
}

/// The trimmed name and text, or [`ERROR_EMPTY`].
fn cleaned(name: &str, text: &str) -> Result<(String, String), String> {
    let (name, text) = (name.trim(), text.trim());
    if name.is_empty() || text.is_empty() {
        return Err(ERROR_EMPTY.to_string());
    }
    Ok((name.to_string(), text.to_string()))
}

fn index_of(library: &[LLMPrompt], id: &str) -> Result<usize, String> {
    library
        .iter()
        .position(|prompt| prompt.id == id)
        .ok_or_else(|| ERROR_NOT_FOUND.to_string())
}

/// Adds a prompt at the end of the library, active in no profile.
pub fn add(
    settings: &mut AppSettings,
    name: &str,
    text: &str,
    now_millis: i64,
) -> Result<LLMPrompt, String> {
    let (name, text) = cleaned(name, text)?;
    if name_taken(&settings.post_process_prompts, &name, None) {
        return Err(ERROR_NAME_TAKEN.to_string());
    }
    let prompt = LLMPrompt {
        id: unique_id(
            &settings.post_process_prompts,
            &format!("prompt_{now_millis}"),
        ),
        name,
        prompt: text,
    };
    settings.post_process_prompts.push(prompt.clone());
    normalize_orders(settings);
    Ok(prompt)
}

/// Changes a prompt's name and text, in every profile.
pub fn update(settings: &mut AppSettings, id: &str, name: &str, text: &str) -> Result<(), String> {
    let (name, text) = cleaned(name, text)?;
    let index = index_of(&settings.post_process_prompts, id)?;
    if name_taken(&settings.post_process_prompts, &name, Some(id)) {
        return Err(ERROR_NAME_TAKEN.to_string());
    }
    let prompt = &mut settings.post_process_prompts[index];
    prompt.name = name;
    prompt.prompt = text;
    Ok(())
}

/// Deletes a prompt from the library and from every profile's order. The
/// last prompt stays, as in Handy.
pub fn delete(settings: &mut AppSettings, id: &str) -> Result<(), String> {
    let index = index_of(&settings.post_process_prompts, id)?;
    if settings.post_process_prompts.len() <= 1 {
        return Err(ERROR_LAST_PROMPT.to_string());
    }
    settings.post_process_prompts.remove(index);
    normalize_orders(settings);
    Ok(())
}

/// Sets the active profile's order (the flat one), made whole.
pub fn set_order(settings: &mut AppSettings, order: Vec<PromptOrderEntry>) {
    settings.post_process_prompt_order = normalized(&order, &settings.post_process_prompts);
}

/// Handy's "selected prompt": prompt `id` becomes the active profile's only
/// active one; the order stays.
pub fn select_only(settings: &mut AppSettings, id: &str) -> Result<(), String> {
    index_of(&settings.post_process_prompts, id)?;
    settings.post_process_prompt_order = normalized(
        &settings.post_process_prompt_order,
        &settings.post_process_prompts,
    )
    .into_iter()
    .map(|entry| PromptOrderEntry {
        active: entry.id == id,
        ..entry
    })
    .collect();
    Ok(())
}

/// The shown names of the defaults (`default_name`, their translated name)
/// and of the profiles that have prompt `id` active, in that order.
pub fn profiles_using(settings: &AppSettings, id: &str, default_name: &str) -> Vec<String> {
    let uses =
        |order: &[PromptOrderEntry]| order.iter().any(|entry| entry.id == id && entry.active);
    // What is in use has its latest order in the flat fields.
    let defaults = profiles::values_of(settings, profiles::DEFAULT_PROFILE_ID)
        .is_some_and(|values| uses(&values.post_process_prompt_order));
    defaults
        .then(|| default_name.to_string())
        .into_iter()
        .chain(
            settings
                .profiles
                .iter()
                .filter(|profile| {
                    let order = if profile.id == settings.active_profile_id {
                        &settings.post_process_prompt_order
                    } else {
                        &profile.values.post_process_prompt_order
                    };
                    uses(order)
                })
                .map(|profile| profiles::display_name(profile, default_name)),
        )
        .collect()
}

// ---------------------------------------------------------------------------
// From a copy per profile to the library (E1)
// ---------------------------------------------------------------------------

/// One profile's prompts as stored before the library.
#[derive(Debug, Clone, PartialEq)]
pub struct LegacyPrompts {
    pub profile_id: String,
    /// Its shown name, appended to a prompt whose name another text has.
    pub profile_name: String,
    pub prompts: Vec<LLMPrompt>,
    /// The prompt it post-processed with (`post_process_selected_prompt_id`).
    pub selected: Option<String>,
}

/// Merges the profiles' copies into one library, the first source first:
/// - Handy's untouched default is replaced first, as it would have been;
/// - the same text is one prompt, under the name it had first;
/// - another text under a name already in the library becomes a prompt of
///   its own, named "<name> · <profile>" (in the first source, which keeps
///   the plain names, "<name> (2)"), with an id of its own;
/// - each profile's order lists its own prompts first, in its order, then the
///   rest of the library; exactly its selected prompt is active.
///
/// Returns the library and each source's order by profile id.
pub fn merge_legacy(
    sources: &[LegacyPrompts],
) -> (Vec<LLMPrompt>, HashMap<String, Vec<PromptOrderEntry>>) {
    let mut library: Vec<LLMPrompt> = Vec::new();
    let mut orders: HashMap<String, Vec<PromptOrderEntry>> = HashMap::new();
    for (index, source) in sources.iter().enumerate() {
        let mut prompts = source.prompts.clone();
        prompt_frame::replace_untouched_handy_default(&mut prompts);
        let mut order: Vec<PromptOrderEntry> = Vec::new();
        let mut selected_found = false;
        for prompt in &prompts {
            let id = match library.iter().find(|known| known.prompt == prompt.prompt) {
                Some(known) => known.id.clone(),
                None => {
                    let plain = prompt.name.trim();
                    let wanted = if index == 0 || !name_taken(&library, plain, None) {
                        plain.to_string()
                    } else {
                        format!("{plain}{NAME_SUFFIX_SEPARATOR}{}", source.profile_name)
                    };
                    let taken: Vec<String> = library.iter().map(|p| p.name.clone()).collect();
                    let base = if library.iter().any(|known| known.id == prompt.id) {
                        format!("{}_{}", prompt.id, source.profile_id)
                    } else {
                        prompt.id.clone()
                    };
                    let added = LLMPrompt {
                        id: unique_id(&library, &base),
                        name: unique_name(&wanted, &taken),
                        prompt: prompt.prompt.clone(),
                    };
                    let id = added.id.clone();
                    library.push(added);
                    id
                }
            };
            let selected =
                !selected_found && source.selected.as_deref() == Some(prompt.id.as_str());
            selected_found |= selected;
            match order.iter_mut().find(|entry| entry.id == id) {
                Some(entry) => entry.active |= selected,
                None => order.push(PromptOrderEntry {
                    id,
                    active: selected,
                }),
            }
        }
        orders.insert(source.profile_id.clone(), order);
    }
    for order in orders.values_mut() {
        *order = normalized(order, &library);
    }
    (library, orders)
}

/// The prompts of every profile as `settings_value` (the stored settings)
/// had them, the default profile first. The active profile's are the flat
/// ones, its latest; a profile that stored none had a new installation's.
fn legacy_sources(
    settings: &AppSettings,
    settings_value: &serde_json::Value,
) -> Vec<LegacyPrompts> {
    let flat = |profile_id: &str, profile_name: String| LegacyPrompts {
        profile_id: profile_id.to_string(),
        profile_name,
        prompts: settings.post_process_prompts.clone(),
        selected: settings.post_process_selected_prompt_id.clone(),
    };
    if settings.profiles.is_empty() {
        // From before profiles: the flat values become the default profile.
        return vec![flat(DEFAULT_PROFILE_ID, String::new())];
    }
    let default_name = profiles::default_profile_name(settings);
    let stored = settings_value
        .get("profiles")
        .and_then(|profiles| profiles.as_array());
    let mut sources: Vec<LegacyPrompts> = settings
        .profiles
        .iter()
        .map(|profile| {
            let name = profiles::display_name(profile, &default_name);
            if profile.id == settings.active_profile_id {
                return flat(&profile.id, name);
            }
            let values = stored
                .and_then(|list| {
                    list.iter().find(|stored| {
                        stored.get("id").and_then(|id| id.as_str()) == Some(profile.id.as_str())
                    })
                })
                .and_then(|stored| stored.get("values"));
            let prompts = values
                .and_then(|values| values.get("post_process_prompts"))
                .and_then(|prompts| serde_json::from_value::<Vec<LLMPrompt>>(prompts.clone()).ok())
                .unwrap_or_else(|| settings::get_default_settings().post_process_prompts);
            let selected = values
                .and_then(|values| values.get("post_process_selected_prompt_id"))
                .and_then(|id| id.as_str())
                .map(str::to_string);
            LegacyPrompts {
                profile_id: profile.id.clone(),
                profile_name: name,
                prompts,
                selected,
            }
        })
        .collect();
    // The default profile keeps the plain names (it becomes the defaults).
    sources.sort_by_key(|source| source.profile_id != DEFAULT_PROFILE_ID);
    sources
}

/// Once, for settings stored before the library (no flat
/// `post_process_prompt_order` in `settings_value`): every profile's copies
/// become one library ([`merge_legacy`]), every profile keeps exactly the
/// prompt it post-processed with, active, so nothing changes in what is
/// sent. Handy's `post_process_selected_prompt_id` is cleared. Returns
/// whether it ran.
pub fn migrate(settings: &mut AppSettings, settings_value: &serde_json::Value) -> bool {
    if settings_value.get("post_process_prompt_order").is_some() {
        return false;
    }
    let sources = legacy_sources(settings, settings_value);
    let (library, orders) = merge_legacy(&sources);
    log::info!(
        "Prompt library: {} prompt(s) from the copies of {} profile(s)",
        library.len(),
        sources.len()
    );
    settings.post_process_prompts = library;
    settings.post_process_selected_prompt_id = None;
    for profile in settings.profiles.iter_mut() {
        if let Some(order) = orders.get(&profile.id) {
            profile.values.post_process_prompt_order = order.clone();
        }
    }
    let flat_id = if settings.profiles.is_empty() {
        DEFAULT_PROFILE_ID
    } else {
        settings.active_profile_id.as_str()
    };
    settings.post_process_prompt_order = orders
        .get(flat_id)
        .cloned()
        .unwrap_or_else(|| default_order(&settings.post_process_prompts));
    normalize_orders(settings);
    true
}

// ---------------------------------------------------------------------------
// Export and import
// ---------------------------------------------------------------------------

/// For an export: the order lists only the active prompts, whose texts
/// travel with it ([`export_prompts`]); the others mean nothing elsewhere.
pub fn keep_active_entries(values: &mut ProfileValues) {
    values
        .post_process_prompt_order
        .retain(|entry| entry.active);
}

/// The active prompts of `order`, with their texts, for an export file.
pub fn export_prompts(library: &[LLMPrompt], order: &[PromptOrderEntry]) -> Vec<LLMPrompt> {
    active_prompts(library, order)
        .into_iter()
        .cloned()
        .collect()
}

/// A file from before the library carries the profile's own copies in its
/// values (`post_process_prompts`, `post_process_selected_prompt_id`): they
/// become the file's prompts, all of them, with the selected one active, so
/// nothing of the file gets lost. A file with the library's order is left as
/// it is.
pub fn prepare_import(
    file_text: &str,
    prompts: &mut Vec<LLMPrompt>,
    order: &mut Vec<PromptOrderEntry>,
) {
    let Ok(file) = serde_saphyr::from_str::<serde_json::Value>(file_text) else {
        return;
    };
    let Some(values) = file
        .get("profile")
        .and_then(|profile| profile.get("values"))
    else {
        return;
    };
    if values.get("post_process_prompt_order").is_some() {
        return;
    }
    let Some(legacy) = values
        .get("post_process_prompts")
        .and_then(|legacy| serde_json::from_value::<Vec<LLMPrompt>>(legacy.clone()).ok())
    else {
        return;
    };
    let mut selected = values
        .get("post_process_selected_prompt_id")
        .and_then(|id| id.as_str());
    *order = legacy
        .iter()
        .map(|prompt| {
            let active = selected == Some(prompt.id.as_str());
            if active {
                selected = None;
            }
            PromptOrderEntry::new(&prompt.id, active)
        })
        .collect();
    *prompts = legacy;
}

/// Puts an imported profile's prompts into the library: one with the same
/// name and text is reused; one with another text under a taken name is
/// added as "<name> · <profile_name>"; a new one keeps its name. Nothing is
/// overwritten. The other profiles get the new prompts inactive. Returns the
/// imported profile's order: the file's prompts as the file had them
/// active, in its order, then the rest of the library.
pub fn import_prompts(
    settings: &mut AppSettings,
    prompts: &[LLMPrompt],
    order: &[PromptOrderEntry],
    profile_name: &str,
) -> Vec<PromptOrderEntry> {
    let mut file_prompts = prompts.to_vec();
    prompt_frame::replace_untouched_handy_default(&mut file_prompts);
    let mut imported: Vec<PromptOrderEntry> = Vec::new();
    for entry in order {
        let Some(prompt) = file_prompts.iter().find(|prompt| prompt.id == entry.id) else {
            continue;
        };
        let library = &settings.post_process_prompts;
        let id = match library
            .iter()
            .find(|known| known.name.trim() == prompt.name.trim() && known.prompt == prompt.prompt)
        {
            Some(same) => same.id.clone(),
            None => {
                let plain = prompt.name.trim();
                let wanted = if name_taken(library, plain, None) {
                    format!("{plain}{NAME_SUFFIX_SEPARATOR}{}", profile_name.trim())
                } else {
                    plain.to_string()
                };
                let taken: Vec<String> = library.iter().map(|p| p.name.clone()).collect();
                let added = LLMPrompt {
                    id: unique_id(library, &prompt.id),
                    name: unique_name(&wanted, &taken),
                    prompt: prompt.prompt.clone(),
                };
                let id = added.id.clone();
                log::info!("Import adds the prompt '{}' to the library", added.name);
                settings.post_process_prompts.push(added);
                id
            }
        };
        if !imported.iter().any(|known| known.id == id) {
            imported.push(PromptOrderEntry::new(&id, entry.active));
        }
    }
    normalize_orders(settings);
    normalized(&imported, &settings.post_process_prompts)
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Sets which prompts the active profile runs, and their order (the whole
/// list). The list editor sends it after every tick, drag or arrow key.
#[tauri::command]
#[specta::specta]
pub fn change_post_process_prompt_order_setting(
    app: AppHandle,
    order: Vec<PromptOrderEntry>,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    set_order(&mut settings, order);
    settings::write_settings(&app, settings);
    // A prompt with ${output} may now leave others out, or no longer.
    prompt_frame::prompts_changed(&app);
    Ok(())
}

/// The shown names of the profiles that have prompt `id` active, so deleting
/// it can name them first.
#[tauri::command]
#[specta::specta]
pub fn profiles_using_prompt(app: AppHandle, id: String) -> Vec<String> {
    let settings = settings::get_settings(&app);
    profiles_using(&settings, &id, &profiles::default_profile_name(&settings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::{ensure_profiles, DEFAULT_PROFILE_ID};
    use crate::prompt_frame::{HANDY_IMPROVE_TRANSCRIPTIONS, IMPROVE_TRANSCRIPTIONS};

    fn prompt(id: &str, name: &str, text: &str) -> LLMPrompt {
        LLMPrompt {
            id: id.to_string(),
            name: name.to_string(),
            prompt: text.to_string(),
        }
    }

    fn entry(id: &str, active: bool) -> PromptOrderEntry {
        PromptOrderEntry::new(id, active)
    }

    fn names(prompts: &[&LLMPrompt]) -> Vec<String> {
        prompts.iter().map(|p| p.name.clone()).collect()
    }

    fn library_names(library: &[LLMPrompt]) -> Vec<String> {
        library.iter().map(|p| p.name.clone()).collect()
    }

    fn ids(order: &[PromptOrderEntry]) -> Vec<String> {
        order.iter().map(|e| e.id.clone()).collect()
    }

    fn library() -> Vec<LLMPrompt> {
        vec![
            prompt("a", "Diktat", "Clean it up."),
            prompt("b", "Absätze", "Add paragraphs."),
            prompt("c", "Mail", "Make an email of it."),
        ]
    }

    // ---- The order a profile keeps -----------------------------------------

    #[test]
    fn an_order_covers_the_whole_library_once() {
        let order = normalized(
            &[
                entry("c", true),
                entry("gone", true),
                entry("a", false),
                entry("c", false),
            ],
            &library(),
        );
        assert_eq!(
            order,
            vec![entry("c", true), entry("a", false), entry("b", false)]
        );
    }

    #[test]
    fn a_new_installation_has_every_prompt_inactive() {
        assert_eq!(
            default_order(&library()),
            vec![entry("a", false), entry("b", false), entry("c", false)]
        );
        let settings = crate::settings::get_default_settings();
        assert_eq!(
            settings.post_process_prompt_order,
            default_order(&settings.post_process_prompts)
        );
    }

    #[test]
    fn normalizing_fixes_the_flat_order_and_every_profiles() {
        let mut s = AppSettings {
            post_process_prompts: library(),
            post_process_prompt_order: vec![entry("b", true)],
            ..Default::default()
        };
        ensure_profiles(&mut s);
        let mut other = crate::profiles::profile_for_test(&s, "other", "Other", |_| {});
        other.values.post_process_prompt_order = vec![entry("gone", true)];
        s.profiles.push(other);
        if let Some(defaults) = s.default_values.as_mut() {
            defaults.post_process_prompt_order = vec![entry("c", true)];
        }
        assert!(normalize_orders(&mut s));
        assert_eq!(
            s.post_process_prompt_order,
            vec![entry("b", true), entry("a", false), entry("c", false)]
        );
        assert_eq!(
            s.profiles[0].values.post_process_prompt_order,
            default_order(&library())
        );
        // The defaults' order too.
        assert_eq!(
            crate::profiles::default_values(&s).post_process_prompt_order,
            vec![entry("c", true), entry("a", false), entry("b", false)]
        );
        assert!(!normalize_orders(&mut s));
    }

    // ---- What one request carries (E4) -------------------------------------

    #[test]
    fn the_active_prompts_go_in_the_profiles_order() {
        let order = vec![entry("c", true), entry("a", false), entry("b", true)];
        assert_eq!(
            names(&request_prompts(&library(), &order)),
            vec!["Mail", "Absätze"]
        );
        assert_eq!(
            names(&active_prompts(&library(), &order)),
            vec!["Mail", "Absätze"]
        );
    }

    #[test]
    fn unknown_and_blank_prompts_are_left_out() {
        let mut lib = library();
        lib[1].prompt = "  \n".to_string();
        let order = vec![entry("gone", true), entry("b", true), entry("a", true)];
        assert_eq!(names(&request_prompts(&lib, &order)), vec!["Diktat"]);
        assert!(request_prompts(&lib, &[entry("b", true)]).is_empty());
        assert!(request_prompts(&lib, &[]).is_empty());
    }

    #[test]
    fn a_prompt_with_output_runs_alone_while_it_is_active() {
        let mut lib = library();
        lib.push(prompt("old", "Old", "Fix: ${output}"));
        lib.push(prompt("old2", "Old 2", "Also: ${output}"));
        let order = vec![
            entry("a", true),
            entry("old", true),
            entry("b", true),
            entry("old2", true),
        ];
        assert_eq!(names(&request_prompts(&lib, &order)), vec!["Old"]);
        // Inactive, it plays no part.
        assert_eq!(
            names(&request_prompts(
                &lib,
                &[entry("old", false), entry("a", true)]
            )),
            vec!["Diktat"]
        );
    }

    #[test]
    fn one_prompt_is_sent_as_its_text_several_under_their_names() {
        let lib = library();
        let one = request_prompts(&lib, &[entry("a", true)]);
        assert_eq!(request_instructions(&one), "Clean it up.");
        assert_eq!(request_names(&one).as_deref(), Some("Diktat"));
        let two = request_prompts(&lib, &[entry("a", true), entry("b", true)]);
        assert_eq!(
            request_instructions(&two),
            crate::prompt_frame::chained_instructions(&two)
        );
        assert_eq!(request_names(&two).as_deref(), Some("Diktat → Absätze"));
        assert_eq!(request_instructions(&[]), "");
        assert_eq!(request_names(&[]), None);
    }

    #[test]
    fn the_settings_request_reads_the_flat_order() {
        let s = AppSettings {
            post_process_prompts: library(),
            post_process_prompt_order: vec![entry("b", true), entry("a", true)],
            ..Default::default()
        };
        assert_eq!(names(&requested(&s)), vec!["Absätze", "Diktat"]);
    }

    #[test]
    fn an_explicit_list_runs_just_those_prompts_in_its_order() {
        let order = vec![entry("a", true), entry("b", false), entry("c", true)];
        let only = with_only(&order, &library(), &["b".to_string(), "gone".to_string()]);
        assert_eq!(
            only,
            vec![entry("b", true), entry("a", false), entry("c", false)]
        );
        let two = with_only(&order, &library(), &["c".to_string(), "b".to_string()]);
        assert_eq!(
            names(&request_prompts(&library(), &two)),
            vec!["Mail", "Absätze"]
        );
    }

    // ---- Editing the library -----------------------------------------------

    /// The library above, the defaults (in use, "a" active) and "work"
    /// ("c" active, its own order).
    fn two_profiles() -> AppSettings {
        let mut s = AppSettings {
            post_process_prompts: library(),
            post_process_prompt_order: vec![entry("a", true)],
            ..Default::default()
        };
        ensure_profiles(&mut s);
        let work = crate::profiles::profile_for_test(&s, "work", "Work", |values| {
            values.post_process_prompt_order =
                vec![entry("c", true), entry("a", false), entry("b", false)];
        });
        s.profiles.push(work);
        s
    }

    #[test]
    fn a_new_prompt_goes_to_the_end_and_is_active_nowhere() {
        let mut s = two_profiles();
        let added = add(&mut s, "  Englisch ", "Translate it.", 42).expect("added");
        assert_eq!(added.name, "Englisch");
        assert_eq!(s.post_process_prompts.last(), Some(&added));
        for order in [
            &s.post_process_prompt_order,
            &crate::profiles::default_values(&s).post_process_prompt_order,
            &s.profiles[0].values.post_process_prompt_order,
        ] {
            assert_eq!(order.last(), Some(&entry(&added.id, false)), "{order:?}");
            assert_eq!(order.len(), 4);
        }
        // Still exactly the prompts that were active.
        assert_eq!(names(&requested(&s)), vec!["Diktat"]);
    }

    #[test]
    fn two_prompts_added_in_one_millisecond_get_their_own_ids() {
        let mut s = two_profiles();
        let first = add(&mut s, "One", "1", 7).expect("added");
        let second = add(&mut s, "Two", "2", 7).expect("added");
        assert_ne!(first.id, second.id);
    }

    #[test]
    fn a_name_is_unique_in_the_library() {
        let mut s = two_profiles();
        assert_eq!(
            add(&mut s, " Mail ", "Other text.", 1),
            Err(ERROR_NAME_TAKEN.to_string())
        );
        assert_eq!(
            update(&mut s, "a", "Mail", "Clean it up."),
            Err(ERROR_NAME_TAKEN.to_string())
        );
        // Its own name is no conflict.
        assert_eq!(update(&mut s, "c", "Mail", "Write a mail."), Ok(()));
        assert_eq!(s.post_process_prompts[2].prompt, "Write a mail.");
        assert!(name_taken(&s.post_process_prompts, "Diktat", None));
        assert!(!name_taken(&s.post_process_prompts, "Diktat", Some("a")));
        assert_eq!(s.post_process_prompts.len(), 3);
    }

    #[test]
    fn a_prompt_needs_a_name_and_a_text() {
        let mut s = two_profiles();
        assert_eq!(add(&mut s, " ", "Text", 1), Err(ERROR_EMPTY.to_string()));
        assert_eq!(add(&mut s, "Name", "  ", 1), Err(ERROR_EMPTY.to_string()));
        assert_eq!(update(&mut s, "a", "", "x"), Err(ERROR_EMPTY.to_string()));
        assert_eq!(
            update(&mut s, "gone", "Name", "x"),
            Err(ERROR_NOT_FOUND.to_string())
        );
    }

    #[test]
    fn a_change_to_a_prompt_applies_in_every_profile() {
        let mut s = two_profiles();
        update(&mut s, "a", "Diktat", "Clean it up, please.").expect("updated");
        crate::profiles::switch_to(&mut s, "work", false).expect("switched");
        s.post_process_prompt_order = vec![entry("a", true)];
        assert_eq!(request_instructions(&requested(&s)), "Clean it up, please.");
    }

    #[test]
    fn deleting_a_prompt_takes_it_out_of_every_order() {
        let mut s = two_profiles();
        delete(&mut s, "a").expect("deleted");
        assert_eq!(
            library_names(&s.post_process_prompts),
            vec!["Absätze", "Mail"]
        );
        for order in [
            &s.post_process_prompt_order,
            &crate::profiles::default_values(&s).post_process_prompt_order,
            &s.profiles[0].values.post_process_prompt_order,
        ] {
            assert!(!ids(order).contains(&"a".to_string()), "{order:?}");
        }
        assert_eq!(delete(&mut s, "gone"), Err(ERROR_NOT_FOUND.to_string()));
        delete(&mut s, "b").expect("deleted");
        assert_eq!(delete(&mut s, "c"), Err(ERROR_LAST_PROMPT.to_string()));
        assert_eq!(s.post_process_prompts.len(), 1);
    }

    #[test]
    fn before_deleting_the_profiles_using_a_prompt_are_named() {
        let mut s = two_profiles();
        assert_eq!(profiles_using(&s, "a", "Standard"), vec!["Standard"]);
        assert_eq!(profiles_using(&s, "c", "Standard"), vec!["Work"]);
        assert!(profiles_using(&s, "b", "Standard").is_empty());
        // The active profile's latest order is the flat one.
        s.post_process_prompt_order = vec![entry("b", true)];
        assert_eq!(profiles_using(&s, "b", "Standard"), vec!["Standard"]);
        assert!(profiles_using(&s, "a", "Standard").is_empty());
    }

    #[test]
    fn setting_the_order_keeps_it_whole() {
        let mut s = two_profiles();
        set_order(&mut s, vec![entry("c", true), entry("gone", true)]);
        assert_eq!(
            s.post_process_prompt_order,
            vec![entry("c", true), entry("a", false), entry("b", false)]
        );
    }

    #[test]
    fn selecting_a_prompt_makes_it_the_only_active_one() {
        let mut s = two_profiles();
        s.post_process_prompt_order = vec![entry("a", true), entry("b", true), entry("c", false)];
        select_only(&mut s, "c").expect("selected");
        assert_eq!(names(&requested(&s)), vec!["Mail"]);
        assert_eq!(ids(&s.post_process_prompt_order), vec!["a", "b", "c"]);
        assert_eq!(
            select_only(&mut s, "gone"),
            Err(ERROR_NOT_FOUND.to_string())
        );
    }

    // ---- From a copy per profile to the library (E1) ------------------------

    const STANDARD: &str = "Diktat, Standard: sagen wir mal = sag mir einmal.";
    const PRIVAT: &str = "Diktat, Privat: ohne die Regel.";
    const ARBEIT: &str = "Diktat, Arbeit: mit der Regel und Absätzen.";

    fn legacy_values(diktat: &str) -> serde_json::Value {
        serde_json::json!({
            "post_process_enabled": true,
            "post_process_prompts": [
                {"id": "default_improve_transcriptions", "name": "Improve Transcriptions", "prompt": IMPROVE_TRANSCRIPTIONS},
                {"id": "diktat_de", "name": "Diktat (DE/EN)", "prompt": diktat}
            ],
            "post_process_selected_prompt_id": "diktat_de"
        })
    }

    /// Shaped like the user's settings on 2026-11-19 (names and lengths
    /// read, texts made up): Standard, Privat and Arbeit each with their own
    /// "Diktat (DE/EN)" and Handy's "Improve Transcriptions", and Playground,
    /// active, with Arbeit's text.
    fn users_store() -> serde_json::Value {
        let mut flat = legacy_values(ARBEIT);
        let flat = flat.as_object_mut().unwrap();
        flat.insert("active_profile_id".into(), "playground".into());
        flat.insert(
            "profiles".into(),
            serde_json::json!([
                {"id": "default", "name": "", "values": legacy_values(STANDARD)},
                {"id": "privat", "name": "Privat", "values": legacy_values(PRIVAT), "apps": ["com.apple.Notes"]},
                {"id": "arbeit", "name": "Arbeit", "values": legacy_values(ARBEIT)},
                {"id": "playground", "name": "Playground", "values": legacy_values(ARBEIT)}
            ]),
        );
        serde_json::Value::Object(flat.clone())
    }

    /// Reads `store` as `get_settings` does: parse, migrate, make the
    /// profiles consistent. Returns the settings and whether the migration
    /// changed anything.
    fn load(store: &serde_json::Value) -> (AppSettings, bool) {
        let mut settings: AppSettings = serde_json::from_value(store.clone()).expect("parses");
        let migrated = migrate(&mut settings, store);
        ensure_profiles(&mut settings);
        (settings, migrated)
    }

    fn order_of<'a>(s: &'a AppSettings, id: &str) -> &'a [PromptOrderEntry] {
        if id == s.active_profile_id {
            &s.post_process_prompt_order
        } else if id == DEFAULT_PROFILE_ID {
            // "Standard" became the defaults (strand A1).
            &s.default_values
                .as_ref()
                .expect("the defaults")
                .post_process_prompt_order
        } else {
            &s.profiles
                .iter()
                .find(|p| p.id == id)
                .expect("the profile")
                .values
                .post_process_prompt_order
        }
    }

    #[test]
    fn the_users_prompts_become_one_library() {
        let (s, migrated) = load(&users_store());
        assert!(migrated);
        assert_eq!(
            library_names(&s.post_process_prompts),
            vec![
                "Improve Transcriptions",
                "Diktat (DE/EN)",
                "Diktat (DE/EN) · Privat",
                "Diktat (DE/EN) · Arbeit",
            ]
        );
        let texts: Vec<&str> = s
            .post_process_prompts
            .iter()
            .map(|p| p.prompt.as_str())
            .collect();
        assert_eq!(
            texts,
            vec![IMPROVE_TRANSCRIPTIONS, STANDARD, PRIVAT, ARBEIT]
        );
        let ids: Vec<&str> = s
            .post_process_prompts
            .iter()
            .map(|p| p.id.as_str())
            .collect();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 4, "{ids:?}");
        assert_eq!(s.post_process_selected_prompt_id, None);
    }

    #[test]
    fn every_profile_keeps_exactly_the_prompt_it_used() {
        let (s, _) = load(&users_store());
        for (profile, active, text) in [
            ("default", "Diktat (DE/EN)", STANDARD),
            ("privat", "Diktat (DE/EN) · Privat", PRIVAT),
            ("arbeit", "Diktat (DE/EN) · Arbeit", ARBEIT),
            ("playground", "Diktat (DE/EN) · Arbeit", ARBEIT),
        ] {
            let order = order_of(&s, profile);
            assert_eq!(
                names(&active_prompts(&s.post_process_prompts, order)),
                vec![active],
                "{profile}"
            );
            // The request is what it was: the selected prompt's text alone.
            let request = request_prompts(&s.post_process_prompts, order);
            assert_eq!(request_instructions(&request), text, "{profile}");
            // Each order covers the whole library.
            assert_eq!(order.len(), 4, "{profile}: {order:?}");
        }
        // The profile's own prompts come first, in its order.
        let privat = order_of(&s, "privat");
        assert_eq!(
            privat
                .iter()
                .map(|e| {
                    s.post_process_prompts
                        .iter()
                        .find(|p| p.id == e.id)
                        .unwrap()
                        .name
                        .as_str()
                })
                .collect::<Vec<_>>(),
            vec![
                "Improve Transcriptions",
                "Diktat (DE/EN) · Privat",
                "Diktat (DE/EN)",
                "Diktat (DE/EN) · Arbeit",
            ]
        );
        // Everything else stays.
        let privat = s.profiles.iter().find(|p| p.id == "privat").unwrap();
        assert_eq!(privat.apps, vec!["com.apple.Notes".to_string()]);
        assert!(privat.values.post_process_enabled);
    }

    #[test]
    fn migrating_twice_changes_nothing_the_second_time() {
        let (s, _) = load(&users_store());
        let stored = serde_json::to_value(&s).unwrap();
        let (again, migrated) = load(&stored);
        assert!(!migrated);
        assert_eq!(again.post_process_prompts, s.post_process_prompts);
        assert_eq!(again.post_process_prompt_order, s.post_process_prompt_order);
        assert_eq!(again.profiles, s.profiles);
    }

    #[test]
    fn handys_untouched_prompt_and_its_replacement_become_one() {
        let mut store = users_store();
        store["profiles"][1]["values"]["post_process_prompts"][0]["prompt"] =
            HANDY_IMPROVE_TRANSCRIPTIONS[1].into();
        let (s, _) = load(&store);
        let improve: Vec<&LLMPrompt> = s
            .post_process_prompts
            .iter()
            .filter(|p| p.name.starts_with("Improve Transcriptions"))
            .collect();
        assert_eq!(improve.len(), 1, "{:?}", s.post_process_prompts);
        assert_eq!(improve[0].prompt, IMPROVE_TRANSCRIPTIONS);
    }

    #[test]
    fn the_default_profile_keeps_the_plain_names_wherever_it_stands() {
        let mut store = users_store();
        let profiles = store["profiles"].as_array_mut().unwrap();
        let default = profiles.remove(0);
        profiles.push(default);
        let (s, _) = load(&store);
        let diktat = s
            .post_process_prompts
            .iter()
            .find(|p| p.name == "Diktat (DE/EN)")
            .expect("the plain name");
        assert_eq!(diktat.prompt, STANDARD);
    }

    #[test]
    fn the_same_text_under_another_name_is_one_prompt() {
        let mut store = users_store();
        store["profiles"][1]["values"]["post_process_prompts"][1]["name"] = "Mein Diktat".into();
        store["profiles"][1]["values"]["post_process_prompts"][1]["prompt"] = STANDARD.into();
        let (s, _) = load(&store);
        assert!(!library_names(&s.post_process_prompts).contains(&"Mein Diktat".to_string()));
        assert_eq!(
            names(&active_prompts(
                &s.post_process_prompts,
                order_of(&s, "privat")
            )),
            vec!["Diktat (DE/EN)"]
        );
    }

    #[test]
    fn two_names_alike_in_one_profile_get_a_number() {
        let sources = vec![LegacyPrompts {
            profile_id: DEFAULT_PROFILE_ID.to_string(),
            profile_name: "Standard".to_string(),
            prompts: vec![prompt("x", "Mail", "One."), prompt("y", "Mail", "Two.")],
            selected: Some("y".to_string()),
        }];
        let (lib, orders) = merge_legacy(&sources);
        assert_eq!(library_names(&lib), vec!["Mail", "Mail (2)"]);
        assert_eq!(
            names(&active_prompts(&lib, &orders[DEFAULT_PROFILE_ID])),
            vec!["Mail (2)"]
        );
    }

    #[test]
    fn a_profile_without_a_selected_prompt_has_none_active() {
        let mut store = users_store();
        store["profiles"][2]["values"]["post_process_selected_prompt_id"] = serde_json::Value::Null;
        let (s, _) = load(&store);
        assert!(active_prompts(&s.post_process_prompts, order_of(&s, "arbeit")).is_empty());
    }

    #[test]
    fn settings_from_before_profiles_keep_their_selected_prompt() {
        let store = legacy_values(STANDARD);
        let (s, migrated) = load(&store);
        assert!(migrated);
        assert!(s.profiles.is_empty());
        assert_eq!(
            library_names(&s.post_process_prompts),
            vec!["Improve Transcriptions", "Diktat (DE/EN)"]
        );
        assert_eq!(
            names(&active_prompts(
                &s.post_process_prompts,
                &crate::profiles::default_values(&s).post_process_prompt_order
            )),
            vec!["Diktat (DE/EN)"]
        );
    }

    #[test]
    fn a_stored_order_means_there_is_nothing_to_migrate() {
        let mut store = users_store();
        store["post_process_prompt_order"] = serde_json::json!([]);
        let mut s: AppSettings = serde_json::from_value(store.clone()).unwrap();
        assert!(!migrate(&mut s, &store));
    }

    // ---- Export and import ---------------------------------------------------

    #[test]
    fn an_export_carries_the_texts_of_the_active_prompts_in_order() {
        let order = vec![entry("c", true), entry("a", false), entry("b", true)];
        assert_eq!(
            export_prompts(&library(), &order),
            vec![library()[2].clone(), library()[1].clone()]
        );
        let mut values = ProfileValues {
            post_process_prompt_order: order,
            ..Default::default()
        };
        keep_active_entries(&mut values);
        assert_eq!(
            values.post_process_prompt_order,
            vec![entry("c", true), entry("b", true)]
        );
    }

    #[test]
    fn an_import_reuses_a_prompt_with_the_same_name_and_text() {
        let mut s = two_profiles();
        let file = vec![prompt("x9", "Mail", "Make an email of it.")];
        let order = import_prompts(&mut s, &file, &[entry("x9", true)], "Büro");
        assert_eq!(s.post_process_prompts, library());
        assert_eq!(
            order,
            vec![entry("c", true), entry("a", false), entry("b", false)]
        );
    }

    #[test]
    fn an_import_never_overwrites_a_prompt_with_another_text() {
        let mut s = two_profiles();
        let file = vec![
            prompt("a", "Diktat", "Clean it up, the office way."),
            prompt("n", "Englisch", "Translate it."),
        ];
        let order = import_prompts(&mut s, &file, &[entry("a", true), entry("n", true)], "Büro");
        assert_eq!(s.post_process_prompts[0], library()[0]);
        assert_eq!(
            library_names(&s.post_process_prompts),
            vec!["Diktat", "Absätze", "Mail", "Diktat · Büro", "Englisch"]
        );
        assert_eq!(
            names(&active_prompts(&s.post_process_prompts, &order)),
            vec!["Diktat · Büro", "Englisch"]
        );
        assert_eq!(order.len(), 5);
        // The other profiles get them, inactive.
        for order in [
            &s.post_process_prompt_order,
            &s.profiles[0].values.post_process_prompt_order,
            &crate::profiles::default_values(&s).post_process_prompt_order,
        ] {
            assert_eq!(order.len(), 5, "{order:?}");
            assert_eq!(active_prompts(&s.post_process_prompts, order).len(), 1);
        }
        let ids: Vec<&str> = s
            .post_process_prompts
            .iter()
            .map(|p| p.id.as_str())
            .collect();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "{ids:?}");
    }

    #[test]
    fn a_file_from_before_the_library_brings_all_its_prompts() {
        let text = "format: danywhere-profile\nversion: 1\nprofile:\n  id: old\n  name: Alt\n  values:\n    post_process_prompts:\n    - id: p1\n      name: Mail\n      prompt: Make an email of it.\n    - id: p2\n      name: Kurz\n      prompt: Make it short.\n    post_process_selected_prompt_id: p2\n";
        let mut prompts = Vec::new();
        let mut order = Vec::new();
        prepare_import(text, &mut prompts, &mut order);
        assert_eq!(
            prompts,
            vec![
                prompt("p1", "Mail", "Make an email of it."),
                prompt("p2", "Kurz", "Make it short.")
            ]
        );
        assert_eq!(order, vec![entry("p1", false), entry("p2", true)]);
        // A file with the library's own fields is left as it is.
        let mut kept = vec![prompt("p3", "Neu", "New.")];
        let mut kept_order = vec![entry("p3", true)];
        prepare_import(
            "format: danywhere-profile\nversion: 1\nprofile:\n  id: new\n  name: Neu\n  values:\n    post_process_prompt_order:\n    - id: p3\n      active: true\n",
            &mut kept,
            &mut kept_order,
        );
        assert_eq!(kept, vec![prompt("p3", "Neu", "New.")]);
        assert_eq!(kept_order, vec![entry("p3", true)]);
    }
}
