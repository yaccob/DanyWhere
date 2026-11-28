//! DanyWhere: which window a dictation ends in, for context from earlier
//! dictations in the same window (`dictation_context`). Its text goes there,
//! and its profile is chosen by that window's app too (`app_profiles`, user,
//! 2026-11-16).
//!
//! When a recording ends, a background thread asks for the frontmost app
//! and the title of its window, with a short time limit, so the end of the
//! recording never waits for it:
//!
//! - macOS with Accessibility: the focused window's title, read through the
//!   Accessibility API.
//! - macOS without it, and in the mode without it (`accessibility`) also
//!   once macOS grants it: a scriptable app is asked for its title by Apple
//!   event ("Automation"), but only when the user has already allowed
//!   DanyWhere to control that app; the consent dialog is never shown during
//!   a dictation, only from the settings (sidecar note
//!   `qa/2026-11-12_2140_automation-fenstertitel.md`). Which apps can be
//!   asked, and how, comes from their bundles' scripting definitions
//!   (`title_query`). An app that hasn't answered yet is asked once when the
//!   recording starts, because a first Apple event can be too slow for the
//!   time limit at the end ([`prime_title_query`]).
//! - Windows: the foreground window's title and its program, which need no
//!   permission.
//! - Linux: nothing yet, so no context.
//!
//! Apps on the user's exclusion list are not captured at all.
//!
//! It also tells where the frontmost window and the mouse pointer are, so the
//! recording overlay shows on that window's screen (`overlay`); that needs no
//! permission on macOS or Windows.

#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;

use crate::accessibility::AccessibilityState;
use crate::dictation_context::{self, CapturedWindow};
// The title query by Apple event exists on macOS only (`title_query`), and
// with it everything below that asks for a title that way.
#[cfg(target_os = "macos")]
use crate::title_query::{self, TitleQuery};

/// How long a capture may take before the dictation goes on without it.
const CAPTURE_LIMIT: Duration = Duration::from_millis(800);

/// How DanyWhere learns window titles on macOS.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleSource {
    /// Every window's title, through the Accessibility API.
    Accessibility,
    /// A scriptable app's title by Apple event, once the user allowed it.
    AppleEvents,
}

/// Decides [`TitleSource`] for `access`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn title_source(access: &AccessibilityState) -> TitleSource {
    if access.can_read_windows() {
        TitleSource::Accessibility
    } else {
        TitleSource::AppleEvents
    }
}

/// Whether DanyWhere may ask an app for its window title by Apple event.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum AutomationStatus {
    Allowed,
    /// The user was never asked.
    NotAsked,
    /// The user (or the Mac's management) said no.
    Denied,
    /// The app isn't running, so macOS can't tell.
    NotRunning,
    Unknown,
}

/// The `OSStatus` of `AEDeterminePermissionToAutomateTarget` as a status.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn status_from_os(code: i32) -> AutomationStatus {
    match code {
        0 => AutomationStatus::Allowed,
        -1744 => AutomationStatus::NotAsked, // errAEEventWouldRequireUserConsent
        -1743 => AutomationStatus::Denied,   // errAEEventNotPermitted
        -600 => AutomationStatus::NotRunning, // procNotFound
        _ => AutomationStatus::Unknown,
    }
}

/// One capture per recording, handed from the capture thread to the end of
/// the recording.
pub struct CaptureSlot {
    state: Mutex<Option<(u64, Option<Option<CapturedWindow>>)>>,
    ready: Condvar,
}

impl CaptureSlot {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(None),
            ready: Condvar::new(),
        }
    }

    /// A capture for `session` has started; replaces any older one.
    pub fn begin(&self, session: u64) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        *state = Some((session, None));
    }

    /// The capture for `session` is done; ignored when a newer one began.
    pub fn finish(&self, session: u64, window: Option<CapturedWindow>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((current, result)) = state.as_mut() {
            if *current == session {
                *result = Some(window);
                self.ready.notify_all();
            }
        }
    }

    /// The window captured for `session`, waiting up to `wait` for a capture
    /// still under way. `None` when there was none or it took too long.
    pub fn take(&self, session: u64, wait: Duration) -> Option<CapturedWindow> {
        let deadline = Instant::now() + wait;
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            match state.as_mut() {
                Some((current, _)) if *current != session => return None,
                None => return None,
                Some((_, Some(_))) => {
                    let (_, result) = state.take()?;
                    return result.flatten();
                }
                Some((_, None)) => {
                    let now = Instant::now();
                    if now >= deadline {
                        return None;
                    }
                    state = self
                        .ready
                        .wait_timeout(state, deadline - now)
                        .unwrap_or_else(|e| e.into_inner())
                        .0;
                }
            }
        }
    }
}

static SLOT: CaptureSlot = CaptureSlot::new();

/// Starts capturing the frontmost window for the recording `session`, when
/// context from earlier dictations is in use. `settings` are the
/// dictation's (`dictation_snapshot`), with its profile's excluded
/// applications.
pub fn begin_capture(settings: &crate::settings::AppSettings, session: u64) {
    if !dictation_context::capture_wanted(settings) {
        return;
    }
    SLOT.begin(session);
    let excluded = settings.dictation_context.excluded_apps.clone();
    let access = crate::accessibility::state(settings);
    std::thread::Builder::new()
        .name("window-capture".into())
        .spawn(move || {
            let started = Instant::now();
            let window = frontmost_window(&excluded, access);
            log::debug!(
                "Captured the dictation's window in {:?}: {}",
                started.elapsed(),
                match &window {
                    Some(w) => format!(
                        "{} ({} title characters)",
                        w.app_id,
                        w.title.chars().count()
                    ),
                    None => "none".to_string(),
                }
            );
            SLOT.finish(session, window);
        })
        .map_err(|e| log::warn!("Could not start the window capture: {e}"))
        .ok();
}

/// The window captured when the recording `session` ended.
pub fn take_capture(session: u64) -> Option<CapturedWindow> {
    SLOT.take(session, CAPTURE_LIMIT)
}

/// DanyWhere: the frontmost application's id and name, without its window
/// title, for choosing a dictation's profile (`app_profiles`). Quick: neither
/// Accessibility nor an Apple event. `None` for DanyWhere itself and on Linux.
pub fn frontmost_app() -> Option<(String, String)> {
    platform::frontmost_app()
}

/// DanyWhere: `(id, name)` of the running applications with a window in the
/// Dock, for assigning them to profiles. Only macOS lists them.
pub fn running_applications() -> Vec<(String, String)> {
    platform::running_applications()
}

/// DanyWhere: `(id, name)` of the installed applications, for assigning them
/// to profiles. Only macOS lists them.
pub fn installed_applications() -> Vec<(String, String)> {
    platform::installed_applications()
}

/// DanyWhere: a rectangle on the screens, for putting the recording overlay
/// on the screen of the frontmost window (`overlay`). macOS: points in the
/// global display space, origin at the top left of the main screen. Windows:
/// physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl ScreenRect {
    /// Whether `point` lies inside, the left and top edges included and the
    /// right and bottom ones not, so a point on the line between two screens
    /// belongs to exactly one.
    pub fn contains(&self, (x, y): (f64, f64)) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    /// The area this rectangle shares with `other`, 0 when they don't overlap.
    pub fn overlap_area(&self, other: &ScreenRect) -> f64 {
        let width = (self.x + self.width).min(other.x + other.width) - self.x.max(other.x);
        let height = (self.y + self.height).min(other.y + other.height) - self.y.max(other.y);
        if width > 0.0 && height > 0.0 {
            width * height
        } else {
            0.0
        }
    }
}

/// DanyWhere: the frame of the frontmost window, for putting the recording
/// overlay on its screen (`overlay`).
///
/// - macOS: the frontmost app's frontmost window, from the window list. Its
///   owner, layer and bounds need neither Accessibility nor Screen Recording;
///   only window names would, and they are not read.
/// - Windows: the bounds of the monitor Windows puts the foreground window on
///   (the one holding most of it).
/// - Linux: `None`.
pub fn frontmost_window_frame() -> Option<ScreenRect> {
    platform::frontmost_window_frame()
}

/// DanyWhere: the mouse pointer, in the space of [`ScreenRect`], without
/// Enigo, which needs Accessibility on macOS. `None` on Linux.
pub fn cursor_location() -> Option<(f64, f64)> {
    platform::cursor_location()
}

/// One entry of macOS's list of the windows on screen, which runs from the
/// frontmost window to the backmost.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq)]
struct ListedWindow {
    owner_pid: i32,
    layer: i32,
    alpha: f64,
    bounds: ScreenRect,
}

/// Windows narrower or lower than this (points) don't count: helper windows
/// some apps keep on screen.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MIN_WINDOW_SIDE: f64 = 20.0;

/// The highest window layer an app's own window in front can have: normal
/// windows (0), floating windows such as palettes (3) and modal dialogs (8,
/// `kCGModalPanelWindowLevel`). The Dock (20), the menu bar (24), status
/// items and DanyWhere's overlay (25) and menus sit higher.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MAX_WINDOW_LAYER: i32 = 8;

/// The frame of `pid`'s frontmost window in `windows` (front to back): the
/// first one on a layer up to [`MAX_WINDOW_LAYER`] that can be seen.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn frontmost_window_of(windows: &[ListedWindow], pid: i32) -> Option<ScreenRect> {
    windows
        .iter()
        .find(|window| {
            window.owner_pid == pid
                && (0..=MAX_WINDOW_LAYER).contains(&window.layer)
                && window.alpha > 0.0
                && window.bounds.width >= MIN_WINDOW_SIDE
                && window.bounds.height >= MIN_WINDOW_SIDE
        })
        .map(|window| window.bounds)
}

