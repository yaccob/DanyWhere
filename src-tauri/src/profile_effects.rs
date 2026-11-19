//! DanyWhere: what a change of the profile values in use needs beyond
//! storing them (contract V4 of the plan of 2026-11-19).
//!
//! The settings commands do it for a value the user changes: the microphone
//! stream is reopened, the overlay moves or comes on. The same values change
//! when another profile comes into use (a switch, `profiles::after_switch`)
//! or a default a profile follows changes; [`apply_profile_value_effects`]
//! does then what the commands do, for every value that differs between the
//! settings before and after.
//!
//! Values a dictation reads when it runs (sounds, paste method, VAD, the
//! post-processing provider and key …) need nothing here. A dictation with
//! another profile than the one in use reads its own values without any of
//! this (`app_profiles::dictation_settings`); its microphone is opened for
//! its recording (`AudioRecordingManager::start_microphone_stream`).

use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::managers::audio::{AudioRecordingManager, MicrophoneMode};
use crate::settings::{AppSettings, OverlayStyle};

/// What a change of the values in use needs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProfileValueEffects {
    /// The microphone stays open between dictations (`true`) or not; set
    /// when it changed.
    pub microphone_mode: Option<bool>,
    /// The microphone, its channel or the headset option changed: the
    /// stream, if open, is opened again with them.
    pub microphone: bool,
    /// The overlay is shown (`true`) or not; set when the style changed.
    pub overlay_enabled: Option<bool>,
    /// The overlay is placed again (another position or style).
    pub place_overlay: bool,
}

impl ProfileValueEffects {
    /// Whether nothing needs doing.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// What the change from `before` to `after` needs.
pub fn effects_of(before: &AppSettings, after: &AppSettings) -> ProfileValueEffects {
    let style_changed = before.overlay_style != after.overlay_style;
    ProfileValueEffects {
        microphone_mode: (before.always_on_microphone != after.always_on_microphone)
            .then_some(after.always_on_microphone),
        microphone: before.selected_microphone != after.selected_microphone
            || before.clamshell_microphone != after.clamshell_microphone
            || before.selected_channel != after.selected_channel
            || before.airpods_mode != after.airpods_mode,
        overlay_enabled: style_changed.then_some(overlay_shown(after.overlay_style)),
        place_overlay: style_changed || before.overlay_position != after.overlay_position,
    }
}

/// Does what the change of the values in use from `before` to `after`
/// needs. The microphone is reopened on a thread of its own, since opening
/// a device can take seconds; a dictation is never running then (a profile
/// switch waits for it).
pub fn apply_profile_value_effects(app: &AppHandle, before: &AppSettings, after: &AppSettings) {
    let effects = effects_of(before, after);
    if effects.is_empty() {
        return;
    }
    log::debug!("Profile values in use changed: {effects:?}");
    if let Some(enabled) = effects.overlay_enabled {
        crate::overlay::update_overlay_enabled_cache(enabled);
    }
    if effects.place_overlay {
        crate::utils::update_overlay_position(app);
    }
    if effects.microphone_mode.is_none() && !effects.microphone {
        return;
    }
    let rm = app.state::<Arc<AudioRecordingManager>>().inner().clone();
    let (channel, take_headset) = (after.selected_channel, after.airpods_mode);
    let spawned = std::thread::Builder::new()
        .name("profile-microphone".into())
        .spawn(move || {
            // Closing first and opening last opens the stream only once.
            if effects.microphone_mode == Some(false) {
                if let Err(e) = rm.update_mode(MicrophoneMode::OnDemand) {
                    log::warn!("Profile switch: microphone mode not changed: {e}");
                }
            }
            if effects.microphone {
                if let Err(e) = rm.use_profile_microphone(channel, take_headset) {
                    log::warn!("Profile switch: microphone not reopened: {e}");
                }
            }
            if effects.microphone_mode == Some(true) {
                if let Err(e) = rm.update_mode(MicrophoneMode::AlwaysOn) {
                    log::warn!("Profile switch: microphone mode not changed: {e}");
                }
            }
        });
    if let Err(e) = spawned {
        log::warn!("Profile switch: could not start the microphone thread: {e}");
    }
}

/// Whether the overlay is shown with `style`.
fn overlay_shown(style: OverlayStyle) -> bool {
    style != OverlayStyle::None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::OverlayPosition;

    fn settings() -> AppSettings {
        AppSettings::default()
    }

    #[test]
    fn the_same_values_need_nothing() {
        let before = settings();
        assert!(effects_of(&before, &before.clone()).is_empty());
    }

    #[test]
    fn values_a_dictation_reads_when_it_runs_need_nothing() {
        let before = settings();
        let mut after = before.clone();
        after.audio_feedback = !before.audio_feedback;
        after.mute_while_recording = !before.mute_while_recording;
        after.vad_enabled = !before.vad_enabled;
        after.custom_words = vec!["Sayuri".to_string()];
        after.post_process_timeout_secs += 10;
        after.selected_output_device = Some("Speakers".to_string());
        assert!(effects_of(&before, &after).is_empty());
    }

    #[test]
    fn another_microphone_reopens_the_stream() {
        let before = settings();
        let changes: [fn(&mut AppSettings); 4] = [
            |s| s.selected_microphone = Some("USB".to_string()),
            |s| s.clamshell_microphone = Some("Desk".to_string()),
            |s| s.selected_channel = Some(1),
            |s| s.airpods_mode = true,
        ];
        for change in changes {
            let mut after = before.clone();
            change(&mut after);
            assert_eq!(
                effects_of(&before, &after),
                ProfileValueEffects {
                    microphone: true,
                    ..Default::default()
                }
            );
        }
    }

    #[test]
    fn the_microphone_mode_follows_the_profile() {
        let before = settings();
        let mut after = before.clone();
        after.always_on_microphone = !before.always_on_microphone;
        assert_eq!(
            effects_of(&before, &after).microphone_mode,
            Some(after.always_on_microphone)
        );
        assert!(!effects_of(&before, &after).microphone);
    }

    #[test]
    fn the_overlay_comes_on_or_off_and_moves_with_its_style() {
        let mut before = settings();
        before.overlay_style = OverlayStyle::None;
        let mut after = before.clone();
        after.overlay_style = OverlayStyle::Live;
        assert_eq!(
            effects_of(&before, &after),
            ProfileValueEffects {
                overlay_enabled: Some(true),
                place_overlay: true,
                ..Default::default()
            }
        );
        // Minimal and Live are both shown: placed again, still enabled.
        let mut minimal = after.clone();
        minimal.overlay_style = OverlayStyle::Minimal;
        assert_eq!(
            effects_of(&after, &minimal),
            ProfileValueEffects {
                overlay_enabled: Some(true),
                place_overlay: true,
                ..Default::default()
            }
        );
        assert_eq!(
            effects_of(&after, &before).overlay_enabled,
            Some(overlay_shown(OverlayStyle::None))
        );
    }

    #[test]
    fn another_overlay_position_moves_the_overlay() {
        let mut before = settings();
        before.overlay_position = OverlayPosition::Bottom;
        let mut after = before.clone();
        after.overlay_position = OverlayPosition::Top;
        assert_eq!(
            effects_of(&before, &after),
            ProfileValueEffects {
                place_overlay: true,
                ..Default::default()
            }
        );
    }
}
