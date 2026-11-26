//! Background transcriptions (a file, or an entry transcribed or
//! post-processed again), one at a time in chunks that yield to dictation.
//! Progress is kept in memory.

use anyhow::{anyhow, Result};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::cell::Cell;
use std::collections::VecDeque;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use crate::actions::process_transcription_output_with;
use crate::audio_toolkit::{decode_audio_file, recording_file_name, save_recording};
use crate::history_versions::{NewTranscription, PostProcessTarget, PostProcessed, VersionError};
use crate::managers::history::{HistoryEntry, HistoryManager};
use crate::managers::transcription::{
    panic_payload_message, ChunkControl, Transcription, TranscriptionManager,
};
use crate::notices::{self, Notice, NoticeCode, NoticeSource};
use crate::post_process_failure::PostProcessFailure;
use crate::settings::PostProcessMode;

/// How often a waiting chunk checks whether dictation has finished.
const DICTATION_POLL: Duration = Duration::from_millis(200);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TranscriptionJobStatus {
    /// Waiting behind another job.
    Queued,
    /// `percent` of the audio is transcribed.
    Running {
        percent: u8,
    },
    /// The transcript is being post-processed.
    PostProcessing,
    /// The entry has its new text, or the job was cancelled.
    Finished,
    /// DanyWhere: a post-processing from History came back with the text
    /// the top version already has, so the entry has no new version
    /// (`history_versions::apply_post_processing`); History says so.
    Unchanged,
    Failed {
        error: String,
    },
}

/// A background transcription of one History entry.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct TranscriptionJob {
    pub entry_id: i64,
    pub status: TranscriptionJobStatus,
}

/// What a History entry's job does again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ReprocessMode {
    /// Transcribe the recording again, and post-process the result if the
    /// entry was post-processed before.
    Transcribe,
    /// Transcribe the recording again and post-process the result.
    TranscribeAndPostProcess,
    /// Keep the versions and post-process one of them, the top unless the
    /// job names another; the result is a new version on top
    /// (`history_versions`). Needs no recording.
    PostProcess,
}

impl ReprocessMode {
    /// Whether the job transcribes the recording.
    fn transcribes(self) -> bool {
        self != Self::PostProcess
    }

    /// Whether the job post-processes, given whether the entry was
    /// post-processed before. An entry a job post-processed stays marked
    /// that way, so transcribing it again later post-processes too.
    fn post_processes(self, requested: bool) -> bool {
        match self {
            Self::Transcribe => requested,
            _ => true,
        }
    }
}

/// The text a post-processing from History starts from, version `version`
/// or the top, and the word-list hint that goes with it: only with the
/// entry's transcript as the speech model wrote it, which is what the word
/// list replaced in. The transcript is found by its step, wherever the user
/// moved it (`history_versions::transcript_index`).
fn post_process_input(
    entry: &HistoryEntry,
    version: Option<u32>,
) -> std::result::Result<(&str, Vec<crate::dictation_context::WordReplacement>), VersionError> {
    let input = crate::history_versions::post_processing_input(&entry.versions, version)?;
    let as_transcribed = crate::history_versions::transcript_index(&entry.versions) == Some(input)
        && !entry.versions[input].edited_by_user();
    let hint = if as_transcribed {
        entry.window_context.word_replacements.clone()
    } else {
        Vec::new()
    };
    Ok((entry.versions[input].text.as_str(), hint))
}

/// The notice for a post-processing that failed: after a transcription the
/// new transcript is kept as it is, without one the entry is unchanged.
/// DanyWhere: it names the reason (`post_process_failure`).
fn post_process_failure_notice(failure: &PostProcessFailure, transcribed: bool) -> Notice {
    let code = if transcribed {
        NoticeCode::FilePostProcessFailed
    } else {
        NoticeCode::HistoryPostProcessFailed
    };
    failure.notice(code, NoticeSource::File)
}

/// DanyWhere: which part of an entry a transcription hears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    /// The recording: the dictation's text.
    Text,
    /// The audio of its spoken instruction.
    Instruction,
}

/// DanyWhere: what transcribing an entry again heard.
struct HeardAgain {
    text: Transcription,
    /// The spoken instruction, heard again from its own audio; `None` when
    /// the entry has none, or its audio is gone or can't be read (the text
    /// the entry has then stays).
    instruction: Option<String>,
}

/// DanyWhere: hears `entry` again with `transcribe`, as "Transcribe Again"
/// does: its recording, then the audio of its spoken instruction when it
/// has one, with the same model and settings (F6 ii). `None`: cancelled.
fn hear_again(
    entry: &HistoryEntry,
    recordings_dir: &Path,
    transcribe: &mut dyn FnMut(Vec<f32>, Part) -> Result<Option<Transcription>>,
) -> Result<Option<HeardAgain>> {
    let samples = HistoryManager::recording_samples_in(entry, recordings_dir)
        .map_err(|e| anyhow!("Failed to load audio: {}", e))?;
    let instruction_audio = instruction_samples(entry, recordings_dir);
    // DanyWhere: an entry made of a spoken instruction alone (F5 i) has an
    // empty recording; only its instruction is heard.
    let text = if samples.is_empty() {
        if instruction_audio.is_none() {
            return Err(anyhow!("Recording has no audio samples"));
        }
        Transcription {
            text: String::new(),
            language: None,
            word_replacements: Vec::new(),
            model_id: None,
            takes_vocabulary: false,
        }
    } else {
        let Some(text) = transcribe(samples, Part::Text)? else {
            return Ok(None);
        };
        text
    };
    let instruction = match instruction_audio {
        Some(samples) => match transcribe(samples, Part::Instruction)? {
            Some(heard) => Some(heard.text.trim().to_string()),
            None => return Ok(None),
        },
        None => None,
    };
    Ok(Some(HeardAgain { text, instruction }))
}

