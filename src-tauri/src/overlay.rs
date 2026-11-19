use crate::app_profiles::OverlayProfile;
use crate::input;
use crate::notices::Notice;
use crate::settings;
use crate::settings::{OverlayPosition, OverlayStyle};
use crate::window_capture::{self, ScreenRect};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize};

#[cfg(not(target_os = "macos"))]
use log::debug;

#[cfg(not(target_os = "macos"))]
use tauri::WebviewWindowBuilder;

#[cfg(target_os = "macos")]
use tauri::WebviewUrl;

#[cfg(target_os = "macos")]
use tauri_nspanel::{tauri_panel, CollectionBehavior, PanelBuilder, PanelLevel, StyleMask};

#[cfg(target_os = "linux")]
use crate::utils;

#[cfg(target_os = "linux")]
use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(RecordingOverlayPanel {
        config: {
            can_become_key_window: false,
            is_floating_panel: true
        }
    })
}

// Native overlay window sizes (logical points). One window is reused for every
// state and resized in `show_overlay_state`; each size need only be at least as
// large as the card it hosts (the `--ov-*` vars in RecordingOverlay.css). The
// card is CSS-anchored flush to the screen edge, so window height doesn't move
// where the card sits — only OVERLAY_TOP_OFFSET / OVERLAY_BOTTOM_OFFSET do. Keep
// these in sync with the CSS card geometry.
//
// On Windows these sizes are additionally multiplied by the accessibility text
// scale (see windows_text_scale_factor), which WebView2 applies as a zoom.
//
// Compact overlay (Minimal / transcribing / processing): the 40h pill animates
// width from 172 (--ov-rest-w) to 216 (--ov-work-w) and expands from center, so
// the window must fit the widest state plus a little slack. DanyWhere: from two
// profiles on, the pill also names the dictation's profile and grows with it
// (src/lib/overlayProfile.ts); 320 lets the working pill keep a 90px name next
// to "Transcribing...". Handy's 266 left 215px for a notice title, the limit
// AGENTS.md still names.
const OVERLAY_WIDTH: f64 = 320.0;
const OVERLAY_HEIGHT: f64 = 50.0;

// Actual is 394x118, just a little extra
const OVERLAY_STREAM_WIDTH: f64 = 400.0;
/// Wide enough for the "on the clipboard" notice text.
const OVERLAY_NOTICE_WIDTH: f64 = 340.0;
const OVERLAY_STREAM_HEIGHT: f64 = 120.0;

const NOTICE_STATE: &str = "notice";
const LIVE_NOTICE_STATE: &str = "live_notice";

/// How long a notice holds before the overlay fades it.
const NOTICE_HOLD: Duration = Duration::from_secs(4);

fn is_live_state(state: &str) -> bool {
    state == "streaming" || state == LIVE_NOTICE_STATE
}

/// The notice form that matches the session it replaces, so the window never
/// changes size for one. A notice with no session gets the compact form.
fn notice_state(session_is_live: bool) -> &'static str {
    if session_is_live {
        LIVE_NOTICE_STATE
    } else {
        NOTICE_STATE
    }
}

/// Overlay window size (logical) for a given UI state.
fn overlay_dimensions(state: &str) -> (f64, f64) {
    if is_live_state(state) {
        (OVERLAY_STREAM_WIDTH, OVERLAY_STREAM_HEIGHT)
    } else if matches!(
        state,
        "clipboard" | "ready" | "not_ready" | "word_list" | "profile" | "context"
    ) {
        (OVERLAY_NOTICE_WIDTH, OVERLAY_HEIGHT)
    } else {
        (OVERLAY_WIDTH, OVERLAY_HEIGHT)
    }
}

static LAST_MIC_LEVEL_EMIT: AtomicU64 = AtomicU64::new(0);
const EMIT_THROTTLE_MS: u64 = 33; // ~30 FPS

#[cfg(target_os = "macos")]
const OVERLAY_TOP_OFFSET: f64 = 46.0;
#[cfg(any(target_os = "windows", target_os = "linux"))]
const OVERLAY_TOP_OFFSET: f64 = 4.0;

#[cfg(target_os = "macos")]
const OVERLAY_BOTTOM_OFFSET: f64 = 15.0;

#[cfg(any(target_os = "windows", target_os = "linux"))]
const OVERLAY_BOTTOM_OFFSET: f64 = 40.0;

/// Configures the edge and offset of a GTK layer surface. gtk-layer-shell
/// commits anchor and margin changes itself, including while the surface is
/// mapped, so changing position does not require a manual hide/show cycle.
#[cfg(target_os = "linux")]
fn configure_layer_shell_position(gtk_window: &gtk::ApplicationWindow, position: OverlayPosition) {
    let (edge, opposite_edge, margin) = match position {
        OverlayPosition::Top => (Edge::Top, Edge::Bottom, OVERLAY_TOP_OFFSET),
        OverlayPosition::Bottom => (Edge::Bottom, Edge::Top, OVERLAY_BOTTOM_OFFSET),
    };

    gtk_window.set_anchor(edge, true);
    gtk_window.set_anchor(opposite_edge, false);
    gtk_window.set_layer_shell_margin(edge, margin.round() as i32);
    gtk_window.set_layer_shell_margin(opposite_edge, 0);
}

/// Configures a GTK layer surface before it is shown.
///
/// Tauri's normal `set_size` path calls `gtk_window_resize`, but layer surfaces
/// derive their dimensions from GTK's size request. gtk-layer-shell documents
/// the `set_size_request` + `resize(1, 1)` sequence for forcing a new size.
#[cfg(target_os = "linux")]
fn configure_layer_shell_surface(
    gtk_window: &gtk::ApplicationWindow,
    position: OverlayPosition,
    width: f64,
    height: f64,
) {
    use gtk::prelude::{GtkWindowExt, WidgetExt};

    configure_layer_shell_position(gtk_window, position);

    gtk_window.set_size_request(
        width.round().max(1.0) as i32,
        height.round().max(1.0) as i32,
    );
    gtk_window.resize(1, 1);
}