/// The frontmost window as a [`CapturedWindow`], unless its app is excluded,
/// is DanyWhere itself or has no usable title. `access` says how its title
/// may be read ([`title_source`]).
fn frontmost_window(excluded: &[String], access: AccessibilityState) -> Option<CapturedWindow> {
    capture_front(
        |wants_title| platform::frontmost(wants_title, access),
        excluded,
    )
}

/// Whether DanyWhere may ask the app with this id and name for its title.
type WantsTitle<'a> = &'a dyn Fn(&str, &str) -> bool;

/// [`frontmost_window`] with the app in front given by `front`, which asks
/// for the title only when its argument says so: an excluded app gets no
/// Apple event and no Accessibility query.
fn capture_front(
    front: impl FnOnce(WantsTitle) -> Option<(String, String, Option<String>)>,
    excluded: &[String],
) -> Option<CapturedWindow> {
    let not_excluded =
        |id: &str, name: &str| !dictation_context::is_excluded_app(id, name, excluded);
    let (app_id, app_name, raw_title) = front(&not_excluded)?;
    if !not_excluded(&app_id, &app_name) {
        log::debug!("The frontmost app is excluded from context");
        return None;
    }
    let title = dictation_context::clean_window_title(&raw_title?, &app_name)?;
    Some(CapturedWindow {
        app_id,
        app_name,
        title,
    })
}

/// What the settings show about window titles on this computer.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct WindowTitleAccess {
    /// This platform can tell window titles at all (not Linux yet).
    pub supported: bool,
    /// macOS: DanyWhere reads every window's title through Accessibility
    /// ([`TitleSource::Accessibility`]).
    pub accessibility: bool,
    /// macOS without Accessibility: the apps that can tell their window title
    /// and whether DanyWhere may ask them, the running ones first, then the
    /// installed ones that aren't running, each by name.
    pub apps: Vec<AutomationApp>,
}

/// An app that can tell its window title by Apple event.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct AutomationApp {
    pub bundle_id: String,
    pub name: String,
    /// Only a running app can be allowed: macOS asks only about one.
    pub running: bool,
    pub status: AutomationStatus,
}

/// How DanyWhere learns window titles here, for the settings.
#[tauri::command]
#[specta::specta]
pub async fn get_window_title_access(app: AppHandle) -> WindowTitleAccess {
    let own_id = app.config().identifier.clone();
    let access = crate::accessibility::state(&crate::settings::get_settings(&app));
    tauri::async_runtime::spawn_blocking(move || platform::access(&own_id, access))
        .await
        .unwrap_or(WindowTitleAccess {
            supported: false,
            accessibility: false,
            apps: Vec::new(),
        })
}

/// DanyWhere: finds out in the background which apps can tell their window
/// title (macOS without Accessibility), so a dictation finds the answer in a
/// cache instead of reading the app's scripting definition.
pub fn warm_up(settings: &crate::settings::AppSettings) {
    platform::warm_up(crate::accessibility::state(settings));
}

/// DanyWhere: the apps that have answered a title query by Apple event, by
/// bundle id and process id. An app's first Apple event can be slow: on
/// 2026-11-18 TextEdit, just allowed, took more than the 700 ms a capture
/// gives it, and later ones about 250 ms. So a recording asks an app that
/// hasn't answered yet once when it starts ([`prime_title_query`]); one that
/// has isn't asked again.
#[derive(Default)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct AnsweredApps {
    apps: std::collections::HashSet<(String, i32)>,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl AnsweredApps {
    /// More entries than this (old processes) start the set afresh.
    const LIMIT: usize = 256;

    /// Whether the app `id` running as `pid` should be asked ahead.
    pub fn needs_priming(&self, id: &str, pid: i32) -> bool {
        !self.apps.contains(&(id.to_string(), pid))
    }

    /// The app `id` running as `pid` has answered.
    pub fn mark(&mut self, id: &str, pid: i32) {
        if self.apps.len() >= Self::LIMIT {
            self.apps.clear();
        }
        self.apps.insert((id.to_string(), pid));
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
static ANSWERED: std::sync::LazyLock<Mutex<AnsweredApps>> =
    std::sync::LazyLock::new(Default::default);

/// Asks the app in front (`front`: id, name, process id) for its title
/// ahead with `ask`, which says whether it answered, unless `wants_title`
/// rules it out or it has answered before; an answer is remembered in
/// `answered`. `None` when it wasn't asked.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn prime_front(
    front: impl FnOnce() -> Option<(String, String, i32)>,
    wants_title: WantsTitle,
    answered: &Mutex<AnsweredApps>,
    ask: impl FnOnce(&str, i32) -> bool,
) -> Option<bool> {
    let (id, name, pid) = front()?;
    if !wants_title(&id, &name) {
        return None;
    }
    let lock = || answered.lock().unwrap_or_else(|e| e.into_inner());
    if !lock().needs_priming(&id, pid) {
        return None;
    }
    let answer = ask(&id, pid);
    if answer {
        lock().mark(&id, pid);
    }
    Some(answer)
}

/// DanyWhere: when a recording starts, asks the app in front for its window
/// title once in the background, if it never answered one (macOS without
/// Accessibility only), so its answer when the recording ends comes in time.
pub fn prime_title_query(app: &AppHandle) {
    // The dictation's profile's excluded applications.
    let settings = crate::app_profiles::dictation_settings(app);
    if !dictation_context::capture_wanted(&settings) {
        return;
    }
    let excluded = settings.dictation_context.excluded_apps.clone();
    let access = crate::accessibility::state(&settings);
    std::thread::Builder::new()
        .name("title-query-prime".into())
        .spawn(move || platform::prime(&excluded, access))
        .map_err(|e| log::warn!("Could not start asking the app in front ahead: {e}"))
        .ok();
}

/// Shows macOS's dialog asking whether DanyWhere may control the app
/// `bundle_id` (only reads its window title). The app has to be running.
#[tauri::command]
#[specta::specta]
pub async fn request_window_title_access(bundle_id: String) -> AutomationStatus {
    tauri::async_runtime::spawn_blocking(move || platform::request(&bundle_id))
        .await
        .unwrap_or(AutomationStatus::Unknown)
}

/// The apps macOS is asked about once a dictation's text is in them (user's
/// choice, 2026-11-24): without Accessibility, an app that could tell its
/// window title but DanyWhere was never allowed to ask gets macOS's dialog
/// once per run, right after a dictation was inserted into it and never
/// before, so the dialog can't take the focus from the text field.
#[derive(Default)]
struct ConsentAsks {
    /// The app to ask after the next insertion.
    pending: Option<String>,
    /// The apps asked in this run.
    asked: std::collections::HashSet<String>,
}

impl ConsentAsks {
    /// `bundle_id` could tell its window title, but DanyWhere was never
    /// allowed to ask it (not asked yet).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn note_not_asked(&mut self, bundle_id: &str) {
        if !self.asked.contains(bundle_id) {
            self.pending = Some(bundle_id.to_string());
        }
    }

    /// The app to ask now that a dictation's text is in, at most once per
    /// app and run.
    fn take_after_insertion(&mut self) -> Option<String> {
        let bundle_id = self.pending.take()?;
        self.asked.insert(bundle_id.clone()).then_some(bundle_id)
    }
}

static CONSENT_ASKS: std::sync::LazyLock<Mutex<ConsentAsks>> =
    std::sync::LazyLock::new(Default::default);

/// A dictation's text is in the application in front: shows macOS's dialog
/// for the app noted as not asked yet, if any, on a thread of its own (it
/// waits for the user's answer).
pub fn ask_pending_consent() {
    let pending = CONSENT_ASKS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take_after_insertion();
    let Some(bundle_id) = pending else {
        return;
    };
    log::info!(
        "Window title of {bundle_id}: the text is in; asking macOS once whether DanyWhere may ask it"
    );
    std::thread::spawn(move || {
        let answer = platform::request(&bundle_id);
        log::info!("Window title of {bundle_id}: macOS's answer: {answer:?}");
    });
}

/// What asking an app for its window title by Apple event needs from macOS
/// ([`ask_for_title`]), so the steps can be tested without apps or
/// osascript.
#[cfg(target_os = "macos")]
trait TitlePort {
    /// Where the bundle of the app running as `pid` lies.
    fn bundle_path(&self, pid: i32) -> Option<PathBuf>;
    /// How the app `bundle_id` can be asked for its title, from the
    /// scripting definition in its bundle.
    fn title_query(&self, bundle_id: &str, bundle: &Path) -> Option<TitleQuery>;
    /// The `OSStatus` of asking macOS, without a dialog, whether DanyWhere
    /// may send `bundle_id` a "get data" event (0: allowed).
    fn permission(&self, bundle_id: &str) -> i32;
    /// Runs `script` with osascript, stopping it after `limit`.
    fn run_script(&self, script: &str, limit: Duration) -> ScriptRun;
}

/// How an osascript run ended.
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq, Eq)]
enum ScriptRun {
    Done {
        success: bool,
        stdout: String,
        stderr: String,
    },
    /// Stopped at the time limit.
    TimedOut,
    /// osascript couldn't be started or waited for.
    CouldNotRun(String),
}

