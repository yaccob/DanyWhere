//! DanyWhere: the shortcut backends behind one trait (test redesign, strand
//! C1, 2026-11-22).
//!
//! handy-keys (`handy_keys`), Tauri's global-shortcut plugin (`tauri_impl`)
//! and "None" (no built-in shortcuts) each implement [`ShortcutBackend`];
//! [`for_implementation`] picks one by `KeyboardImplementation`, the only
//! place that does. Registering several shortcuts goes through
//! [`register_all`] and unregistering them through [`unregister_all`], so
//! the loops and the logic around them run in tests with a recording fake
//! ([`fake::RecordingBackend`]) instead of the app.

use tauri::AppHandle;

use crate::settings::{self, AppSettings, KeyboardImplementation, ShortcutBinding};

/// A way of registering global shortcuts. Each backend holds what it needs
/// to reach the system (the `AppHandle`), so callers need nothing else.
pub(crate) trait ShortcutBackend: Send + Sync {
    /// Which setting selects this backend.
    fn implementation(&self) -> KeyboardImplementation;
    /// Registers `binding.current_binding` for `binding.id`.
    fn register(&self, binding: &ShortcutBinding) -> Result<(), String>;
    /// Unregisters `binding`.
    fn unregister(&self, binding: &ShortcutBinding) -> Result<(), String>;
    /// Whether this backend can register the key combination `raw`; the
    /// backend's own rule (`validate_shortcut_for_implementation`).
    fn validate(&self, raw: &str) -> Result<(), String> {
        super::validate_shortcut_for_implementation(raw, self.implementation())
    }
}

/// "None": no built-in shortcuts; DanyWhere is triggered by CLI flags or
/// signals. It registers nothing and accepts every key combination.
pub(crate) struct NoShortcuts;

impl ShortcutBackend for NoShortcuts {
    fn implementation(&self) -> KeyboardImplementation {
        KeyboardImplementation::None
    }

    fn register(&self, _binding: &ShortcutBinding) -> Result<(), String> {
        Ok(())
    }

    fn unregister(&self, _binding: &ShortcutBinding) -> Result<(), String> {
        Ok(())
    }
}

/// The backend `implementation` selects.
pub(crate) fn for_implementation(
    app: &AppHandle,
    implementation: KeyboardImplementation,
) -> Box<dyn ShortcutBackend> {
    match implementation {
        KeyboardImplementation::Tauri => Box::new(super::tauri_impl::TauriBackend::new(app)),
        KeyboardImplementation::HandyKeys => {
            Box::new(super::handy_keys::HandyKeysBackend::new(app))
        }
        KeyboardImplementation::None => Box::new(NoShortcuts),
    }
}

/// The backend the stored settings select now.
pub(crate) fn active(app: &AppHandle) -> Box<dyn ShortcutBackend> {
    for_implementation(app, settings::get_settings(app).keyboard_implementation)
}

/// What [`register_all`] did: the bindings the backend took, and those it
/// refused, with its error.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct RegisterReport {
    pub registered: Vec<ShortcutBinding>,
    pub failed: Vec<(ShortcutBinding, String)>,
}

/// The one registration loop: registers each of `bindings` with `backend`,
/// in order, and goes on after a refusal; `on_failure` logs a refusal right
/// when it happens (each caller has its own line). The "None" backend
/// registers nothing, so nothing counts as registered with it.
pub(crate) fn register_all(
    backend: &dyn ShortcutBackend,
    bindings: &[ShortcutBinding],
    mut on_failure: impl FnMut(&ShortcutBinding, &str),
) -> RegisterReport {
    let mut report = RegisterReport::default();
    if backend.implementation() == KeyboardImplementation::None {
        return report;
    }
    for binding in bindings {
        match backend.register(binding) {
            Ok(()) => report.registered.push(binding.clone()),
            Err(err) => {
                on_failure(binding, &err);
                report.failed.push((binding.clone(), err));
            }
        }
    }
    report
}

/// Unregisters each of `bindings` with `backend`, in order, and goes on
/// after a refusal; `on_failure` logs it.
pub(crate) fn unregister_all(
    backend: &dyn ShortcutBackend,
    bindings: &[ShortcutBinding],
    mut on_failure: impl FnMut(&ShortcutBinding, &str),
) {
    for binding in bindings {
        if let Err(err) = backend.unregister(binding) {
            on_failure(binding, &err);
        }
    }
}

