use crate::end_keys::EndChoice;
use crate::managers::audio::AudioRecordingManager;
use crate::start_keys::Roles;
use log::{debug, error, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

const DEBOUNCE: Duration = Duration::from_millis(30);
const RELEASE_GRACE: Duration = Duration::from_millis(50);

/// Recording phase events for windows that follow along (onboarding's Try it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum RecordingPhase {
    /// Recording began.
    Started,
    /// The key came up before the hold threshold (or the binding toggles):
    /// recording stays on until the next press.
    Locked,
    /// Recording ended; transcription follows.
    Stopped,
    /// The session was cancelled.
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct RecordingStateEvent {
    pub phase: RecordingPhase,
}

fn emit_recording_phase(app: &AppHandle, phase: RecordingPhase) {
    if let Err(e) = (RecordingStateEvent { phase }).emit(app) {
        warn!("Failed to emit recording state {phase:?}: {e}");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PttAction {
    Passthrough,
    DeferRelease,
    CancelRelease,
}

/// DanyWhere: how a press drives a recording, from the lists its
/// combination is in (`start_keys::Roles`; user, 2026-11-26). Handy's three
/// activation modes, now one per press instead of one per profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PressMode {
    /// Only start shortcuts (and remote control): the press starts and the
    /// recording locks until an end shortcut (Handy's "Toggle").
    Tap,
    /// Only hold shortcuts: the press starts; a release from the threshold
    /// on ends with the hold shortcut's values, a shorter one discards the
    /// recording, as a cancel would (question 3 a).
    Hold,
    /// Start and hold shortcuts: a short press locks, a long one ends with
    /// the hold shortcut on its release (Handy's "Auto").
    Auto,
}

/// DanyWhere: a press's mode, and the hold shortcut whose values its
/// release ends with.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Press {
    mode: PressMode,
    hold_id: Option<String>,
}

impl Press {
    /// A press that locks at once: a start shortcut's alone, remote
    /// control's, a service's.
    fn tap() -> Self {
        Self {
            mode: PressMode::Tap,
            hold_id: None,
        }
    }

    /// How a release of the key that made this press ends the recording
    /// of `binding_id`: with the hold shortcut's values.
    fn held_end(&self, binding_id: &str) -> EndChoice {
        match &self.hold_id {
            Some(id) => EndChoice::Held(id.clone()),
            None => crate::start_keys::end_as_press(binding_id)
                .unwrap_or_else(|| EndChoice::Start(binding_id.to_string())),
        }
    }
}

/// A key-up deferred by `RELEASE_GRACE` so a synthesized X11 auto-repeat
/// press can cancel it (#1539). When the grace elapses the hold is resolved
/// by [`CoordinatorState::finish_hold`] (recording) or
/// [`CoordinatorState::finish_pending_hold`] (press remembered while busy).
struct PendingRelease {
    binding_id: String,
    hotkey_string: String,
    deadline: Instant,
    /// When the key actually went up. The hold duration is measured to this
    /// instant, not to the grace expiry.
    released_at: Instant,
    /// Holds at least this long end the recording with the hold shortcut;
    /// a shorter one locks it ([`PressMode::Auto`]) or discards it
    /// ([`PressMode::Hold`]).
    hold_threshold: Duration,
}

/// A press that arrived while the pipeline was still busy processing the
/// previous transcription. Toggle-style triggers (SIGUSR2, CLI flags, some
/// pedal setups) flip state on every edge, so dropping a busy press desyncs
/// the parity: the next edge starts a recording nobody will ever stop.
struct PendingPress {
    binding_id: String,
    hotkey_string: String,
    /// The real key-down time, so a hold that straddles the drain is still
    /// measured from when the user pressed, not from when recording began.
    pressed_at: Instant,
    /// The recording will start locked on when the pipeline drains: set from
    /// the start for a tap, and for "Auto" once the key came back up within
    /// the threshold. An unlocked pending press is a key we believe is still
    /// held.
    locked: bool,
    /// DanyWhere: how it drives the recording it starts.
    press: Press,
}

impl PendingPress {
    fn remembered(&self) -> Remembered {
        if self.locked {
            Remembered::Locked
        } else {
            Remembered::Held
        }
    }
}

/// What kind of press is already waiting for the pipeline to drain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Remembered {
    /// The key is still down as far as we know.
    Held,
    /// A toggle press or a classified tap: it will start a locked session.
    Locked,
}

/// Bookkeeping for the key press that started the current recording.
struct Hold {
    pressed_at: Instant,
    /// Recording outlives the key: the next press stops it, releases are
    /// ignored. Always set for a tap; set for "Auto" once a release has been
    /// classified as a tap.
    locked: bool,
    /// DanyWhere: how the press that started it drives it.
    press: Press,
}

/// What to do with an input that arrives while the pipeline is busy
/// (`Stage::Processing`). `remembered` is the press for the same binding
/// already waiting for the pipeline to drain, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BusyAction {
    /// Ignore the input entirely.
    Ignore,
    /// Remember the press; start recording when the pipeline finishes.
    Remember,
    /// This press cancels a previously remembered press: two presses during
    /// one busy window net to no-op, exactly as a press stops a locked
    /// session once recording.
    Forget,
}

fn classify_busy_input(
    is_pressed: bool,
    mode: PressMode,
    remembered: Option<Remembered>,
) -> BusyAction {
    use PressMode::*;
    match (mode, is_pressed, remembered) {
        // Tap: presses alternate remember/forget to preserve parity.
        (Tap, true, Some(_)) => BusyAction::Forget,
        (Tap, true, None) => BusyAction::Remember,
        // A tap ignores releases.
        (Tap, false, _) => BusyAction::Ignore,
        // Hold modes: a press while busy means the user is holding the key —
        // start as soon as the pipeline drains. A press on a queued tap stops
        // it (parity); a press while the key is already down is a repeat.
        (Hold | Auto, true, None) => BusyAction::Remember,
        (Hold | Auto, true, Some(Remembered::Locked)) => BusyAction::Forget,
        (Hold | Auto, true, Some(Remembered::Held)) => BusyAction::Ignore,
        // Releases of a held pending press are deferred by the grace window
        // before reaching here and resolved by `finish_pending_hold`; any
        // other release (no press remembered, or already locked) is noise.
        (Hold | Auto, false, _) => BusyAction::Ignore,
    }
}

/// Pipeline lifecycle.
#[derive(Debug, PartialEq, Eq)]
enum Stage {
    Idle,
    Recording(String), // binding_id
    /// DanyWhere: "Recording: instruction" (user, 2026-11-26; sidecar note
    /// `2026-11-26_1720_nachbearbeitung-gesprochene-anweisung`): the
    /// recording of the binding went on to a spoken instruction with an
    /// instruction key ([`CoordinatorState::instruction`]). The microphone
    /// records the instruction now (`dictation_flow::instruction`); it ends
    /// as a recording ends, and its press's release can end it too.
    Instruction(String),
    Processing,
}

/// DanyWhere: the instruction key's press that switched the recording to
/// its instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
struct InstructionPress {
    /// The instruction key's id (`instruction_keys`).
    id: String,
    pressed_at: Instant,
    /// Tapped (released before `threshold`): an end shortcut sends the
    /// dictation; its next release means nothing.
    locked: bool,
    /// The start or hold shortcut binding it came as, when it has exactly
    /// that one's combination (`start_keys::Roles::instruction`); `None`
    /// for its own binding (`instruction_key:<id>`).
    via: Option<String>,
    /// Held at least this long, its release sends the dictation with the
    /// instruction key's values.
    threshold: Duration,
}

/// A keyboard/signal edge for a start or hold shortcut.
struct InputEvent {
    binding_id: String,
    hotkey_string: String,
    is_pressed: bool,
    /// A press of a combination in both lists held at least this long holds;
    /// a hold shortcut alone released sooner discards its recording.
    hold_threshold: Duration,
    /// External triggers (SIGUSR2, CLI flags) rather than physical keys,
    /// with how their toggle during a recording ends it (DanyWhere). They
    /// fire on every edge by design and must never be debounced — dropping
    /// one desyncs toggle parity and wedges recording on.
    external: Option<EndChoice>,
    /// When the sender observed the edge, not when the coordinator thread
    /// dequeues it. The coordinator executes `Effect::Start` inline, and the
    /// first activation after startup spends hundreds of milliseconds
    /// initializing the microphone; a release queued behind that block must
    /// still be measured from the real key-up, or a short tap classifies as
    /// a hold and the just-started recording stops with zero samples
    /// (#2089). The loop passes this as the `now` of `on_input`.
    received_at: Instant,
    /// DanyWhere: what the pressed combination is in the profile that
    /// applies now (`start_keys::roles_now`), filled in by the coordinator
    /// thread for a key's press: its start, hold and end shortcut.
    roles: Roles,
}

impl InputEvent {
    /// How this press drives a recording: a tap for remote control, else by
    /// the lists its combination is in; `None` when it is in none now (the
    /// profile changed since it was registered).
    fn press(&self) -> Option<Press> {
        if self.external.is_some() {
            return Some(Press::tap());
        }
        let mode = match (self.roles.start.is_some(), self.roles.hold.is_some()) {
            (true, true) => PressMode::Auto,
            (true, false) => PressMode::Tap,
            (false, true) => PressMode::Hold,
            (false, false) => return None,
        };
        Some(Press {
            mode,
            hold_id: self.roles.hold.clone(),
        })
    }
}

/// A side effect decided by [`CoordinatorState`]; the coordinator thread is
/// the only executor. Keeping decisions pure lets tests drive the exact
/// production transitions without a Tauri `AppHandle` or real timers.
#[derive(Debug, PartialEq, Eq)]
enum Effect {
    Start {
        binding_id: String,
        hotkey_string: String,
    },
    Stop {
        /// The binding whose recording is stopped.
        binding_id: String,
        hotkey_string: String,
        /// DanyWhere: how the recording ends (`end_keys`): the hold
        /// shortcut's values when it was released, else the end shortcut
        /// pressed (also as a start or hold shortcut with its combination),
        /// or as a press of a start shortcut for the services and remote
        /// control.
        end: EndChoice,
    },
    /// DanyWhere: the recording of `binding_id` is dropped, as a cancel
    /// would drop it (a hold shortcut alone released before the threshold).
    Discard { binding_id: String },
    /// DanyWhere: the recording of `binding_id` goes on to a spoken
    /// instruction with instruction key `instruction_id`: it is cut, and
    /// the microphone records the instruction (`dictation_flow::instruction`).
    BeginInstruction {
        binding_id: String,
        instruction_id: String,
    },
}

/// Commands processed sequentially by the coordinator thread.
enum Command {
    Input(InputEvent),
    Cancel {
        recording_was_active: bool,
    },
    ProcessingFinished,
    /// DanyWhere: end the running recording, if any (DanyWhere's services,
    /// which then insert its text), with `end`, or as a press of its own
    /// key without one.
    Finish {
        end: Option<EndChoice>,
    },
    /// DanyWhere: start a locked recording if nothing records (an "end
    /// dictation" service called while idle).
    StartLocked {
        binding_id: String,
    },
    /// DanyWhere: an end shortcut (`end_keys`) was pressed.
    EndKey {
        end_id: String,
    },
    /// DanyWhere: an instruction key's edge (`instruction_keys`), stamped
    /// when it was observed, as a key's edge is.
    InstructionKey {
        instruction_id: String,
        is_pressed: bool,
        hold_threshold: Duration,
        received_at: Instant,
    },
}

/// Decide whether a key-up should be deferred (so auto-repeat can cancel it)
/// or a key-down cancels a deferred release. `hold_to_talk` is whether a
/// release currently ends the session: true for an unlocked session that
/// holds, false for a tap and a locked session. `held_binding` is the
/// binding whose key we believe is down — the one recording, or the one
/// remembered while the pipeline is busy.
fn classify_ptt_event(
    pending_release_binding: Option<&str>,
    is_pressed: bool,
    hold_to_talk: bool,
    binding_id: &str,
    held_binding: Option<&str>,
) -> PttAction {
    if !hold_to_talk {
        return PttAction::Passthrough;
    }

    if is_pressed {
        if pending_release_binding == Some(binding_id) {
            PttAction::CancelRelease
        } else {
            PttAction::Passthrough
        }
    } else if held_binding == Some(binding_id) && pending_release_binding.is_none() {
        PttAction::DeferRelease
    } else {
        PttAction::Passthrough
    }
}

/// Pure lifecycle state machine: owns every transition decision (release
/// grace, hold-vs-tap classification, debounce, busy-pipeline
/// remember/forget, cancel, drain). Produces [`Effect`]s instead of touching
/// the app, so unit tests exercise the real production logic.
///
/// All three modes of a press ([`PressMode`]) run through one machine. A
/// recording starts on key-down in every mode; what differs is how it ends:
///
/// * hold — a release from the threshold on ends it with the hold
///   shortcut, a shorter one discards it
/// * tap — releases are ignored, the next press of an end shortcut stops
///   (locked from the start)
/// * auto — a release after a long hold ends it with the hold shortcut; a
///   release after a short tap locks the session
///
/// During a recording a press of a start or hold shortcut ends it only as
/// the end shortcut with exactly its combination (`Roles::end`).
///
/// DanyWhere: an instruction key pressed during a recording (its own
/// binding, or a start or hold shortcut with exactly its combination,
/// `Roles::instruction`) goes on to `Stage::Instruction` once; tapped, an
/// end shortcut sends the dictation, held, its release does
/// (`EndChoice::Instruction`); the release of the key that holds the
/// recording sends it as its end; Escape drops everything.
struct CoordinatorState {
    stage: Stage,
    hold: Option<Hold>,
    last_press: Option<Instant>,
    pending_release: Option<PendingRelease>,
    pending_press: Option<PendingPress>,
    /// DanyWhere: the instruction key's press during `Stage::Instruction`.
    instruction: Option<InstructionPress>,
}

impl CoordinatorState {
    fn new() -> Self {
        Self {
            stage: Stage::Idle,
            hold: None,
            last_press: None,
            pending_release: None,
            pending_press: None,
            instruction: None,
        }
    }

    /// DanyWhere: the binding of the recording, while it records its text
    /// or its instruction.
    fn recording_binding(&self) -> Option<&str> {
        match &self.stage {
            Stage::Recording(id) | Stage::Instruction(id) => Some(id.as_str()),
            Stage::Idle | Stage::Processing => None,
        }
    }

    /// DanyWhere: an edge of instruction key `instruction_id`, registered as
    /// its own binding (`instruction_key:<id>`, only during a recording).
    /// A press during a recording goes on to the instruction; once only,
    /// another press means nothing. Its release from `threshold` on sends
    /// the dictation with the instruction key's values; a shorter one locks
    /// the instruction until an end shortcut sends it. Nothing while idle
    /// or transcribing.
    fn on_instruction_key(
        &mut self,
        instruction_id: &str,
        is_pressed: bool,
        now: Instant,
        threshold: Duration,
    ) -> Option<Effect> {
        if is_pressed {
            self.begin_instruction(instruction_id, None, now, threshold)
        } else {
            self.instruction_released(instruction_id, None, now)
        }
    }

    /// DanyWhere: an instruction key was pressed (as its own binding, or as
    /// start or hold shortcut binding `via`): from a recording of its text
    /// on to its instruction; nothing in any other stage.
    fn begin_instruction(
        &mut self,
        instruction_id: &str,
        via: Option<String>,
        now: Instant,
        threshold: Duration,
    ) -> Option<Effect> {
        let Stage::Recording(binding_id) = &self.stage else {
            debug!("Instruction key '{instruction_id}' ignored: no recording of a text");
            return None;
        };
        let binding_id = binding_id.clone();
        self.stage = Stage::Instruction(binding_id.clone());
        self.instruction = Some(InstructionPress {
            id: instruction_id.to_string(),
            pressed_at: now,
            locked: false,
            via,
            threshold,
        });
        Some(Effect::BeginInstruction {
            binding_id,
            instruction_id: instruction_id.to_string(),
        })
    }

