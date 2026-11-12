//! DanyWhere: which window a dictation starts in, for context from earlier
//! dictations in the same window (`dictation_context`).
//!
//! When a recording starts, a background thread asks for the frontmost app
//! and the title of its window, with a short time limit, so the start of the
//! recording never waits for it:
//!
//! - macOS with Accessibility: the focused window's title, read through the
//!   Accessibility API.
//! - macOS without it: for a few scriptable apps (iTerm2, Terminal, Chrome,
//!   Safari, legacy Outlook) the app is asked for its title by Apple event
//!   ("Automation"), but only when the user has already allowed DanyWhere to
//!   control that app; the consent dialog is never shown during a dictation,
//!   only from the settings (sidecar note
//!   `qa/2026-11-12_2140_automation-fenstertitel.md`).
//! - Windows: the foreground window's title and its program, which need no
//!   permission.
//! - Linux: nothing yet, so no context.
//!
//! Apps on the user's exclusion list are not captured at all.

use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;

use crate::dictation_context::{self, CapturedWindow};
use crate::settings;

/// How long a capture may take before the dictation goes on without it.
const CAPTURE_LIMIT: Duration = Duration::from_millis(800);

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

/// The scriptable apps whose window title DanyWhere can ask for, by bundle
/// id, with their usual name.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const AUTOMATION_APPS: &[(&str, &str)] = &[
    ("com.googlecode.iterm2", "iTerm2"),
    ("com.apple.Terminal", "Terminal"),
    ("com.google.Chrome", "Google Chrome"),
    ("com.apple.Safari", "Safari"),
    ("com.microsoft.Outlook", "Microsoft Outlook"),
];

