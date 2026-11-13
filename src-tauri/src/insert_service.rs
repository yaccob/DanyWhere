//! "Insert Last Dictation": a macOS service that inserts the last dictation
//! without the Accessibility permission and without the general clipboard.
//!
//! The app the user types in calls the service through its shortcut, asks
//! DanyWhere for text, and inserts the reply itself. The text travels on a
//! pasteboard that macOS creates for this one request, so neither input
//! simulation nor the user's clipboard is involved. The service is declared
//! in `Info.plist` (`NSServices`); its shortcut lives in the system's services
//! settings (domain `pbs`, key `NSServicesStatus`), where System Settings →
//! Keyboard → Keyboard Shortcuts → Services writes it too.
//!
//! Called during a dictation, the service ends the recording and answers
//! once the text is ready, so one shortcut both ends and inserts a
//! dictation (user, 2026-11-12). The calling app waits meanwhile.

/// The Objective-C message named by `NSMessage` in `Info.plist`; the
/// provider's method is `insertLastDictation:userData:error:`.
pub const SERVICE_MESSAGE: &str = "insertLastDictation";

/// ⌃⌥⌘V, in the notation of `NSServicesStatus` (`@` ⌘, `~` ⌥, `^` ⌃, `$` ⇧).
pub const DEFAULT_KEY_EQUIVALENT: &str = "@~^v";

/// The services DanyWhere declares in `Info.plist`, all answered the same
/// way: during a dictation they end it and insert its text once it's ready,
/// otherwise they insert the last dictation.
///
/// The three "end dictation" services carry the dictation shortcuts (plain,
/// with post-processing, with context) as their own (user, 2026-11-12; the
/// context one since 2026-11-13): while a recording runs without
/// Accessibility, DanyWhere releases those shortcuts, so their second press
/// reaches the app being typed in and calls the service there (see
/// `dictation_key_inserts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceKind {
    /// "Insert Last Dictation", with a shortcut of its own (⌃⌥⌘V).
    InsertLastDictation,
    /// The dictation shortcut's service.
    EndDictation,
    /// The post-process shortcut's service: ends a recording with
    /// post-processing.
    EndDictationWithPostProcess,
    /// The context shortcut's service: ends a recording with post-processing
    /// and context from earlier dictations in the same window.
    EndDictationWithContext,
}

impl ServiceKind {
    #[cfg(test)]
    pub const ALL: [ServiceKind; 4] = [
        ServiceKind::InsertLastDictation,
        ServiceKind::EndDictation,
        ServiceKind::EndDictationWithPostProcess,
        ServiceKind::EndDictationWithContext,
    ];

    /// The Objective-C message `NSMessage` names in `Info.plist`.
    pub fn message(self) -> &'static str {
        match self {
            ServiceKind::InsertLastDictation => SERVICE_MESSAGE,
            ServiceKind::EndDictation => "endDictationAndInsert",
            ServiceKind::EndDictationWithPostProcess => "endDictationWithPostProcessAndInsert",
            ServiceKind::EndDictationWithContext => "endDictationWithContextAndInsert",
        }
    }

    #[cfg(test)]
    pub fn from_message(message: &str) -> Option<ServiceKind> {
        ServiceKind::ALL
            .into_iter()
            .find(|kind| kind.message() == message)
    }

    /// The dictation shortcut whose service this is, if any. Ending a
    /// recording through the service does what a press of that shortcut
    /// would: the post-process and context services add post-processing or
    /// context.
    pub fn binding_id(self) -> Option<&'static str> {
        match self {
            ServiceKind::InsertLastDictation => None,
            ServiceKind::EndDictation => Some("transcribe"),
            ServiceKind::EndDictationWithPostProcess => Some("transcribe_with_post_process"),
            ServiceKind::EndDictationWithContext => Some(crate::dictation_context::CONTEXT_BINDING),
        }
    }
}

/// What the service hands back to the app that called it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceReply {
    /// Insert this text.
    Insert(String),
    /// A dictation is still recording or transcribing; insert nothing rather
    /// than the one before it.
    NotReady,
    /// There is no dictation to insert.
    Nothing,
}

/// Decides the service's reply. `latest` is the most recent dictation's text
/// (as "Paste Last Transcript" would deliver it).
pub fn service_reply(
    dictation_in_progress: bool,
    latest: Option<&str>,
    append_trailing_space: bool,
) -> ServiceReply {
    if dictation_in_progress {
        return ServiceReply::NotReady;
    }
    match latest {
        Some(text) if !text.trim().is_empty() => ServiceReply::Insert(if append_trailing_space {
            format!("{text} ")
        } else {
            text.to_string()
        }),
        _ => ServiceReply::Nothing,
    }
}

/// How a call of the service is answered, by the dictation's phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallPlan {
    /// Answer right away with the last dictation (or nothing).
    AnswerNow,
    /// End the recording, then answer once its text is ready.
    StopAndWait,
    /// The dictation is being transcribed: answer once its text is ready.
    Wait,
}

/// Decides how to answer a call. `waits_for_service` is whether finished
/// dictations wait for this service (no Accessibility); otherwise a call
/// during a dictation keeps inserting nothing, as before.
pub fn plan_call(waits_for_service: bool, phase: crate::tray::TrayIconState) -> CallPlan {
    use crate::tray::TrayIconState;
    match (waits_for_service, phase) {
        (true, TrayIconState::Recording) => CallPlan::StopAndWait,
        (true, TrayIconState::Transcribing) => CallPlan::Wait,
        _ => CallPlan::AnswerNow,
    }
}

/// How an "end dictation" service answers. Unlike "Insert Last Dictation" it
/// never inserts an earlier dictation (user, 2026-11-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndCallPlan {
    /// End the recording, then answer once its text is ready.
    StopAndWait,
    /// The dictation is being transcribed: answer once its text is ready.
    Wait,
    /// Nothing records: answer with nothing at once, then start a locked
    /// recording for the service's shortcut.
    StartRecording,
    /// Answer with nothing.
    Nothing,
}

/// Decides how an "end dictation" service answers a call.
pub fn plan_end_call(
    waits_for_service: bool,
    phase: crate::tray::TrayIconState,
    when_idle: crate::settings::DictationServiceWhenIdle,
) -> EndCallPlan {
    use crate::settings::DictationServiceWhenIdle;
    use crate::tray::TrayIconState;
    match (waits_for_service, phase, when_idle) {
        (true, TrayIconState::Recording, _) => EndCallPlan::StopAndWait,
        (true, TrayIconState::Transcribing, _) => EndCallPlan::Wait,
        (true, TrayIconState::Idle, DictationServiceWhenIdle::StartRecording) => {
            EndCallPlan::StartRecording
        }
        (true, TrayIconState::Idle, DictationServiceWhenIdle::DoNothing) | (false, _, _) => {
            EndCallPlan::Nothing
        }
    }
}

/// How long a call waits for the dictation it ended. The calling app waits
/// as long as `NSTimeout` in `Info.plist`, which must be longer.
pub const WAIT_FOR_DICTATION: std::time::Duration = std::time::Duration::from_secs(20);

#[derive(Debug)]
enum WaitState {
    Idle,
    Waiting,
    Delivered(String),
}

/// Hands a finished dictation's text to the service call waiting for it.
/// The dictation's pipeline delivers; the call takes the text or gives up.
#[derive(Debug)]
pub struct WaitSlot {
    state: std::sync::Mutex<WaitState>,
}