/// Initializes GTK layer shell for Linux overlay window
/// Returns true if layer shell was successfully initialized, false otherwise
#[cfg(target_os = "linux")]
fn init_gtk_layer_shell(overlay_window: &tauri::webview::WebviewWindow) -> bool {
    if utils::env_flag_enabled("HANDY_NO_GTK_LAYER_SHELL") {
        debug!("Skipping GTK layer shell init (HANDY_NO_GTK_LAYER_SHELL is enabled)");
        return false;
    }

    if !gtk_layer_shell::is_supported() {
        return false;
    }

    // Try to get the GTK window from the Tauri webview
    if let Ok(gtk_window) = overlay_window.gtk_window() {
        gtk_window.init_layer_shell();
        gtk_window.set_layer(Layer::Overlay);
        gtk_window.set_keyboard_mode(KeyboardMode::None);
        gtk_window.set_exclusive_zone(0);

        let overlay_position = settings::get_settings(overlay_window.app_handle()).overlay_position;
        configure_layer_shell_surface(&gtk_window, overlay_position, OVERLAY_WIDTH, OVERLAY_HEIGHT);

        let initialized = gtk_window.is_layer_window();
        LAYER_SHELL_ACTIVE.store(initialized, Ordering::SeqCst);
        return initialized;
    }
    false
}

/// Forces a window to be topmost using Win32 API (Windows only)
/// This is more reliable than Tauri's set_always_on_top which can be overridden
#[cfg(target_os = "windows")]
fn force_overlay_topmost(overlay_window: &tauri::webview::WebviewWindow) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    };

    // Clone because run_on_main_thread takes 'static
    let overlay_clone = overlay_window.clone();

    // Make sure the Win32 call happens on the UI thread
    let _ = overlay_clone.clone().run_on_main_thread(move || {
        if let Ok(hwnd) = overlay_clone.hwnd() {
            unsafe {
                // Force Z-order: make this window topmost without changing size/pos or stealing focus
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
        }
    });
}

/// DanyWhere: the monitor the overlay goes to, chosen anew at every change of
/// its state (recording, transcribing, processing, a notice) and not followed
/// in between: the one holding most of the frontmost window, else the one
/// with the mouse pointer, else the main screen.
///
/// Handy took the monitor with the mouse pointer, which it learned from
/// Enigo. Without Accessibility Enigo doesn't start, so on macOS the overlay
/// then always showed on the main screen. The window frame and the pointer
/// now come from `window_capture`, which needs no permission on macOS or
/// Windows; Enigo still tells the pointer on Linux.
fn overlay_monitor(app_handle: &AppHandle) -> Option<tauri::Monitor> {
    let monitors = app_handle.available_monitors().unwrap_or_default();
    let screens: Vec<ScreenRect> = monitors
        .iter()
        .map(|monitor| {
            // Windows compares everything in physical pixels; macOS and Linux
            // in points (see `monitor_bounds`).
            let divisor = if cfg!(target_os = "windows") {
                1.0
            } else {
                monitor.scale_factor()
            };
            monitor_bounds(*monitor.position(), *monitor.size(), divisor)
        })
        .collect();
    let window = window_capture::frontmost_window_frame();
    let cursor = window_capture::cursor_location()
        .or_else(|| input::get_cursor_position(app_handle).map(|(x, y)| (x as f64, y as f64)));
    let choice = choose_overlay_screen(window, cursor, &screens);
    // Only how the screen was chosen, never anything about the window.
    log::debug!(
        "Overlay screen: {:?} of {} screen(s)",
        choice,
        screens.len()
    );
    match choice {
        OverlayScreen::Window(index) | OverlayScreen::Cursor(index) => {
            monitors.into_iter().nth(index)
        }
        OverlayScreen::Primary => app_handle.primary_monitor().ok().flatten(),
    }
}

/// DanyWhere: which screen the overlay goes to, and why (for the log).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OverlayScreen {
    /// The screen at this index holds most of the frontmost window.
    Window(usize),
    /// The screen at this index has the mouse pointer.
    Cursor(usize),
    /// Neither told: the main screen.
    Primary,
}

/// A monitor's bounds as tao reports them, in the space the window frame and
/// the cursor use. `divisor` is the monitor's own scale on macOS and Linux:
/// tao multiplies each monitor's origin and size in points by that monitor's
/// scale, so dividing by it gives points again, even with mixed scales. On
/// Windows it is 1, everything there being physical pixels.
fn monitor_bounds(
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    divisor: f64,
) -> ScreenRect {
    ScreenRect {
        x: position.x as f64 / divisor,
        y: position.y as f64 / divisor,
        width: size.width as f64 / divisor,
        height: size.height as f64 / divisor,
    }
}

/// Chooses the overlay's screen from the frontmost window's frame, the mouse
/// pointer and the screens' bounds, all in one space:
///
/// 1. the screen sharing the largest area with the window; on a tie the one
///    with the pointer, if it is among them, else the first in the list;
/// 2. with no window, or one on no screen (e.g. minimized), the screen with
///    the pointer;
/// 3. else the main screen.
fn choose_overlay_screen(
    window: Option<ScreenRect>,
    cursor: Option<(f64, f64)>,
    screens: &[ScreenRect],
) -> OverlayScreen {
    let cursor_screen =
        cursor.and_then(|point| screens.iter().position(|screen| screen.contains(point)));
    if let Some(window) = window {
        let areas: Vec<f64> = screens
            .iter()
            .map(|screen| screen.overlap_area(&window))
            .collect();
        let largest = areas.iter().copied().fold(0.0, f64::max);
        if largest > 0.0 {
            let index = cursor_screen
                .filter(|&index| areas[index] == largest)
                .or_else(|| areas.iter().position(|&area| area == largest));
            if let Some(index) = index {
                return OverlayScreen::Window(index);
            }
        }
    }
    match cursor_screen {
        Some(index) => OverlayScreen::Cursor(index),
        None => OverlayScreen::Primary,
    }
}

/// Returns overlay position in logical coordinates (points on macOS).
///
/// The Bottom anchor uses the macOS work area (visibleFrame) so the overlay
/// tracks the Dock — above it when shown, at the screen edge when hidden.
/// This relies on tauri 2.11's work_area.position.y fix (#14655), the same
/// bug that led PR #969 to abandon work_area for full monitor bounds. Top and
/// the other platforms keep full monitor bounds plus the fixed offsets
/// (work_area is unreliable on Wayland; Windows' offset clears the taskbar).
///
/// We must use LogicalPosition (not PhysicalPosition) because Tauri/tao
/// converts PhysicalPosition using the scale factor of the monitor the window
/// is *currently* on, which is wrong when moving cross-monitor. Windows uses
/// `place_windows_overlay` instead (no single logical space across mixed DPI).
fn calculate_overlay_position(
    app_handle: &AppHandle,
    width: f64,
    height: f64,
) -> Option<(f64, f64)> {
    let monitor = overlay_monitor(app_handle)?;
    let scale = monitor.scale_factor();
    let monitor_x = monitor.position().x as f64 / scale;
    let monitor_y = monitor.position().y as f64 / scale;
    let monitor_width = monitor.size().width as f64 / scale;

    let settings = settings::get_settings(app_handle);

    let x = monitor_x + (monitor_width - width) / 2.0;
    let y = match settings.overlay_position {
        OverlayPosition::Top => monitor_y + OVERLAY_TOP_OFFSET,
        OverlayPosition::Bottom => {
            // work_area.position shares monitor.position's global coordinate
            // space, so no monitor offset is added.
            #[cfg(target_os = "macos")]
            let bottom = {
                let wa = monitor.work_area();
                (wa.position.y as f64 + wa.size.height as f64) / scale
            };
            #[cfg(not(target_os = "macos"))]
            let bottom = monitor_y + monitor.size().height as f64 / scale;

            bottom - height - OVERLAY_BOTTOM_OFFSET
        }
    };

    Some((x, y))
}

