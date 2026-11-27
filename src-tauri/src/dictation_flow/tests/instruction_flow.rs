//! DanyWhere: a dictation with a spoken instruction, as a whole, from the
//! cut to the paste (sidecar note
//! `qa/2026-11-26_1720_nachbearbeitung-gesprochene-anweisung.md`): the
//! instruction key cuts the recording ([`begin`]), the text is transcribed
//! in the background, the end transcribes the instruction after it and
//! post-processes with both. Fakes as in the other flow tests, real History
//! and real post-processing against a local server.

use super::*;
use crate::dictation_flow::fakes::Gate;
use crate::dictation_flow::instruction::{begin, Cut, InstructionSlot};

const LONG: Duration = Duration::from_secs(10);

/// The instruction key the tests press.
const KEY: &str = "instruction_1795300000001";

/// The length of a scripted instruction: half a second.
const SAID: usize = 8_000;

/// What the scripted instruction is heard as.
const POLISH: &str = "translate it into Polish";

/// A slot of the tests' own, as the app's `PENDING`.
fn slot() -> &'static InstructionSlot {
    Box::leak(Box::new(InstructionSlot::new()))
}

/// `len` samples of sound; speech when there are any.
fn tone(len: usize) -> Recording {
    Recording {
        samples: vec![0.1; len],
        input_silent: false,
        speech: len > 0,
    }
}

/// A dictation whose text section is `text` and whose instruction section,
/// `instruction`, is heard as `heard`.
fn script(text: Recording, instruction: Recording, heard: &str) -> Script {
    Script {
        split: Some(text),
        recording: Some(instruction),
        instruction: Ok(heard.to_string()),
        ..Default::default()
    }
}

/// The overlay at the cut, and once the text is transcribed.
fn instruction_overlay(text_ready: bool, heard: Option<&str>) -> Event {
    Event::Overlay(Overlay::Instruction {
        text_ready,
        heard: heard.map(str::to_string),
    })
}

/// Presses the instruction key during the recording of session 1 (the
/// session of these tests' snapshots), as strand Z2's coordinator does.
fn press(ports: &FakePorts, slot: &'static InstructionSlot, streaming: bool) -> bool {
    begin(
        ports,
        slot,
        Cut {
            session: 1,
            binding_id: START,
            key_id: KEY,
            values: None,
            use_streaming_overlay: streaming,
        },
    )
}

/// [`press`], then waits until the text is transcribed (the check mark).
fn cut(ports: &FakePorts, slot: &'static InstructionSlot) {
    assert!(press(ports, slot, false), "the recording is cut");
    assert!(
        ports.wait_for(&instruction_overlay(true, None), LONG),
        "the check mark comes once the text is transcribed"
    );
}

/// Ends the dictation as `actions::stop_transcription` does: takes the
/// waiting instruction for `snapshot`'s session, shows the stop and
/// finishes it the instruction's way.
fn end(ports: &FakePorts, slot: &'static InstructionSlot, snapshot: DictationSnapshot) {
    let pending = slot
        .take(snapshot.session, START)
        .expect("the cut waits for the end");
    let snapshot = snapshot.with_spoken_instruction(pending.use_streaming_overlay());
    let held = crate::delivery::begin_input_source(&snapshot.settings, &ports.input_method());
    stopped(&snapshot, ports);
    tauri::async_runtime::block_on(finish_dictation(
        Dictation {
            snapshot,
            binding_id: START.to_string(),
            stop_time: Instant::now(),
            input_source: held,
            instruction: Some(pending.into_part()),
        },
        ports.clone(),
    ));
}

/// Post-processing at `url`, ended with the end shortcut that doesn't
/// post-process, appending `append_text`, with context if `send_context`.
fn ended_without_post_processing(
    url: &str,
    append_text: &str,
    send_context: bool,
) -> DictationSnapshot {
    let settings = with_end(post_process_settings(url), PLAIN, append_text, send_context);
    let snapshot = snapshot_of(settings, None, EndChoice::Key(PLAIN.to_string()));
    assert!(!snapshot.post_process, "the end alone doesn't post-process");
    snapshot
}