/// DanyWhere: the spoken instruction a job post-processes `entry` with: the
/// one `heard` again from its audio, else the one the entry keeps; none for
/// the wand's "Without the spoken instruction" (`use_instruction` false) or
/// an entry without one.
fn instruction_for(
    entry: &HistoryEntry,
    heard: Option<String>,
    use_instruction: bool,
) -> Option<String> {
    if !use_instruction {
        return None;
    }
    heard.or_else(|| entry.spoken_instruction.clone())
}

/// DanyWhere: whether a job of `mode` on an entry post-processed before as
/// `requested` asks for post-processing: with a spoken `instruction` always,
/// as the dictation did (`dictation_snapshot::wants_post_processing`). An
/// entry keeps this as its mark, so a later job post-processes it too.
fn job_wants_post_processing(
    mode: ReprocessMode,
    requested: bool,
    instruction: Option<&str>,
) -> bool {
    mode.post_processes(requested) || instruction.is_some()
}

/// DanyWhere: whether the job post-processes, in the profile in use with
/// `profile_mode`: as it asks ([`job_wants_post_processing`]), unless that
/// profile never post-processes; then nothing goes to a provider, as
/// History's page offers neither the wand nor post-processing with
/// "Re-transcribe" then (`postProcessEntry.ts`, `postProcessAvailability`).
fn job_post_processes(
    profile_mode: PostProcessMode,
    mode: ReprocessMode,
    requested: bool,
    instruction: Option<&str>,
) -> bool {
    profile_mode.allows() && job_wants_post_processing(mode, requested, instruction)
}

/// DanyWhere: a transcription heard again as `text` has nothing to work on:
/// no words, and no spoken instruction that could work alone (F5 i).
fn nothing_to_work_on(text: &str, instruction: Option<&str>) -> bool {
    text.trim().is_empty() && instruction.is_none_or(|instruction| instruction.trim().is_empty())
}

/// DanyWhere: the samples of the entry's spoken instruction, when its audio
/// is there and has any; else `None`, and the instruction keeps its text.
fn instruction_samples(entry: &HistoryEntry, recordings_dir: &Path) -> Option<Vec<f32>> {
    let file = entry.spoken_instruction_file.as_deref()?;
    if entry.recording_removed {
        return None;
    }
    match crate::audio_toolkit::read_recording_samples(recordings_dir.join(file)) {
        Ok(samples) if !samples.is_empty() => Some(samples),
        Ok(_) => None,
        Err(e) => {
            warn!(
                "Kept the spoken instruction of history entry {} as it was: {}",
                entry.id, e
            );
            None
        }
    }
}

struct Running {
    entry_id: i64,
    cancelled: Arc<AtomicBool>,
}

/// What a job does to its entry.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Work {
    mode: ReprocessMode,
    /// The version a post-processing starts from; `None`: the top.
    version: Option<u32>,
    /// DanyWhere: the prompts of the library its post-processing runs, in
    /// this order; `None`: the profile's active ones (`prompt_library`).
    prompt_ids: Option<Vec<String>>,
    /// DanyWhere: its post-processing runs with the entry's spoken
    /// instruction, if it has one; false: the wand's "Without the spoken
    /// instruction".
    use_instruction: bool,
}

impl Work {
    /// A file's first transcription, or an entry's again.
    const TRANSCRIBE: Work = Work {
        mode: ReprocessMode::Transcribe,
        version: None,
        prompt_ids: None,
        use_instruction: true,
    };
}

#[derive(Default)]
struct Jobs {
    /// Entry ids waiting to be transcribed or post-processed again, with
    /// what to do. They go before files: someone is waiting on them.
    retries: VecDeque<(i64, Work)>,
    /// Entry ids of files waiting to be transcribed.
    files: VecDeque<i64>,
    running: Option<Running>,
}

impl Jobs {
    fn has(&self, entry_id: i64) -> bool {
        self.running
            .as_ref()
            .is_some_and(|r| r.entry_id == entry_id)
            || self.retries.iter().any(|&(id, _)| id == entry_id)
            || self.files.contains(&entry_id)
    }

    fn is_idle(&self) -> bool {
        self.running.is_none() && self.retries.is_empty() && self.files.is_empty()
    }

    /// Queues the entry unless it already has a job; whether it was queued.
    /// `None` is a file, transcribed for the first time.
    fn push(&mut self, entry_id: i64, work: Option<Work>) -> bool {
        if self.has(entry_id) {
            return false;
        }
        match work {
            Some(work) => self.retries.push_back((entry_id, work)),
            None => self.files.push_back(entry_id),
        }
        true
    }

    /// The next job to run: entries someone waits on, then files. A file
    /// is transcribed like an entry transcribed again.
    fn next(&mut self) -> Option<(i64, Work)> {
        self.retries
            .pop_front()
            .or_else(|| self.files.pop_front().map(|id| (id, Work::TRANSCRIBE)))
    }

    /// Drops the entry from the queues; whether it was in one.
    fn unqueue(&mut self, entry_id: i64) -> bool {
        let before = self.retries.len() + self.files.len();
        self.retries.retain(|&(id, _)| id != entry_id);
        self.files.retain(|&id| id != entry_id);
        self.retries.len() + self.files.len() != before
    }
}

pub struct TranscriptionJobManager {
    app: AppHandle,
    jobs: Mutex<Jobs>,
    wake: Condvar,
}

impl TranscriptionJobManager {
    /// Starts the thread that runs the jobs.
    pub fn new(app: &AppHandle) -> Arc<Self> {
        let manager = Arc::new(Self {
            app: app.clone(),
            jobs: Mutex::new(Jobs::default()),
            wake: Condvar::new(),
        });
        let runner = Arc::clone(&manager);
        thread::Builder::new()
            .name("transcription-jobs".into())
            .spawn(move || runner.run_jobs())
            .expect("Failed to start the transcription job thread");
        manager
    }