/// The AppleScript that asks the app with `bundle_id` for its title. An
/// incognito window in Chrome answers nothing, so it is never captured.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn automation_script(bundle_id: &str) -> Option<&'static str> {
    Some(match bundle_id {
        "com.googlecode.iterm2" => {
            r#"tell application id "com.googlecode.iterm2" to return name of current session of current window"#
        }
        "com.apple.Terminal" => {
            r#"tell application id "com.apple.Terminal" to return name of front window"#
        }
        "com.google.Chrome" => {
            r#"tell application id "com.google.Chrome"
if mode of front window is "incognito" then return ""
return title of active tab of front window
end tell"#
        }
        "com.apple.Safari" => {
            r#"tell application id "com.apple.Safari" to return name of current tab of front window"#
        }
        "com.microsoft.Outlook" => {
            r#"tell application id "com.microsoft.Outlook" to return name of front window"#
        }
        _ => return None,
    })
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
/// context from earlier dictations is in use.
pub fn begin_capture(app: &AppHandle, session: u64) {
    let settings = settings::get_settings(app);
    if !dictation_context::capture_wanted(&settings) {
        return;
    }
    SLOT.begin(session);
    let excluded = settings.dictation_context.excluded_apps.clone();
    std::thread::Builder::new()
        .name("window-capture".into())
        .spawn(move || {
            let started = Instant::now();
            let window = frontmost_window(&excluded);
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

/// The window captured when the recording `session` started.
pub fn take_capture(session: u64) -> Option<CapturedWindow> {
    SLOT.take(session, CAPTURE_LIMIT)
}

/// The frontmost window as a [`CapturedWindow`], unless its app is excluded,
/// is DanyWhere itself or has no usable title.
fn frontmost_window(excluded: &[String]) -> Option<CapturedWindow> {
    let (app_id, app_name, raw_title) = platform::frontmost()?;
    if dictation_context::is_excluded_app(&app_id, &app_name, excluded) {
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
    /// macOS without Accessibility: the scriptable apps and whether DanyWhere
    /// may ask them.
    pub apps: Vec<AutomationApp>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct AutomationApp {
    pub bundle_id: String,
    pub name: String,
    pub status: AutomationStatus,
}

/// How DanyWhere learns window titles here, for the settings.
#[tauri::command]
#[specta::specta]
pub fn get_window_title_access() -> WindowTitleAccess {
    platform::access()
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
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::NSString;

    use super::{
        automation_script, status_from_os, AutomationApp, AutomationStatus, WindowTitleAccess,
        AUTOMATION_APPS,
    };

    type CFTypeRef = *const c_void;
    type AXUIElementRef = *const c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
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

    /// The frontmost app's id and name, and the title of its window if
    /// DanyWhere may learn it.
    pub fn frontmost() -> Option<(String, String, Option<String>)> {
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
        // SAFETY: a plain query without side effects.
        let title = if unsafe { AXIsProcessTrusted() } {
            accessibility_title(pid)
        } else {
            automation_title(&bundle_id)
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

    /// The title by Apple event, for the scriptable apps DanyWhere may
    /// already control; never shows the consent dialog.
    fn automation_title(bundle_id: &str) -> Option<String> {
        let script = automation_script(bundle_id)?;
        if automation_permission(bundle_id, false) != AutomationStatus::Allowed {
            return None;
        }
        // osascript runs on DanyWhere's behalf (macOS asks about DanyWhere,
        // not osascript), and can be stopped when the app doesn't answer.
        let mut child = Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(script)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let deadline = Instant::now() + Duration::from_millis(700);
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

    fn running(bundle_id: &str) -> bool {
        let workspace = NSWorkspace::sharedWorkspace();
        workspace.runningApplications().iter().any(|app| {
            app.bundleIdentifier()
                .is_some_and(|id| id.to_string() == bundle_id)
        })
    }

    fn status_of(bundle_id: &str, ask: bool) -> AutomationStatus {
        if !running(bundle_id) {
            return AutomationStatus::NotRunning;
        }
        automation_permission(bundle_id, ask)
    }

    pub fn access() -> WindowTitleAccess {
        // SAFETY: a plain query without side effects.
        let accessibility = unsafe { AXIsProcessTrusted() };
        WindowTitleAccess {
            supported: true,
            accessibility,
            apps: AUTOMATION_APPS
                .iter()
                .map(|(bundle_id, name)| AutomationApp {
                    bundle_id: bundle_id.to_string(),
                    name: name.to_string(),
                    status: if accessibility {
                        AutomationStatus::Unknown
                    } else {
                        status_of(bundle_id, false)
                    },
                })
                .collect(),
        }
    }

    pub fn request(bundle_id: &str) -> AutomationStatus {
        if !AUTOMATION_APPS.iter().any(|(id, _)| *id == bundle_id) {
            return AutomationStatus::Unknown;
        }
        status_of(bundle_id, true)
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{AutomationStatus, WindowTitleAccess};
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, MAX_PATH};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    };

    /// The foreground window's program (exe file name), its name, and the
    /// window's title.
    pub fn frontmost() -> Option<(String, String, Option<String>)> {
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
            Some((exe, name, title))
        }
    }

    pub fn access() -> WindowTitleAccess {
        WindowTitleAccess {
            supported: true,
            accessibility: true,
            apps: Vec::new(),
        }
    }

    pub fn request(_bundle_id: &str) -> AutomationStatus {
        AutomationStatus::Unknown
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    use super::{AutomationStatus, WindowTitleAccess};

    /// Linux: not yet, so dictations there have no context.
    pub fn frontmost() -> Option<(String, String, Option<String>)> {
        None
    }

    pub fn access() -> WindowTitleAccess {
        WindowTitleAccess {
            supported: false,
            accessibility: false,
            apps: Vec::new(),
        }
    }

    pub fn request(_bundle_id: &str) -> AutomationStatus {
        AutomationStatus::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn every_scriptable_app_has_a_script_for_its_title() {
        for (bundle_id, _) in AUTOMATION_APPS {
            let script = automation_script(bundle_id).expect("a script");
            assert!(script.contains(bundle_id), "{script}");
        }
        assert!(automation_script("com.microsoft.VSCode").is_none());
    }

    #[test]
    fn chrome_answers_nothing_for_an_incognito_window() {
        let script = automation_script("com.google.Chrome").expect("a script");
        assert!(script.contains("incognito"));
        assert!(script.contains("title of active tab of front window"));
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

    #[test]
    fn a_capture_that_never_began_is_none_at_once() {
        let slot = CaptureSlot::new();
        let started = Instant::now();
        assert_eq!(slot.take(5, Duration::from_secs(5)), None);
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