/// The ✨ steps of an entry: whether each ran with the spoken instruction.
fn ai_steps_with_instruction(entry: &crate::managers::history::HistoryEntry) -> Vec<bool> {
    entry
        .versions
        .iter()
        .flat_map(|version| &version.steps)
        .filter(|step| step.by == crate::history_versions::VersionAuthor::Ai)
        .map(|step| step.spoken_instruction)
        .collect()
}

/// From the post-processing on, a pasted dictation whose result is
/// "polished", saved as entry 1, with `samples` in all.
fn polished_and_pasted(values: &ProfileValues, append_text: &str, samples: usize) -> Vec<Event> {
    [
        saved_first(PLAIN),
        vec![
            Event::Hook("polished".to_string()),
            Event::SpawnPaste,
            Event::RedoLookedAt(1),
            paste("polished", values, append_text),
            Event::ClearNotice,
            Event::SoundAfterDictation(pasted_end()),
            Event::Overlay(Overlay::AfterDictation(None)),
            Event::Tray(TrayIconState::Idle),
            Event::Complete(TranscriptionOutcome::Typed, samples),
            Event::Finished,
        ],
    ]
    .concat()
}

#[test]
fn a_dictation_with_a_spoken_instruction_from_the_cut_to_the_paste() {
    let (url, requests) = llm_server(vec![("200 OK", POLISHED)]);
    let gate = Gate::default();
    let ports = FakePorts::new(Script {
        hold_text_section: Some(gate.clone()),
        ..script(tone(SAMPLES), tone(SAID), POLISH)
    });
    let slot = slot();

    // The instruction key: the recording is cut without a sound, the text
    // goes to transcription in the background, the pill says
    // "Instruction"; the check mark comes once the text is transcribed.
    assert!(press(&ports, slot, false));
    assert!(ports.wait_for(&Event::TranscribeTextSection { samples: SAMPLES }, LONG));
    assert!(
        !ports.wait_for(&instruction_overlay(true, None), Duration::from_millis(200)),
        "no check mark while the text is transcribed"
    );
    gate.open();
    assert!(ports.wait_for(&instruction_overlay(true, None), LONG));
    assert_eq!(
        ports.take_events(),
        [
            Event::SplitRecording,
            instruction_overlay(false, None),
            Event::TranscribeTextSection { samples: SAMPLES },
            instruction_overlay(true, None),
        ],
        "no stop sound, the tray as it was, the microphone on"
    );
    assert_eq!(slot.key_of(1).as_deref(), Some(KEY));

    // An end shortcut that doesn't post-process ends it: post-processed
    // anyway, with the instruction, then pasted with its "Append" text.
    let snapshot = ended_without_post_processing(&url, " (sent)", false);
    let values = snapshot.values.clone();
    end(&ports, slot, snapshot);

    let mut expected = stop_events(false);
    expected.extend([
        Event::StopRecording,
        Event::TranscribeInstruction {
            samples: SAID,
            load_end_model: false,
        },
        Event::ReleaseModel,
        instruction_overlay(true, Some(POLISH)),
        Event::Overlay(Overlay::Working {
            working: Working::PostProcessing,
            streaming: false,
        }),
        Event::SpokenInstruction(POLISH.to_string()),
        Event::PostProcess {
            post_process: true,
            with_window: false,
        },
    ]);
    expected.extend(polished_and_pasted(&values, " (sent)", SAMPLES + SAID));
    assert_eq!(ports.take_events(), expected);
    assert!(slot.take(1, START).is_none(), "the end took it");

    // The request carries the instruction in its block, the text as the
    // transcript.
    let request = requests.recv_timeout(LONG).unwrap().to_string();
    assert!(request.contains("<spoken_instruction>"), "{request}");
    assert!(request.contains(POLISH), "{request}");
    assert!(
        request.contains("<transcript>\\nhello\\n</transcript>"),
        "{request}"
    );

    // History keeps the text, the result, and the instruction with its
    // audio; the ✨ step says it ran with the instruction.
    let entry = &ports.entries()[0];
    assert_eq!(entry.transcription_text, "hello");
    assert!(entry.post_process_requested);
    assert_eq!(entry.post_processed_text.as_deref(), Some("polished"));
    assert_eq!(entry.spoken_instruction.as_deref(), Some(POLISH));
    let audio = entry
        .spoken_instruction_file
        .as_deref()
        .expect("the instruction's audio");
    assert!(ports.history().recordings_dir().join(audio).is_file());
    assert!(ports
        .history()
        .recordings_dir()
        .join(&entry.file_name)
        .is_file());
    assert_eq!(ai_steps_with_instruction(entry), [true]);
}