impl WaitSlot {
    pub const fn new() -> Self {
        Self {
            state: std::sync::Mutex::new(WaitState::Idle),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, WaitState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A call starts waiting. `false` when another call already waits.
    pub fn begin(&self) -> bool {
        let mut state = self.lock();
        match *state {
            WaitState::Idle => {
                *state = WaitState::Waiting;
                true
            }
            WaitState::Waiting | WaitState::Delivered(_) => false,
        }
    }

    /// The pipeline hands over a dictation's text. `true` when a call waits
    /// for it and will insert it, `false` when no call waits.
    pub fn deliver(&self, text: &str) -> bool {
        let mut state = self.lock();
        match *state {
            WaitState::Waiting => {
                *state = WaitState::Delivered(text.to_string());
                true
            }
            WaitState::Idle | WaitState::Delivered(_) => false,
        }
    }

    /// The delivered text, if it has arrived; the call keeps waiting if not.
    pub fn take(&self) -> Option<String> {
        let mut state = self.lock();
        match std::mem::replace(&mut *state, WaitState::Idle) {
            WaitState::Delivered(text) => Some(text),
            other => {
                *state = other;
                None
            }
        }
    }

    /// The call stops waiting. Returns a text delivered at the last moment,
    /// so it is inserted rather than lost.
    pub fn give_up(&self) -> Option<String> {
        match std::mem::replace(&mut *self.lock(), WaitState::Idle) {
            WaitState::Delivered(text) => Some(text),
            WaitState::Idle | WaitState::Waiting => None,
        }
    }
}

impl Default for WaitSlot {
    fn default() -> Self {
        Self::new()
    }
}

/// What a waiting call does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitStep {
    KeepWaiting,
    /// The dictation's text arrived: insert it.
    Insert(String),
    /// The dictation ended without text (empty, cancelled or failed).
    EndedWithoutText,
    /// Waited too long: insert nothing; the dictation's text waits as usual.
    GaveUp,
}

/// One round of a waiting call: `dictation_in_progress` is whether the
/// dictation still records or transcribes, `deadline_passed` whether
/// [`WAIT_FOR_DICTATION`] is over.
pub fn next_wait_step(
    slot: &WaitSlot,
    dictation_in_progress: bool,
    deadline_passed: bool,
) -> WaitStep {
    if let Some(text) = slot.take() {
        return WaitStep::Insert(text);
    }
    if dictation_in_progress && !deadline_passed {
        return WaitStep::KeepWaiting;
    }
    match slot.give_up() {
        Some(text) => WaitStep::Insert(text),
        None if !dictation_in_progress => WaitStep::EndedWithoutText,
        None => WaitStep::GaveUp,
    }
}

/// The one call that may wait for a dictation at a time.
static WAITING_CALL: WaitSlot = WaitSlot::new();

/// Hands a finished dictation's text to a service call waiting for it.
/// `true` when the call takes it: the dictation is then inserted, and the
/// waiting sound and the "Ready" notice are not needed.
pub fn deliver_to_waiting_call(text: &str) -> bool {
    WAITING_CALL.deliver(text)
}

/// When the "end dictation" services last got a shortcut, as DanyWhere keeps
/// it (`dictation_service_shortcuts_written_at`) after syncing them:
/// `wrote_shortcut` is whether this sync wrote one, `has_shortcuts` whether
/// any of them has one now. Settings from before this was kept get, once,
/// the time the services settings last changed (`services_file_modified`,
/// at most later than the real write, which errs on the safe side), or `now`.
pub fn shortcuts_written_at(
    stored: Option<u64>,
    wrote_shortcut: bool,
    has_shortcuts: bool,
    now: u64,
    services_file_modified: Option<u64>,
) -> Option<u64> {
    if wrote_shortcut {
        Some(now)
    } else if stored.is_some() || !has_shortcuts {
        stored
    } else {
        Some(services_file_modified.unwrap_or(now))
    }
}

/// The key under which `NSServicesStatus` keeps a service's settings.
pub fn status_key(bundle_id: &str, menu_title: &str, message: &str) -> String {
    format!("{bundle_id} - {menu_title} - {message}")
}

/// Shows a key equivalent from `NSServicesStatus` the way macOS menus do,
/// e.g. `@~^v` → `⌃⌥⌘V`. `None` when there is no shortcut.
pub fn display_key_equivalent(key_equivalent: &str) -> Option<String> {
    const MODIFIERS: [(char, char); 4] = [('^', '⌃'), ('~', '⌥'), ('$', '⇧'), ('@', '⌘')];
    let is_modifier = |c: char| MODIFIERS.iter().any(|(code, _)| *code == c);
    let key: String = key_equivalent
        .chars()
        .filter(|c| !is_modifier(*c))
        .collect();
    if key.is_empty() {
        return None;
    }
    let mut shown: String = MODIFIERS
        .iter()
        .filter(|(code, _)| key_equivalent.contains(*code))
        .map(|(_, symbol)| *symbol)
        .collect();
    shown.push_str(&key.to_uppercase());
    Some(shown)
}

/// Whether DanyWhere should give the service its default shortcut now: only
/// once, and never over an entry the user (or System Settings) made.
pub fn should_set_default_shortcut(already_set_once: bool, entry_exists: bool) -> bool {
    !already_set_once && !entry_exists
}

/// Why a recorded combination can't be the service's shortcut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutProblem {
    /// Only modifiers were pressed.
    NoKey,
    /// The key isn't a single letter, digit or punctuation character (e.g.
    /// Space or F5), or there is more than one.
    UnsupportedKey(String),
    /// Without ⌘ or ⌃ the combination would type a character instead.
    NeedsCommandOrControl,
    /// A character key with at most ⇧ would type the character in the app
    /// instead of reaching the service.
    TypesCharacter,
}

/// Turns a recorded combination such as `ctrl+option+command+v` into the
/// notation of `NSServicesStatus`, `@~^v`.
pub fn key_equivalent_from_binding(binding: &str) -> Result<String, ShortcutProblem> {
    let (modifiers, keys) = split_binding(binding);
    let key = match keys.as_slice() {
        [] => return Err(ShortcutProblem::NoKey),
        [key] if key.chars().count() == 1 => key.clone(),
        _ => return Err(ShortcutProblem::UnsupportedKey(keys.join("+"))),
    };
    if !modifiers.contains(&'@') && !modifiers.contains(&'^') {
        return Err(ShortcutProblem::NeedsCommandOrControl);
    }
    let mut key_equivalent: String = KEY_EQUIVALENT_ORDER
        .iter()
        .filter(|code| modifiers.contains(code))
        .collect();
    key_equivalent.push_str(&key);
    Ok(key_equivalent)
}

/// The modifier codes in the order macOS writes them (`@~^v` for ⌃⌥⌘V).
const KEY_EQUIVALENT_ORDER: [char; 4] = ['@', '~', '^', '$'];

/// Turns a dictation shortcut (e.g. `ctrl+space`) into the key equivalent
/// of its "end dictation" service. Unlike [`key_equivalent_from_binding`],
/// which guards the shortcut the user picks for "Insert Last Dictation", it
/// takes what a dictation shortcut can be: Space, and function keys (written
/// as AppKit's function-key characters, F1 = U+F704) with any modifiers. A
/// character key needs ⌘ or ⌃: with at most ⇧ it types the character, and
/// with ⌥ it types one too (⌥Space a no-break space), so the press never
/// reaches the service (⌥Space and ⌥⇧Space on the company Mac, 2026-11-13).
pub fn dictation_key_equivalent(binding: &str) -> Result<String, ShortcutProblem> {
    let (modifiers, keys) = split_binding(binding);
    let key = match keys.as_slice() {
        [] => return Err(ShortcutProblem::NoKey),
        [key] => key.clone(),
        _ => return Err(ShortcutProblem::UnsupportedKey(keys.join("+"))),
    };
    let (character, types) = if key == "space" {
        (" ".to_string(), true)
    } else if key.chars().count() == 1 {
        (key.clone(), true)
    } else if let Some(function_key) = function_key_character(&key) {
        (function_key.to_string(), false)
    } else {
        return Err(ShortcutProblem::UnsupportedKey(key));
    };
    if types && modifiers.iter().all(|code| *code == '$') {
        return Err(ShortcutProblem::TypesCharacter);
    }
    if types && modifiers.iter().all(|code| *code == '$' || *code == '~') {
        return Err(ShortcutProblem::NeedsCommandOrControl);
    }
    let mut key_equivalent: String = KEY_EQUIVALENT_ORDER
        .iter()
        .filter(|code| modifiers.contains(code))
        .collect();
    key_equivalent.push_str(&character);
    Ok(key_equivalent)
}

/// AppKit's character for a function key (`NSF1FunctionKey` = U+F704 …
/// `NSF35FunctionKey`), e.g. `f5` → U+F708.
fn function_key_character(key: &str) -> Option<char> {
    let number: u32 = key.strip_prefix('f')?.parse().ok()?;
    if !(1..=35).contains(&number) {
        return None;
    }
    char::from_u32(0xF704 + number - 1)
}

/// The key equivalents the three "end dictation" services should have: the
/// dictation shortcuts' own when `enabled` (the setting "Dictation shortcut
/// also inserts"), else none. A shortcut that can't be a key equivalent, or
/// isn't set, leaves its service without one.
pub fn dictation_service_shortcuts(
    bindings: &std::collections::HashMap<String, crate::settings::ShortcutBinding>,
    enabled: bool,
) -> Vec<(ServiceKind, Option<String>)> {
    [
        ServiceKind::EndDictation,
        ServiceKind::EndDictationWithPostProcess,
        ServiceKind::EndDictationWithContext,
    ]
    .into_iter()
    .map(|kind| {
        let key_equivalent = kind
            .binding_id()
            .filter(|_| enabled)
            .and_then(|id| bindings.get(id))
            .and_then(|binding| dictation_key_equivalent(&binding.current_binding).ok());
        (kind, key_equivalent)
    })
    .collect()
}