    fn lock(&self) -> MutexGuard<'_, Jobs> {
        self.jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Decodes `path` into a recording, then adds it to History and queues
    /// it. The recording exists first, so no entry is ever without audio.
    pub fn transcribe_file(&self, path: PathBuf) -> Result<HistoryEntry> {
        // A malformed file may panic the decoder.
        let samples = catch_unwind(AssertUnwindSafe(|| decode_audio_file(&path))).unwrap_or_else(
            |payload| {
                Err(anyhow!(
                    "Failed to read the file: {}",
                    panic_payload_message(payload.as_ref())
                ))
            },
        )?;
        if samples.is_empty() {
            return Err(anyhow!("The file has no audio"));
        }

        let hm = self.app.state::<Arc<HistoryManager>>();
        let file_name = recording_file_name(&format!(
            "handy-file-{}",
            chrono::Utc::now().timestamp_millis()
        ));
        let recording = hm.get_audio_file_path(&file_name);
        // Written aside and renamed, so a crash never leaves a partial one.
        let partial = recording.with_file_name(format!("{file_name}.partial"));
        if let Err(e) = save_recording(&partial, &samples)
            .and_then(|()| std::fs::rename(&partial, &recording).map_err(Into::into))
        {
            remove_recording(&partial);
            return Err(e);
        }
        drop(samples);

        let source_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let entry = match hm.save_file_entry(file_name, source_name) {
            Ok(entry) => entry,
            Err(e) => {
                remove_recording(&recording);
                return Err(e);
            }
        };
        info!("Queued file transcription for history entry {}", entry.id);
        self.queue(entry.id, None);
        Ok(entry)
    }

    /// Queues the entry to be transcribed again from its recording, ahead
    /// of any file.
    pub fn retry(&self, entry_id: i64) {
        self.reprocess(entry_id, ReprocessMode::Transcribe, None);
    }

    /// Queues the entry to be transcribed or post-processed again, ahead of
    /// any file. `version`: the version a post-processing starts from
    /// (`None`: the top); its result goes on top.
    pub fn reprocess(&self, entry_id: i64, mode: ReprocessMode, version: Option<u32>) {
        self.reprocess_with_prompts(entry_id, mode, version, None, true);
    }

    /// DanyWhere: [`Self::reprocess`]; with `prompt_ids` its post-processing
    /// runs exactly those prompts of the library, in that order, instead of
    /// the profile's active ones (History's wand menu, `prompt_library`).
    /// `use_instruction`: with the entry's spoken instruction, if it has
    /// one; false for the wand's "Without the spoken instruction".
    pub fn reprocess_with_prompts(
        &self,
        entry_id: i64,
        mode: ReprocessMode,
        version: Option<u32>,
        prompt_ids: Option<Vec<String>>,
        use_instruction: bool,
    ) {
        self.queue(
            entry_id,
            Some(Work {
                mode,
                version,
                prompt_ids,
                use_instruction,
            }),
        );
    }

    /// Queues the entry's job, unless it already has one. `None` is a file,
    /// transcribed for the first time.
    fn queue(&self, entry_id: i64, work: Option<Work>) {
        {
            let mut jobs = self.lock();
            if !jobs.push(entry_id, work) {
                return;
            }
            self.sync_job_active(&jobs);
        }
        self.emit(entry_id, TranscriptionJobStatus::Queued);
        crate::tray::update_tray_menu(&self.app);
        self.wake.notify_one();
    }

    /// Stops the entry's job: a queued one is dropped, a running one stops
    /// before its next chunk.
    pub fn cancel(&self, entry_id: i64) {
        let (was_queued, idle) = {
            let mut jobs = self.lock();
            let was_queued = jobs.unqueue(entry_id);
            if let Some(running) = jobs.running.as_ref().filter(|r| r.entry_id == entry_id) {
                running.cancelled.store(true, Ordering::Release);
            }
            self.sync_job_active(&jobs);
            (was_queued, jobs.is_idle())
        };
        // A running job reports its own end from the job loop; a queued one
        // never reaches it, so it is reported here.
        if was_queued {
            if idle && !crate::tray::dictation_in_progress(&self.app) {
                self.app
                    .state::<Arc<TranscriptionManager>>()
                    .maybe_unload_immediately("background transcription");
            }
            crate::tray::update_tray_menu(&self.app);
            self.emit(entry_id, TranscriptionJobStatus::Finished);
        }
    }

    /// Keeps the model held for as long as any job is queued or running.
    fn sync_job_active(&self, jobs: &Jobs) {
        self.app
            .state::<Arc<TranscriptionManager>>()
            .set_job_active(!jobs.is_idle());
    }

    fn emit(&self, entry_id: i64, status: TranscriptionJobStatus) {
        if let Err(e) = (TranscriptionJob { entry_id, status }).emit(&self.app) {
            error!("Failed to emit transcription job event: {}", e);
        }
    }