/// DanyWhere (review of 2026-11-26, finding 1): the dictation was cut for an
/// instruction in a profile that post-processes, but the profile it ends
/// with never does (the user switched to its application meanwhile).
/// Nothing goes to a provider: the text is pasted as heard, and History
/// keeps the instruction as text.
#[test]
fn an_instruction_in_a_dictation_that_ends_in_a_profile_set_to_never_sends_nothing() {
    let (url, requests) = llm_server(vec![("200 OK", POLISHED)]);
    let ports = FakePorts::new(script(tone(SAMPLES), tone(SAID), POLISH));
    let slot = slot();
    cut(&ports, slot);
    ports.take_events();
    let mut settings = with_end(post_process_settings(&url), PLAIN, " (sent)", true);
    settings.post_process_mode = crate::settings::PostProcessMode::Never;
    let snapshot = snapshot_of(settings, None, EndChoice::Key(PLAIN.to_string()));
    let values = snapshot.values.clone();
    end(&ports, slot, snapshot);

    let events = ports.take_events();
    assert!(
        events.contains(&Event::PostProcess {
            post_process: false,
            with_window: false,
        }),
        "{events:#?}"
    );
    assert!(
        !events.contains(&Event::Overlay(Overlay::Working {
            working: Working::PostProcessing,
            streaming: false,
        })),
        "{events:#?}"
    );
    assert!(
        events.contains(&paste("hello", &values, " (sent)")),
        "the text as heard, with its end's append text: {events:#?}"
    );
    assert!(events.contains(&Event::Complete(
        TranscriptionOutcome::Typed,
        SAMPLES + SAID
    )));
    assert!(
        requests.recv_timeout(Duration::from_millis(500)).is_err(),
        "no request left the computer"
    );
    let entry = &ports.entries()[0];
    assert_eq!(entry.transcription_text, "hello");
    assert_eq!(entry.post_processed_text, None);
    assert!(!entry.post_process_requested);
    assert_eq!(entry.spoken_instruction.as_deref(), Some(POLISH));
    assert!(entry.spoken_instruction_file.is_some());
    assert_eq!(ai_steps_with_instruction(entry), Vec::<bool>::new());
}

/// DanyWhere: the post-processing setup (sidecar note
/// `2026-11-26_2250_nachbearbeitung-einstieg-umsetzung`, section 2): ⇥
/// before post-processing is set up. The text goes out as heard, without
/// the instruction; nothing leaves the computer, and the notice says what
/// is missing, which offers the setup: before the setup was ever finished
/// "not set up" (review of 2026-11-27, M7), after it the part.
#[test]
fn an_instruction_before_post_processing_is_set_up_inserts_the_text_as_heard() {
    for (missing, finished, reason) in [
        ("key", false, "not-set-up"),
        ("provider", false, "not-set-up"),
        ("key", true, "no-api-key"),
        ("provider", true, "no-provider"),
    ] {
        let (url, requests) = llm_server(vec![("200 OK", POLISHED)]);
        let ports = FakePorts::new(script(tone(SAMPLES), tone(SAID), POLISH));
        let slot = slot();
        cut(&ports, slot);
        ports.take_events();
        let mut settings = with_end(post_process_settings(&url), PLAIN, "", false);
        settings.post_process_setup.finished = finished;
        if missing == "key" {
            settings.post_process_api_keys.clear();
        } else {
            settings.post_process_provider_id = "gone".to_string();
        }
        let snapshot = snapshot_of(settings, None, EndChoice::Key(PLAIN.to_string()));
        let values = snapshot.values.clone();
        end(&ports, slot, snapshot);

        let events = ports.take_events();
        assert!(
            events.contains(&paste("hello", &values, "")),
            "{missing}: the text as heard: {events:#?}"
        );
        let raised: Vec<&crate::notices::Notice> = events
            .iter()
            .filter_map(|event| match event {
                Event::Raise(notice) => Some(notice),
                _ => None,
            })
            .collect();
        assert_eq!(raised.len(), 1, "{missing}: {events:#?}");
        assert_eq!(raised[0].code, NoticeCode::PostProcessFailed);
        let detail: serde_json::Value =
            serde_json::from_str(raised[0].detail.as_deref().expect("a detail")).unwrap();
        assert_eq!(detail["reason"], reason, "{missing}");
        assert!(
            requests.recv_timeout(Duration::from_millis(500)).is_err(),
            "{missing}: no request left the computer"
        );
        let entry = &ports.entries()[0];
        assert_eq!(entry.transcription_text, "hello");
        assert_eq!(entry.post_processed_text, None);
        assert_eq!(entry.spoken_instruction.as_deref(), Some(POLISH));
    }
}

