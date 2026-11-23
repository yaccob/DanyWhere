//! DanyWhere: the app as the ports of a dictation's start
//! ([`super::StartPorts`]): each port does what Handy's
//! `TranscribeAction::start` did there.

use std::sync::Arc;

use log::debug;
use tauri::{AppHandle, Manager};

use super::{
    start_dictation, Microphone, StartModel, StartNotice, StartOutcome, StartOverlay, StartPorts,
    StartProfile, StartReport, StartSession,
};
use crate::app_profiles::OverlayProfile;
use crate::audio_feedback::{play_feedback_sound_blocking, SoundType};
use crate::audio_toolkit::VadPolicy;
use crate::input_method::InputMethod;
use crate::managers::audio::{AudioRecordingManager, RecordingReadiness};
use crate::managers::model::ModelManager;
use crate::managers::transcription::TranscriptionManager;
use crate::notices::{self, Notice, NoticeSource};
use crate::profiles::ProfileValues;
use crate::settings::AppSettings;
use crate::tray::{set_tray_state, TrayIconState};

/// Starts a dictation with start shortcut `binding_id` in the app.
pub(crate) fn start(app: &AppHandle, binding_id: &str) -> StartOutcome {
    let rm = Arc::clone(app.state::<Arc<AudioRecordingManager>>().inner());
    let tm = Arc::clone(app.state::<Arc<TranscriptionManager>>().inner());
    let ports = AppStart {
        app: app.clone(),
        tm: Arc::clone(&tm),
    };
    let report = AppReport {
        app: app.clone(),
        tm,
        rm: Arc::clone(&rm),
    };
    start_dictation(
        binding_id,
        &StartPorts {
            profile: &ports,
            model: &ports,
            microphone: rm,
            report: Arc::new(report),
            session: &ports,
        },
    )
}

/// The dictation's profile, the speech model and the session in the app.
struct AppStart {
    app: AppHandle,
    tm: Arc<TranscriptionManager>,
}

impl StartProfile for AppStart {
    fn begin(&self) -> Option<OverlayProfile> {
        crate::app_profiles::begin_dictation(&self.app)
    }

    fn end(&self) {
        crate::app_profiles::end_dictation();
    }

    fn settings(&self) -> AppSettings {
        crate::app_profiles::dictation_settings(&self.app)
    }

    fn values(&self) -> ProfileValues {
        crate::app_profiles::dictation_profile_and_values(&self.app).1
    }
}

impl StartModel for AppStart {
    fn initiate_load(&self) -> bool {
        self.tm.initiate_model_load(true)
    }

    fn loaded(&self) -> Option<String> {
        self.tm
            .is_model_loaded()
            .then(|| self.tm.get_current_model())
            .flatten()
    }

    fn files(&self, model: &str) -> Result<(), String> {
        self.app
            .state::<Arc<ModelManager>>()
            .get_model_path(model)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn streams(&self, model: &str) -> bool {
        self.app
            .state::<Arc<ModelManager>>()
            .get_model_info(model)
            .is_some_and(|m| m.supports_streaming)
    }

    fn start_stream(&self) {
        self.tm.start_stream();
    }

    fn cancel_stream(&self) {
        self.tm.cancel_stream();
    }
}

impl StartSession for AppStart {
    fn begin(&self) {
        crate::actions::begin_recording_session();
    }

    fn prime_title_query(&self) {
        crate::window_capture::prime_title_query(&self.app);
    }

    fn register_cancel_shortcut(&self) {
        // Dynamically registered in a separate task to avoid deadlock.
        crate::shortcut::register_cancel_shortcut(&self.app);
    }

    fn warm_up(&self, settings: &AppSettings, post_process: bool) {
        crate::model_warmup::start(settings, post_process);
    }

    fn input_method(&self) -> Arc<dyn InputMethod> {
        crate::input_method::current()
    }

    fn keep_input_source(&self, hold: crate::delivery::InputSourceHold) {
        crate::delivery::keep_recording_hold(hold);
    }
}

/// Tray, overlay, sounds, the system's sound and notices in the app.
struct AppReport {
    app: AppHandle,
    tm: Arc<TranscriptionManager>,
    rm: Arc<AudioRecordingManager>,
}

impl StartReport for AppReport {
    fn tray(&self, state: TrayIconState) {
        set_tray_state(&self.app, state);
    }

    fn profile(&self, shown: Option<OverlayProfile>) {
        crate::overlay::set_dictation_profile(&self.app, shown);
    }

    fn context_bubble(&self, sends: bool) {
        crate::send_context::begin_dictation(&self.app, sends);
    }

    fn overlay(&self, overlay: StartOverlay) {
        match overlay {
            StartOverlay::Streaming => crate::utils::show_streaming_overlay(&self.app),
            StartOverlay::Recording => crate::utils::show_recording_overlay(&self.app),
            StartOverlay::Hidden => {} // show_overlay_state no-ops on None anyway
        }
    }

    fn ready(&self) {
        crate::utils::emit_recording_ready(&self.app);
    }

    fn start_sound(&self) {
        // The helper returns immediately when feedback is disabled.
        play_feedback_sound_blocking(&self.app, SoundType::Start);
    }

    fn mute(&self) {
        self.rm.apply_mute();
    }

    fn notice(&self, notice: StartNotice) {
        match notice {
            // Not over the overlay of a dictation still in progress.
            StartNotice::NoModelSelected => self.tm.raise_no_model_selected(),
            StartNotice::ModelLoadFailed { model, error } => self
                .tm
                .raise_model_load_failed(&model, &anyhow::anyhow!(error)),
            StartNotice::RecordingFailed { code, detail } => notices::raise(
                &self.app,
                Notice::new(code, NoticeSource::Dictation).with_detail(detail),
            ),
        }
    }
}

impl Microphone for AudioRecordingManager {
    fn preload_vad_in_background(&self) {
        // A clone shares every piece of state with this manager.
        let rm = self.clone();
        std::thread::spawn(move || {
            if let Err(e) = rm.preload_vad() {
                debug!("VAD pre-load failed: {}", e);
            }
        });
    }

    fn try_start(
        &self,
        binding_id: &str,
        vad_policy: VadPolicy,
    ) -> anyhow::Result<RecordingReadiness> {
        self.try_start_recording(binding_id, vad_policy)
    }

    fn readiness_current(&self, generation: u64) -> bool {
        self.is_recording_readiness_current(generation)
    }
}
