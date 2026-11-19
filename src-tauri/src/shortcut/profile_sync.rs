//! DanyWhere: the shortcuts follow the profile that applies now (idea 3;
//! user, 2026-11-19, T1 and F3).
//!
//! The shortcuts are profile values (`ProfileValues::bindings`), and only
//! the profile that applies now has its start shortcuts registered: the
//! profile of the application in front while "Choose Profile by
//! Application" is on, else the active one (`app_profiles::profile_now`).
//! Another profile's shortcuts reach the application. End shortcuts with a
//! key combination are registered only while a recording runs, like Escape,
//! again those of the profile that applies now (F3: after an application
//! switch during the recording, the new profile's).
//!
//! DanyWhere keeps what it registered in one table and re-registers only
//! what differs from what should be registered now ([`desired`],
//! [`plan`]), on a profile switch, fixing or unfixing a profile, the
//! "profile by application" toggle, a changed shortcut, a recording's start
//! and end, and whenever another application comes to the front (the
//! observer, [`start_observer`]).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::AppHandle;

use crate::settings::{AppSettings, ShortcutBinding};

/// What is registered, by binding id.
pub type Registered = BTreeMap<String, ShortcutBinding>;

/// The shortcuts to have registered now. `settings_now` hold the values of
/// the profile that applies now (`app_profiles::settings_now`); `recording`
/// adds its end shortcuts. The cancel shortcut keeps its own handling
/// (`register_cancel_shortcut`); `service_available` is
/// `delivery::INSERT_SERVICE_AVAILABLE` (see `settings::registers_binding`).
pub fn desired(settings_now: &AppSettings, recording: bool, service_available: bool) -> Registered {
    let mut wanted = Registered::new();
    // The bindings the backends register at their start, as they do.
    for (id, default) in crate::settings::get_default_settings().bindings {
        if id == CANCEL || !crate::settings::registers_binding(&id, settings_now, service_available)
        {
            continue;
        }
        let binding = settings_now.bindings.get(&id).cloned().unwrap_or(default);
        wanted.insert(id, binding);
    }
    if recording && END_KEYS_WHILE_RECORDING {
        let mut taken: Vec<String> = wanted
            .values()
            .chain(settings_now.bindings.get(CANCEL))
            .map(keys_of)
            .collect();
        for binding in crate::end_keys::bindings_to_register(&settings_now.end_keys) {
            let keys = keys_of(&binding);
            if taken.contains(&keys) {
                log::warn!(
                    "End shortcut {}: {} is another shortcut's; not registered",
                    binding.id,
                    binding.current_binding
                );
                continue;
            }
            taken.push(keys);
            wanted.insert(binding.id.clone(), binding);
        }
    }
    wanted
}

/// The cancel shortcut's binding id.
const CANCEL: &str = "cancel";

/// Whether end shortcuts are registered during a recording. Not on Linux,
/// where shortcuts registered only while recording are unstable (Handy keeps
/// Escape off there for the same reason).
const END_KEYS_WHILE_RECORDING: bool = !cfg!(target_os = "linux");

/// A binding's key combination, for comparing.
fn keys_of(binding: &ShortcutBinding) -> String {
    binding.current_binding.trim().to_lowercase()
}

/// What to change to get from `registered` to `desired`: first unregister,
/// then register. Bindings in `frozen` are left as they are (the dictation
/// shortcuts released during a locked recording without Accessibility,
/// `dictation_key_inserts`).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub unregister: Vec<ShortcutBinding>,
    pub register: Vec<ShortcutBinding>,
}

/// See [`Plan`]. A binding counts as unchanged while its key combination is.
pub fn plan(registered: &Registered, desired: &Registered, frozen: &[&str]) -> Plan {
    let same = |a: &ShortcutBinding, b: Option<&ShortcutBinding>| {
        b.is_some_and(|b| keys_of(a) == keys_of(b))
    };
    let free = |id: &String| !frozen.contains(&id.as_str());
    Plan {
        unregister: registered
            .iter()
            .filter(|(id, binding)| free(id) && !same(binding, desired.get(*id)))
            .map(|(_, binding)| binding.clone())
            .collect(),
        register: desired
            .iter()
            .filter(|(id, binding)| free(id) && !same(binding, registered.get(*id)))
            .map(|(_, binding)| binding.clone())
            .collect(),
    }
}