#[test]
fn an_instruction_heard_as_nothing_post_processes_with_the_prompts_alone() {
    let (url, requests) = llm_server(vec![("200 OK", POLISHED)]);
    let ports = FakePorts::new(script(tone(SAMPLES), tone(SAID), "  "));
    let slot = slot();
    cut(&ports, slot);
    ports.take_events();
    let snapshot = ended_without_post_processing(&url, "", false);
    let values = snapshot.values.clone();
    end(&ports, slot, snapshot);

    let mut expected = stop_events(false);
    expected.extend([
        Event::StopRecording,
        Event::TranscribeInstruction {
            samples: SAID,
            load_end_model: false,
        },
        Event::ReleaseModel,
        Event::Overlay(Overlay::Working {
            working: Working::PostProcessing,
            streaming: false,
        }),
        Event::PostProcess {
            post_process: true,
            with_window: false,
        },
    ]);
    expected.extend(polished_and_pasted(&values, "", SAMPLES + SAID));
    assert_eq!(ports.take_events(), expected);
    let request = requests.recv_timeout(LONG).unwrap().to_string();
    assert!(!request.contains("spoken_instruction"), "{request}");
    let entry = &ports.entries()[0];
    assert_eq!(entry.spoken_instruction.as_deref(), Some(""));
    assert_eq!(ai_steps_with_instruction(entry), [false]);
}

#[test]
fn an_instruction_section_without_speech_is_not_transcribed() {
    // Only the hangover of the text after the cut, or a cough: no voice.
    let (url, requests) = llm_server(vec![("200 OK", POLISHED)]);
    let silent = Recording {
        speech: false,
        ..tone(SAID)
    };
    let ports = FakePorts::new(script(tone(SAMPLES), silent, "Thank you."));
    let slot = slot();
    cut(&ports, slot);
    ports.take_events();
    end(&ports, slot, ended_without_post_processing(&url, "", false));

    let events = ports.take_events();
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::TranscribeInstruction { .. })),
        "{events:#?}"
    );
    assert!(events.contains(&Event::PostProcess {
        post_process: true,
        with_window: false,
    }));
    assert!(events.contains(&Event::Complete(
        TranscriptionOutcome::Typed,
        SAMPLES + SAID
    )));
    let request = requests.recv_timeout(LONG).unwrap().to_string();
    assert!(!request.contains("Thank you"), "{request}");
    let entry = &ports.entries()[0];
    assert_eq!(entry.spoken_instruction.as_deref(), Some(""));
    assert_eq!(
        entry.spoken_instruction_file, None,
        "no audio to hear again: without the detection it would be made up"
    );
}

