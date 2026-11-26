use std::sync::{mpsc, Arc, Mutex, MutexGuard, PoisonError};

use handy_recorder::{AudioChunk, Sink};

use crate::audio_toolkit::{
    audio::AudioVisualiser,
    constants,
    vad::{VadFrame, VoiceActivityDetector},
};

/// How 16 kHz mono frames should be filtered for one recording session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VadPolicy {
    /// Bypass VAD and forward every frame.
    Disabled,
    /// Current offline-tuned VAD profile.
    Offline,
    /// VAD profile with a longer post-speech tail for streaming-capable models.
    Streaming,
}

/// A single VAD engine plus the two hangover-tail lengths its smoothing wrapper
/// should use. The offline and streaming policies are never active
/// concurrently, so one detector is reconfigured per recording rather than
/// kept as two resident engines.
pub struct CaptureVad {
    detector: Box<dyn VoiceActivityDetector>,
    offline_hangover_frames: usize,
    streaming_hangover_frames: usize,
}

impl CaptureVad {
    pub fn new(
        detector: Box<dyn VoiceActivityDetector>,
        offline_hangover_frames: usize,
        streaming_hangover_frames: usize,
    ) -> Self {
        assert!(
            detector.frame_samples() > 0,
            "VAD frame size must be non-zero"
        );
        Self {
            detector,
            offline_hangover_frames,
            streaming_hangover_frames,
        }
    }

    /// Samples per detector frame; the recorder delivers chunks of this size.
    pub fn frame_samples(&self) -> usize {
        self.detector.frame_samples()
    }

    /// Post-speech hangover tail (in detector frames) for the given policy.
    /// `Disabled` never reaches the detector, so it maps to the offline value.
    fn hangover_for(&self, policy: VadPolicy) -> usize {
        match policy {
            VadPolicy::Streaming => self.streaming_hangover_frames,
            VadPolicy::Offline | VadPolicy::Disabled => self.offline_hangover_frames,
        }
    }
}

/// Receives spectrum levels for the recording overlay.
pub type LevelCallback = Arc<dyn Fn(Vec<f32>) + Send + Sync + 'static>;
/// Receives each 16 kHz mono frame that passes the recording's VAD policy, in
/// order. Used to feed live streaming transcription as audio arrives.
pub type AudioFrameCallback = Arc<dyn Fn(&[f32]) + Send + Sync + 'static>;

/// What a finished recording hands back.
pub struct CapturedAudio {
    /// 16 kHz mono samples that passed the VAD policy.
    pub samples: Vec<f32>,
    /// Every sample received (pre-VAD) was exactly zero, e.g. a muted mic.
    /// False when no audio arrived at all.
    pub input_silent: bool,
    /// The detector the recording borrowed, for the next one.
    pub vad: CaptureVad,
}

/// DanyWhere: the first section of a recording cut in two while the
/// microphone runs ([`RecordingSections::split`]).
#[derive(Debug, Clone, PartialEq)]
pub struct SplitSection {
    /// 16 kHz mono samples that passed the VAD policy before the cut.
    pub samples: Vec<f32>,
    /// Every sample of the section (pre-VAD) was exactly zero; false when no
    /// audio arrived in it.
    pub input_silent: bool,
}

/// What a sink kept of the section it records into now.
#[derive(Default)]
struct Section {
    /// 16 kHz mono samples that passed the VAD policy.
    samples: Vec<f32>,
    /// A chunk arrived in this section.
    got_audio: bool,
    /// One of its chunks was not exact digital silence.
    heard_sound: bool,
}

impl Section {
    /// Every sample received was exactly zero, e.g. a muted mic. False when
    /// no audio arrived at all.
    fn input_silent(&self) -> bool {
        self.got_audio && !self.heard_sound
    }
}

#[derive(Default)]
struct Sections {
    current: Section,
    /// The first section was handed over: frames go into the second, and
    /// no longer to the live feed.
    split: bool,
    /// The sink finished; nothing is cut any more.
    finished: bool,
}

/// DanyWhere: the audio a recording's sink has kept, shared with whoever may
/// cut the recording in two while the microphone runs (the spoken
/// instruction). The sink adds each frame it keeps under the same lock that
/// a cut takes, so a frame lands in exactly one section, and its live feed
/// ends with the first. Clones share it.
#[derive(Clone, Default)]
pub struct RecordingSections(Arc<Mutex<Sections>>);

impl RecordingSections {
    fn lock(&self) -> MutexGuard<'_, Sections> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Ends the first section at the next frame boundary: hands over what
    /// the sink kept so far; from here on its frames go into the second
    /// section, which `finish` hands back, and no longer to the live feed.
    /// `None` after a cut or once the recording finished.
    pub fn split(&self) -> Option<SplitSection> {
        let mut sections = self.lock();
        if sections.split || sections.finished {
            return None;
        }
        sections.split = true;
        let first = std::mem::take(&mut sections.current);
        Some(SplitSection {
            input_silent: first.input_silent(),
            samples: first.samples,
        })
    }