/// Current overlay window size in logical units (points), for repositioning
/// without assuming a fixed size (compact vs. streaming).
#[cfg(not(target_os = "windows"))]
fn current_overlay_logical_size(window: &tauri::webview::WebviewWindow) -> Option<(f64, f64)> {
    let size = window.inner_size().ok()?;
    let scale = window.scale_factor().ok()?;
    Some((size.width as f64 / scale, size.height as f64 / scale))
}

#[cfg(target_os = "windows")]
static WINDOWS_OVERLAY_IS_STREAMING: AtomicBool = AtomicBool::new(false);

/// Windows accessibility text size (Settings > Accessibility > Text size), a
/// separate axis from display scaling that WebView2 applies as a document zoom.
#[cfg(target_os = "windows")]
fn windows_text_scale_factor() -> f64 {
    // Absent until the user moves the slider off 100%; stored as a percentage.
    winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Accessibility")
        .and_then(|key| key.get_value::<u32, _>("TextScaleFactor"))
        .map(|percent| (percent as f64 / 100.0).clamp(1.0, 2.25))
        .unwrap_or(1.0)
}

/// Overlay rectangle in the destination monitor's physical pixels, so nothing
/// is converted through the window's previous-monitor DPI.
#[cfg(target_os = "windows")]
fn windows_overlay_bounds(
    monitor_position: PhysicalPosition<i32>,
    monitor_size: PhysicalSize<u32>,
    scale: f64,
    text_scale: f64,
    logical_width: f64,
    logical_height: f64,
    overlay_position: OverlayPosition,
) -> (i32, i32, i32, i32) {
    // Grow the window with the text scale; offsets stay DPI-only since the
    // card sits flush against the window's screen-edge side.
    let content_scale = scale * text_scale;
    let width = (logical_width * content_scale).round().max(1.0) as i32;
    let height = (logical_height * content_scale).round().max(1.0) as i32;
    let x = (monitor_position.x as f64 + (monitor_size.width as f64 - width as f64) / 2.0).round()
        as i32;
    let y = match overlay_position {
        OverlayPosition::Top => {
            (monitor_position.y as f64 + OVERLAY_TOP_OFFSET * scale).round() as i32
        }
        OverlayPosition::Bottom => (monitor_position.y as f64 + monitor_size.height as f64
            - height as f64
            - OVERLAY_BOTTOM_OFFSET * scale)
            .round() as i32,
    };

    (x, y, width, height)
}

/// Moves and sizes the overlay in one native SetWindowPos, bypassing tao's
/// current-DPI logical conversion that mislands cross-monitor moves.
#[cfg(target_os = "windows")]
fn place_windows_overlay(
    app_handle: &AppHandle,
    overlay_window: &tauri::webview::WebviewWindow,
    logical_width: f64,
    logical_height: f64,
) -> Result<(), String> {
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER};

    let monitor = overlay_monitor(app_handle)
        .ok_or_else(|| "failed to determine the overlay's monitor".to_string())?;
    let text_scale = windows_text_scale_factor();
    let (x, y, width, height) = windows_overlay_bounds(
        *monitor.position(),
        *monitor.size(),
        monitor.scale_factor(),
        text_scale,
        logical_width,
        logical_height,
        settings::get_settings(app_handle).overlay_position,
    );
    let hwnd = overlay_window
        .hwnd()
        .map_err(|error| format!("failed to get overlay window handle: {error}"))?;

    unsafe {
        SetWindowPos(
            hwnd,
            None,
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE | SWP_NOZORDER,
        )
        .map_err(|error| format!("failed to set overlay bounds: {error}"))?;
    }

    log::debug!(
        "windows overlay bounds: x={} y={} width={} height={} scale={} text_scale={}",
        x,
        y,
        width,
        height,
        monitor.scale_factor(),
        text_scale
    );
    Ok(())
}

/// Creates the recording overlay window and keeps it hidden by default
#[cfg(not(target_os = "macos"))]
pub fn create_recording_overlay(app_handle: &AppHandle) {
    // On Linux (Wayland), monitor detection often fails, but we don't need exact coordinates
    // for Layer Shell as we use anchors. On other platforms, we require a monitor.
    #[cfg(not(target_os = "linux"))]
    {
        let position = calculate_overlay_position(app_handle, OVERLAY_WIDTH, OVERLAY_HEIGHT);
        if position.is_none() {
            debug!("Failed to determine overlay position, not creating overlay window");
            return;
        }
    }

    // On X11 the overlay must stay resizable. tao turns resizing off on the
    // first map, and GTK then sizes a non-resizable window to its content's
    // natural size, which WebKitWebView reports as 200px tall. The window
    // grew past the 50px its position was computed for, and the card, anchored
    // to the window's bottom edge, was drawn below the screen (#1950). The
    // window has no decorations, so this offers the user nothing new.
    #[cfg(target_os = "linux")]
    let resizable = !utils::is_wayland();
    #[cfg(not(target_os = "linux"))]
    let resizable = false;

    // Position starts unset — update_overlay_position() sets the correct
    // LogicalPosition before the overlay is shown.
    let mut builder = WebviewWindowBuilder::new(
        app_handle,
        "recording_overlay",
        tauri::WebviewUrl::App("src/overlay/index.html".into()),
    )
    .title("Recording")
    .resizable(resizable)
    .inner_size(OVERLAY_WIDTH, OVERLAY_HEIGHT)
    .shadow(false)
    .maximizable(false)
    .minimizable(false)
    .closable(false)
    .accept_first_mouse(true)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .transparent(true)
    .focusable(false)
    .focused(false)
    .visible(false);

    if let Some(data_dir) = crate::portable::data_dir() {
        builder = builder.data_directory(data_dir.join("webview"));
    }

    #[allow(unused_variables)]
    match builder.build() {
        Ok(window) => {
            #[cfg(target_os = "linux")]
            {
                // Try to initialize GTK layer shell, ignore errors if compositor doesn't support it
                if init_gtk_layer_shell(&window) {
                    debug!("GTK layer shell initialized for overlay window");
                } else {
                    debug!("GTK layer shell not available, falling back to regular window");
                    // X11 fallback: otherwise the WM activates the overlay on map and steals focus (#2316)
                    if let Ok(gtk_window) = window.gtk_window() {
                        use gtk::prelude::GtkWindowExt;
                        gtk_window.set_focus_on_map(false);
                    }
                }
            }

            debug!("Recording overlay window created successfully (hidden)");
        }
        Err(e) => {
            debug!("Failed to create recording overlay window: {}", e);
        }
    }
}

