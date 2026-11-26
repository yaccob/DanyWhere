use super::{AudioFrameCallback, CaptureSink, CaptureVad, LevelCallback, SplitSection, VadPolicy};
use crate::audio_toolkit::vad::{VadFrame, VoiceActivityDetector};
use handy_recorder::{AudioChunk, Sink};
use std::sync::{mpsc, Arc, Mutex};

/// Keeps frames whose first sample is non-zero, and records how the sink
/// configured it.
#[derive(Default)]
struct ScriptedVad {
    frame_samples: usize,
    hangover: Arc<Mutex<Option<usize>>>,
    resets: Arc<Mutex<usize>>,
}

impl VoiceActivityDetector for ScriptedVad {
    fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> anyhow::Result<VadFrame<'a>> {
        Ok(if frame[0] != 0.0 {
            VadFrame::Speech(frame)
        } else {
            VadFrame::Noise
        })
    }

    fn frame_samples(&self) -> usize {
        self.frame_samples
    }

    fn set_hangover_frames(&mut self, frames: usize) {
        *self.hangover.lock().unwrap() = Some(frames);
    }

    fn reset(&mut self) {
        *self.resets.lock().unwrap() += 1;
    }
}

const FRAME: usize = 4;

struct Harness {
    sink: CaptureSink,
    ready: mpsc::Receiver<()>,
    fed: Arc<Mutex<Vec<f32>>>,
    hangover: Arc<Mutex<Option<usize>>>,
    resets: Arc<Mutex<usize>>,
}

fn harness(policy: VadPolicy) -> Harness {
    let vad = ScriptedVad {
        frame_samples: FRAME,
        ..Default::default()
    };
    let hangover = Arc::clone(&vad.hangover);
    let resets = Arc::clone(&vad.resets);
    let fed = Arc::new(Mutex::new(Vec::new()));
    let on_audio: AudioFrameCallback = {
        let fed = Arc::clone(&fed);
        Arc::new(move |frame: &[f32]| fed.lock().unwrap().extend_from_slice(frame))
    };
    let on_levels: LevelCallback = Arc::new(|_| {});
    let (ready_tx, ready) = mpsc::channel();
    let sink = CaptureSink::new(
        CaptureVad::new(Box::new(vad), 10, 20),
        policy,
        on_levels,
        on_audio,
        ready_tx,
    );
    Harness {
        sink,
        ready,
        fed,
        hangover,
        resets,
    }
}

fn chunk(samples: &[f32], valid_frames: usize) -> AudioChunk<'_> {
    AudioChunk {
        samples,
        sample_rate: 16_000,
        channels: 1,
        valid_frames,
    }
}

#[test]
fn readiness_fires_once_on_the_first_chunk() {
    let mut h = harness(VadPolicy::Disabled);
    assert!(h.ready.try_recv().is_err());

    h.sink.process_chunk(chunk(&[0.0; FRAME], FRAME));
    h.sink.process_chunk(chunk(&[0.0; FRAME], FRAME));

    assert!(h.ready.try_recv().is_ok());
    // The sender is gone after the first chunk, so a second signal can't come.
    assert_eq!(h.ready.try_recv(), Err(mpsc::TryRecvError::Disconnected));
}

#[test]
fn readiness_waiter_wakes_when_a_recording_ends_without_audio() {
    let h = harness(VadPolicy::Offline);
    let _ = h.sink.finish();
    assert!(h.ready.recv().is_err());
}

#[test]
fn disabled_vad_keeps_every_real_sample_and_drops_padding() {
    let mut h = harness(VadPolicy::Disabled);
    h.sink.process_chunk(chunk(&[1.0, 0.0, 2.0, 0.0], FRAME));
    // Final chunk: one real frame, three of zero padding.
    h.sink.process_chunk(chunk(&[3.0, 0.0, 0.0, 0.0], 1));

    let expected = vec![1.0, 0.0, 2.0, 0.0, 3.0];
    assert_eq!(*h.fed.lock().unwrap(), expected);
    assert_eq!(h.sink.finish().samples, expected);
    assert_eq!(
        *h.resets.lock().unwrap(),
        0,
        "a disabled policy leaves VAD alone"
    );
}

#[test]
fn a_recording_of_only_zeros_is_silent_input() {
    let mut h = harness(VadPolicy::Offline);
    h.sink.process_chunk(chunk(&[0.0; FRAME], FRAME));
    h.sink.process_chunk(chunk(&[0.0; FRAME], FRAME));
    assert!(h.sink.finish().input_silent);
}

#[test]
fn a_recording_stopped_before_any_audio_is_not_silent_input() {
    // A quick tap that ends before the device delivers its first chunk says
    // nothing about the microphone.
    let h = harness(VadPolicy::Offline);
    assert!(!h.sink.finish().input_silent);
}

#[test]
fn sound_counts_even_when_vad_drops_it() {
    let mut h = harness(VadPolicy::Offline);
    // Noise to the detector (its first sample is zero), but not silence.
    h.sink.process_chunk(chunk(&[0.0, 0.01, 0.0, 0.0], FRAME));
    let captured = h.sink.finish();
    assert!(captured.samples.is_empty());
    assert!(!captured.input_silent);
}

#[test]
fn vad_keeps_speech_frames_only() {
    let mut h = harness(VadPolicy::Offline);
    h.sink.process_chunk(chunk(&[1.0; FRAME], FRAME));
    h.sink.process_chunk(chunk(&[0.0; FRAME], FRAME));
    h.sink.process_chunk(chunk(&[2.0; FRAME], FRAME));

    let expected = [[1.0; FRAME], [2.0; FRAME]].concat();
    assert_eq!(*h.fed.lock().unwrap(), expected);
    assert_eq!(h.sink.finish().samples, expected);
}

