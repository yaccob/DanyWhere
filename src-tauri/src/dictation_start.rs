//! DanyWhere: the start of a dictation, from a start shortcut's press to a
//! running recording (test redesign, strand B4). [`check_model`] and
//! [`plan_start`] decide without the app; [`start_dictation`] acts on their
//! answers through small ports ([`StartPorts`]), so a test runs the whole
//! start with the real `AudioRecordingManager` over a fake microphone and
//! records what the user would see and hear. The app's ports are in
//! [`app_ports`].

pub(crate) mod app_ports;

use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

use log::debug;

use crate::app_profiles::OverlayProfile;
use crate::audio_toolkit::VadPolicy;
use crate::delivery::InputSourceHold;
use crate::input_method::InputMethod;
use crate::managers::audio::{recording_notice_code, RecordingReadiness};
use crate::managers::transcription::policy::{live_decode, LiveDecode, ModelTraits};
use crate::notices::NoticeCode;
use crate::profiles::ProfileValues;
use crate::settings::{AppSettings, OverlayStyle};
use crate::tray::TrayIconState;

/// A notice the start of a dictation raises.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartNotice {
    /// The dictation's profile has no model selected.
    NoModelSelected,
    /// The selected model's files can't be found, and no load reports it.
    ModelLoadFailed { model: String, error: String },
    /// The microphone didn't start (`recording_notice_code`); `detail` is
    /// the error with its causes.
    RecordingFailed { code: NoticeCode, detail: String },
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
/// values (its own, or the one in use), and `model_streams` whether its
/// model says it streams (unknown counts as not, until the model registry
/// knows better).
pub fn plan_start(
    settings: &AppSettings,
    values: &ProfileValues,
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
    let post_process = crate::app_profiles::effective_post_process(&expected_end, values);
    StartPlan {
        vad_policy,
        live_decode,
        overlay,
        post_process,
        sends_context: post_process && expected_end.send_context,
    }
}

// ---------------------------------------------------------------- ports

/// The dictation's profile and the settings it reads (`app_profiles`).
pub trait StartProfile {
    /// Chooses the dictation's profile from the application in front;
    /// returns what its pills show of it.
    fn begin(&self) -> Option<OverlayProfile>;
    /// Ends the dictation's own profile: the active one applies again.
    fn end(&self);
    /// The settings with the values of the dictation's profile.
    fn settings(&self) -> AppSettings;
    /// The values of the dictation's profile: its own, or the one in use.
    fn values(&self) -> ProfileValues;
}

/// The speech model (`TranscriptionManager`, `ModelManager`).
pub trait StartModel {
    /// Starts loading the dictation's model unless it is loaded or loading
    /// already; whether it did (that load then reports its own failure).
    fn initiate_load(&self) -> bool;
    /// The model loaded now, if any.
    fn loaded(&self) -> Option<String>;
    /// Looks for `model`'s files.
    fn files(&self, model: &str) -> Result<(), String>;
    /// Whether `model` says it streams; unknown counts as not.
    fn streams(&self, model: &str) -> bool;
    /// Starts the worker that decodes the recording while it runs.
    fn start_stream(&self);
    fn cancel_stream(&self);
}

/// The microphone (`AudioRecordingManager`).
pub trait Microphone: Send + Sync {
    /// Loads the VAD in the background, so the recording needn't wait.
    fn preload_vad_in_background(&self);
    /// Starts recording for `binding_id`; the readiness comes with its first
    /// audio.
    fn try_start(
        &self,
        binding_id: &str,
        vad_policy: VadPolicy,
    ) -> anyhow::Result<RecordingReadiness>;
    /// Whether the recording that got readiness `generation` still runs.
    fn readiness_current(&self, generation: u64) -> bool;
}

/// What the user sees and hears of a dictation that starts: tray, overlay,
/// sounds, the system's sound and notices.
pub trait StartReport: Send + Sync {
    fn tray(&self, state: TrayIconState);
    /// The dictation's profile on its pills (from two profiles on).
    fn profile(&self, shown: Option<OverlayProfile>);
    /// The speech bubble while the expected end sends context.
    fn context_bubble(&self, sends: bool);
    fn overlay(&self, overlay: StartOverlay);
    /// The microphone delivers: the overlay stops arming.
    fn ready(&self);
    /// The start sound; returns once it has played.
    fn start_sound(&self);
    /// Mutes the system's sound while recording, if the profile says so.
    fn mute(&self);
    fn notice(&self, notice: StartNotice);
}

