//! DanyWhere: starting and switching the shortcut backend, and registering
//! the cancel shortcut (test redesign, strand C1, 2026-11-22).
//!
//! The steps run against a [`ShortcutHost`]: the app in production
//! ([`AppHost`]), a recording fake in the tests. So the fallback to Tauri
//! when handy-keys can't start, the rollback of a failed switch and what
//! "None" does are tested without the app.

use log::{error, info, warn};
use tauri::{AppHandle, Emitter, Manager};

use super::backend::{self, ShortcutBackend};
use super::{handy_keys, profile_sync, ImplementationChangeResult};
use crate::settings::{self, AppSettings, KeyboardImplementation, ShortcutBinding};

/// What starting and switching the backend need from the app.
pub(crate) trait ShortcutHost {
    /// The stored settings (`settings::get_settings`).
    fn settings(&self) -> AppSettings;
    /// The same, as a backend's start reads them
    /// (`settings::load_or_create_app_settings`, which logs them).
    fn load_settings(&self) -> AppSettings;
    fn write_settings(&self, settings: AppSettings);
    fn backend(&self, implementation: KeyboardImplementation) -> Box<dyn ShortcutBackend>;
    /// Whether handy-keys' manager runs (the app keeps its state).
    fn handy_keys_running(&self) -> bool;
    /// Starts handy-keys' manager; `register` registers with it before the
    /// app keeps it. Fails without Accessibility, for one.
    fn start_handy_keys(
        &self,
        register: &mut dyn FnMut(&dyn ShortcutBackend),
    ) -> Result<(), String>;
    /// Re-plans the Carbon shadows of the Secure Input fallback
    /// (`secure_input::reconcile_fallback`).
    fn reconcile_fallback(&self);
    /// Records a registration in the table of the shortcuts that follow the
    /// profile (`profile_sync`).
    fn note_registered(&self, binding: &ShortcutBinding);
    /// Empties that table; returns what was in it.
    fn take_registered(&self) -> Vec<ShortcutBinding>;
    /// Registers what the profile that applies now wants
    /// (`profile_sync::sync_now`).
    fn follow_profile(&self);
    /// Tells the frontend the backend changed (`settings-changed`).
    fn announce(&self, implementation: &str, reset_bindings: &[String]);
}

/// The app as the [`ShortcutHost`].
pub(crate) struct AppHost<'a>(pub &'a AppHandle);

impl ShortcutHost for AppHost<'_> {
    fn settings(&self) -> AppSettings {
        settings::get_settings(self.0)
    }

    fn load_settings(&self) -> AppSettings {
        settings::load_or_create_app_settings(self.0)
    }

    fn write_settings(&self, settings: AppSettings) {
        settings::write_settings(self.0, settings);
    }

    fn backend(&self, implementation: KeyboardImplementation) -> Box<dyn ShortcutBackend> {
        backend::for_implementation(self.0, implementation)
    }

    fn handy_keys_running(&self) -> bool {
        self.0.try_state::<handy_keys::HandyKeysState>().is_some()
    }

    fn start_handy_keys(
        &self,
        register: &mut dyn FnMut(&dyn ShortcutBackend),
    ) -> Result<(), String> {
        handy_keys::start(self.0, register)
    }

    fn reconcile_fallback(&self) {
        crate::secure_input::reconcile_fallback(self.0);
    }

    fn note_registered(&self, binding: &ShortcutBinding) {
        profile_sync::note_registered(binding);
    }

    fn take_registered(&self) -> Vec<ShortcutBinding> {
        profile_sync::take_all()
    }

    fn follow_profile(&self) {
        profile_sync::sync_now(self.0);
    }

    fn announce(&self, implementation: &str, reset_bindings: &[String]) {
        let _ = self.0.emit(
            "settings-changed",
            serde_json::json!({
                "setting": "keyboard_implementation",
                "value": implementation,
                "reset_bindings": reset_bindings
            }),
        );
    }
}