/// Creates the recording overlay panel and keeps it hidden by default (macOS)
#[cfg(target_os = "macos")]
pub fn create_recording_overlay(app_handle: &AppHandle) {
    if let Some((x, y)) = calculate_overlay_position(app_handle, OVERLAY_WIDTH, OVERLAY_HEIGHT) {
        // PanelBuilder creates a Tauri window then converts it to NSPanel.
        // The window remains registered, so get_webview_window() still works.
        match PanelBuilder::<_, RecordingOverlayPanel>::new(app_handle, "recording_overlay")
            .url(WebviewUrl::App("src/overlay/index.html".into()))
            .title("Recording")
            .position(tauri::Position::Logical(tauri::LogicalPosition { x, y }))
            .level(PanelLevel::Status)
            .size(tauri::Size::Logical(tauri::LogicalSize {
                width: OVERLAY_WIDTH,
                height: OVERLAY_HEIGHT,
            }))
            .has_shadow(false)
            .transparent(true)
            .no_activate(true)
            .corner_radius(0.0)
            .style_mask(StyleMask::empty().borderless().nonactivating_panel())
            .with_window(|w| w.decorations(false).transparent(true).focusable(false))
            .collection_behavior(
                CollectionBehavior::new()
                    .can_join_all_spaces()
                    .full_screen_auxiliary(),
            )
            .build()
        {
            Ok(panel) => {
                panel.hide();
            }
            Err(e) => {
                log::error!("Failed to create recording overlay panel: {}", e);
            }
        }
    }
}

fn show_overlay_state(app_handle: &AppHandle, state: &str) {
    // Whether the overlay shows at all is governed by overlay_style; position
    // only chooses Top vs Bottom placement. Checked here (off the main thread)
    // so the common overlay-disabled case never pays for a main-thread hop.
    let settings = settings::get_settings(app_handle);
    if settings.overlay_style == OverlayStyle::None {
        return;
    }
    OVERLAY_SESSION_SHOWING.store(true, Ordering::SeqCst);
    OVERLAY_SESSION_LIVE.store(is_live_state(state), Ordering::SeqCst);

    // The rest queries monitors and the cursor and mutates window geometry. On
    // Linux the monitor/cursor lookups hit GDK/Xlib on the process's shared X11
    // connection, which is only safe from the GTK main thread — running them on
    // a background thread corrupts the connection and hard-crashes the app
    // (issue #227). Hop to the main thread on every platform to keep the
    // geometry path uniform (a no-op cost on Windows, and it also keeps macOS's
    // NSScreen access main-thread-correct). run_on_main_thread runs the closure
    // inline when already on the main thread, so this never deadlocks.
    let handle = app_handle.clone();
    let state = state.to_string();
    let _ = app_handle.run_on_main_thread(move || show_overlay_state_on_main(&handle, &state));
}

fn show_overlay_state_on_main(app_handle: &AppHandle, state: &str) {
    // Size the overlay for this state (compact vs. streaming), then position it.
    let (width, height) = overlay_dimensions(state);
    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        // Invalidate any delayed hide still in flight from a previous session
        // (see `hide_recording_overlay`).
        OVERLAY_SHOW_GENERATION.fetch_add(1, Ordering::SeqCst);

        #[cfg(target_os = "linux")]
        let shown_with_layer_shell = if LAYER_SHELL_ACTIVE.load(Ordering::SeqCst) {
            let position = settings::get_settings(app_handle).overlay_position;
            match overlay_window.gtk_window() {
                Ok(gtk_window) => {
                    configure_layer_shell_surface(&gtk_window, position, width, height)
                }
                Err(error) => log::error!("Failed to access GTK overlay window: {error}"),
            }
            let _ = overlay_window.show();
            true
        } else {
            false
        };
        #[cfg(not(target_os = "linux"))]
        let shown_with_layer_shell = false;

        if !shown_with_layer_shell {
            let size_started = std::time::Instant::now();
            #[cfg(not(target_os = "windows"))]
            let _ =
                overlay_window.set_size(tauri::Size::Logical(tauri::LogicalSize { width, height }));
            #[cfg(target_os = "windows")]
            WINDOWS_OVERLAY_IS_STREAMING.store(is_live_state(state), Ordering::Relaxed);
            let size_elapsed = size_started.elapsed();

            let pos_started = std::time::Instant::now();
            #[cfg(not(target_os = "windows"))]
            let set_pos_elapsed =
                if let Some((x, y)) = calculate_overlay_position(app_handle, width, height) {
                    let set_pos_started = std::time::Instant::now();
                    let _ = overlay_window
                        .set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
                    set_pos_started.elapsed()
                } else {
                    std::time::Duration::ZERO
                };
            #[cfg(target_os = "windows")]
            let set_pos_elapsed = {
                let set_pos_started = std::time::Instant::now();
                if let Err(error) =
                    place_windows_overlay(app_handle, &overlay_window, width, height)
                {
                    log::error!("Failed to place recording overlay: {error}");
                }
                set_pos_started.elapsed()
            };
            let pos_calc_elapsed = pos_started.elapsed() - set_pos_elapsed;

            let show_started = std::time::Instant::now();
            let _ = overlay_window.show();
            let show_elapsed = show_started.elapsed();

            // On Windows, aggressively re-assert "topmost" in the native Z-order after showing
            #[cfg(target_os = "windows")]
            force_overlay_topmost(&overlay_window);

            // Re-assert bounds after show(): the pre-show move crosses the DPI
            // boundary, and tao's WM_DPICHANGED reflow clobbers the first placement.
            #[cfg(target_os = "windows")]
            if let Err(error) = place_windows_overlay(app_handle, &overlay_window, width, height) {
                log::error!("Failed to re-assert recording overlay position: {error}");
            }

            log::debug!(
                "overlay '{}': set_size={:?} pos_calc={:?} set_pos={:?} show={:?}",
                state,
                size_elapsed,
                pos_calc_elapsed,
                set_pos_elapsed,
                show_elapsed
            );
        }

        let _ = overlay_window.emit("show-overlay", state);
    }
}