    /// DanyWhere: the key that switched to the instruction (`via`, see
    /// [`InstructionPress::via`]) came up: held long enough, the dictation
    /// is sent with its values ([`EndChoice::Instruction`]); a tap locks
    /// the instruction. A release of another key, or after the lock, means
    /// nothing.
    fn instruction_released(
        &mut self,
        instruction_id: &str,
        via: Option<&str>,
        now: Instant,
    ) -> Option<Effect> {
        let Stage::Instruction(binding_id) = &self.stage else {
            return None;
        };
        let binding_id = binding_id.clone();
        let press = self
            .instruction
            .as_mut()
            .filter(|press| press.id == instruction_id && press.via.as_deref() == via)?;
        if press.locked {
            return None;
        }
        let held = now.saturating_duration_since(press.pressed_at);
        if held < press.threshold {
            debug!("Instruction key '{instruction_id}' tapped ({held:?}): an end shortcut sends");
            press.locked = true;
            return None;
        }
        // A key-up still in its grace window belongs to this recording.
        self.pending_release = None;
        Some(self.begin_processing_with(
            binding_id,
            "instruction-key".to_string(),
            EndChoice::Instruction(instruction_id.to_string()),
        ))
    }

    /// DanyWhere: whether the recording was cut for its instruction
    /// (`dictation_flow::instruction::begin_instruction`). Not cut, it stays
    /// a recording of its text, as before the press.
    fn on_instruction_result(&mut self, began: bool) {
        if began {
            return;
        }
        if let Stage::Instruction(binding_id) = &self.stage {
            debug!("The recording of '{binding_id}' was not cut; it records its text on");
            self.stage = Stage::Recording(binding_id.clone());
            self.instruction = None;
        }
    }

    /// Deadline of the deferred release, if any — drives `recv_timeout`.
    fn grace_deadline(&self) -> Option<Instant> {
        self.pending_release.as_ref().map(|p| p.deadline)
    }

    /// Whether the current session (recording, or remembered for the drain)
    /// outlives the key, so releases are ignored and the next press ends it.
    fn is_locked(&self) -> bool {
        self.hold.as_ref().is_some_and(|h| h.locked)
            || self.pending_press.as_ref().is_some_and(|p| p.locked)
    }

    /// A recording is running and outlives its key (a tap, or toggle mode).
    fn is_recording_locked(&self) -> bool {
        self.recording_binding().is_some() && self.hold.as_ref().is_some_and(|h| h.locked)
    }

    /// Whether the press of the current session (recording, or remembered
    /// for the drain) can end on its release: it isn't a tap.
    fn session_holds(&self) -> bool {
        let mode = match &self.stage {
            Stage::Recording(_) | Stage::Instruction(_) => self.hold.as_ref().map(|h| h.press.mode),
            Stage::Processing => self.pending_press.as_ref().map(|p| p.press.mode),
            Stage::Idle => None,
        };
        mode.is_some_and(|mode| mode != PressMode::Tap)
    }

    fn on_input(&mut self, input: InputEvent, now: Instant) -> Option<Effect> {
        let pending_release_binding = self
            .pending_release
            .as_ref()
            .map(|pending| pending.binding_id.as_str());
        let held_binding = match &self.stage {
            Stage::Recording(id) | Stage::Instruction(id) => Some(id.as_str()),
            Stage::Processing => self.pending_press.as_ref().map(|p| p.binding_id.as_str()),
            Stage::Idle => None,
        };
        let hold_to_talk = self.session_holds() && !self.is_locked();

        match classify_ptt_event(
            pending_release_binding,
            input.is_pressed,
            hold_to_talk,
            &input.binding_id,
            held_binding,
        ) {
            PttAction::CancelRelease => {
                self.pending_release = None;
                return None;
            }
            PttAction::DeferRelease => {
                self.pending_release = Some(PendingRelease {
                    hold_threshold: input.hold_threshold,
                    binding_id: input.binding_id,
                    hotkey_string: input.hotkey_string,
                    deadline: now + RELEASE_GRACE,
                    released_at: now,
                });
                return None;
            }
            PttAction::Passthrough => {}
        }

        // Debounce rapid-fire press events (key repeat / double-tap).
        // Releases in the hold modes are deferred above to absorb X11 auto-repeat.
        // External triggers are exempt: each one is a deliberate edge from the
        // user's own integration, and dropping it desyncs toggle parity.
        if input.is_pressed && input.external.is_none() {
            if self
                .last_press
                .is_some_and(|t| now.duration_since(t) < DEBOUNCE)
            {
                debug!("Debounced press for '{}'", input.binding_id);
                return None;
            }
            self.last_press = Some(now);
        }

        let press = input.press();

        // A busy pipeline can't accept lifecycle changes now: classify the
        // input against any already-remembered press instead of dropping it
        // silently.
        if let Stage::Processing = self.stage {
            // Only one press can be remembered. Once a binding has claimed it,
            // inputs for a different binding are ignored — the same rule as a
            // different binding pressed while recording — rather than silently
            // replacing the remembered press and breaking its parity.
            if let Some(pending) = &self.pending_press {
                if pending.binding_id != input.binding_id {
                    debug!(
                        "Ignoring input for '{}': '{}' is already pending",
                        input.binding_id, pending.binding_id
                    );
                    return None;
                }
            }
            let mode = match (&press, input.is_pressed) {
                (Some(press), _) => press.mode,
                (None, true) => {
                    debug!(
                        "Ignoring press for '{}': its combination is in no list now",
                        input.binding_id
                    );
                    return None;
                }
                // Releases aren't remembered in any mode.
                (None, false) => PressMode::Tap,
            };
            let remembered = self.pending_press.as_ref().map(|p| p.remembered());
            match (
                classify_busy_input(input.is_pressed, mode, remembered),
                press,
            ) {
                (BusyAction::Remember, Some(press)) => {
                    debug!(
                        "Remembering press for '{}': pipeline busy",
                        input.binding_id
                    );
                    self.pending_press = Some(PendingPress {
                        // A tap never ends on a release: locked from the start.
                        locked: press.mode == PressMode::Tap,
                        binding_id: input.binding_id,
                        hotkey_string: input.hotkey_string,
                        pressed_at: now,
                        press,
                    });
                }
                (BusyAction::Forget, _) => {
                    debug!("Forgetting remembered press for '{}'", input.binding_id);
                    self.pending_press = None;
                }
                _ => {
                    debug!("Ignoring input for '{}': pipeline busy", input.binding_id);
                }
            }
            return None;
        }

        if input.is_pressed {
            match &self.stage {
                Stage::Idle => {
                    let Some(press) = press else {
                        debug!(
                            "Ignoring press for '{}': its combination is in no list now",
                            input.binding_id
                        );
                        return None;
                    };
                    // A tap never ends on a release: locked from the start.
                    let locked = press.mode == PressMode::Tap;
                    return Some(self.begin_recording(
                        input.binding_id,
                        input.hotkey_string,
                        now,
                        locked,
                        press,
                    ));
                }
                Stage::Recording(id)
                    if id == &input.binding_id && !self.is_locked() && input.external.is_none() =>
                {
                    // The key is still held (its release will end this
                    // recording), so a repeated press means nothing.
                    debug!("Ignoring press for '{}': key is held", input.binding_id);
                }
                // DanyWhere: start, hold and end shortcuts are apart (user,
                // 2026-11-22 and 2026-11-26). A start or hold shortcut's
                // press during a recording, its own second press or another
                // one's, held or locked, is a press of the end shortcut with
                // its combination, if any; the recording stays owned by the
                // binding that started it. Remote control always ends, so
                // its toggles stay paired.
                // DanyWhere: an instruction key with exactly this
                // combination (`start_keys::Roles::instruction`) goes on to
                // the instruction; the key's release can send it then.
                Stage::Recording(_)
                    if input.external.is_none() && input.roles.instruction.is_some() =>
                {
                    let instruction_id = input.roles.instruction.unwrap_or_default();
                    return self.begin_instruction(
                        &instruction_id,
                        Some(input.binding_id),
                        now,
                        input.hold_threshold,
                    );
                }
                // DanyWhere: during the instruction the key still held means
                // nothing, nor does an instruction key's second press.
                Stage::Instruction(id)
                    if input.external.is_none()
                        && ((id == &input.binding_id && !self.is_locked())
                            || input.roles.instruction.is_some()) =>
                {
                    debug!(
                        "Ignoring press for '{}' during the instruction",
                        input.binding_id
                    );
                }
                Stage::Recording(id) | Stage::Instruction(id) => {
                    let end = match (input.roles.end, input.external) {
                        (_, Some(end)) => end,
                        (Some(end_id), None) => EndChoice::Key(end_id),
                        (None, None) => {
                            debug!(
                                "Ignoring press for '{}': no end shortcut has its combination",
                                input.binding_id
                            );
                            return None;
                        }
                    };
                    let id = id.clone();
                    // A key-up still in its grace window belongs to this recording.
                    self.pending_release = None;
                    return Some(self.begin_processing_with(id, input.hotkey_string, end));
                }
                Stage::Processing => {}
            }
        } else if let Some(instruction_id) = self
            .instruction
            .as_ref()
            .filter(|press| press.via.as_deref() == Some(input.binding_id.as_str()))
            .map(|press| press.id.clone())
        {
            // DanyWhere: the release of the start or hold shortcut binding
            // the instruction key came as.
            return self.instruction_released(&instruction_id, Some(&input.binding_id), now);
        } else if hold_to_talk && self.recording_binding() == Some(input.binding_id.as_str()) {
            // A release that was not deferred (one is already pending for this
            // binding): resolve it immediately rather than dropping it.
            return self.finish_hold(
                input.binding_id,
                input.hotkey_string,
                now,
                input.hold_threshold,
            );
        }
        None
    }

    /// The `RELEASE_GRACE` window elapsed with no cancelling press arriving:
    /// resolve the deferred release against whatever that binding's key was
    /// holding — the live recording, or a press remembered while busy.
    fn on_grace_expired(&mut self) -> Option<Effect> {
        let pending = self.pending_release.take()?;
        match &self.stage {
            Stage::Recording(id) | Stage::Instruction(id) if *id == pending.binding_id => self
                .finish_hold(
                    pending.binding_id,
                    pending.hotkey_string,
                    pending.released_at,
                    pending.hold_threshold,
                ),
            Stage::Processing => {
                self.finish_pending_hold(&pending);
                None
            }
            _ => None,
        }
    }

    /// A press remembered while the pipeline was busy has been released for
    /// real, still before the drain. A completed hold has nothing left to
    /// start, and neither has a hold shortcut's short press, which would be
    /// discarded; an "Auto" tap queues a locked session so the drain starts
    /// it — the same rule as [`CoordinatorState::finish_hold`].
    fn finish_pending_hold(&mut self, release: &PendingRelease) {
        let Some(pending) = self
            .pending_press
            .as_mut()
            .filter(|p| p.binding_id == release.binding_id)
        else {
            return;
        };
        let held = release
            .released_at
            .saturating_duration_since(pending.pressed_at);
        if pending.press.mode == PressMode::Auto && held < release.hold_threshold {
            debug!(
                "Tap ({held:?}) for '{}' while busy: will start locked on when the pipeline drains",
                release.binding_id
            );
            pending.locked = true;
        } else {
            debug!(
                "Forgetting remembered press for '{}': released after {held:?} while busy",
                release.binding_id
            );
            self.pending_press = None;
        }
    }

    /// The key that started the current recording has been released for real.
    /// A hold at least `threshold` long ends recording with the hold
    /// shortcut's values; anything shorter was a tap, which locks the
    /// session on until the next press ("Auto"), or discards it (a hold
    /// shortcut alone).
    fn finish_hold(
        &mut self,
        binding_id: String,
        hotkey_string: String,
        released_at: Instant,
        threshold: Duration,
    ) -> Option<Effect> {
        let Some(hold) = &mut self.hold else {
            // No hold bookkeeping means we cannot tell a tap from a hold;
            // stopping is the safe reading (it is what push-to-talk always did).
            let end = Press::tap().held_end(&binding_id);
            return Some(self.begin_processing_with(binding_id, hotkey_string, end));
        };
        let held = released_at.saturating_duration_since(hold.pressed_at);
        if held >= threshold {
            let end = hold.press.held_end(&binding_id);
            return Some(self.begin_processing_with(binding_id, hotkey_string, end));
        }
        match hold.press.mode {
            PressMode::Auto => {
                debug!(
                    "Tap ({held:?}) for '{binding_id}': recording locked on until the next press"
                );
                hold.locked = true;
                None
            }
            PressMode::Hold => {
                debug!("Short press ({held:?}) of hold shortcut '{binding_id}': discarded");
                Some(self.discard(binding_id))
            }
            PressMode::Tap => None,
        }
    }

    fn on_cancel(&mut self, recording_was_active: bool) {
        self.pending_release = None;
        // An explicit cancel abandons any remembered start too — the user
        // asked for silence, not a deferred recording.
        self.pending_press = None;
        // Don't reset during processing — wait for the pipeline to finish.
        if !matches!(self.stage, Stage::Processing)
            && (recording_was_active || self.recording_binding().is_some())
        {
            self.stage = Stage::Idle;
            self.hold = None;
            // DanyWhere: the instruction goes with it.
            self.instruction = None;
        }
    }

    /// DanyWhere: the "Insert Last Dictation" service ends the recording, so
    /// that it can insert the text once it's ready. Stops a running recording
    /// however it was started; never starts one and leaves a press remembered
    /// during processing alone.
    #[cfg(test)]
    fn on_finish(&mut self) -> Option<Effect> {
        self.on_finish_as(None)
    }

    /// [`CoordinatorState::on_finish`] for an "end dictation" service: the
    /// recording ends with `end` (`start_keys::service_end`), or without
    /// one as a press of its own key would (`start_keys::end_as_press`).
    fn on_finish_as(&mut self, end: Option<EndChoice>) -> Option<Effect> {
        let binding_id = self.recording_binding()?.to_string();
        // A key-up still in its grace window belongs to this recording.
        self.pending_release = None;
        let end = end
            .or_else(|| crate::start_keys::end_as_press(&binding_id))
            .unwrap_or_else(|| EndChoice::Start(binding_id.clone()));
        Some(self.begin_processing_with(binding_id, "insert-service".to_string(), end))
    }

    /// DanyWhere: an "end dictation" service was called while nothing was
    /// recording (e.g. macOS started DanyWhere to answer it): start a locked
    /// recording for `binding_id`, so the shortcut's next press ends it. Only
    /// from idle; never stops or replaces anything.
    fn on_start_locked(&mut self, binding_id: String, now: Instant) -> Option<Effect> {
        if self.stage != Stage::Idle {
            return None;
        }
        Some(self.begin_recording(
            binding_id,
            "insert-service".to_string(),
            now,
            true,
            Press::tap(),
        ))
    }

    /// DanyWhere: an end shortcut (`end_keys`) was pressed: it ends the
    /// recording however it was started, held or locked, with that end.
    /// Nothing while idle or transcribing.
    fn on_end_key(&mut self, end_id: &str) -> Option<Effect> {
        let binding_id = self.recording_binding()?.to_string();
        // A key-up still in its grace window belongs to this recording.
        self.pending_release = None;
        Some(self.begin_processing_with(
            binding_id,
            "end-key".to_string(),
            EndChoice::Key(end_id.to_string()),
        ))
    }

    fn on_processing_finished(&mut self) -> Option<Effect> {
        self.stage = Stage::Idle;
        self.hold = None;
        let pending = self.pending_press.take()?;
        debug!(
            "Pipeline drained; starting remembered press for '{}'",
            pending.binding_id
        );
        Some(self.begin_recording(
            pending.binding_id,
            pending.hotkey_string,
            pending.pressed_at,
            pending.locked,
            pending.press,
        ))
    }

    /// Reconcile the optimistic `Stage::Recording` after the executor reports
    /// whether recording actually began (microphone access can be denied).
    fn on_start_result(&mut self, binding_id: &str, started: bool) {
        if !started && matches!(&self.stage, Stage::Recording(id) if id == binding_id) {
            self.stage = Stage::Idle;
            self.hold = None;
        }
    }

    /// Optimistic transition to `Recording`; rolled back via
    /// [`CoordinatorState::on_start_result`] if the effect fails to start
    /// recording for real.
    fn begin_recording(
        &mut self,
        binding_id: String,
        hotkey_string: String,
        pressed_at: Instant,
        locked: bool,
        press: Press,
    ) -> Effect {
        self.stage = Stage::Recording(binding_id.clone());
        self.hold = Some(Hold {
            pressed_at,
            locked,
            press,
        });
        Effect::Start {
            binding_id,
            hotkey_string,
        }
    }