/// What DanyWhere registered of the shortcuts it manages here.
static REGISTERED: Mutex<Registered> = Mutex::new(BTreeMap::new());
/// Whether a recording runs (its end shortcuts are wanted).
static RECORDING: AtomicBool = AtomicBool::new(false);
/// Whether a shortcut is being recorded in the settings: nothing registered.
static SUSPENDED: AtomicBool = AtomicBool::new(false);

fn registered_lock() -> std::sync::MutexGuard<'static, Registered> {
    REGISTERED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Records a registration made outside this module (the backends' start).
pub fn note_registered(binding: &ShortcutBinding) {
    registered_lock().insert(binding.id.clone(), binding.clone());
}

/// Forgets every registration (the backend is being switched); returns them.
pub fn take_all() -> Vec<ShortcutBinding> {
    std::mem::take(&mut *registered_lock())
        .into_values()
        .collect()
}

/// Registers and unregisters what differs from what should be registered
/// now. Returns the bindings that could not be registered, with the error.
/// Never call it on the shortcut handler thread (`off_handler_thread`).
pub fn sync_now(app: &AppHandle) -> Vec<(String, String)> {
    let mut registered = registered_lock();
    if SUSPENDED.load(Ordering::SeqCst) {
        return Vec::new();
    }
    let settings = crate::settings::get_settings(app);
    if settings.keyboard_implementation == crate::settings::KeyboardImplementation::None {
        return Vec::new();
    }
    let frontmost = crate::app_profiles::frontmost_if_choosing(&settings);
    let now = crate::app_profiles::settings_now(&settings, frontmost.as_deref());
    let wanted = desired(
        &now,
        RECORDING.load(Ordering::SeqCst),
        crate::delivery::INSERT_SERVICE_AVAILABLE,
    );
    let frozen = crate::dictation_key_inserts::released_bindings();
    let plan = plan(&registered, &wanted, &frozen);
    if plan == Plan::default() {
        return Vec::new();
    }
    log::debug!(
        "Shortcuts for {}: unregistering {:?}, registering {:?}",
        frontmost.as_deref().unwrap_or("the active profile"),
        plan.unregister.iter().map(|b| &b.id).collect::<Vec<_>>(),
        plan.register.iter().map(|b| &b.id).collect::<Vec<_>>()
    );
    for binding in plan.unregister {
        // One that is gone already (a backend switch) fails harmlessly.
        if let Err(err) = super::unregister_shortcut(app, binding.clone()) {
            log::debug!("Unregistering {}: {err}", binding.id);
        }
        registered.remove(&binding.id);
    }
    let mut failures = Vec::new();
    for binding in plan.register {
        match super::register_shortcut(app, binding.clone()) {
            Ok(()) => {
                registered.insert(binding.id.clone(), binding);
            }
            Err(err) => {
                log::warn!(
                    "Couldn't register {} ({}): {err}",
                    binding.id,
                    binding.current_binding
                );
                failures.push((binding.id, err));
            }
        }
    }
    drop(registered);
    crate::secure_input::reconcile_fallback(app);
    failures
}

/// The re-registration after a change of the settings in effect (contract
/// V4 of the build plan, called after a profile switch and by whoever
/// changes coupled values): `after` are the stored settings now. What is
/// registered is known here, so `before` only goes into the log.
pub fn sync_profile_shortcuts(app: &AppHandle, before: &AppSettings, after: &AppSettings) {
    if before.active_profile_id != after.active_profile_id {
        log::debug!(
            "Shortcuts follow profile {} (was {})",
            after.active_profile_id,
            before.active_profile_id
        );
    }
    sync_now(app);
}

/// A recording began or ended: its end shortcuts are registered or
/// unregistered off the calling thread (the coordinator's, which must not
/// wait for a registration); every pass applies the latest state.
pub fn set_recording(app: &AppHandle, recording: bool) {
    RECORDING.store(recording, Ordering::SeqCst);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        sync_now(&app);
    });
}

/// While a shortcut is recorded in the settings, nothing is registered.
pub fn suspend(app: &AppHandle) {
    let mut registered = registered_lock();
    SUSPENDED.store(true, Ordering::SeqCst);
    for (id, binding) in std::mem::take(&mut *registered) {
        if let Err(err) = super::unregister_shortcut(app, binding) {
            log::debug!("Suspending {id}: {err}");
        }
    }
}

/// After recording a shortcut: everything that should be registered is.
pub fn resume(app: &AppHandle) -> Vec<(String, String)> {
    SUSPENDED.store(false, Ordering::SeqCst);
    sync_now(app)
}

