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
    /// text and whether it sends context.
    pub end: EndKey,
    /// Whether it post-processes ([`wants_post_processing`]): after an
    /// instruction key, or always, by the mode of the profile it ends with.
    pub post_process: bool,
    /// Whether earlier dictations go along as context: only when it
    /// post-processes.
    pub with_context: bool,
    /// Whether an instruction key was pressed during its recording (with or
    /// without a spoken instruction): a redo elsewhere post-processes by
    /// the same rule (`profile_redo`).
    pub instruction_key: bool,
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
            instruction_key: self.instruction_key,
        }
    }

    /// DanyWhere: the snapshot of a dictation whose recording went on to a
    /// spoken instruction (`dictation_flow::instruction`): an instruction key
    /// was pressed, so it post-processes unless its profile never does
    /// ([`wants_post_processing`]), with context as its end says;
    /// `use_streaming_overlay` is what the overlay showed at the cut, since
    /// the live stream ended there.
    pub fn with_spoken_instruction(self, use_streaming_overlay: bool) -> Self {
        let post_process = wants_post_processing(&self.values, true);
        Self {
            with_context: sends_context(post_process, &self.end),
            post_process,
            instruction_key: true,
            use_streaming_overlay,
            ..self
        }
    }
}

/// DanyWhere: whether a dictation with `values`, those of the profile it
/// ends with, post-processes (variant b, user, 2026-11-26): when an
/// instruction key was pressed in it (`instruction_key`: an instruction is
/// never lost, F2 i, and one heard as nothing still asks for the profile's
/// prompts), or always, by the profile's mode (`settings::PostProcessMode`);
/// never with "never": then no text goes to a language model, and the
/// context it would send stays home too (user, 2026-11-23, option a). The
/// end shortcuts lost their box "Post-process".
///
/// The one rule for what a dictation expects at its start
/// (`dictation_start::plan_start`, no instruction key yet), what it does
/// when it ends (here) and what a redo for another profile counts and does
/// (`profile_redo`). History's own actions (✨, "Post-process with") don't
/// ask it.
pub fn wants_post_processing(values: &ProfileValues, instruction_key: bool) -> bool {
    values.post_process_mode.post_processes(instruction_key)
}

/// DanyWhere: whether earlier dictations go along as context: only when it
/// post-processes, and as its end says.
fn sends_context(post_process: bool, end: &EndKey) -> bool {
    post_process && end.send_context
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
    // append text and whether earlier dictations go along. That profile's
    // mode decides with the instruction key whether to post-process
    // ([`wants_post_processing`]).
    let (profile_id, values) = match &inputs.own_profile {
        Some((id, values)) => (id.clone(), values.clone()),
        None => (
            inputs.stored.active_profile_id.clone(),
            ProfileValues::from_settings(inputs.stored),
        ),
    };
    let end = crate::end_keys::resolve(&values, inputs.end, inputs.binding_id);
    let instruction_key = inputs.end.after_instruction_key();
    let post_process = wants_post_processing(&values, instruction_key);
    DictationSnapshot {
        use_streaming_overlay: should_use_streaming_overlay(
            settings.overlay_style,
            inputs.streaming,
        ),
        settings,
        profile_id,
        values,
        with_context: sends_context(post_process, &end),
        instruction_key,
        post_process,
        end,
        transcribed_with_end_values: inputs.transcribed_with_end_values,
        session: inputs.session,
        cancel_generation: inputs.cancel_generation,
    }
}