    fn begin_processing_with(
        &mut self,
        binding_id: String,
        hotkey_string: String,
        end: EndChoice,
    ) -> Effect {
        self.stage = Stage::Processing;
        self.hold = None;
        self.instruction = None;
        Effect::Stop {
            binding_id,
            hotkey_string,
            end,
        }
    }

    /// DanyWhere: the recording of `binding_id` is dropped; nothing follows
    /// it, so the machine is idle at once.
    fn discard(&mut self, binding_id: String) -> Effect {
        self.stage = Stage::Idle;
        self.hold = None;
        self.pending_release = None;
        self.instruction = None;
        Effect::Discard { binding_id }
    }
}

/// Serialises all transcription lifecycle events through a single thread
/// to eliminate race conditions between keyboard shortcuts, signals, and
/// the async transcribe-paste pipeline. The thread is a thin shell: it
/// transports commands to the pure [`CoordinatorState`] and executes the
/// returned [`Effect`]s.
pub struct TranscriptionCoordinator {
    tx: Sender<Command>,
}

impl TranscriptionCoordinator {
    pub fn new(app: AppHandle) -> Self {
        Self::with_runner(AppEffects(app))
    }

    /// DanyWhere: the coordinator thread with `runner` as its executor: the
    /// app's is [`AppEffects`], a test's records what it was asked to do.
    pub(crate) fn with_runner(mut runner: impl EffectRunner) -> Self {
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut state = CoordinatorState::new();
                // DanyWhere: whether a locked recording runs, as last reported
                // to `dictation_key_inserts`, which releases the dictation
                // shortcuts meanwhile.
                let mut reported_locked = false;

                loop {
                    let cmd = if let Some(deadline) = state.grace_deadline() {
                        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                            Ok(cmd) => cmd,
                            Err(mpsc::RecvTimeoutError::Timeout) => {
                                let was_locked = state.is_recording_locked();
                                if let Some(effect) = state.on_grace_expired() {
                                    run_effect(&mut runner, &mut state, effect);
                                }
                                if !was_locked && state.is_recording_locked() {
                                    runner.phase(RecordingPhase::Locked);
                                }
                                report_recording_locked(&mut runner, &state, &mut reported_locked);
                                continue;
                            }
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                    } else {
                        match rx.recv() {
                            Ok(cmd) => cmd,
                            Err(_) => break,
                        }
                    };

                    let was_locked = state.is_recording_locked();
                    match cmd {
                        Command::Input(mut input) => {
                            // Hold durations are measured edge-to-edge via the
                            // sender's stamp, so time spent blocked in a prior
                            // effect (first-activation mic init) cannot turn a
                            // tap into a hold (#2089).
                            let now = input.received_at;
                            // DanyWhere: what the pressed combination is in
                            // the profile that applies now.
                            if input.is_pressed && input.external.is_none() {
                                input.roles = runner.roles(&input.binding_id);
                            }
                            if let Some(effect) = state.on_input(input, now) {
                                run_effect(&mut runner, &mut state, effect);
                            }
                        }
                        Command::Cancel {
                            recording_was_active,
                        } => {
                            // A cancel while transcribing ends the take too.
                            let busy =
                                recording_was_active || matches!(state.stage, Stage::Processing);
                            // DanyWhere: a recording dropped before its
                            // stop; a stopped one is the dictation's.
                            let dropped = state.recording_binding().is_some();
                            state.on_cancel(recording_was_active);
                            if busy {
                                runner.phase(RecordingPhase::Cancelled);
                            }
                            if dropped {
                                runner.recording_cancelled();
                            }
                        }
                        Command::ProcessingFinished => {
                            if let Some(effect) = state.on_processing_finished() {
                                run_effect(&mut runner, &mut state, effect);
                            }
                        }
                        Command::Finish { end } => {
                            if let Some(effect) = state.on_finish_as(end) {
                                run_effect(&mut runner, &mut state, effect);
                            }
                        }
                        Command::StartLocked { binding_id } => {
                            match state.on_start_locked(binding_id, Instant::now()) {
                                Some(effect) => run_effect(&mut runner, &mut state, effect),
                                None => debug!("Not starting a recording: one runs already"),
                            }
                        }
                        Command::EndKey { end_id } => match state.on_end_key(&end_id) {
                            Some(effect) => run_effect(&mut runner, &mut state, effect),
                            None => debug!("End shortcut '{end_id}' without a recording"),
                        },
                        Command::InstructionKey {
                            instruction_id,
                            is_pressed,
                            hold_threshold,
                            received_at,
                        } => {
                            if let Some(effect) = state.on_instruction_key(
                                &instruction_id,
                                is_pressed,
                                received_at,
                                hold_threshold,
                            ) {
                                run_effect(&mut runner, &mut state, effect);
                            }
                        }
                    }
                    if !was_locked && state.is_recording_locked() {
                        runner.phase(RecordingPhase::Locked);
                    }
                    report_recording_locked(&mut runner, &state, &mut reported_locked);
                }
                debug!("Transcription coordinator exited");
            }));
            if let Err(e) = result {
                error!("Transcription coordinator panicked: {e:?}");
            }
        });

        Self { tx }
    }

    /// Send a key's edge for a start or hold shortcut (`start_keys`); the
    /// coordinator thread finds what its combination is now.
    /// `hold_threshold` decides between a tap and a hold.
    pub fn send_input(
        &self,
        binding_id: &str,
        hotkey_string: &str,
        is_pressed: bool,
        hold_threshold: Duration,
    ) {
        self.send(InputEvent {
            binding_id: binding_id.to_string(),
            hotkey_string: hotkey_string.to_string(),
            is_pressed,
            hold_threshold,
            external: None,
            received_at: Instant::now(),
            roles: Roles::default(),
        });
    }

    /// Send an external trigger (SIGUSR2, CLI flag) as `binding_id`. Always a
    /// toggle press, always exempt from debounce — see
    /// [`InputEvent::external`]; during a recording it ends it with `end`
    /// (`start_keys::remote_toggle`).
    pub fn send_external_input(&self, binding_id: &str, source: &str, end: EndChoice) {
        self.send(InputEvent {
            binding_id: binding_id.to_string(),
            hotkey_string: source.to_string(),
            is_pressed: true,
            hold_threshold: Duration::ZERO,
            external: Some(end),
            received_at: Instant::now(),
            roles: Roles::default(),
        });
    }

    fn send(&self, input: InputEvent) {
        if self.tx.send(Command::Input(input)).is_err() {
            warn!("Transcription coordinator channel closed");
        }
    }

    pub fn notify_cancel(&self, recording_was_active: bool) {
        if self
            .tx
            .send(Command::Cancel {
                recording_was_active,
            })
            .is_err()
        {
            warn!("Transcription coordinator channel closed");
        }
    }

    pub fn notify_processing_finished(&self) {
        if self.tx.send(Command::ProcessingFinished).is_err() {
            warn!("Transcription coordinator channel closed");
        }
    }

    /// DanyWhere: ends the running recording, however it was started, as the
    /// next press of its shortcut would. Does nothing without a recording.
    pub fn finish_recording(&self) {
        self.finish_recording_as(None);
    }

    /// DanyWhere: starts a locked recording for `binding_id` if nothing is
    /// recording or being transcribed (see `CoordinatorState::on_start_locked`).
    pub fn start_locked_recording(&self, binding_id: &str) {
        if self
            .tx
            .send(Command::StartLocked {
                binding_id: binding_id.to_string(),
            })
            .is_err()
        {
            warn!("Transcription coordinator channel closed");
        }
    }

    /// DanyWhere: an end shortcut (`end_keys`) was pressed: ends the running
    /// recording with it; nothing without one.
    pub fn finish_with_end_key(&self, end_id: &str) {
        if self
            .tx
            .send(Command::EndKey {
                end_id: end_id.to_string(),
            })
            .is_err()
        {
            warn!("Transcription coordinator channel closed");
        }
    }

    /// DanyWhere: an edge of instruction key `instruction_id`
    /// (`instruction_keys`, registered only during a recording);
    /// `hold_threshold` decides between a tap and a hold.
    pub fn send_instruction_key(
        &self,
        instruction_id: &str,
        is_pressed: bool,
        hold_threshold: Duration,
    ) {
        let command = Command::InstructionKey {
            instruction_id: instruction_id.to_string(),
            is_pressed,
            hold_threshold,
            received_at: Instant::now(),
        };
        if self.tx.send(command).is_err() {
            warn!("Transcription coordinator channel closed");
        }
    }

    /// DanyWhere: [`TranscriptionCoordinator::finish_recording`] for an "end
    /// dictation" service: ends the recording with `end`
    /// (`start_keys::service_end`), or as a press of its own key without one
    /// (see `CoordinatorState::on_finish_as`).
    pub fn finish_recording_as(&self, end: Option<EndChoice>) {
        if self.tx.send(Command::Finish { end }).is_err() {
            warn!("Transcription coordinator channel closed");
        }
    }
}

/// DanyWhere: everything the coordinator thread does to the app, so tests
/// can run the real thread with a fake ([`AppEffects`] in the app).
pub(crate) trait EffectRunner: Send + 'static {
    /// Start recording for `binding_id`; whether it began.
    fn start(&mut self, binding_id: &str, hotkey: &str) -> bool;
    /// Stop the recording of `binding_id`, ending it with `end`.
    fn stop(&mut self, binding_id: &str, hotkey: &str, end: &EndChoice);
    /// Drop the recording of `binding_id`, as a cancel would, without a
    /// transcription (a hold shortcut alone released before the threshold).
    fn discard(&mut self, binding_id: &str);
    /// Tell the windows that follow along (onboarding's Try it).
    fn phase(&mut self, phase: RecordingPhase);
    /// A locked recording began or ended (`dictation_key_inserts`).
    fn recording_locked_changed(&mut self, locked: bool);
    /// What the combination registered as `binding_id` is in the profile
    /// that applies now (`start_keys::roles_now`).
    fn roles(&mut self, binding_id: &str) -> Roles;
    /// A running recording was cancelled (Escape) or discarded: no stop
    /// follows, so its dictation ends here, and with it its own hold on
    /// DanyWhere's input source, if its start took one
    /// (`delivery::take_recording_hold`). A stopped recording's dictation
    /// ends its hold itself (`dictation_flow`).
    fn recording_cancelled(&mut self) {}
    /// The recording of `binding_id` goes on to a spoken instruction with
    /// instruction key `instruction_id`; whether it was cut (STUB).
    fn begin_instruction(&mut self, _binding_id: &str, _instruction_id: &str) -> bool {
        false
    }
}

/// DanyWhere: the app as the coordinator's executor.
struct AppEffects(AppHandle);

impl EffectRunner for AppEffects {
    fn start(&mut self, binding_id: &str, hotkey: &str) -> bool {
        start(&self.0, binding_id, hotkey)
    }

    fn stop(&mut self, binding_id: &str, hotkey: &str, end: &EndChoice) {
        crate::actions::stop_transcription(&self.0, binding_id, hotkey, end);
    }

    fn discard(&mut self, binding_id: &str) {
        debug!("Discarding the recording of '{binding_id}'");
        crate::utils::discard_current_recording(&self.0);
    }

    fn phase(&mut self, phase: RecordingPhase) {
        emit_recording_phase(&self.0, phase);
    }

    fn recording_locked_changed(&mut self, locked: bool) {
        crate::dictation_key_inserts::recording_locked_changed(&self.0, locked);
    }

    fn roles(&mut self, binding_id: &str) -> Roles {
        crate::start_keys::roles_now(&self.0, binding_id)
    }

    fn recording_cancelled(&mut self) {
        drop(crate::delivery::take_recording_hold());
    }

    /// DanyWhere: cuts the recording for its instruction (strand Z1,
    /// `dictation_flow::instruction`), only while the dictation's profile
    /// takes instruction keys (not "never", a provider with a model, not on
    /// Linux): a press that came through just before a change cuts nothing.
    fn begin_instruction(&mut self, binding_id: &str, instruction_id: &str) -> bool {
        let settings = crate::app_profiles::dictation_settings(&self.0);
        if !crate::instruction_keys::taken_here(&settings) {
            debug!(
                "Instruction key '{instruction_id}': the dictation's profile takes none; no cut"
            );
            return false;
        }
        crate::dictation_flow::instruction::begin_instruction(&self.0, binding_id, instruction_id)
    }
}

/// DanyWhere: tells `dictation_key_inserts` when a locked recording begins
/// or ends (stopped, cancelled, failed to start), so it releases the
/// dictation shortcuts only meanwhile.
fn report_recording_locked(
    runner: &mut impl EffectRunner,
    state: &CoordinatorState,
    reported: &mut bool,
) {
    let locked = state.is_recording_locked();
    if locked != *reported {
        *reported = locked;
        runner.recording_locked_changed(locked);
    }
}

fn run_effect(runner: &mut impl EffectRunner, state: &mut CoordinatorState, effect: Effect) {
    match effect {
        Effect::Start {
            binding_id,
            hotkey_string,
        } => {
            let started = runner.start(&binding_id, &hotkey_string);
            state.on_start_result(&binding_id, started);
            if started {
                runner.phase(RecordingPhase::Started);
            }
        }
        Effect::Stop {
            binding_id,
            hotkey_string,
            end,
        } => {
            runner.stop(&binding_id, &hotkey_string, &end);
            runner.phase(RecordingPhase::Stopped);
        }
        Effect::Discard { binding_id } => {
            runner.discard(&binding_id);
            runner.phase(RecordingPhase::Cancelled);
            runner.recording_cancelled();
        }
        Effect::BeginInstruction {
            binding_id,
            instruction_id,
        } => {
            let began = runner.begin_instruction(&binding_id, &instruction_id);
            state.on_instruction_result(began);
        }
    }
}