    /// The section recorded into now, for the sink's `finish`; nothing is
    /// cut afterwards.
    fn finish(&self) -> Section {
        let mut sections = self.lock();
        sections.finished = true;
        std::mem::take(&mut sections.current)
    }
}

/// One recording's consumer of 16 kHz mono audio from handy-recorder. Runs on
/// the recorder's delivery thread, never the real-time audio thread, so it
/// can run VAD inline. Chunks are exactly one VAD frame long.
pub struct CaptureSink {
    vad: CaptureVad,
    policy: VadPolicy,
    visualizer: AudioVisualiser,
    on_levels: LevelCallback,
    on_audio: AudioFrameCallback,
    /// Signalled on the first chunk: the device is delivering audio. Silence
    /// counts; readiness is not speech.
    ready: Option<mpsc::Sender<()>>,
    /// DanyWhere: what it kept, in the section it records into now.
    sections: RecordingSections,
}

/// Spectrum window at 16 kHz: 32 ms, near the overlay's ~30 Hz refresh.
const VISUALIZER_WINDOW: usize = 512;
const VISUALIZER_BUCKETS: usize = 16;

impl CaptureSink {
    /// A sink for one recording. Resets the detector and applies the policy's
    /// hangover tail, so no state carries over from the previous recording.
    pub fn new(
        mut vad: CaptureVad,
        policy: VadPolicy,
        on_levels: LevelCallback,
        on_audio: AudioFrameCallback,
        ready: mpsc::Sender<()>,
    ) -> Self {
        if policy != VadPolicy::Disabled {
            let hangover = vad.hangover_for(policy);
            vad.detector.set_hangover_frames(hangover);
            vad.detector.reset();
        }
        Self {
            vad,
            policy,
            visualizer: AudioVisualiser::new(
                constants::WHISPER_SAMPLE_RATE,
                VISUALIZER_WINDOW,
                VISUALIZER_BUCKETS,
                400.0,
                4000.0,
            ),
            on_levels,
            on_audio,
            ready: Some(ready),
            sections: RecordingSections::default(),
        }
    }

    /// DanyWhere: the handle that cuts this recording in two.
    pub fn sections(&self) -> RecordingSections {
        self.sections.clone()
    }

    /// The detector, from a sink that never recorded (its start failed).
    pub fn into_vad(self) -> CaptureVad {
        self.vad
    }

    /// Hands back the recording and the detector. Logs what VAD still held
    /// back when the recording stopped.
    pub fn finish(self) -> CapturedAudio {
        // Diagnostic for audio still withheld when capture stopped; it is not
        // conclusive in either direction.
        if self.policy != VadPolicy::Disabled {
            if let Some(report) = self.vad.detector.tail_report() {
                log::debug!(
                    "VAD at stop: withheld tail {} frames (~{}ms, {} voiced), in_speech={}, onset_counter={}, hangover_counter={}",
                    report.withheld_frames,
                    report.withheld_frames * self.vad.frame_samples() * 1000
                        / constants::WHISPER_SAMPLE_RATE as usize,
                    report.withheld_voiced_frames,
                    report.in_speech,
                    report.onset_counter,
                    report.hangover_counter
                );
            }
        }
        // DanyWhere: the section recorded into last, the second after a cut.
        let section = self.sections.finish();
        CapturedAudio {
            input_silent: section.input_silent(),
            samples: section.samples,
            vad: self.vad,
        }
    }

    /// Keeps `frame` in `section` and feeds it to the live stream, unless
    /// the first section was cut off (`live` is then `None`).
    fn emit(section: &mut Section, live: Option<&AudioFrameCallback>, frame: &[f32]) {
        section.samples.extend_from_slice(frame);
        if let Some(on_audio) = live {
            on_audio(frame);
        }
    }
}

impl Sink for CaptureSink {
    fn process_chunk(&mut self, chunk: AudioChunk<'_>) {
        if let Some(ready) = self.ready.take() {
            let _ = ready.send(());
        }

        // Mono by construction: the recorder mixes down or picks one channel.
        let real = &chunk.samples[..chunk.valid_frames];
        if let Some(levels) = self.visualizer.feed(real) {
            (self.on_levels)(levels);
        }

        // DanyWhere: under the lock a cut takes, so this chunk belongs to
        // one section, whole (`RecordingSections::split`).
        let mut sections = self.sections.lock();
        let Sections { current, split, .. } = &mut *sections;
        current.got_audio = true;
        if !current.heard_sound && !chunk.is_digital_silence() {
            current.heard_sound = true;
        }
        let live = (!*split).then_some(&self.on_audio);

        if self.policy == VadPolicy::Disabled {
            Self::emit(current, live, real);
            return;
        }

        // The detector needs whole frames, so the final chunk goes in with its
        // zero padding; at most one frame of silence reaches the recording.
        // DanyWhere: its state goes on across a cut, so the two sections
        // hold what one recording would.
        match self
            .vad
            .detector
            .push_frame(chunk.samples)
            .unwrap_or(VadFrame::Speech(chunk.samples))
        {
            VadFrame::Speech(frame) => Self::emit(current, live, frame),
            VadFrame::Noise => {}
        }
    }
}

#[cfg(test)]
mod tests;
