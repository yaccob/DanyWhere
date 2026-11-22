//! DanyWhere: the app's side of the ports (`ports`), over the `AppHandle`
//! and its managers. Each method is what the end of a dictation called
//! inline before it went through ports.

use std::sync::Arc;
use std::time::Instant;

use tauri::{AppHandle, Manager};

use super::ports::{
    Deliver, History, Overlay, PostProcess, Recording, Report, Session, Transcribe,
};
use crate::actions::{ProcessedTranscription, TranscriptionOutcome};
use crate::audio_feedback::{play_feedback_sound, SoundType};
use crate::delivery::DictationEnd;
use crate::dictation_context::{CapturedWindow, ContextQuery};
use crate::dictation_key_inserts::ClipboardReason;
use crate::dictation_snapshot::DictationSnapshot;
use crate::managers::audio::AudioRecordingManager;
use crate::managers::history::{HistoryEntry, HistoryManager};
use crate::managers::transcription::{StreamWorkKind, Transcription, TranscriptionManager};
use crate::notices::{self, Notice, NoticeSource};
use crate::profile_redo::{ForInsert, MadeWith, Working};
use crate::profiles::ProfileValues;
use crate::settings::AppSettings;
use crate::tray::{set_tray_state, TrayIconState};
use crate::utils::{self, PasteOutcome};
use crate::TranscriptionCoordinator;

/// The ports of one dictation in the app: its session and cancel
/// generation from the snapshot, the managers from the app's state.
#[derive(Clone)]
pub struct AppPorts {
    app: AppHandle,
    rm: Arc<AudioRecordingManager>,
    tm: Arc<TranscriptionManager>,
    hm: Arc<HistoryManager>,
    session: u64,
    cancel_generation: u64,
}

impl AppPorts {
    pub fn new(app: &AppHandle, snapshot: &DictationSnapshot) -> Self {
        Self {
            app: app.clone(),
            rm: Arc::clone(&app.state::<Arc<AudioRecordingManager>>()),
            tm: Arc::clone(&app.state::<Arc<TranscriptionManager>>()),
            hm: Arc::clone(&app.state::<Arc<HistoryManager>>()),
            session: snapshot.session,
            cancel_generation: snapshot.cancel_generation,
        }
    }
}

impl Transcribe for AppPorts {
    fn stop_recording(&self, binding_id: &str) -> Option<Recording> {
        let samples = self.rm.stop_recording(binding_id, self.cancel_generation)?;
        Some(Recording {
            samples,
            input_silent: self.rm.last_input_silent(),
        })
    }

    fn cancel_stream(&self) {
        self.tm.cancel_stream();
    }

    fn transcribe(&self, samples: Vec<f32>, load_end_model: bool) -> anyhow::Result<Transcription> {
        if load_end_model {
            self.tm.load_wanted_model_after_current(true);
        }
        self.tm.transcribe_recording(samples)
    }
}

impl PostProcess for AppPorts {
    fn post_process(
        &self,
        settings: &AppSettings,
        transcription: &str,
        post_process: bool,
        context: &ContextQuery,
    ) -> impl std::future::Future<Output = ProcessedTranscription> + Send {
        super::post_process_dictation(&self.hm, settings, transcription, post_process, context)
    }
}

impl History for AppPorts {
    fn history(&self) -> &HistoryManager {
        &self.hm
    }

    fn remember(&self, made: MadeWith) {
        crate::profile_redo::remember(made);
    }
}

impl Deliver for AppPorts {
    fn copies_instead_of_pasting(&self, session: u64) -> bool {
        crate::dictation_key_inserts::copies_instead_of_pasting(session)
    }

    fn call_waiting(&self) -> bool {
        crate::insert_service::call_waiting()
    }

    fn redo_for_app_in_front(
        &self,
        entry: &HistoryEntry,
        show: &dyn Fn(Working),
    ) -> Option<ForInsert> {
        crate::profile_redo::for_insert(&self.app, entry, show)
    }

    fn apply_hook(&self, text: String) -> impl std::future::Future<Output = String> + Send {
        crate::transcription_hook::apply(&self.app, text)
    }