/// Watches which application comes to the front and re-registers then.
/// macOS: `NSWorkspaceDidActivateApplicationNotification`, delivered on
/// the main thread, which only wakes a worker. Windows: the foreground
/// application, looked at every `FRONT_APP_POLL`. Elsewhere none: the
/// shortcuts follow the active profile only.
pub fn start_observer(app: &AppHandle) {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let (wake, woken) = std::sync::mpsc::channel::<()>();
    let worker_app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("profile-shortcuts".into())
        .spawn(move || {
            while woken.recv().is_ok() {
                // Several activations in a row need one pass.
                while woken.try_recv().is_ok() {}
                sync_now(&worker_app);
            }
        });
    if let Err(err) = spawned {
        log::error!("Couldn't start following the application in front: {err}");
        return;
    }
    observe(app, wake);
}

#[cfg(target_os = "macos")]
fn observe(_app: &AppHandle, wake: std::sync::mpsc::Sender<()>) {
    use block2::RcBlock;
    use objc2_app_kit::{NSWorkspace, NSWorkspaceDidActivateApplicationNotification};
    use objc2_foundation::NSNotification;

    let block = RcBlock::new(move |_: std::ptr::NonNull<NSNotification>| {
        let _ = wake.send(());
    });
    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    // SAFETY: the name is AppKit's constant; no object filter and no queue
    // (delivered on the posting thread, AppKit's main thread); the block
    // only sends on a channel, which is thread-safe.
    let token = unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceDidActivateApplicationNotification),
            None,
            None,
            &block,
        )
    };
    // Observed for the app's whole life.
    std::mem::forget(token);
    log::info!("Shortcuts follow the application in front");
}

/// How often Windows looks which application is in front.
#[cfg(target_os = "windows")]
const FRONT_APP_POLL: std::time::Duration = std::time::Duration::from_millis(300);