/// Whether the overlay of `style` shows a stream's working states.
pub(crate) fn should_use_streaming_overlay(style: OverlayStyle, is_streaming: bool) -> bool {
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
    use crate::start_keys::FACTORY_START_ID;

    /// The factory start shortcut as registered (`start_keys`).
    const START: &str = "start_key:start";
    use crate::profiles::{ensure_profiles, DEFAULT_PROFILE_ID};

    fn end_key(id: &str, append_text: &str, send_context: bool) -> EndKey {
        EndKey {
            id: id.to_string(),
            name: String::new(),
            shortcut: format!("ctrl+option+{}", &id[..1]),
            append_text: append_text.to_string(),
            send_context,
        }
    }

    /// The defaults in use (Parakeet, never post-processing, the small
    /// overlay, the two built-in ends of `end_keys::with_builtin_ends`) and
    /// "mail" (Whisper, post-processing with an instruction key, the live
    /// overlay) with two ends of its own: "send" appends a line break,
    /// "quote" a quote mark; both send context.
    fn with_mail_profile() -> AppSettings {
        let mut settings = AppSettings {
            selected_model: "parakeet".to_string(),
            post_process_mode: crate::settings::PostProcessMode::Never,
            overlay_style: OverlayStyle::Minimal,
            ..crate::end_keys::with_builtin_ends(AppSettings::default())
        };
        ensure_profiles(&mut settings);
        let mut mail = crate::profiles::profile_for_test(&settings, "mail", "Mail", |values| {
            values.selected_model = "whisper".to_string();
            values.post_process_mode = crate::settings::PostProcessMode::WithInstructionKey;
            values.overlay_style = OverlayStyle::Live;
            values.end_keys.push(end_key("send", "\n", true));
            values.end_keys.push(end_key("quote", " (quoted)", true));
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
            binding_id: START,
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
            EndChoice::Start(FACTORY_START_ID.to_string()),
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
            EndChoice::Start(FACTORY_START_ID.to_string()),
        );
        assert_eq!(in_use.profile_id, DEFAULT_PROFILE_ID);
        assert_eq!(in_use.values.selected_model, "parakeet");
        assert_eq!(in_use.settings.selected_model, "parakeet");
    }

    #[test]
    fn the_end_shortcut_brings_its_append_text_and_context() {
        let stored = with_mail_profile();
        let sent = snapshot(
            &stored,
            Some(mail_values(&stored)),
            EndChoice::Key("send".to_string()).with_instruction_key(),
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
        mail.post_process_mode = crate::settings::PostProcessMode::Never;
        let sent = snapshot(
            &stored,
            Some(mail),
            EndChoice::Key("send".to_string()).with_instruction_key(),
        );
        assert_eq!(sent.end.id, "send");
        assert!(!sent.post_process);
        assert!(!sent.with_context);
    }

    #[test]
    fn the_dictation_and_its_redo_agree_whether_it_was_post_processed() {
        // Every mode, with an instruction key or without, and the profile in
        // use or one of its own: the redo for another profile counts the
        // entry as post-processed exactly when the dictation post-processed
        // it. So it redoes the post-processing for a target that
        // post-processes only when the dictation didn't, and for one that
        // doesn't only when it did.
        use crate::profile_redo::{redo_from_with_instruction, RedoFrom};
        use crate::settings::PostProcessMode::*;
        for mode in [Never, WithInstructionKey, Always] {
            for instruction_key in [true, false] {
                for own in [false, true] {
                    let mut stored = with_mail_profile();
                    stored.post_process_mode = mode;
                    let mut mail = mail_values(&stored);
                    mail.post_process_mode = mode;
                    let start = EndChoice::Start(FACTORY_START_ID.to_string());
                    let end = if instruction_key {
                        start.with_instruction_key()
                    } else {
                        start
                    };
                    let made = snapshot(&stored, own.then_some(mail), end);
                    let case =
                        format!("{mode:?}, instruction key {instruction_key}, own profile {own}");
                    assert_eq!(
                        made.post_process,
                        mode.post_processes(instruction_key),
                        "{case}"
                    );
                    assert_eq!(made.instruction_key, instruction_key, "{case}");

                    // A target like the dictation's profile that always
                    // post-processes, and one that never does.
                    let mut posting = made.values.clone();
                    posting.post_process_mode = Always;
                    let mut plain = made.values.clone();
                    plain.post_process_mode = Never;
                    let redo_into = |target: &ProfileValues| {
                        redo_from_with_instruction(
                            &stored,
                            &made.values,
                            target,
                            &made.end,
                            &made.end,
                            made.instruction_key,
                        )
                    };
                    let expected = |redone: bool| {
                        if redone {
                            RedoFrom::PostProcessing
                        } else {
                            RedoFrom::Nothing
                        }
                    };
                    assert_eq!(
                        redo_into(&posting),
                        expected(!made.post_process),
                        "{case}: into a profile that post-processes"
                    );
                    assert_eq!(
                        redo_into(&plain),
                        expected(made.post_process),
                        "{case}: into a profile that doesn't"
                    );
                }
            }
        }
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
        let end = EndChoice::Start(FACTORY_START_ID.to_string());
        let read = |own: Option<ProfileValues>, streaming: bool| {
            build(SnapshotInputs {
                stored: &stored,
                own_profile: own.map(|values| ("mail".to_string(), values)),
                end: &end,
                binding_id: START,
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

    // DanyWhere: a dictation with a spoken instruction post-processes unless
    // its profile never does (strand Z1; F2 i; variant b).

    #[test]
    fn the_instruction_key_post_processes_unless_never() {
        use crate::settings::PostProcessMode::*;
        let values = |mode| ProfileValues {
            post_process_mode: mode,
            ..ProfileValues::default()
        };
        assert!(wants_post_processing(&values(WithInstructionKey), true));
        assert!(!wants_post_processing(&values(WithInstructionKey), false));
        assert!(wants_post_processing(&values(Always), false));
        assert!(!wants_post_processing(&values(Never), true));
    }

    #[test]
    fn with_a_spoken_instruction_an_end_without_post_processing_post_processes() {
        let stored = with_mail_profile();
        // "quote" doesn't post-process but asks for context.
        let quoted = snapshot(
            &stored,
            Some(mail_values(&stored)),
            EndChoice::Key("quote".to_string()),
        );
        assert!(!quoted.post_process);
        assert!(!quoted.with_context);
        let instructed = quoted.with_spoken_instruction(true);
        assert!(instructed.post_process);
        assert!(
            instructed.with_context,
            "as the end says, now that it post-processes"
        );
        assert!(
            instructed.use_streaming_overlay,
            "the live overlay as at the cut"
        );
        assert_eq!(instructed.end.id, "quote");
        assert_eq!(instructed.end.append_text, " (quoted)");
    }

    #[test]
    fn with_a_spoken_instruction_an_end_without_context_sends_none() {
        let stored = with_mail_profile();
        let plain = snapshot(
            &stored,
            Some(mail_values(&stored)),
            EndChoice::Start(FACTORY_START_ID.to_string()),
        )
        .with_spoken_instruction(false);
        assert!(plain.post_process, "whatever its end says");
        assert!(!plain.with_context);
        assert!(!plain.use_streaming_overlay);
    }

    // ---- Variant b (user, 2026-11-26): the profile's mode decides --------

    /// The defaults in use with `mode`, an instruction key ⇥ and the two
    /// built-in ends ("plain" without context, "post_process" with it).
    fn in_mode(mode: crate::settings::PostProcessMode) -> AppSettings {
        let mut settings = crate::end_keys::with_builtin_ends(AppSettings::default());
        settings.post_process_mode = mode;
        settings.instruction_keys = vec![EndKey {
            id: "tab".to_string(),
            shortcut: "tab".to_string(),
            append_text: " ".to_string(),
            send_context: true,
            ..Default::default()
        }];
        for end in &mut settings.end_keys {
            end.send_context = end.id == crate::end_keys::POST_PROCESS_ID;
        }
        settings
    }

    fn ended(settings: &AppSettings, end: EndChoice) -> DictationSnapshot {
        snapshot(settings, None, end)
    }

    fn after_instruction_key(id: &str) -> EndChoice {
        EndChoice::AfterInstructionKey(Box::new(EndChoice::Key(id.to_string())))
    }

    #[test]
    fn with_the_instruction_key_mode_only_an_instruction_key_post_processes() {
        use crate::end_keys::{PLAIN_ID, POST_PROCESS_ID};
        let settings = in_mode(crate::settings::PostProcessMode::WithInstructionKey);
        for id in [PLAIN_ID, POST_PROCESS_ID] {
            let direct = ended(&settings, EndChoice::Key(id.to_string()));
            assert!(!direct.post_process, "{id}");
            assert!(!direct.with_context, "{id}");
            assert!(!direct.instruction_key, "{id}");
        }
        let after = ended(&settings, after_instruction_key(POST_PROCESS_ID));
        assert!(after.post_process);
        assert!(after.with_context, "as its end says");
        assert!(after.instruction_key);
        assert_eq!(after.end.id, POST_PROCESS_ID);
        let plain = ended(&settings, after_instruction_key(PLAIN_ID));
        assert!(plain.post_process);
        assert!(!plain.with_context, "as its end says");
    }

    #[test]
    fn a_held_instruction_key_ends_post_processed_with_its_own_values() {
        let settings = in_mode(crate::settings::PostProcessMode::WithInstructionKey);
        let held = ended(&settings, EndChoice::Instruction("tab".to_string()));
        assert!(held.post_process);
        assert!(held.with_context);
        assert_eq!(held.end.id, "instruction:tab");
        assert_eq!(held.end.append_text, " ");
    }

    #[test]
    fn always_post_processes_every_dictation() {
        let settings = in_mode(crate::settings::PostProcessMode::Always);
        let direct = ended(
            &settings,
            EndChoice::Key(crate::end_keys::POST_PROCESS_ID.to_string()),
        );
        assert!(direct.post_process);
        assert!(direct.with_context);
        assert!(!direct.instruction_key);
    }

    #[test]
    fn never_post_processes_nothing_not_even_after_an_instruction_key() {
        let settings = in_mode(crate::settings::PostProcessMode::Never);
        for end in [
            after_instruction_key(crate::end_keys::POST_PROCESS_ID),
            EndChoice::Instruction("tab".to_string()),
        ] {
            let snapshot = ended(&settings, end.clone());
            assert!(!snapshot.post_process, "{end:?}");
            assert!(!snapshot.with_context, "{end:?}");
        }
        let instructed = ended(
            &settings,
            EndChoice::Key(crate::end_keys::POST_PROCESS_ID.to_string()),
        )
        .with_spoken_instruction(false);
        assert!(!instructed.post_process, "no text goes to a provider");
        assert!(!instructed.with_context);
    }

    #[test]
    fn a_spoken_instruction_counts_as_an_instruction_key() {
        let settings = in_mode(crate::settings::PostProcessMode::WithInstructionKey);
        let instructed = ended(
            &settings,
            EndChoice::Key(crate::end_keys::PLAIN_ID.to_string()),
        )
        .with_spoken_instruction(false);
        assert!(instructed.post_process);
        assert!(instructed.instruction_key);
        assert!(instructed.made_with(7).instruction_key, "a redo knows it");
    }

    #[test]
    fn live_overlay_uses_streaming_states_only_for_streaming_models() {
        assert!(should_use_streaming_overlay(OverlayStyle::Live, true));
        assert!(!should_use_streaming_overlay(OverlayStyle::Live, false));
        assert!(!should_use_streaming_overlay(OverlayStyle::Minimal, true));
        assert!(!should_use_streaming_overlay(OverlayStyle::None, true));
    }
}
