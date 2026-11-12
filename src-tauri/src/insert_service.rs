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
/// for an enabled service.
pub fn status_entry_value(key_equivalent: &str) -> String {
    format!("{{ \"key_equivalent\" = \"{key_equivalent}\"; }}")
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
/// recording if needed, then waits for its text and returns it. `pump` lets
/// the main thread's other work run for a moment between two checks; the
/// service is answered on the main thread, which the dictation may need.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn answer_waiting_call(
    app: &tauri::AppHandle,
    plan: CallPlan,
    mut pump: impl FnMut(),
) -> Option<String> {
    use tauri::Manager;

    if !WAITING_CALL.begin() {
        log::info!("Insert Last Dictation: another call already waits for this dictation.");
        return None;
    }
    if plan == CallPlan::StopAndWait {
        if let Some(coordinator) = app.try_state::<crate::TranscriptionCoordinator>() {
            coordinator.finish_recording();
        }
        log::info!("Insert Last Dictation: ending the recording and waiting for its text.");
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
        answer_call, answer_waiting_call, plan_call, should_set_default_shortcut,
        status_entry_value, status_key, CallPlan, DEFAULT_KEY_EQUIVALENT, SERVICE_MESSAGE,
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
            // Named by NSMessage in Info.plist. AppKit calls it on the main
            // thread with a pasteboard made for this one request.
            #[unsafe(method(insertLastDictation:userData:error:))]
            fn insert_last_dictation(
                &self,
                pasteboard: &NSPasteboard,
                _user_data: Option<&NSString>,
                _error: *mut *mut NSString,
            ) {
                let app = &self.ivars().app;
                let settings = crate::settings::get_settings(app);
                let waits_for_service = crate::delivery::dictation_target(
                    &settings,
                    crate::delivery::INSERT_SERVICE_AVAILABLE,
                ) == crate::delivery::DictationTarget::InsertService;
                let reply = match plan_call(waits_for_service, crate::tray::dictation_phase(app)) {
                    CallPlan::AnswerNow => answer_call(app),
                    plan => answer_waiting_call(app, plan, pump_main_run_loop),
                };
                let Some(text) = reply else {
                    return;
                };
                pasteboard.clearContents();
                // SAFETY: NSPasteboardTypeString is a read-only framework static.
                let written = pasteboard
                    .setString_forType(&NSString::from_str(&text), unsafe { NSPasteboardTypeString });
                if !written {
                    error!("Insert Last Dictation: could not write to the service pasteboard.");
                }
            }
        }
    );

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

    /// `(menu title, message)` of the service this bundle declares, or `None`
    /// when it declares none (e.g. a binary run outside its app bundle).
    fn declared_service() -> Option<(String, String)> {
        let services = NSBundle::mainBundle()
            .objectForInfoDictionaryKey(&NSString::from_str("NSServices"))?
            .downcast::<objc2_foundation::NSArray>()
            .ok()?;
        let service = services.firstObject()?.downcast::<NSDictionary>().ok()?;
        let string_for = |dict: &NSDictionary, key: &str| -> Option<String> {
            let key = NSString::from_str(key);
            let value = dict.objectForKey(&key)?.downcast::<NSString>().ok()?;
            Some(value.to_string())
        };
        let message = string_for(&service, "NSMessage")?;
        let menu_item = service
            .objectForKey(&NSString::from_str("NSMenuItem"))?
            .downcast::<NSDictionary>()
            .ok()?;
        let title = string_for(&menu_item, "default")?;
        Some((title, message))
    }

    /// The `NSServicesStatus` key of this app's service.
    fn this_status_key(app: &AppHandle) -> Option<String> {
        let (title, message) = declared_service()?;
        if message != SERVICE_MESSAGE {
            warn!("Info.plist names the service message {message}, expected {SERVICE_MESSAGE}.");
        }
        Some(status_key(&app.config().identifier, &title, &message))
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
        let entry = status_entry(&this_status_key(app)?)?;
        let key = NSString::from_str("key_equivalent");
        let value = entry.objectForKey(&key)?.downcast::<NSString>().ok()?;
        Some(value.to_string())
    }

    pub fn register(app: &AppHandle) {
        let Some(mtm) = MainThreadMarker::new() else {
            warn!("Insert Last Dictation: not on the main thread; service not offered.");
            return;
        };
        let Some((title, _)) = declared_service() else {
            info!("Insert Last Dictation: no service in Info.plist (unbundled run).");
            return;
        };
        let provider = InsertServiceProvider::new(app.clone());
        let provider_object: &AnyObject = &provider;
        // SAFETY: the provider implements the method Info.plist names, and it
        // is kept alive for the app's lifetime below.
        unsafe { NSApplication::sharedApplication(mtm).setServicesProvider(Some(provider_object)) };
        NSUpdateDynamicServices();
        // AppKit doesn't keep the provider; DanyWhere needs it until it quits.
        std::mem::forget(provider);
        info!("Insert Last Dictation: offering the service \"{title}\".");
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