/// Starts the stored backend and registers its start shortcuts. When
/// handy-keys can't start, falls back to Tauri and stores that, so the next
/// launch doesn't try handy-keys again.
pub(crate) fn start(host: &dyn ShortcutHost) {
    // Check which implementation to use
    match host.load_settings().keyboard_implementation {
        KeyboardImplementation::Tauri => start_tauri(host),
        KeyboardImplementation::HandyKeys => {
            if let Err(e) = start_handy_keys(host) {
                error!("Failed to initialize handy-keys shortcuts: {}", e);
                // Fall back to Tauri implementation and persist this fallback
                warn!("Falling back to Tauri global shortcut implementation and saving fallback to settings");

                // Update settings to persist the fallback so we don't retry HandyKeys on next launch
                let mut settings = host.settings();
                settings.keyboard_implementation = KeyboardImplementation::Tauri;
                host.write_settings(settings);

                start_tauri(host);
            }
        }
        KeyboardImplementation::None => {
            info!("Built-in keyboard shortcuts disabled; use CLI flags or signals");
        }
    }
}

/// Registers the start shortcuts with Tauri's global-shortcut plugin.
fn start_tauri(host: &dyn ShortcutHost) {
    let user_settings = host.load_settings();
    // Register all default shortcuts, applying user customizations
    let bindings =
        backend::start_bindings(&user_settings, crate::delivery::INSERT_SERVICE_AVAILABLE);
    let tauri = host.backend(KeyboardImplementation::Tauri);
    let report = backend::register_all(tauri.as_ref(), &bindings, |binding, e| {
        error!(
            "Failed to register shortcut {} during init: {}",
            binding.id, e
        )
    });
    // DanyWhere: known to the shortcuts that follow the profile.
    for binding in &report.registered {
        host.note_registered(binding);
    }
}

/// Starts handy-keys' manager and registers the start shortcuts with it.
fn start_handy_keys(host: &dyn ShortcutHost) -> Result<(), String> {
    host.start_handy_keys(&mut |manager| {
        let user_settings = host.load_settings();
        // Register all bindings except cancel (which is dynamic)
        let bindings =
            backend::start_bindings(&user_settings, crate::delivery::INSERT_SERVICE_AVAILABLE);
        let report = backend::register_all(manager, &bindings, |binding, e| {
            error!(
                "Failed to register handy-keys shortcut {} during init: {}",
                binding.id, e
            )
        });
        // DanyWhere: known to the shortcuts that follow the profile.
        for binding in &report.registered {
            host.note_registered(binding);
        }
    })
}

/// Switches to the backend named `implementation` at runtime: unregisters
/// everything from the old one, stores the new one, and registers the start
/// shortcuts with it, each one it can't register reset to its default.
/// When handy-keys can't start, goes back to Tauri and fails.
pub(crate) fn switch(
    host: &dyn ShortcutHost,
    implementation: &str,
) -> Result<ImplementationChangeResult, String> {
    let current_impl = host.settings().keyboard_implementation;
    let new_impl = super::parse_keyboard_implementation(implementation);

    // If same implementation, nothing to do
    if current_impl == new_impl {
        return Ok(ImplementationChangeResult {
            success: true,
            reset_bindings: vec![],
        });
    }

    info!(
        "Switching keyboard implementation from {:?} to {:?}",
        current_impl, new_impl
    );

    // Unregister all shortcuts from the current implementation. DanyWhere:
    // what is registered is known (`profile_sync`); the cancel shortcut is
    // not among it, as it is registered dynamically.
    let old = host.backend(current_impl);
    backend::unregister_all(old.as_ref(), &host.take_registered(), |binding, e| {
        warn!(
            "Failed to unregister shortcut '{}' during switch: {}",
            binding.id, e
        )
    });

    // Update the setting
    let mut settings = host.settings();
    settings.keyboard_implementation = new_impl;
    host.write_settings(settings);

    // Carbon fallback registrations use the Tauri plugin. Remove them before
    // registering the full Tauri implementation to avoid duplicate conflicts.
    if new_impl == KeyboardImplementation::Tauri {
        host.reconcile_fallback();
    }

    // Initialize new implementation if needed (HandyKeys needs state)
    if new_impl == KeyboardImplementation::HandyKeys && start_handy_keys_with_rollback(host)? {
        // Shortcuts already registered during init.
        host.follow_profile();
        host.reconcile_fallback();
        return Ok(ImplementationChangeResult {
            success: true,
            reset_bindings: vec![],
        });
    }

    // Register all shortcuts with new implementation, resetting invalid ones
    let reset_bindings = register_start_bindings_with(host, new_impl);
    // DanyWhere: then those of the profile that applies now.
    host.follow_profile();
    host.reconcile_fallback();

    // Emit event to notify frontend of the change
    host.announce(implementation, &reset_bindings);

    info!("Keyboard implementation switched to {:?}", new_impl);

    Ok(ImplementationChangeResult {
        success: true,
        reset_bindings,
    })
}

