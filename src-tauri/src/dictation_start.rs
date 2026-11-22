//! DanyWhere: the start of a dictation, from a start shortcut's press to a
//! running recording (test redesign, strand B4). [`check_model`] and
//! [`plan_start`] decide without the app, so tests can check every decision
//! the start makes.

use crate::audio_toolkit::VadPolicy;
use crate::managers::transcription::policy::{live_decode, LiveDecode, ModelTraits};
use crate::profiles::ProfileValues;
use crate::settings::{AppSettings, OverlayStyle};

/// A notice the start of a dictation raises.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartNotice {
    /// The dictation's profile has no model selected.
    NoModelSelected,
    /// The selected model's files can't be found, and no load reports it.
    ModelLoadFailed { model: String, error: String },
}

/// What the state of the speech model says about recording now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelCheck {
    /// The model is loaded, or its files are there to load while recording.
    Record,
    /// Nothing could transcribe the recording, so the microphone stays
    /// closed; with the notice to raise, if any.
    Refuse(Option<StartNotice>),
}

/// Whether a dictation with model `selected` may open the microphone.
/// `loaded`: the model loaded now, if any. `load_started`: the start began
/// loading it, and that load reports its own failure. `files` looks for the
/// model's files (`ModelManager::get_model_path`, which tidies up as it
/// looks), only when the model isn't loaded.
pub fn check_model(
    selected: &str,
    loaded: Option<&str>,
    load_started: bool,
    files: impl FnOnce() -> Result<(), String>,
) -> ModelCheck {
    // Also when another model than this dictation's is loaded.
    if loaded == Some(selected) {
        return ModelCheck::Record;
    }
    if selected.is_empty() {
        log::warn!("Not starting recording: no model is selected");
        return ModelCheck::Refuse(Some(StartNotice::NoModelSelected));
    }
    match files() {
        Ok(()) => ModelCheck::Record,
        Err(error) => {
            log::warn!("Not starting recording: no model can transcribe it ({error})");
            // A load the start began reports its own failure.
            ModelCheck::Refuse((!load_started).then(|| StartNotice::ModelLoadFailed {
                model: selected.to_string(),
                error,
            }))
        }
    }
}

/// The overlay a recording starts with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartOverlay {
    /// The live overlay, for a model that streams.
    Streaming,
    /// The compact recording pill.
    Recording,
    /// None ("Overlay: None").
    Hidden,
}

/// How a recording starts, decided before the microphone opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartPlan {
    pub vad_policy: VadPolicy,
    /// A worker decodes the recording while it runs
    /// (`TranscriptionManager::start_stream`).
    pub live_decode: bool,
    pub overlay: StartOverlay,
    /// The end the recording is expected to take post-processes, so a local
    /// post-processing model warms up (`model_warmup`).
    pub post_process: bool,
    /// That end also sends context: the pill shows the speech bubble
    /// (`send_context`).
    pub sends_context: bool,
}