    fn paste(
        &self,
        text: String,
        values: Option<ProfileValues>,
        append_text: &str,
    ) -> Result<PasteOutcome, String> {
        utils::paste_with_values(text, self.app.clone(), values, append_text)
    }

    fn deliver_to_waiting_call(&self, text: &str) -> bool {
        crate::insert_service::deliver_to_waiting_call(text)
    }

    fn put_on_clipboard_if_requested(&self, session: u64, text: &str) -> Option<ClipboardReason> {
        crate::dictation_key_inserts::put_on_clipboard_if_requested(&self.app, session, text)
    }
}

impl Report for AppPorts {
    fn tray(&self, state: TrayIconState) {
        set_tray_state(&self.app, state);
    }

    fn overlay(&self, overlay: Overlay) {
        match overlay {
            Overlay::Working {
                working,
                streaming: true,
            } => self.tm.emit_stream_working(match working {
                Working::Transcribing => StreamWorkKind::Transcribing,
                Working::PostProcessing => StreamWorkKind::Polishing,
            }),
            Overlay::Working {
                working: Working::Transcribing,
                streaming: false,
            } => utils::show_transcribing_overlay(&self.app),
            Overlay::Working {
                working: Working::PostProcessing,
                streaming: false,
            } => utils::show_processing_overlay(&self.app),
            Overlay::Hide => utils::hide_recording_overlay(&self.app),
            Overlay::AfterDictation => crate::delivery::finish_overlay_after_dictation(&self.app),
            Overlay::OnClipboard(reason) => {
                crate::dictation_key_inserts::show_on_clipboard_notice(&self.app, &reason)
            }
        }
    }

    fn stop_sound(&self) {
        self.rm.remove_mute();
        play_feedback_sound(&self.app, SoundType::Stop);
    }

    fn sends_context(&self, sends: bool) {
        crate::send_context::decide_at_stop(&self.app, sends);
    }

    fn sound_after_dictation(&self, end: DictationEnd) {
        crate::delivery::play_sound_after_dictation(&self.app, end);
    }

    fn raise(&self, notice: Notice) {
        notices::raise(&self.app, notice);
    }

    fn raise_without_overlay(&self, notice: Notice) {
        notices::raise_without_overlay(&self.app, notice);
    }

    fn clear_notice(&self) {
        notices::clear(&self.app, NoticeSource::Dictation);
    }

    fn complete(&self, outcome: TranscriptionOutcome, sample_count: usize, stop_time: Instant) {
        crate::actions::emit_transcription_complete(&self.app, outcome, sample_count, stop_time);
    }
}

impl Session for AppPorts {
    fn cancelled(&self) -> bool {
        self.rm.was_cancelled_since(self.cancel_generation)
    }

    fn owns_ui(&self) -> bool {
        crate::actions::current_recording_session() == self.session
    }

    fn begin_window_capture(&self, settings: &AppSettings) {
        crate::window_capture::begin_capture(settings, self.session);
    }

    fn take_window(&self) -> Option<CapturedWindow> {
        crate::window_capture::take_capture(self.session)
    }

    fn spawn_paste(
        &self,
        values: ProfileValues,
        run: Box<dyn FnOnce() + Send>,
    ) -> std::io::Result<()> {
        std::thread::Builder::new()
            .name("paste".into())
            .spawn(move || {
                crate::app_profiles::keep_values_here(values);
                run();
            })
            .map(|_| ())
    }

    /// Finishes the transcription pipeline, including immediate model
    /// unloading on early exits.
    fn finished(&self) {
        // DanyWhere: the dictation's own profile ends with it, before the
        // coordinator may start the next one.
        crate::app_profiles::end_dictation();
        self.tm.maybe_unload_immediately("transcription session");
        if let Some(c) = self.app.try_state::<TranscriptionCoordinator>() {
            c.notify_processing_finished();
        }
        // The pipeline just freed its large transient buffers (captured PCM,
        // WAV copy, engine scratch); hand the cached pages back to the OS so
        // they don't sit in malloc arenas until they get swapped out (#1792).
        crate::memory::trim_freed_memory();
    }
}