/// Starts handy-keys unless it runs already, with rollback to Tauri on
/// failure. `Ok(true)`: started, and the start shortcuts registered with it.
fn start_handy_keys_with_rollback(host: &dyn ShortcutHost) -> Result<bool, String> {
    if host.handy_keys_running() {
        return Ok(false); // Already initialized, caller should continue
    }

    if let Err(e) = start_handy_keys(host) {
        error!("Failed to initialize HandyKeys: {}", e);
        // Rollback to Tauri
        let mut settings = host.settings();
        settings.keyboard_implementation = KeyboardImplementation::Tauri;
        host.write_settings(settings);
        host.reconcile_fallback();
        start_tauri(host);
        return Err(format!(
            "Failed to initialize HandyKeys: {}. Reverted to Tauri.",
            e
        ));
    }

    // The start already registered shortcuts
    Ok(true)
}

/// Registers the start shortcuts with the backend `implementation` selects,
/// each one it can't register reset to its default first (and stored).
/// Returns the ids reset.
fn register_start_bindings_with(
    host: &dyn ShortcutHost,
    implementation: KeyboardImplementation,
) -> Vec<String> {
    let mut current_settings = host.settings();
    let target = host.backend(implementation);
    // Skip cancel shortcut as it's dynamically registered, and those
    // `registers_binding` leaves out.
    let mut bindings =
        backend::start_bindings(&current_settings, crate::delivery::INSERT_SERVICE_AVAILABLE);
    // Validate the shortcuts for the target implementation
    let reset_bindings =
        backend::reset_invalid(target.as_ref(), &mut bindings, &mut current_settings);
    // Nothing counts as registered with "None".
    let report = backend::register_all(target.as_ref(), &bindings, |binding, e| {
        error!(
            "Failed to register shortcut '{}' for {:?}: {}",
            binding.id, implementation, e
        )
    });
    for binding in &report.registered {
        host.note_registered(binding);
    }

    // Save settings if any bindings were reset
    if !reset_bindings.is_empty() {
        host.write_settings(current_settings);
    }

    reset_bindings
}

