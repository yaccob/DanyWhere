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

use crate::actions::process_transcription_output;
use crate::audio_toolkit::{decode_audio_file, recording_file_name, save_recording};
use crate::history_versions::{NewTranscription, PostProcessTarget, PostProcessed, VersionError};
use crate::managers::history::{HistoryEntry, HistoryManager};
use crate::managers::transcription::{
    panic_payload_message, ChunkControl, Transcription, TranscriptionManager,
};
use crate::notices::{self, Notice, NoticeCode, NoticeSource};

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
fn post_process_failure_notice(transcribed: bool) -> NoticeCode {
    if transcribed {
        NoticeCode::FilePostProcessFailed
    } else {
        NoticeCode::HistoryPostProcessFailed
    }
}

struct Running {
    entry_id: i64,
    cancelled: Arc<AtomicBool>,
}

/// What a job does to its entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Work {
    mode: ReprocessMode,
    /// The version a post-processing starts from; `None`: the top.
    version: Option<u32>,
}

impl Work {
    /// A file's first transcription, or an entry's again.
    const TRANSCRIBE: Work = Work {
        mode: ReprocessMode::Transcribe,
        version: None,
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
        self.queue(entry_id, Some(Work { mode, version }));
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
                    post_process_failed,
                    transcribed,
                }) => {
                    // Clear first, so the warning about this job survives it.
                    notices::clear(&self.app, NoticeSource::File);
                    if post_process_failed {
                        notices::raise(
                            &self.app,
                            Notice::new(
                                post_process_failure_notice(transcribed),
                                NoticeSource::File,
                            ),
                        );
                    }
                    TranscriptionJobStatus::Finished
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
        if !work.mode.transcribes() {
            return self.post_process_again(&hm, &entry, work.version, cancelled);
        }
        let post_process = work.mode.post_processes(entry.post_process_requested);

        let tm = Arc::clone(&self.app.state::<Arc<TranscriptionManager>>());
        let samples = HistoryManager::recording_samples_in(&entry, hm.recordings_dir())
            .map_err(|e| anyhow!("Failed to load audio: {}", e))?;
        if samples.is_empty() {
            return Err(anyhow!("Recording has no audio samples"));
        }

        let control = Control {
            manager: self,
            entry_id: entry.id,
            cancelled,
            percent: Cell::new(0),
        };
        let Some(Transcription {
            text,
            language,
            word_replacements,
        }) = tm.transcribe_in_chunks(samples, &control)?
        else {
            return Ok(JobEnd::Cancelled);
        };
        if text.is_empty() {
            return Err(anyhow!("Recording contains no speech"));
        }
        if cancelled.load(Ordering::Acquire) {
            return Ok(JobEnd::Cancelled);
        }

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
        let processed = tauri::async_runtime::block_on(process_transcription_output(
            &self.app,
            &text,
            post_process,
            &context_query,
        ));
        // Post-processing can take a while; a cancel during it still wins.
        if cancelled.load(Ordering::Acquire) {
            return Ok(JobEnd::Cancelled);
        }
        let post_process_failed = processed.post_process_failed;
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
                }),
                post_process_requested: post_process,
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
            post_process_failed,
            transcribed: true,
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
        let processed = tauri::async_runtime::block_on(process_transcription_output(
            &self.app,
            transcript,
            true,
            &context_query,
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
        let post_process_failed = match processed.post_processed_text {
            Some(text) => {
                hm.store_post_processing(
                    entry.id,
                    PostProcessTarget::OnTop,
                    PostProcessed {
                        text,
                        prompt: processed.post_process_prompt,
                        prompt_name: processed.post_process_prompt_name,
                    },
                )?;
                false
            }
            None => true,
        };
        Ok(JobEnd::Finished {
            post_process_failed,
            transcribed: false,
        })
    }
}

/// A cancelled job changes no notices.
enum JobEnd {
    /// `transcribed` is false when the job only post-processed.
    Finished {
        post_process_failed: bool,
        transcribed: bool,
    },
    Cancelled,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history_versions::{test_entry, test_version, HistoryVersion, VersionAuthor};
    use ReprocessMode::{PostProcess, Transcribe, TranscribeAndPostProcess};

    fn work(mode: ReprocessMode, version: Option<u32>) -> Option<Work> {
        Some(Work { mode, version })
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
        assert_eq!(
            post_process_failure_notice(true),
            NoticeCode::FilePostProcessFailed
        );
        assert_eq!(
            post_process_failure_notice(false),
            NoticeCode::HistoryPostProcessFailed
        );
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
}