/// DanyWhere (review of 2026-11-26, finding 4): with "unload the model
/// right after use" the end frees the model once both sections are dealt
/// with, before post-processing, as a dictation without a cut does; also
/// when no speech was heard in the instruction (⌃ Space, dictate, ⇥, say
/// nothing, ↩) and when the text is transcribed again for the end profile.
#[test]
fn the_model_is_released_once_both_sections_are_dealt_with() {
    let silent = Recording {
        speech: false,
        ..tone(SAID)
    };
    for (instruction, end_values) in [(tone(SAID), false), (silent, false), (tone(SAID), true)] {
        let case = format!("speech {}, end values {end_values}", instruction.speech);
        let transcribed = instruction.speech;
        let (url, _requests) = llm_server(vec![("200 OK", POLISHED)]);
        let ports = FakePorts::new(script(tone(SAMPLES), instruction, POLISH));
        let slot = slot();
        cut(&ports, slot);
        ports.take_events();
        let mut snapshot = ended_without_post_processing(&url, "", false);
        snapshot.transcribed_with_end_values = end_values;
        end(&ports, slot, snapshot);

        let events = ports.take_events();
        let first = |found: &dyn Fn(&Event) -> bool| events.iter().position(found);
        let released = first(&|event| *event == Event::ReleaseModel)
            .unwrap_or_else(|| panic!("{case}: the model is released: {events:#?}"));
        let post_processed = first(&|event| matches!(event, Event::PostProcess { .. }))
            .unwrap_or_else(|| panic!("{case}: post-processed: {events:#?}"));
        assert!(released < post_processed, "{case}: {events:#?}");
        let heard = first(&|event| matches!(event, Event::TranscribeInstruction { .. }));
        assert_eq!(heard.is_some(), transcribed, "{case}: {events:#?}");
        if let Some(heard) = heard {
            assert!(heard < released, "{case}: {events:#?}");
        }
        assert_eq!(
            events
                .iter()
                .filter(|event| **event == Event::ReleaseModel)
                .count(),
            1,
            "{case}"
        );
    }
}

#[test]
fn an_instruction_without_a_dictation_post_processes_the_instruction_alone() {
    // The instruction key right after the start (F5 i).
    let (url, requests) = llm_server(vec![("200 OK", POLISHED)]);
    let ports = FakePorts::new(script(tone(0), tone(SAID), "write a polite no"));
    let slot = slot();
    cut(&ports, slot);
    assert_eq!(
        ports.take_events(),
        [
            Event::SplitRecording,
            instruction_overlay(false, None),
            Event::CancelStream,
            instruction_overlay(true, None),
        ],
        "nothing to transcribe before the cut"
    );
    let snapshot = ended_without_post_processing(&url, "", false);
    let values = snapshot.values.clone();
    end(&ports, slot, snapshot);

    let mut expected = stop_events(false);
    expected.extend([
        Event::StopRecording,
        Event::TranscribeInstruction {
            samples: SAID,
            load_end_model: false,
        },
        Event::ReleaseModel,
        instruction_overlay(true, Some("write a polite no")),
        Event::Overlay(Overlay::Working {
            working: Working::PostProcessing,
            streaming: false,
        }),
        Event::SpokenInstruction("write a polite no".to_string()),
        Event::PostProcess {
            post_process: true,
            with_window: false,
        },
    ]);
    expected.extend(polished_and_pasted(&values, "", SAID));
    assert_eq!(ports.take_events(), expected);
    let request = requests.recv_timeout(LONG).unwrap().to_string();
    assert!(request.contains("write a polite no"), "{request}");
    let entry = &ports.entries()[0];
    assert_eq!(entry.transcription_text, "");
    assert_eq!(entry.post_processed_text.as_deref(), Some("polished"));
    assert_eq!(
        entry.spoken_instruction.as_deref(),
        Some("write a polite no")
    );
    assert!(
        ports
            .history()
            .recordings_dir()
            .join(&entry.file_name)
            .is_file(),
        "an empty recording, so the entry can be transcribed again"
    );
}

#[test]
fn neither_a_dictation_nor_an_instruction_ends_as_an_empty_dictation() {
    let ports = FakePorts::new(script(tone(0), tone(0), "never heard"));
    let slot = slot();
    cut(&ports, slot);
    ports.take_events();
    end(
        &ports,
        slot,
        ended_without_post_processing(&no_server(), "", false),
    );

    let mut expected = stop_events(false);
    expected.extend([
        Event::StopRecording,
        Event::Overlay(Overlay::Hide),
        Event::Tray(TrayIconState::Idle),
        Event::Complete(TranscriptionOutcome::Empty, 0),
        Event::Finished,
    ]);
    assert_eq!(ports.take_events(), expected);
    assert!(ports.entries().is_empty());
}

