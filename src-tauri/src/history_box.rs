//! DanyWhere: how tall a History text box may grow before it scrolls.
//!
//! A box is as tall as its text up to a maximum: 15 lines for the newest
//! entry, 4 for the others (`historyBox.ts`). Dragging a box's grip sets
//! the maximum for its kind of entry, so every box of that kind follows; a
//! double-click on the grip puts the kind back to its default. The heights
//! are kept per kind, never per entry.

use crate::settings::{get_settings, write_settings, AppSettings};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;

/// The two kinds of entry that have a height of their own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum HistoryBoxKind {
    /// The first entry of the list shown.
    Newest,
    /// Every other entry.
    Other,
}

/// The smallest height stored, in pixels. The frontend keeps a box at least
/// two lines tall; this only guards against a hand-edited value.
pub const MIN_HEIGHT_PX: u32 = 16;
/// The largest height stored, in pixels.
pub const MAX_HEIGHT_PX: u32 = 4000;

/// Sets the dragged height of `kind`, in pixels; `None` = its default lines.
/// A height outside [`MIN_HEIGHT_PX`]..=[`MAX_HEIGHT_PX`] is stored as the
/// nearest bound.
pub fn set_box_height(settings: &mut AppSettings, kind: HistoryBoxKind, height: Option<u32>) {
    let height = height.map(|px| px.clamp(MIN_HEIGHT_PX, MAX_HEIGHT_PX));
    match kind {
        HistoryBoxKind::Newest => settings.history_box_height_newest = height,
        HistoryBoxKind::Other => settings.history_box_height_other = height,
    }
}

/// Sets the height a History text box of `kind` may grow to, in pixels;
/// `None` puts the kind back to its default.
#[tauri::command]
#[specta::specta]
pub fn change_history_box_height_setting(
    app: AppHandle,
    kind: HistoryBoxKind,
    height: Option<u32>,
) -> Result<(), String> {
    let mut settings = get_settings(&app);
    set_box_height(&mut settings, kind, height);
    write_settings(&app, settings);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::get_default_settings;

    #[test]
    fn new_and_older_settings_have_no_dragged_height() {
        let fresh = get_default_settings();
        assert_eq!(fresh.history_box_height_newest, None);
        assert_eq!(fresh.history_box_height_other, None);

        let mut stored = serde_json::to_value(get_default_settings()).unwrap();
        let object = stored.as_object_mut().unwrap();
        object.remove("history_box_height_newest");
        object.remove("history_box_height_other");
        let older: AppSettings = serde_json::from_value(stored).unwrap();

        assert_eq!(older.history_box_height_newest, None);
        assert_eq!(older.history_box_height_other, None);
    }

    #[test]
    fn a_dragged_height_is_stored_and_read_back() {
        let mut settings = get_default_settings();
        set_box_height(&mut settings, HistoryBoxKind::Other, Some(240));

        let stored = serde_json::to_value(&settings).unwrap();
        assert_eq!(stored["history_box_height_other"], 240);
        let read: AppSettings = serde_json::from_value(stored).unwrap();
        assert_eq!(read.history_box_height_other, Some(240));
    }

    #[test]
    fn each_kind_has_its_own_height() {
        let mut settings = get_default_settings();
        set_box_height(&mut settings, HistoryBoxKind::Newest, Some(300));
        set_box_height(&mut settings, HistoryBoxKind::Other, Some(90));

        assert_eq!(settings.history_box_height_newest, Some(300));
        assert_eq!(settings.history_box_height_other, Some(90));

        set_box_height(&mut settings, HistoryBoxKind::Other, Some(120));
        assert_eq!(settings.history_box_height_newest, Some(300));
        assert_eq!(settings.history_box_height_other, Some(120));
    }

    #[test]
    fn none_puts_only_that_kind_back_to_its_default() {
        let mut settings = get_default_settings();
        set_box_height(&mut settings, HistoryBoxKind::Newest, Some(300));
        set_box_height(&mut settings, HistoryBoxKind::Other, Some(90));

        set_box_height(&mut settings, HistoryBoxKind::Newest, None);

        assert_eq!(settings.history_box_height_newest, None);
        assert_eq!(settings.history_box_height_other, Some(90));
    }

    #[test]
    fn a_height_out_of_bounds_is_stored_as_the_nearest_bound() {
        let mut settings = get_default_settings();
        set_box_height(&mut settings, HistoryBoxKind::Newest, Some(0));
        set_box_height(&mut settings, HistoryBoxKind::Other, Some(1_000_000));

        assert_eq!(settings.history_box_height_newest, Some(MIN_HEIGHT_PX));
        assert_eq!(settings.history_box_height_other, Some(MAX_HEIGHT_PX));
    }

    #[test]
    fn the_kinds_reach_the_frontend_in_snake_case() {
        assert_eq!(
            serde_json::to_value(HistoryBoxKind::Newest).unwrap(),
            "newest"
        );
        assert_eq!(
            serde_json::to_value(HistoryBoxKind::Other).unwrap(),
            "other"
        );
    }
}