/// A recorded combination's modifiers as `NSServicesStatus` codes, and its
/// other keys, lowercased.
fn split_binding(binding: &str) -> (Vec<char>, Vec<String>) {
    let mut modifiers = Vec::new();
    let mut keys = Vec::new();
    for part in binding.split('+').map(|p| p.trim().to_lowercase()) {
        let code = match part.as_str() {
            "ctrl" | "control" => Some('^'),
            "option" | "alt" => Some('~'),
            "shift" => Some('$'),
            "command" | "cmd" | "meta" | "super" => Some('@'),
            _ => None,
        };
        match code {
            Some(code) if !modifiers.contains(&code) => modifiers.push(code),
            Some(_) => {}
            None if part.is_empty() => {}
            None => keys.push(part),
        }
    }
    (modifiers, keys)
}

/// The recorded-combination form of a key equivalent, e.g. `@~^v` →
/// `ctrl+option+command+v`, so the settings show it like other shortcuts.
pub fn binding_from_key_equivalent(key_equivalent: &str) -> Option<String> {
    const NAMES: [(char, &str); 4] = [
        ('^', "ctrl"),
        ('~', "option"),
        ('$', "shift"),
        ('@', "command"),
    ];
    let key: String = key_equivalent
        .chars()
        .filter(|c| !NAMES.iter().any(|(code, _)| code == c))
        .collect();
    if key.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = NAMES
        .iter()
        .filter(|(code, _)| key_equivalent.contains(*code))
        .map(|(_, name)| name.to_string())
        .collect();
    parts.push(key.to_lowercase());
    Some(parts.join("+"))
}

/// The id of a DanyWhere shortcut that already uses `binding`: its global
/// shortcut would take the keypress before the app could see it.
pub fn conflicting_binding(
    binding: &str,
    bindings: &std::collections::HashMap<String, crate::settings::ShortcutBinding>,
) -> Option<String> {
    let normalized = |binding: &str| {
        let (mut modifiers, mut keys) = split_binding(binding);
        modifiers.sort_unstable();
        keys.sort();
        (modifiers, keys)
    };
    let wanted = normalized(binding);
    let mut ids: Vec<&String> = bindings
        .iter()
        .filter(|(_, b)| normalized(&b.current_binding) == wanted)
        .map(|(id, _)| id)
        .collect();
    ids.sort();
    ids.first().map(|id| id.to_string())
}

/// The `NSServicesStatus` entry for a shortcut, as System Settings stores it
/// for an enabled service. Characters outside printable ASCII, such as
/// AppKit's function-key characters, are written as `\UXXXX` escapes, the
/// way the old-style plist syntax `defaults` reads takes them; an empty key
/// equivalent means no shortcut.
pub fn status_entry_value(key_equivalent: &str) -> String {
    let escaped: String = key_equivalent
        .chars()
        .map(|c| match c {
            '"' | '\\' => format!("\\{c}"),
            ' '..='~' => c.to_string(),
            _ => format!("\\U{:04X}", c as u32),
        })
        .collect();
    format!("{{ \"key_equivalent\" = \"{escaped}\"; }}")
}

/// Whether to flash "Not ready yet" when the service is called during a
/// dictation: only if the overlay doesn't already show the session, whose
/// pill says as much and must not be covered.
pub fn shows_not_ready_notice(overlay_shows_session: bool) -> bool {
    !overlay_shows_session
}

/// How long the "Not ready yet" notice stays.
const NOT_READY_NOTICE: std::time::Duration = std::time::Duration::from_millis(1500);

/// Answers a call of the service: the text to insert, or `None`.
fn answer_call(app: &tauri::AppHandle) -> Option<String> {
    let settings = crate::settings::get_settings(app);
    let in_progress = crate::tray::dictation_in_progress(app);
    let latest = if in_progress {
        None
    } else {
        crate::delivery::latest_transcript_text(app)
    };
    match service_reply(
        in_progress,
        latest.as_deref(),
        settings.append_trailing_space,
    ) {
        ServiceReply::Insert(text) => {
            log::info!("Insert Last Dictation: handing over the last dictation.");
            crate::overlay::hide_ready_notice(app);
            Some(text)
        }
        ServiceReply::NotReady => {
            log::info!("Insert Last Dictation: a dictation is still in progress.");
            if shows_not_ready_notice(crate::overlay::session_showing()) {
                crate::overlay::show_not_ready_notice(app, NOT_READY_NOTICE);
            }
            None
        }
        ServiceReply::Nothing => {
            log::info!("Insert Last Dictation: no dictation to insert.");
            None
        }
    }
}

/// Answers a call made during a dictation, as `plan` says: ends the
/// recording if needed (as a press of the shortcut `finish_as` would, see
/// `ServiceKind::binding_id`), then waits for its text and returns it. `pump` lets
/// the main thread's other work run for a moment between two checks; the
/// service is answered on the main thread, which the dictation may need.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn answer_waiting_call(
    app: &tauri::AppHandle,
    plan: CallPlan,
    finish_as: Option<&'static str>,
    mut pump: impl FnMut(),
) -> Option<String> {
    use tauri::Manager;

    if !WAITING_CALL.begin() {
        log::info!("Insert Last Dictation: another call already waits for this dictation.");
        return None;
    }
    if plan == CallPlan::StopAndWait {
        if let Some(coordinator) = app.try_state::<crate::TranscriptionCoordinator>() {
            coordinator.finish_recording_as(finish_as);
        }
        log::info!(
            "Insert Last Dictation: ending the recording (service of {}) and waiting for its text.",
            finish_as.unwrap_or("no shortcut")
        );
    } else {
        log::info!("Insert Last Dictation: waiting for the dictation's text.");
    }
    let started = std::time::Instant::now();
    loop {
        let step = next_wait_step(
            &WAITING_CALL,
            crate::tray::dictation_in_progress(app),
            started.elapsed() >= WAIT_FOR_DICTATION,
        );
        match step {
            WaitStep::KeepWaiting => pump(),
            WaitStep::Insert(text) => {
                log::info!(
                    "Insert Last Dictation: handing over the dictation after {:?}.",
                    started.elapsed()
                );
                crate::overlay::hide_ready_notice(app);
                let settings = crate::settings::get_settings(app);
                return match service_reply(false, Some(&text), settings.append_trailing_space) {
                    ServiceReply::Insert(text) => Some(text),
                    ServiceReply::NotReady | ServiceReply::Nothing => None,
                };
            }
            WaitStep::EndedWithoutText => {
                log::info!("Insert Last Dictation: the dictation ended without text.");
                return None;
            }
            WaitStep::GaveUp => {
                log::info!(
                    "Insert Last Dictation: gave up after {:?}; the dictation waits as usual.",
                    started.elapsed()
                );
                return None;
            }
        }
    }
}

/// Answers a call of one of DanyWhere's services. During a dictation without
/// Accessibility it ends the recording (with post-processing or context for
/// those shortcuts' services) and answers once the text is ready.
/// Otherwise "Insert Last Dictation" answers with the last dictation, while
/// the three "end dictation" services never insert an earlier dictation: they
/// answer with nothing and, as the user chose, start a recording or do
/// nothing (user, 2026-11-12).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn answer_service(app: &tauri::AppHandle, kind: ServiceKind, pump: impl FnMut()) -> Option<String> {
    let settings = crate::settings::get_settings(app);
    let waits_for_service =
        crate::delivery::dictation_target(&settings, crate::delivery::INSERT_SERVICE_AVAILABLE)
            == crate::delivery::DictationTarget::InsertService;
    let phase = crate::tray::dictation_phase(app);
    let Some(binding_id) = kind.binding_id() else {
        return match plan_call(waits_for_service, phase) {
            CallPlan::AnswerNow => answer_call(app),
            plan => answer_waiting_call(app, plan, None, pump),
        };
    };
    match plan_end_call(
        waits_for_service,
        phase,
        settings.dictation_service_when_idle,
    ) {
        EndCallPlan::StopAndWait => {
            answer_waiting_call(app, CallPlan::StopAndWait, Some(binding_id), pump)
        }
        EndCallPlan::Wait => answer_waiting_call(app, CallPlan::Wait, Some(binding_id), pump),
        EndCallPlan::StartRecording => {
            log::info!("{kind:?}: called while nothing records; starting a recording.");
            start_recording_when_ready(app, binding_id);
            None
        }
        EndCallPlan::Nothing => {
            log::info!("{kind:?}: called while nothing records; doing nothing.");
            None
        }
    }
}