#[test]
fn escape_after_the_cut_throws_the_instruction_away() {
    let gate = Gate::default();
    let ports = FakePorts::new(Script {
        hold_text_section: Some(gate.clone()),
        ..script(tone(SAMPLES), tone(SAID), POLISH)
    });
    let slot = slot();
    assert!(press(&ports, slot, false));
    assert!(ports.wait_for(&Event::TranscribeTextSection { samples: SAMPLES }, LONG));

    // `utils::cancel_current_operation`: the dictation is cancelled and the
    // waiting instruction thrown away.
    ports.cancel();
    assert!(slot.discard(), "one waited");
    gate.open();
    assert!(
        !ports.wait_for(&instruction_overlay(true, None), Duration::from_millis(300)),
        "no check mark after Escape"
    );
    assert_eq!(
        ports.take_events(),
        [
            Event::SplitRecording,
            instruction_overlay(false, None),
            Event::TranscribeTextSection { samples: SAMPLES },
        ]
    );
    assert!(slot.take(1, START).is_none());
    assert!(!slot.discard(), "nothing waits any more");
    assert!(ports.entries().is_empty());
}

#[test]
fn escape_while_it_ends_saves_and_pastes_nothing() {
    let ports = FakePorts::new(Script {
        cancel_at: Some(CancelAt::Recording),
        ..script(tone(SAMPLES), tone(SAID), POLISH)
    });
    let slot = slot();
    cut(&ports, slot);
    ports.take_events();
    end(
        &ports,
        slot,
        ended_without_post_processing(&no_server(), "", false),
    );

    let mut expected = stop_events(false);
    expected.extend([
        Event::StopRecording,
        Event::CancelStream,
        Event::Overlay(Overlay::Hide),
        Event::Tray(TrayIconState::Idle),
        Event::Finished,
    ]);
    assert_eq!(ports.take_events(), expected);
    assert!(ports.entries().is_empty());
}

/// What a dictation cancelled after its recording stopped shows: the stop,
/// then nothing more on screen.
fn cancelled_after_the_stop() -> Vec<Event> {
    let mut expected = stop_events(false);
    expected.extend([
        Event::StopRecording,
        Event::Overlay(Overlay::Hide),
        Event::Tray(TrayIconState::Idle),
        Event::Finished,
    ]);
    expected
}

/// DanyWhere (review of 2026-11-26, finding 7): ✕ or Escape while the end
/// waits for the text: the instruction isn't transcribed, nothing is saved
/// or pasted.
#[test]
fn escape_while_the_end_waits_for_the_text_transcribes_no_instruction() {
    let gate = Gate::default();
    let ports = FakePorts::new(Script {
        hold_text_section: Some(gate.clone()),
        ..script(tone(SAMPLES), tone(SAID), POLISH)
    });
    let slot = slot();
    assert!(press(&ports, slot, false));
    assert!(ports.wait_for(&Event::TranscribeTextSection { samples: SAMPLES }, LONG));
    ports.take_events();
    let ending = {
        let ports = ports.clone();
        let snapshot = ended_without_post_processing(&no_server(), "", false);
        std::thread::spawn(move || end(&ports, slot, snapshot))
    };
    assert!(ports.wait_for(&Event::StopRecording, LONG));
    ports.cancel();
    gate.open();
    ending.join().expect("the end");

    assert_eq!(ports.take_events(), cancelled_after_the_stop());
    assert!(ports.entries().is_empty());
}

/// The same when the profile it ends with transcribes otherwise: a cancel
/// while the text is transcribed again leaves the instruction alone.
#[test]
fn escape_while_the_text_is_transcribed_again_transcribes_no_instruction() {
    let ports = FakePorts::new(Script {
        cancel_at: Some(CancelAt::Transcribing),
        ..script(tone(SAMPLES), tone(SAID), POLISH)
    });
    let slot = slot();
    cut(&ports, slot);
    ports.take_events();
    let mut snapshot = ended_without_post_processing(&no_server(), "", false);
    snapshot.transcribed_with_end_values = true;
    end(&ports, slot, snapshot);

    let events = ports.take_events();
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::TranscribeInstruction { .. })),
        "{events:#?}"
    );
    assert_eq!(
        after(
            &events,
            &Event::Transcribe {
                samples: SAMPLES,
                load_end_model: true,
            }
        ),
        [
            Event::Overlay(Overlay::Hide),
            Event::Tray(TrayIconState::Idle),
            Event::Finished,
        ]
    );
    assert!(ports.entries().is_empty());
}

