//! DanyWhere: what the end of a dictation runs with, read once when its
//! recording ends.
//!
//! The code after a recording used to read the settings again in almost
//! every step: the stored ones through the `AppHandle`, with the values of
//! the dictation's own profile over them (`app_profiles::dictation_settings`).
//! A change the user made while the dictation was transcribed (a setting, or
//! another profile put in use) then applied to the rest of it from whichever
//! step read next, so one dictation could end with a mix of two setups. The
//! snapshot is taken once, right after the profile was chosen again by the
//! application in front (`app_profiles::settle_dictation`), and the steps
//! after it read the snapshot. A change made meanwhile applies from the next
//! dictation on.
//!
//! [`build`] makes it from what was read and needs no app, so tests build it
//! from fixed settings; [`take`] reads those inputs through the `AppHandle`.
//!
//! Helpers the end calls still read the settings themselves, through
//! `app_profiles::dictation_settings`: the stop sound, the overlay and the
//! transcription manager get the values of a profile of the dictation's own
//! as the snapshot has them, but those of the profile in use as they are
//! when they read. On the paste thread, which keeps the snapshot's values
//! (`app_profiles::keep_values_here`), the paste and the sounds and notices
//! after it get the snapshot's profile values; only values no profile holds
//! (e.g. the paste delays) come from when they read.

use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::end_keys::{EndChoice, EndKey};
use crate::managers::audio::AudioRecordingManager;
use crate::managers::transcription::TranscriptionManager;
use crate::profiles::ProfileValues;
use crate::settings::{AppSettings, OverlayStyle};

/// What the end of one dictation runs with.
#[derive(Debug, Clone)]
pub struct DictationSnapshot {
    /// The stored settings with the values of the dictation's profile in
    /// their profile fields (`app_profiles::with_values`). Never store these.
    pub settings: AppSettings,
    /// The profile the dictation ends with: its own, or the one in use.
    pub profile_id: String,
    /// That profile's values.
    pub values: ProfileValues,
    /// The end it takes in that profile (`end_keys::resolve`): its "Append"
    /// text, whether it post-processes and whether it sends context.
    pub end: EndKey,
    /// Whether it post-processes: the end asks for it and, for a profile of
    /// its own (not the one in use), that profile's switch allows it
    /// (`app_profiles::effective_post_process`).
    pub post_process: bool,
    /// Whether earlier dictations go along as context: only when it
    /// post-processes.
    pub with_context: bool,
    /// The recording is transcribed in one go with the end profile's values
    /// (`app_profiles::EndChange::transcription_differs`).
    pub transcribed_with_end_values: bool,
    /// The live overlay shows the working states: style "Live" while a
    /// stream runs, read before the stream is finalized, so every later
    /// working state targets the overlay that was shown.
    pub use_streaming_overlay: bool,
    /// The dictation's recording session (`actions::current_recording_session`);
    /// a newer one owns the overlay and the tray.
    pub session: u64,
    /// The cancel generation when the recording ended; a cancel after it
    /// cancels this dictation (`AudioRecordingManager::was_cancelled_since`).
    pub cancel_generation: u64,
}

impl DictationSnapshot {
    /// What the dictation saved as History entry `entry_id` was made with,
    /// for redoing it elsewhere (`profile_redo`).
    pub fn made_with(&self, entry_id: i64) -> crate::profile_redo::MadeWith {
        crate::profile_redo::MadeWith {
            entry_id,
            profile_id: self.profile_id.clone(),
            values: self.values.clone(),
            end: self.end.clone(),
        }
    }
}

/// What a snapshot is built from, as read when the recording ends.
pub struct SnapshotInputs<'a> {
    /// The stored settings (`settings::get_settings`).
    pub stored: &'a AppSettings,
    /// The dictation's own profile (id and values) when it isn't the one in
    /// use (`app_profiles::own_dictation_profile`), after `settle_dictation`.
    pub own_profile: Option<(String, ProfileValues)>,
    /// How the recording ended.
    pub end: &'a EndChoice,
    /// The start shortcut that started the recording.
    pub binding_id: &'a str,
    /// What `settle_dictation` said about the transcription.
    pub transcribed_with_end_values: bool,
    /// Whether a live stream runs (`TranscriptionManager::is_streaming`).
    pub streaming: bool,
    pub session: u64,
    pub cancel_generation: u64,
}