/// The shortcuts a backend registers when it starts or is switched to, by
/// id: every default binding but the cancel shortcut (registered only while
/// recording, `register_cancel_shortcut`) and those `registers_binding`
/// leaves out, with the value stored in `settings`, else the default; then
/// the start and hold shortcuts of `settings`, each combination once
/// (`start_keys::bindings_to_register`; a hold shortcut that post-processes
/// only while post-processing is reachable). `service_available` is
/// `delivery::INSERT_SERVICE_AVAILABLE`.
pub(crate) fn start_bindings(
    settings: &AppSettings,
    service_available: bool,
) -> Vec<ShortcutBinding> {
    let mut defaults: Vec<(String, ShortcutBinding)> = settings::get_default_settings()
        .bindings
        .into_iter()
        .collect();
    defaults.sort_by(|(a, _), (b, _)| a.cmp(b));
    let mut bindings: Vec<ShortcutBinding> = defaults
        .into_iter()
        .filter(|(id, _)| {
            id != "cancel" && settings::registers_binding(id, settings, service_available)
        })
        .map(|(id, default)| settings.bindings.get(&id).cloned().unwrap_or(default))
        .collect();
    bindings.extend(crate::start_keys::bindings_to_register(
        &settings.start_keys,
        &settings.hold_keys,
        crate::app_profiles::post_processing_reachable(settings),
    ));
    bindings
}

/// Resets each of `bindings` that `backend` can't register to its default
/// key combination, in `bindings` and in `settings`; a start or hold
/// shortcut to the factory one with its id, where there is one the backend
/// takes (`start_keys::reset_to_factory`). Returns the ids reset.
pub(crate) fn reset_invalid(
    backend: &dyn ShortcutBackend,
    bindings: &mut [ShortcutBinding],
    settings: &mut AppSettings,
) -> Vec<String> {
    let defaults = settings::get_default_settings().bindings;
    let mut reset = Vec::new();
    for binding in bindings.iter_mut() {
        if crate::start_keys::is_dictation_binding(&binding.id) {
            if backend.validate(&binding.current_binding).is_err() {
                if let Some(keys) =
                    crate::start_keys::reset_to_factory(settings, &binding.id, |keys| {
                        backend.validate(keys).is_ok()
                    })
                {
                    log::info!(
                        "Shortcut '{}' ({}) is invalid for {:?}; back to {keys}.",
                        binding.id,
                        binding.current_binding,
                        backend.implementation()
                    );
                    binding.current_binding = keys.clone();
                    binding.default_binding = keys;
                    reset.push(binding.id.clone());
                }
            }
            continue;
        }
        let Some(default) = defaults.get(&binding.id) else {
            continue;
        };
        if let Err(e) = backend.validate(&binding.current_binding) {
            log::info!(
                "Shortcut '{}' ({}) is invalid for {:?}: {}. Resetting to default.",
                binding.id,
                binding.current_binding,
                backend.implementation(),
                e
            );
            binding.current_binding = default.current_binding.clone();
            settings
                .bindings
                .insert(binding.id.clone(), binding.clone());
            reset.push(binding.id.clone());
        }
    }
    reset
}

/// A backend for tests that records every call in a shared log.
#[cfg(test)]
pub(crate) mod fake {
    use std::sync::{Arc, Mutex};

    use super::ShortcutBackend;
    use crate::settings::{KeyboardImplementation, ShortcutBinding};

    /// The calls, as lines like `tauri: register transcribe ctrl+space`.
    #[derive(Clone, Default)]
    pub(crate) struct CallLog(Arc<Mutex<Vec<String>>>);

    impl CallLog {
        pub fn push(&self, line: impl Into<String>) {
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(line.into());
        }

        pub fn lines(&self) -> Vec<String> {
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }
    }