/// One pass of the cancel shortcut's reconciliation
/// (`reconcile_cancel_shortcut`): registers `cancel` while `requested` and
/// not `registered`, unregisters it in the opposite case; `registered`
/// follows only what the backend did, so a failed pass is tried again.
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) fn reconcile_cancel(
    backend: &dyn ShortcutBackend,
    cancel: &ShortcutBinding,
    requested: bool,
    registered: &mut bool,
) {
    if requested == *registered {
        return;
    }
    if requested {
        match backend.register(cancel) {
            Ok(()) => *registered = true,
            Err(e) => error!("Failed to register cancel shortcut: {}", e),
        }
    } else {
        match backend.unregister(cancel) {
            Ok(()) => *registered = false,
            Err(e) => error!("Failed to unregister cancel shortcut: {}", e),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::settings::get_default_settings;
    use crate::shortcut::backend::fake::{binding, name, CallLog, RecordingBackend};
    use crate::shortcut::backend::{start_bindings, NoShortcuts};
    use crate::shortcut::profile_sync::Registered;

    const AVAILABLE: bool = crate::delivery::INSERT_SERVICE_AVAILABLE;

    /// The app, recorded: every effect is a line in `log`, as are the
    /// backends' calls.
    struct FakeHost {
        log: CallLog,
        settings: Mutex<AppSettings>,
        registered: Mutex<Registered>,
        handy_keys_running: Mutex<bool>,
        handy_keys_fails: Option<String>,
        refuse: Vec<(KeyboardImplementation, String)>,
    }

    impl FakeHost {
        fn new(implementation: KeyboardImplementation) -> Self {
            let mut settings = get_default_settings();
            settings.keyboard_implementation = implementation;
            Self {
                log: CallLog::default(),
                settings: Mutex::new(settings),
                registered: Mutex::new(Registered::new()),
                handy_keys_running: Mutex::new(false),
                handy_keys_fails: None,
                refuse: Vec::new(),
            }
        }

        fn with_shortcut(self, id: &str, keys: &str) -> Self {
            self.settings
                .lock()
                .unwrap()
                .bindings
                .get_mut(id)
                .expect("a default binding")
                .current_binding = keys.to_string();
            self
        }

        /// What the old backend has registered.
        fn having_registered(self, bindings: &[ShortcutBinding]) -> Self {
            for b in bindings {
                self.registered
                    .lock()
                    .unwrap()
                    .insert(b.id.clone(), b.clone());
            }
            self
        }

        fn lines(&self) -> Vec<String> {
            self.log.lines()
        }

        fn stored(&self) -> AppSettings {
            self.settings.lock().unwrap().clone()
        }

        fn registered_ids(&self) -> Vec<String> {
            self.registered.lock().unwrap().keys().cloned().collect()
        }

        /// The ids of the start shortcuts the stored settings have.
        fn start_ids(&self) -> Vec<String> {
            let mut ids: Vec<String> = start_bindings(&self.stored(), AVAILABLE)
                .into_iter()
                .map(|b| b.id)
                .collect();
            ids.sort();
            ids
        }
    }

    impl ShortcutHost for FakeHost {
        fn settings(&self) -> AppSettings {
            self.stored()
        }

        fn load_settings(&self) -> AppSettings {
            self.stored()
        }

        fn write_settings(&self, settings: AppSettings) {
            self.log
                .push(format!("write {}", name(settings.keyboard_implementation)));
            *self.settings.lock().unwrap() = settings;
        }

        fn backend(&self, implementation: KeyboardImplementation) -> Box<dyn ShortcutBackend> {
            let mut backend = RecordingBackend::new(implementation, &self.log);
            for (refusing, id) in &self.refuse {
                if *refusing == implementation {
                    backend = backend.refusing_register(id);
                }
            }
            Box::new(backend)
        }

        fn handy_keys_running(&self) -> bool {
            *self.handy_keys_running.lock().unwrap()
        }

        fn start_handy_keys(
            &self,
            register: &mut dyn FnMut(&dyn ShortcutBackend),
        ) -> Result<(), String> {
            if let Some(err) = &self.handy_keys_fails {
                self.log.push("handy_keys: start failed");
                return Err(err.clone());
            }
            self.log.push("handy_keys: start");
            let manager = self.backend(KeyboardImplementation::HandyKeys);
            register(manager.as_ref());
            *self.handy_keys_running.lock().unwrap() = true;
            self.log.push("handy_keys: kept");
            Ok(())
        }

        fn reconcile_fallback(&self) {
            self.log.push("reconcile fallback");
        }

        fn note_registered(&self, binding: &ShortcutBinding) {
            self.log.push(format!("note {}", binding.id));
            self.registered
                .lock()
                .unwrap()
                .insert(binding.id.clone(), binding.clone());
        }

        fn take_registered(&self) -> Vec<ShortcutBinding> {
            self.log.push("take registered");
            std::mem::take(&mut *self.registered.lock().unwrap())
                .into_values()
                .collect()
        }

        fn follow_profile(&self) {
            self.log.push("follow profile");
        }

        fn announce(&self, implementation: &str, reset_bindings: &[String]) {
            self.log
                .push(format!("announce {implementation} {reset_bindings:?}"));
        }
    }

    /// Where `line` is in `lines`; fails the test when it isn't.
    fn at(lines: &[String], line: &str) -> usize {
        lines
            .iter()
            .position(|l| l == line)
            .unwrap_or_else(|| panic!("no line {line:?} in {lines:#?}"))
    }

    fn last_at(lines: &[String], line: &str) -> usize {
        lines
            .iter()
            .rposition(|l| l == line)
            .unwrap_or_else(|| panic!("no line {line:?} in {lines:#?}"))
    }

    fn starting_with<'a>(lines: &'a [String], prefix: &str) -> Vec<&'a String> {
        lines.iter().filter(|l| l.starts_with(prefix)).collect()
    }

    fn default_keys(id: &str) -> String {
        get_default_settings().bindings[id].current_binding.clone()
    }

    // ---- the start ----

    #[test]
    fn the_stored_backend_registers_its_start_shortcuts_and_notes_them() {
        let host = FakeHost::new(KeyboardImplementation::Tauri);
        start(&host);
        let lines = host.lines();
        assert_eq!(
            starting_with(&lines, "tauri: register").len(),
            host.start_ids().len(),
            "{lines:#?}"
        );
        assert!(!lines.iter().any(|l| l.contains("cancel")), "{lines:#?}");
        assert_eq!(host.registered_ids(), host.start_ids());
        assert!(starting_with(&lines, "write").is_empty(), "nothing stored");
    }

    #[test]
    fn handy_keys_registers_its_start_shortcuts_before_the_app_keeps_it() {
        let host = FakeHost::new(KeyboardImplementation::HandyKeys);
        start(&host);
        let lines = host.lines();
        let first = at(&lines, "handy_keys: start");
        let kept = at(&lines, "handy_keys: kept");
        let registers: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.starts_with("handy_keys: register"))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(registers.len(), host.start_ids().len(), "{lines:#?}");
        assert!(
            registers.iter().all(|i| first < *i && *i < kept),
            "{lines:#?}"
        );
        assert_eq!(host.registered_ids(), host.start_ids());
    }

    #[test]
    fn when_handy_keys_cannot_start_the_app_falls_back_to_tauri_for_good() {
        let mut host = FakeHost::new(KeyboardImplementation::HandyKeys);
        host.handy_keys_fails = Some("no event tap".to_string());
        start(&host);
        let lines = host.lines();
        let failed = at(&lines, "handy_keys: start failed");
        let stored = at(&lines, "write tauri");
        assert!(failed < stored, "{lines:#?}");
        assert_eq!(
            host.stored().keyboard_implementation,
            KeyboardImplementation::Tauri
        );
        let registers = starting_with(&lines, "tauri: register");
        assert_eq!(registers.len(), host.start_ids().len(), "{lines:#?}");
        assert!(at(&lines, registers[0]) > stored);
        assert_eq!(host.registered_ids(), host.start_ids());
    }

    #[test]
    fn with_none_nothing_is_registered_at_the_start() {
        let host = FakeHost::new(KeyboardImplementation::None);
        start(&host);
        assert!(host.lines().is_empty(), "{:#?}", host.lines());
        assert!(host.registered_ids().is_empty());
    }

    // ---- switching ----

    #[test]
    fn switching_to_the_backend_in_use_changes_nothing() {
        let host = FakeHost::new(KeyboardImplementation::Tauri);
        let result = switch(&host, "tauri").expect("switched");
        assert!(result.success);
        assert!(result.reset_bindings.is_empty());
        assert!(host.lines().is_empty(), "{:#?}", host.lines());
    }

    #[test]
    fn switching_to_tauri_moves_the_shortcuts_and_resets_what_tauri_cannot_register() {
        let host = FakeHost::new(KeyboardImplementation::HandyKeys)
            .with_shortcut("transcribe", "fn+space")
            .having_registered(&[
                binding("transcribe", "fn+space"),
                binding("cycle_word_list", "ctrl+shift+l"),
            ]);
        let result = switch(&host, "tauri").expect("switched");
        let lines = host.lines();

        // The old backend gives up what it had registered.
        let unregistered = at(&lines, "handy_keys: unregister transcribe fn+space");
        at(
            &lines,
            "handy_keys: unregister cycle_word_list ctrl+shift+l",
        );
        // Then the new one is stored, the Carbon shadows go, and it registers.
        let stored = at(&lines, "write tauri");
        let fallback = at(&lines, "reconcile fallback");
        let transcribe = at(
            &lines,
            &format!("tauri: register transcribe {}", default_keys("transcribe")),
        );
        assert!(unregistered < stored && stored < fallback && fallback < transcribe);
        assert_eq!(
            starting_with(&lines, "tauri: register").len(),
            host.start_ids().len()
        );
        // The reset is stored after registering.
        let last_register = lines
            .iter()
            .rposition(|l| l.starts_with("tauri: register"))
            .unwrap();
        assert!(last_at(&lines, "write tauri") > last_register, "{lines:#?}");
        assert_eq!(
            host.stored().bindings["transcribe"].current_binding,
            default_keys("transcribe")
        );
        assert_eq!(result.reset_bindings, ["transcribe"]);
        assert!(result.success);
        assert_eq!(host.registered_ids(), host.start_ids());
        // Then the profile's shortcuts, the fallback again, and the frontend.
        let n = lines.len();
        assert_eq!(
            lines[n - 3..],
            [
                "follow profile".to_string(),
                "reconcile fallback".to_string(),
                "announce tauri [\"transcribe\"]".to_string()
            ]
        );
    }

    #[test]
    fn a_shortcut_the_new_backend_refuses_is_not_noted_and_the_switch_succeeds() {
        let mut host = FakeHost::new(KeyboardImplementation::HandyKeys);
        host.refuse
            .push((KeyboardImplementation::Tauri, "cycle_word_list".to_string()));
        let result = switch(&host, "tauri").expect("switched");
        assert!(result.success);
        assert!(host
            .lines()
            .iter()
            .any(|l| l.starts_with("tauri: register cycle_word_list")));
        assert!(!host
            .registered_ids()
            .contains(&"cycle_word_list".to_string()));
        assert!(host.registered_ids().contains(&"transcribe".to_string()));
    }

    #[test]
    fn switching_to_handy_keys_starts_it_and_it_registers_the_start_shortcuts() {
        let host = FakeHost::new(KeyboardImplementation::Tauri)
            .having_registered(&[binding("transcribe", "ctrl+space")]);
        let result = switch(&host, "handy_keys").expect("switched");
        let lines = host.lines();
        let stored = at(&lines, "write handy_keys");
        let started = at(&lines, "handy_keys: start");
        assert!(at(&lines, "tauri: unregister transcribe ctrl+space") < stored);
        assert!(stored < started);
        // No fallback pass before the start: that one is for Tauri.
        assert!(!lines[..started].contains(&"reconcile fallback".to_string()));
        assert_eq!(
            starting_with(&lines, "handy_keys: register").len(),
            host.start_ids().len()
        );
        assert_eq!(host.registered_ids(), host.start_ids());
        // Registered during the start: the profile's, the fallback, no event.
        let n = lines.len();
        assert_eq!(
            lines[n - 2..],
            [
                "follow profile".to_string(),
                "reconcile fallback".to_string()
            ]
        );
        assert!(starting_with(&lines, "announce").is_empty(), "{lines:#?}");
        assert!(result.success);
        assert!(result.reset_bindings.is_empty());
    }

    #[test]
    fn when_handy_keys_cannot_start_the_switch_goes_back_to_tauri_and_fails() {
        let mut host = FakeHost::new(KeyboardImplementation::Tauri)
            .having_registered(&[binding("transcribe", "ctrl+space")]);
        host.handy_keys_fails = Some("no event tap".to_string());
        let result = switch(&host, "handy_keys");
        assert_eq!(
            result.err().as_deref(),
            Some("Failed to initialize HandyKeys: no event tap. Reverted to Tauri.")
        );
        let lines = host.lines();
        let failed = at(&lines, "handy_keys: start failed");
        let back = at(&lines, "write tauri");
        let fallback = at(&lines, "reconcile fallback");
        assert!(at(&lines, "write handy_keys") < failed);
        assert!(failed < back && back < fallback);
        let registers = starting_with(&lines, "tauri: register");
        assert_eq!(registers.len(), host.start_ids().len(), "{lines:#?}");
        assert!(at(&lines, registers[0]) > fallback);
        assert_eq!(
            host.stored().keyboard_implementation,
            KeyboardImplementation::Tauri
        );
        assert_eq!(host.registered_ids(), host.start_ids());
        assert!(!lines.contains(&"follow profile".to_string()), "{lines:#?}");
        assert!(starting_with(&lines, "announce").is_empty(), "{lines:#?}");
    }

    #[test]
    fn switching_to_handy_keys_while_it_runs_registers_like_any_backend() {
        let host = FakeHost::new(KeyboardImplementation::Tauri);
        *host.handy_keys_running.lock().unwrap() = true;
        let result = switch(&host, "handy_keys").expect("switched");
        let lines = host.lines();
        assert!(
            !lines.contains(&"handy_keys: start".to_string()),
            "{lines:#?}"
        );
        assert_eq!(
            starting_with(&lines, "handy_keys: register").len(),
            host.start_ids().len()
        );
        assert_eq!(
            lines.last().map(String::as_str),
            Some("announce handy_keys []")
        );
        assert!(result.reset_bindings.is_empty());
    }

    #[test]
    fn switching_to_none_unregisters_everything_and_registers_nothing() {
        let host = FakeHost::new(KeyboardImplementation::Tauri).having_registered(&[
            binding("transcribe", "ctrl+space"),
            binding("cycle_word_list", "ctrl+shift+l"),
        ]);
        let result = switch(&host, "none").expect("switched");
        let lines = host.lines();
        at(&lines, "tauri: unregister transcribe ctrl+space");
        at(&lines, "tauri: unregister cycle_word_list ctrl+shift+l");
        at(&lines, "write none");
        assert!(starting_with(&lines, "none:").is_empty(), "{lines:#?}");
        assert!(starting_with(&lines, "note").is_empty(), "{lines:#?}");
        assert!(host.registered_ids().is_empty());
        assert_eq!(
            host.stored().keyboard_implementation,
            KeyboardImplementation::None
        );
        assert_eq!(lines.last().map(String::as_str), Some("announce none []"));
        assert!(result.success);
        assert!(result.reset_bindings.is_empty());
    }

    #[test]
    fn an_unknown_name_switches_to_tauri_and_is_announced_as_given() {
        let host = FakeHost::new(KeyboardImplementation::HandyKeys);
        switch(&host, "qwerty").expect("switched");
        assert_eq!(
            host.stored().keyboard_implementation,
            KeyboardImplementation::Tauri
        );
        assert_eq!(
            host.lines().last().map(String::as_str),
            Some("announce qwerty []")
        );
    }

    // ---- the cancel shortcut ----

    fn cancel() -> ShortcutBinding {
        binding("cancel", "escape")
    }

    #[test]
    fn the_cancel_shortcut_is_registered_when_a_recording_wants_it() {
        let log = CallLog::default();
        let backend = RecordingBackend::new(KeyboardImplementation::HandyKeys, &log);
        let mut registered = false;
        reconcile_cancel(&backend, &cancel(), true, &mut registered);
        assert!(registered);
        assert_eq!(log.lines(), ["handy_keys: register cancel escape"]);
    }

    #[test]
    fn a_refused_cancel_registration_is_tried_again_on_the_next_pass() {
        let log = CallLog::default();
        let refusing =
            RecordingBackend::new(KeyboardImplementation::Tauri, &log).refusing_register("cancel");
        let mut registered = false;
        reconcile_cancel(&refusing, &cancel(), true, &mut registered);
        assert!(!registered);
        let taking = RecordingBackend::new(KeyboardImplementation::Tauri, &log);
        reconcile_cancel(&taking, &cancel(), true, &mut registered);
        assert!(registered);
        assert_eq!(
            log.lines(),
            [
                "tauri: register cancel escape",
                "tauri: register cancel escape"
            ]
        );
    }

    #[test]
    fn the_cancel_shortcut_is_unregistered_when_no_longer_wanted_and_kept_if_that_fails() {
        let log = CallLog::default();
        let refusing = RecordingBackend::new(KeyboardImplementation::Tauri, &log)
            .refusing_unregister("cancel");
        let mut registered = true;
        reconcile_cancel(&refusing, &cancel(), false, &mut registered);
        assert!(registered, "still registered after a refusal");
        let taking = RecordingBackend::new(KeyboardImplementation::Tauri, &log);
        reconcile_cancel(&taking, &cancel(), false, &mut registered);
        assert!(!registered);
        assert_eq!(
            log.lines(),
            [
                "tauri: unregister cancel escape",
                "tauri: unregister cancel escape"
            ]
        );
    }

    #[test]
    fn nothing_happens_while_the_cancel_shortcut_is_as_wanted() {
        let log = CallLog::default();
        let backend = RecordingBackend::new(KeyboardImplementation::Tauri, &log);
        for state in [false, true] {
            let mut registered = state;
            reconcile_cancel(&backend, &cancel(), state, &mut registered);
            assert_eq!(registered, state);
        }
        assert!(log.lines().is_empty());
    }

    #[test]
    fn with_no_backend_the_cancel_shortcut_counts_as_registered() {
        let mut registered = false;
        reconcile_cancel(&NoShortcuts, &cancel(), true, &mut registered);
        assert!(registered);
        reconcile_cancel(&NoShortcuts, &cancel(), false, &mut registered);
        assert!(!registered);
    }
}
