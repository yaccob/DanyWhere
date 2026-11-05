//! Named word lists. The global list (`custom_words`) always applies; at most
//! one named additional list is switched on in addition to it. The model gets
//! the additional list's words first, so they win when Whisper's prompt
//! budget is tight.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Emitter};

use crate::settings::{self, AppSettings};

/// How long the overlay shows which list the shortcut switched to.
const SWITCH_NOTICE: std::time::Duration = std::time::Duration::from_millis(2000);

/// A named list of words that can be switched on in addition to the global
/// list.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct WordList {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub words: Vec<String>,
}

/// The additional list in use, if any. An id that no longer names a list
/// (e.g. a hand-edited store) counts as none.
pub fn active_list(settings: &AppSettings) -> Option<&WordList> {
    let id = settings.active_word_list_id.as_deref()?;
    settings.word_lists.iter().find(|list| list.id == id)
}

/// The words the model gets: the additional list's first, then the global
/// list's, each once. transcribe-cpp drops terms from the end when Whisper's
/// prompt budget runs out, so the words switched on for the topic at hand win.
pub fn merged_vocabulary(settings: &AppSettings) -> Vec<String> {
    let additional = active_list(settings).map(|list| list.words.as_slice());
    let mut merged: Vec<String> = Vec::new();
    for word in additional
        .unwrap_or_default()
        .iter()
        .chain(&settings.custom_words)
    {
        if !merged.contains(word) {
            merged.push(word.clone());
        }
    }
    merged
}

/// The additional list the switch shortcut selects next: none → the first
/// list → … → the last list → none. From an id that names no list, it starts
/// at the first.
pub fn next_active_id(lists: &[WordList], current: Option<&str>) -> Option<String> {
    let next = match current.and_then(|id| lists.iter().position(|list| list.id == id)) {
        Some(index) => lists.get(index + 1),
        None => lists.first(),
    };
    next.map(|list| list.id.clone())
}

fn clean_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("A word list needs a name".to_string());
    }
    Ok(name.to_string())
}

fn list_mut<'a>(settings: &'a mut AppSettings, id: &str) -> Result<&'a mut WordList, String> {
    settings
        .word_lists
        .iter_mut()
        .find(|list| list.id == id)
        .ok_or_else(|| format!("Word list '{id}' not found"))
}

/// Add a new, empty list named `name`. It isn't switched on.
pub fn add_list(
    settings: &mut AppSettings,
    name: &str,
    now_millis: i64,
) -> Result<WordList, String> {
    let name = clean_name(name)?;
    let base = format!("word_list_{now_millis}");
    let mut id = base.clone();
    let mut suffix = 1;
    while settings.word_lists.iter().any(|list| list.id == id) {
        suffix += 1;
        id = format!("{base}_{suffix}");
    }
    let list = WordList {
        id,
        name,
        words: Vec::new(),
    };
    settings.word_lists.push(list.clone());
    Ok(list)
}

/// Rename list `id`.
pub fn rename_list(settings: &mut AppSettings, id: &str, name: &str) -> Result<(), String> {
    let name = clean_name(name)?;
    list_mut(settings, id)?.name = name;
    Ok(())
}

/// Replace the words of list `id`: trimmed, without blanks, each once.
pub fn set_words(settings: &mut AppSettings, id: &str, words: Vec<String>) -> Result<(), String> {
    let mut cleaned: Vec<String> = Vec::new();
    for word in words {
        let word = word.trim().to_string();
        if !word.is_empty() && !cleaned.contains(&word) {
            cleaned.push(word);
        }
    }
    list_mut(settings, id)?.words = cleaned;
    Ok(())
}

/// Delete list `id`. Deleting the active list leaves only the global list.
pub fn remove_list(settings: &mut AppSettings, id: &str) -> Result<(), String> {
    let before = settings.word_lists.len();
    settings.word_lists.retain(|list| list.id != id);
    if settings.word_lists.len() == before {
        return Err(format!("Word list '{id}' not found"));
    }
    if settings.active_word_list_id.as_deref() == Some(id) {
        settings.active_word_list_id = None;
    }
    Ok(())
}

/// Switch on list `id`, or only the global list with `None`.
pub fn set_active(settings: &mut AppSettings, id: Option<&str>) -> Result<(), String> {
    if let Some(id) = id {
        if !settings.word_lists.iter().any(|list| list.id == id) {
            return Err(format!("Word list '{id}' not found"));
        }
    }
    settings.active_word_list_id = id.map(str::to_string);
    Ok(())
}