/// How long a service-started recording waits for DanyWhere to finish
/// starting (macOS may have launched DanyWhere just to answer the service).
const START_WHEN_READY: std::time::Duration = std::time::Duration::from_secs(15);

/// Starts a locked recording for `binding_id` off the main thread, so the
/// calling app gets its (empty) answer at once. Waits for the transcription
/// coordinator if DanyWhere is still starting, and gives up with a log line
/// after [`START_WHEN_READY`].
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn start_recording_when_ready(app: &tauri::AppHandle, binding_id: &'static str) {
    use tauri::Manager;

    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("service-start".into())
        .spawn(move || {
            let started = std::time::Instant::now();
            loop {
                if let Some(coordinator) = app.try_state::<crate::TranscriptionCoordinator>() {
                    coordinator.start_locked_recording(binding_id);
                    log::info!(
                        "Started a recording for '{binding_id}' through its service after {:?}.",
                        started.elapsed()
                    );
                    return;
                }
                if started.elapsed() >= START_WHEN_READY {
                    log::warn!(
                        "Not starting a recording for '{binding_id}': DanyWhere wasn't ready after {START_WHEN_READY:?}."
                    );
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        });
    if let Err(err) = spawned {
        log::error!("Couldn't start the thread for a service-started recording: {err}");
    }
}

/// Gives the three "end dictation" services the dictation shortcuts as their
/// own while DanyWhere runs without Accessibility and "Dictation shortcut
/// also inserts" is on, and takes them away otherwise. Call at startup and
/// whenever a dictation shortcut, the setting or the mode changes. Apps that
/// are already open learn a changed shortcut only after a restart.
pub fn sync_dictation_service_shortcuts(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    macos::sync_dictation_service_shortcuts(app);
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// Offers the service to other apps. Call once on the main thread at startup.
pub fn register(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    macos::register(app);
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// Gives the service its default shortcut ⌃⌥⌘V, once, when DanyWhere runs
/// without Accessibility. Never touches a shortcut the user set or removed.
pub fn ensure_default_shortcut(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    macos::ensure_default_shortcut(app);
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// The service's current shortcut as macOS shows it (e.g. `⌃⌥⌘V`), or
/// `None` when it has none or the platform has no services.
pub fn current_shortcut(app: &tauri::AppHandle) -> Option<String> {
    #[cfg(target_os = "macos")]
    let key_equivalent = macos::current_key_equivalent(app);
    #[cfg(not(target_os = "macos"))]
    let key_equivalent: Option<String> = {
        let _ = app;
        None
    };
    key_equivalent.as_deref().and_then(display_key_equivalent)
}

/// The "Insert Last Dictation" service's current shortcut for the settings,
/// as a recorded combination (e.g. `ctrl+option+command+v`), or `None`.
#[tauri::command]
#[specta::specta]
pub fn get_insert_service_shortcut(app: tauri::AppHandle) -> Option<String> {
    #[cfg(target_os = "macos")]
    let key_equivalent = macos::current_key_equivalent(&app);
    #[cfg(not(target_os = "macos"))]
    let key_equivalent: Option<String> = {
        let _ = app;
        None
    };
    key_equivalent
        .as_deref()
        .and_then(binding_from_key_equivalent)
}

/// Gives the service the recorded combination `binding` as its shortcut and
/// returns it as stored. Errors are codes the settings translate: `invalid`,
/// `conflict:<binding id>`, `unsupported` (no services on this platform) or
/// `failed:<reason>`.
#[tauri::command]
#[specta::specta]
pub fn set_insert_service_shortcut(
    app: tauri::AppHandle,
    binding: String,
) -> Result<String, String> {
    let key_equivalent = key_equivalent_from_binding(&binding).map_err(|problem| {
        log::info!("Insert Last Dictation: refused shortcut {binding}: {problem:?}");
        "invalid".to_string()
    })?;
    let settings = crate::settings::get_settings(&app);
    if let Some(id) = conflicting_binding(&binding, &settings.bindings) {
        return Err(format!("conflict:{id}"));
    }
    let stored = binding_from_key_equivalent(&key_equivalent).unwrap_or_default();
    #[cfg(target_os = "macos")]
    let result = macos::set_key_equivalent(&app, &key_equivalent)
        .map(|()| stored)
        .map_err(|err| format!("failed:{err}"));
    #[cfg(not(target_os = "macos"))]
    let result: Result<String, String> = {
        let _ = (app, stored);
        Err("unsupported".to_string())
    };
    result
}

#[cfg(target_os = "macos")]
mod macos {
    use log::{error, info, warn};
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadMarker};
    use objc2_app_kit::{
        NSApplication, NSPasteboard, NSPasteboardTypeString, NSUpdateDynamicServices,
    };
    use objc2_foundation::{
        NSBundle, NSDate, NSDefaultRunLoopMode, NSDictionary, NSObject, NSRunLoop, NSString,
        NSUserDefaults,
    };
    use tauri::AppHandle;

    use super::{
        answer_service, dictation_service_shortcuts, shortcuts_written_at,
        should_set_default_shortcut, status_entry_value, status_key, ServiceKind,
        DEFAULT_KEY_EQUIVALENT,
    };

    pub struct ProviderIvars {
        app: AppHandle,
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements; the ivars are
        // plain Rust values.
        #[unsafe(super(NSObject))]
        #[name = "DanyWhereInsertServiceProvider"]
        #[ivars = ProviderIvars]
        pub struct InsertServiceProvider;

        impl InsertServiceProvider {
            // Named by NSMessage in Info.plist. AppKit calls them on the main
            // thread with a pasteboard made for this one request.
            #[unsafe(method(insertLastDictation:userData:error:))]
            fn insert_last_dictation(
                &self,
                pasteboard: &NSPasteboard,
                _user_data: Option<&NSString>,
                _error: *mut *mut NSString,
            ) {
                self.reply(pasteboard, ServiceKind::InsertLastDictation);
            }

            #[unsafe(method(endDictationAndInsert:userData:error:))]
            fn end_dictation_and_insert(
                &self,
                pasteboard: &NSPasteboard,
                _user_data: Option<&NSString>,
                _error: *mut *mut NSString,
            ) {
                self.reply(pasteboard, ServiceKind::EndDictation);
            }

            #[unsafe(method(endDictationWithPostProcessAndInsert:userData:error:))]
            fn end_dictation_with_post_process_and_insert(
                &self,
                pasteboard: &NSPasteboard,
                _user_data: Option<&NSString>,
                _error: *mut *mut NSString,
            ) {
                self.reply(pasteboard, ServiceKind::EndDictationWithPostProcess);
            }

            #[unsafe(method(endDictationWithContextAndInsert:userData:error:))]
            fn end_dictation_with_context_and_insert(
                &self,
                pasteboard: &NSPasteboard,
                _user_data: Option<&NSString>,
                _error: *mut *mut NSString,
            ) {
                self.reply(pasteboard, ServiceKind::EndDictationWithContext);
            }
        }
    );

    impl InsertServiceProvider {
        /// Answers a call of `kind` on the service's own pasteboard.
        fn reply(&self, pasteboard: &NSPasteboard, kind: ServiceKind) {
            let Some(text) = answer_service(&self.ivars().app, kind, pump_main_run_loop) else {
                return;
            };
            pasteboard.clearContents();
            // SAFETY: NSPasteboardTypeString is a read-only framework static.
            let written = pasteboard.setString_forType(&NSString::from_str(&text), unsafe {
                NSPasteboardTypeString
            });
            if !written {
                error!("{kind:?}: could not write to the service pasteboard.");
            }
        }
    }

    /// Runs the main thread's run loop for a moment, so that work the
    /// dictation queues there keeps going while the service call waits.
    fn pump_main_run_loop() {
        // SAFETY: NSDefaultRunLoopMode is a read-only framework static.
        let mode = unsafe { NSDefaultRunLoopMode };
        let until = NSDate::dateWithTimeIntervalSinceNow(PUMP_INTERVAL_SECONDS);
        NSRunLoop::currentRunLoop().runMode_beforeDate(mode, &until);
    }

    /// How long one round of [`pump_main_run_loop`] runs at most.
    const PUMP_INTERVAL_SECONDS: f64 = 0.02;

    impl InsertServiceProvider {
        fn new(app: AppHandle) -> Retained<Self> {
            let this = Self::alloc().set_ivars(ProviderIvars { app });
            unsafe { msg_send![super(this), init] }
        }
    }

    /// `(menu title, message)` of each service this bundle declares; empty
    /// when it declares none (e.g. a binary run outside its app bundle).
    fn declared_services() -> Vec<(String, String)> {
        let Some(services) = NSBundle::mainBundle()
            .objectForInfoDictionaryKey(&NSString::from_str("NSServices"))
            .and_then(|services| services.downcast::<objc2_foundation::NSArray>().ok())
        else {
            return Vec::new();
        };
        let string_for = |dict: &NSDictionary, key: &str| -> Option<String> {
            let key = NSString::from_str(key);
            let value = dict.objectForKey(&key)?.downcast::<NSString>().ok()?;
            Some(value.to_string())
        };
        services
            .iter()
            .filter_map(|service| {
                let service = service.downcast::<NSDictionary>().ok()?;
                let message = string_for(&service, "NSMessage")?;
                let menu_item = service
                    .objectForKey(&NSString::from_str("NSMenuItem"))?
                    .downcast::<NSDictionary>()
                    .ok()?;
                let title = string_for(&menu_item, "default")?;
                Some((title, message))
            })
            .collect()
    }

    /// The `NSServicesStatus` key of one of this app's services.
    fn status_key_for(app: &AppHandle, kind: ServiceKind) -> Option<String> {
        let (title, message) = declared_services()
            .into_iter()
            .find(|(_, message)| message == kind.message())?;
        Some(status_key(&app.config().identifier, &title, &message))
    }

    /// The `NSServicesStatus` key of "Insert Last Dictation".
    fn this_status_key(app: &AppHandle) -> Option<String> {
        status_key_for(app, ServiceKind::InsertLastDictation)
    }

    /// The services settings of the current user (`defaults read pbs`).
    fn services_defaults() -> Option<Retained<NSUserDefaults>> {
        NSUserDefaults::initWithSuiteName(NSUserDefaults::alloc(), Some(&NSString::from_str("pbs")))
    }

    /// This service's entry in `NSServicesStatus`, if there is one.
    fn status_entry(key: &str) -> Option<Retained<NSDictionary>> {
        let status =
            services_defaults()?.dictionaryForKey(&NSString::from_str("NSServicesStatus"))?;
        status
            .objectForKey(&NSString::from_str(key))?
            .downcast::<NSDictionary>()
            .ok()
    }

    pub fn current_key_equivalent(app: &AppHandle) -> Option<String> {
        key_equivalent_of(&this_status_key(app)?)
    }

    /// The key equivalent stored for a service, if any.
    fn key_equivalent_of(status_key: &str) -> Option<String> {
        let entry = status_entry(status_key)?;
        let key = NSString::from_str("key_equivalent");
        let value = entry.objectForKey(&key)?.downcast::<NSString>().ok()?;
        Some(value.to_string())
    }

    pub fn sync_dictation_service_shortcuts(app: &AppHandle) {
        let mut settings = crate::settings::get_settings(app);
        let enabled = settings.accessibility_skipped && settings.dictation_key_inserts;
        let mut wrote_shortcut = false;
        let mut has_shortcuts = false;
        for (kind, wanted) in dictation_service_shortcuts(&settings.bindings, enabled) {
            let Some(key) = status_key_for(app, kind) else {
                continue;
            };
            has_shortcuts |= wanted.is_some();
            let current = key_equivalent_of(&key).filter(|k| !k.is_empty());
            if current == wanted {
                continue;
            }
            // No entry and none wanted: nothing to take away.
            if wanted.is_none() && status_entry(&key).is_none() {
                continue;
            }
            let key_equivalent = wanted.clone().unwrap_or_default();
            match write_key_equivalent(&key, &key_equivalent) {
                Ok(()) => {
                    info!("{kind:?}: shortcut set to {wanted:?}.");
                    wrote_shortcut |= wanted.is_some();
                }
                Err(err) => error!("{kind:?}: could not set the shortcut: {err}"),
            }
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or_default();
        let written_at = shortcuts_written_at(
            settings.dictation_service_shortcuts_written_at,
            wrote_shortcut,
            has_shortcuts,
            now,
            services_file_modified(),
        );
        if written_at != settings.dictation_service_shortcuts_written_at {
            info!("The services got their shortcuts at {written_at:?} (Unix time); apps launched before know them after a restart.");
            settings.dictation_service_shortcuts_written_at = written_at;
            crate::settings::write_settings(app, settings);
        }
    }

    /// When the services settings (`~/Library/Preferences/pbs.plist`) last
    /// changed, in seconds since the Unix epoch.
    fn services_file_modified() -> Option<u64> {
        let home = std::env::var_os("HOME")?;
        let path = std::path::Path::new(&home).join("Library/Preferences/pbs.plist");
        let modified = std::fs::metadata(path).ok()?.modified().ok()?;
        modified
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|elapsed| elapsed.as_secs())
    }

    pub fn register(app: &AppHandle) {
        let Some(mtm) = MainThreadMarker::new() else {
            warn!("Insert Last Dictation: not on the main thread; service not offered.");
            return;
        };
        let services = declared_services();
        if services.is_empty() {
            info!("Insert Last Dictation: no service in Info.plist (unbundled run).");
            return;
        }
        let titles: Vec<&str> = services.iter().map(|(title, _)| title.as_str()).collect();
        let provider = InsertServiceProvider::new(app.clone());
        let provider_object: &AnyObject = &provider;
        // SAFETY: the provider implements the method Info.plist names, and it
        // is kept alive for the app's lifetime below.
        unsafe { NSApplication::sharedApplication(mtm).setServicesProvider(Some(provider_object)) };
        NSUpdateDynamicServices();
        // AppKit doesn't keep the provider; DanyWhere needs it until it quits.
        std::mem::forget(provider);
        info!("Offering the services {titles:?}.");
    }

    pub fn ensure_default_shortcut(app: &AppHandle) {
        let mut settings = crate::settings::get_settings(app);
        if !settings.accessibility_skipped || settings.insert_service_shortcut_set {
            return;
        }
        let Some(key) = this_status_key(app) else {
            return;
        };
        let entry_exists = status_entry(&key).is_some();
        if should_set_default_shortcut(settings.insert_service_shortcut_set, entry_exists) {
            if let Err(err) = write_key_equivalent(&key, DEFAULT_KEY_EQUIVALENT) {
                error!("Insert Last Dictation: could not set the shortcut: {err}");
                return;
            }
            info!("Insert Last Dictation: set its shortcut to {DEFAULT_KEY_EQUIVALENT}.");
        } else {
            info!("Insert Last Dictation: keeping the shortcut found in the services settings.");
        }
        settings.insert_service_shortcut_set = true;
        crate::settings::write_settings(app, settings);
    }

    /// Gives this app's service `key_equivalent` as its shortcut, on the
    /// user's request in the settings.
    pub fn set_key_equivalent(app: &AppHandle, key_equivalent: &str) -> Result<(), String> {
        let key = this_status_key(app).ok_or("this build declares no service")?;
        write_key_equivalent(&key, key_equivalent)?;
        info!("Insert Last Dictation: shortcut changed to {key_equivalent}.");
        let mut settings = crate::settings::get_settings(app);
        settings.insert_service_shortcut_set = true;
        crate::settings::write_settings(app, settings);
        Ok(())
    }

    /// Writes the entry the way System Settings → Keyboard → Keyboard
    /// Shortcuts → Services stores it for an enabled service, then has the
    /// services daemon reload it. Apps that are already running only pick a
    /// new shortcut up after a restart: neither `NSUpdateDynamicServices`,
    /// `pbs -flush`, a change in System Settings nor opening the app's
    /// Services menu by hand tells them (tried 2026-11-11/12, DanyWhere's
    /// sidecar, `qa/2026-11-11_2306_insert-service.md`).
    fn write_key_equivalent(key: &str, key_equivalent: &str) -> Result<(), String> {
        let entry = status_entry_value(key_equivalent);
        let status = std::process::Command::new("/usr/bin/defaults")
            .args(["write", "pbs", "NSServicesStatus", "-dict-add", key, &entry])
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!("defaults exited with {status}"));
        }
        // Reloading takes a moment; nothing waits for it.
        std::thread::spawn(|| {
            match std::process::Command::new("/System/Library/CoreServices/pbs")
                .arg("-update")
                .status()
            {
                Ok(status) if status.success() => {}
                Ok(status) => warn!("pbs -update exited with {status}"),
                Err(err) => warn!("pbs -update failed: {err}"),
            }
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The menu title in `Info.plist` (`NSMenuItem` → `default`); the code
    /// reads it from the bundle, because the test builds (`DanyWhere Dev`,
    /// `DanyWhere NoAX`) get their own name in front.
    const SERVICE_MENU_TITLE: &str = "DanyWhere: Insert Last Dictation";

    fn read_manifest_file(relative: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
        std::fs::read_to_string(&path).unwrap_or_default()
    }

    /// The first `<string>` value after `<key>{key}</key>` in a plist.
    fn string_after_key<'a>(plist: &'a str, key: &str) -> Option<&'a str> {
        let rest = &plist[plist.find(&format!("<key>{key}</key>"))?..];
        let start = rest.find("<string>")? + "<string>".len();
        let end = start + rest[start..].find("</string>")?;
        Some(&rest[start..end])
    }

    // -----------------------------------------------------------------
    // The shortcut also ends a dictation (user, 2026-11-12): a call during
    // a dictation stops the recording and inserts the text once it's ready.
    // -----------------------------------------------------------------

    use crate::tray::TrayIconState;

    #[test]
    fn a_call_while_recording_ends_the_recording_and_waits() {
        assert_eq!(
            plan_call(true, TrayIconState::Recording),
            CallPlan::StopAndWait
        );
    }

    #[test]
    fn a_call_while_transcribing_waits_for_the_text() {
        assert_eq!(plan_call(true, TrayIconState::Transcribing), CallPlan::Wait);
    }

    #[test]
    fn a_call_without_a_dictation_answers_right_away() {
        assert_eq!(plan_call(true, TrayIconState::Idle), CallPlan::AnswerNow);
    }

    #[test]
    fn with_accessibility_a_call_never_ends_or_waits_for_a_dictation() {
        for phase in [
            TrayIconState::Idle,
            TrayIconState::Recording,
            TrayIconState::Transcribing,
        ] {
            assert_eq!(plan_call(false, phase), CallPlan::AnswerNow, "{phase:?}");
        }
    }

    #[test]
    fn a_waiting_call_gets_the_delivered_text() {
        let slot = WaitSlot::new();
        assert!(slot.begin());
        assert_eq!(slot.take(), None, "nothing delivered yet");
        assert!(slot.deliver("Hallo Welt"));
        assert_eq!(slot.take(), Some("Hallo Welt".to_string()));
    }

    #[test]
    fn without_a_waiting_call_nothing_is_delivered() {
        let slot = WaitSlot::new();
        assert!(!slot.deliver("Hallo Welt"));
        assert_eq!(slot.take(), None);
    }

    #[test]
    fn only_one_call_waits_at_a_time() {
        let slot = WaitSlot::new();
        assert!(slot.begin());
        assert!(
            !slot.begin(),
            "a second call must not wait for the same text"
        );
    }

    #[test]
    fn a_call_that_gave_up_frees_the_slot_and_refuses_later_text() {
        let slot = WaitSlot::new();
        assert!(slot.begin());
        assert_eq!(slot.give_up(), None);
        assert!(
            !slot.deliver("zu spät"),
            "nobody waits any more: the text must take the normal path"
        );
        assert!(slot.begin(), "the next call may wait again");
    }

    #[test]
    fn a_text_delivered_at_the_last_moment_is_not_lost() {
        let slot = WaitSlot::new();
        assert!(slot.begin());
        assert!(slot.deliver("gerade noch"));
        assert_eq!(slot.give_up(), Some("gerade noch".to_string()));
    }

    #[test]
    fn after_the_text_was_taken_the_slot_is_free() {
        let slot = WaitSlot::new();
        assert!(slot.begin());
        assert!(slot.deliver("eins"));
        assert_eq!(slot.take(), Some("eins".to_string()));
        assert!(!slot.deliver("zwei"));
        assert!(slot.begin());
    }

    #[test]
    fn a_waiting_call_keeps_waiting_while_the_dictation_runs() {
        let slot = WaitSlot::new();
        assert!(slot.begin());
        assert_eq!(next_wait_step(&slot, true, false), WaitStep::KeepWaiting);
    }

    #[test]
    fn a_waiting_call_inserts_the_text_once_it_arrives() {
        let slot = WaitSlot::new();
        assert!(slot.begin());
        assert!(slot.deliver("fertig"));
        assert_eq!(
            next_wait_step(&slot, true, false),
            WaitStep::Insert("fertig".into())
        );
    }

    #[test]
    fn a_text_that_arrives_as_the_dictation_ends_is_inserted() {
        let slot = WaitSlot::new();
        assert!(slot.begin());
        assert!(slot.deliver("fertig"));
        assert_eq!(
            next_wait_step(&slot, false, false),
            WaitStep::Insert("fertig".into())
        );
    }

    #[test]
    fn a_dictation_that_ends_without_text_ends_the_wait() {
        let slot = WaitSlot::new();
        assert!(slot.begin());
        assert_eq!(
            next_wait_step(&slot, false, false),
            WaitStep::EndedWithoutText
        );
        assert!(!slot.deliver("später"), "the slot must be free again");
    }

    #[test]
    fn a_waiting_call_gives_up_at_the_deadline() {
        let slot = WaitSlot::new();
        assert!(slot.begin());
        assert_eq!(next_wait_step(&slot, true, true), WaitStep::GaveUp);
        assert!(
            !slot.deliver("zu spät"),
            "the text must take the normal path"
        );
    }

    #[test]
    fn a_text_delivered_at_the_deadline_is_still_inserted() {
        let slot = WaitSlot::new();
        assert!(slot.begin());
        assert!(slot.deliver("knapp"));
        assert_eq!(
            next_wait_step(&slot, true, true),
            WaitStep::Insert("knapp".into())
        );
    }

    #[test]
    fn the_calling_app_waits_longer_than_danywhere() {
        let plist = read_manifest_file("Info.plist");
        let timeout_ms: u128 = string_after_key(&plist, "NSTimeout")
            .and_then(|value| value.parse().ok())
            .expect("Info.plist sets NSTimeout in milliseconds");
        assert!(
            timeout_ms >= WAIT_FOR_DICTATION.as_millis() + 5_000,
            "NSTimeout {timeout_ms} ms must leave DanyWhere's {} ms wait room to answer",
            WAIT_FOR_DICTATION.as_millis()
        );
    }

    #[test]
    fn inserts_nothing_while_a_dictation_is_in_progress() {
        assert_eq!(
            service_reply(true, Some("the previous dictation"), false),
            ServiceReply::NotReady
        );
    }

    #[test]
    fn inserts_the_last_dictation() {
        assert_eq!(
            service_reply(false, Some("Hallo Welt"), false),
            ServiceReply::Insert("Hallo Welt".into())
        );
    }

    #[test]
    fn adds_the_trailing_space_when_set() {
        assert_eq!(
            service_reply(false, Some("Hallo"), true),
            ServiceReply::Insert("Hallo ".into())
        );
    }

    #[test]
    fn inserts_nothing_without_a_dictation() {
        assert_eq!(service_reply(false, None, false), ServiceReply::Nothing);
        assert_eq!(
            service_reply(false, Some("  \n"), true),
            ServiceReply::Nothing
        );
    }

    #[test]
    fn builds_the_key_system_settings_use_for_the_shortcut() {
        assert_eq!(
            status_key("com.yaccob.danywhere", SERVICE_MENU_TITLE, SERVICE_MESSAGE),
            "com.yaccob.danywhere - DanyWhere: Insert Last Dictation - insertLastDictation"
        );
    }

    #[test]
    fn shows_the_default_shortcut_with_mac_symbols() {
        assert_eq!(
            display_key_equivalent(DEFAULT_KEY_EQUIVALENT).as_deref(),
            Some("⌃⌥⌘V")
        );
    }

    #[test]
    fn orders_modifiers_as_macos_menus_do() {
        assert_eq!(display_key_equivalent("$@^~k").as_deref(), Some("⌃⌥⇧⌘K"));
        assert_eq!(display_key_equivalent("@$v").as_deref(), Some("⇧⌘V"));
    }

    #[test]
    fn no_shortcut_for_an_empty_key_equivalent() {
        assert_eq!(display_key_equivalent(""), None);
        assert_eq!(display_key_equivalent("@~"), None);
    }

    #[test]
    fn sets_the_default_shortcut_once() {
        assert!(should_set_default_shortcut(false, false));
    }

    #[test]
    fn never_sets_the_default_shortcut_again() {
        // The user may have removed it in System Settings since.
        assert!(!should_set_default_shortcut(true, false));
    }

    #[test]
    fn leaves_a_shortcut_the_user_chose_alone() {
        assert!(!should_set_default_shortcut(false, true));
    }

    #[test]
    fn turns_a_recorded_combination_into_the_services_notation() {
        assert_eq!(
            key_equivalent_from_binding("ctrl+option+command+v").as_deref(),
            Ok("@~^v")
        );
        assert_eq!(
            key_equivalent_from_binding("command+shift+k").as_deref(),
            Ok("@$k")
        );
    }

    #[test]
    fn accepts_every_spelling_of_the_modifiers() {
        assert_eq!(
            key_equivalent_from_binding("control+alt+cmd+J").as_deref(),
            Ok("@~^j")
        );
        assert_eq!(
            key_equivalent_from_binding("meta+ctrl+b").as_deref(),
            Ok("@^b")
        );
    }

    #[test]
    fn accepts_digits_and_punctuation() {
        assert_eq!(
            key_equivalent_from_binding("command+option+,").as_deref(),
            Ok("@~,")
        );
        assert_eq!(
            key_equivalent_from_binding("ctrl+command+1").as_deref(),
            Ok("@^1")
        );
    }

    #[test]
    fn needs_command_or_control() {
        assert_eq!(
            key_equivalent_from_binding("option+v"),
            Err(ShortcutProblem::NeedsCommandOrControl)
        );
        assert_eq!(
            key_equivalent_from_binding("shift+v"),
            Err(ShortcutProblem::NeedsCommandOrControl)
        );
    }

    #[test]
    fn needs_a_key_besides_the_modifiers() {
        assert_eq!(
            key_equivalent_from_binding("ctrl+command"),
            Err(ShortcutProblem::NoKey)
        );
    }

    #[test]
    fn rejects_keys_that_are_not_one_character() {
        assert_eq!(
            key_equivalent_from_binding("ctrl+command+space"),
            Err(ShortcutProblem::UnsupportedKey("space".into()))
        );
        assert_eq!(
            key_equivalent_from_binding("command+f5"),
            Err(ShortcutProblem::UnsupportedKey("f5".into()))
        );
        assert_eq!(
            key_equivalent_from_binding("command+a+b"),
            Err(ShortcutProblem::UnsupportedKey("a+b".into()))
        );
    }

    #[test]
    fn turns_the_services_notation_back_into_a_combination() {
        assert_eq!(
            binding_from_key_equivalent("@~^v").as_deref(),
            Some("ctrl+option+command+v")
        );
        assert_eq!(
            binding_from_key_equivalent("@$k").as_deref(),
            Some("shift+command+k")
        );
        assert_eq!(binding_from_key_equivalent(""), None);
        assert_eq!(binding_from_key_equivalent("@~"), None);
    }

    #[test]
    fn a_combination_survives_the_round_trip() {
        for binding in ["ctrl+option+command+v", "shift+command+k", "ctrl+command+1"] {
            let key_equivalent = key_equivalent_from_binding(binding).unwrap();
            assert_eq!(
                binding_from_key_equivalent(&key_equivalent).as_deref(),
                Some(binding)
            );
        }
    }

    #[test]
    fn finds_a_danywhere_shortcut_that_already_uses_the_combination() {
        let mut bindings = crate::settings::get_default_settings().bindings;
        bindings
            .get_mut("paste_last_transcript")
            .unwrap()
            .current_binding = "ctrl+shift+v".into();
        assert_eq!(
            conflicting_binding("shift+ctrl+v", &bindings).as_deref(),
            Some("paste_last_transcript")
        );
        assert_eq!(
            conflicting_binding("control+shift+V", &bindings).as_deref(),
            Some("paste_last_transcript")
        );
        assert_eq!(
            conflicting_binding("ctrl+option+command+v", &bindings),
            None
        );
    }

    #[test]
    fn stores_the_shortcut_as_system_settings_does() {
        assert_eq!(
            status_entry_value("@~^v"),
            "{ \"key_equivalent\" = \"@~^v\"; }"
        );
    }

    // -----------------------------------------------------------------
    // The dictation shortcut also inserts (user, 2026-11-12): two more
    // services carry the dictation shortcuts as their key equivalents.
    // -----------------------------------------------------------------

    fn binding(id: &str, keys: &str) -> (String, crate::settings::ShortcutBinding) {
        (
            id.to_string(),
            crate::settings::ShortcutBinding {
                id: id.to_string(),
                name: id.to_string(),
                description: String::new(),
                default_binding: keys.to_string(),
                current_binding: keys.to_string(),
            },
        )
    }

    use crate::settings::DictationServiceWhenIdle;

    #[test]
    fn an_end_service_ends_a_recording_or_waits_for_its_text() {
        for when_idle in [
            DictationServiceWhenIdle::StartRecording,
            DictationServiceWhenIdle::DoNothing,
        ] {
            assert_eq!(
                plan_end_call(true, TrayIconState::Recording, when_idle),
                EndCallPlan::StopAndWait
            );
            assert_eq!(
                plan_end_call(true, TrayIconState::Transcribing, when_idle),
                EndCallPlan::Wait
            );
        }
    }

    #[test]
    fn an_end_service_called_while_idle_starts_a_recording_by_default() {
        assert_eq!(
            plan_end_call(
                true,
                TrayIconState::Idle,
                DictationServiceWhenIdle::default()
            ),
            EndCallPlan::StartRecording
        );
    }

    #[test]
    fn an_end_service_called_while_idle_can_do_nothing() {
        assert_eq!(
            plan_end_call(
                true,
                TrayIconState::Idle,
                DictationServiceWhenIdle::DoNothing
            ),
            EndCallPlan::Nothing
        );
    }

    #[test]
    fn with_accessibility_an_end_service_does_nothing() {
        for phase in [
            TrayIconState::Idle,
            TrayIconState::Recording,
            TrayIconState::Transcribing,
        ] {
            assert_eq!(
                plan_end_call(false, phase, DictationServiceWhenIdle::StartRecording),
                EndCallPlan::Nothing
            );
        }
    }

    #[test]
    fn settings_from_before_the_choice_start_a_recording() {
        let mut value = serde_json::to_value(crate::settings::get_default_settings()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("dictation_service_when_idle");
        let settings: crate::settings::AppSettings = serde_json::from_value(value).unwrap();
        assert_eq!(
            settings.dictation_service_when_idle,
            DictationServiceWhenIdle::StartRecording
        );
        assert_eq!(
            serde_json::to_value(DictationServiceWhenIdle::DoNothing).unwrap(),
            "do_nothing"
        );
    }

    #[test]
    fn each_service_has_its_own_message() {
        assert_eq!(ServiceKind::InsertLastDictation.message(), SERVICE_MESSAGE);
        assert_eq!(ServiceKind::EndDictation.message(), "endDictationAndInsert");
        assert_eq!(
            ServiceKind::EndDictationWithPostProcess.message(),
            "endDictationWithPostProcessAndInsert"
        );
        assert_eq!(
            ServiceKind::EndDictationWithContext.message(),
            "endDictationWithContextAndInsert"
        );
        for kind in ServiceKind::ALL {
            assert_eq!(ServiceKind::from_message(kind.message()), Some(kind));
        }
        assert_eq!(ServiceKind::from_message("somethingElse"), None);
    }

    #[test]
    fn each_end_service_belongs_to_its_dictation_shortcut() {
        assert_eq!(ServiceKind::EndDictation.binding_id(), Some("transcribe"));
        assert_eq!(
            ServiceKind::EndDictationWithContext.binding_id(),
            Some(crate::dictation_context::CONTEXT_BINDING)
        );
        assert_eq!(
            ServiceKind::EndDictationWithPostProcess.binding_id(),
            Some("transcribe_with_post_process")
        );
        assert_eq!(ServiceKind::InsertLastDictation.binding_id(), None);
    }

    #[test]
    fn a_dictation_shortcut_with_space_becomes_a_key_equivalent() {
        assert_eq!(dictation_key_equivalent("ctrl+space"), Ok("^ ".to_string()));
        assert_eq!(
            dictation_key_equivalent("ctrl+shift+space"),
            Ok("^$ ".to_string())
        );
        assert_eq!(
            dictation_key_equivalent("ctrl+option+shift+space"),
            Ok("~^$ ".to_string())
        );
        assert_eq!(
            dictation_key_equivalent("ctrl+shift+command+space"),
            Ok("@^$ ".to_string())
        );
    }

    #[test]
    fn a_dictation_shortcut_with_command_or_control_becomes_a_key_equivalent() {
        assert_eq!(
            dictation_key_equivalent("command+shift+d"),
            Ok("@$d".to_string())
        );
        assert_eq!(
            dictation_key_equivalent("option+shift+command+space"),
            Ok("@~$ ".to_string())
        );
    }

    #[test]
    fn a_dictation_shortcut_with_only_option_is_refused() {
        // ⌥ with a character key types a character (⌥Space a no-break
        // space), so the press never reaches the service: ⌥Space and ⌥⇧Space
        // did nothing on the company Mac, ⌃ worked (user, 2026-11-13).
        for binding in ["option+space", "option+shift+space", "option+d"] {
            assert_eq!(
                dictation_key_equivalent(binding),
                Err(ShortcutProblem::NeedsCommandOrControl),
                "{binding}"
            );
        }
        // A function key types nothing, with ⌥ or without.
        assert_eq!(
            dictation_key_equivalent("option+f5"),
            Ok("~\u{F708}".to_string())
        );
    }

    #[test]
    fn writing_a_shortcut_records_the_time() {
        assert_eq!(
            shortcuts_written_at(Some(100), true, true, 500, Some(450)),
            Some(500)
        );
        assert_eq!(shortcuts_written_at(None, true, true, 500, None), Some(500));
    }

    #[test]
    fn the_recorded_time_stays_until_the_next_write() {
        assert_eq!(
            shortcuts_written_at(Some(100), false, true, 500, Some(450)),
            Some(100)
        );
        // Taking the shortcuts away (e.g. Accessibility granted) leaves it.
        assert_eq!(
            shortcuts_written_at(Some(100), false, false, 500, Some(450)),
            Some(100)
        );
    }

    #[test]
    fn shortcuts_from_before_the_time_was_kept_get_the_services_files_time() {
        // DanyWhere 0.1.5 wrote them without keeping the time.
        assert_eq!(
            shortcuts_written_at(None, false, true, 500, Some(450)),
            Some(450)
        );
        assert_eq!(
            shortcuts_written_at(None, false, true, 500, None),
            Some(500)
        );
    }

    #[test]
    fn no_shortcuts_no_time() {
        assert_eq!(
            shortcuts_written_at(None, false, false, 500, Some(450)),
            None
        );
    }

    #[test]
    fn function_keys_become_appkits_function_key_characters() {
        assert_eq!(dictation_key_equivalent("f5"), Ok("\u{F708}".to_string()));
        assert_eq!(
            dictation_key_equivalent("shift+f13"),
            Ok("$\u{F710}".to_string())
        );
    }

    #[test]
    fn a_dictation_shortcut_that_would_type_is_refused() {
        for binding in ["space", "shift+a", "a", "shift+space"] {
            assert_eq!(
                dictation_key_equivalent(binding),
                Err(ShortcutProblem::TypesCharacter),
                "{binding}"
            );
        }
    }

    #[test]
    fn a_dictation_shortcut_needs_a_key_and_a_supported_one() {
        assert_eq!(
            dictation_key_equivalent("ctrl+option"),
            Err(ShortcutProblem::NoKey)
        );
        assert_eq!(
            dictation_key_equivalent("ctrl+backspace"),
            Err(ShortcutProblem::UnsupportedKey("backspace".to_string()))
        );
    }

    #[test]
    fn the_dictation_services_take_the_dictation_shortcuts() {
        let bindings = std::collections::HashMap::from([
            binding("transcribe", "ctrl+space"),
            binding("transcribe_with_post_process", "ctrl+shift+space"),
            binding("transcribe_with_context", "ctrl+shift+command+space"),
            binding("paste_last_transcript", "ctrl+shift+v"),
        ]);
        assert_eq!(
            dictation_service_shortcuts(&bindings, true),
            vec![
                (ServiceKind::EndDictation, Some("^ ".to_string())),
                (
                    ServiceKind::EndDictationWithPostProcess,
                    Some("^$ ".to_string())
                ),
                (
                    ServiceKind::EndDictationWithContext,
                    Some("@^$ ".to_string())
                ),
            ]
        );
    }

    #[test]
    fn option_only_dictation_shortcuts_give_their_services_none() {
        // The defaults up to 0.1.5: the press never reached the service.
        let bindings = std::collections::HashMap::from([
            binding("transcribe", "option+space"),
            binding("transcribe_with_post_process", "option+shift+space"),
            binding("transcribe_with_context", "option+shift+command+space"),
        ]);
        assert_eq!(
            dictation_service_shortcuts(&bindings, true),
            vec![
                (ServiceKind::EndDictation, None),
                (ServiceKind::EndDictationWithPostProcess, None),
                (
                    ServiceKind::EndDictationWithContext,
                    Some("@~$ ".to_string())
                ),
            ]
        );
    }

    #[test]
    fn the_dictation_services_have_no_shortcut_when_the_option_is_off() {
        let bindings = std::collections::HashMap::from([
            binding("transcribe", "option+space"),
            binding("transcribe_with_post_process", "option+shift+space"),
            binding("transcribe_with_context", "option+shift+command+space"),
        ]);
        assert_eq!(
            dictation_service_shortcuts(&bindings, false),
            vec![
                (ServiceKind::EndDictation, None),
                (ServiceKind::EndDictationWithPostProcess, None),
                (ServiceKind::EndDictationWithContext, None),
            ]
        );
    }

    #[test]
    fn a_shortcut_that_cant_be_a_key_equivalent_leaves_its_service_without_one() {
        let bindings = std::collections::HashMap::from([
            binding("transcribe", "ctrl+backspace"),
            binding("transcribe_with_post_process", "ctrl+shift+space"),
        ]);
        assert_eq!(
            dictation_service_shortcuts(&bindings, true),
            vec![
                (ServiceKind::EndDictation, None),
                (
                    ServiceKind::EndDictationWithPostProcess,
                    Some("^$ ".to_string())
                ),
                // No context shortcut set: no shortcut for its service.
                (ServiceKind::EndDictationWithContext, None),
            ]
        );
    }

    #[test]
    fn function_key_characters_are_written_as_plist_escapes() {
        assert_eq!(
            status_entry_value("$\u{F710}"),
            "{ \"key_equivalent\" = \"$\\UF710\"; }"
        );
        assert_eq!(status_entry_value("~ "), "{ \"key_equivalent\" = \"~ \"; }");
    }

    #[test]
    fn info_plist_declares_the_dictation_services() {
        let plist = read_manifest_file("Info.plist");
        for kind in ServiceKind::ALL {
            assert!(
                plist.contains(&format!("<string>{}</string>", kind.message())),
                "{kind:?}"
            );
        }
        assert!(plist.contains("<string>DanyWhere: End Dictation and Insert</string>"));
        assert!(plist
            .contains("<string>DanyWhere: End Dictation with Post-processing and Insert</string>"));
        assert!(plist.contains("<string>DanyWhere: End Dictation with Context and Insert</string>"));
        // Every service waits as long as the first one.
        assert_eq!(plist.matches("<key>NSTimeout</key>").count(), 4);
        assert_eq!(plist.matches("<key>NSPortName</key>").count(), 4);
    }

    #[test]
    fn the_german_services_menu_translates_the_dictation_services() {
        let strings = read_manifest_file("macos/de.lproj/ServicesMenu.strings");
        assert!(strings.contains(
            "\"DanyWhere: End Dictation and Insert\" = \"DanyWhere: Diktat beenden und einfügen\";"
        ));
        assert!(strings.contains(
            "\"DanyWhere: End Dictation with Post-processing and Insert\" = \"DanyWhere: Diktat mit Nachbearbeitung beenden und einfügen\";"
        ));
        assert!(strings.contains(
            "\"DanyWhere: End Dictation with Context and Insert\" = \"DanyWhere: Diktat mit Kontext beenden und einfügen\";"
        ));
    }

    #[test]
    fn says_not_ready_only_when_the_overlay_shows_no_session() {
        assert!(shows_not_ready_notice(false));
        assert!(!shows_not_ready_notice(true));
    }

    #[test]
    fn info_plist_declares_the_service() {
        let plist = read_manifest_file("Info.plist");
        assert!(plist.contains("<key>NSServices</key>"), "no NSServices");
        assert!(plist.contains(&format!("<string>{SERVICE_MESSAGE}</string>")));
        assert!(plist.contains(&format!("<string>{SERVICE_MENU_TITLE}</string>")));
        // Same as productName, as in the tested service app.
        assert_eq!(string_after_key(&plist, "NSPortName"), Some("DanyWhere"));
        assert!(plist.contains("<string>NSStringPboardType</string>"));
        // The service takes nothing from the app it is called in.
        assert!(!plist.contains("NSSendTypes"));
    }

    #[test]
    fn the_german_services_menu_translates_the_title() {
        let strings = read_manifest_file("macos/de.lproj/ServicesMenu.strings");
        assert!(strings.contains(&format!(
            "\"{SERVICE_MENU_TITLE}\" = \"DanyWhere: Letztes Diktat einfügen\";"
        )));
    }

    #[test]
    fn the_bundle_carries_the_german_services_menu() {
        let conf: serde_json::Value =
            serde_json::from_str(&read_manifest_file("tauri.conf.json")).unwrap();
        assert_eq!(
            conf["bundle"]["macOS"]["files"]["Resources/de.lproj/ServicesMenu.strings"],
            "macos/de.lproj/ServicesMenu.strings"
        );
    }
}
