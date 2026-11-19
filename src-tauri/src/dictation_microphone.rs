//! DanyWhere: the microphone a recording opens. The microphone, its channel
//! and the headset option are profile values (P6), so a dictation into an
//! application with a profile of its own records with that profile's
//! microphone (`app_profiles`), without switching the profile in use.
//!
//! The audio manager remembers what its stream was opened with; a recording
//! that wants another microphone opens the stream again for itself, as an
//! on-demand stream would open anyway. With the microphone always on, the
//! stream then stays with that microphone until a recording or the profile
//! in use wants another (`profile_effects`): no stream is reopened behind
//! the user's back while the next dictation may already be starting.

use crate::profiles::ProfileValues;
use crate::settings::AppSettings;

/// What a microphone stream is opened with, as the settings name it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicrophoneChoice {
    /// The selected microphone's name; `None`: the system's default.
    pub selected: Option<String>,
    /// The microphone used while the lid is closed, if one is set.
    pub clamshell: Option<String>,
    /// The input channel; `None`: all channels averaged.
    pub channel: Option<u16>,
    /// Move a Bluetooth headset to this Mac while recording.
    pub take_headset: bool,
}

/// The settings a stream resolves its device with and what it is opened
/// with: those of the dictation's own profile when one applies
/// (`dictation`), else the settings in use (`settings`) with the channel and
/// headset option the audio manager holds (`channel`, `take_headset`),
/// which a command applies before it stores them.
pub fn choose(
    settings: &AppSettings,
    channel: Option<u16>,
    take_headset: bool,
    dictation: Option<&ProfileValues>,
) -> (AppSettings, MicrophoneChoice) {
    let (channel, take_headset) = match dictation {
        Some(values) => (values.selected_channel, values.airpods_mode),
        None => (channel, take_headset),
    };
    let settings = crate::app_profiles::with_values(settings, dictation);
    let choice = MicrophoneChoice {
        selected: settings.selected_microphone.clone(),
        clamshell: settings.clamshell_microphone.clone(),
        channel,
        take_headset,
    };
    (settings, choice)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::testing::values_with;

    fn settings() -> AppSettings {
        AppSettings {
            selected_microphone: Some("MacBook Pro Microphone".to_string()),
            clamshell_microphone: None,
            selected_channel: Some(0),
            airpods_mode: false,
            ..Default::default()
        }
    }

    #[test]
    fn without_a_dictation_profile_the_settings_in_use_apply() {
        let s = settings();
        // The manager's channel wins: a command applies it before storing.
        let (resolved, choice) = choose(&s, Some(2), true, None);
        assert_eq!(resolved.selected_microphone, s.selected_microphone);
        assert_eq!(
            choice,
            MicrophoneChoice {
                selected: Some("MacBook Pro Microphone".to_string()),
                clamshell: None,
                channel: Some(2),
                take_headset: true,
            }
        );
    }

    #[test]
    fn a_dictation_profile_brings_its_own_microphone() {
        let s = settings();
        let mail = values_with(
            &crate::profiles::ProfileValues::from_settings(&s),
            serde_json::json!({
                "selected_microphone": "AirPods",
                "clamshell_microphone": "Desk Mic",
                "selected_channel": null,
                "airpods_mode": true,
            }),
        );
        let (resolved, choice) = choose(&s, Some(0), false, Some(&mail));
        assert_eq!(resolved.selected_microphone.as_deref(), Some("AirPods"));
        assert_eq!(resolved.clamshell_microphone.as_deref(), Some("Desk Mic"));
        assert_eq!(
            choice,
            MicrophoneChoice {
                selected: Some("AirPods".to_string()),
                clamshell: Some("Desk Mic".to_string()),
                channel: None,
                take_headset: true,
            }
        );
    }

    #[test]
    fn the_same_microphone_needs_no_reopen() {
        let s = settings();
        let same = crate::profiles::ProfileValues::from_settings(&s);
        let (_, with_profile) = choose(&s, s.selected_channel, s.airpods_mode, Some(&same));
        let (_, without) = choose(&s, s.selected_channel, s.airpods_mode, None);
        assert_eq!(with_profile, without);
    }
}