    fn run_jobs(&self) {
        loop {
            let (entry_id, work, cancelled) = {
                let mut jobs = self.lock();
                let (entry_id, work) = loop {
                    if let Some(job) = jobs.next() {
                        break job;
                    }
                    jobs = self
                        .wake
                        .wait(jobs)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                };
                let cancelled = Arc::new(AtomicBool::new(false));
                jobs.running = Some(Running {
                    entry_id,
                    cancelled: Arc::clone(&cancelled),
                });
                (entry_id, work, cancelled)
            };
            let mode = work.mode;
            self.emit(
                entry_id,
                if mode.transcribes() {
                    TranscriptionJobStatus::Running { percent: 0 }
                } else {
                    TranscriptionJobStatus::PostProcessing
                },
            );

            // A panic fails only this job; the rest of the loop still has to
            // run, or the model would stay held and the queue stuck.
            let result = catch_unwind(AssertUnwindSafe(|| {
                self.run_job(entry_id, work, &cancelled)
            }))
            .unwrap_or_else(|payload| Err(crashed(payload.as_ref())));

            let idle = {
                let mut jobs = self.lock();
                jobs.running = None;
                self.sync_job_active(&jobs);
                jobs.is_idle()
            };
            let tm = self.app.state::<Arc<TranscriptionManager>>();
            if idle {
                // A dictation under way unloads the model itself when it ends.
                if !crate::tray::dictation_in_progress(&self.app) {
                    tm.maybe_unload_immediately("background transcription");
                }
                crate::tray::update_tray_menu(&self.app);
            }
            let status = match result {
                Err(e) if !cancelled.load(Ordering::Acquire) => {
                    let (what, code) = if mode.transcribes() {
                        ("Transcription", NoticeCode::FileTranscriptionFailed)
                    } else {
                        ("Post-processing", NoticeCode::HistoryPostProcessFailed)
                    };
                    warn!("{} of history entry {} failed: {}", what, entry_id, e);
                    notices::raise(
                        &self.app,
                        Notice::new(code, NoticeSource::File).with_detail(e.to_string()),
                    );
                    TranscriptionJobStatus::Failed {
                        error: e.to_string(),
                    }
                }
                Ok(JobEnd::Finished {
                    post_process_failure,
                    transcribed,
                    unchanged,
                }) => {
                    // Clear first, so the warning about this job survives it.
                    notices::clear(&self.app, NoticeSource::File);
                    if let Some(failure) = &post_process_failure {
                        notices::raise(
                            &self.app,
                            post_process_failure_notice(failure, transcribed),
                        );
                    }
                    finished_status(unchanged)
                }
                _ => TranscriptionJobStatus::Finished,
            };
            self.emit(entry_id, status);
        }
    }

    fn run_job(&self, entry_id: i64, work: Work, cancelled: &AtomicBool) -> Result<JobEnd> {
        let hm = Arc::clone(&self.app.state::<Arc<HistoryManager>>());
        let entry = tauri::async_runtime::block_on(hm.get_entry_by_id(entry_id))?
            .ok_or_else(|| anyhow!("History entry {} not found", entry_id))?;
        let prompts = work.prompt_ids.as_deref();
        // DanyWhere: the mode of the profile in use, which the job runs with
        // (`app_profiles::dictation_settings`): with "never" nothing goes to
        // a provider.
        let profile_mode = crate::app_profiles::dictation_settings(&self.app).post_process_mode;
        if !work.mode.transcribes() {
            let instruction = instruction_for(&entry, None, work.use_instruction);
            if !job_post_processes(
                profile_mode,
                work.mode,
                entry.post_process_requested,
                instruction.as_deref(),
            ) {
                return Err(anyhow!(
                    "Post-processing is set to \"Never\" in the profile in use"
                ));
            }
            return self.post_process_again(
                &hm,
                &entry,
                work.version,
                cancelled,
                prompts,
                instruction.as_deref(),
            );
        }

        let tm = Arc::clone(&self.app.state::<Arc<TranscriptionManager>>());
        let control = Control {
            manager: self,
            entry_id: entry.id,
            cancelled,
            percent: Cell::new(0),
        };
        // DanyWhere: the text, and the spoken instruction from its own audio
        // when the entry has one (F6 ii); the instruction's chunks report no
        // progress, the text's already went to 100 %.
        let Some(HeardAgain {
            text:
                Transcription {
                    text,
                    language,
                    word_replacements,
                    model_id,
                    ..
                },
            instruction,
        }) = hear_again(
            &entry,
            hm.recordings_dir(),
            &mut |samples, part| match part {
                Part::Text => tm.transcribe_in_chunks(samples, &control),
                Part::Instruction => tm.transcribe_in_chunks(samples, &Quiet(&control)),
            },
        )?
        else {
            return Ok(JobEnd::Cancelled);
        };
        // DanyWhere: post-processed with the spoken instruction heard again
        // (else the one the entry keeps), as the dictation was.
        let heard_again = instruction.clone();
        let instruction = instruction_for(&entry, instruction, work.use_instruction);
        if nothing_to_work_on(&text, instruction.as_deref()) {
            return Err(anyhow!("Recording contains no speech"));
        }
        if cancelled.load(Ordering::Acquire) {
            return Ok(JobEnd::Cancelled);
        }
        let wanted = job_wants_post_processing(
            work.mode,
            entry.post_process_requested,
            instruction.as_deref(),
        );
        let post_process = job_post_processes(
            profile_mode,
            work.mode,
            entry.post_process_requested,
            instruction.as_deref(),
        );

        if post_process {
            self.emit(entry.id, TranscriptionJobStatus::PostProcessing);
        }
        // DanyWhere: the entries made before it in the same window go along
        // as context when it was dictated with context (`dictation_context`).
        let with_context = post_process && entry.window_context.context_requested;
        let mut context_query =
            crate::dictation_context::ContextQuery::for_entry(&entry, word_replacements.clone());
        if !with_context {
            context_query.window = None;
        }
        let processed = tauri::async_runtime::block_on(process_transcription_output_with(
            &self.app,
            &text,
            post_process,
            &context_query,
            prompts,
            instruction.as_deref(),
        ));
        let instruction_applied = processed.post_processed_text.is_some()
            && instruction
                .as_deref()
                .is_some_and(|instruction| !instruction.trim().is_empty());
        // Post-processing can take a while; a cancel during it still wins.
        if cancelled.load(Ordering::Acquire) {
            return Ok(JobEnd::Cancelled);
        }
        let post_process_failure = processed.post_process_failure.clone();
        let context_entry_ids = processed.context_entry_ids.clone();
        // DanyWhere: the new transcription goes on top; the versions there
        // stay (`history_versions`).
        hm.add_transcription(
            entry.id,
            NewTranscription {
                text,
                language,
                post_processed: processed.post_processed_text.map(|text| PostProcessed {
                    text,
                    prompt: processed.post_process_prompt,
                    prompt_name: processed.post_process_prompt_name,
                    spoken_instruction: instruction_applied,
                }),
                // DanyWhere: the mark keeps what the job asked for, also
                // where "never" kept it from post-processing.
                post_process_requested: wanted,
                model: model_id,
                spoken_instruction: heard_again,
                instruction_applied,
            },
        )?;
        if let Err(e) = hm.record_entry_context(
            entry.id,
            None,
            &word_replacements,
            with_context.then_some(context_entry_ids.as_slice()),
        ) {
            warn!("Failed to store the entry's context: {}", e);
        }
        Ok(JobEnd::Finished {
            post_process_failure,
            transcribed: true,
            unchanged: false,
        })
    }