/// Notify the visible recording overlay that the input stream has delivered its
/// first sample chunk. Audio feedback uses the same backend readiness signal,
/// but this targeted event is skipped when overlays are disabled.
pub fn emit_recording_ready(app_handle: &AppHandle) {
    if !OVERLAY_ENABLED.load(Ordering::Relaxed) {
        return;
    }

    // Showing the overlay is also queued onto the main thread. Queue readiness
    // there as well so a very fast always-on stream cannot overtake show-overlay
    // and then get reset back to the arming state by the frontend.
    let handle = app_handle.clone();
    let _ = app_handle.run_on_main_thread(move || {
        let _ = handle.emit_to("recording_overlay", "recording-ready", ());
    });
}

/// Shows the recording overlay window with fade-in animation
pub fn show_recording_overlay(app_handle: &AppHandle) {
    show_overlay_state(app_handle, "recording");
}

/// Shows the larger streaming overlay that displays live transcription text
pub fn show_streaming_overlay(app_handle: &AppHandle) {
    show_overlay_state(app_handle, "streaming");
}

/// Shows the transcribing overlay window
pub fn show_transcribing_overlay(app_handle: &AppHandle) {
    show_overlay_state(app_handle, "transcribing");
}

/// Shows the processing overlay window
pub fn show_processing_overlay(app_handle: &AppHandle) {
    show_overlay_state(app_handle, "processing");
}

/// Shows `notice` in place of the session's overlay and fades it after
/// [`NOTICE_HOLD`] unless something newer has shown. With the overlay off, it
/// still hides the session's overlay in case the setting changed mid-session.
pub fn show_overlay_notice(app_handle: &AppHandle, notice: &Notice) {
    if !OVERLAY_ENABLED.load(Ordering::Relaxed) {
        hide_recording_overlay(app_handle);
        return;
    }
    OVERLAY_SESSION_SHOWING.store(false, Ordering::SeqCst);

    let state = notice_state(OVERLAY_SESSION_LIVE.load(Ordering::SeqCst));
    let handle = app_handle.clone();
    let notice = notice.clone();
    // On the main thread for the same reason as show_overlay_state.
    let _ = app_handle.run_on_main_thread(move || {
        let _ = handle.emit_to("recording_overlay", "overlay-notice", notice);
        show_overlay_state_on_main(&handle, state);

        let generation = OVERLAY_SHOW_GENERATION.load(Ordering::SeqCst);
        OVERLAY_NOTICE_GENERATION.store(generation, Ordering::SeqCst);
        std::thread::spawn(move || {
            std::thread::sleep(NOTICE_HOLD);
            if OVERLAY_SHOW_GENERATION.load(Ordering::SeqCst) == generation {
                hide_recording_overlay(&handle);
            }
        });
    });
}

/// Hides the notice early, unless a new session has replaced it.
pub fn dismiss_overlay_notice(app_handle: &AppHandle) {
    let shown = OVERLAY_SHOW_GENERATION.load(Ordering::SeqCst);
    if OVERLAY_NOTICE_GENERATION.load(Ordering::SeqCst) == shown {
        hide_recording_overlay(app_handle);
    }
}

pub fn is_session_showing() -> bool {
    OVERLAY_SESSION_SHOWING.load(Ordering::SeqCst)
}

/// Updates the overlay window position based on current settings
pub fn update_overlay_position(app_handle: &AppHandle) {
    // Positioning queries monitors/cursor (GDK/Xlib on Linux) and moves the
    // window, so it must run on the main thread — see show_overlay_state.
    let handle = app_handle.clone();
    let _ = app_handle.run_on_main_thread(move || update_overlay_position_on_main(&handle));
}

fn update_overlay_position_on_main(app_handle: &AppHandle) {
    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        #[cfg(target_os = "linux")]
        if LAYER_SHELL_ACTIVE.load(Ordering::SeqCst) {
            let position = settings::get_settings(app_handle).overlay_position;
            match overlay_window.gtk_window() {
                Ok(gtk_window) => configure_layer_shell_position(&gtk_window, position),
                Err(error) => log::error!("Failed to access GTK overlay window: {error}"),
            }
            return;
        }

        #[cfg(target_os = "windows")]
        {
            let state = if WINDOWS_OVERLAY_IS_STREAMING.load(Ordering::Relaxed) {
                "streaming"
            } else {
                "recording"
            };
            let (width, height) = overlay_dimensions(state);
            if let Err(error) = place_windows_overlay(app_handle, &overlay_window, width, height) {
                log::error!("Failed to update recording overlay position: {error}");
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            // Use the window's current size so centering stays correct whether the
            // overlay is in compact or streaming layout.
            let (width, height) = current_overlay_logical_size(&overlay_window)
                .unwrap_or((OVERLAY_WIDTH, OVERLAY_HEIGHT));
            if let Some((x, y)) = calculate_overlay_position(app_handle, width, height) {
                let _ = overlay_window
                    .set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
            }
        }
    }
}

/// Generation counter bumped every time the overlay is shown. The delayed
/// `hide()` below only unmaps the window if no show happened after it was
/// scheduled, so a hide left over from a finished transcription can never
/// take down the overlay of a session that started in the meantime — e.g. a
/// press the coordinator remembered while the pipeline was busy and started
/// the instant it drained, well inside the 300 ms hide delay.
static OVERLAY_SHOW_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Shows the "on the clipboard" notice for `duration`, then hides the overlay
/// unless something else (e.g. a new recording) has shown it in the meantime.
/// Unlike the recording states it ignores the overlay style: the notice tells
/// the user where the text went, which matters even with the overlay off.
pub fn show_clipboard_notice(app_handle: &AppHandle, duration: std::time::Duration) {
    show_timed_state(app_handle, "clipboard", duration);
}

/// The show generation of the latest "ready to insert" notice; it's on screen
/// while this matches `OVERLAY_SHOW_GENERATION`.
static READY_NOTICE_GENERATION: AtomicU64 = AtomicU64::new(u64::MAX);