/// The rest of the app around a recording that starts.
pub trait StartSession {
    /// A new recording session begins (`actions::current_recording_session`).
    fn begin(&self);
    /// Asks the app in front for its window title ahead, so its answer
    /// comes in time when the recording ends (`window_capture`).
    fn prime_title_query(&self);
    /// Registers the cancel shortcut while the recording runs.
    fn register_cancel_shortcut(&self);
    /// Warms up a local post-processing model, if the dictation needs one.
    fn warm_up(&self, settings: &AppSettings, post_process: bool);
    /// DanyWhere's input method (`input_method::current`), whose source the
    /// recording selects when its text is to go through it.
    fn input_method(&self) -> Arc<dyn InputMethod>;
    /// Keeps the recording's hold on that source until its stop hands it to
    /// the dictation or a cancel drops it (`delivery::keep_recording_hold`).
    fn keep_input_source(&self, hold: InputSourceHold);
}

/// Everything the start of a dictation acts on. The microphone and the
/// report go on to the thread that waits for the microphone.
pub struct StartPorts<'a> {
    pub profile: &'a dyn StartProfile,
    pub model: &'a dyn StartModel,
    pub microphone: Arc<dyn Microphone>,
    pub report: Arc<dyn StartReport>,
    pub session: &'a dyn StartSession,
}