    /// Post-processes version `version` of the entry (`None`: the top); the
    /// result goes on top, every version stays (`history_versions`). Needs
    /// no recording and no model. When nothing new comes out, the entry
    /// keeps what it had.
    fn post_process_again(
        &self,
        hm: &HistoryManager,
        entry: &HistoryEntry,
        version: Option<u32>,
        cancelled: &AtomicBool,
        prompts: Option<&[String]>,
        instruction: Option<&str>,
    ) -> Result<JobEnd> {
        let (transcript, replacements) = post_process_input(entry, version)?;
        if transcript.trim().is_empty() {
            return Err(anyhow!("The entry has no transcript to post-process"));
        }
        // DanyWhere: the entries made before it in the same window go along
        // as context when it was dictated with context.
        let with_context = entry.window_context.context_requested;
        let mut context_query =
            crate::dictation_context::ContextQuery::for_entry(entry, replacements);
        if !with_context {
            context_query.window = None;
        }
        let processed = tauri::async_runtime::block_on(process_transcription_output_with(
            &self.app,
            transcript,
            true,
            &context_query,
            prompts,
            instruction,
        ));
        if cancelled.load(Ordering::Acquire) {
            return Ok(JobEnd::Cancelled);
        }
        if with_context {
            if let Err(e) = hm.record_entry_context(
                entry.id,
                None,
                &entry.window_context.word_replacements,
                Some(&processed.context_entry_ids),
            ) {
                warn!("Failed to store the entry's context: {}", e);
            }
        }
        // The result goes onto the stack as it is now.
        let (post_process_failure, unchanged) = match processed.post_processed_text {
            Some(text) => {
                let stored = hm.store_post_processing(
                    entry.id,
                    PostProcessTarget::OnTop,
                    PostProcessed {
                        text,
                        prompt: processed.post_process_prompt,
                        prompt_name: processed.post_process_prompt_name,
                        // DanyWhere: the wand with the entry's instruction.
                        spoken_instruction: instruction
                            .is_some_and(|instruction| !instruction.trim().is_empty()),
                    },
                )?;
                (None, stored.unchanged)
            }
            None => (
                Some(
                    processed
                        .post_process_failure
                        .unwrap_or_else(PostProcessFailure::unknown),
                ),
                false,
            ),
        };
        Ok(JobEnd::Finished {
            post_process_failure,
            transcribed: false,
            unchanged,
        })
    }
}

/// A cancelled job changes no notices.
enum JobEnd {
    /// `transcribed` is false when the job only post-processed;
    /// `unchanged`: its result equals the entry's top version;
    /// `post_process_failure`: why its post-processing failed, if it did.
    Finished {
        post_process_failure: Option<PostProcessFailure>,
        transcribed: bool,
        unchanged: bool,
    },
    Cancelled,
}

/// The status a job that ran to its end reports: `Unchanged` when its
/// post-processing added nothing, so History can say why no version came.
fn finished_status(unchanged: bool) -> TranscriptionJobStatus {
    if unchanged {
        TranscriptionJobStatus::Unchanged
    } else {
        TranscriptionJobStatus::Finished
    }
}

/// Best-effort cleanup; the caller is already reporting the failure.
fn remove_recording(path: &Path) {
    if let Err(e) = std::fs::remove_file(path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            error!("Failed to remove {}: {}", path.display(), e);
        }
    }
}

fn crashed(payload: &(dyn std::any::Any + Send)) -> anyhow::Error {
    anyhow!("Transcription crashed: {}", panic_payload_message(payload))
}

struct Control<'a> {
    manager: &'a TranscriptionJobManager,
    entry_id: i64,
    cancelled: &'a AtomicBool,
    /// The last percent reported; the job starts at 0.
    percent: Cell<u8>,
}

impl ChunkControl for Control<'_> {
    fn proceed(&self) -> bool {
        // Dictation goes first: wait out a recording and its transcription.
        // A chunk started while a stream is open would end that stream.
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return false;
            }
            if !crate::tray::dictation_in_progress(&self.manager.app) {
                return true;
            }
            thread::sleep(DICTATION_POLL);
        }
    }

    fn progress(&self, done: usize, total: usize) {
        // The chunk a cancel caught still finishes; don't report it as running.
        if self.cancelled.load(Ordering::Acquire) {
            return;
        }
        let percent = (done * 100 / total.max(1)).min(100) as u8;
        if self.percent.replace(percent) == percent {
            return;
        }
        self.manager
            .emit(self.entry_id, TranscriptionJobStatus::Running { percent });
    }
}

/// DanyWhere: the chunks of a spoken instruction heard again: they wait for
/// dictation and stop on a cancel as the text's do, but report no progress,
/// which the text's run already brought to 100 %.
struct Quiet<'a>(&'a Control<'a>);