/// Store `settings` and tell the settings window and the tray.
fn save(app: &AppHandle, settings: AppSettings) {
    settings::write_settings(app, settings);
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({ "setting": "word_lists" }),
    );
    crate::tray::rebuild_tray_menu(app);
}

/// Apply `change` to the stored settings and save them if it succeeds.
fn change<T>(
    app: &AppHandle,
    change: impl FnOnce(&mut AppSettings) -> Result<T, String>,
) -> Result<T, String> {
    let mut settings = settings::get_settings(app);
    let result = change(&mut settings)?;
    save(app, settings);
    Ok(result)
}

/// The switch shortcut: activate the next additional list and say which one
/// is now in use. While a recording or transcription owns the overlay, only
/// the settings and the tray change.
pub fn switch_to_next(app: &AppHandle) {
    let mut settings = settings::get_settings(app);
    settings.active_word_list_id = next_active_id(
        &settings.word_lists,
        settings.active_word_list_id.as_deref(),
    );
    let name = active_list(&settings).map(|list| list.name.clone());
    log::info!(
        "Word list switched to {}",
        name.as_deref().unwrap_or("the global list only")
    );
    save(app, settings);
    if !crate::overlay::is_session_showing() {
        crate::overlay::show_word_list_notice(app, name, SWITCH_NOTICE);
    }
}

/// Switch on list `id` (`None`: only the global list) from the tray.
pub fn select_from_tray(app: &AppHandle, id: Option<&str>) -> Result<(), String> {
    change(app, |settings| set_active(settings, id))
}

#[tauri::command]
#[specta::specta]
pub fn add_word_list(app: AppHandle, name: String) -> Result<WordList, String> {
    let now = chrono::Utc::now().timestamp_millis();
    change(&app, |settings| add_list(settings, &name, now))
}

#[tauri::command]
#[specta::specta]
pub fn rename_word_list(app: AppHandle, id: String, name: String) -> Result<(), String> {
    change(&app, |settings| rename_list(settings, &id, &name))
}

#[tauri::command]
#[specta::specta]
pub fn set_word_list_words(app: AppHandle, id: String, words: Vec<String>) -> Result<(), String> {
    change(&app, |settings| set_words(settings, &id, words))
}

#[tauri::command]
#[specta::specta]
pub fn delete_word_list(app: AppHandle, id: String) -> Result<(), String> {
    change(&app, |settings| remove_list(settings, &id))
}