/// How the start of a dictation ended.
#[derive(Debug)]
pub enum StartOutcome {
    /// No model could transcribe it: the microphone stayed closed.
    NoModel,
    /// The microphone didn't start; the start was taken back.
    RecordingFailed,
    /// It records. The thread waits for the microphone's first audio and
    /// then gives the cues (ready, start sound, mute); tests wait for it,
    /// the app lets it run.
    Recording(#[cfg_attr(not(test), allow(dead_code))] JoinHandle<()>),
}

/// Starts a dictation with start shortcut `binding_id` (Handy's
/// `TranscribeAction::start`).
pub fn start_dictation(binding_id: &str, ports: &StartPorts) -> StartOutcome {
    let start_time = Instant::now();
    debug!("TranscribeAction::start called for binding: {}", binding_id);

    // DanyWhere: the profile of the app in front, when it has one of its
    // own (`app_profiles`); its values apply until the dictation ends. What
    // the overlay shows of the dictation's profile comes back.
    let shown = ports.profile.begin();

    // Load the ASR model and the VAD in parallel. An empty selection has
    // nothing to load; the check below says so.
    let kickoff_started = Instant::now();
    let selected_model = ports.profile.settings().selected_model;
    let load_started = !selected_model.is_empty() && ports.model.initiate_load();
    ports.microphone.preload_vad_in_background();
    let kickoff_elapsed = kickoff_started.elapsed();

    // Don't open the mic if nothing can transcribe the recording.
    if let ModelCheck::Refuse(notice) = check_model(
        &selected_model,
        ports.model.loaded().as_deref(),
        load_started,
        || ports.model.files(&selected_model),
    ) {
        ports.profile.end();
        if let Some(notice) = notice {
            ports.report.notice(notice);
        }
        return StartOutcome::NoModel;
    }

    // DanyWhere: the dictation's window is captured when its recording ends
    // (`stop_transcription`), as its profile is chosen then.
    ports.session.begin();
    let tray_started = Instant::now();
    ports.report.tray(TrayIconState::Recording);
    let tray_elapsed = tray_started.elapsed();

    // DanyWhere: with the values of the dictation's own profile, if any.
    let plan_started = Instant::now();
    let settings = ports.profile.settings();
    let plan = plan_start(
        &settings,
        &ports.profile.values(),
        binding_id,
        ports.model.streams(&settings.selected_model),
    );
    if plan.live_decode {
        ports.model.start_stream();
    }
    let plan_elapsed = plan_started.elapsed();

    // DanyWhere: from two profiles on, the overlay names the dictation's
    // profile with the glyph of choosing it by application, and shows a
    // speech bubble while the end it is expected to take sends context.
    let overlay_started = Instant::now();
    ports.report.profile(shown);
    ports.report.context_bubble(plan.sends_context);
    ports.report.overlay(plan.overlay);
    // Everything above runs before capture can begin, so each span here is
    // added keypress->capture latency.
    debug!(
        "start-path pre-recording steps: model_kickoff={:?} tray={:?} settings+stream_plan={:?} overlay={:?}",
        kickoff_elapsed,
        tray_elapsed,
        plan_elapsed,
        overlay_started.elapsed()
    );
    debug!(
        "Microphone mode - always_on: {}",
        settings.always_on_microphone
    );

    let recording_start_time = Instant::now();
    let outcome = match ports.microphone.try_start(binding_id, plan.vad_policy) {
        Ok(readiness) => {
            debug!(
                "Recording request accepted in {:?}; waiting for first microphone samples",
                recording_start_time.elapsed()
            );
            // DanyWhere: an app that answers its first Apple event slowly is
            // asked now, so the window title comes in time when the recording
            // ends (`window_capture`, macOS without Accessibility).
            ports.session.prime_title_query();
            let cues = give_cues_when_ready(
                readiness,
                Arc::clone(&ports.microphone),
                Arc::clone(&ports.report),
            );
            ports.session.register_cancel_shortcut();
            // DanyWhere: a post-processing model on this computer starts
            // loading now, so its cold start falls into the speaking time.
            ports.session.warm_up(&settings, plan.post_process);
            // DanyWhere: DanyWhere's input source while the recording runs,
            // when its text is to go through the input method (user,
            // 2026-11-23). The recording keeps its hold: its stop hands it
            // to the dictation, which ends it once the text is in; a cancel
            // drops it (the coordinator).
            if let Some(hold) =
                crate::delivery::begin_input_source(&settings, &ports.session.input_method())
            {
                ports.session.keep_input_source(hold);
            }
            StartOutcome::Recording(cues)
        }
        Err(e) => {
            // Starting failed (for example due to blocked microphone
            // permissions). Revert UI state so we don't stay stuck in the
            // recording overlay.
            debug!("Failed to start recording: {}", e);
            ports.model.cancel_stream();
            ports.profile.end();
            ports.report.tray(TrayIconState::Idle);
            ports.report.notice(StartNotice::RecordingFailed {
                code: recording_notice_code(&e),
                detail: format!("{e:#}"),
            });
            StartOutcome::RecordingFailed
        }
    };
    debug!(
        "TranscribeAction::start completed in {:?}",
        start_time.elapsed()
    );
    outcome
}

/// Waits on its own thread for the microphone's first audio, then gives the
/// cues: ready, the start sound, muting. Each only while the recording that
/// got `readiness` still runs, so a stop before it gets none.
fn give_cues_when_ready(
    readiness: RecordingReadiness,
    microphone: Arc<dyn Microphone>,
    report: Arc<dyn StartReport>,
) -> JoinHandle<()> {
    let generation = readiness.generation();
    std::thread::spawn(move || {
        if !readiness.wait() {
            debug!("Microphone readiness wait ended without receiving samples");
            return;
        }

        // Development-only preview hook for evaluating the brief arming
        // animation on hardware that normally starts too fast to make it
        // visible.
        #[cfg(debug_assertions)]
        if let Ok(delay_ms) = std::env::var("HANDY_DEBUG_MIC_READY_DELAY_MS")
            .unwrap_or_default()
            .parse::<u64>()
        {
            let delay_ms = delay_ms.min(10_000);
            if delay_ms > 0 {
                debug!("Delaying microphone-ready cue by {delay_ms}ms for UI preview");
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            }
        }

        if !microphone.readiness_current(generation) {
            debug!("Microphone became ready for an inactive recording");
            return;
        }

        debug!("Microphone is receiving samples; recording is ready");
        report.ready();

        // The start chime is a readiness cue, so it must follow the first
        // real input callback rather than Stream::play() or a fixed delay.
        // The helper returns immediately when feedback is disabled; mute
        // still follows the same readiness point.
        if microphone.readiness_current(generation) {
            report.start_sound();
        }
        if microphone.readiness_current(generation) {
            report.mute();
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::end_keys::START_BINDINGS;
    use crate::input_method::testing::{inserted, FakeInputMethod, ImCall};
    use crate::managers::audio::{fake_microphone, AudioRecordingManager};
    use crate::settings::{get_default_settings, ShortcutActivation};
    use handy_recorder::testing::{FakeError, FakeMic};
    use std::cell::Cell;
    use std::sync::Mutex;

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
        plan_start(settings, &values, binding, streams)
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
        let mut settings = settings();
        settings.post_process_enabled = true;
        assert!(!plan(&settings, START_BINDINGS[0], false).post_process);
        assert!(plan(&settings, START_BINDINGS[1], false).post_process);
    }

    #[test]
    fn with_post_processing_off_no_start_expects_post_processing() {
        // As the defaults come: the switch is off, so neither the second
        // start shortcut nor a hold end with post-processing warms up a
        // model or shows the speech bubble.
        let mut settings = settings();
        assert!(!settings.post_process_enabled);
        let mut values = ProfileValues::from_settings(&settings);
        for end in &mut values.end_keys {
            end.send_context = true;
        }
        let pressed = plan_start(&settings, &values, START_BINDINGS[1], false);
        assert!(!pressed.post_process);
        assert!(!pressed.sends_context);
        settings.shortcut_activation = ShortcutActivation::PushToTalk;
        let held = values
            .hold_ends
            .get_mut(START_BINDINGS[0])
            .expect("the first start shortcut has a hold end");
        held.post_process = true;
        held.send_context = true;
        let held = plan_start(&settings, &values, START_BINDINGS[0], false);
        assert!(!held.post_process);
        assert!(!held.sends_context);
    }

    #[test]
    fn with_hold_the_hold_end_decides_post_processing() {
        let mut settings = settings();
        settings.post_process_enabled = true;
        settings.shortcut_activation = ShortcutActivation::PushToTalk;
        let mut values = ProfileValues::from_settings(&settings);
        values
            .hold_ends
            .get_mut(START_BINDINGS[0])
            .expect("the first start shortcut has a hold end")
            .post_process = true;
        let plan = plan_start(&settings, &values, START_BINDINGS[0], false);
        assert!(plan.post_process);
    }

    #[test]
    fn an_own_profile_without_post_processing_expects_none() {
        // Its own switch counts, not that of the profile in use.
        let mut settings = settings();
        settings.post_process_enabled = true;
        let mut own = ProfileValues::from_settings(&settings);
        own.post_process_enabled = false;
        let plan = plan_start(&settings, &own, START_BINDINGS[1], false);
        assert!(!plan.post_process);
    }

    #[test]
    fn the_speech_bubble_shows_when_the_expected_end_post_processes_with_context() {
        let mut settings = settings();
        settings.post_process_enabled = true;
        let mut values = ProfileValues::from_settings(&settings);
        for end in &mut values.end_keys {
            end.send_context = true;
        }
        let with_post_process = plan_start(&settings, &values, START_BINDINGS[1], false);
        assert!(with_post_process.sends_context);
        // Context goes only with post-processing.
        let plain = plan_start(&settings, &values, START_BINDINGS[0], false);
        assert!(!plain.sends_context);
    }

    // start_dictation, with the real AudioRecordingManager over a fake
    // microphone and fakes for the rest of the app.

    /// What the start did to the app, in order.
    #[derive(Debug, Clone, PartialEq)]
    enum Did {
        ProfileBegun,
        ProfileEnded,
        LoadAsked,
        StreamStarted,
        StreamCancelled,
        SessionBegun,
        TitleQueryPrimed,
        CancelShortcutRegistered,
        WarmUp { post_process: bool },
        Tray(TrayIconState),
        PillProfile,
        ContextBubble(bool),
        Overlay(StartOverlay),
        Ready,
        StartSound,
        Mute,
        Notice(StartNotice),
        InputMethod(ImCall),
    }

    type Log = Arc<Mutex<Vec<Did>>>;

    fn note(log: &Log, did: Did) {
        log.lock().unwrap().push(did);
    }

    /// The profile, the model and the session of a test.
    struct FakeApp {
        log: Log,
        settings: AppSettings,
        loaded: Option<String>,
        files: Result<(), String>,
        streams: bool,
        /// The user has added DanyWhere's input source.
        input_method_added: bool,
        /// The holds on its source the start kept for the recording.
        kept: Mutex<Vec<InputSourceHold>>,
    }

    impl StartProfile for FakeApp {
        fn begin(&self) -> Option<OverlayProfile> {
            note(&self.log, Did::ProfileBegun);
            None
        }
        fn end(&self) {
            note(&self.log, Did::ProfileEnded);
        }
        fn settings(&self) -> AppSettings {
            self.settings.clone()
        }
        fn values(&self) -> ProfileValues {
            ProfileValues::from_settings(&self.settings)
        }
    }

    impl StartModel for FakeApp {
        fn initiate_load(&self) -> bool {
            note(&self.log, Did::LoadAsked);
            false
        }
        fn loaded(&self) -> Option<String> {
            self.loaded.clone()
        }
        fn files(&self, _model: &str) -> Result<(), String> {
            self.files.clone()
        }
        fn streams(&self, _model: &str) -> bool {
            self.streams
        }
        fn start_stream(&self) {
            note(&self.log, Did::StreamStarted);
        }
        fn cancel_stream(&self) {
            note(&self.log, Did::StreamCancelled);
        }
    }

    impl StartSession for FakeApp {
        fn begin(&self) {
            note(&self.log, Did::SessionBegun);
        }
        fn prime_title_query(&self) {
            note(&self.log, Did::TitleQueryPrimed);
        }
        fn register_cancel_shortcut(&self) {
            note(&self.log, Did::CancelShortcutRegistered);
        }
        fn warm_up(&self, _settings: &AppSettings, post_process: bool) {
            note(&self.log, Did::WarmUp { post_process });
        }
        fn input_method(&self) -> Arc<dyn InputMethod> {
            let log = Arc::clone(&self.log);
            Arc::new(FakeInputMethod::new(
                self.input_method_added,
                inserted(),
                move |call| note(&log, Did::InputMethod(call)),
            ))
        }
        fn keep_input_source(&self, hold: InputSourceHold) {
            self.kept.lock().unwrap().push(hold);
        }
    }

    /// The reporter of a test: notes what the user would see and hear.
    struct FakeReport(Log);

    impl StartReport for FakeReport {
        fn tray(&self, state: TrayIconState) {
            note(&self.0, Did::Tray(state));
        }
        fn profile(&self, _shown: Option<OverlayProfile>) {
            note(&self.0, Did::PillProfile);
        }
        fn context_bubble(&self, sends: bool) {
            note(&self.0, Did::ContextBubble(sends));
        }
        fn overlay(&self, overlay: StartOverlay) {
            note(&self.0, Did::Overlay(overlay));
        }
        fn ready(&self) {
            note(&self.0, Did::Ready);
        }
        fn start_sound(&self) {
            note(&self.0, Did::StartSound);
        }
        fn mute(&self) {
            note(&self.0, Did::Mute);
        }
        fn notice(&self, notice: StartNotice) {
            note(&self.0, Did::Notice(notice));
        }
    }

    /// A dictation's start over `mic`: the model is loaded, VAD off (so the
    /// recording keeps every sample) and the overlay is the pill.
    struct Bench {
        log: Log,
        mic: FakeMic,
        rm: Arc<AudioRecordingManager>,
        app: FakeApp,
        report: Arc<FakeReport>,
    }

    impl Bench {
        fn new(mic: FakeMic) -> Self {
            let mut settings = fake_microphone::settings();
            settings.selected_model = MODEL.to_string();
            settings.vad_enabled = false;
            settings.overlay_style = OverlayStyle::Minimal;
            settings.long_form_chunking = false;
            settings.shortcut_activation = ShortcutActivation::Toggle;
            let log = Log::default();
            Self {
                rm: Arc::new(fake_microphone::manager(&mic, settings.clone())),
                mic,
                app: FakeApp {
                    log: Arc::clone(&log),
                    settings,
                    loaded: Some(MODEL.to_string()),
                    files: Ok(()),
                    streams: false,
                    input_method_added: false,
                    kept: Mutex::default(),
                },
                report: Arc::new(FakeReport(Arc::clone(&log))),
                log,
            }
        }

        fn start(&self, binding: &str) -> StartOutcome {
            start_dictation(
                binding,
                &StartPorts {
                    profile: &self.app,
                    model: &self.app,
                    microphone: Arc::clone(&self.rm) as Arc<dyn Microphone>,
                    report: Arc::clone(&self.report) as Arc<dyn StartReport>,
                    session: &self.app,
                },
            )
        }

        fn log(&self) -> Vec<Did> {
            self.log.lock().unwrap().clone()
        }
    }

    /// Waits for the thread that waits for the microphone.
    fn cues_given(outcome: StartOutcome) {
        match outcome {
            StartOutcome::Recording(cues) => cues.join().expect("the cues' thread"),
            other => panic!("expected a recording, got {other:?}"),
        }
    }

    #[test]
    fn a_dictation_records_gives_its_cues_in_order_and_stops_with_the_microphones_samples() {
        let bench = Bench::new(fake_microphone::mic());
        let outcome = bench.start(START_BINDINGS[0]);
        assert!(bench.mic.is_streaming(), "the microphone is open");
        assert!(bench.rm.is_recording());
        let speech = fake_microphone::speech(24_000);
        fake_microphone::play(&bench.mic, &speech);
        cues_given(outcome);

        assert_eq!(
            bench.log(),
            vec![
                Did::ProfileBegun,
                Did::LoadAsked,
                Did::SessionBegun,
                Did::Tray(TrayIconState::Recording),
                Did::PillProfile,
                Did::ContextBubble(false),
                Did::Overlay(StartOverlay::Recording),
                Did::TitleQueryPrimed,
                Did::CancelShortcutRegistered,
                Did::WarmUp {
                    post_process: false
                },
                // When the microphone delivers, in this order.
                Did::Ready,
                Did::StartSound,
                Did::Mute,
            ]
        );
        assert_eq!(
            fake_microphone::stop(&bench.rm, &bench.mic, START_BINDINGS[0]),
            Some(speech)
        );
    }

    #[test]
    fn a_microphone_that_does_not_start_takes_the_start_back_with_its_notice() {
        let mic = fake_microphone::mic();
        mic.fail_next_open(FakeError::PermissionDenied);
        let bench = Bench::new(mic);
        let outcome = bench.start(START_BINDINGS[0]);
        assert!(matches!(outcome, StartOutcome::RecordingFailed));
        assert!(!bench.rm.is_recording());

        let log = bench.log();
        let (start, taken_back) = log.split_at(7);
        assert_eq!(start.last(), Some(&Did::Overlay(StartOverlay::Recording)));
        assert!(
            matches!(
                taken_back,
                [
                    Did::StreamCancelled,
                    Did::ProfileEnded,
                    Did::Tray(TrayIconState::Idle),
                    Did::Notice(StartNotice::RecordingFailed {
                        code: NoticeCode::MicPermissionDenied,
                        ..
                    }),
                ]
            ),
            "taken back: {taken_back:?}"
        );
    }

    #[test]
    fn a_stop_before_the_microphone_delivers_gives_no_cues() {
        let bench = Bench::new(fake_microphone::mic());
        let outcome = bench.start(START_BINDINGS[0]);
        assert_eq!(
            fake_microphone::stop(&bench.rm, &bench.mic, START_BINDINGS[0]),
            Some(Vec::new())
        );
        cues_given(outcome);
        let log = bench.log();
        assert!(
            !log.iter()
                .any(|did| matches!(did, Did::Ready | Did::StartSound | Did::Mute)),
            "no cues: {log:?}"
        );
    }

    #[test]
    fn without_a_model_the_microphone_stays_closed() {
        let mut bench = Bench::new(fake_microphone::mic());
        bench.app.settings.selected_model = String::new();
        bench.app.loaded = None;
        let outcome = bench.start(START_BINDINGS[0]);
        assert!(matches!(outcome, StartOutcome::NoModel));
        assert!(!bench.mic.is_streaming());
        assert!(!bench.rm.is_recording());
        // An empty selection has nothing to load.
        assert_eq!(
            bench.log(),
            vec![
                Did::ProfileBegun,
                Did::ProfileEnded,
                Did::Notice(StartNotice::NoModelSelected),
            ]
        );
    }

    #[test]
    fn a_streaming_model_starts_its_worker_and_the_live_overlay_before_the_microphone() {
        let mut bench = Bench::new(fake_microphone::mic());
        bench.app.streams = true;
        bench.app.settings.overlay_style = OverlayStyle::Live;
        bench.app.settings.post_process_enabled = true;
        let outcome = bench.start(START_BINDINGS[1]);
        assert!(matches!(outcome, StartOutcome::Recording(_)));
        assert_eq!(
            bench.log()[..9],
            [
                Did::ProfileBegun,
                Did::LoadAsked,
                Did::SessionBegun,
                Did::Tray(TrayIconState::Recording),
                Did::StreamStarted,
                Did::PillProfile,
                Did::ContextBubble(false),
                Did::Overlay(StartOverlay::Streaming),
                Did::TitleQueryPrimed,
            ]
        );
        // The second start shortcut post-processes with the switch on: its
        // model warms up.
        assert!(bench.log().contains(&Did::WarmUp { post_process: true }));
        fake_microphone::stop(&bench.rm, &bench.mic, START_BINDINGS[1]);
    }

    // DanyWhere's input method (user's decision, 2026-11-23): a recording
    // whose text is to go through it selects its source.

    /// The input method's calls in the start's log.
    fn input_method_calls(log: &[Did]) -> Vec<ImCall> {
        log.iter()
            .filter_map(|did| match did {
                Did::InputMethod(call) => Some(call.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_recording_whose_text_goes_through_the_input_method_selects_its_source() {
        // With Accessibility or without: the input method inserts either way.
        for skipped in [false, true] {
            let mut bench = Bench::new(fake_microphone::mic());
            bench.app.input_method_added = true;
            bench.app.settings.paste_method = crate::settings::PasteMethod::CtrlV;
            bench.app.settings.accessibility_skipped = skipped;
            let outcome = bench.start(START_BINDINGS[0]);
            fake_microphone::play(&bench.mic, &fake_microphone::speech(24_000));
            cues_given(outcome);
            assert_eq!(
                bench.log(),
                vec![
                    Did::ProfileBegun,
                    Did::LoadAsked,
                    Did::SessionBegun,
                    Did::Tray(TrayIconState::Recording),
                    Did::PillProfile,
                    Did::ContextBubble(false),
                    Did::Overlay(StartOverlay::Recording),
                    Did::TitleQueryPrimed,
                    Did::CancelShortcutRegistered,
                    Did::WarmUp {
                        post_process: false
                    },
                    // Once the recording runs.
                    Did::InputMethod(ImCall::Begin),
                    Did::Ready,
                    Did::StartSound,
                    Did::Mute,
                ],
                "skipped {skipped}"
            );
            // The recording keeps its hold; whoever ends the recording ends
            // it, once.
            let kept = std::mem::take(&mut *bench.app.kept.lock().unwrap());
            assert_eq!(kept.len(), 1, "skipped {skipped}");
            drop(kept);
            assert_eq!(
                input_method_calls(&bench.log()),
                vec![ImCall::Begin, ImCall::End],
                "skipped {skipped}"
            );
            fake_microphone::stop(&bench.rm, &bench.mic, START_BINDINGS[0]);
        }
    }

    #[test]
    fn a_recording_whose_text_does_not_go_through_the_input_method_leaves_the_source_alone() {
        use crate::settings::PasteMethod;
        for (added, method) in [
            (false, PasteMethod::CtrlV),
            (true, PasteMethod::None),
            (true, PasteMethod::ExternalScript),
        ] {
            let mut bench = Bench::new(fake_microphone::mic());
            bench.app.input_method_added = added;
            bench.app.settings.paste_method = method;
            let outcome = bench.start(START_BINDINGS[0]);
            assert!(matches!(outcome, StartOutcome::Recording(_)));
            fake_microphone::stop(&bench.rm, &bench.mic, START_BINDINGS[0]);
            cues_given(outcome);
            assert_eq!(
                input_method_calls(&bench.log()),
                Vec::new(),
                "added {added}, {method:?}"
            );
            assert!(
                bench.app.kept.lock().unwrap().is_empty(),
                "added {added}, {method:?}: no hold to end"
            );
        }
    }

    #[test]
    fn a_start_that_does_not_record_leaves_the_source_alone() {
        // No model.
        let mut bench = Bench::new(fake_microphone::mic());
        bench.app.input_method_added = true;
        bench.app.settings.paste_method = crate::settings::PasteMethod::CtrlV;
        bench.app.settings.selected_model = String::new();
        bench.app.loaded = None;
        assert!(matches!(
            bench.start(START_BINDINGS[0]),
            StartOutcome::NoModel
        ));
        assert_eq!(input_method_calls(&bench.log()), Vec::new());
        // The microphone doesn't start.
        let mic = fake_microphone::mic();
        mic.fail_next_open(FakeError::PermissionDenied);
        let mut bench = Bench::new(mic);
        bench.app.input_method_added = true;
        bench.app.settings.paste_method = crate::settings::PasteMethod::CtrlV;
        assert!(matches!(
            bench.start(START_BINDINGS[0]),
            StartOutcome::RecordingFailed
        ));
        assert_eq!(input_method_calls(&bench.log()), Vec::new());
    }
}