impl ChunkControl for Quiet<'_> {
    fn proceed(&self) -> bool {
        self.0.proceed()
    }

    fn progress(&self, _done: usize, _total: usize) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history_versions::{test_entry, test_version, HistoryVersion, VersionAuthor};
    use ReprocessMode::{PostProcess, Transcribe, TranscribeAndPostProcess};

    fn work(mode: ReprocessMode, version: Option<u32>) -> Option<Work> {
        Some(Work {
            mode,
            version,
            prompt_ids: None,
            use_instruction: true,
        })
    }

    // DanyWhere: History's wand menu asks for exactly some prompts
    // (`prompt_library`).
    #[test]
    fn a_job_keeps_the_prompts_it_was_asked_for() {
        let mut jobs = Jobs::default();
        let chosen = Work {
            mode: PostProcess,
            version: Some(1),
            prompt_ids: Some(vec!["b".to_string(), "a".to_string()]),
            use_instruction: true,
        };
        assert!(jobs.push(1, Some(chosen.clone())));
        assert!(jobs.push(2, work(PostProcess, None)));
        assert_eq!(jobs.next(), Some((1, chosen)));
        assert_eq!(jobs.next(), Some((2, work(PostProcess, None).unwrap())));
    }

    #[test]
    fn a_post_process_job_keeps_its_mode_and_version() {
        let mut jobs = Jobs::default();
        assert!(jobs.push(1, work(PostProcess, Some(0))));
        assert_eq!(jobs.next(), Some((1, work(PostProcess, Some(0)).unwrap())));
    }

    #[test]
    fn a_job_that_transcribes_and_post_processes_keeps_its_mode() {
        let mut jobs = Jobs::default();
        assert!(jobs.push(1, work(TranscribeAndPostProcess, None)));
        assert_eq!(
            jobs.next(),
            Some((1, work(TranscribeAndPostProcess, None).unwrap()))
        );
    }

    #[test]
    fn entries_go_before_files() {
        let mut jobs = Jobs::default();
        jobs.push(1, None);
        jobs.push(2, work(Transcribe, None));
        jobs.push(3, work(PostProcess, None));
        assert_eq!(jobs.next(), Some((2, Work::TRANSCRIBE)));
        assert_eq!(jobs.next(), Some((3, work(PostProcess, None).unwrap())));
        assert_eq!(jobs.next(), Some((1, Work::TRANSCRIBE)));
        assert_eq!(jobs.next(), None);
    }

    #[test]
    fn an_entry_has_one_job_at_a_time() {
        let mut jobs = Jobs::default();
        assert!(jobs.push(1, work(PostProcess, None)));
        assert!(!jobs.push(1, work(Transcribe, None)));
        assert_eq!(jobs.next(), Some((1, work(PostProcess, None).unwrap())));
        assert_eq!(jobs.next(), None);
    }

    #[test]
    fn unqueue_drops_a_post_process_job() {
        let mut jobs = Jobs::default();
        jobs.push(1, work(PostProcess, None));
        assert!(jobs.unqueue(1));
        assert!(jobs.is_idle());
        assert_eq!(jobs.next(), None);
    }

    #[test]
    fn only_post_processing_skips_the_transcription() {
        assert!(Transcribe.transcribes());
        assert!(TranscribeAndPostProcess.transcribes());
        assert!(!PostProcess.transcribes());
    }

    fn entry_with_hint(versions: Vec<HistoryVersion>) -> HistoryEntry {
        let mut entry = test_entry(versions);
        entry.window_context.word_replacements = vec![crate::dictation_context::WordReplacement {
            original: "Bild".into(),
            replacement: "Build".into(),
        }];
        entry
    }

    #[test]
    fn post_processing_starts_from_the_version_chosen_or_the_top() {
        let processed = entry_with_hint(vec![
            HistoryVersion::transcription("raw"),
            test_version("Clean.", VersionAuthor::Ai),
        ]);
        let (text, hint) = post_process_input(&processed, None).unwrap();
        assert_eq!(text, "Clean.");
        assert!(hint.is_empty());
        let (text, hint) = post_process_input(&processed, Some(0)).unwrap();
        assert_eq!(text, "raw");
        assert_eq!(hint.len(), 1);
        assert_eq!(
            post_process_input(&processed, Some(2)).err(),
            Some(VersionError::NoSuchVersion)
        );
    }

    #[test]
    fn the_word_list_hint_goes_along_only_with_the_transcript_wherever_it_is() {
        let moved = entry_with_hint(vec![
            test_version("Clean.", VersionAuthor::Ai),
            HistoryVersion::transcription("raw"),
        ]);
        let (text, hint) = post_process_input(&moved, None).unwrap();
        assert_eq!(text, "raw");
        assert_eq!(hint.len(), 1);
        let (_, hint) = post_process_input(&moved, Some(0)).unwrap();
        assert!(hint.is_empty());
        // Transcribed again: the hint is the newer transcription's.
        let again = entry_with_hint(vec![
            HistoryVersion::transcribed("old", Some(1)),
            HistoryVersion::transcribed("new", Some(5)),
        ]);
        assert!(post_process_input(&again, Some(0)).unwrap().1.is_empty());
        assert_eq!(post_process_input(&again, Some(1)).unwrap().1.len(), 1);
        // The user changed it: not what the word list replaced in.
        let mut edited = HistoryVersion::transcription("raw, mine");
        edited.steps.push(crate::history_versions::VersionStep {
            by: VersionAuthor::User,
            at: Some(2),
            prompt_name: None,
            model: None,
            spoken_instruction: false,
        });
        let edited = entry_with_hint(vec![edited]);
        assert!(post_process_input(&edited, None).unwrap().1.is_empty());
    }

    #[test]
    fn transcribing_again_post_processes_as_dictated() {
        assert!(!Transcribe.post_processes(false));
        assert!(Transcribe.post_processes(true));
    }

    #[test]
    fn the_post_processing_modes_always_post_process() {
        for requested in [false, true] {
            assert!(TranscribeAndPostProcess.post_processes(requested));
            assert!(PostProcess.post_processes(requested));
        }
    }

    #[test]
    fn a_failed_post_processing_without_transcription_has_its_own_notice() {
        let failure = PostProcessFailure::unknown();
        assert_eq!(
            post_process_failure_notice(&failure, true).code,
            NoticeCode::FilePostProcessFailed
        );
        assert_eq!(
            post_process_failure_notice(&failure, false).code,
            NoticeCode::HistoryPostProcessFailed
        );
    }

    /// DanyWhere: both notices name the reason, as a dictation's does.
    #[test]
    fn a_failed_post_processing_of_an_entry_names_its_reason() {
        let mut settings = crate::settings::get_default_settings();
        settings
            .post_process_models
            .insert("openai".to_string(), "gpt-9".to_string());
        let failure = PostProcessFailure::new(
            &settings,
            crate::post_process_failure::FailureReason::ModelNotFound,
        );
        let detail = serde_json::json!({
            "reason": "model-not-found",
            "providerId": "openai",
            "provider": "OpenAI",
            "model": "gpt-9",
        })
        .to_string();
        for transcribed in [true, false] {
            let notice = post_process_failure_notice(&failure, transcribed);
            assert_eq!(notice.source, NoticeSource::File);
            assert_eq!(notice.detail.as_deref(), Some(detail.as_str()));
        }
    }

    #[test]
    fn reprocess_modes_reach_the_frontend_in_snake_case() {
        assert_eq!(
            serde_json::to_string(&TranscribeAndPostProcess).unwrap(),
            "\"transcribe_and_post_process\""
        );
        assert_eq!(
            serde_json::to_string(&PostProcess).unwrap(),
            "\"post_process\""
        );
        assert_eq!(
            serde_json::to_string(&Transcribe).unwrap(),
            "\"transcribe\""
        );
    }

    // ---- Hearing an entry again, its spoken instruction too (F6 ii) ------

    fn tone(n: usize) -> Vec<f32> {
        (0..n).map(|i| 0.4 * (i as f32 * 0.05).sin()).collect()
    }

    /// What a fake speech model hears in `len` samples of `part`.
    fn heard(len: usize, part: Part) -> Transcription {
        Transcription {
            text: format!("{part:?} of {len}"),
            language: None,
            word_replacements: Vec::new(),
            model_id: Some("fake-model".into()),
            takes_vocabulary: false,
        }
    }

    /// An entry whose recording `handy-1.opus` has 16 000 samples, with an
    /// instruction whose audio is `instruction` (its file and samples; no
    /// samples: the file isn't there).
    fn entry_with_audio(dir: &Path, instruction: Option<(&str, usize)>) -> HistoryEntry {
        crate::audio_toolkit::save_opus_file(dir.join("handy-1.opus"), &tone(16_000)).unwrap();
        let mut entry = test_entry(vec![HistoryVersion::transcription("old words")]);
        if let Some((file, samples)) = instruction {
            if samples > 0 {
                crate::audio_toolkit::save_opus_file(dir.join(file), &tone(samples)).unwrap();
            }
            entry.spoken_instruction = Some("old instruction".into());
            entry.spoken_instruction_file = Some(file.into());
        }
        entry
    }

    /// Hears `entry` again with the fake model; the parts it was asked to
    /// hear, with their lengths.
    fn hear(
        entry: &HistoryEntry,
        dir: &Path,
        cancel_at: Option<Part>,
    ) -> (Option<HeardAgain>, Vec<(Part, usize)>) {
        let mut calls = Vec::new();
        let heard_again = hear_again(entry, dir, &mut |samples, part| {
            calls.push((part, samples.len()));
            Ok((cancel_at != Some(part)).then(|| heard(samples.len(), part)))
        })
        .expect("hear again");
        (heard_again, calls)
    }

    #[test]
    fn transcribing_again_hears_the_text_and_the_spoken_instruction() {
        let dir = tempfile::tempdir().unwrap();
        let entry = entry_with_audio(dir.path(), Some(("handy-1-instruction.opus", 4_000)));

        let (again, calls) = hear(&entry, dir.path(), None);

        assert_eq!(
            calls,
            vec![(Part::Text, 16_000), (Part::Instruction, 4_000)]
        );
        let again = again.expect("not cancelled");
        assert_eq!(again.text.text, "Text of 16000");
        assert_eq!(again.instruction.as_deref(), Some("Instruction of 4000"));
    }

    #[test]
    fn without_a_spoken_instruction_only_the_text_is_heard() {
        let dir = tempfile::tempdir().unwrap();
        let entry = entry_with_audio(dir.path(), None);

        let (again, calls) = hear(&entry, dir.path(), None);

        assert_eq!(calls, vec![(Part::Text, 16_000)]);
        assert_eq!(again.expect("not cancelled").instruction, None);
    }

    #[test]
    fn an_instruction_whose_audio_is_gone_keeps_its_text() {
        let dir = tempfile::tempdir().unwrap();
        let entry = entry_with_audio(dir.path(), Some(("handy-1-instruction.opus", 0)));

        let (again, calls) = hear(&entry, dir.path(), None);

        assert_eq!(calls, vec![(Part::Text, 16_000)]);
        assert_eq!(again.expect("not cancelled").instruction, None);
    }

    #[test]
    fn a_cancel_while_the_instruction_is_heard_cancels_the_job() {
        let dir = tempfile::tempdir().unwrap();
        let entry = entry_with_audio(dir.path(), Some(("handy-1-instruction.opus", 4_000)));

        let (again, calls) = hear(&entry, dir.path(), Some(Part::Instruction));

        assert_eq!(
            calls,
            vec![(Part::Text, 16_000), (Part::Instruction, 4_000)]
        );
        assert!(again.is_none());
    }

    // ---- Post-processing with the spoken instruction (strand Z1) ---------

    fn with_instruction(text: Option<&str>) -> HistoryEntry {
        let mut entry = test_entry(vec![HistoryVersion::transcription("words")]);
        entry.spoken_instruction = text.map(str::to_string);
        entry
    }

    #[test]
    fn transcribing_again_post_processes_with_the_instruction_heard_again() {
        let entry = with_instruction(Some("translate it into Polsh"));
        assert_eq!(
            instruction_for(&entry, Some("translate it into Polish".into()), true).as_deref(),
            Some("translate it into Polish")
        );
    }

    #[test]
    fn an_instruction_whose_audio_is_gone_post_processes_with_its_text() {
        let entry = with_instruction(Some("make it formal"));
        assert_eq!(
            instruction_for(&entry, None, true).as_deref(),
            Some("make it formal")
        );
    }

    #[test]
    fn the_wand_without_the_instruction_leaves_it_out() {
        let entry = with_instruction(Some("make it formal"));
        assert_eq!(instruction_for(&entry, None, false), None);
        assert_eq!(instruction_for(&entry, Some("again".into()), false), None);
        assert_eq!(instruction_for(&with_instruction(None), None, true), None);
    }

    #[test]
    fn a_job_keeps_whether_it_uses_the_instruction() {
        let mut jobs = Jobs::default();
        let without = Work {
            use_instruction: false,
            ..work(PostProcess, Some(1)).unwrap()
        };
        jobs.push(7, Some(without.clone()));
        assert_eq!(jobs.next(), Some((7, without)));
        // A file has none anyway; an entry transcribed again uses its own.
        jobs.push(8, None);
        assert!(jobs.next().is_some_and(|(_, work)| work.use_instruction));
    }

    #[test]
    fn with_an_instruction_transcribing_again_always_post_processes() {
        let mode = PostProcessMode::WithInstructionKey;
        assert!(job_post_processes(
            mode,
            Transcribe,
            false,
            Some("make it formal")
        ));
        assert!(
            job_post_processes(mode, Transcribe, false, Some("")),
            "heard as nothing"
        );
        assert!(!job_post_processes(mode, Transcribe, false, None));
        assert!(job_post_processes(mode, Transcribe, true, None));
    }

    /// DanyWhere (review of 2026-11-26, finding 1): with "never" in the
    /// profile in use no job of History sends anything to a provider, as
    /// History's page offers neither the wand nor post-processing with
    /// "Re-transcribe" then (`postProcessEntry.ts`). The entry keeps its
    /// mark, so a job in a profile that post-processes does it again.
    #[test]
    fn with_never_in_the_profile_in_use_no_job_post_processes() {
        for mode in [Transcribe, TranscribeAndPostProcess, PostProcess] {
            for requested in [false, true] {
                for instruction in [None, Some(""), Some("make it formal")] {
                    let case = format!("{mode:?}, requested {requested}, {instruction:?}");
                    assert!(
                        !job_post_processes(PostProcessMode::Never, mode, requested, instruction),
                        "{case}"
                    );
                    for allowed in [PostProcessMode::WithInstructionKey, PostProcessMode::Always] {
                        assert_eq!(
                            job_post_processes(allowed, mode, requested, instruction),
                            job_wants_post_processing(mode, requested, instruction),
                            "{case}, {allowed:?}"
                        );
                    }
                }
            }
        }
        assert!(
            job_wants_post_processing(Transcribe, true, None),
            "the mark"
        );
    }

    #[test]
    fn an_instruction_alone_is_something_to_work_on() {
        assert!(!nothing_to_work_on("", Some("write a polite no")));
        assert!(nothing_to_work_on("", Some("  ")));
        assert!(nothing_to_work_on("", None));
        assert!(!nothing_to_work_on("words", None));
    }

    #[test]
    fn an_entry_made_of_an_instruction_alone_is_heard_again() {
        // Its recording is empty (F5 i): only the instruction is heard.
        let dir = tempfile::tempdir().unwrap();
        let mut entry = entry_with_audio(dir.path(), Some(("handy-1-instruction.opus", 4_000)));
        crate::audio_toolkit::save_opus_file(dir.path().join("handy-1.opus"), &[]).unwrap();
        entry.versions = vec![HistoryVersion::transcription("")];

        let (again, calls) = hear(&entry, dir.path(), None);

        assert_eq!(calls, vec![(Part::Instruction, 4_000)]);
        let again = again.expect("not cancelled");
        assert_eq!(again.text.text, "");
        assert_eq!(again.instruction.as_deref(), Some("Instruction of 4000"));
    }

    #[test]
    fn an_empty_recording_without_an_instruction_still_fails() {
        let dir = tempfile::tempdir().unwrap();
        let entry = entry_with_audio(dir.path(), None);
        crate::audio_toolkit::save_opus_file(dir.path().join("handy-1.opus"), &[]).unwrap();
        assert!(hear_again(&entry, dir.path(), &mut |_, _| unreachable!()).is_err());
    }

    #[test]
    fn a_post_processing_that_added_nothing_ends_unchanged() {
        assert_eq!(finished_status(true), TranscriptionJobStatus::Unchanged);
        assert_eq!(finished_status(false), TranscriptionJobStatus::Finished);
        // The frontend's notice listens for this state.
        assert_eq!(
            serde_json::to_string(&TranscriptionJobStatus::Unchanged).unwrap(),
            r#"{"state":"unchanged"}"#
        );
    }
}