/// Shows "Ready – <keys> to insert" for `duration` after a dictation that
/// waits for the "Insert Last Dictation" service; `keys` is its shortcut, if
/// it has one. Like the clipboard notice it ignores the overlay style.
pub fn show_ready_notice(
    app_handle: &AppHandle,
    keys: Option<String>,
    duration: std::time::Duration,
) {
    // Sent before the state, so the overlay has the keys when it renders.
    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        let _ = overlay_window.emit("overlay-ready", serde_json::json!({ "keys": keys }));
    }
    // The show below bumps the generation by one.
    READY_NOTICE_GENERATION.store(
        OVERLAY_SHOW_GENERATION.load(Ordering::SeqCst) + 1,
        Ordering::SeqCst,
    );
    show_timed_state(app_handle, "ready", duration);
}

/// Hides the "ready to insert" notice once the dictation was inserted, unless
/// something else has taken the overlay since.
pub fn hide_ready_notice(app_handle: &AppHandle) {
    if READY_NOTICE_GENERATION.load(Ordering::SeqCst)
        == OVERLAY_SHOW_GENERATION.load(Ordering::SeqCst)
    {
        hide_recording_overlay(app_handle);
    }
}

/// Shows "Not ready yet" for `duration`, when the service is called while a
/// dictation is still in progress and the overlay doesn't show it.
pub fn show_not_ready_notice(app_handle: &AppHandle, duration: std::time::Duration) {
    show_timed_state(app_handle, "not_ready", duration);
}

/// Whether the overlay shows a recording or transcribing session right now.
pub fn session_showing() -> bool {
    OVERLAY_SESSION_SHOWING.load(Ordering::SeqCst)
}

/// Shows which additional word list is now in use (`None`: only the global
/// list) for `duration`, like the clipboard notice. The caller skips it while
/// a recording or transcription owns the overlay.
pub fn show_word_list_notice(
    app_handle: &AppHandle,
    name: Option<String>,
    duration: std::time::Duration,
) {
    // Sent before the state, so the overlay has the name when it renders.
    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        let _ = overlay_window.emit("overlay-word-list", serde_json::json!({ "name": name }));
    }
    show_timed_state(app_handle, "word_list", duration);
}

/// DanyWhere: shows a profile for `duration`, as the glyph of choosing the
/// profile by application and the profile's name, like the word list notice:
/// after a switch in the sidebar or the tray, and after the switch's shortcut
/// (`app_profiles`). The caller skips it while a recording or transcription
/// owns the overlay.
pub fn show_profile_notice(
    app_handle: &AppHandle,
    profile: OverlayProfile,
    duration: std::time::Duration,
) {
    // Sent before the state, so the overlay has the profile when it renders.
    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        let _ = overlay_window.emit("overlay-profile", profile);
    }
    show_timed_state(app_handle, "profile", duration);
}

/// DanyWhere: the profile a dictation runs with and the glyph of choosing
/// the profile by application, shown next to its waveform and its working
/// label (`app_profiles`); `None` (a single profile) shows neither. Sent at
/// the start of every dictation.
pub fn set_dictation_profile(app_handle: &AppHandle, profile: Option<OverlayProfile>) {
    if !OVERLAY_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        let _ = overlay_window.emit("overlay-dictation-profile", profile);
    }
}

/// DanyWhere: whether the dictation in progress sends context, shown as a
/// speech bubble next to its profile (`send_context`). Sent at the start of
/// every dictation, as its start shortcut's default end would decide, and
/// once more when it stops, with the end it took (`end_keys`).
pub fn set_dictation_context(app_handle: &AppHandle, sends: bool) {
    if !OVERLAY_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        let _ = overlay_window.emit("overlay-dictation-context", sends);
    }
}

/// Shows `state` for `duration`, then hides the overlay unless something else
/// has shown it in the meantime.
fn show_timed_state(app_handle: &AppHandle, state: &'static str, duration: std::time::Duration) {
    let before = OVERLAY_SHOW_GENERATION.load(Ordering::SeqCst);
    let handle = app_handle.clone();
    let _ = app_handle.run_on_main_thread(move || show_overlay_state_on_main(&handle, state));
    let handle = app_handle.clone();
    std::thread::spawn(move || {
        std::thread::sleep(duration);
        // Our own show bumped the generation by exactly one.
        if OVERLAY_SHOW_GENERATION.load(Ordering::SeqCst) == before + 1 {
            hide_recording_overlay(&handle);
        }
    });
}

/// The show generation of the latest notice; it's on screen while this matches
/// `OVERLAY_SHOW_GENERATION`.
static OVERLAY_NOTICE_GENERATION: AtomicU64 = AtomicU64::new(u64::MAX);

/// Set while the overlay shows a recording or transcribing session.
static OVERLAY_SESSION_SHOWING: AtomicBool = AtomicBool::new(false);

/// Whether the overlay's current session uses the Live window. The style alone
/// can't say: a model that doesn't stream gets the compact pill under Live too.
static OVERLAY_SESSION_LIVE: AtomicBool = AtomicBool::new(false);

/// Hides the recording overlay window with fade-out animation
pub fn hide_recording_overlay(app_handle: &AppHandle) {
    OVERLAY_SESSION_SHOWING.store(false, Ordering::SeqCst);
    OVERLAY_SESSION_LIVE.store(false, Ordering::SeqCst);
    // Always hide the overlay regardless of settings - if setting was changed while recording,
    // we still want to hide it properly
    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        // Snapshot before doing anything observable, so any show that lands
        // after this point invalidates the delayed hide below.
        let scheduled_at = OVERLAY_SHOW_GENERATION.load(Ordering::SeqCst);
        // Emit event to trigger fade-out animation
        let _ = overlay_window.emit("hide-overlay", ());
        // Hide the window after a short delay to allow animation to complete,
        // unless a newer session has shown the overlay again by then.
        let window_clone = overlay_window.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            if OVERLAY_SHOW_GENERATION.load(Ordering::SeqCst) != scheduled_at {
                log::debug!("Skipping stale overlay hide: a newer session is showing the overlay");
                return;
            }
            let _ = window_clone.hide();
        });
    }
}

// Cached "overlay is enabled" flag, kept in sync with overlay_style. Avoids
// reading the Tauri store on every audio callback (~24 Hz during recording).
// Defaults to false so the audio path doesn't emit until lib.rs::setup
// populates the cache from initial settings.
static OVERLAY_ENABLED: AtomicBool = AtomicBool::new(false);