/// What asking an app for its window title by Apple event gave: the title,
/// or why there is none.
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq, Eq)]
enum TitleOutcome {
    Title(String),
    /// The app's bundle wasn't found (the app quit meanwhile).
    NoBundle,
    /// The app can't be asked: its scripting definition offers no window
    /// title (or its bundle id can't stand in a script).
    NoQuery,
    /// DanyWhere may not ask it; the `OSStatus` of the permission check
    /// ([`status_from_os`]).
    NotAllowed(i32),
    /// osascript couldn't run.
    CouldNotRun(String),
    /// The script failed, with the error number osascript gave, if any.
    Failed(Option<i32>),
    /// The app didn't answer within the time limit.
    TimedOut,
    /// The app answered with an empty title.
    Empty,
}

#[cfg(target_os = "macos")]
impl TitleOutcome {
    fn into_title(self) -> Option<String> {
        match self {
            TitleOutcome::Title(title) => Some(title),
            _ => None,
        }
    }

    /// What the log says about it: a key that stays the same each time the
    /// app answers this way, and the text, which never holds the title
    /// itself, only its length.
    fn log_reason(&self, limit: Duration) -> (String, String) {
        match self {
            TitleOutcome::Title(title) => (
                "title".to_string(),
                format!("told by Apple event ({} characters)", title.chars().count()),
            ),
            TitleOutcome::NoBundle => (
                "no bundle".to_string(),
                "none, the app's bundle wasn't found".to_string(),
            ),
            TitleOutcome::NoQuery => (
                "no query".to_string(),
                "none, it can't be asked by Apple event: its scripting definition offers \
                 no window title"
                    .to_string(),
            ),
            TitleOutcome::NotAllowed(code) => (
                format!("not allowed {code}"),
                format!("none, not allowed to ask it: {}", not_allowed_reason(*code)),
            ),
            TitleOutcome::CouldNotRun(error) => (
                "could not run".to_string(),
                format!("none, osascript could not run: {error}"),
            ),
            TitleOutcome::Failed(Some(number)) => (
                format!("failed {number}"),
                match apple_event_error(*number) {
                    Some(meaning) => {
                        format!("none, the Apple event failed with error {number} ({meaning})")
                    }
                    None => format!("none, the Apple event failed with error {number}"),
                },
            ),
            TitleOutcome::Failed(None) => (
                "failed".to_string(),
                "none, the Apple event failed without an error number".to_string(),
            ),
            TitleOutcome::TimedOut => (
                "timed out".to_string(),
                format!("none, it didn't answer within {limit:?}"),
            ),
            TitleOutcome::Empty => (
                "empty".to_string(),
                "none, it answered with an empty title".to_string(),
            ),
        }
    }
}

/// The permission check's `OSStatus` `code` by name, with where to change
/// it.
#[cfg(target_os = "macos")]
fn not_allowed_reason(code: i32) -> String {
    match status_from_os(code) {
        AutomationStatus::NotAsked => format!(
            "not asked yet (OSStatus {code}); allow it under Defaults › Post Process › \
             Context from Earlier Dictations › Window Titles while the app runs"
        ),
        AutomationStatus::Denied => format!(
            "denied (OSStatus {code}); change it in System Settings › Privacy & Security \
             › Automation"
        ),
        AutomationStatus::NotRunning => format!("not running (OSStatus {code})"),
        AutomationStatus::Allowed => format!("allowed (OSStatus {code})"),
        AutomationStatus::Unknown => format!("other (OSStatus {code})"),
    }
}

/// What an error number of an Apple event or of osascript means, for the
/// ones a title query can meet.
#[cfg(target_os = "macos")]
fn apple_event_error(number: i32) -> Option<&'static str> {
    Some(match number {
        -600 => "procNotFound: the app isn't running",
        -609 => "connectionInvalid: the connection to the app broke off",
        -1700 => "errAECoercionFail: the answer can't be made into text",
        -1708 => "errAEEventNotHandled: the app doesn't understand the request",
        -1712 => "errAETimeout: the app took too long",
        -1719 => "errAEIllegalIndex: no such element",
        -1728 => "errAENoSuchObject: no such object, e.g. no window in front",
        -1743 => "errAEEventNotPermitted: not permitted",
        -1744 => "errAEEventWouldRequireUserConsent: needs the user's consent",
        -2740 => "errOSASyntaxError: the script doesn't compile",
        -2741 => "errOSASyntaxTypeError: the script doesn't compile for this app",
        -10000 => "errAEEventFailed: the app couldn't handle the request",
        -10004 => "errAEPrivilegeError: privilege violation",
        _ => return None,
    })
}

/// Asks the app `bundle_id` running as `pid` for its window title through
/// `port`, within `limit`; never shows the consent dialog. An app that can't
/// be asked isn't checked for permission, and one DanyWhere may not ask gets
/// no script.
#[cfg(target_os = "macos")]
fn ask_for_title(
    port: &impl TitlePort,
    bundle_id: &str,
    pid: i32,
    limit: Duration,
) -> TitleOutcome {
    let Some(bundle) = port.bundle_path(pid) else {
        return TitleOutcome::NoBundle;
    };
    let Some(query) = port.title_query(bundle_id, &bundle) else {
        return TitleOutcome::NoQuery;
    };
    let Some(script) = title_query::script(query, bundle_id) else {
        return TitleOutcome::NoQuery;
    };
    let permission = port.permission(bundle_id);
    if status_from_os(permission) != AutomationStatus::Allowed {
        return TitleOutcome::NotAllowed(permission);
    }
    match port.run_script(&script, limit) {
        ScriptRun::Done {
            success: true,
            stdout,
            ..
        } => {
            let title = stdout.trim();
            if title.is_empty() {
                TitleOutcome::Empty
            } else {
                TitleOutcome::Title(title.to_string())
            }
        }
        ScriptRun::Done {
            success: false,
            stderr,
            ..
        } => TitleOutcome::Failed(error_number(&stderr)),
        ScriptRun::TimedOut => TitleOutcome::TimedOut,
        ScriptRun::CouldNotRun(error) => TitleOutcome::CouldNotRun(error),
    }
}

/// The error number osascript ends its message with, as in "0:42: execution
/// error: Can’t get window 1 of application \"Terminal\". (-1728)": the
/// last line that ends with one.
#[cfg(target_os = "macos")]
fn error_number(stderr: &str) -> Option<i32> {
    stderr.lines().rev().find_map(|line| {
        let inner = line.trim_end().strip_suffix(')')?;
        let open = inner.rfind('(')?;
        inner[open + 1..].parse().ok()
    })
}

/// The reasons already logged on INFO in this run, by bundle id, so the log
/// tells each app's reason once and not at every dictation.
#[derive(Default)]
#[cfg(target_os = "macos")]
struct LoggedReasons {
    seen: std::collections::HashSet<(String, String)>,
}

#[cfg(target_os = "macos")]
impl LoggedReasons {
    /// More entries than this start the set afresh.
    const LIMIT: usize = 256;

    /// Whether `bundle_id` gives the reason `key` for the first time in this
    /// run; remembers it.
    fn first_time(&mut self, bundle_id: &str, key: &str) -> bool {
        let entry = (bundle_id.to_string(), key.to_string());
        if self.seen.contains(&entry) {
            return false;
        }
        if self.seen.len() >= Self::LIMIT {
            self.seen.clear();
        }
        self.seen.insert(entry)
    }
}

#[cfg(target_os = "macos")]
static LOGGED_REASONS: std::sync::LazyLock<Mutex<LoggedReasons>> =
    std::sync::LazyLock::new(Default::default);