#[test]
fn escape_while_it_is_post_processed_saves_and_pastes_nothing() {
    let ports = FakePorts::new(Script {
        cancel_at: Some(CancelAt::PostProcessing),
        ..script(tone(SAMPLES), tone(SAID), POLISH)
    });
    let slot = slot();
    cut(&ports, slot);
    ports.take_events();
    end(
        &ports,
        slot,
        ended_without_post_processing(&no_server(), "", false),
    );

    let events = ports.take_events();
    assert_eq!(
        after(&events, &Event::SpokenInstruction(POLISH.to_string())),
        [
            Event::PostProcess {
                post_process: true,
                with_window: false,
            },
            Event::Overlay(Overlay::Hide),
            Event::Tray(TrayIconState::Idle),
            Event::Finished,
        ]
    );
    assert!(ports.entries().is_empty());
}

#[test]
fn the_instruction_key_cuts_once_and_only_a_running_recording() {
    let ports = FakePorts::new(Script::default());
    let slot = slot();
    assert!(!press(&ports, slot, false), "nothing to cut");
    assert_eq!(ports.take_events(), [Event::SplitRecording]);
    assert!(slot.take(1, START).is_none());

    let ports = FakePorts::new(script(tone(SAMPLES), tone(SAID), POLISH));
    cut(&ports, slot);
    ports.take_events();
    assert!(
        !press(&ports, slot, false),
        "a second press during the instruction does nothing"
    );
    assert_eq!(ports.take_events(), []);
    assert!(slot.holds(1));
    assert!(!slot.holds(2), "another session's");
}

#[test]
fn the_end_of_another_recording_leaves_no_instruction_behind() {
    let ports = FakePorts::new(script(tone(SAMPLES), tone(SAID), POLISH));
    let slot = slot();
    cut(&ports, slot);
    assert!(slot.take(1, "another_binding").is_none());
    assert!(
        slot.take(1, START).is_none(),
        "a stale instruction is thrown away"
    );
    let ports = FakePorts::new(script(tone(SAMPLES), tone(SAID), POLISH));
    cut(&ports, slot);
    assert!(slot.take(2, START).is_none(), "another session");
    assert!(!slot.holds(1));
}

#[test]
fn an_end_profile_with_another_model_transcribes_the_text_again() {
    let (url, _requests) = llm_server(vec![("200 OK", POLISHED)]);
    let ports = FakePorts::new(script(tone(SAMPLES), tone(SAID), POLISH));
    let slot = slot();
    cut(&ports, slot);
    ports.take_events();
    let mut snapshot = ended_without_post_processing(&url, "", false);
    snapshot.transcribed_with_end_values = true;
    end(&ports, slot, snapshot);

    let events = ports.take_events();
    assert_eq!(
        after(&events, &Event::StopRecording)[..2],
        [
            Event::Transcribe {
                samples: SAMPLES,
                load_end_model: true,
            },
            Event::TranscribeInstruction {
                samples: SAID,
                load_end_model: true,
            },
        ]
    );
    assert_eq!(ports.entries()[0].transcription_text, "hello");
}

/// DanyWhere (user, 2026-11-26 22:15): a failed instruction pastes nothing
/// and leaves the clipboard alone, but its text is the last transcript:
/// "Paste Last Transcript" and "Copy Last Transcript" deliver it
/// (`delivery::latest_entry_and_text`), and the notice says so.
#[test]
fn a_failed_instruction_keeps_its_text_ready_for_paste_last_and_pastes_nothing() {
    let ports = FakePorts::new(Script {
        instruction: Err("the model failed".to_string()),
        ..script(tone(SAMPLES), tone(SAID), POLISH)
    });
    let slot = slot();
    cut(&ports, slot);
    ports.take_events();
    end(
        &ports,
        slot,
        ended_without_post_processing(&no_server(), "", false),
    );

    let mut expected = stop_events(false);
    expected.extend([
        Event::StopRecording,
        Event::TranscribeInstruction {
            samples: SAID,
            load_end_model: false,
        },
        Event::ReleaseModel,
        Event::HistoryAdded(1),
        Event::Raise(
            notice(NoticeCode::InstructionTranscriptionFailed)
                .with_detail("the model failed".to_string()),
        ),
        Event::Tray(TrayIconState::Idle),
        Event::Complete(TranscriptionOutcome::Failed, SAMPLES + SAID),
        Event::Finished,
    ]);
    assert_eq!(ports.take_events(), expected);
    // "Transcribe Again" hears both again and post-processes.
    let entry = &ports.entries()[0];
    assert_eq!(entry.transcription_text, "hello");
    assert!(entry.post_process_requested);
    assert_eq!(entry.spoken_instruction.as_deref(), Some(""));
    assert!(entry.spoken_instruction_file.is_some());
    // What "Paste Last Transcript" and "Copy Last Transcript" deliver.
    let (last, text) = crate::delivery::latest_entry_and_text_in(ports.history())
        .expect("the text is the last transcript");
    assert_eq!((last.id, text.as_str()), (entry.id, "hello"));
}