/// Tracks whether gtk-layer-shell was successfully initialized (Linux only).
/// Used to skip layer-shell calls when the window is a regular fallback.
#[cfg(target_os = "linux")]
static LAYER_SHELL_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Update the cached overlay-enabled flag. Called from `lib.rs` at
/// startup after settings load, and from `change_overlay_style_setting`
/// whenever the user changes whether the overlay is shown.
pub fn update_overlay_enabled_cache(enabled: bool) {
    OVERLAY_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn emit_levels(app_handle: &AppHandle, levels: &[f32]) {
    // Skip emission when the overlay is disabled. The recording_overlay
    // window is created at boot regardless of overlay_style, so without this
    // guard a hidden overlay's WebKit subprocess still
    // processes every event. Each event drives some kind of WebKit
    // C++ allocation that accumulates without bound (mechanism not
    // directly characterized; see issue #1279 for the investigation).
    // For users with `overlay_style: none` (the Linux default) this skip
    // eliminates the upstream driver of that accumulation.
    if !OVERLAY_ENABLED.load(Ordering::Relaxed) {
        return;
    }

    // Throttle to ~30 FPS. Even with the overlay enabled, the raw audio
    // callback fires far faster than the UI needs; capping emission rate
    // cuts the per-frame `eval_script`/IPC volume that drives the wry
    // memory growth in issue #1279 (upstream tauri-apps/wry#1489).
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let last = LAST_MIC_LEVEL_EMIT.load(Ordering::Relaxed);
    if now.saturating_sub(last) < EMIT_THROTTLE_MS {
        return;
    }
    LAST_MIC_LEVEL_EMIT.store(now, Ordering::Relaxed);

    // Target only the overlay window. In Tauri 2 both `AppHandle::emit`
    // and `WebviewWindow::emit` broadcast to all webviews; Tauri's
    // listener filter then skips webviews with no registered listener
    // for the event, so the settings webview never received `mic-level`.
    // But the previous dual-call pattern still produced two `eval_script`
    // calls to the overlay per audio callback (one from each .emit()).
    // `emit_to` with the overlay's window label produces a single
    // eval_script call per callback, cutting the per-callback WebKit
    // dispatch work in half.
    let _ = app_handle.emit_to("recording_overlay", "mic-level", levels);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notice_keeps_the_size_of_its_session() {
        for (session, live) in [("streaming", true), ("recording", false)] {
            assert_eq!(
                overlay_dimensions(notice_state(is_live_state(session))),
                overlay_dimensions(session)
            );
            assert_eq!(is_live_state(session), live);
        }
    }

    /// DanyWhere: the notice after turning "Send Context" over is as wide
    /// as the profile notice it looks like.
    #[test]
    fn the_context_notice_gets_the_notice_window() {
        assert_eq!(
            overlay_dimensions("context"),
            (OVERLAY_NOTICE_WIDTH, OVERLAY_HEIGHT)
        );
        assert_eq!(overlay_dimensions("context"), overlay_dimensions("profile"));
    }

    fn rect(x: f64, y: f64, width: f64, height: f64) -> ScreenRect {
        ScreenRect {
            x,
            y,
            width,
            height,
        }
    }

    /// A MacBook (1512x982 points) and a screen to its right, 200 points
    /// higher.
    fn two_screens() -> [ScreenRect; 2] {
        [
            rect(0.0, 0.0, 1512.0, 982.0),
            rect(1512.0, -200.0, 2560.0, 1440.0),
        ]
    }

    #[test]
    fn the_screen_holding_most_of_the_window_gets_the_overlay() {
        let screens = two_screens();
        // 312 points on the left screen, 688 on the right one.
        let window = rect(1200.0, 100.0, 1000.0, 600.0);
        assert_eq!(
            choose_overlay_screen(Some(window), Some((100.0, 100.0)), &screens),
            OverlayScreen::Window(1),
            "the window wins over the cursor"
        );
        let window = rect(100.0, 100.0, 800.0, 600.0);
        assert_eq!(
            choose_overlay_screen(Some(window), Some((2000.0, 0.0)), &screens),
            OverlayScreen::Window(0)
        );
        assert_eq!(
            choose_overlay_screen(Some(window), None, &screens),
            OverlayScreen::Window(0)
        );
    }

    #[test]
    fn without_a_window_the_cursor_picks_the_screen() {
        let screens = two_screens();
        assert_eq!(
            choose_overlay_screen(None, Some((2000.0, -150.0)), &screens),
            OverlayScreen::Cursor(1)
        );
        assert_eq!(
            choose_overlay_screen(None, Some((1511.5, 981.0)), &screens),
            OverlayScreen::Cursor(0)
        );
    }

    /// A minimized window on Windows sits at -32000, -32000.
    #[test]
    fn a_window_on_no_screen_leaves_the_choice_to_the_cursor() {
        let screens = two_screens();
        let window = rect(-32000.0, -32000.0, 160.0, 28.0);
        assert_eq!(
            choose_overlay_screen(Some(window), Some((2000.0, 0.0)), &screens),
            OverlayScreen::Cursor(1)
        );
    }

    #[test]
    fn without_window_or_cursor_on_a_screen_the_main_screen_gets_it() {
        let screens = two_screens();
        assert_eq!(
            choose_overlay_screen(None, None, &screens),
            OverlayScreen::Primary
        );
        assert_eq!(
            choose_overlay_screen(None, Some((9000.0, 9000.0)), &screens),
            OverlayScreen::Primary
        );
        let window = rect(100.0, 100.0, 800.0, 600.0);
        assert_eq!(
            choose_overlay_screen(Some(window), Some((100.0, 100.0)), &[]),
            OverlayScreen::Primary,
            "no screen listed"
        );
    }

    /// A window split exactly in half goes to the screen with the cursor;
    /// without one there, to the first screen in the list.
    #[test]
    fn a_tie_goes_to_the_cursors_screen_else_to_the_first() {
        let screens = [
            rect(0.0, 0.0, 1000.0, 800.0),
            rect(1000.0, 0.0, 1000.0, 800.0),
        ];
        let window = rect(500.0, 100.0, 1000.0, 400.0);
        assert_eq!(
            choose_overlay_screen(Some(window), Some((1500.0, 50.0)), &screens),
            OverlayScreen::Window(1)
        );
        assert_eq!(
            choose_overlay_screen(Some(window), Some((9000.0, 50.0)), &screens),
            OverlayScreen::Window(0)
        );
        assert_eq!(
            choose_overlay_screen(Some(window), None, &screens),
            OverlayScreen::Window(0)
        );
    }

    /// macOS, Retina (2.0) on the left, a plain screen (1.0) to its right:
    /// tao reports each monitor's points times its own scale.
    #[test]
    fn mixed_scales_retina_left_plain_right() {
        let screens = [
            monitor_bounds(
                PhysicalPosition::new(0, 0),
                PhysicalSize::new(3024, 1964),
                2.0,
            ),
            monitor_bounds(
                PhysicalPosition::new(1512, -200),
                PhysicalSize::new(2560, 1440),
                1.0,
            ),
        ];
        assert_eq!(screens, two_screens());
        let window = rect(1700.0, 100.0, 900.0, 600.0);
        assert_eq!(
            choose_overlay_screen(Some(window), Some((100.0, 100.0)), &screens),
            OverlayScreen::Window(1)
        );
    }

    /// macOS, a plain screen on the left and Retina to its right: the Retina
    /// screen starts at 1920 points, which tao reports as 3840.
    #[test]
    fn mixed_scales_plain_left_retina_right() {
        let position = PhysicalPosition::new(3840, 0);
        let size = PhysicalSize::new(3024, 1964);
        let screens = [
            monitor_bounds(
                PhysicalPosition::new(0, 0),
                PhysicalSize::new(1920, 1080),
                1.0,
            ),
            monitor_bounds(position, size, 2.0),
        ];
        assert_eq!(screens[1], rect(1920.0, 0.0, 1512.0, 982.0));
        let window = rect(2000.0, 100.0, 800.0, 600.0);
        assert_eq!(
            choose_overlay_screen(Some(window), None, &screens),
            OverlayScreen::Window(1)
        );
        // Comparing tao's values as they are, the window would be on no
        // screen at all.
        let unscaled = [screens[0], monitor_bounds(position, size, 1.0)];
        assert_ne!(
            choose_overlay_screen(Some(window), None, &unscaled),
            OverlayScreen::Window(1)
        );
    }

    /// macOS, a plain screen left of the Retina main screen and higher, so
    /// its origin is negative.
    #[test]
    fn mixed_scales_with_a_negative_origin() {
        let screens = [
            monitor_bounds(
                PhysicalPosition::new(-2560, -200),
                PhysicalSize::new(2560, 1440),
                1.0,
            ),
            monitor_bounds(
                PhysicalPosition::new(0, 0),
                PhysicalSize::new(3024, 1964),
                2.0,
            ),
        ];
        assert_eq!(screens[0], rect(-2560.0, -200.0, 2560.0, 1440.0));
        assert_eq!(screens[1], rect(0.0, 0.0, 1512.0, 982.0));
        assert_eq!(
            choose_overlay_screen(
                Some(rect(-1800.0, 100.0, 1200.0, 800.0)),
                Some((700.0, 500.0)),
                &screens
            ),
            OverlayScreen::Window(0)
        );
        // 300 points on the left screen, 900 on the main one.
        assert_eq!(
            choose_overlay_screen(
                Some(rect(-300.0, 100.0, 1200.0, 700.0)),
                Some((-1000.0, 500.0)),
                &screens
            ),
            OverlayScreen::Window(1)
        );
    }

    /// Windows: monitors, the cursor and the foreground window's monitor
    /// (from MonitorFromWindow) are all physical pixels, compared unscaled.
    #[test]
    fn on_windows_everything_is_compared_in_physical_pixels() {
        let screens = [
            monitor_bounds(
                PhysicalPosition::new(0, 0),
                PhysicalSize::new(1920, 1080),
                1.0,
            ),
            monitor_bounds(
                PhysicalPosition::new(1920, 0),
                PhysicalSize::new(3840, 2160),
                1.0,
            ),
        ];
        // The second monitor runs at 150 %; MonitorFromWindow reports its
        // bounds as they are.
        let foreground_monitor = rect(1920.0, 0.0, 3840.0, 2160.0);
        assert_eq!(
            choose_overlay_screen(Some(foreground_monitor), Some((100.0, 100.0)), &screens),
            OverlayScreen::Window(1)
        );
        assert_eq!(
            choose_overlay_screen(None, Some((5000.0, 1000.0)), &screens),
            OverlayScreen::Cursor(1)
        );
        // Scaling the bounds down as on macOS would miss a cursor that is
        // visibly on the monitor at 150 %.
        let scaled = monitor_bounds(
            PhysicalPosition::new(1920, 0),
            PhysicalSize::new(3840, 2160),
            1.5,
        );
        assert!(!scaled.contains((5000.0, 1000.0)));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_overlay_bounds_use_destination_monitor_scale() {
        let monitor_position = PhysicalPosition::new(1920, 0);
        let monitor_size = PhysicalSize::new(3840, 2160);

        assert_eq!(
            windows_overlay_bounds(
                monitor_position,
                monitor_size,
                1.5,
                1.0,
                OVERLAY_WIDTH,
                OVERLAY_HEIGHT,
                OverlayPosition::Bottom,
            ),
            (3600, 2025, 480, 75)
        );
        assert_eq!(
            windows_overlay_bounds(
                monitor_position,
                monitor_size,
                1.5,
                1.0,
                OVERLAY_WIDTH,
                OVERLAY_HEIGHT,
                OverlayPosition::Top,
            ),
            (3600, 6, 480, 75)
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_overlay_bounds_support_negative_monitor_origins() {
        assert_eq!(
            windows_overlay_bounds(
                PhysicalPosition::new(-2560, -200),
                PhysicalSize::new(2560, 1440),
                1.25,
                1.0,
                OVERLAY_STREAM_WIDTH,
                OVERLAY_STREAM_HEIGHT,
                OverlayPosition::Bottom,
            ),
            (-1530, 1040, 500, 150)
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_overlay_bounds_grow_with_text_scale_without_moving_the_anchored_edge() {
        let monitor_position = PhysicalPosition::new(-2560, -200);
        let monitor_size = PhysicalSize::new(2560, 1440);

        let (x, y, width, height) = windows_overlay_bounds(
            monitor_position,
            monitor_size,
            1.25,
            1.1,
            OVERLAY_STREAM_WIDTH,
            OVERLAY_STREAM_HEIGHT,
            OverlayPosition::Bottom,
        );
        // 400x120 logical at 1.25 DPI x 1.1 text, still centered horizontally.
        assert_eq!((x, y, width, height), (-1555, 1025, 550, 165));
        // Bottom edge unchanged from the 1.0 case above (1040 + 150).
        assert_eq!(y + height, 1190);

        let (_, top_y, _, _) = windows_overlay_bounds(
            monitor_position,
            monitor_size,
            1.25,
            1.1,
            OVERLAY_STREAM_WIDTH,
            OVERLAY_STREAM_HEIGHT,
            OverlayPosition::Top,
        );
        // Top offset rides the DPI scale alone, so the top edge doesn't move.
        assert_eq!(top_y, -195);
    }
}