    pub(crate) fn name(implementation: KeyboardImplementation) -> &'static str {
        match implementation {
            KeyboardImplementation::Tauri => "tauri",
            KeyboardImplementation::HandyKeys => "handy_keys",
            KeyboardImplementation::None => "none",
        }
    }

    /// Takes every binding unless told to refuse it, and checks key
    /// combinations by the rule of the backend it stands for unless told to
    /// reject one.
    #[derive(Clone)]
    pub(crate) struct RecordingBackend {
        pub implementation: KeyboardImplementation,
        pub log: CallLog,
        pub refuse_register: Vec<String>,
        pub refuse_unregister: Vec<String>,
        pub reject: Vec<String>,
    }

    impl RecordingBackend {
        pub fn new(implementation: KeyboardImplementation, log: &CallLog) -> Self {
            Self {
                implementation,
                log: log.clone(),
                refuse_register: Vec::new(),
                refuse_unregister: Vec::new(),
                reject: Vec::new(),
            }
        }

        pub fn refusing_register(mut self, id: &str) -> Self {
            self.refuse_register.push(id.to_string());
            self
        }

        pub fn refusing_unregister(mut self, id: &str) -> Self {
            self.refuse_unregister.push(id.to_string());
            self
        }
    }

    impl ShortcutBackend for RecordingBackend {
        fn implementation(&self) -> KeyboardImplementation {
            self.implementation
        }

        fn register(&self, binding: &ShortcutBinding) -> Result<(), String> {
            self.log.push(format!(
                "{}: register {} {}",
                name(self.implementation),
                binding.id,
                binding.current_binding
            ));
            if self.refuse_register.contains(&binding.id) {
                return Err(format!("{} refused", binding.id));
            }
            Ok(())
        }

        fn unregister(&self, binding: &ShortcutBinding) -> Result<(), String> {
            self.log.push(format!(
                "{}: unregister {} {}",
                name(self.implementation),
                binding.id,
                binding.current_binding
            ));
            if self.refuse_unregister.contains(&binding.id) {
                return Err(format!("{} refused", binding.id));
            }
            Ok(())
        }

        fn validate(&self, raw: &str) -> Result<(), String> {
            if self.reject.iter().any(|keys| keys == raw) {
                return Err(format!("{raw} rejected"));
            }
            super::super::validate_shortcut_for_implementation(raw, self.implementation)
        }
    }

    pub(crate) fn binding(id: &str, keys: &str) -> ShortcutBinding {
        ShortcutBinding {
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
            default_binding: keys.to_string(),
            current_binding: keys.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::{binding, CallLog, RecordingBackend};
    use super::*;
    use crate::settings::get_default_settings;

    fn ids(bindings: &[ShortcutBinding]) -> Vec<&str> {
        bindings.iter().map(|b| b.id.as_str()).collect()
    }

    #[test]
    fn the_loop_registers_every_binding_in_order_and_reports_them() {
        let log = CallLog::default();
        let backend = RecordingBackend::new(KeyboardImplementation::Tauri, &log);
        let bindings = [
            binding("transcribe", "ctrl+space"),
            binding("cycle_word_list", "ctrl+shift+l"),
        ];
        let mut failures = 0;
        let report = register_all(&backend, &bindings, |_, _| failures += 1);
        assert_eq!(
            log.lines(),
            [
                "tauri: register transcribe ctrl+space",
                "tauri: register cycle_word_list ctrl+shift+l"
            ]
        );
        assert_eq!(ids(&report.registered), ["transcribe", "cycle_word_list"]);
        assert!(report.failed.is_empty());
        assert_eq!(failures, 0);
    }

    #[test]
    fn a_refused_binding_is_reported_and_logged_and_the_rest_still_registered() {
        let log = CallLog::default();
        let backend = RecordingBackend::new(KeyboardImplementation::HandyKeys, &log)
            .refusing_register("transcribe");
        let bindings = [
            binding("transcribe", "ctrl+space"),
            binding("paste_last_transcript", "ctrl+shift+v"),
        ];
        let mut logged = Vec::new();
        let report = register_all(&backend, &bindings, |b, err| {
            logged.push(format!("{}: {err}", b.id))
        });
        assert_eq!(log.lines().len(), 2, "both were tried: {:?}", log.lines());
        assert_eq!(ids(&report.registered), ["paste_last_transcript"]);
        assert_eq!(
            report.failed,
            vec![(bindings[0].clone(), "transcribe refused".to_string())]
        );
        assert_eq!(logged, ["transcribe: transcribe refused"]);
    }

    #[test]
    fn unregistering_goes_on_after_a_refusal_and_logs_it() {
        let log = CallLog::default();
        let backend = RecordingBackend::new(KeyboardImplementation::Tauri, &log)
            .refusing_unregister("transcribe");
        let bindings = [
            binding("transcribe", "ctrl+space"),
            binding("cycle_word_list", "ctrl+shift+l"),
        ];
        let mut logged = Vec::new();
        unregister_all(&backend, &bindings, |b, err| {
            logged.push(format!("{}: {err}", b.id))
        });
        assert_eq!(
            log.lines(),
            [
                "tauri: unregister transcribe ctrl+space",
                "tauri: unregister cycle_word_list ctrl+shift+l"
            ]
        );
        assert_eq!(logged, ["transcribe: transcribe refused"]);
    }

    #[test]
    fn with_no_backend_nothing_counts_as_registered() {
        let bindings = [binding("transcribe", "ctrl+space")];
        let mut failures = 0;
        let report = register_all(&NoShortcuts, &bindings, |_, _| failures += 1);
        assert_eq!(report, RegisterReport::default());
        assert_eq!(failures, 0);
        assert_eq!(NoShortcuts.validate("fn"), Ok(()));
    }

    #[test]
    fn the_start_bindings_leave_out_cancel_and_take_the_stored_value() {
        let mut settings = get_default_settings();
        settings
            .bindings
            .get_mut("copy_last_transcript")
            .unwrap()
            .current_binding = "f13".to_string();
        settings.start_keys[0].shortcut = "f14".to_string();
        settings.bindings.remove("cycle_word_list");
        let start = start_bindings(&settings, true);
        let found = |id: &str| start.iter().find(|b| b.id == id).cloned();
        assert!(found("cancel").is_none());
        assert_eq!(
            found("copy_last_transcript").unwrap().current_binding,
            "f13"
        );
        // DanyWhere: the start shortcuts too (`start_keys`).
        assert_eq!(found("start_key:start").unwrap().current_binding, "f14");
        // Missing from the store: the default.
        assert_eq!(
            found("cycle_word_list").unwrap().current_binding,
            get_default_settings().bindings["cycle_word_list"].current_binding
        );
        // The same order on every start.
        assert_eq!(ids(&start), ids(&start_bindings(&settings, true)));
    }

    #[test]
    fn the_start_bindings_leave_out_what_registers_binding_leaves_out() {
        // No profile with applications post-processes either.
        let mut settings = get_default_settings();
        settings.post_process_enabled = false;
        assert!(!crate::app_profiles::post_processing_reachable(&settings));
        let start = start_bindings(&settings, true);
        for id in settings.bindings.keys() {
            let expected =
                id != "cancel" && crate::settings::registers_binding(id, &settings, true);
            assert_eq!(
                start.iter().any(|b| &b.id == id),
                expected,
                "{id} in the start bindings"
            );
        }
        // The hold shortcut that post-processes isn't taken either.
        assert!(!start.iter().any(|b| b.id == "hold_key:hold_post"));
        assert!(start.iter().any(|b| b.id == "start_key:start"));
    }

    #[test]
    fn what_the_backend_cannot_register_is_reset_to_its_default() {
        let log = CallLog::default();
        let backend = RecordingBackend::new(KeyboardImplementation::Tauri, &log);
        let mut settings = get_default_settings();
        settings
            .bindings
            .get_mut("copy_last_transcript")
            .unwrap()
            .current_binding = "fn+c".to_string();
        settings.start_keys[0].shortcut = "fn+space".to_string();
        settings
            .bindings
            .get_mut("cycle_word_list")
            .unwrap()
            .current_binding = "ctrl+shift+k".to_string();
        let start = crate::start_keys::bindings_to_register(
            &settings.start_keys,
            &settings.hold_keys,
            true,
        );
        let mut bindings = vec![
            settings.bindings["cycle_word_list"].clone(),
            settings.bindings["copy_last_transcript"].clone(),
            start[0].clone(),
        ];
        let reset = reset_invalid(&backend, &mut bindings, &mut settings);
        let defaults = get_default_settings();
        let default = defaults.bindings["copy_last_transcript"]
            .current_binding
            .clone();
        assert_eq!(reset, ["copy_last_transcript", "start_key:start"]);
        assert_eq!(bindings[1].current_binding, default);
        assert_eq!(
            settings.bindings["copy_last_transcript"].current_binding,
            default
        );
        // A start shortcut goes back to the factory one with its id.
        let factory = defaults.start_keys[0].shortcut.clone();
        assert_eq!(bindings[2].current_binding, factory);
        assert_eq!(settings.start_keys[0].shortcut, factory);
        // Valid ones stay as they are.
        assert_eq!(bindings[0].current_binding, "ctrl+shift+k");
        assert_eq!(
            settings.bindings["cycle_word_list"].current_binding,
            "ctrl+shift+k"
        );
        assert!(log.lines().is_empty(), "validating registers nothing");
    }
}
