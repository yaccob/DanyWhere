use crate::TranscriptionCoordinator;
#[cfg(unix)]
use log::debug;
use log::warn;
use tauri::{AppHandle, Manager};

#[cfg(target_os = "macos")]
use signal_hook::consts::SIGUSR1;
#[cfg(unix)]
use signal_hook::consts::SIGUSR2;
#[cfg(unix)]
use signal_hook::iterator::Signals;
#[cfg(unix)]
use std::thread;

/// Send a transcription input to the coordinator.
/// Used by signal handlers, CLI flags, and any other external trigger.
/// DanyWhere: it toggles as a tap of the first start shortcut, in the
/// profile that applies now; with `post_process` its end is as after an
/// instruction shortcut without an instruction (`start_keys::remote_toggle`).
pub fn send_transcription_input(app: &AppHandle, post_process: bool, source: &str) {
    let Some(c) = app.try_state::<TranscriptionCoordinator>() else {
        warn!("TranscriptionCoordinator not initialized");
        return;
    };
    let settings = crate::settings::get_settings(app);
    let frontmost = crate::app_profiles::frontmost_if_choosing(&settings);
    let (_, values) = crate::app_profiles::profile_now(&settings, frontmost.as_deref());
    match crate::start_keys::remote_toggle(&values, post_process) {
        Some((binding_id, end)) => c.send_external_input(&binding_id, source, end),
        None => warn!("{source}: no start or hold shortcut to toggle as"),
    }
}

/// Listen for Unix signals that remotely toggle transcription.
///
/// SIGUSR2 toggles plain transcription on all Unix platforms. SIGUSR1
/// (transcription with post-processing) is only handled on macOS: on Linux,
/// WebKitGTK's JavaScriptCore garbage collector sends SIGUSR1 to its own
/// threads to suspend them, so handling it caused phantom recordings on every
/// GC cycle (#1660). Linux users should use `handy --toggle-post-process`
/// instead.
#[cfg(unix)]
pub fn setup_signal_handler(app_handle: AppHandle) {
    #[cfg(target_os = "macos")]
    let mut signals =
        Signals::new([SIGUSR1, SIGUSR2]).expect("failed to register transcription signal handlers");
    #[cfg(not(target_os = "macos"))]
    let mut signals =
        Signals::new([SIGUSR2]).expect("failed to register transcription signal handlers");
    #[cfg(target_os = "macos")]
    debug!("Signal handlers registered (SIGUSR1, SIGUSR2)");
    #[cfg(not(target_os = "macos"))]
    debug!("Signal handler registered (SIGUSR2; SIGUSR1 is left to WebKitGTK)");
    thread::spawn(move || {
        for sig in signals.forever() {
            let (post_process, signal_name) = match sig {
                #[cfg(target_os = "macos")]
                SIGUSR1 => (true, "SIGUSR1"),
                SIGUSR2 => (false, "SIGUSR2"),
                _ => continue,
            };
            debug!("Received {signal_name}");
            send_transcription_input(&app_handle, post_process, signal_name);
        }
    });
}
