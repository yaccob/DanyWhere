//! DanyWhere: the one place that answers whether DanyWhere has the macOS
//! Accessibility permission, and so what it may do. Every question about it
//! goes through [`state`] and the questions of [`AccessibilityState`]; no
//! other code reads `AppSettings::accessibility_skipped` or asks macOS
//! (sidecar rule of 2026-10-24: one replaceable check, both states tested).
//!
//! Two things decide it:
//!
//! - **The user's choice.** "Continue without Accessibility" in onboarding
//!   (`shortcut::apply_no_accessibility_mode`) stores
//!   `accessibility_skipped`; nothing resets it. It switches on the mode
//!   without the permission: Tauri shortcuts, the clipboard, and on macOS the
//!   "Insert Last Dictation" service.
//! - **macOS's answer** (`AXIsProcessTrusted`), asked each time it matters,
//!   never with a prompt. Other systems have nothing to grant.
//!
//! The choice wins. A Mac that grants the permission later stays in the mode
//! without it until the user sets DanyWhere up again.
//!
//! Tests replace macOS's answer with [`testing::with_system_trusted`], so
//! both states run without the real permission.

use crate::settings::AppSettings;

/// What DanyWhere may do about Accessibility, from the user's choice and
/// macOS's answer. macOS is only asked by a question that needs its answer,
/// so the decisions that follow the choice alone never ask it.
#[derive(Clone, Copy)]
pub struct AccessibilityState {
    /// The user chose to continue without Accessibility.
    skipped: bool,
    /// Asks macOS whether DanyWhere has the permission.
    ask_system: fn() -> bool,
}

impl std::fmt::Debug for AccessibilityState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccessibilityState")
            .field("skipped", &self.skipped)
            .finish_non_exhaustive()
    }
}

impl AccessibilityState {
    /// A state with macOS's answer fixed to `trusted`.
    #[cfg(test)]
    pub fn fixed(skipped: bool, trusted: bool) -> Self {
        let ask_system: fn() -> bool = if trusted { || true } else { || false };
        Self {
            skipped,
            ask_system,
        }
    }

    /// The mode without Accessibility is on: the user chose it in
    /// onboarding. Then shortcuts go through the Tauri backend, and on macOS
    /// a dictation waits for the "Insert Last Dictation" service, whose
    /// shortcut is the one of "Paste Last Transcript".
    pub fn runs_without_accessibility(&self) -> bool {
        self.skipped
    }

    /// Whether DanyWhere may simulate key presses (Enigo): the paste
    /// keystroke and the key presses after a paste. Only the choice counts,
    /// as before this module: without it macOS would ask for the permission
    /// again on every launch. A Mac that took the permission back without
    /// the user choosing the mode still tries, and the paste fails
    /// (`NoticeCode::PasteFailed`).
    pub fn can_simulate_input(&self) -> bool {
        !self.skipped
    }

    /// Whether DanyWhere reads window titles through the Accessibility API:
    /// macOS grants it and the user didn't choose to go without. In the mode
    /// without it titles come by Apple event, also once macOS grants the
    /// permission, like the rest of the mode.
    pub fn can_read_windows(&self) -> bool {
        !self.skipped && self.trusted()
    }

    /// macOS's own answer (`AXIsProcessTrusted`), asked now; always `true`
    /// elsewhere. Callers ask a question above instead, unless they need
    /// exactly this.
    pub fn trusted(&self) -> bool {
        (self.ask_system)()
    }
}

/// The state for `settings`: their stored choice, and macOS asked when a
/// question needs it.
pub fn state(settings: &AppSettings) -> AccessibilityState {
    AccessibilityState {
        skipped: settings.accessibility_skipped,
        ask_system: system_trusted,
    }
}

/// macOS's answer, replaced in tests ([`testing::with_system_trusted`]).
#[cfg(not(test))]
fn system_trusted() -> bool {
    platform_trusted()
}

#[cfg(test)]
fn system_trusted() -> bool {
    testing::answer()
}