#[test]
fn each_recording_resets_the_detector_with_its_policys_hangover() {
    let offline = harness(VadPolicy::Offline);
    assert_eq!(*offline.hangover.lock().unwrap(), Some(10));
    assert_eq!(*offline.resets.lock().unwrap(), 1);

    let streaming = harness(VadPolicy::Streaming);
    assert_eq!(*streaming.hangover.lock().unwrap(), Some(20));
    assert_eq!(*streaming.resets.lock().unwrap(), 1);
}

#[test]
fn finish_returns_the_detector_for_the_next_recording() {
    let h = harness(VadPolicy::Offline);
    let captured = h.sink.finish();
    assert_eq!(captured.vad.frame_samples(), FRAME);
    assert_eq!(captured.vad.hangover_for(VadPolicy::Streaming), 20);
}

// DanyWhere: cutting a recording in two while the microphone runs
// (`RecordingSections`), for the spoken instruction.

#[test]
fn a_split_hands_over_the_first_section_and_the_finish_the_second() {
    let mut h = harness(VadPolicy::Disabled);
    let sections = h.sink.sections();
    h.sink.process_chunk(chunk(&[1.0, 2.0, 3.0, 4.0], FRAME));
    h.sink.process_chunk(chunk(&[5.0, 6.0, 7.0, 8.0], FRAME));

    let first = sections.split().expect("the recording is cut");
    assert_eq!(first.samples, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
    assert!(!first.input_silent);

    h.sink.process_chunk(chunk(&[9.0, 10.0, 11.0, 12.0], FRAME));
    h.sink.process_chunk(chunk(&[13.0, 0.0, 0.0, 0.0], 1));
    assert_eq!(h.sink.finish().samples, [9.0, 10.0, 11.0, 12.0, 13.0]);
}

#[test]
fn after_a_split_the_live_feed_gets_nothing_more() {
    let mut h = harness(VadPolicy::Offline);
    let sections = h.sink.sections();
    h.sink.process_chunk(chunk(&[1.0; FRAME], FRAME));
    let first = sections.split().expect("the recording is cut");
    h.sink.process_chunk(chunk(&[2.0; FRAME], FRAME));

    assert_eq!(first.samples, [1.0; FRAME]);
    assert_eq!(
        *h.fed.lock().unwrap(),
        first.samples,
        "the live stream had exactly the first section"
    );
    assert_eq!(h.sink.finish().samples, [2.0; FRAME]);
}

#[test]
fn each_section_says_whether_its_own_input_was_silent() {
    let mut h = harness(VadPolicy::Offline);
    let sections = h.sink.sections();
    h.sink.process_chunk(chunk(&[0.0; FRAME], FRAME));
    let first = sections.split().expect("the recording is cut");
    assert!(first.input_silent, "the first section had only zeros");
    h.sink.process_chunk(chunk(&[0.0, 0.01, 0.0, 0.0], FRAME));
    assert!(
        !h.sink.finish().input_silent,
        "the second section had sound"
    );

    let mut h = harness(VadPolicy::Offline);
    let sections = h.sink.sections();
    h.sink.process_chunk(chunk(&[1.0; FRAME], FRAME));
    assert!(!sections.split().expect("the recording is cut").input_silent);
    h.sink.process_chunk(chunk(&[0.0; FRAME], FRAME));
    assert!(
        h.sink.finish().input_silent,
        "the second section had only zeros"
    );
}

#[test]
fn a_split_before_any_audio_is_empty_and_not_silent() {
    let mut h = harness(VadPolicy::Disabled);
    let sections = h.sink.sections();
    let first = sections.split().expect("the recording is cut");
    assert_eq!(
        first,
        SplitSection {
            samples: Vec::new(),
            input_silent: false,
        }
    );
    h.sink.process_chunk(chunk(&[1.0; FRAME], FRAME));
    assert_eq!(h.sink.finish().samples, [1.0; FRAME]);
}

#[test]
fn a_recording_is_cut_only_once_and_not_after_it_finished() {
    let mut h = harness(VadPolicy::Disabled);
    let sections = h.sink.sections();
    h.sink.process_chunk(chunk(&[1.0; FRAME], FRAME));
    assert!(sections.split().is_some());
    h.sink.process_chunk(chunk(&[2.0; FRAME], FRAME));
    assert_eq!(sections.split(), None, "a second cut gives nothing");
    assert_eq!(h.sink.finish().samples, [2.0; FRAME]);

    let mut h = harness(VadPolicy::Disabled);
    let sections = h.sink.sections();
    h.sink.process_chunk(chunk(&[1.0; FRAME], FRAME));
    assert_eq!(h.sink.finish().samples, [1.0; FRAME]);
    assert_eq!(sections.split(), None, "a finished recording isn't cut");
}

#[test]
fn a_recording_never_cut_finishes_as_before() {
    let mut h = harness(VadPolicy::Disabled);
    let _sections = h.sink.sections();
    h.sink.process_chunk(chunk(&[1.0; FRAME], FRAME));
    h.sink.process_chunk(chunk(&[2.0; FRAME], FRAME));
    let both = [[1.0; FRAME], [2.0; FRAME]].concat();
    assert_eq!(*h.fed.lock().unwrap(), both);
    assert_eq!(h.sink.finish().samples, both);
}