#[tauri::command]
#[specta::specta]
pub fn set_active_word_list(app: AppHandle, id: Option<String>) -> Result<(), String> {
    change(&app, |settings| set_active(settings, id.as_deref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(id: &str, words: &[&str]) -> WordList {
        WordList {
            id: id.to_string(),
            name: id.to_uppercase(),
            words: words.iter().map(|w| w.to_string()).collect(),
        }
    }

    fn settings(global: &[&str], lists: Vec<WordList>, active: Option<&str>) -> AppSettings {
        AppSettings {
            custom_words: global.iter().map(|w| w.to_string()).collect(),
            word_lists: lists,
            active_word_list_id: active.map(str::to_string),
            ..Default::default()
        }
    }

    fn strings(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    // ---- What the model gets --------------------------------------------

    #[test]
    fn without_an_additional_list_the_model_gets_the_global_list() {
        let s = settings(
            &["Sayuri", "DanyWhere"],
            vec![list("work", &["Jira"])],
            None,
        );
        assert_eq!(merged_vocabulary(&s), strings(&["Sayuri", "DanyWhere"]));
        assert!(active_list(&s).is_none());
    }

    #[test]
    fn the_active_list_comes_first_then_the_global_list() {
        let s = settings(
            &["Sayuri", "DanyWhere"],
            vec![
                list("work", &["Jira", "Confluence"]),
                list("code", &["Rust"]),
            ],
            Some("work"),
        );
        assert_eq!(
            merged_vocabulary(&s),
            strings(&["Jira", "Confluence", "Sayuri", "DanyWhere"])
        );
        assert_eq!(active_list(&s).map(|l| l.id.as_str()), Some("work"));
    }

    #[test]
    fn each_word_goes_to_the_model_once() {
        let s = settings(
            &["Claude", "Sayuri", "Claude"],
            vec![list("code", &["Rust", "Claude", "Rust"])],
            Some("code"),
        );
        assert_eq!(
            merged_vocabulary(&s),
            strings(&["Rust", "Claude", "Sayuri"])
        );
    }

    #[test]
    fn an_unknown_active_list_means_only_the_global_list() {
        let s = settings(&["Sayuri"], vec![list("work", &["Jira"])], Some("gone"));
        assert_eq!(merged_vocabulary(&s), strings(&["Sayuri"]));
        assert!(active_list(&s).is_none());
    }

    // ---- The switch shortcut ----------------------------------------------

    #[test]
    fn switching_without_lists_stays_on_the_global_list() {
        assert_eq!(next_active_id(&[], None), None);
        assert_eq!(next_active_id(&[], Some("gone")), None);
    }

    #[test]
    fn switching_with_one_list_toggles_it() {
        let lists = [list("work", &[])];
        assert_eq!(next_active_id(&lists, None).as_deref(), Some("work"));
        assert_eq!(next_active_id(&lists, Some("work")), None);
    }

    #[test]
    fn switching_steps_through_the_lists_in_order_and_back_to_none() {
        let lists = [list("a", &[]), list("b", &[]), list("c", &[])];
        assert_eq!(next_active_id(&lists, None).as_deref(), Some("a"));
        assert_eq!(next_active_id(&lists, Some("a")).as_deref(), Some("b"));
        assert_eq!(next_active_id(&lists, Some("b")).as_deref(), Some("c"));
        assert_eq!(next_active_id(&lists, Some("c")), None);
    }

    #[test]
    fn switching_from_an_unknown_list_starts_at_the_first() {
        let lists = [list("a", &[]), list("b", &[])];
        assert_eq!(next_active_id(&lists, Some("gone")).as_deref(), Some("a"));
    }

    // ---- Managing the lists -----------------------------------------------

    #[test]
    fn a_new_list_is_empty_with_a_unique_id_and_a_trimmed_name() {
        let mut s = settings(&[], vec![], None);
        let first = add_list(&mut s, "  Work  ", 1_000).unwrap();
        let second = add_list(&mut s, "Code", 1_000).unwrap();
        assert_eq!(first.name, "Work");
        assert!(first.words.is_empty());
        assert_ne!(first.id, second.id);
        assert_eq!(s.word_lists, vec![first, second]);
        assert_eq!(s.active_word_list_id, None, "adding doesn't switch it on");
    }

    #[test]
    fn a_list_needs_a_name() {
        let mut s = settings(&[], vec![list("a", &[])], None);
        assert!(add_list(&mut s, "   ", 1).is_err());
        assert!(rename_list(&mut s, "a", "").is_err());
        assert_eq!(s.word_lists.len(), 1);
    }

    #[test]
    fn renaming_and_editing_a_list() {
        let mut s = settings(&[], vec![list("a", &["x"])], None);
        rename_list(&mut s, "a", " Arbeit ").unwrap();
        set_words(&mut s, "a", strings(&["Jira", "Jira", " ", "Confluence"])).unwrap();
        assert_eq!(s.word_lists[0].name, "Arbeit");
        assert_eq!(s.word_lists[0].words, strings(&["Jira", "Confluence"]));
        assert!(rename_list(&mut s, "gone", "X").is_err());
        assert!(set_words(&mut s, "gone", vec![]).is_err());
    }

    #[test]
    fn deleting_the_active_list_leaves_only_the_global_list() {
        let mut s = settings(&["g"], vec![list("a", &[]), list("b", &[])], Some("b"));
        remove_list(&mut s, "b").unwrap();
        assert_eq!(s.active_word_list_id, None);
        assert_eq!(s.word_lists, vec![list("a", &[])]);
    }

    #[test]
    fn deleting_another_list_keeps_the_active_one() {
        let mut s = settings(&["g"], vec![list("a", &[]), list("b", &[])], Some("b"));
        remove_list(&mut s, "a").unwrap();
        assert_eq!(s.active_word_list_id.as_deref(), Some("b"));
        assert!(remove_list(&mut s, "gone").is_err());
    }

    #[test]
    fn only_an_existing_list_can_be_switched_on() {
        let mut s = settings(&[], vec![list("a", &[])], None);
        set_active(&mut s, Some("a")).unwrap();
        assert_eq!(s.active_word_list_id.as_deref(), Some("a"));
        assert!(set_active(&mut s, Some("gone")).is_err());
        assert_eq!(s.active_word_list_id.as_deref(), Some("a"));
        set_active(&mut s, None).unwrap();
        assert_eq!(s.active_word_list_id, None);
    }
}