/// A failed instruction with no text before it, or with a text that failed
/// itself, leaves nothing to paste: the notice says only that the
/// transcription failed, and "Paste Last Transcript" finds nothing.
#[test]
fn a_failed_instruction_without_a_text_leaves_nothing_to_paste() {
    let failing =
        |text: Recording, transcription: Result<&str, &str>, instruction: Result<&str, &str>| {
            Script {
                transcription: transcription.map(str::to_string).map_err(str::to_string),
                instruction: instruction.map(str::to_string).map_err(str::to_string),
                ..script(text, tone(SAID), POLISH)
            }
        };
    let cases = [
        (
            "nothing said before the instruction",
            failing(tone(0), Ok("hello"), Err("the model failed")),
            "the model failed",
        ),
        (
            "the text heard as nothing",
            failing(tone(SAMPLES), Ok(" "), Err("the model failed")),
            "the model failed",
        ),
        (
            "the text's transcription failed",
            failing(tone(SAMPLES), Err("the text failed"), Ok(POLISH)),
            "the text failed",
        ),
    ];
    for (case, script, detail) in cases {
        let ports = FakePorts::new(script);
        let slot = slot();
        cut(&ports, slot);
        ports.take_events();
        end(
            &ports,
            slot,
            ended_without_post_processing(&no_server(), "", false),
        );

        let events = ports.take_events();
        let raised: Vec<_> = events
            .iter()
            .filter(|event| matches!(event, Event::Raise(_) | Event::RaiseWithoutOverlay(_)))
            .collect();
        assert_eq!(
            raised,
            [&Event::Raise(
                notice(NoticeCode::TranscriptionFailed).with_detail(detail.to_string())
            )],
            "{case}"
        );
        assert!(
            !events.iter().any(|event| matches!(
                event,
                Event::Paste { .. }
                    | Event::LeftOnClipboard(_)
                    | Event::PutOnClipboard(_)
                    | Event::InputMethod(_)
                    | Event::ServiceInserted(_)
            )),
            "{case}: {events:#?}"
        );
        assert_eq!(ports.entries().len(), 1, "{case}: kept to transcribe again");
        assert!(
            crate::delivery::latest_entry_and_text_in(ports.history()).is_none(),
            "{case}: nothing to paste"
        );
    }
}

#[test]
fn the_instruction_never_goes_along_as_context_of_a_later_dictation() {
    let (url, requests) = llm_server(vec![("200 OK", POLISHED), ("200 OK", POLISHED)]);
    let window = Script {
        window: Some(notes_window()),
        ..Script::default()
    };
    let ports = FakePorts::new(Script {
        window: Some(notes_window()),
        ..script(tone(SAMPLES), tone(SAID), POLISH)
    });
    let slot = slot();
    cut(&ports, slot);
    // With context: the first dictation's request may carry earlier ones.
    end(&ports, slot, ended_without_post_processing(&url, "", true));
    let _first = requests.recv_timeout(LONG).unwrap();

    // The next dictation in the same window, post-processed with context.
    ports.set_script(window);
    let settings = with_end(post_process_settings(&url), POST_PROCESS_ID, "", true);
    finish(&ports, snapshot_of(settings, None, post_processing()));
    let second = requests.recv_timeout(LONG).unwrap().to_string();
    assert!(
        second.contains("polished"),
        "the first one's text: {second}"
    );
    assert!(!second.contains(POLISH), "{second}");
    assert!(!second.contains("spoken_instruction"), "{second}");
}