/// How a dictation started with `binding_id` records: `settings` are the
/// dictation's (`app_profiles::dictation_settings`), `values` its profile's
/// values, `own_values` those of its own profile when it isn't the active
/// one, and `model_streams` whether its model says it streams (unknown
/// counts as not, until the model registry knows better).
pub fn plan_start(
    settings: &AppSettings,
    values: &ProfileValues,
    own_values: Option<&ProfileValues>,
    binding_id: &str,
    model_streams: bool,
) -> StartPlan {
    let vad_policy = if !settings.vad_enabled {
        VadPolicy::Disabled
    } else if model_streams {
        VadPolicy::Streaming
    } else {
        VadPolicy::Offline
    };
    // Before the model loads, whether it keeps its accuracy on long audio
    // is unknown, so assume not: the worker decides again with the loaded
    // model, and a wrong guess costs only an idle worker.
    let expected_model = ModelTraits {
        streaming: model_streams,
        long_form: false,
    };
    let live_decode = live_decode(expected_model, settings.long_form_chunking) != LiveDecode::None;
    // Sizing the overlay follows the same advertised capability. A model that
    // doesn't stream (or whose capability is not known yet) gets the compact
    // pill instead of an oversized transparent live window.
    let overlay = match settings.overlay_style {
        OverlayStyle::Live if model_streams => StartOverlay::Streaming,
        OverlayStyle::Live | OverlayStyle::Minimal => StartOverlay::Recording,
        OverlayStyle::None => StartOverlay::Hidden,
    };
    // The end the recording is expected to take (`end_keys::expected_end`).
    let expected_end =
        crate::end_keys::expected_end(values, binding_id, settings.shortcut_activation);
    let post_process =
        crate::app_profiles::effective_post_process(expected_end.post_process, own_values);
    StartPlan {
        vad_policy,
        live_decode,
        overlay,
        post_process,
        sends_context: post_process && expected_end.send_context,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::end_keys::START_BINDINGS;
    use crate::settings::{get_default_settings, ShortcutActivation};
    use std::cell::Cell;

    const MODEL: &str = "parakeet-tdt-0.6b-v3";

    // check_model

    #[test]
    fn a_loaded_model_records_without_looking_for_its_files() {
        let looked = Cell::new(false);
        let check = check_model(MODEL, Some(MODEL), false, || {
            looked.set(true);
            Ok(())
        });
        assert_eq!(check, ModelCheck::Record);
        assert!(!looked.get());
    }

    #[test]
    fn no_model_selected_refuses_with_its_notice() {
        let looked = Cell::new(false);
        let check = check_model("", None, false, || {
            looked.set(true);
            Ok(())
        });
        assert_eq!(
            check,
            ModelCheck::Refuse(Some(StartNotice::NoModelSelected))
        );
        assert!(!looked.get());
    }

    #[test]
    fn a_model_not_loaded_records_when_its_files_are_there() {
        assert_eq!(
            check_model(MODEL, None, true, || Ok(())),
            ModelCheck::Record
        );
    }

    #[test]
    fn another_loaded_model_makes_the_start_look_for_this_ones_files() {
        let looked = Cell::new(false);
        let check = check_model(MODEL, Some("whisper-small"), false, || {
            looked.set(true);
            Err("Model not available".to_string())
        });
        assert!(looked.get());
        assert_eq!(
            check,
            ModelCheck::Refuse(Some(StartNotice::ModelLoadFailed {
                model: MODEL.to_string(),
                error: "Model not available".to_string(),
            }))
        );
    }

    #[test]
    fn missing_files_refuse_silently_when_the_started_load_reports_them() {
        assert_eq!(
            check_model(MODEL, None, true, || Err("gone".to_string())),
            ModelCheck::Refuse(None)
        );
    }

    // plan_start

    fn settings() -> AppSettings {
        let mut settings = get_default_settings();
        settings.vad_enabled = true;
        settings.overlay_style = OverlayStyle::Live;
        settings.long_form_chunking = false;
        settings.shortcut_activation = ShortcutActivation::Toggle;
        settings
    }

    fn plan(settings: &AppSettings, binding: &str, streams: bool) -> StartPlan {
        let values = ProfileValues::from_settings(settings);
        plan_start(settings, &values, None, binding, streams)
    }

    #[test]
    fn vad_follows_the_setting_and_whether_the_model_streams() {
        let mut settings = settings();
        assert_eq!(
            plan(&settings, START_BINDINGS[0], false).vad_policy,
            VadPolicy::Offline
        );
        assert_eq!(
            plan(&settings, START_BINDINGS[0], true).vad_policy,
            VadPolicy::Streaming
        );
        settings.vad_enabled = false;
        assert_eq!(
            plan(&settings, START_BINDINGS[0], true).vad_policy,
            VadPolicy::Disabled
        );
    }

    #[test]
    fn a_streaming_model_decodes_while_recording() {
        assert!(plan(&settings(), START_BINDINGS[0], true).live_decode);
        assert!(!plan(&settings(), START_BINDINGS[0], false).live_decode);
    }

    #[test]
    fn long_audio_is_cut_while_recording_when_chunking_is_on() {
        let mut settings = settings();
        settings.long_form_chunking = true;
        // Before it loads, a model counts as losing accuracy on long audio.
        assert!(plan(&settings, START_BINDINGS[0], false).live_decode);
    }

    #[test]
    fn the_live_overlay_needs_a_streaming_model() {
        let mut settings = settings();
        assert_eq!(
            plan(&settings, START_BINDINGS[0], true).overlay,
            StartOverlay::Streaming
        );
        assert_eq!(
            plan(&settings, START_BINDINGS[0], false).overlay,
            StartOverlay::Recording
        );
        settings.overlay_style = OverlayStyle::Minimal;
        assert_eq!(
            plan(&settings, START_BINDINGS[0], true).overlay,
            StartOverlay::Recording
        );
        settings.overlay_style = OverlayStyle::None;
        assert_eq!(
            plan(&settings, START_BINDINGS[0], true).overlay,
            StartOverlay::Hidden
        );
    }

    #[test]
    fn the_second_start_shortcut_is_expected_to_post_process() {
        let settings = settings();
        assert!(!plan(&settings, START_BINDINGS[0], false).post_process);
        assert!(plan(&settings, START_BINDINGS[1], false).post_process);
    }

    #[test]
    fn with_hold_the_hold_end_decides_post_processing() {
        let mut settings = settings();
        settings.shortcut_activation = ShortcutActivation::PushToTalk;
        let mut values = ProfileValues::from_settings(&settings);
        values
            .hold_ends
            .get_mut(START_BINDINGS[0])
            .expect("the first start shortcut has a hold end")
            .post_process = true;
        let plan = plan_start(&settings, &values, None, START_BINDINGS[0], false);
        assert!(plan.post_process);
    }

    #[test]
    fn an_own_profile_without_post_processing_expects_none() {
        let settings = settings();
        let mut own = ProfileValues::from_settings(&settings);
        own.post_process_enabled = false;
        let plan = plan_start(&settings, &own, Some(&own), START_BINDINGS[1], false);
        assert!(!plan.post_process);
    }

    #[test]
    fn the_speech_bubble_shows_when_the_expected_end_post_processes_with_context() {
        let settings = settings();
        let mut values = ProfileValues::from_settings(&settings);
        for end in &mut values.end_keys {
            end.send_context = true;
        }
        let with_post_process = plan_start(&settings, &values, None, START_BINDINGS[1], false);
        assert!(with_post_process.sends_context);
        // Context goes only with post-processing.
        let plain = plan_start(&settings, &values, None, START_BINDINGS[0], false);
        assert!(!plain.sends_context);
    }
}