/// The snapshot `inputs` give. No app, no global state.
pub fn build(inputs: SnapshotInputs<'_>) -> DictationSnapshot {
    let own_values = inputs.own_profile.as_ref().map(|(_, values)| values);
    let settings = crate::app_profiles::with_values(inputs.stored, own_values);
    // The end shortcut it ended with, in the profile it ends with: its
    // append text, whether it post-processes and whether earlier dictations
    // go along. A profile of its own decides with its own switch whether to
    // post-process (`app_profiles::effective_post_process`).
    let (profile_id, values) = match &inputs.own_profile {
        Some((id, values)) => (id.clone(), values.clone()),
        None => (
            inputs.stored.active_profile_id.clone(),
            ProfileValues::from_settings(inputs.stored),
        ),
    };
    let end = crate::end_keys::resolve(&values, inputs.end, inputs.binding_id);
    let post_process = crate::app_profiles::effective_post_process(end.post_process, own_values);
    DictationSnapshot {
        use_streaming_overlay: should_use_streaming_overlay(
            settings.overlay_style,
            inputs.streaming,
        ),
        settings,
        profile_id,
        values,
        with_context: post_process && end.send_context,
        post_process,
        end,
        transcribed_with_end_values: inputs.transcribed_with_end_values,
        session: inputs.session,
        cancel_generation: inputs.cancel_generation,
    }
}

/// Whether the overlay of `style` shows a stream's working states.
fn should_use_streaming_overlay(style: OverlayStyle, is_streaming: bool) -> bool {
    style == OverlayStyle::Live && is_streaming
}