/// The log level and line for `outcome` of asking `bundle_id` within
/// `limit`: INFO the first time the app gives this reason in this run,
/// DEBUG after that.
#[cfg(target_os = "macos")]
fn title_log(
    logged: &Mutex<LoggedReasons>,
    bundle_id: &str,
    outcome: &TitleOutcome,
    limit: Duration,
) -> (log::Level, String) {
    let (key, text) = outcome.log_reason(limit);
    let first = logged
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .first_time(bundle_id, &key);
    let level = if first {
        log::Level::Info
    } else {
        log::Level::Debug
    };
    (level, format!("Window title of {bundle_id}: {text}"))
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::c_void;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use objc2_app_kit::{NSApplicationActivationPolicy, NSRunningApplication, NSWorkspace};
    use objc2_foundation::{NSBundle, NSString};

    use super::{
        ask_for_title, frontmost_window_of, prime_front, status_from_os, title_log, title_source,
        AccessibilityState, AutomationApp, AutomationStatus, ListedWindow, ScreenRect, ScriptRun,
        TitlePort, TitleSource, WantsTitle, WindowTitleAccess, ANSWERED, LOGGED_REASONS,
    };
    use crate::dictation_context;
    use crate::title_query::{self, Bundle, TitleQuery};

    /// How long a capture's Apple event may take.
    const TITLE_LIMIT: Duration = Duration::from_millis(700);
    /// How long the question ahead may take: it runs while the user speaks.
    const PRIME_LIMIT: Duration = Duration::from_secs(5);

    type CFTypeRef = *const c_void;
    type AXUIElementRef = *const c_void;
    type CFArrayRef = *const c_void;
    type CFDictionaryRef = *const c_void;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CGSize {
        width: f64,
        height: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CGRect {
        origin: CGPoint,
        size: CGSize,
    }

    /// `kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements`.
    const ON_SCREEN_WITHOUT_DESKTOP: u32 = (1 << 0) | (1 << 4);
    /// `kCGNullWindowID`: every window, not just those above or below one.
    const NULL_WINDOW_ID: u32 = 0;
    /// `kCFNumberSInt32Type`.
    const CF_NUMBER_SINT32: isize = 3;
    /// `kCFNumberFloat64Type`.
    const CF_NUMBER_FLOAT64: isize = 6;

    #[link(name = "CoreGraphics", kind = "framework")]
    #[allow(non_upper_case_globals)]
    extern "C" {
        static kCGWindowOwnerPID: CFTypeRef;
        static kCGWindowLayer: CFTypeRef;
        static kCGWindowAlpha: CFTypeRef;
        static kCGWindowBounds: CFTypeRef;
        fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> CFArrayRef;
        fn CGRectMakeWithDictionaryRepresentation(dict: CFDictionaryRef, rect: *mut CGRect)
            -> bool;
        fn CGEventCreate(source: CFTypeRef) -> CFTypeRef;
        fn CGEventGetLocation(event: CFTypeRef) -> CGPoint;
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: CFTypeRef,
            value: *mut CFTypeRef,
        ) -> i32;
        fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, seconds: f32) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: CFTypeRef);
        fn CFGetTypeID(cf: CFTypeRef) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFNumberGetTypeID() -> usize;
        fn CFDictionaryGetTypeID() -> usize;
        fn CFArrayGetCount(array: CFArrayRef) -> isize;
        fn CFArrayGetValueAtIndex(array: CFArrayRef, index: isize) -> CFTypeRef;
        fn CFDictionaryGetValue(dict: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef;
        fn CFNumberGetValue(number: CFTypeRef, number_type: isize, value: *mut c_void) -> u8;
    }

    #[repr(C)]
    struct AEDesc {
        descriptor_type: u32,
        data_handle: *mut c_void,
    }

    #[link(name = "CoreServices", kind = "framework")]
    extern "C" {
        fn AECreateDesc(
            type_code: u32,
            data_ptr: *const c_void,
            data_size: isize,
            result: *mut AEDesc,
        ) -> i16;
        fn AEDisposeDesc(desc: *mut AEDesc) -> i16;
        fn AEDeterminePermissionToAutomateTarget(
            target: *const AEDesc,
            event_class: u32,
            event_id: u32,
            ask_user_if_needed: u8,
        ) -> i32;
    }

    const fn four_cc(code: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*code)
    }

    /// Whether DanyWhere may send the app `bundle_id` a "get data" event.
    fn automation_permission(bundle_id: &str, ask: bool) -> AutomationStatus {
        status_from_os(automation_permission_code(bundle_id, ask))
    }

    /// [`automation_permission`] as the `OSStatus` macOS gave.
    fn automation_permission_code(bundle_id: &str, ask: bool) -> i32 {
        let mut desc = AEDesc {
            descriptor_type: 0,
            data_handle: std::ptr::null_mut(),
        };
        // SAFETY: AECreateDesc copies `bundle_id`'s bytes into `desc`, which
        // is disposed of below.
        let created = unsafe {
            AECreateDesc(
                four_cc(b"bund"),
                bundle_id.as_ptr().cast(),
                bundle_id.len() as isize,
                &mut desc,
            )
        };
        if created != 0 {
            return i32::from(created);
        }
        // SAFETY: `desc` is a valid descriptor created above.
        let status = unsafe {
            AEDeterminePermissionToAutomateTarget(
                &desc,
                four_cc(b"core"),
                four_cc(b"getd"),
                u8::from(ask),
            )
        };
        // SAFETY: disposing of the descriptor created above.
        unsafe {
            AEDisposeDesc(&mut desc);
        }
        status
    }

    /// The frontmost app's id, name and process id, unless it is DanyWhere.
    fn frontmost_app_with_pid() -> Option<(String, String, i32)> {
        let workspace = NSWorkspace::sharedWorkspace();
        let app = workspace.frontmostApplication()?;
        let bundle_id = app.bundleIdentifier()?.to_string();
        let name = app
            .localizedName()
            .map(|n| n.to_string())
            .unwrap_or_else(|| bundle_id.clone());
        let pid = app.processIdentifier();
        if pid == std::process::id() as i32 {
            return None;
        }
        Some((bundle_id, name, pid))
    }

    pub fn frontmost_app() -> Option<(String, String)> {
        frontmost_app_with_pid().map(|(bundle_id, name, _)| (bundle_id, name))
    }

    /// The frontmost app's process id. Unlike [`frontmost_app_with_pid`] it
    /// includes DanyWhere: its own settings window can be the one in front.
    fn frontmost_pid() -> Option<i32> {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        Some(app.processIdentifier())
    }

    pub fn frontmost_window_frame() -> Option<ScreenRect> {
        let pid = frontmost_pid()?;
        frontmost_window_of(&on_screen_windows(), pid)
    }

    /// The windows on screen, front to back, without the desktop.
    fn on_screen_windows() -> Vec<ListedWindow> {
        // SAFETY: the list comes from a Copy function, so it is ours; its
        // entries are read while it lives and it is released at the end.
        unsafe {
            let list = CGWindowListCopyWindowInfo(ON_SCREEN_WITHOUT_DESKTOP, NULL_WINDOW_ID);
            if list.is_null() {
                return Vec::new();
            }
            let windows = (0..CFArrayGetCount(list))
                .filter_map(|index| listed_window(CFArrayGetValueAtIndex(list, index)))
                .collect();
            CFRelease(list);
            windows
        }
    }

    /// One entry of the window list, or `None` when it lacks a field.
    ///
    /// # Safety
    ///
    /// `entry` must be null or a CFDictionary that lives for the call.
    unsafe fn listed_window(entry: CFDictionaryRef) -> Option<ListedWindow> {
        if entry.is_null() || CFGetTypeID(entry) != CFDictionaryGetTypeID() {
            return None;
        }
        let owner_pid = number_i32(CFDictionaryGetValue(entry, kCGWindowOwnerPID))?;
        let layer = number_i32(CFDictionaryGetValue(entry, kCGWindowLayer))?;
        let alpha = number_f64(CFDictionaryGetValue(entry, kCGWindowAlpha)).unwrap_or(1.0);
        let bounds = CFDictionaryGetValue(entry, kCGWindowBounds);
        if bounds.is_null() || CFGetTypeID(bounds) != CFDictionaryGetTypeID() {
            return None;
        }
        let mut rect = CGRect::default();
        if !CGRectMakeWithDictionaryRepresentation(bounds, &mut rect) {
            return None;
        }
        Some(ListedWindow {
            owner_pid,
            layer,
            alpha,
            bounds: ScreenRect {
                x: rect.origin.x,
                y: rect.origin.y,
                width: rect.size.width,
                height: rect.size.height,
            },
        })
    }

    /// # Safety
    ///
    /// `number` must be null or a CF object that lives for the call.
    unsafe fn number_i32(number: CFTypeRef) -> Option<i32> {
        if number.is_null() || CFGetTypeID(number) != CFNumberGetTypeID() {
            return None;
        }
        let mut value = 0i32;
        let exact = CFNumberGetValue(number, CF_NUMBER_SINT32, (&mut value as *mut i32).cast());
        (exact != 0).then_some(value)
    }

    /// # Safety
    ///
    /// `number` must be null or a CF object that lives for the call.
    unsafe fn number_f64(number: CFTypeRef) -> Option<f64> {
        if number.is_null() || CFGetTypeID(number) != CFNumberGetTypeID() {
            return None;
        }
        let mut value = 0f64;
        let exact = CFNumberGetValue(number, CF_NUMBER_FLOAT64, (&mut value as *mut f64).cast());
        (exact != 0).then_some(value)
    }

    /// The mouse pointer, from the location of an empty event, which needs
    /// no permission.
    pub fn cursor_location() -> Option<(f64, f64)> {
        // SAFETY: the event is created here and released after reading it.
        unsafe {
            let event = CGEventCreate(std::ptr::null());
            if event.is_null() {
                return None;
            }
            let point = CGEventGetLocation(event);
            CFRelease(event);
            Some((point.x, point.y))
        }
    }

    pub fn running_applications() -> Vec<(String, String)> {
        let own = std::process::id() as i32;
        NSWorkspace::sharedWorkspace()
            .runningApplications()
            .iter()
            .filter(|app| {
                app.activationPolicy() == NSApplicationActivationPolicy::Regular
                    && app.processIdentifier() != own
            })
            .filter_map(|app| {
                let bundle_id = app.bundleIdentifier()?.to_string();
                let name = app
                    .localizedName()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| bundle_id.clone());
                Some((bundle_id, name))
            })
            .collect()
    }

    pub fn installed_applications() -> Vec<(String, String)> {
        installed_bundles()
            .into_iter()
            .map(|bundle| (bundle.id, bundle.name))
            .collect()
    }

    /// The running applications with a window in the Dock, with where their
    /// bundles lie.
    fn running_bundles() -> Vec<Bundle> {
        let own = std::process::id() as i32;
        NSWorkspace::sharedWorkspace()
            .runningApplications()
            .iter()
            .filter(|app| {
                app.activationPolicy() == NSApplicationActivationPolicy::Regular
                    && app.processIdentifier() != own
            })
            .filter_map(|app| {
                let id = app.bundleIdentifier()?.to_string();
                let path = PathBuf::from(app.bundleURL()?.path()?.to_string());
                let name = app
                    .localizedName()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| id.clone());
                Some(Bundle { id, name, path })
            })
            .collect()
    }

    /// Where the bundle of the running app with process id `pid` lies.
    fn bundle_path_of(pid: i32) -> Option<PathBuf> {
        let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?;
        Some(PathBuf::from(app.bundleURL()?.path()?.to_string()))
    }

    /// The installed applications in the usual folders, with their bundles.
    fn installed_bundles() -> Vec<Bundle> {
        let mut folders = vec![
            std::path::PathBuf::from("/Applications"),
            std::path::PathBuf::from("/Applications/Utilities"),
            std::path::PathBuf::from("/System/Applications"),
            std::path::PathBuf::from("/System/Applications/Utilities"),
        ];
        if let Some(home) = std::env::var_os("HOME") {
            folders.push(std::path::PathBuf::from(home).join("Applications"));
        }
        let mut apps = Vec::new();
        for folder in folders {
            let Ok(entries) = std::fs::read_dir(&folder) else {
                continue;
            };
            for path in entries.flatten().map(|entry| entry.path()) {
                if path.extension().is_none_or(|ext| ext != "app") {
                    continue;
                }
                let Some(bundle_id) = path
                    .to_str()
                    .and_then(|p| NSBundle::bundleWithPath(&NSString::from_str(p)))
                    .and_then(|bundle| bundle.bundleIdentifier())
                else {
                    continue;
                };
                let name = path
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().to_string())
                    .unwrap_or_else(|| bundle_id.to_string());
                apps.push(Bundle {
                    id: bundle_id.to_string(),
                    name,
                    path,
                });
            }
        }
        apps
    }

    /// The frontmost app's id and name, and the title of its window if
    /// DanyWhere may learn it and `wants_title` allows asking; `access`
    /// says how ([`title_source`]).
    pub fn frontmost(
        wants_title: WantsTitle,
        access: AccessibilityState,
    ) -> Option<(String, String, Option<String>)> {
        let (bundle_id, name, pid) = frontmost_app_with_pid()?;
        let title = if !wants_title(&bundle_id, &name) {
            None
        } else {
            match title_source(&access) {
                TitleSource::Accessibility => accessibility_title(pid),
                TitleSource::AppleEvents => automation_title(&bundle_id, pid),
            }
        };
        Some((bundle_id, name, title))
    }

    /// The focused window's title through the Accessibility API.
    fn accessibility_title(pid: i32) -> Option<String> {
        // SAFETY: AX calls on an element created here and released below;
        // the attribute names are CFStrings bridged from NSString.
        unsafe {
            let app = AXUIElementCreateApplication(pid);
            if app.is_null() {
                return None;
            }
            AXUIElementSetMessagingTimeout(app, 0.4);
            let focused_window = NSString::from_str("AXFocusedWindow");
            let mut window: CFTypeRef = std::ptr::null();
            let error = AXUIElementCopyAttributeValue(
                app,
                (&*focused_window as *const NSString).cast(),
                &mut window,
            );
            CFRelease(app);
            if error != 0 || window.is_null() {
                return None;
            }
            let title_attribute = NSString::from_str("AXTitle");
            let mut title: CFTypeRef = std::ptr::null();
            let error = AXUIElementCopyAttributeValue(
                window,
                (&*title_attribute as *const NSString).cast(),
                &mut title,
            );
            CFRelease(window);
            if error != 0 || title.is_null() {
                return None;
            }
            if CFGetTypeID(title) != CFStringGetTypeID() {
                CFRelease(title);
                return None;
            }
            // A CFString is an NSString; take over the +1 reference.
            let title: objc2::rc::Retained<NSString> =
                objc2::rc::Retained::from_raw(title as *mut NSString)?;
            Some(title.to_string())
        }
    }

    /// The title by Apple event, for a scriptable app DanyWhere may already
    /// control; never shows the consent dialog. How to ask the app comes from
    /// its scripting definition, usually from the cache [`warm_up`] filled.
    fn automation_title(bundle_id: &str, pid: i32) -> Option<String> {
        let title = query_title(bundle_id, pid, TITLE_LIMIT)?;
        ANSWERED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .mark(bundle_id, pid);
        Some(title)
    }

    /// The window title of the app `bundle_id` running as `pid` by Apple
    /// event, if it answers within `limit`; `None` without permission, query
    /// or title. The log tells why there is none ([`title_log`]); an app
    /// not asked yet gets macOS's dialog once the dictation's text is in it
    /// ([`super::ask_pending_consent`]).
    fn query_title(bundle_id: &str, pid: i32, limit: Duration) -> Option<String> {
        let outcome = ask_for_title(&Mac, bundle_id, pid, limit);
        let (level, line) = title_log(&LOGGED_REASONS, bundle_id, &outcome, limit);
        log::log!(level, "{line}");
        if let super::TitleOutcome::NotAllowed(code) = outcome {
            if status_from_os(code) == AutomationStatus::NotAsked {
                super::CONSENT_ASKS
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .note_not_asked(bundle_id);
            }
        }
        outcome.into_title()
    }

    /// This Mac, as [`ask_for_title`] asks it.
    struct Mac;

    impl TitlePort for Mac {
        fn bundle_path(&self, pid: i32) -> Option<PathBuf> {
            bundle_path_of(pid)
        }

        fn title_query(&self, bundle_id: &str, bundle: &Path) -> Option<TitleQuery> {
            title_query::title_query(bundle_id, bundle)
        }

        fn permission(&self, bundle_id: &str) -> i32 {
            automation_permission_code(bundle_id, false)
        }

        fn run_script(&self, script: &str, limit: Duration) -> ScriptRun {
            run_osascript(script, limit)
        }
    }

    /// Runs `script` with osascript, which runs on DanyWhere's behalf (macOS
    /// asks about DanyWhere, not osascript) and is stopped when the app
    /// doesn't answer within `limit`.
    fn run_osascript(script: &str, limit: Duration) -> ScriptRun {
        let mut child = match Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(script)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => return ScriptRun::CouldNotRun(error.to_string()),
        };
        let deadline = Instant::now() + limit;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(15))
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return ScriptRun::TimedOut;
                }
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return ScriptRun::CouldNotRun(error.to_string());
                }
            }
        };
        fn read_all(pipe: Option<impl std::io::Read>) -> String {
            let mut text = String::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_string(&mut text);
            }
            text
        }
        ScriptRun::Done {
            success: status.success(),
            stdout: read_all(child.stdout.take()),
            stderr: read_all(child.stderr.take()),
        }
    }

    /// Through Accessibility ([`title_source`]) every title comes from it;
    /// otherwise the apps that can tell theirs by Apple event, running ones with their permission
    /// (asked without a dialog).
    pub fn access(own_id: &str, access: AccessibilityState) -> WindowTitleAccess {
        let accessibility = title_source(&access) == TitleSource::Accessibility;
        let apps = if accessibility {
            Vec::new()
        } else {
            title_query::title_apps(
                running_bundles(),
                installed_bundles(),
                own_id,
                title_query::title_query,
            )
            .into_iter()
            .map(|app| AutomationApp {
                status: if app.running {
                    automation_permission(&app.id, false)
                } else {
                    AutomationStatus::NotRunning
                },
                bundle_id: app.id,
                name: app.name,
                running: app.running,
            })
            .collect()
        };
        WindowTitleAccess {
            supported: true,
            accessibility,
            apps,
        }
    }

    /// Asks the user whether DanyWhere may control `bundle_id`: only for a
    /// running app that can tell its window title.
    pub fn request(bundle_id: &str) -> AutomationStatus {
        if !title_query::valid_bundle_id(bundle_id) {
            return AutomationStatus::Unknown;
        }
        let Some(bundle) = running_bundles()
            .into_iter()
            .find(|bundle| bundle.id == bundle_id)
        else {
            return AutomationStatus::NotRunning;
        };
        if title_query::title_query(bundle_id, &bundle.path).is_none() {
            return AutomationStatus::Unknown;
        }
        automation_permission(bundle_id, true)
    }

    /// Without Accessibility, reads in the background how to ask the running
    /// and installed apps for their window title, so dictations find it in
    /// the cache. With Accessibility nothing is needed.
    /// Asks the app in front ahead for its window title (see
    /// [`super::prime_title_query`]): only without Accessibility, only an
    /// app that isn't excluded and hasn't answered yet.
    pub fn prime(excluded: &[String], access: AccessibilityState) {
        if title_source(&access) == TitleSource::Accessibility {
            return;
        }
        let not_excluded =
            |id: &str, name: &str| !dictation_context::is_excluded_app(id, name, excluded);
        let started = Instant::now();
        let mut asked = String::new();
        let answered = prime_front(
            frontmost_app_with_pid,
            &not_excluded,
            &ANSWERED,
            |id, pid| {
                asked = id.to_string();
                query_title(id, pid, PRIME_LIMIT).is_some()
            },
        );
        if let Some(answered) = answered {
            log::debug!(
                "Asked {asked} ahead for its window title in {:?} (answered: {answered})",
                started.elapsed()
            );
        }
    }

    pub fn warm_up(access: AccessibilityState) {
        if title_source(&access) == TitleSource::Accessibility {
            return;
        }
        std::thread::Builder::new()
            .name("title-query-warm-up".into())
            .spawn(|| {
                let started = Instant::now();
                let bundles: Vec<Bundle> = running_bundles()
                    .into_iter()
                    .chain(installed_bundles())
                    .collect();
                let found = bundles
                    .iter()
                    .filter(|bundle| title_query::title_query(&bundle.id, &bundle.path).is_some())
                    .count();
                log::debug!(
                    "{found} of {} apps can tell their window title by Apple event (read in {:?})",
                    bundles.len(),
                    started.elapsed()
                );
            })
            .map_err(|e| log::warn!("Could not start reading the apps' scripting definitions: {e}"))
            .ok();
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{AccessibilityState, AutomationStatus, ScreenRect, WantsTitle, WindowTitleAccess};
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, MAX_PATH, POINT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONULL,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
        GetWindowThreadProcessId,
    };

    pub fn frontmost_app() -> Option<(String, String)> {
        foreground(&|_: &str, _: &str| false).map(|(exe, name, _)| (exe, name))
    }

    /// The bounds of the monitor Windows puts the foreground window on, in
    /// physical pixels, which match the monitor's own bounds exactly.
    pub fn frontmost_window_frame() -> Option<ScreenRect> {
        // SAFETY: Win32 calls on the foreground window handle; `info` is
        // written by GetMonitorInfoW, which reads its size from `cbSize`.
        unsafe {
            let window = GetForegroundWindow();
            if window.0.is_null() {
                return None;
            }
            let monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONULL);
            if monitor.is_invalid() {
                return None;
            }
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                return None;
            }
            let bounds = info.rcMonitor;
            Some(ScreenRect {
                x: bounds.left as f64,
                y: bounds.top as f64,
                width: (bounds.right - bounds.left) as f64,
                height: (bounds.bottom - bounds.top) as f64,
            })
        }
    }

    /// The mouse pointer in physical pixels.
    pub fn cursor_location() -> Option<(f64, f64)> {
        let mut point = POINT::default();
        // SAFETY: GetCursorPos writes into `point`.
        unsafe { GetCursorPos(&mut point) }.ok()?;
        Some((point.x as f64, point.y as f64))
    }

    pub fn running_applications() -> Vec<(String, String)> {
        Vec::new()
    }

    pub fn installed_applications() -> Vec<(String, String)> {
        Vec::new()
    }

    /// The foreground window's program (exe file name), its name, and the
    /// window's title when `wants_title` allows it. Windows needs no
    /// permission for it, so `_access` doesn't matter here.
    pub fn frontmost(
        wants_title: WantsTitle,
        _access: AccessibilityState,
    ) -> Option<(String, String, Option<String>)> {
        foreground(wants_title)
    }

    /// [`frontmost`] without the Accessibility state, for
    /// [`frontmost_app`], which has no settings to build one from.
    fn foreground(wants_title: WantsTitle) -> Option<(String, String, Option<String>)> {
        // SAFETY: Win32 calls on the foreground window handle and a process
        // handle opened and closed here.
        unsafe {
            let window = GetForegroundWindow();
            if window.0.is_null() {
                return None;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(window, Some(&mut pid as *mut u32));
            if pid == 0 || pid == std::process::id() {
                return None;
            }
            let length = GetWindowTextLengthW(window);
            let title = if length > 0 {
                let mut buffer = vec![0u16; length as usize + 1];
                let copied = GetWindowTextW(window, &mut buffer);
                Some(String::from_utf16_lossy(&buffer[..copied.max(0) as usize]))
            } else {
                None
            };
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buffer = vec![0u16; MAX_PATH as usize * 4];
            let mut size = buffer.len() as u32;
            let named = QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &mut size,
            );
            let _ = CloseHandle(process);
            named.ok()?;
            let path = String::from_utf16_lossy(&buffer[..size as usize]);
            let exe = std::path::Path::new(&path)
                .file_name()?
                .to_string_lossy()
                .to_string();
            let name = std::path::Path::new(&exe)
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_string())
                .unwrap_or_else(|| exe.clone());
            let title = title.filter(|_| wants_title(&exe, &name));
            Some((exe, name, title))
        }
    }

    pub fn access(_own_id: &str, _access: AccessibilityState) -> WindowTitleAccess {
        WindowTitleAccess {
            supported: true,
            accessibility: true,
            apps: Vec::new(),
        }
    }

    /// Windows needs no permission for titles, so nothing to read ahead.
    pub fn warm_up(_access: AccessibilityState) {}

    pub fn prime(_excluded: &[String], _access: AccessibilityState) {}

    pub fn request(_bundle_id: &str) -> AutomationStatus {
        AutomationStatus::Unknown
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    use super::{AccessibilityState, AutomationStatus, ScreenRect, WantsTitle, WindowTitleAccess};

    /// Linux: not yet, so dictations there have no context.
    pub fn frontmost(
        _wants_title: WantsTitle,
        _access: AccessibilityState,
    ) -> Option<(String, String, Option<String>)> {
        None
    }

    /// Linux: not yet; the overlay asks Enigo for the cursor instead.
    pub fn frontmost_window_frame() -> Option<ScreenRect> {
        None
    }

    pub fn cursor_location() -> Option<(f64, f64)> {
        None
    }

    pub fn frontmost_app() -> Option<(String, String)> {
        None
    }

    pub fn running_applications() -> Vec<(String, String)> {
        Vec::new()
    }

    pub fn installed_applications() -> Vec<(String, String)> {
        Vec::new()
    }

    pub fn access(_own_id: &str, _access: AccessibilityState) -> WindowTitleAccess {
        WindowTitleAccess {
            supported: false,
            accessibility: false,
            apps: Vec::new(),
        }
    }

    pub fn warm_up(_access: AccessibilityState) {}

    pub fn prime(_excluded: &[String], _access: AccessibilityState) {}

    pub fn request(_bundle_id: &str) -> AutomationStatus {
        AutomationStatus::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accessibility::testing::{with_choice, with_system_trusted};
    use std::cell::Cell;

    /// The app in front as `capture_front` gets it: it records whether its
    /// title was asked for, as an Apple event or Accessibility query would be.
    fn front_app<'a>(
        id: &'a str,
        name: &'a str,
        asked: &'a Cell<bool>,
    ) -> impl FnOnce(WantsTitle) -> Option<(String, String, Option<String>)> + 'a {
        move |wants_title| {
            let title = wants_title(id, name).then(|| {
                asked.set(true);
                "Notes about the trip".to_string()
            });
            Some((id.to_string(), name.to_string(), title))
        }
    }

    /// [`title_source`] for the user's choice `skipped` and macOS answering
    /// `trusted`.
    fn source_for(skipped: bool, trusted: bool) -> TitleSource {
        let settings = with_choice(crate::settings::get_default_settings(), skipped);
        with_system_trusted(trusted, || {
            title_source(&crate::accessibility::state(&settings))
        })
    }

    #[test]
    fn titles_come_through_accessibility_where_macos_grants_it() {
        assert_eq!(source_for(false, true), TitleSource::Accessibility);
    }

    #[test]
    fn titles_come_by_apple_event_where_macos_refuses() {
        assert_eq!(source_for(false, false), TitleSource::AppleEvents);
        assert_eq!(source_for(true, false), TitleSource::AppleEvents);
    }

    #[test]
    fn titles_come_by_apple_event_when_the_user_chose_to_go_without() {
        // Also once macOS grants Accessibility after onboarding: the mode
        // stays the mode, as Enigo stays off (analysis 2026-11-22, example
        // 2).
        assert_eq!(source_for(true, true), TitleSource::AppleEvents);
    }

    #[test]
    fn an_excluded_app_is_never_asked_for_its_title() {
        let excluded = vec!["com.example.vault".to_string(), "Mail".to_string()];
        for (id, name) in [("com.example.vault", "Vault"), ("com.apple.mail", "Mail")] {
            let asked = Cell::new(false);
            assert_eq!(capture_front(front_app(id, name, &asked), &excluded), None);
            assert!(!asked.get(), "{name} was asked for its title");
        }
    }

    /// `prime_front` with TextEdit in front as process `pid`, counting the
    /// questions and answering with `answers`.
    fn prime_textedit(
        pid: i32,
        excluded: bool,
        answered: &Mutex<AnsweredApps>,
        asked: &Cell<u32>,
        answers: bool,
    ) -> Option<bool> {
        let wants = move |_: &str, _: &str| !excluded;
        prime_front(
            || {
                Some((
                    "com.apple.TextEdit".to_string(),
                    "TextEdit".to_string(),
                    pid,
                ))
            },
            &wants,
            answered,
            |_, _| {
                asked.set(asked.get() + 1);
                answers
            },
        )
    }

    #[test]
    fn a_new_app_is_asked_ahead_until_it_has_answered() {
        let answered = Mutex::new(AnsweredApps::default());
        let asked = Cell::new(0);
        assert_eq!(
            prime_textedit(42, false, &answered, &asked, true),
            Some(true)
        );
        assert_eq!(prime_textedit(42, false, &answered, &asked, true), None);
        assert_eq!(asked.get(), 1, "an app that answered was asked again");
    }

    #[test]
    fn an_app_that_did_not_answer_is_asked_ahead_again() {
        let answered = Mutex::new(AnsweredApps::default());
        let asked = Cell::new(0);
        assert_eq!(
            prime_textedit(42, false, &answered, &asked, false),
            Some(false)
        );
        assert_eq!(
            prime_textedit(42, false, &answered, &asked, true),
            Some(true)
        );
        assert_eq!(asked.get(), 2);
    }

    #[test]
    fn an_excluded_app_is_not_asked_ahead() {
        let answered = Mutex::new(AnsweredApps::default());
        let asked = Cell::new(0);
        assert_eq!(prime_textedit(42, true, &answered, &asked, true), None);
        assert_eq!(asked.get(), 0);
    }

    #[test]
    fn a_new_process_of_the_app_is_asked_ahead_again() {
        let answered = Mutex::new(AnsweredApps::default());
        let asked = Cell::new(0);
        assert_eq!(
            prime_textedit(42, false, &answered, &asked, true),
            Some(true)
        );
        assert_eq!(
            prime_textedit(43, false, &answered, &asked, true),
            Some(true)
        );
        assert_eq!(asked.get(), 2);
    }

    #[test]
    fn the_answered_apps_stay_few() {
        let mut answered = AnsweredApps::default();
        for pid in 0..=(AnsweredApps::LIMIT as i32) {
            answered.mark("com.example.app", pid);
        }
        assert!(answered.apps.len() <= AnsweredApps::LIMIT);
        let newest = AnsweredApps::LIMIT as i32;
        assert!(!answered.needs_priming("com.example.app", newest));
        assert!(answered.needs_priming("com.example.app", newest + 1));
    }

    #[test]
    fn an_app_that_is_not_excluded_is_asked_for_its_title() {
        let excluded = vec!["com.example.vault".to_string()];
        let asked = Cell::new(false);
        let captured = capture_front(front_app("com.apple.Notes", "Notes", &asked), &excluded);
        assert!(asked.get());
        let captured = captured.expect("a window");
        assert_eq!(captured.app_id, "com.apple.Notes");
        assert_eq!(captured.title, "Notes about the trip");
    }

    fn window(title: &str) -> CapturedWindow {
        CapturedWindow {
            app_id: "com.googlecode.iterm2".to_string(),
            app_name: "iTerm2".to_string(),
            title: title.to_string(),
        }
    }

    #[test]
    fn the_os_status_tells_whether_danywhere_may_ask() {
        assert_eq!(status_from_os(0), AutomationStatus::Allowed);
        assert_eq!(status_from_os(-1744), AutomationStatus::NotAsked);
        assert_eq!(status_from_os(-1743), AutomationStatus::Denied);
        assert_eq!(status_from_os(-600), AutomationStatus::NotRunning);
        assert_eq!(status_from_os(-50), AutomationStatus::Unknown);
    }

    #[test]
    fn a_finished_capture_is_taken_once() {
        let slot = CaptureSlot::new();
        slot.begin(7);
        slot.finish(7, Some(window("Release")));
        assert_eq!(slot.take(7, Duration::ZERO), Some(window("Release")));
        assert_eq!(slot.take(7, Duration::ZERO), None);
    }

    #[test]
    fn taking_waits_for_a_capture_under_way() {
        let slot = std::sync::Arc::new(CaptureSlot::new());
        slot.begin(3);
        let writer = std::sync::Arc::clone(&slot);
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            writer.finish(3, Some(window("Later")));
        });
        assert_eq!(slot.take(3, Duration::from_secs(2)), Some(window("Later")));
        handle.join().unwrap();
    }

    #[test]
    fn an_older_sessions_capture_doesnt_count() {
        let slot = CaptureSlot::new();
        slot.begin(1);
        slot.begin(2);
        slot.finish(1, Some(window("Old")));
        assert_eq!(slot.take(2, Duration::ZERO), None);
        slot.finish(2, Some(window("New")));
        assert_eq!(slot.take(1, Duration::ZERO), None);
    }

    fn listed(owner_pid: i32, layer: i32, x: f64, width: f64) -> ListedWindow {
        ListedWindow {
            owner_pid,
            layer,
            alpha: 1.0,
            bounds: ScreenRect {
                x,
                y: 40.0,
                width,
                height: 600.0,
            },
        }
    }

    /// The list runs front to back: the first normal window of the app wins,
    /// past other apps' windows and the app's own menus and status items.
    #[test]
    fn the_frontmost_normal_window_of_the_app_gives_the_frame() {
        let windows = [
            listed(400, 25, 0.0, 30.0),    // a status item of another app
            listed(77, 101, 10.0, 200.0),  // the app's open menu
            listed(500, 0, 20.0, 900.0),   // another app's window in front
            listed(77, 0, 1600.0, 1000.0), // the app's frontmost window
            listed(77, 0, 30.0, 800.0),    // the app's window behind it
        ];
        assert_eq!(
            frontmost_window_of(&windows, 77),
            Some(windows[3].bounds),
            "the app's first window, past menus and status items"
        );
        assert_eq!(frontmost_window_of(&windows, 500), Some(windows[2].bounds));
    }

    /// A dialog in front is a modal panel (layer 8, as a real run with a
    /// Finder dialog showed on 2026-11-16), a palette a floating window
    /// (layer 3): either is the window in front. The overlay itself (status
    /// level, 25) and the Dock (20) are not.
    #[test]
    fn a_dialog_or_floating_window_in_front_gives_the_frame() {
        let windows = [
            listed(77, 25, 0.0, 400.0),    // DanyWhere's overlay, status level
            listed(77, 8, 546.0, 420.0),   // the app's dialog
            listed(77, 0, 1600.0, 1000.0), // the app's window behind it
        ];
        assert_eq!(frontmost_window_of(&windows, 77), Some(windows[1].bounds));
        let floating = [listed(77, 3, 1600.0, 300.0), listed(77, 0, 0.0, 1000.0)];
        assert_eq!(frontmost_window_of(&floating, 77), Some(floating[0].bounds));
        let dock = [listed(77, 20, 0.0, 1512.0), listed(77, 0, 1600.0, 1000.0)];
        assert_eq!(frontmost_window_of(&dock, 77), Some(dock[1].bounds));
    }

    /// Invisible and tiny windows some apps keep on screen don't count.
    #[test]
    fn invisible_and_tiny_windows_dont_count() {
        let mut invisible = listed(77, 0, 0.0, 1000.0);
        invisible.alpha = 0.0;
        let mut tiny = listed(77, 0, 0.0, 1.0);
        tiny.bounds.height = 1.0;
        let window = listed(77, 0, 1600.0, 900.0);
        assert_eq!(
            frontmost_window_of(&[invisible, tiny, window], 77),
            Some(window.bounds)
        );
    }

    #[test]
    fn an_app_without_a_window_on_screen_has_no_frame() {
        let windows = [listed(500, 0, 0.0, 900.0), listed(77, 25, 0.0, 30.0)];
        assert_eq!(frontmost_window_of(&windows, 77), None);
        assert_eq!(frontmost_window_of(&[], 77), None);
    }

    #[test]
    fn a_point_on_the_line_between_two_screens_belongs_to_one() {
        let left = ScreenRect {
            x: -2560.0,
            y: -200.0,
            width: 2560.0,
            height: 1440.0,
        };
        assert!(left.contains((-2560.0, -200.0)));
        assert!(left.contains((-0.5, 1239.5)));
        assert!(!left.contains((0.0, 0.0)));
        assert!(!left.contains((-1.0, 1240.0)));
    }

    #[test]
    fn a_capture_that_never_began_is_none_at_once() {
        let slot = CaptureSlot::new();
        let started = Instant::now();
        assert_eq!(slot.take(5, Duration::from_secs(5)), None);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    // --- macOS's dialog once the text is in (user's choice "A", 2026-11-24) ---

    #[test]
    fn an_app_not_asked_yet_is_asked_once_its_text_is_in() {
        let mut asks = ConsentAsks::default();
        asks.note_not_asked("com.apple.Terminal");
        assert_eq!(
            asks.take_after_insertion(),
            Some("com.apple.Terminal".to_string())
        );
        assert_eq!(asks.take_after_insertion(), None);
    }

    #[test]
    fn an_app_is_asked_at_most_once_per_run() {
        let mut asks = ConsentAsks::default();
        asks.note_not_asked("com.microsoft.Outlook");
        assert!(asks.take_after_insertion().is_some());
        // The user closed the dialog or macOS didn't show it: no second one.
        asks.note_not_asked("com.microsoft.Outlook");
        assert_eq!(asks.take_after_insertion(), None);
    }

    #[test]
    fn the_app_noted_last_before_the_insertion_is_the_one_asked() {
        // Asked ahead at the start in one app, the text went into another.
        let mut asks = ConsentAsks::default();
        asks.note_not_asked("com.apple.Safari");
        asks.note_not_asked("com.apple.Terminal");
        assert_eq!(
            asks.take_after_insertion(),
            Some("com.apple.Terminal".to_string())
        );
        assert_eq!(asks.take_after_insertion(), None);
    }

    #[test]
    fn nothing_is_asked_without_an_app_not_asked_yet() {
        let mut asks = ConsentAsks::default();
        assert_eq!(asks.take_after_insertion(), None);
    }

    /// Asking for a title by Apple event, which exists on macOS only.
    #[cfg(target_os = "macos")]
    mod asking_for_titles {
        use super::*;

        /// A Mac as [`ask_for_title`] sees it, counting the permission checks
        /// and the scripts it runs.
        struct FakeMac {
            bundle: Option<PathBuf>,
            query: Option<TitleQuery>,
            permission: i32,
            run: ScriptRun,
            permission_checks: Cell<u32>,
            scripts: Cell<u32>,
        }

        impl FakeMac {
            /// TextEdit, allowed, answering with `run`.
            fn answering(run: ScriptRun) -> Self {
                FakeMac {
                    bundle: Some(PathBuf::from("/System/Applications/TextEdit.app")),
                    query: Some(TitleQuery::FrontWindow),
                    permission: 0,
                    run,
                    permission_checks: Cell::new(0),
                    scripts: Cell::new(0),
                }
            }

            fn ask(&self) -> TitleOutcome {
                ask_for_title(self, "com.apple.TextEdit", 42, Duration::from_millis(700))
            }
        }

        impl TitlePort for FakeMac {
            fn bundle_path(&self, _pid: i32) -> Option<PathBuf> {
                self.bundle.clone()
            }

            fn title_query(&self, _bundle_id: &str, _bundle: &Path) -> Option<TitleQuery> {
                self.query
            }

            fn permission(&self, _bundle_id: &str) -> i32 {
                self.permission_checks.set(self.permission_checks.get() + 1);
                self.permission
            }

            fn run_script(&self, _script: &str, _limit: Duration) -> ScriptRun {
                self.scripts.set(self.scripts.get() + 1);
                self.run.clone()
            }
        }

        fn done(success: bool, stdout: &str, stderr: &str) -> ScriptRun {
            ScriptRun::Done {
                success,
                stdout: stdout.to_string(),
                stderr: stderr.to_string(),
            }
        }

        #[test]
        fn an_allowed_app_tells_its_title_trimmed() {
            let mac = FakeMac::answering(done(true, "Notes about the trip\n", ""));
            assert_eq!(
                mac.ask(),
                TitleOutcome::Title("Notes about the trip".to_string())
            );
            assert_eq!(mac.scripts.get(), 1);
        }

        #[test]
        fn an_app_not_asked_yet_gets_no_script() {
            let mut mac = FakeMac::answering(done(true, "Notes", ""));
            mac.permission = -1744;
            assert_eq!(mac.ask(), TitleOutcome::NotAllowed(-1744));
            assert_eq!(mac.scripts.get(), 0, "a script ran without permission");
        }

        #[test]
        fn a_denied_or_quit_app_gets_no_script() {
            for code in [-1743, -600, -50] {
                let mut mac = FakeMac::answering(done(true, "Notes", ""));
                mac.permission = code;
                assert_eq!(mac.ask(), TitleOutcome::NotAllowed(code));
                assert_eq!(mac.scripts.get(), 0);
            }
        }

        #[test]
        fn an_app_without_a_title_query_is_not_checked_for_permission() {
            let mut mac = FakeMac::answering(done(true, "Notes", ""));
            mac.query = None;
            assert_eq!(mac.ask(), TitleOutcome::NoQuery);
            assert_eq!(mac.permission_checks.get(), 0);
            assert_eq!(mac.scripts.get(), 0);
        }

        #[test]
        fn an_app_whose_bundle_is_gone_is_not_asked() {
            let mut mac = FakeMac::answering(done(true, "Notes", ""));
            mac.bundle = None;
            assert_eq!(mac.ask(), TitleOutcome::NoBundle);
            assert_eq!(mac.permission_checks.get(), 0);
            assert_eq!(mac.scripts.get(), 0);
        }

        #[test]
        fn a_failing_script_tells_its_error_number() {
            let mac = FakeMac::answering(done(
                false,
                "",
                "0:72: execution error: Can’t get window 1 of application \"TextEdit\". (-1728)\n",
            ));
            assert_eq!(mac.ask(), TitleOutcome::Failed(Some(-1728)));
            let mac = FakeMac::answering(done(false, "", ""));
            assert_eq!(mac.ask(), TitleOutcome::Failed(None));
        }

        #[test]
        fn a_script_out_of_time_has_timed_out() {
            let mac = FakeMac::answering(ScriptRun::TimedOut);
            assert_eq!(mac.ask(), TitleOutcome::TimedOut);
        }

        #[test]
        fn a_script_that_could_not_run_says_why() {
            let mac = FakeMac::answering(ScriptRun::CouldNotRun("No such file".to_string()));
            assert_eq!(
                mac.ask(),
                TitleOutcome::CouldNotRun("No such file".to_string())
            );
        }

        #[test]
        fn an_empty_answer_is_empty() {
            let mac = FakeMac::answering(done(true, "  \n", ""));
            assert_eq!(mac.ask(), TitleOutcome::Empty);
        }

        #[test]
        fn the_error_number_is_read_from_osascripts_message() {
            assert_eq!(
                error_number("0:72: execution error: Can’t get window 1. (-1728)\n"),
                Some(-1728)
            );
            assert_eq!(
            error_number("0:31: execution error: Microsoft Outlook got an error: AppleEvent handler failed. (-10000)"),
            Some(-10000)
        );
            // A script's own error number can be positive; the last line counts.
            assert_eq!(
                error_number("warning: something\n0:9: execution error: Oops (5)\n"),
                Some(5)
            );
            assert_eq!(error_number(""), None);
            assert_eq!(error_number("execution error: no number"), None);
            assert_eq!(error_number("execution error: (see above)"), None);
        }

        fn reason(outcome: TitleOutcome) -> String {
            outcome.log_reason(Duration::from_millis(700)).1
        }

        #[test]
        fn the_reason_names_the_permission_state() {
            let not_asked = reason(TitleOutcome::NotAllowed(-1744));
            assert!(not_asked.contains("not asked yet"), "{not_asked}");
            assert!(not_asked.contains("-1744"), "{not_asked}");
            assert!(
                not_asked.contains(
                    "Defaults › Post Process › Context from Earlier Dictations › Window Titles"
                ),
                "{not_asked}"
            );
            let denied = reason(TitleOutcome::NotAllowed(-1743));
            assert!(denied.contains("denied"), "{denied}");
            assert!(
                denied.contains("Privacy & Security › Automation"),
                "{denied}"
            );
            assert!(reason(TitleOutcome::NotAllowed(-600)).contains("not running"));
            let other = reason(TitleOutcome::NotAllowed(-50));
            assert!(other.contains("other") && other.contains("-50"), "{other}");
        }

        #[test]
        fn the_reason_tells_what_went_wrong() {
            let failed = reason(TitleOutcome::Failed(Some(-1728)));
            assert!(failed.contains("-1728") && failed.contains("errAENoSuchObject"));
            assert!(reason(TitleOutcome::Failed(None)).contains("without an error number"));
            assert!(reason(TitleOutcome::TimedOut).contains("700ms"));
            assert!(reason(TitleOutcome::NoQuery).contains("scripting definition"));
            assert!(reason(TitleOutcome::NoBundle).contains("bundle"));
            assert!(reason(TitleOutcome::Empty).contains("empty"));
            assert!(
                reason(TitleOutcome::CouldNotRun("No such file".into())).contains("No such file")
            );
        }

        #[test]
        fn a_reason_goes_to_info_once_per_app() {
            let logged = Mutex::new(LoggedReasons::default());
            let limit = Duration::from_millis(700);
            let not_asked = TitleOutcome::NotAllowed(-1744);
            let log = |id: &str, outcome: &TitleOutcome| title_log(&logged, id, outcome, limit);

            let (level, line) = log("com.apple.Terminal", &not_asked);
            assert_eq!(level, log::Level::Info);
            assert!(
                line.starts_with("Window title of com.apple.Terminal: "),
                "{line}"
            );
            assert!(line.contains("not asked yet"), "{line}");
            assert_eq!(log("com.apple.Terminal", &not_asked).0, log::Level::Debug);
            // Another reason of the same app, the same reason of another app.
            assert_eq!(
                log("com.apple.Terminal", &TitleOutcome::Failed(Some(-1728))).0,
                log::Level::Info
            );
            assert_eq!(log("com.microsoft.Outlook", &not_asked).0, log::Level::Info);
            // Another error number is another reason; the same one isn't.
            assert_eq!(
                log("com.apple.Terminal", &TitleOutcome::Failed(Some(-1708))).0,
                log::Level::Info
            );
            assert_eq!(
                log("com.apple.Terminal", &TitleOutcome::Failed(Some(-1728))).0,
                log::Level::Debug
            );
        }

        #[test]
        fn a_title_goes_to_the_log_by_its_length_only() {
            let logged = Mutex::new(LoggedReasons::default());
            let limit = Duration::from_millis(700);
            let title = TitleOutcome::Title("Plan for Anna's party".to_string());
            let (level, line) = title_log(&logged, "com.apple.TextEdit", &title, limit);
            assert_eq!(level, log::Level::Info);
            assert!(!line.contains("Anna"), "{line}");
            assert!(line.contains("21 characters"), "{line}");
            // Another title of the same app is no new reason.
            let other = TitleOutcome::Title("Shopping".to_string());
            assert_eq!(
                title_log(&logged, "com.apple.TextEdit", &other, limit).0,
                log::Level::Debug
            );
        }

        #[test]
        fn the_logged_reasons_stay_few() {
            let logged = Mutex::new(LoggedReasons::default());
            let limit = Duration::from_millis(700);
            for app in 0..=LoggedReasons::LIMIT {
                title_log(
                    &logged,
                    &format!("com.example.app{app}"),
                    &TitleOutcome::Empty,
                    limit,
                );
            }
            assert!(logged.lock().unwrap().seen.len() <= LoggedReasons::LIMIT);
        }
    }
}