#[cfg(target_os = "windows")]
fn observe(_app: &AppHandle, wake: std::sync::mpsc::Sender<()>) {
    let spawned = std::thread::Builder::new()
        .name("front-app-poll".into())
        .spawn(move || {
            let mut last = crate::window_capture::frontmost_app().map(|(id, _)| id);
            loop {
                std::thread::sleep(FRONT_APP_POLL);
                let front = crate::window_capture::frontmost_app().map(|(id, _)| id);
                if front != last {
                    last = front;
                    if wake.send(()).is_err() {
                        return;
                    }
                }
            }
        });
    if let Err(err) = spawned {
        log::error!("Couldn't watch the application in front: {err}");
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn observe(_app: &AppHandle, wake: std::sync::mpsc::Sender<()>) {
    drop(wake);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::end_keys::EndKey;
    use crate::profiles::ensure_profiles;
    use crate::settings::get_default_settings;

    fn with_shortcut(mut settings: AppSettings, id: &str, keys: &str) -> AppSettings {
        settings
            .bindings
            .get_mut(id)
            .expect("a default binding")
            .current_binding = keys.to_string();
        settings
    }

    fn send_key(shortcut: &str) -> EndKey {
        EndKey {
            id: "send".to_string(),
            name: "Send".to_string(),
            shortcut: shortcut.to_string(),
            append_text: "{Enter}".to_string(),
            post_process: false,
            send_context: false,
        }
    }

    /// The default profile (active, ⌃Space) and "work" (⌥Space, an end
    /// shortcut ⌃↩) assigned to VS Code.
    fn two_profiles() -> AppSettings {
        let mut s = get_default_settings();
        s.post_process_enabled = true;
        s = with_shortcut(s, "transcribe", "ctrl+space");
        ensure_profiles(&mut s);
        let mut work = s.profiles[0].clone();
        work.id = "work".to_string();
        work.name = "Work".to_string();
        work.apps = vec!["com.microsoft.VSCode".to_string()];
        work.values
            .bindings
            .get_mut("transcribe")
            .unwrap()
            .current_binding = "option+space".to_string();
        work.values.end_keys.push(send_key("ctrl+enter"));
        s.profiles.push(work);
        s
    }

    fn keys(set: &Registered, id: &str) -> Option<String> {
        set.get(id).map(|binding| binding.current_binding.clone())
    }

    #[test]
    fn only_the_start_shortcuts_of_the_profile_that_applies_now_are_wanted() {
        let s = two_profiles();
        let here = crate::app_profiles::settings_now(&s, Some("com.apple.TextEdit"));
        let wanted = desired(&here, false, true);
        assert_eq!(keys(&wanted, "transcribe").as_deref(), Some("ctrl+space"));
        let there = crate::app_profiles::settings_now(&s, Some("com.microsoft.VSCode"));
        let wanted = desired(&there, false, true);
        assert_eq!(keys(&wanted, "transcribe").as_deref(), Some("option+space"));
        assert!(wanted.contains_key("paste_last_transcript"));
    }

    #[test]
    fn the_cancel_shortcut_keeps_its_own_handling() {
        let s = two_profiles();
        assert!(!desired(&s, true, true).contains_key("cancel"));
    }

    #[test]
    fn the_post_processing_start_shortcut_only_while_some_dictation_can_post_process() {
        let mut s = two_profiles();
        assert!(desired(&s, false, true).contains_key("transcribe_with_post_process"));
        s.post_process_enabled = false;
        s.profiles[1].values.post_process_enabled = false;
        assert!(!desired(&s, false, true).contains_key("transcribe_with_post_process"));
    }

    #[test]
    fn end_shortcuts_only_while_a_recording_runs() {
        let s = two_profiles();
        let there = crate::app_profiles::settings_now(&s, Some("com.microsoft.VSCode"));
        assert!(!desired(&there, false, true).contains_key("end_key:send"));
        let recording = desired(&there, true, true);
        if cfg!(target_os = "linux") {
            // Like Escape: no shortcuts registered only while recording.
            assert!(!recording.contains_key("end_key:send"));
        } else {
            assert_eq!(
                keys(&recording, "end_key:send").as_deref(),
                Some("ctrl+enter")
            );
        }
        // The other profile has none.
        let here = crate::app_profiles::settings_now(&s, Some("com.apple.TextEdit"));
        assert!(!desired(&here, true, true).contains_key("end_key:send"));
    }

    #[test]
    fn an_end_shortcut_on_a_start_shortcuts_keys_is_left_out() {
        let mut s = two_profiles();
        s.profiles[1].values.end_keys = vec![send_key("option+space")];
        let there = crate::app_profiles::settings_now(&s, Some("com.microsoft.VSCode"));
        let wanted = desired(&there, true, true);
        assert!(!wanted.contains_key("end_key:send"));
        assert_eq!(keys(&wanted, "transcribe").as_deref(), Some("option+space"));
    }

    fn binding(id: &str, keys: &str) -> ShortcutBinding {
        ShortcutBinding {
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
            default_binding: keys.to_string(),
            current_binding: keys.to_string(),
        }
    }

    fn set(bindings: &[(&str, &str)]) -> Registered {
        bindings
            .iter()
            .map(|(id, keys)| (id.to_string(), binding(id, keys)))
            .collect()
    }

    #[test]
    fn the_plan_changes_only_what_differs() {
        let registered = set(&[
            ("transcribe", "ctrl+space"),
            ("cycle_word_list", "ctrl+shift+l"),
            ("end_key:send", "ctrl+enter"),
        ]);
        let wanted = set(&[
            ("transcribe", "option+space"),
            ("cycle_word_list", "ctrl+shift+l"),
            ("paste_last_transcript", "ctrl+shift+v"),
        ]);
        let plan = plan(&registered, &wanted, &[]);
        let ids = |list: &[ShortcutBinding]| -> Vec<(String, String)> {
            list.iter()
                .map(|b| (b.id.clone(), b.current_binding.clone()))
                .collect()
        };
        assert_eq!(
            ids(&plan.unregister),
            vec![
                ("end_key:send".to_string(), "ctrl+enter".to_string()),
                ("transcribe".to_string(), "ctrl+space".to_string()),
            ]
        );
        assert_eq!(
            ids(&plan.register),
            vec![
                (
                    "paste_last_transcript".to_string(),
                    "ctrl+shift+v".to_string()
                ),
                ("transcribe".to_string(), "option+space".to_string()),
            ]
        );
    }

    #[test]
    fn the_same_set_needs_nothing() {
        let registered = set(&[("transcribe", "ctrl+space")]);
        assert_eq!(plan(&registered, &registered.clone(), &[]), Plan::default());
    }

    #[test]
    fn released_dictation_shortcuts_are_left_alone() {
        let registered = set(&[("transcribe", "ctrl+space")]);
        let wanted = set(&[("transcribe", "option+space")]);
        assert_eq!(plan(&registered, &wanted, &["transcribe"]), Plan::default());
        assert_eq!(
            plan(&Registered::new(), &wanted, &["transcribe"]),
            Plan::default()
        );
    }
}
