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
//! - macOS without it: a scriptable app is asked for its title by Apple
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

use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;

use crate::accessibility::AccessibilityState;
use crate::dictation_context::{self, CapturedWindow};

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
    if access.trusted() {
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
    /// macOS: DanyWhere has Accessibility, so it reads every window's title.
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

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::c_void;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use objc2_app_kit::{NSApplicationActivationPolicy, NSRunningApplication, NSWorkspace};
    use objc2_foundation::{NSBundle, NSString};

    use super::{
        frontmost_window_of, prime_front, status_from_os, title_source, AccessibilityState,
        AutomationApp, AutomationStatus, ListedWindow, ScreenRect, TitleSource, WantsTitle,
        WindowTitleAccess, ANSWERED,
    };
    use crate::dictation_context;
    use crate::title_query::{self, Bundle};

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
            return AutomationStatus::Unknown;
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
        status_from_os(status)
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
    /// or title.
    fn query_title(bundle_id: &str, pid: i32, limit: Duration) -> Option<String> {
        let bundle = bundle_path_of(pid)?;
        let query = title_query::title_query(bundle_id, &bundle)?;
        let script = title_query::script(query, bundle_id)?;
        if automation_permission(bundle_id, false) != AutomationStatus::Allowed {
            return None;
        }
        // osascript runs on DanyWhere's behalf (macOS asks about DanyWhere,
        // not osascript), and can be stopped when the app doesn't answer.
        let mut child = Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(&script)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let deadline = Instant::now() + limit;
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => break,
                Ok(Some(_)) => return None,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(15))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    log::debug!("{bundle_id} didn't tell its window title in time");
                    return None;
                }
            }
        }
        let mut output = String::new();
        use std::io::Read;
        child.stdout.take()?.read_to_string(&mut output).ok()?;
        let title = output.trim().to_string();
        (!title.is_empty()).then_some(title)
    }

    /// With Accessibility every title comes from it; otherwise the apps that
    /// can tell theirs by Apple event, running ones with their permission
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
        frontmost(&|_: &str, _: &str| false).map(|(exe, name, _)| (exe, name))
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
    /// window's title when `wants_title` allows it.
    pub fn frontmost(
        wants_title: WantsTitle,
        _access: AccessibilityState,
    ) -> Option<(String, String, Option<String>)> {
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
        use crate::accessibility::testing::{with_choice, with_system_trusted};
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
    fn titles_follow_macos_alone_even_when_the_user_chose_to_go_without() {
        // As before the one source: the mode without Accessibility reads
        // titles through it once macOS grants it.
        assert_eq!(source_for(true, true), TitleSource::Accessibility);
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
}