/// Asks the system: `AXIsProcessTrusted` on macOS, which never shows a
/// prompt; nothing to grant elsewhere.
#[cfg_attr(test, allow(dead_code))]
fn platform_trusted() -> bool {
    #[cfg(target_os = "macos")]
    {
        #[link(name = "ApplicationServices", kind = "framework")]
        extern "C" {
            fn AXIsProcessTrusted() -> bool;
        }
        // SAFETY: a plain query without side effects.
        unsafe { AXIsProcessTrusted() }
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Setting both parts of the state in tests, without the real permission.
#[cfg(test)]
pub mod testing {
    use std::cell::Cell;

    use crate::settings::{get_default_settings, AppSettings};

    thread_local! {
        static TRUSTED: Cell<bool> = const { Cell::new(true) };
        static ASKED: Cell<usize> = const { Cell::new(0) };
    }

    /// Every combination of the user's choice and macOS's answer, as
    /// `(skipped, trusted)`.
    pub const STATES: [(bool, bool); 4] =
        [(false, true), (false, false), (true, true), (true, false)];

    /// macOS's answer in this test thread, counted.
    pub(super) fn answer() -> bool {
        ASKED.with(|asked| asked.set(asked.get() + 1));
        TRUSTED.with(Cell::get)
    }

    /// Runs `f` with macOS answering `trusted` in this thread (`true`
    /// otherwise, as on systems without the permission).
    pub fn with_system_trusted<R>(trusted: bool, f: impl FnOnce() -> R) -> R {
        struct Restore(bool);
        impl Drop for Restore {
            fn drop(&mut self) {
                TRUSTED.with(|answer| answer.set(self.0));
            }
        }
        let _restore = Restore(TRUSTED.with(|answer| answer.replace(trusted)));
        f()
    }

    /// `f`'s result and how often it asked macOS.
    pub fn times_asked<R>(f: impl FnOnce() -> R) -> (R, usize) {
        let before = ASKED.with(Cell::get);
        let result = f();
        (result, ASKED.with(Cell::get) - before)
    }

    /// `settings` with only the stored choice set to `skipped`: everything
    /// else, the paste method included, stays as it is.
    pub fn with_choice(mut settings: AppSettings, skipped: bool) -> AppSettings {
        settings.accessibility_skipped = skipped;
        settings
    }

    /// The default settings as onboarding leaves them after "Continue
    /// without Accessibility".
    pub fn settings_without_accessibility() -> AppSettings {
        let mut settings = get_default_settings();
        crate::shortcut::apply_no_accessibility_mode(&mut settings);
        settings
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{
        settings_without_accessibility, times_asked, with_choice, with_system_trusted, STATES,
    };
    use super::{state, AccessibilityState};
    use crate::settings::get_default_settings;

    #[test]
    fn by_default_danywhere_runs_with_accessibility() {
        let access = state(&get_default_settings());
        assert!(!access.runs_without_accessibility());
        assert!(access.can_simulate_input());
    }

    #[test]
    fn onboardings_choice_switches_the_mode_on() {
        let access = state(&settings_without_accessibility());
        assert!(access.runs_without_accessibility());
        assert!(!access.can_simulate_input());
    }

    #[test]
    fn the_stored_choice_alone_decides_the_mode() {
        for (skipped, trusted) in STATES {
            let settings = with_choice(get_default_settings(), skipped);
            let access = with_system_trusted(trusted, || state(&settings));
            assert_eq!(
                with_system_trusted(trusted, || access.runs_without_accessibility()),
                skipped,
                "skipped {skipped}, trusted {trusted}"
            );
        }
    }

    #[test]
    fn input_is_simulated_unless_the_user_chose_to_go_without() {
        // As before this module: macOS's answer doesn't count.
        for (skipped, trusted) in STATES {
            let settings = with_choice(get_default_settings(), skipped);
            assert_eq!(
                with_system_trusted(trusted, || state(&settings).can_simulate_input()),
                !skipped,
                "skipped {skipped}, trusted {trusted}"
            );
        }
    }

    #[test]
    fn macos_answer_is_replaced_in_tests() {
        for (skipped, trusted) in STATES {
            let settings = with_choice(get_default_settings(), skipped);
            assert_eq!(
                with_system_trusted(trusted, || state(&settings).trusted()),
                trusted,
                "skipped {skipped}, trusted {trusted}"
            );
        }
    }

    #[test]
    fn a_replaced_answer_ends_with_its_closure() {
        let settings = get_default_settings();
        with_system_trusted(false, || assert!(!state(&settings).trusted()));
        assert!(state(&settings).trusted());
    }

    #[test]
    fn the_questions_that_follow_the_choice_never_ask_macos() {
        for (skipped, _) in STATES {
            let access = state(&with_choice(get_default_settings(), skipped));
            let (_, asked) = times_asked(|| {
                access.runs_without_accessibility();
                access.can_simulate_input();
            });
            assert_eq!(asked, 0, "skipped {skipped}");
        }
    }

    #[test]
    fn macos_is_asked_each_time_its_answer_is_needed() {
        // A permission granted or taken back while DanyWhere runs counts at
        // once.
        let access = state(&get_default_settings());
        let (_, asked) = times_asked(|| {
            access.trusted();
            access.trusted();
        });
        assert_eq!(asked, 2);
    }

    #[test]
    fn windows_are_read_only_where_macos_grants_it_and_the_user_kept_it() {
        for (skipped, trusted) in STATES {
            let settings = with_choice(get_default_settings(), skipped);
            assert_eq!(
                with_system_trusted(trusted, || state(&settings).can_read_windows()),
                !skipped && trusted,
                "skipped {skipped}, trusted {trusted}"
            );
        }
    }

    #[test]
    fn the_mode_without_accessibility_never_asks_macos_about_windows() {
        let access = state(&settings_without_accessibility());
        let (read, asked) = times_asked(|| access.can_read_windows());
        assert!(!read);
        assert_eq!(asked, 0);
        let access = state(&get_default_settings());
        let (_, asked) = times_asked(|| access.can_read_windows());
        assert_eq!(asked, 1);
    }

    #[test]
    fn a_fixed_state_answers_as_given() {
        for (skipped, trusted) in STATES {
            let access = AccessibilityState::fixed(skipped, trusted);
            assert_eq!(access.runs_without_accessibility(), skipped);
            assert_eq!(access.trusted(), trusted);
        }
    }
}
