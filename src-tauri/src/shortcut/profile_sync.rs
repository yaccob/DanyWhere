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

use super::backend::{register_all, unregister_all, ShortcutBackend};
use crate::settings::{AppSettings, KeyboardImplementation, ShortcutBinding};

/// What is registered, by binding id.
pub type Registered = BTreeMap<String, ShortcutBinding>;

/// The shortcuts to have registered now. `settings_now` hold the values of
/// the profile that applies now (`app_profiles::settings_now`); `recording`
/// adds its end shortcuts. The cancel shortcut keeps its own handling
/// (`register_cancel_shortcut`); `service_available` is
/// `delivery::INSERT_SERVICE_AVAILABLE` (see `settings::registers_binding`).
pub fn desired(settings_now: &AppSettings, recording: bool, service_available: bool) -> Registered {
    // The bindings the backends register at their start, as they do.
    let mut wanted: Registered = super::backend::start_bindings(settings_now, service_available)
        .into_iter()
        .map(|binding| (binding.id.clone(), binding))
        .collect();
    if recording && END_KEYS_WHILE_RECORDING {
        let mut taken: Vec<(String, String)> = wanted
            .values()
            .chain(settings_now.bindings.get(CANCEL))
            .map(|binding| (binding.id.clone(), keys_of(binding)))
            .collect();
        for binding in crate::end_keys::bindings_to_register(&settings_now.end_keys) {
            let keys = keys_of(&binding);
            if let Some((id, _)) = taken
                .iter()
                .find(|(_, other)| crate::key_sides::same_press(other, &keys))
            {
                // One with exactly a start shortcut's keys is that start
                // shortcut's press (`end_keys`); the coordinator ends with it.
                if crate::end_keys::START_BINDINGS.contains(&id.as_str()) {
                    log::debug!("End shortcut {}: ends as {id}'s press", binding.id);
                } else {
                    log::warn!(
                        "End shortcut {}: {} is {id}'s; not registered",
                        binding.id,
                        binding.current_binding
                    );
                }
                continue;
            }
            taken.push((binding.id.clone(), keys));
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

/// What DanyWhere registered of the shortcuts it manages here, and whether
/// a shortcut is being recorded in the settings (nothing registered then).
/// The app keeps one ([`FOLLOWING`]); the tests their own.
#[derive(Debug, Default)]
pub(crate) struct Following {
    registered: Registered,
    suspended: bool,
}

/// Where a pass of [`Following::sync`] is to arrive.
pub(crate) struct Target {
    /// The shortcuts to have registered ([`desired`]).
    pub wanted: Registered,
    /// Those to leave as they are ([`plan`]).
    pub frozen: Vec<&'static str>,
    /// For whom, in the log: the application in front or the active profile.
    pub label: String,
}

impl Following {
    pub const fn new() -> Self {
        Self {
            registered: BTreeMap::new(),
            suspended: false,
        }
    }

    /// Records a registration made outside (the backends' start).
    pub fn note_registered(&mut self, binding: &ShortcutBinding) {
        self.registered.insert(binding.id.clone(), binding.clone());
    }

    /// Forgets every registration (the backend is being switched); returns
    /// them.
    pub fn take_all(&mut self) -> Vec<ShortcutBinding> {
        std::mem::take(&mut self.registered).into_values().collect()
    }

    /// One pass: nothing while suspended or with the "None" backend; else
    /// registers and unregisters what differs from `target()`, which is
    /// asked only then (it may ask the system which application is in
    /// front). `None` when nothing changed; else the bindings that could
    /// not be registered, with the error.
    pub fn sync(
        &mut self,
        backend: &dyn ShortcutBackend,
        target: impl FnOnce() -> Target,
    ) -> Option<Vec<(String, String)>> {
        if self.suspended || backend.implementation() == KeyboardImplementation::None {
            return None;
        }
        let target = target();
        let plan = plan(&self.registered, &target.wanted, &target.frozen);
        if plan == Plan::default() {
            return None;
        }
        log::debug!(
            "Shortcuts for {}: unregistering {:?}, registering {:?}",
            target.label,
            plan.unregister.iter().map(|b| &b.id).collect::<Vec<_>>(),
            plan.register.iter().map(|b| &b.id).collect::<Vec<_>>()
        );
        // One that is gone already (a backend switch) fails harmlessly.
        unregister_all(backend, &plan.unregister, |binding, err| {
            log::debug!("Unregistering {}: {err}", binding.id)
        });
        for binding in &plan.unregister {
            self.registered.remove(&binding.id);
        }
        let report = register_all(backend, &plan.register, |binding, err| {
            log::warn!(
                "Couldn't register {} ({}): {err}",
                binding.id,
                binding.current_binding
            )
        });
        for binding in report.registered {
            self.registered.insert(binding.id.clone(), binding);
        }
        Some(
            report
                .failed
                .into_iter()
                .map(|(binding, err)| (binding.id, err))
                .collect(),
        )
    }

    /// A shortcut is being recorded in the settings: unregisters everything
    /// and registers nothing until [`Following::resume`].
    pub fn suspend(&mut self, backend: &dyn ShortcutBackend) {
        self.suspended = true;
        let suspended = self.take_all();
        unregister_all(backend, &suspended, |binding, err| {
            log::debug!("Suspending {}: {err}", binding.id)
        });
    }

    /// The recording of a shortcut ended; the next pass registers again.
    pub fn resume(&mut self) {
        self.suspended = false;
    }
}

/// The shortcuts DanyWhere follows the profile with.
static FOLLOWING: Mutex<Following> = Mutex::new(Following::new());
/// Whether a recording runs (its end shortcuts are wanted).
static RECORDING: AtomicBool = AtomicBool::new(false);

fn following_lock() -> std::sync::MutexGuard<'static, Following> {
    FOLLOWING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Records a registration made outside this module (the backends' start).
pub fn note_registered(binding: &ShortcutBinding) {
    following_lock().note_registered(binding);
}

/// Forgets every registration (the backend is being switched); returns them.
pub fn take_all() -> Vec<ShortcutBinding> {
    following_lock().take_all()
}

/// Registers and unregisters what differs from what should be registered
/// now. Returns the bindings that could not be registered, with the error.
/// Never call it on the shortcut handler thread (`off_handler_thread`).
pub fn sync_now(app: &AppHandle) -> Vec<(String, String)> {
    let mut following = following_lock();
    // Before reading the settings, as ever (`sync` checks it too).
    if following.suspended {
        return Vec::new();
    }
    let settings = crate::settings::get_settings(app);
    let backend = super::backend::for_implementation(app, settings.keyboard_implementation);
    let failures = following.sync(backend.as_ref(), || {
        let frontmost = crate::app_profiles::frontmost_if_choosing(&settings);
        let now = crate::app_profiles::settings_now(&settings, frontmost.as_deref());
        Target {
            wanted: desired(
                &now,
                RECORDING.load(Ordering::SeqCst),
                crate::delivery::INSERT_SERVICE_AVAILABLE,
            ),
            frozen: crate::dictation_key_inserts::released_bindings(),
            label: frontmost.unwrap_or_else(|| "the active profile".to_string()),
        }
    });
    drop(following);
    let Some(failures) = failures else {
        return Vec::new();
    };
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
    let mut following = following_lock();
    let backend = super::backend::active(app);
    following.suspend(backend.as_ref());
}

/// After recording a shortcut: everything that should be registered is.
pub fn resume(app: &AppHandle) -> Vec<(String, String)> {
    following_lock().resume();
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
    /// A new installation's settings with the two built-in ends these tests
    /// are written against (`end_keys::with_builtin_ends`): macOS has other
    /// factory ends since 2026-11-24.
    fn get_default_settings() -> crate::settings::AppSettings {
        crate::end_keys::with_builtin_ends(crate::settings::get_default_settings())
    }

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

    /// The defaults (in use, ⌃Space) and "work" (⌥Space, an end shortcut
    /// ⌃↩, its own values) assigned to VS Code.
    fn two_profiles() -> AppSettings {
        let mut s = get_default_settings();
        s.post_process_enabled = true;
        s = with_shortcut(s, "transcribe", "ctrl+space");
        ensure_profiles(&mut s);
        let mut work = crate::profiles::profile_for_test(&s, "work", "Work", |values| {
            values
                .bindings
                .get_mut("transcribe")
                .unwrap()
                .current_binding = "option+space".to_string();
            values.end_keys.push(send_key("ctrl+enter"));
        });
        work.apps = vec!["com.microsoft.VSCode".to_string()];
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
        s.profiles[0].values.post_process_enabled = false;
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
        s.profiles[0].values.end_keys = vec![send_key("option+space")];
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

    // ---- passes, and suspending while a shortcut is recorded ----

    use crate::shortcut::backend::fake::{CallLog, RecordingBackend};
    use crate::shortcut::backend::NoShortcuts;

    fn target(bindings: &[(&str, &str)]) -> Target {
        Target {
            wanted: set(bindings),
            frozen: Vec::new(),
            label: "the active profile".to_string(),
        }
    }

    fn following(bindings: &[(&str, &str)]) -> Following {
        let mut following = Following::new();
        for (id, keys) in bindings {
            following.note_registered(&binding(id, keys));
        }
        following
    }

    fn registered_keys(following: &mut Following) -> Vec<(String, String)> {
        let all = following.take_all();
        for binding in &all {
            following.note_registered(binding);
        }
        all.into_iter().map(|b| (b.id, b.current_binding)).collect()
    }

    #[test]
    fn a_pass_registers_and_unregisters_what_differs_and_remembers_it() {
        let log = CallLog::default();
        let backend = RecordingBackend::new(KeyboardImplementation::HandyKeys, &log);
        let mut following =
            following(&[("transcribe", "ctrl+space"), ("end_key:send", "ctrl+enter")]);
        let failures = following.sync(&backend, || {
            target(&[
                ("transcribe", "option+space"),
                ("cycle_word_list", "ctrl+shift+l"),
            ])
        });
        assert_eq!(failures, Some(Vec::new()));
        assert_eq!(
            log.lines(),
            [
                "handy_keys: unregister end_key:send ctrl+enter",
                "handy_keys: unregister transcribe ctrl+space",
                "handy_keys: register cycle_word_list ctrl+shift+l",
                "handy_keys: register transcribe option+space",
            ]
        );
        assert_eq!(
            registered_keys(&mut following),
            [
                ("cycle_word_list".to_string(), "ctrl+shift+l".to_string()),
                ("transcribe".to_string(), "option+space".to_string()),
            ]
        );
    }

    #[test]
    fn a_refused_registration_is_reported_and_a_refused_unregistration_forgotten() {
        let log = CallLog::default();
        let backend = RecordingBackend::new(KeyboardImplementation::Tauri, &log)
            .refusing_register("cycle_word_list")
            .refusing_unregister("end_key:send");
        let mut following = following(&[("end_key:send", "ctrl+enter")]);
        let failures = following.sync(&backend, || {
            target(&[
                ("transcribe", "ctrl+space"),
                ("cycle_word_list", "ctrl+shift+l"),
            ])
        });
        assert_eq!(
            failures,
            Some(vec![(
                "cycle_word_list".to_string(),
                "cycle_word_list refused".to_string()
            )])
        );
        // Gone already after a backend switch, say: forgotten all the same.
        assert_eq!(
            registered_keys(&mut following),
            [("transcribe".to_string(), "ctrl+space".to_string())]
        );
        // The refused one is tried again on the next pass.
        let again = following.sync(&backend, || {
            target(&[
                ("transcribe", "ctrl+space"),
                ("cycle_word_list", "ctrl+shift+l"),
            ])
        });
        assert!(again.is_some());
        assert_eq!(
            log.lines().last().map(String::as_str),
            Some("tauri: register cycle_word_list ctrl+shift+l")
        );
    }

    #[test]
    fn while_a_shortcut_is_recorded_nothing_is_registered_until_resumed() {
        let log = CallLog::default();
        let backend = RecordingBackend::new(KeyboardImplementation::HandyKeys, &log);
        let mut following = following(&[
            ("transcribe", "ctrl+space"),
            ("cycle_word_list", "ctrl+shift+l"),
        ]);
        following.suspend(&backend);
        assert_eq!(
            log.lines(),
            [
                "handy_keys: unregister cycle_word_list ctrl+shift+l",
                "handy_keys: unregister transcribe ctrl+space",
            ]
        );
        assert!(registered_keys(&mut following).is_empty());

        let mut asked = false;
        let suspended = following.sync(&backend, || {
            asked = true;
            target(&[("transcribe", "ctrl+space")])
        });
        assert_eq!(suspended, None);
        assert!(!asked, "while suspended, the target isn't even asked");
        assert_eq!(log.lines().len(), 2, "{:?}", log.lines());

        following.resume();
        let resumed = following.sync(&backend, || target(&[("transcribe", "ctrl+space")]));
        assert_eq!(resumed, Some(Vec::new()));
        assert_eq!(
            log.lines().last().map(String::as_str),
            Some("handy_keys: register transcribe ctrl+space")
        );
        assert_eq!(
            registered_keys(&mut following),
            [("transcribe".to_string(), "ctrl+space".to_string())]
        );
    }

    #[test]
    fn with_no_backend_a_pass_changes_nothing() {
        let mut following = Following::new();
        let mut asked = false;
        let result = following.sync(&NoShortcuts, || {
            asked = true;
            target(&[("transcribe", "ctrl+space")])
        });
        assert_eq!(result, None);
        assert!(!asked);
        assert!(registered_keys(&mut following).is_empty());
    }

    #[test]
    fn a_pass_without_differences_changes_nothing() {
        let log = CallLog::default();
        let backend = RecordingBackend::new(KeyboardImplementation::Tauri, &log);
        let mut following = following(&[("transcribe", "ctrl+space")]);
        let result = following.sync(&backend, || target(&[("transcribe", "ctrl+space")]));
        assert_eq!(result, None);
        assert!(log.lines().is_empty());
    }

    #[test]
    fn the_table_holds_what_was_noted_until_it_is_taken() {
        let mut following = following(&[("transcribe", "ctrl+space")]);
        let taken = following.take_all();
        assert_eq!(taken, vec![binding("transcribe", "ctrl+space")]);
        assert!(following.take_all().is_empty());
    }
}
