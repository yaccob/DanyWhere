//! DanyWhere: where the recording overlay sits and whether it shows, for the
//! dictation it shows. Position and style are profile values (P6), so a
//! dictation into an application with a profile of its own shows its pills
//! as that profile says (`app_profiles`).
//!
//! The look is taken when the dictation names its profile in the overlay
//! (`overlay::set_dictation_profile`, at its start and again when its
//! recording ends in another profile's application) and kept until the
//! overlay hides, so the notices after the dictation ("on the clipboard", a
//! failed post-processing) appear where its pills were, although the
//! dictation's profile ends before them. Everything else the overlay shows
//! (a switched profile or word list) follows the settings in use.

use std::sync::Mutex;

use tauri::AppHandle;

use crate::settings::{AppSettings, OverlayPosition, OverlayStyle};

/// Where the overlay sits and how it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayLook {
    pub position: OverlayPosition,
    pub style: OverlayStyle,
}

impl OverlayLook {
    /// The look `settings` give.
    pub fn of(settings: &AppSettings) -> Self {
        Self {
            position: settings.overlay_position,
            style: settings.overlay_style,
        }
    }
}

/// The look of the dictation the overlay shows; `None`: the settings in use.
static SESSION_LOOK: Mutex<Option<OverlayLook>> = Mutex::new(None);

fn session_look() -> Option<OverlayLook> {
    *SESSION_LOOK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn set_session_look(look: Option<OverlayLook>) {
    *SESSION_LOOK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = look;
}

/// The look in effect: the dictation's (`session`) while the overlay shows
/// one, else the settings in use (`in_use`, read only then).
pub fn look_in_effect(
    session: Option<OverlayLook>,
    in_use: impl FnOnce() -> OverlayLook,
) -> OverlayLook {
    session.unwrap_or_else(in_use)
}

/// Whether the overlay shows: by the dictation's style while one applies,
/// else as the settings in use say (`in_use`, the overlay's cached flag).
pub fn enabled_in_effect(session: Option<OverlayLook>, in_use: bool) -> bool {
    session.map_or(in_use, |look| look.style != OverlayStyle::None)
}

/// The dictation the overlay shows now has the values of `settings` (its
/// profile's): its pills take their look.
pub fn begin_session(settings: &AppSettings) {
    set_session_look(Some(OverlayLook::of(settings)));
}

/// The overlay hid: what it shows next follows the settings in use, until a
/// dictation names its profile again.
pub fn end_session() {
    set_session_look(None);
}

/// The look in effect now.
pub fn current(app: &AppHandle) -> OverlayLook {
    look_in_effect(session_look(), || {
        OverlayLook::of(&crate::settings::get_settings(app))
    })
}

/// Whether the overlay shows now; `in_use` is the overlay's cached flag for
/// the settings in use.
pub fn enabled(in_use: bool) -> bool {
    enabled_in_effect(session_look(), in_use)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(position: OverlayPosition, style: OverlayStyle) -> OverlayLook {
        OverlayLook { position, style }
    }

    #[test]
    fn a_dictations_look_wins_while_the_overlay_shows_it() {
        let dictation = look(OverlayPosition::Top, OverlayStyle::Minimal);
        let in_use = look(OverlayPosition::Bottom, OverlayStyle::Live);
        assert_eq!(look_in_effect(Some(dictation), || in_use), dictation);
        assert_eq!(look_in_effect(None, || in_use), in_use);
    }

    #[test]
    fn a_dictation_with_the_overlay_shows_it_even_when_the_profile_in_use_hides_it() {
        let shown = look(OverlayPosition::Bottom, OverlayStyle::Live);
        let hidden = look(OverlayPosition::Bottom, OverlayStyle::None);
        assert!(enabled_in_effect(Some(shown), false));
        assert!(!enabled_in_effect(Some(hidden), true));
        assert!(enabled_in_effect(None, true));
        assert!(!enabled_in_effect(None, false));
    }

    #[test]
    fn the_look_is_taken_from_the_settings() {
        let settings = AppSettings {
            overlay_position: OverlayPosition::Top,
            overlay_style: OverlayStyle::Minimal,
            ..Default::default()
        };
        assert_eq!(
            OverlayLook::of(&settings),
            look(OverlayPosition::Top, OverlayStyle::Minimal)
        );
    }
}