/// Takes the snapshot of the dictation whose recording `binding_id` ended as
/// `end`: reads what [`build`] needs through `app`. Call it right after
/// `app_profiles::settle_dictation`, which says whether the recording is
/// `transcribed_with_end_values`.
pub fn take(
    app: &AppHandle,
    end: &EndChoice,
    binding_id: &str,
    transcribed_with_end_values: bool,
) -> DictationSnapshot {
    let stored = crate::settings::get_settings(app);
    build(SnapshotInputs {
        stored: &stored,
        own_profile: crate::app_profiles::own_dictation_profile(),
        end,
        binding_id,
        transcribed_with_end_values,
        streaming: app.state::<Arc<TranscriptionManager>>().is_streaming(),
        session: crate::actions::current_recording_session(),
        cancel_generation: app
            .state::<Arc<AudioRecordingManager>>()
            .cancel_generation(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::end_keys::START_BINDINGS;
    use crate::profiles::{ensure_profiles, DEFAULT_PROFILE_ID};

    fn end_key(id: &str, append_text: &str, post_process: bool, send_context: bool) -> EndKey {
        EndKey {
            id: id.to_string(),
            name: String::new(),
            shortcut: format!("ctrl+option+{}", &id[..1]),
            append_text: append_text.to_string(),
            post_process,
            send_context,
        }
    }

    /// The defaults in use (Parakeet, no post-processing, the small
    /// overlay) and "mail" (Whisper, post-processing on, the live overlay)
    /// with two ends of its own: "send" appends a line break, post-processes
    /// and sends context; "quote" sends context without post-processing.
    fn with_mail_profile() -> AppSettings {
        let mut settings = AppSettings {
            selected_model: "parakeet".to_string(),
            post_process_enabled: false,
            overlay_style: OverlayStyle::Minimal,
            ..Default::default()
        };
        ensure_profiles(&mut settings);
        let mut mail = crate::profiles::profile_for_test(&settings, "mail", "Mail", |values| {
            values.selected_model = "whisper".to_string();
            values.post_process_enabled = true;
            values.overlay_style = OverlayStyle::Live;
            values.end_keys.push(end_key("send", "\n", true, true));
            values
                .end_keys
                .push(end_key("quote", " (quoted)", false, true));
        });
        mail.apps = vec!["com.apple.mail".to_string()];
        settings.profiles.push(mail);
        settings
    }

    fn mail_values(settings: &AppSettings) -> ProfileValues {
        crate::profiles::values_of(settings, "mail").expect("mail")
    }

    /// A dictation started with the first start shortcut, ending as `end`,
    /// with `own` as its profile of its own (`None`: the one in use).
    fn snapshot(
        stored: &AppSettings,
        own: Option<ProfileValues>,
        end: EndChoice,
    ) -> DictationSnapshot {
        build(SnapshotInputs {
            stored,
            own_profile: own.map(|values| ("mail".to_string(), values)),
            end: &end,
            binding_id: START_BINDINGS[0],
            transcribed_with_end_values: false,
            streaming: false,
            session: 1,
            cancel_generation: 0,
        })
    }

    #[test]
    fn the_snapshot_holds_the_dictation_profiles_values_not_those_in_use() {
        let stored = with_mail_profile();
        let mail = snapshot(
            &stored,
            Some(mail_values(&stored)),
            EndChoice::Start(START_BINDINGS[0].to_string()),
        );
        assert_eq!(mail.profile_id, "mail");
        assert_eq!(mail.values.selected_model, "whisper");
        assert_eq!(mail.settings.selected_model, "whisper");
        assert_eq!(mail.settings.overlay_style, OverlayStyle::Live);
        // The profile in use and the stored profiles stay as they were.
        assert_eq!(mail.settings.active_profile_id, DEFAULT_PROFILE_ID);
        assert_eq!(mail.settings.profiles.len(), stored.profiles.len());

        let in_use = snapshot(
            &stored,
            None,
            EndChoice::Start(START_BINDINGS[0].to_string()),
        );
        assert_eq!(in_use.profile_id, DEFAULT_PROFILE_ID);
        assert_eq!(in_use.values.selected_model, "parakeet");
        assert_eq!(in_use.settings.selected_model, "parakeet");
    }

    #[test]
    fn the_end_shortcut_brings_its_append_text_post_processing_and_context() {
        let stored = with_mail_profile();
        let sent = snapshot(
            &stored,
            Some(mail_values(&stored)),
            EndChoice::Key("send".to_string()),
        );
        assert_eq!(sent.end.id, "send");
        assert_eq!(sent.end.append_text, "\n");
        assert!(sent.post_process);
        assert!(sent.with_context);
    }

    #[test]
    fn a_profile_of_its_own_with_post_processing_off_does_not_post_process() {
        let stored = with_mail_profile();
        let mut mail = mail_values(&stored);
        mail.post_process_enabled = false;
        let sent = snapshot(&stored, Some(mail), EndChoice::Key("send".to_string()));
        assert_eq!(sent.end.id, "send");
        assert!(!sent.post_process);
        assert!(!sent.with_context);
        // The profile in use: its end decides alone, as before
        // (`app_profiles::effective_post_process`).
        assert!(!stored.post_process_enabled);
        let in_use = snapshot(
            &stored,
            None,
            EndChoice::Key(crate::end_keys::POST_PROCESS_ID.to_string()),
        );
        assert_eq!(in_use.end.id, crate::end_keys::POST_PROCESS_ID);
        assert!(in_use.post_process);
    }

    #[test]
    fn context_goes_along_only_with_post_processing() {
        let stored = with_mail_profile();
        let quoted = snapshot(
            &stored,
            Some(mail_values(&stored)),
            EndChoice::Key("quote".to_string()),
        );
        assert_eq!(quoted.end.id, "quote");
        assert!(quoted.end.send_context);
        assert!(!quoted.post_process);
        assert!(!quoted.with_context);
    }

    #[test]
    fn what_was_read_when_the_recording_ended_is_kept() {
        let stored = with_mail_profile();
        let end = EndChoice::Start(START_BINDINGS[0].to_string());
        let read = |own: Option<ProfileValues>, streaming: bool| {
            build(SnapshotInputs {
                stored: &stored,
                own_profile: own.map(|values| ("mail".to_string(), values)),
                end: &end,
                binding_id: START_BINDINGS[0],
                transcribed_with_end_values: true,
                streaming,
                session: 7,
                cancel_generation: 3,
            })
        };
        let live = read(Some(mail_values(&stored)), true);
        assert!(live.transcribed_with_end_values);
        assert_eq!(live.session, 7);
        assert_eq!(live.cancel_generation, 3);
        // The dictation profile's live overlay while a stream runs.
        assert!(live.use_streaming_overlay);
        assert!(!read(Some(mail_values(&stored)), false).use_streaming_overlay);
        // The small overlay of the profile in use shows no stream.
        assert!(!read(None, true).use_streaming_overlay);
    }

    #[test]
    fn a_saved_dictation_remembers_its_profile_and_end() {
        let stored = with_mail_profile();
        let sent = snapshot(
            &stored,
            Some(mail_values(&stored)),
            EndChoice::Key("send".to_string()),
        );
        let made = sent.made_with(42);
        assert_eq!(made.entry_id, 42);
        assert_eq!(made.profile_id, "mail");
        assert_eq!(made.values, mail_values(&stored));
        assert_eq!(made.end.id, "send");
    }

    #[test]
    fn live_overlay_uses_streaming_states_only_for_streaming_models() {
        assert!(should_use_streaming_overlay(OverlayStyle::Live, true));
        assert!(!should_use_streaming_overlay(OverlayStyle::Live, false));
        assert!(!should_use_streaming_overlay(OverlayStyle::Minimal, true));
        assert!(!should_use_streaming_overlay(OverlayStyle::None, true));
    }
}