/// Execute a start effect; returns whether recording actually began, so the
/// state machine can roll back its optimistic transition on failure.
fn start(app: &AppHandle, binding_id: &str, _hotkey_string: &str) -> bool {
    // DanyWhere: decided and done through ports (`dictation_start`), so
    // tests run it without the app.
    crate::dictation_start::app_ports::start(app, binding_id);
    let recording = app
        .try_state::<Arc<AudioRecordingManager>>()
        .is_some_and(|a| a.is_recording());
    if !recording {
        debug!("Start for '{binding_id}' did not begin recording; staying idle");
        // DanyWhere: no stop or cancel follows; a hold its start may have
        // taken on DanyWhere's input source ends now.
        drop(crate::delivery::take_recording_hold());
    }
    recording
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three activation modes Handy had, as the tests below were
    /// written: since 2026-11-26 the lists the pressed combination is in
    /// (`start_keys::Roles`) say how a press drives a recording.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ShortcutActivation {
        /// Only start shortcuts have the combination.
        Toggle,
        /// Only hold shortcuts have it.
        PushToTalk,
        /// Start and hold shortcuts have it.
        HoldOrToggle,
    }

    impl ShortcutActivation {
        fn mode(self) -> PressMode {
            match self {
                Self::Toggle => PressMode::Tap,
                Self::PushToTalk => PressMode::Hold,
                Self::HoldOrToggle => PressMode::Auto,
            }
        }
    }

    /// The roles of `binding_id`'s combination in `mode`, with end shortcut
    /// `end`; its start and hold shortcuts have the binding's id.
    fn roles_in(
        binding_id: &str,
        mode: ShortcutActivation,
        end: Option<String>,
    ) -> crate::start_keys::Roles {
        crate::start_keys::Roles {
            start: (mode != ShortcutActivation::PushToTalk).then(|| binding_id.to_string()),
            hold: (mode != ShortcutActivation::Toggle).then(|| binding_id.to_string()),
            end,
            instruction: None,
        }
    }

    /// A key's edge of `binding_id` in `mode`.
    fn key_input(
        binding_id: &str,
        mode: ShortcutActivation,
        is_pressed: bool,
        hold_threshold: Duration,
        end: Option<String>,
    ) -> InputEvent {
        InputEvent {
            binding_id: binding_id.to_string(),
            hotkey_string: binding_id.to_string(),
            is_pressed,
            hold_threshold,
            external: None,
            received_at: Instant::now(),
            roles: roles_in(binding_id, mode, end),
        }
    }

    /// Remote control's toggle as `binding_id`, ending as a press of it
    /// would.
    fn remote_input(binding_id: &str) -> InputEvent {
        InputEvent {
            binding_id: binding_id.to_string(),
            hotkey_string: binding_id.to_string(),
            is_pressed: true,
            hold_threshold: Duration::ZERO,
            external: Some(EndChoice::Start(binding_id.to_string())),
            received_at: Instant::now(),
            roles: crate::start_keys::Roles::default(),
        }
    }

    #[test]
    fn push_to_talk_release_while_recording_defers_release() {
        assert_eq!(
            classify_ptt_event(None, false, true, "transcribe", Some("transcribe")),
            PttAction::DeferRelease
        );
    }

    #[test]
    fn push_to_talk_press_matching_pending_release_cancels_release() {
        assert_eq!(
            classify_ptt_event(
                Some("transcribe"),
                true,
                true,
                "transcribe",
                Some("transcribe")
            ),
            PttAction::CancelRelease
        );
    }

    #[test]
    fn toggle_mode_press_and_release_pass_through() {
        assert_eq!(
            classify_ptt_event(
                Some("transcribe"),
                true,
                false,
                "transcribe",
                Some("transcribe")
            ),
            PttAction::Passthrough
        );
        assert_eq!(
            classify_ptt_event(None, false, false, "transcribe", Some("transcribe")),
            PttAction::Passthrough
        );
    }

    #[test]
    fn press_for_different_binding_than_pending_release_passes_through() {
        assert_eq!(
            classify_ptt_event(
                Some("transcribe"),
                true,
                true,
                "transcribe_with_post_process",
                Some("transcribe")
            ),
            PttAction::Passthrough
        );
    }

    #[test]
    fn press_matching_pending_release_cancels_without_recording_state() {
        assert_eq!(
            classify_ptt_event(Some("transcribe"), true, true, "transcribe", None),
            PttAction::CancelRelease
        );
    }

    // ---------------------------------------------------------------------
    // Busy-pipeline input classification.
    //
    // Toggle-style triggers (SIGUSR2, CLI flags, pedals that signal on both
    // edges) flip state on every edge. Dropping a press that arrives while
    // the previous pipeline is still processing desyncs the parity: the next
    // edge then starts a recording no one will stop, leaving the overlay
    // waiting for input with the button long released.
    // ---------------------------------------------------------------------

    #[test]
    fn toggle_press_during_processing_remembers_start() {
        assert_eq!(
            classify_busy_input(true, ShortcutActivation::Toggle.mode(), None),
            BusyAction::Remember
        );
    }

    #[test]
    fn second_toggle_press_during_processing_forgets_press() {
        assert_eq!(
            classify_busy_input(
                true,
                ShortcutActivation::Toggle.mode(),
                Some(Remembered::Locked)
            ),
            BusyAction::Forget
        );
    }

    #[test]
    fn toggle_release_during_processing_is_ignored() {
        assert_eq!(
            classify_busy_input(false, ShortcutActivation::Toggle.mode(), None),
            BusyAction::Ignore
        );
        assert_eq!(
            classify_busy_input(
                false,
                ShortcutActivation::Toggle.mode(),
                Some(Remembered::Locked)
            ),
            BusyAction::Ignore
        );
    }

    #[test]
    fn hold_modes_classify_busy_inputs_by_pending_state() {
        let cases = [
            (true, None, BusyAction::Remember),
            (true, Some(Remembered::Held), BusyAction::Ignore),
            (true, Some(Remembered::Locked), BusyAction::Forget),
            (false, None, BusyAction::Ignore),
            (false, Some(Remembered::Held), BusyAction::Ignore),
            (false, Some(Remembered::Locked), BusyAction::Ignore),
        ];

        for mode in [
            ShortcutActivation::PushToTalk,
            ShortcutActivation::HoldOrToggle,
        ] {
            for (is_pressed, remembered, expected) in cases {
                assert_eq!(
                    classify_busy_input(is_pressed, mode.mode(), remembered),
                    expected
                );
            }
        }
    }

    /// Toggle parity across a busy window: an odd number of presses remembers
    /// one start, each further press flips the remembered press off/on again.
    #[test]
    fn toggle_presses_alternate_remember_and_forget_while_busy() {
        let mut remembered = None;
        for expected in [
            BusyAction::Remember,
            BusyAction::Forget,
            BusyAction::Remember,
        ] {
            let action = classify_busy_input(true, ShortcutActivation::Toggle.mode(), remembered);
            assert_eq!(action, expected);
            remembered = (action == BusyAction::Remember).then_some(Remembered::Locked);
        }
        assert!(remembered.is_some());
    }

    // ---------------------------------------------------------------------
    // Sequence-level regression coverage for issue #1539.
    //
    // Under X11 key auto-repeat, holding a push-to-talk key does not emit one
    // long press. It emits the initial press followed by a stream of
    // synthesized release/press pairs, then a single genuine release on key-up.
    // Before the fix, every synthesized release passed straight through and
    // stopped recording, so holding the key "rapidly toggled" recording on and
    // off. The fix defers each release for a short grace window and cancels it
    // when the matching auto-repeat press arrives.
    //
    // The unit tests above assert the classifiers in isolation. The harness
    // below drives the real `CoordinatorState` through whole event sequences
    // — the same `on_input` / `on_grace_expired` handlers the coordinator
    // thread runs — so a burst can be exercised deterministically without a
    // Tauri AppHandle or real timers, and the tests can never drift from the
    // production transitions.
    // ---------------------------------------------------------------------

    const BINDING: &str = "transcribe";

    /// The end with a start shortcut's combination out of the box: every
    /// start shortcut has one (`end_keys::builtin`).
    fn factory_end(binding_id: &str) -> Option<String> {
        Some(
            match binding_id {
                "transcribe_with_post_process" => "post_process",
                _ => "plain",
            }
            .to_string(),
        )
    }

    #[derive(Clone, Copy)]
    enum Ev {
        /// A key-down event (real initial press or a synthesized auto-repeat press).
        Press,
        /// A key-up event (synthesized auto-repeat release or the genuine key-up).
        Release,
        /// The `RELEASE_GRACE` window elapsed with no cancelling press arriving.
        Grace,
    }

    struct DriveResult {
        starts: u32,
        stops: u32,
        stage: Stage,
    }

    /// A hold shortcut alone, every release ending it (no threshold).
    fn ptt_input(is_pressed: bool) -> InputEvent {
        key_input(
            BINDING,
            ShortcutActivation::PushToTalk,
            is_pressed,
            Duration::ZERO,
            factory_end(BINDING),
        )
    }

    /// Feeds an event sequence to a real [`CoordinatorState`] the way the
    /// coordinator thread would; effects are counted instead of executed.
    fn drive(events: &[Ev]) -> DriveResult {
        let mut state = CoordinatorState::new();
        let mut clock = Instant::now();
        let mut starts = 0u32;
        let mut stops = 0u32;

        for ev in events {
            // Auto-repeat events arrive a few ms apart, well inside DEBOUNCE.
            clock += Duration::from_millis(5);

            let effect = match ev {
                Ev::Grace => state.on_grace_expired(),
                Ev::Press | Ev::Release => {
                    state.on_input(ptt_input(matches!(ev, Ev::Press)), clock)
                }
            };
            match effect {
                Some(Effect::Start { .. }) => starts += 1,
                Some(Effect::Stop { .. }) => stops += 1,
                Some(Effect::Discard { .. } | Effect::BeginInstruction { .. }) | None => {}
            }
        }

        DriveResult {
            starts,
            stops,
            stage: state.stage,
        }
    }

    /// Initial press plus several synthesized release/press pairs, as X11 emits
    /// while a push-to-talk key is held down.
    fn autorepeat_burst() -> Vec<Ev> {
        let mut events = vec![Ev::Press];
        for _ in 0..6 {
            events.push(Ev::Release);
            events.push(Ev::Press);
        }
        events
    }

    /// Regression for #1539: a burst of X11 auto-repeat release/press pairs must
    /// not stop recording. Before the fix the first synthesized release stopped
    /// recording immediately (stops == 1, stage left Recording), which produced
    /// the rapid on/off toggling. With the fix the releases are coalesced and
    /// recording stays continuously active for the whole burst.
    #[test]
    fn x11_autorepeat_burst_does_not_toggle_recording() {
        let result = drive(&autorepeat_burst());
        assert_eq!(result.starts, 1, "recording should start exactly once");
        assert_eq!(
            result.stops, 0,
            "synthesized auto-repeat releases must not stop recording mid-burst"
        );
        assert_eq!(
            result.stage,
            Stage::Recording(BINDING.to_string()),
            "recording must remain active across the entire auto-repeat burst"
        );
    }

    /// Complements the burst test: once the key is genuinely released and the
    /// grace window elapses with no re-press, recording stops exactly once. This
    /// proves the debounce only coalesces synthesized releases and does not wedge
    /// the coordinator or swallow the real key-up.
    #[test]
    fn genuine_release_after_grace_stops_recording_once() {
        let mut events = autorepeat_burst();
        events.push(Ev::Release); // genuine key-up
        events.push(Ev::Grace); // grace window elapses, no cancelling press
        let result = drive(&events);
        assert_eq!(result.starts, 1, "recording should start exactly once");
        assert_eq!(
            result.stops, 1,
            "a genuine release should stop recording exactly once"
        );
        assert_eq!(result.stage, Stage::Processing);
    }

    // ---------------------------------------------------------------------
    // Sequence-level coverage of the busy-pipeline and cancel paths, driven
    // through the real machine.
    // ---------------------------------------------------------------------

    /// PTT press while the pipeline is busy is remembered and starts recording
    /// once the pipeline drains.
    #[test]
    fn press_during_processing_starts_after_drain() {
        let mut state = CoordinatorState::new();
        let now = Instant::now();

        let effect = state.on_input(ptt_input(true), now);
        assert!(matches!(effect, Some(Effect::Start { .. })));

        let effect = state.on_input(ptt_input(false), now + Duration::from_millis(100));
        assert!(effect.is_none(), "release should be deferred, not fired");

        let effect = state.on_grace_expired();
        assert!(matches!(effect, Some(Effect::Stop { .. })));

        let effect = state.on_input(ptt_input(true), now + Duration::from_millis(200));
        assert!(effect.is_none(), "busy pipeline must remember, not start");

        let effect = state.on_processing_finished();
        assert!(
            matches!(effect, Some(Effect::Start { .. })),
            "remembered press should start once the pipeline drains"
        );
    }

    /// Two toggle presses inside one busy window net to no-op: nothing starts
    /// when the pipeline drains (toggle parity).
    #[test]
    fn toggle_presses_during_processing_net_noop_after_drain() {
        let mut state = CoordinatorState::new();
        let now = Instant::now();

        let effect = state.on_input(ptt_input(true), now);
        assert!(matches!(effect, Some(Effect::Start { .. })));
        let effect = state.on_input(ptt_input(false), now + Duration::from_millis(100));
        assert!(effect.is_none());
        let effect = state.on_grace_expired();
        assert!(matches!(effect, Some(Effect::Stop { .. })));

        let toggle = |state: &mut CoordinatorState, at: Instant| {
            state.on_input(
                InputEvent {
                    received_at: at,
                    ..remote_input(BINDING)
                },
                at,
            )
        };

        let effect = toggle(&mut state, now + Duration::from_millis(200));
        assert!(effect.is_none());
        let effect = toggle(&mut state, now + Duration::from_millis(300));
        assert!(effect.is_none());

        let effect = state.on_processing_finished();
        assert!(
            effect.is_none(),
            "even number of busy toggle presses must not start recording"
        );
        assert_eq!(state.stage, Stage::Idle);
    }

    /// Cancel while processing abandons a remembered press: the pipeline drains
    /// to idle and nothing starts.
    #[test]
    fn cancel_during_processing_drops_remembered_press() {
        let mut state = CoordinatorState::new();
        let now = Instant::now();

        let effect = state.on_input(ptt_input(true), now);
        assert!(matches!(effect, Some(Effect::Start { .. })));
        let effect = state.on_input(ptt_input(false), now + Duration::from_millis(100));
        assert!(effect.is_none());
        let effect = state.on_grace_expired();
        assert!(matches!(effect, Some(Effect::Stop { .. })));

        let effect = state.on_input(ptt_input(true), now + Duration::from_millis(200));
        assert!(effect.is_none());

        state.on_cancel(false);
        assert_eq!(
            state.stage,
            Stage::Processing,
            "cancel must not reset mid-processing — the pipeline still finishes"
        );

        let effect = state.on_processing_finished();
        assert!(
            effect.is_none(),
            "cancelled session must not spawn a deferred recording"
        );
        assert_eq!(state.stage, Stage::Idle);
    }

    fn toggle_input(external: bool) -> InputEvent {
        toggle_input_for(BINDING, external)
    }

    fn toggle_input_for(binding_id: &str, external: bool) -> InputEvent {
        if external {
            return remote_input(binding_id);
        }
        key_input(
            binding_id,
            ShortcutActivation::Toggle,
            true,
            Duration::ZERO,
            factory_end(binding_id),
        )
    }

    /// Start and stop one toggle recording so the machine sits in `Processing`.
    fn drive_into_processing(state: &mut CoordinatorState, now: Instant) {
        let effect = state.on_input(toggle_input(true), now);
        assert!(matches!(effect, Some(Effect::Start { .. })));
        let effect = state.on_input(toggle_input(true), now + Duration::from_millis(100));
        assert!(matches!(effect, Some(Effect::Stop { .. })));
        assert_eq!(state.stage, Stage::Processing);
    }

    const OTHER_BINDING: &str = "transcribe_with_post_process";

    /// Only one press can be pending. Once a binding has claimed it, a toggle
    /// for a different binding is ignored (as it is while recording) instead of
    /// replacing the remembered press, so the pending binding's parity holds:
    /// two transcribe toggles still net to no-op.
    #[test]
    fn different_binding_does_not_replace_pending_press() {
        let mut state = CoordinatorState::new();
        let now = Instant::now();
        drive_into_processing(&mut state, now);

        let at = |ms| now + Duration::from_millis(ms);
        assert!(state.on_input(toggle_input(true), at(200)).is_none());
        assert!(state
            .on_input(toggle_input_for(OTHER_BINDING, true), at(300))
            .is_none());
        assert!(state.on_input(toggle_input(true), at(400)).is_none());

        let effect = state.on_processing_finished();
        assert!(
            effect.is_none(),
            "two transcribe toggles net to no-op; the ignored post-process toggle must not start"
        );
        assert_eq!(state.stage, Stage::Idle);
    }

    /// The binding that claimed the pending press is the one that starts on
    /// drain, regardless of other bindings toggled in between.
    #[test]
    fn drain_starts_the_pending_binding_not_a_later_one() {
        let mut state = CoordinatorState::new();
        let now = Instant::now();
        drive_into_processing(&mut state, now);

        let at = |ms| now + Duration::from_millis(ms);
        assert!(state.on_input(toggle_input(true), at(200)).is_none());
        assert!(state
            .on_input(toggle_input_for(OTHER_BINDING, true), at(300))
            .is_none());

        match state.on_processing_finished() {
            Some(Effect::Start { binding_id, .. }) => assert_eq!(binding_id, BINDING),
            other => panic!("expected Start for '{BINDING}', got {other:?}"),
        }
    }

    // ---------------------------------------------------------------------
    // DanyWhere: the "Insert Last Dictation" service ends a recording, so it
    // can insert the text as soon as it's ready (user, 2026-11-12).
    // ---------------------------------------------------------------------

    #[test]
    fn finish_stops_a_toggle_recording() {
        let mut state = CoordinatorState::new();
        let now = Instant::now();
        assert!(matches!(
            state.on_input(toggle_input(false), now),
            Some(Effect::Start { .. })
        ));
        assert_stop(state.on_finish(), BINDING, BINDING);
        assert_eq!(state.stage, Stage::Processing);
    }

    #[test]
    fn finish_keeps_the_post_process_action_of_the_recording() {
        let mut state = CoordinatorState::new();
        let now = Instant::now();
        assert!(matches!(
            state.on_input(toggle_input_for(OTHER_BINDING, false), now),
            Some(Effect::Start { .. })
        ));
        assert_stop(state.on_finish(), OTHER_BINDING, OTHER_BINDING);
    }

    #[test]
    fn finish_stops_a_held_push_to_talk_recording_and_drops_its_release() {
        let mut state = CoordinatorState::new();
        let now = Instant::now();
        assert!(matches!(
            state.on_input(ptt_input(true), now),
            Some(Effect::Start { .. })
        ));
        // The key comes up; its release waits out the grace window.
        assert!(state
            .on_input(ptt_input(false), now + Duration::from_millis(500))
            .is_none());
        assert!(state.grace_deadline().is_some());

        assert_stop(state.on_finish(), BINDING, BINDING);
        assert!(
            state.grace_deadline().is_none(),
            "the stopped recording's release must not be resolved again"
        );
        assert!(state.on_grace_expired().is_none());
        assert_eq!(state.stage, Stage::Processing);
    }

    #[test]
    fn finish_with_post_processing_stops_a_plain_recording_with_post_processing() {
        let mut state = CoordinatorState::new();
        let now = Instant::now();
        assert!(matches!(
            state.on_input(toggle_input(false), now),
            Some(Effect::Start { .. })
        ));
        assert_stop(
            state.on_finish_as(Some(EndChoice::Start(OTHER_BINDING.to_string()))),
            BINDING,
            OTHER_BINDING,
        );
        assert_eq!(state.stage, Stage::Processing);
    }

    #[test]
    fn finish_with_post_processing_keeps_a_post_process_recording() {
        let mut state = CoordinatorState::new();
        let now = Instant::now();
        assert!(matches!(
            state.on_input(toggle_input_for(OTHER_BINDING, false), now),
            Some(Effect::Start { .. })
        ));
        assert_stop(
            state.on_finish_as(Some(EndChoice::Start(OTHER_BINDING.to_string()))),
            OTHER_BINDING,
            OTHER_BINDING,
        );
    }

    #[test]
    fn finish_with_post_processing_while_idle_does_nothing() {
        let mut state = CoordinatorState::new();
        assert!(state
            .on_finish_as(Some(EndChoice::Start(OTHER_BINDING.to_string())))
            .is_none());
        assert_eq!(state.stage, Stage::Idle);
    }

    // DanyWhere: the context shortcut and its service are gone (2026-11-15);
    // context follows the profile's switch "Send Context" (`send_context`).
    const RETIRED_CONTEXT: &str = crate::send_context::RETIRED_BINDING;

    #[test]
    fn the_retired_context_shortcut_is_no_dictation_binding() {
        assert!(!crate::start_keys::is_dictation_binding(RETIRED_CONTEXT));
    }

    #[test]
    fn a_service_call_while_idle_starts_a_locked_recording() {
        let mut state = CoordinatorState::new();
        let now = Instant::now();
        assert!(matches!(
            state.on_start_locked(OTHER_BINDING.to_string(), now),
            Some(Effect::Start { binding_id, .. }) if binding_id == OTHER_BINDING
        ));
        assert!(
            state.is_recording_locked(),
            "the next press of the shortcut must end it"
        );
        // That next press ends it, with the binding that started it.
        assert_stop(state.on_finish(), OTHER_BINDING, OTHER_BINDING);
    }

    #[test]
    fn a_service_start_never_touches_a_running_recording() {
        let mut state = CoordinatorState::new();
        let now = Instant::now();
        assert!(matches!(
            state.on_input(toggle_input(false), now),
            Some(Effect::Start { .. })
        ));
        assert!(state
            .on_start_locked(OTHER_BINDING.to_string(), now + Duration::from_millis(200))
            .is_none());
        assert_eq!(state.stage, Stage::Recording(BINDING.to_string()));
    }

    #[test]
    fn a_service_start_waits_for_nothing_while_processing() {
        let mut state = CoordinatorState::new();
        let now = Instant::now();
        drive_into_processing(&mut state, now);
        assert!(state
            .on_start_locked(BINDING.to_string(), now + Duration::from_millis(200))
            .is_none());
        assert_eq!(state.stage, Stage::Processing);
        assert!(
            state.on_processing_finished().is_none(),
            "nothing was remembered to start later"
        );
    }

    #[test]
    fn finish_while_idle_does_nothing() {
        let mut state = CoordinatorState::new();
        assert!(state.on_finish().is_none());
        assert_eq!(state.stage, Stage::Idle);
    }

    #[test]
    fn finish_while_processing_keeps_a_remembered_press() {
        let mut state = CoordinatorState::new();
        let now = Instant::now();
        drive_into_processing(&mut state, now);
        // The user starts the next dictation while this one is transcribing.
        assert!(state
            .on_input(toggle_input(true), now + Duration::from_millis(200))
            .is_none());

        assert!(state.on_finish().is_none());
        assert_eq!(state.stage, Stage::Processing);
        assert!(
            matches!(state.on_processing_finished(), Some(Effect::Start { .. })),
            "the remembered press must still start the next recording"
        );
    }

    // ---------------------------------------------------------------------
    // DanyWhere: start and end shortcuts are apart (user, 2026-11-22). A
    // start shortcut's press during a recording ends it only when an end
    // shortcut has exactly its combination, and then as that end; releasing
    // a held hold shortcut ends with its own values. Services and remote
    // control end as the end they name (a press of their start shortcut).
    // ---------------------------------------------------------------------

    fn input_for(binding_id: &str, mode: ShortcutActivation, is_pressed: bool) -> InputEvent {
        key_input(
            binding_id,
            mode,
            is_pressed,
            Duration::from_millis(300),
            factory_end(binding_id),
        )
    }

    /// [`input_for`] where the profile has no end with the binding's
    /// combination.
    fn input_without_end(
        binding_id: &str,
        mode: ShortcutActivation,
        is_pressed: bool,
    ) -> InputEvent {
        key_input(
            binding_id,
            mode,
            is_pressed,
            Duration::from_millis(300),
            None,
        )
    }

    /// A stop of `expected_binding`'s recording as a press of start shortcut
    /// `as_start` would end it (services, remote control).
    fn assert_stop(effect: Option<Effect>, expected_binding: &str, as_start: &str) {
        assert_end(
            effect,
            expected_binding,
            EndChoice::Start(as_start.to_string()),
        );
    }

    fn key(id: &str) -> EndChoice {
        EndChoice::Key(id.to_string())
    }

    #[test]
    fn the_second_press_ends_with_the_end_of_its_combination() {
        let mode = ShortcutActivation::Toggle;
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();
        state.on_input(input_for(OTHER_BINDING, mode, true), t0);
        assert_end(
            state.on_input(input_for(OTHER_BINDING, mode, true), t0 + ms(500)),
            OTHER_BINDING,
            key("post_process"),
        );
    }

    #[test]
    fn a_second_press_without_an_end_of_its_combination_does_nothing() {
        let t0 = Instant::now();
        for mode in [ShortcutActivation::Toggle, ShortcutActivation::HoldOrToggle] {
            let mut state = CoordinatorState::new();
            state.on_input(input_without_end(BINDING, mode, true), t0);
            // A tap locks the automatic mode.
            state.on_input(input_without_end(BINDING, mode, false), t0 + ms(100));
            assert!(state.on_grace_expired().is_none());
            assert!(state
                .on_input(input_without_end(BINDING, mode, true), t0 + ms(2000))
                .is_none());
            assert_eq!(
                state.stage,
                Stage::Recording(BINDING.to_string()),
                "{mode:?}"
            );
            // An end shortcut still ends it.
            assert_end(state.on_end_key("send"), BINDING, key("send"));
        }
    }

    #[test]
    fn another_start_shortcut_ends_with_the_end_of_its_combination() {
        let mode = ShortcutActivation::Toggle;
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();
        state.on_input(input_for(BINDING, mode, true), t0);
        assert_end(
            state.on_input(input_for(OTHER_BINDING, mode, true), t0 + ms(1000)),
            BINDING,
            key("post_process"),
        );
    }

    /// The old rule, by which another start shortcut ended a recording only
    /// when its default end post-processed and the recording's didn't, is
    /// gone: it is an ordinary press of that end.
    #[test]
    fn a_start_shortcut_without_post_processing_ends_a_post_processing_recording() {
        let mode = ShortcutActivation::Toggle;
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();
        state.on_input(input_for(OTHER_BINDING, mode, true), t0);
        assert_end(
            state.on_input(input_for(BINDING, mode, true), t0 + ms(1000)),
            OTHER_BINDING,
            key("plain"),
        );
    }

    #[test]
    fn another_start_shortcut_ends_a_held_recording_too() {
        for mode in [
            ShortcutActivation::HoldOrToggle,
            ShortcutActivation::PushToTalk,
        ] {
            let mut state = CoordinatorState::new();
            let t0 = Instant::now();
            state.on_input(input_for(BINDING, mode, true), t0);
            assert_end(
                state.on_input(input_for(OTHER_BINDING, mode, true), t0 + ms(500)),
                BINDING,
                key("post_process"),
            );
            // The held key comes up afterwards: nothing more.
            assert!(state
                .on_input(input_for(BINDING, mode, false), t0 + ms(900))
                .is_none());
            assert!(state.on_grace_expired().is_none());
            assert_eq!(state.stage, Stage::Processing, "{mode:?}");
        }
    }

    #[test]
    fn another_start_shortcut_without_an_end_of_its_combination_is_ignored() {
        let mode = ShortcutActivation::Toggle;
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();
        state.on_input(input_for(BINDING, mode, true), t0);
        assert!(state
            .on_input(input_without_end(OTHER_BINDING, mode, true), t0 + ms(1000))
            .is_none());
        assert_eq!(state.stage, Stage::Recording(BINDING.to_string()));
    }

    #[test]
    fn releasing_a_held_start_shortcut_ends_with_its_hold_end() {
        let held = |binding: &str| EndChoice::Held(binding.to_string());
        // Automatic: held past the threshold.
        let mode = ShortcutActivation::HoldOrToggle;
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();
        state.on_input(input_for(OTHER_BINDING, mode, true), t0);
        assert!(state
            .on_input(input_for(OTHER_BINDING, mode, false), t0 + ms(800))
            .is_none());
        assert_end(state.on_grace_expired(), OTHER_BINDING, held(OTHER_BINDING));
        // Hold: a release from the threshold on, also without an end of its
        // combination.
        let mode = ShortcutActivation::PushToTalk;
        let mut state = CoordinatorState::new();
        state.on_input(input_without_end(BINDING, mode, true), t0);
        assert!(state
            .on_input(input_without_end(BINDING, mode, false), t0 + ms(400))
            .is_none());
        assert_end(state.on_grace_expired(), BINDING, held(BINDING));
    }

    #[test]
    fn remote_control_always_ends_so_its_toggles_stay_paired() {
        let t0 = Instant::now();
        let mut state = CoordinatorState::new();
        assert!(matches!(
            state.on_input(remote_input(BINDING), t0),
            Some(Effect::Start { .. })
        ));
        assert_stop(
            state.on_input(remote_input(BINDING), t0 + ms(5)),
            BINDING,
            BINDING,
        );
        // With the end it names, also where an end shortcut has the
        // combination (`start_keys::remote_toggle` names it).
        let mut state = CoordinatorState::new();
        state.on_input(remote_input(BINDING), t0);
        let mut with_end = remote_input(OTHER_BINDING);
        with_end.roles.end = Some("post_process".to_string());
        assert_stop(
            state.on_input(with_end, t0 + ms(500)),
            BINDING,
            OTHER_BINDING,
        );
    }

    /// External triggers fire on every edge by design (e.g. SIGUSR2 sent on
    /// both key press and release). Two edges inside the debounce window must
    /// both be honoured, or the parity desyncs and recording wedges on.
    #[test]
    fn external_edges_inside_debounce_window_are_not_dropped() {
        let mut state = CoordinatorState::new();
        let now = Instant::now();

        let effect = state.on_input(toggle_input(true), now);
        assert!(matches!(effect, Some(Effect::Start { .. })));

        let effect = state.on_input(toggle_input(true), now + Duration::from_millis(5));
        assert!(
            matches!(effect, Some(Effect::Stop { .. })),
            "second external edge inside DEBOUNCE must stop the recording"
        );
        assert_eq!(state.stage, Stage::Processing);
    }

    /// Physical keyboard presses keep the debounce: a repeat inside the window
    /// is still dropped and recording stays active.
    #[test]
    fn keyboard_press_inside_debounce_window_is_still_dropped() {
        let mut state = CoordinatorState::new();
        let now = Instant::now();

        let effect = state.on_input(toggle_input(false), now);
        assert!(matches!(effect, Some(Effect::Start { .. })));

        let effect = state.on_input(toggle_input(false), now + Duration::from_millis(5));
        assert!(
            effect.is_none(),
            "keyboard repeat inside DEBOUNCE must be debounced"
        );
        assert_eq!(state.stage, Stage::Recording(BINDING.to_string()));
    }

    /// If the start effect fails to begin recording (e.g. microphone access
    /// denied), the optimistic transition rolls back to idle.
    #[test]
    fn failed_start_rolls_back_to_idle() {
        let mut state = CoordinatorState::new();

        let effect = state.on_input(ptt_input(true), Instant::now());
        assert!(matches!(effect, Some(Effect::Start { .. })));

        state.on_start_result(BINDING, false);
        assert_eq!(state.stage, Stage::Idle);
    }

    // ---------------------------------------------------------------------
    // Hold-or-toggle (the combined mode from #147) and the two legacy modes,
    // driven through the real machine on a synthetic clock. Recording starts
    // on key-down in every mode; the tests pin how each mode ends it.
    // ---------------------------------------------------------------------

    const HOLD_THRESHOLD: Duration = Duration::from_millis(300);

    fn input(mode: ShortcutActivation, is_pressed: bool) -> InputEvent {
        key_input(
            BINDING,
            mode,
            is_pressed,
            HOLD_THRESHOLD,
            factory_end(BINDING),
        )
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Hold-or-toggle: a key held past the threshold is push-to-talk — the
    /// (deferred) release stops recording.
    #[test]
    fn hold_or_toggle_long_hold_stops_on_release() {
        let mode = ShortcutActivation::HoldOrToggle;
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();

        assert!(matches!(
            state.on_input(input(mode, true), t0),
            Some(Effect::Start { .. })
        ));
        assert!(state.on_input(input(mode, false), t0 + ms(800)).is_none());
        assert!(
            matches!(state.on_grace_expired(), Some(Effect::Stop { .. })),
            "an 800ms hold must stop when its release grace elapses"
        );
        assert_eq!(state.stage, Stage::Processing);
    }

    /// Hold-or-toggle: a tap keeps recording (locked on); the next press stops.
    #[test]
    fn hold_or_toggle_tap_locks_recording_until_next_press() {
        let mode = ShortcutActivation::HoldOrToggle;
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();

        assert!(matches!(
            state.on_input(input(mode, true), t0),
            Some(Effect::Start { .. })
        ));
        assert!(state.on_input(input(mode, false), t0 + ms(120)).is_none());
        assert!(
            state.on_grace_expired().is_none(),
            "a 120ms tap must not stop recording"
        );
        assert_eq!(state.stage, Stage::Recording(BINDING.to_string()));
        assert!(state.is_locked());

        // Seconds later the user presses again to finish.
        assert!(matches!(
            state.on_input(input(mode, true), t0 + ms(5000)),
            Some(Effect::Stop { .. })
        ));
        assert_eq!(state.stage, Stage::Processing);
        // The release of that stopping press lands in the busy window and is
        // ignored, so nothing is remembered for the drain.
        assert!(state.on_input(input(mode, false), t0 + ms(5080)).is_none());
        assert!(state.on_processing_finished().is_none());
        assert_eq!(state.stage, Stage::Idle);
    }

    /// Regression for #2089: the first activation after startup blocks the
    /// coordinator thread in `Effect::Start` while the microphone initializes
    /// (~400ms in the report). A short tap whose release is queued behind
    /// that block must still classify as a tap. The loop therefore measures
    /// the hold from the sender's `received_at` stamps (edge-to-edge), never
    /// from its own dequeue time; this drives the machine with exactly those
    /// stamps. Before the fix the release was stamped at dequeue — after the
    /// init — so a 150ms tap measured past the 300ms threshold, the
    /// just-started recording stopped on the spot, and the transcription
    /// came back empty.
    #[test]
    fn auto_mode_tap_survives_first_activation_start_latency() {
        let mode = ShortcutActivation::HoldOrToggle;
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();

        let mut press = input(mode, true);
        press.received_at = t0;
        assert!(matches!(
            state.on_input(press, t0),
            Some(Effect::Start { .. })
        ));

        // The user taps: the key really goes up 150ms in, while the
        // coordinator is still blocked initializing the microphone. The
        // release carries the edge time, not the much later dequeue time.
        let mut release = input(mode, false);
        release.received_at = t0 + ms(150);
        assert!(state.on_input(release, t0 + ms(150)).is_none());
        assert!(
            state.on_grace_expired().is_none(),
            "a 150ms tap measured edge-to-edge must not stop the recording"
        );
        assert_eq!(state.stage, Stage::Recording(BINDING.to_string()));
        assert!(state.is_locked(), "the tap must lock the session on");

        // The next press — long after the drain of that empty start — is what
        // ends the session, per tap semantics.
        let mut stop_press = input(mode, true);
        stop_press.received_at = t0 + ms(5000);
        assert!(matches!(
            state.on_input(stop_press, t0 + ms(5000)),
            Some(Effect::Stop { .. })
        ));
    }

    /// Hold-or-toggle: a locked session ignores stray releases — only a press
    /// ends it.
    #[test]
    fn hold_or_toggle_locked_session_ignores_release() {
        let mode = ShortcutActivation::HoldOrToggle;
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();

        state.on_input(input(mode, true), t0);
        state.on_input(input(mode, false), t0 + ms(100));
        assert!(state.on_grace_expired().is_none());
        assert!(state.is_locked());

        assert!(state.on_input(input(mode, false), t0 + ms(900)).is_none());
        assert!(
            state.grace_deadline().is_none(),
            "no release may be deferred once locked"
        );
        assert_eq!(state.stage, Stage::Recording(BINDING.to_string()));
    }

    /// Hold-or-toggle: while the key is genuinely held, extra presses do not
    /// stop the recording (that is the release's job).
    #[test]
    fn hold_or_toggle_press_while_held_is_ignored() {
        let mode = ShortcutActivation::HoldOrToggle;
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();

        state.on_input(input(mode, true), t0);
        assert!(state.on_input(input(mode, true), t0 + ms(400)).is_none());
        assert_eq!(state.stage, Stage::Recording(BINDING.to_string()));
        assert!(!state.is_locked());
    }

    /// Hold-or-toggle under X11 auto-repeat: the synthesized release/press
    /// pairs must not be misread as taps. The hold is measured from the
    /// original key-down to the genuine key-up.
    #[test]
    fn hold_or_toggle_autorepeat_burst_is_one_long_hold() {
        let mode = ShortcutActivation::HoldOrToggle;
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();
        let mut clock = t0;

        assert!(matches!(
            state.on_input(input(mode, true), clock),
            Some(Effect::Start { .. })
        ));
        // ~600ms of auto-repeat pairs a few ms apart.
        for _ in 0..60 {
            clock += ms(5);
            assert!(state.on_input(input(mode, false), clock).is_none());
            clock += ms(5);
            assert!(state.on_input(input(mode, true), clock).is_none());
            assert!(
                state.grace_deadline().is_none(),
                "auto-repeat press must cancel the deferred release"
            );
        }
        assert!(!state.is_locked(), "no tap may be classified mid-burst");

        clock += ms(5);
        assert!(state.on_input(input(mode, false), clock).is_none());
        assert!(
            matches!(state.on_grace_expired(), Some(Effect::Stop { .. })),
            "the genuine release after a ~600ms hold must stop recording"
        );
    }

    /// Hold-or-toggle: a press remembered during the busy window is measured
    /// from the real key-down, so a hold that straddles the drain still counts
    /// as a hold when it is released shortly after recording actually starts.
    #[test]
    fn hold_or_toggle_remembered_press_measures_hold_from_real_key_down() {
        let mode = ShortcutActivation::HoldOrToggle;
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();

        // Previous session: hold, release, stop → Processing.
        state.on_input(input(mode, true), t0);
        state.on_input(input(mode, false), t0 + ms(800));
        assert!(matches!(
            state.on_grace_expired(),
            Some(Effect::Stop { .. })
        ));

        // Pressed again while busy; still held when the pipeline drains 700ms later.
        assert!(state.on_input(input(mode, true), t0 + ms(1000)).is_none());
        assert!(matches!(
            state.on_processing_finished(),
            Some(Effect::Start { .. })
        ));
        // Released 100ms after recording began — but 800ms after key-down.
        assert!(state.on_input(input(mode, false), t0 + ms(1800)).is_none());
        assert!(
            matches!(state.on_grace_expired(), Some(Effect::Stop { .. })),
            "held 800ms overall: must stop, not lock"
        );
    }

    /// Toggle: releases never stop, the next press does. (Toggle is the
    /// combined machine with the session locked from the start.)
    #[test]
    fn toggle_mode_ignores_release_and_stops_on_next_press() {
        let mode = ShortcutActivation::Toggle;
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();

        assert!(matches!(
            state.on_input(input(mode, true), t0),
            Some(Effect::Start { .. })
        ));
        assert!(state.is_locked());
        assert!(state.on_input(input(mode, false), t0 + ms(100)).is_none());
        assert!(
            state.grace_deadline().is_none(),
            "toggle never defers releases"
        );
        assert!(state.on_input(input(mode, false), t0 + ms(3000)).is_none());
        assert!(matches!(
            state.on_input(input(mode, true), t0 + ms(4000)),
            Some(Effect::Stop { .. })
        ));
    }

    /// Push-to-talk (a hold shortcut alone): a press shorter than the
    /// threshold discards the recording, as a cancel would (2026-11-26,
    /// question 3 a); there is no tap-to-lock in this mode.
    #[test]
    fn push_to_talk_short_press_discards_the_recording() {
        let mode = ShortcutActivation::PushToTalk;
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();

        assert!(matches!(
            state.on_input(input(mode, true), t0),
            Some(Effect::Start { .. })
        ));
        assert!(state.on_input(input(mode, false), t0 + ms(40)).is_none());
        assert!(matches!(
            state.on_grace_expired(),
            Some(Effect::Discard { .. })
        ));
        assert_eq!(state.stage, Stage::Idle);
    }

    /// Cancel (Escape) during a locked hold-or-toggle session resets cleanly so
    /// the next press starts a fresh recording rather than stopping a dead one.
    #[test]
    fn hold_or_toggle_cancel_clears_locked_session() {
        let mode = ShortcutActivation::HoldOrToggle;
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();

        state.on_input(input(mode, true), t0);
        state.on_input(input(mode, false), t0 + ms(100));
        assert!(state.on_grace_expired().is_none());
        assert!(state.is_locked());

        state.on_cancel(true);
        assert_eq!(state.stage, Stage::Idle);
        assert!(!state.is_locked());
        assert!(matches!(
            state.on_input(input(mode, true), t0 + ms(2000)),
            Some(Effect::Start { .. })
        ));
    }

    /// A press of the held key's own combination while it is held is a
    /// repeat, whatever lists its combination is in by now (the profile may
    /// have changed meanwhile); its release ends the recording.
    #[test]
    fn a_press_of_the_held_key_is_a_repeat_whatever_its_lists_are_now() {
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();

        state.on_input(input(ShortcutActivation::HoldOrToggle, true), t0);
        assert!(!state.is_locked());
        assert!(state
            .on_input(input(ShortcutActivation::Toggle, true), t0 + ms(2000))
            .is_none());
        assert_eq!(state.stage, Stage::Recording(BINDING.to_string()));
    }

    // Hold-vs-tap classification while the previous transcription is busy.
    fn hold_or_toggle_into_processing(state: &mut CoordinatorState, t0: Instant) {
        let mode = ShortcutActivation::HoldOrToggle;
        assert!(matches!(
            state.on_input(input(mode, true), t0),
            Some(Effect::Start { .. })
        ));
        assert!(state.on_input(input(mode, false), t0 + ms(800)).is_none());
        assert!(matches!(
            state.on_grace_expired(),
            Some(Effect::Stop { .. })
        ));
        assert_eq!(state.stage, Stage::Processing);
    }

    #[test]
    fn hold_or_toggle_tap_during_processing_queues_locked_start() {
        let mode = ShortcutActivation::HoldOrToggle;
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();
        hold_or_toggle_into_processing(&mut state, t0);

        assert!(state.on_input(input(mode, true), t0 + ms(1000)).is_none());
        assert!(state.on_input(input(mode, false), t0 + ms(1100)).is_none());
        assert!(state.on_grace_expired().is_none());
        assert!(state.is_locked(), "a busy tap should queue a locked start");
        assert!(matches!(
            state.on_processing_finished(),
            Some(Effect::Start { .. })
        ));
        assert!(state.is_locked());
    }

    #[test]
    fn hold_or_toggle_completed_hold_during_processing_nets_noop() {
        let mode = ShortcutActivation::HoldOrToggle;
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();
        hold_or_toggle_into_processing(&mut state, t0);

        assert!(state.on_input(input(mode, true), t0 + ms(1000)).is_none());
        assert!(state.on_input(input(mode, false), t0 + ms(1600)).is_none());
        assert!(state.on_grace_expired().is_none());
        assert!(!state.is_locked());

        assert!(
            state.on_processing_finished().is_none(),
            "a 600ms hold that ended before the drain has nothing left to start"
        );
        assert_eq!(state.stage, Stage::Idle);
    }

    #[test]
    fn hold_or_toggle_two_taps_during_processing_net_noop() {
        let mode = ShortcutActivation::HoldOrToggle;
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();
        hold_or_toggle_into_processing(&mut state, t0);

        assert!(state.on_input(input(mode, true), t0 + ms(1000)).is_none());
        assert!(state.on_input(input(mode, false), t0 + ms(1100)).is_none());
        assert!(state.on_grace_expired().is_none());
        assert!(state.is_locked());

        assert!(state.on_input(input(mode, true), t0 + ms(1500)).is_none());
        assert!(
            !state.is_locked(),
            "the second tap's press forgets the queued tap"
        );
        assert!(state.on_input(input(mode, false), t0 + ms(1600)).is_none());
        assert!(state.grace_deadline().is_none());

        assert!(state.on_processing_finished().is_none());
        assert_eq!(state.stage, Stage::Idle);
    }

    #[test]
    fn ptt_tap_inside_busy_window_nets_noop() {
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();
        hold_or_toggle_into_processing(&mut state, t0);

        assert!(state.on_input(ptt_input(true), t0 + ms(1000)).is_none());
        assert!(state.on_input(ptt_input(false), t0 + ms(1040)).is_none());
        assert!(state.on_grace_expired().is_none());
        assert!(state.on_processing_finished().is_none());
    }

    /// The pipeline drains inside the 50ms grace of a busy tap: recording
    /// starts first (unlocked, from the real key-down), and the grace then
    /// resolves against the live recording, locking it as the tap it was.
    #[test]
    fn hold_or_toggle_drain_inside_busy_release_grace_still_classifies_tap() {
        let mode = ShortcutActivation::HoldOrToggle;
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();
        hold_or_toggle_into_processing(&mut state, t0);

        assert!(state.on_input(input(mode, true), t0 + ms(1000)).is_none());
        assert!(state.on_input(input(mode, false), t0 + ms(1100)).is_none());
        assert!(matches!(
            state.on_processing_finished(),
            Some(Effect::Start { .. })
        ));
        assert!(!state.is_locked());

        assert!(state.on_grace_expired().is_none());
        assert!(state.is_locked(), "the deferred 100ms release is a tap");
    }

    /// X11 auto-repeat while busy, key still held at the drain: recording
    /// starts measured from the first press, not from the last synthesized
    /// press before the drain. Released 400ms after the real key-down but
    /// only ~100ms after the drain — a hold, so it must stop rather than lock.
    #[test]
    fn hold_or_toggle_autorepeat_burst_straddling_drain_measures_from_first_press() {
        let mode = ShortcutActivation::HoldOrToggle;
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();
        hold_or_toggle_into_processing(&mut state, t0);

        let mut clock = t0 + ms(1000);
        assert!(state.on_input(input(mode, true), clock).is_none());
        for _ in 0..30 {
            clock += ms(5);
            assert!(state.on_input(input(mode, false), clock).is_none());
            clock += ms(5);
            assert!(state.on_input(input(mode, true), clock).is_none());
            assert!(state.grace_deadline().is_none());
        }

        // Drain at ~t0 + 1300ms with the key still down.
        assert!(matches!(
            state.on_processing_finished(),
            Some(Effect::Start { .. })
        ));
        assert!(!state.is_locked());

        for _ in 0..10 {
            clock += ms(5);
            assert!(state.on_input(input(mode, false), clock).is_none());
            clock += ms(5);
            assert!(state.on_input(input(mode, true), clock).is_none());
        }
        assert_eq!(clock, t0 + ms(1400));
        assert!(state.on_input(input(mode, false), clock).is_none());
        assert!(
            matches!(state.on_grace_expired(), Some(Effect::Stop { .. })),
            "held 400ms since the real key-down: must stop, not lock"
        );
        assert_eq!(state.stage, Stage::Processing);
    }

    // ---------------------------------------------------------------------
    // DanyWhere: end shortcuts (`end_keys`, idea 3). An end shortcut ends
    // the recording with its own end, however it was started.
    // ---------------------------------------------------------------------

    fn assert_end(effect: Option<Effect>, expected_binding: &str, expected_end: EndChoice) {
        match effect {
            Some(Effect::Stop {
                binding_id, end, ..
            }) => {
                assert_eq!(
                    binding_id, expected_binding,
                    "stop must target the recording binding"
                );
                assert_eq!(end, expected_end, "wrong end");
            }
            other => panic!("expected Stop, got {other:?}"),
        }
    }

    #[test]
    fn an_end_shortcut_ends_a_locked_recording_with_its_end() {
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();
        let mode = ShortcutActivation::HoldOrToggle;
        state.on_input(input_for(BINDING, mode, true), t0);
        state.on_input(
            input_for(BINDING, mode, false),
            t0 + Duration::from_millis(100),
        );
        assert!(
            state.on_grace_expired().is_none(),
            "a tap locks the recording"
        );
        assert_end(
            state.on_end_key("send"),
            BINDING,
            EndChoice::Key("send".to_string()),
        );
        assert_eq!(state.stage, Stage::Processing);
    }

    #[test]
    fn an_end_shortcut_ends_a_held_recording_and_its_release_is_dropped() {
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();
        state.on_input(ptt_input(true), t0);
        assert!(state
            .on_input(ptt_input(false), t0 + Duration::from_millis(400))
            .is_none());
        assert_end(
            state.on_end_key("line"),
            BINDING,
            EndChoice::Key("line".to_string()),
        );
        assert!(state.grace_deadline().is_none());
        assert!(state.on_grace_expired().is_none());
    }

    #[test]
    fn an_end_shortcut_does_nothing_without_a_recording() {
        let mut state = CoordinatorState::new();
        assert!(state.on_end_key("send").is_none());
        assert_eq!(state.stage, Stage::Idle);
        drive_into_processing(&mut state, Instant::now());
        assert!(state.on_end_key("send").is_none());
        assert_eq!(state.stage, Stage::Processing);
    }

    // ---- The coordinator thread with a fake executor (DanyWhere) ----------
    //
    // These run the real thread of `TranscriptionCoordinator`, its timers
    // included, and record what it has the executor do.

    use std::collections::{HashMap, VecDeque};

    /// What the coordinator thread had the executor do.
    #[derive(Debug, PartialEq)]
    enum Did {
        Start(String),
        Stop {
            binding: String,
            end: EndChoice,
        },
        Phase(RecordingPhase),
        LockedChanged(bool),
        Discard(String),
        BeginInstruction {
            binding: String,
            instruction: String,
        },
    }

    /// An executor that reports what it was asked to do.
    struct FakeEffects {
        did: mpsc::Sender<Did>,
        /// The end with each binding's combination.
        keys_ends: HashMap<String, String>,
        /// The lists the pressed combination is in now.
        mode: Arc<std::sync::Mutex<ShortcutActivation>>,
        /// Whether each start begins recording; `true` once these run out.
        start_results: VecDeque<bool>,
        /// Whether a recording is cut for its instruction.
        cuts: bool,
    }

    impl EffectRunner for FakeEffects {
        fn start(&mut self, binding_id: &str, _hotkey: &str) -> bool {
            let _ = self.did.send(Did::Start(binding_id.to_string()));
            self.start_results.pop_front().unwrap_or(true)
        }

        fn stop(&mut self, binding_id: &str, _hotkey: &str, end: &EndChoice) {
            let _ = self.did.send(Did::Stop {
                binding: binding_id.to_string(),
                end: end.clone(),
            });
        }

        fn phase(&mut self, phase: RecordingPhase) {
            let _ = self.did.send(Did::Phase(phase));
        }

        fn recording_locked_changed(&mut self, locked: bool) {
            let _ = self.did.send(Did::LockedChanged(locked));
        }

        fn discard(&mut self, binding_id: &str) {
            let _ = self.did.send(Did::Discard(binding_id.to_string()));
        }

        fn begin_instruction(&mut self, binding_id: &str, instruction_id: &str) -> bool {
            let _ = self.did.send(Did::BeginInstruction {
                binding: binding_id.to_string(),
                instruction: instruction_id.to_string(),
            });
            self.cuts
        }

        fn roles(&mut self, binding_id: &str) -> crate::start_keys::Roles {
            let mode = *self
                .mode
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            roles_in(binding_id, mode, self.keys_ends.get(binding_id).cloned())
        }
    }

    /// Long enough for the thread to act, even on a busy machine.
    const ACTS_WITHIN: Duration = Duration::from_secs(5);
    /// How long nothing more may happen once the expected effects came.
    const QUIET_FOR: Duration = Duration::from_millis(150);
    /// Longer than `DEBOUNCE`, so the next press of a key counts.
    const AFTER_DEBOUNCE: Duration = Duration::from_millis(40);

    const TRANSCRIBE: &str = "transcribe";

    /// The real coordinator thread with a [`FakeEffects`].
    struct Running {
        coordinator: TranscriptionCoordinator,
        did: mpsc::Receiver<Did>,
        mode: Arc<std::sync::Mutex<ShortcutActivation>>,
    }

    fn running(keys_ends: &[(&str, &str)], start_results: &[bool]) -> Running {
        running_cutting(keys_ends, start_results, true)
    }

    /// [`running`] whose recordings are cut for an instruction, or not
    /// (`cuts`).
    fn running_cutting(keys_ends: &[(&str, &str)], start_results: &[bool], cuts: bool) -> Running {
        let (tx, did) = mpsc::channel();
        let mode = Arc::new(std::sync::Mutex::new(ShortcutActivation::Toggle));
        let coordinator = TranscriptionCoordinator::with_runner(FakeEffects {
            did: tx,
            keys_ends: keys_ends
                .iter()
                .map(|(binding, end)| (binding.to_string(), end.to_string()))
                .collect(),
            mode: Arc::clone(&mode),
            start_results: start_results.iter().copied().collect(),
            cuts,
        });
        Running {
            coordinator,
            did,
            mode,
        }
    }

    impl Running {
        /// The next `n` things done, and whatever else follows within
        /// `QUIET_FOR`.
        fn next(&self, n: usize) -> Vec<Did> {
            let mut got = Vec::new();
            while got.len() < n {
                match self.did.recv_timeout(ACTS_WITHIN) {
                    Ok(did) => got.push(did),
                    Err(_) => return got,
                }
            }
            while let Ok(extra) = self.did.recv_timeout(QUIET_FOR) {
                got.push(extra);
            }
            got
        }

        /// A key's edge, its combination in the lists of `mode`.
        fn key(&self, mode: ShortcutActivation, is_pressed: bool, threshold: Duration) {
            *self
                .mode
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = mode;
            self.coordinator
                .send_input(TRANSCRIBE, "ctrl+space", is_pressed, threshold);
        }

        fn toggle_press(&self) {
            self.key(ShortcutActivation::Toggle, true, Duration::ZERO);
        }
    }

    fn locked_start() -> Vec<Did> {
        vec![
            Did::Start(TRANSCRIBE.into()),
            Did::Phase(RecordingPhase::Started),
            Did::Phase(RecordingPhase::Locked),
            Did::LockedChanged(true),
        ]
    }

    fn stopped(end: EndChoice, was_locked: bool) -> Vec<Did> {
        let mut did = vec![
            Did::Stop {
                binding: TRANSCRIBE.into(),
                end,
            },
            Did::Phase(RecordingPhase::Stopped),
        ];
        if was_locked {
            did.push(Did::LockedChanged(false));
        }
        did
    }

    // DanyWhere: the instruction key through the thread (variant b).

    fn began_instruction() -> Did {
        Did::BeginInstruction {
            binding: TRANSCRIBE.into(),
            instruction: "tab".into(),
        }
    }

    #[test]
    fn the_thread_goes_on_to_the_instruction_and_a_held_release_sends_it() {
        let run = running(&[], &[]);
        run.toggle_press();
        assert_eq!(run.next(4), locked_start());
        run.coordinator
            .send_instruction_key("tab", true, Duration::ZERO);
        assert_eq!(run.next(1), vec![began_instruction()]);
        run.coordinator
            .send_instruction_key("tab", false, Duration::ZERO);
        assert_eq!(
            run.next(3),
            stopped(EndChoice::Instruction("tab".into()), true)
        );
    }

    #[test]
    fn the_thread_sends_a_tapped_instruction_with_an_end_shortcut() {
        let run = running(&[], &[]);
        run.toggle_press();
        assert_eq!(run.next(4), locked_start());
        // A threshold no release reaches, however slow the machine.
        let threshold = Duration::from_secs(2);
        run.coordinator.send_instruction_key("tab", true, threshold);
        run.coordinator
            .send_instruction_key("tab", false, threshold);
        run.coordinator.send_instruction_key("tab", true, threshold);
        assert_eq!(run.next(1), vec![began_instruction()], "once only");
        run.coordinator.finish_with_end_key("send");
        assert_eq!(run.next(3), stopped(EndChoice::Key("send".into()), true));
    }

    #[test]
    fn the_thread_records_the_text_on_when_the_recording_was_not_cut() {
        let run = running_cutting(&[], &[], false);
        run.toggle_press();
        assert_eq!(run.next(4), locked_start());
        run.coordinator
            .send_instruction_key("tab", true, Duration::ZERO);
        assert_eq!(run.next(1), vec![began_instruction()]);
        // Its release sends nothing: the recording records its text.
        run.coordinator
            .send_instruction_key("tab", false, Duration::ZERO);
        assert!(run.next(0).is_empty());
        run.coordinator.finish_with_end_key("send");
        assert_eq!(run.next(3), stopped(EndChoice::Key("send".into()), true));
    }

    #[test]
    fn the_thread_ignores_an_instruction_key_without_a_recording() {
        let run = running(&[], &[]);
        run.coordinator
            .send_instruction_key("tab", true, Duration::ZERO);
        run.coordinator
            .send_instruction_key("tab", false, Duration::ZERO);
        assert!(run.next(0).is_empty());
    }

    #[test]
    fn the_thread_starts_a_locked_recording_on_a_toggle_press() {
        let run = running(&[], &[]);
        run.toggle_press();
        assert_eq!(run.next(4), locked_start());
    }

    #[test]
    fn the_thread_ends_on_the_second_press_with_the_end_of_its_combination() {
        let run = running(&[(TRANSCRIBE, "send")], &[]);
        run.toggle_press();
        assert_eq!(run.next(4), locked_start());
        std::thread::sleep(AFTER_DEBOUNCE);
        run.toggle_press();
        assert_eq!(run.next(3), stopped(EndChoice::Key("send".into()), true));
    }

    #[test]
    fn the_thread_ignores_a_second_press_without_an_end_and_an_end_shortcut_ends() {
        let run = running(&[], &[]);
        run.toggle_press();
        assert_eq!(run.next(4), locked_start());
        std::thread::sleep(AFTER_DEBOUNCE);
        run.toggle_press();
        assert_eq!(run.next(0), vec![], "the recording goes on");
        run.coordinator.finish_with_end_key("send");
        assert_eq!(run.next(3), stopped(EndChoice::Key("send".into()), true));
    }

    #[test]
    fn the_thread_ends_push_to_talk_with_its_hold_end_on_release() {
        let run = running(&[(TRANSCRIBE, "send")], &[]);
        run.key(ShortcutActivation::PushToTalk, true, Duration::ZERO);
        assert_eq!(
            run.next(2),
            vec![
                Did::Start(TRANSCRIBE.into()),
                Did::Phase(RecordingPhase::Started)
            ]
        );
        run.key(ShortcutActivation::PushToTalk, false, Duration::ZERO);
        assert_eq!(
            run.next(2),
            stopped(EndChoice::Held(TRANSCRIBE.into()), false)
        );
    }

    #[test]
    fn the_thread_ends_a_held_hold_or_toggle_key_on_release() {
        let run = running(&[], &[]);
        let threshold = Duration::from_millis(100);
        run.key(ShortcutActivation::HoldOrToggle, true, threshold);
        assert_eq!(
            run.next(2),
            vec![
                Did::Start(TRANSCRIBE.into()),
                Did::Phase(RecordingPhase::Started)
            ]
        );
        std::thread::sleep(Duration::from_millis(150));
        run.key(ShortcutActivation::HoldOrToggle, false, threshold);
        assert_eq!(
            run.next(2),
            stopped(EndChoice::Held(TRANSCRIBE.into()), false)
        );
    }

    #[test]
    fn the_thread_locks_a_tapped_hold_or_toggle_recording() {
        let run = running(&[], &[]);
        // A threshold no tap reaches, however slow the machine.
        let threshold = Duration::from_secs(2);
        run.key(ShortcutActivation::HoldOrToggle, true, threshold);
        run.key(ShortcutActivation::HoldOrToggle, false, threshold);
        assert_eq!(run.next(4), locked_start(), "locked once the grace passed");
        run.coordinator.finish_with_end_key("line");
        assert_eq!(run.next(3), stopped(EndChoice::Key("line".into()), true));
    }

    #[test]
    fn the_thread_reports_a_cancel_and_starts_afresh() {
        let run = running(&[], &[]);
        run.toggle_press();
        assert_eq!(run.next(4), locked_start());
        run.coordinator.notify_cancel(true);
        assert_eq!(
            run.next(2),
            vec![
                Did::Phase(RecordingPhase::Cancelled),
                Did::LockedChanged(false)
            ]
        );
        std::thread::sleep(AFTER_DEBOUNCE);
        run.toggle_press();
        assert_eq!(run.next(4), locked_start());
    }

    #[test]
    fn the_thread_starts_a_press_made_while_processing_once_it_finished() {
        let run = running(&[], &[]);
        run.toggle_press();
        assert_eq!(run.next(4), locked_start());
        run.coordinator.finish_with_end_key("send");
        assert_eq!(run.next(3), stopped(EndChoice::Key("send".into()), true));
        std::thread::sleep(AFTER_DEBOUNCE);
        run.toggle_press();
        assert_eq!(run.next(0), vec![], "remembered while processing");
        run.coordinator.notify_processing_finished();
        assert_eq!(run.next(4), locked_start());
    }

    #[test]
    fn the_thread_stays_idle_when_a_start_does_not_begin_recording() {
        let run = running(&[(TRANSCRIBE, "send")], &[false]);
        run.toggle_press();
        assert_eq!(run.next(1), vec![Did::Start(TRANSCRIBE.into())]);
        std::thread::sleep(AFTER_DEBOUNCE);
        // Idle again: the next press starts, it doesn't end anything.
        run.toggle_press();
        assert_eq!(run.next(4), locked_start());
    }

    #[test]
    fn the_thread_always_ends_on_a_second_remote_control_toggle() {
        let run = running(&[], &[]);
        run.coordinator
            .send_external_input(TRANSCRIBE, "cli", EndChoice::Start(TRANSCRIBE.into()));
        assert_eq!(run.next(4), locked_start());
        run.coordinator
            .send_external_input(TRANSCRIBE, "cli", EndChoice::Start(TRANSCRIBE.into()));
        assert_eq!(
            run.next(3),
            stopped(EndChoice::Start(TRANSCRIBE.into()), true)
        );
    }

    #[test]
    fn the_thread_runs_a_service_started_recording_to_its_end() {
        let run = running(&[], &[]);
        run.coordinator.start_locked_recording(TRANSCRIBE);
        assert_eq!(run.next(4), locked_start());
        run.coordinator.finish_recording_as(Some(EndChoice::Start(
            "transcribe_with_post_process".into(),
        )));
        assert_eq!(
            run.next(3),
            stopped(
                EndChoice::Start("transcribe_with_post_process".into()),
                true
            )
        );
    }

    // ---------------------------------------------------------------------
    // DanyWhere: start, hold and end shortcuts of one's own (user,
    // 2026-11-26). What a press does follows from the lists its combination
    // is in, resolved per press (`start_keys::Roles`): only start shortcuts
    // lock, only hold shortcuts hold (and a short press discards), both
    // decide by the threshold. During a recording a press ends it only as
    // the end shortcut with exactly its combination.
    // ---------------------------------------------------------------------

    use crate::start_keys::Roles;

    const START: &str = "start_key:start";
    const HOLD: &str = "hold_key:hold_post";
    const HOLD_DIRECT: &str = "hold_key:hold_direct";
    const THRESHOLD: Duration = Duration::from_millis(300);

    fn roles(start: Option<&str>, hold: Option<&str>, end: Option<&str>) -> Roles {
        Roles {
            start: start.map(str::to_string),
            hold: hold.map(str::to_string),
            end: end.map(str::to_string),
            instruction: None,
        }
    }

    /// A start shortcut alone.
    fn only_start() -> Roles {
        roles(Some("start"), None, None)
    }

    /// The second factory hold shortcut alone (its id from before variant
    /// b).
    fn only_hold() -> Roles {
        roles(None, Some("hold_post"), None)
    }

    /// A key-down of `binding_id`, whose combination has `roles`.
    fn press(binding_id: &str, roles: Roles) -> InputEvent {
        InputEvent {
            binding_id: binding_id.to_string(),
            hotkey_string: binding_id.to_string(),
            is_pressed: true,
            hold_threshold: THRESHOLD,
            external: None,
            received_at: Instant::now(),
            roles,
        }
    }

    /// A key-up of `binding_id`; roles are resolved for presses only.
    fn release(binding_id: &str) -> InputEvent {
        InputEvent {
            is_pressed: false,
            ..press(binding_id, Roles::default())
        }
    }

    /// Remote control toggling as `binding_id`, ending with `end`.
    fn remote(binding_id: &str, end: EndChoice) -> InputEvent {
        InputEvent {
            hold_threshold: Duration::ZERO,
            external: Some(end),
            ..press(binding_id, Roles::default())
        }
    }

    fn held(id: &str) -> EndChoice {
        EndChoice::Held(id.to_string())
    }

    fn is_start(effect: Option<Effect>) -> bool {
        matches!(effect, Some(Effect::Start { .. }))
    }

    /// A recording of [`START`] tapped and locked at `t0`.
    fn tapped_start(state: &mut CoordinatorState, t0: Instant) {
        assert!(is_start(state.on_input(press(START, only_start()), t0)));
        assert!(state.on_input(release(START), t0 + ms(100)).is_none());
        assert!(state.on_grace_expired().is_none());
        assert!(state.is_recording_locked());
    }

    #[test]
    fn a_tap_of_a_start_shortcut_alone_locks_the_recording() {
        let mut state = CoordinatorState::new();
        tapped_start(&mut state, Instant::now());
        assert_eq!(state.stage, Stage::Recording(START.to_string()));
    }

    #[test]
    fn a_start_shortcut_alone_held_long_still_locks_the_recording() {
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();
        assert!(is_start(state.on_input(press(START, only_start()), t0)));
        assert!(state.on_input(release(START), t0 + ms(800)).is_none());
        assert!(state.on_grace_expired().is_none());
        assert_eq!(state.stage, Stage::Recording(START.to_string()));
        assert!(state.is_recording_locked());
    }

    #[test]
    fn a_hold_shortcut_alone_held_ends_with_its_own_values() {
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();
        assert!(is_start(state.on_input(press(HOLD, only_hold()), t0)));
        assert!(state.on_input(release(HOLD), t0 + ms(800)).is_none());
        assert_end(state.on_grace_expired(), HOLD, held("hold_post"));
    }

    #[test]
    fn a_short_press_of_a_hold_shortcut_alone_discards_the_recording() {
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();
        assert!(is_start(state.on_input(press(HOLD, only_hold()), t0)));
        assert!(state.on_input(release(HOLD), t0 + ms(100)).is_none());
        assert_eq!(
            state.on_grace_expired(),
            Some(Effect::Discard {
                binding_id: HOLD.to_string()
            })
        );
        assert_eq!(state.stage, Stage::Idle);
        // The next press starts afresh.
        assert!(is_start(
            state.on_input(press(HOLD, only_hold()), t0 + ms(2000))
        ));
    }

    #[test]
    fn a_combination_in_both_lists_locks_when_tapped_and_holds_when_held() {
        let both = || roles(Some("start"), Some("hold_direct"), None);
        let t0 = Instant::now();
        let mut tapped = CoordinatorState::new();
        assert!(is_start(tapped.on_input(press(START, both()), t0)));
        assert!(tapped.on_input(release(START), t0 + ms(100)).is_none());
        assert!(tapped.on_grace_expired().is_none());
        assert!(tapped.is_recording_locked());
        let mut held_long = CoordinatorState::new();
        assert!(is_start(held_long.on_input(press(START, both()), t0)));
        assert!(held_long.on_input(release(START), t0 + ms(800)).is_none());
        assert_end(held_long.on_grace_expired(), START, held("hold_direct"));
    }

    #[test]
    fn a_start_shortcut_during_a_recording_ends_only_as_the_end_with_its_combination() {
        let t0 = Instant::now();
        let mut with_end = CoordinatorState::new();
        tapped_start(&mut with_end, t0);
        assert_end(
            with_end.on_input(
                press(START, roles(Some("start"), None, Some("direct_space"))),
                t0 + ms(2000),
            ),
            START,
            key("direct_space"),
        );
        let mut without = CoordinatorState::new();
        tapped_start(&mut without, t0);
        assert!(without
            .on_input(press(START, only_start()), t0 + ms(2000))
            .is_none());
        assert_eq!(without.stage, Stage::Recording(START.to_string()));
    }

    #[test]
    fn a_hold_shortcut_during_another_keys_recording_ends_only_as_its_end() {
        let t0 = Instant::now();
        let mut state = CoordinatorState::new();
        tapped_start(&mut state, t0);
        assert!(state
            .on_input(press(HOLD, only_hold()), t0 + ms(2000))
            .is_none());
        // Its release ends nothing: it didn't start this recording.
        assert!(state.on_input(release(HOLD), t0 + ms(2900)).is_none());
        assert!(state.on_grace_expired().is_none());
        assert_eq!(state.stage, Stage::Recording(START.to_string()));
        assert_end(
            state.on_input(
                press(HOLD, roles(None, Some("hold_post"), Some("post_no_space"))),
                t0 + ms(4000),
            ),
            START,
            key("post_no_space"),
        );
    }

    #[test]
    fn releasing_a_foreign_hold_shortcut_leaves_the_recording_to_its_own() {
        let t0 = Instant::now();
        let mut state = CoordinatorState::new();
        let direct = || roles(None, Some("hold_direct"), None);
        assert!(is_start(state.on_input(press(HOLD_DIRECT, direct()), t0)));
        assert!(state
            .on_input(press(HOLD, only_hold()), t0 + ms(300))
            .is_none());
        assert!(state.on_input(release(HOLD), t0 + ms(600)).is_none());
        assert!(state.on_grace_expired().is_none());
        assert_eq!(state.stage, Stage::Recording(HOLD_DIRECT.to_string()));
        assert!(state
            .on_input(release(HOLD_DIRECT), t0 + ms(1200))
            .is_none());
        assert_end(state.on_grace_expired(), HOLD_DIRECT, held("hold_direct"));
    }

    #[test]
    fn remote_control_toggles_in_pairs_with_the_end_it_names() {
        let t0 = Instant::now();
        let start = EndChoice::Start("start".to_string());
        let mut plain = CoordinatorState::new();
        assert!(is_start(plain.on_input(remote(START, start.clone()), t0)));
        assert!(plain.is_recording_locked());
        assert_end(
            plain.on_input(remote(START, start.clone()), t0 + ms(5)),
            START,
            start,
        );
        let mut post = CoordinatorState::new();
        assert!(is_start(post.on_input(remote(HOLD, held("hold_post")), t0)));
        assert!(post.is_recording_locked(), "a remote hold toggles, locked");
        assert_end(
            post.on_input(remote(HOLD, held("hold_post")), t0 + ms(5)),
            HOLD,
            held("hold_post"),
        );
    }

    /// A recording of [`START`] ended by an end shortcut, so the machine
    /// sits in `Processing`.
    fn processing(state: &mut CoordinatorState, t0: Instant) {
        tapped_start(state, t0);
        assert!(state.on_end_key("send").is_some());
        assert_eq!(state.stage, Stage::Processing);
    }

    #[test]
    fn a_hold_shortcuts_press_while_processing_is_forgotten_when_released_before_the_drain() {
        let t0 = Instant::now();
        for released_after in [ms(100), ms(800)] {
            let mut state = CoordinatorState::new();
            processing(&mut state, t0);
            assert!(state
                .on_input(press(HOLD, only_hold()), t0 + ms(1000))
                .is_none());
            assert!(state
                .on_input(release(HOLD), t0 + ms(1000) + released_after)
                .is_none());
            assert!(state.on_grace_expired().is_none());
            assert!(
                state.on_processing_finished().is_none(),
                "released after {released_after:?}"
            );
            assert_eq!(state.stage, Stage::Idle);
        }
    }

    #[test]
    fn a_start_shortcuts_press_while_processing_starts_locked_once_it_drained() {
        let t0 = Instant::now();
        let mut state = CoordinatorState::new();
        processing(&mut state, t0);
        assert!(state
            .on_input(press(START, only_start()), t0 + ms(1000))
            .is_none());
        assert!(state.on_input(release(START), t0 + ms(1800)).is_none());
        assert!(state.on_grace_expired().is_none());
        assert!(is_start(state.on_processing_finished()));
        assert!(state.is_recording_locked());
    }

    #[test]
    fn a_hold_shortcut_still_held_at_the_drain_ends_with_its_values() {
        let t0 = Instant::now();
        let mut state = CoordinatorState::new();
        processing(&mut state, t0);
        assert!(state
            .on_input(press(HOLD, only_hold()), t0 + ms(1000))
            .is_none());
        assert!(is_start(state.on_processing_finished()));
        // 500 ms since the real key-down: a hold.
        assert!(state.on_input(release(HOLD), t0 + ms(1500)).is_none());
        assert_end(state.on_grace_expired(), HOLD, held("hold_post"));
    }

    #[test]
    fn the_thread_discards_a_short_press_of_a_hold_shortcut_alone() {
        let run = running(&[], &[]);
        // A threshold no press reaches, however slow the machine.
        let threshold = Duration::from_secs(2);
        run.key(ShortcutActivation::PushToTalk, true, threshold);
        run.key(ShortcutActivation::PushToTalk, false, threshold);
        assert_eq!(
            run.next(4),
            vec![
                Did::Start(TRANSCRIBE.into()),
                Did::Phase(RecordingPhase::Started),
                Did::Discard(TRANSCRIBE.into()),
                Did::Phase(RecordingPhase::Cancelled),
            ]
        );
        // Idle again: the next press starts afresh.
        std::thread::sleep(AFTER_DEBOUNCE);
        run.toggle_press();
        assert_eq!(run.next(4), locked_start());
    }

    #[test]
    fn a_press_whose_combination_is_in_no_list_any_more_does_nothing() {
        let mut state = CoordinatorState::new();
        assert!(state
            .on_input(press(START, Roles::default()), Instant::now())
            .is_none());
        assert_eq!(state.stage, Stage::Idle);
    }

    // ---------------------------------------------------------------------
    // DanyWhere: the spoken instruction (variant b, user, 2026-11-26). An
    // instruction key pressed during a recording switches it to the stage
    // "Recording: instruction"; tapped it locks and an end shortcut sends
    // it, held its release sends it with its own values. A second press
    // does nothing, Escape drops everything, remote control ends it.
    // ---------------------------------------------------------------------

    const TAB: &str = "tab";

    fn instruction(instruction_id: &str) -> EndChoice {
        EndChoice::Instruction(instruction_id.to_string())
    }

    /// The instruction key's edge at `at`.
    fn instruction_key(
        state: &mut CoordinatorState,
        is_pressed: bool,
        at: Instant,
    ) -> Option<Effect> {
        state.on_instruction_key(TAB, is_pressed, at, THRESHOLD)
    }

    fn is_begin_instruction(effect: Option<Effect>, binding: &str) -> bool {
        effect
            == Some(Effect::BeginInstruction {
                binding_id: binding.to_string(),
                instruction_id: TAB.to_string(),
            })
    }

    /// A tapped recording of [`START`] that went on to the instruction at
    /// `t0 + 1 s`, the cut made.
    fn instructing(state: &mut CoordinatorState, t0: Instant) {
        tapped_start(state, t0);
        assert!(is_begin_instruction(
            instruction_key(state, true, t0 + ms(1000)),
            START
        ));
        state.on_instruction_result(true);
        assert_eq!(state.stage, Stage::Instruction(START.to_string()));
    }

    #[test]
    fn an_instruction_key_during_a_recording_goes_on_to_the_instruction() {
        let mut state = CoordinatorState::new();
        instructing(&mut state, Instant::now());
        assert!(
            state.is_recording_locked(),
            "the tapped recording stays locked"
        );
    }

    #[test]
    fn an_instruction_key_does_nothing_without_a_recording() {
        let t0 = Instant::now();
        let mut idle = CoordinatorState::new();
        assert!(instruction_key(&mut idle, true, t0).is_none());
        assert!(instruction_key(&mut idle, false, t0 + ms(800)).is_none());
        assert_eq!(idle.stage, Stage::Idle);
        let mut busy = CoordinatorState::new();
        processing(&mut busy, t0);
        assert!(instruction_key(&mut busy, true, t0 + ms(2000)).is_none());
        assert!(instruction_key(&mut busy, false, t0 + ms(2800)).is_none());
        assert_eq!(busy.stage, Stage::Processing);
    }

    #[test]
    fn a_second_press_during_the_instruction_does_nothing() {
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();
        instructing(&mut state, t0);
        assert!(instruction_key(&mut state, true, t0 + ms(1050)).is_none());
        assert_eq!(state.stage, Stage::Instruction(START.to_string()));
    }

    #[test]
    fn a_tapped_instruction_key_locks_and_an_end_shortcut_sends() {
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();
        instructing(&mut state, t0);
        assert!(instruction_key(&mut state, false, t0 + ms(1100)).is_none());
        assert_eq!(state.stage, Stage::Instruction(START.to_string()));
        // Pressed and released again much later: still locked.
        assert!(instruction_key(&mut state, true, t0 + ms(3000)).is_none());
        assert!(instruction_key(&mut state, false, t0 + ms(4000)).is_none());
        assert_end(state.on_end_key("send"), START, key("send"));
        assert_eq!(state.stage, Stage::Processing);
    }

    #[test]
    fn releasing_a_held_instruction_key_sends_with_its_own_values() {
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();
        instructing(&mut state, t0);
        assert_end(
            instruction_key(&mut state, false, t0 + ms(1000) + THRESHOLD),
            START,
            instruction(TAB),
        );
        assert_eq!(state.stage, Stage::Processing);
    }

    #[test]
    fn a_start_or_hold_key_during_the_instruction_ends_only_as_the_end_of_its_combination() {
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();
        instructing(&mut state, t0);
        assert!(state
            .on_input(press(HOLD, only_hold()), t0 + ms(2000))
            .is_none());
        assert_eq!(state.stage, Stage::Instruction(START.to_string()));
        assert_end(
            state.on_input(
                press(START, roles(Some("start"), None, Some("space"))),
                t0 + ms(3000),
            ),
            START,
            key("space"),
        );
    }

    #[test]
    fn releasing_the_hold_key_that_holds_the_recording_sends_as_its_end() {
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();
        assert!(is_start(state.on_input(press(HOLD, only_hold()), t0)));
        assert!(is_begin_instruction(
            instruction_key(&mut state, true, t0 + ms(500)),
            HOLD
        ));
        state.on_instruction_result(true);
        assert!(state.on_input(release(HOLD), t0 + ms(900)).is_none());
        assert_end(state.on_grace_expired(), HOLD, held("hold_post"));
        assert_eq!(state.stage, Stage::Processing);
        // The instruction key's release afterwards belongs to nothing.
        assert!(instruction_key(&mut state, false, t0 + ms(1500)).is_none());
    }

    #[test]
    fn a_short_release_of_the_starting_key_during_the_instruction_follows_its_rule() {
        let t0 = Instant::now();
        // Start and hold shortcut: a short press locks, the instruction goes on.
        let mut auto = CoordinatorState::new();
        let both = roles(Some("start"), Some("hold_post"), None);
        assert!(is_start(auto.on_input(press(START, both), t0)));
        assert!(is_begin_instruction(
            instruction_key(&mut auto, true, t0 + ms(50)),
            START
        ));
        auto.on_instruction_result(true);
        assert!(auto.on_input(release(START), t0 + ms(100)).is_none());
        assert!(auto.on_grace_expired().is_none());
        assert_eq!(auto.stage, Stage::Instruction(START.to_string()));
        assert!(auto.is_recording_locked());
        // A hold shortcut alone: a short press discards everything.
        let mut hold = CoordinatorState::new();
        assert!(is_start(hold.on_input(press(HOLD, only_hold()), t0)));
        assert!(is_begin_instruction(
            instruction_key(&mut hold, true, t0 + ms(50)),
            HOLD
        ));
        hold.on_instruction_result(true);
        assert!(hold.on_input(release(HOLD), t0 + ms(100)).is_none());
        assert_eq!(
            hold.on_grace_expired(),
            Some(Effect::Discard {
                binding_id: HOLD.to_string()
            })
        );
        assert_eq!(hold.stage, Stage::Idle);
        assert!(instruction_key(&mut hold, false, t0 + ms(900)).is_none());
    }

    #[test]
    fn escape_drops_the_text_and_the_instruction() {
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();
        instructing(&mut state, t0);
        state.on_cancel(true);
        assert_eq!(state.stage, Stage::Idle);
        assert!(!state.is_recording_locked());
        assert!(instruction_key(&mut state, false, t0 + ms(2000)).is_none());
        assert!(state.on_end_key("send").is_none());
    }

    #[test]
    fn remote_control_and_the_services_end_the_instruction() {
        let t0 = Instant::now();
        let start = EndChoice::Start("start".to_string());
        let mut remote_state = CoordinatorState::new();
        instructing(&mut remote_state, t0);
        assert_end(
            remote_state.on_input(remote(START, start.clone()), t0 + ms(2000)),
            START,
            start,
        );
        let mut service = CoordinatorState::new();
        instructing(&mut service, t0);
        assert_stop(service.on_finish_as(None), START, "start");
    }

    #[test]
    fn a_cut_that_was_not_made_leaves_the_recording_as_it_was() {
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();
        tapped_start(&mut state, t0);
        assert!(is_begin_instruction(
            instruction_key(&mut state, true, t0 + ms(1000)),
            START
        ));
        state.on_instruction_result(false);
        assert_eq!(state.stage, Stage::Recording(START.to_string()));
        assert!(instruction_key(&mut state, false, t0 + ms(2000)).is_none());
        assert_end(state.on_end_key("send"), START, key("send"));
    }

    #[test]
    fn an_instruction_key_on_a_start_shortcuts_combination_goes_on_to_the_instruction() {
        // ⇧⌃Space is a hold shortcut and the instruction key: idle it holds,
        // during the recording of ⌃Space it instructs.
        let mut state = CoordinatorState::new();
        let t0 = Instant::now();
        tapped_start(&mut state, t0);
        let instructs = Roles {
            instruction: Some(TAB.to_string()),
            ..only_hold()
        };
        assert!(is_begin_instruction(
            state.on_input(press(HOLD, instructs.clone()), t0 + ms(1000)),
            START
        ));
        state.on_instruction_result(true);
        assert_eq!(state.stage, Stage::Instruction(START.to_string()));
        // Its own release, held, sends with the instruction key's values.
        assert_end(
            state.on_input(release(HOLD), t0 + ms(1000) + THRESHOLD),
            START,
            instruction(TAB),
        );
        // Idle, the same combination starts a recording as before.
        let mut idle = CoordinatorState::new();
        assert!(is_start(idle.on_input(press(HOLD, instructs), t0)));
        assert_eq!(idle.stage, Stage::Recording(HOLD.to_string()));
    }
}
