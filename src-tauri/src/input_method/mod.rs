//! DanyWhere: inserting through an input method of DanyWhere's own on macOS
//! (user's decision, 2026-11-23; ROADMAP "Inserting on macOS: an input method
//! of DanyWhere's own"). Names, link and protocol:
//! `src-tauri/macos/input-method/PROTOCOL.md`.
//!
//! One rule by what the Mac can do: (1) once the user has added the DanyWhere
//! input source, DanyWhere inserts through it, with or without Accessibility,
//! without the clipboard; (2) else, with Accessibility, it pastes with ⌘V;
//! (3) else the text waits for the "Insert Last Dictation" service.
//!
//! Everything else asks the input method through [`InputMethod`], got with
//! [`current`]. The app installs its macOS implementation (`platform`) at
//! startup; until then, in tests, on Windows and Linux, and in builds that
//! carry no input method, [`NotAvailable`] answers: never added, so rule 1
//! never applies and everything stays as before.

// The contract comes first; the flow and the platform code use it.
#![allow(dead_code)]

#[cfg(target_os = "macos")]
pub mod platform;

use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};
use specta::Type;

/// Where the input method stands, for the settings and the setup guide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
pub struct InputMethodStatus {
    /// This build carries an input method (macOS builds made by the release
    /// and test-build scripts).
    pub available: bool,
    /// A copy is in `~/Library/Input Methods`.
    pub installed: bool,
    /// The user has added the DanyWhere input source in System Settings.
    pub added: bool,
    /// The input method is running and linked to DanyWhere.
    pub connected: bool,
}

/// An insertion through the input method that went through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inserted {
    /// Bundle id of the application the text went into, if known.
    pub client: Option<String>,
}

/// Why an insertion through the input method did not happen. The caller
/// then falls back to rule 2 or 3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertFailure {
    /// The source isn't added, or this build has no input method.
    NotAdded,
    /// The source could not be selected.
    NotSelected,
    /// The input method didn't link up in time.
    NotConnected,
    /// No text field has the input method (no focus in a text field, a
    /// password field, an app without input method support).
    NoClient,
    /// No answer in time, or another error; the detail is for the log.
    Failed(String),
}

/// What DanyWhere asks of its input method.
pub trait InputMethod: Send + Sync {
    /// Whether the user has added the DanyWhere input source and this build
    /// carries the input method. Cheap; asked at every decision.
    fn added(&self) -> bool;

    /// The state for the settings and the setup guide.
    fn status(&self) -> InputMethodStatus;

    /// At a recording's start, when its text is to be inserted through the
    /// input method: remembers the current input source and keyboard layout,
    /// tells the input method the layout, and selects the DanyWhere source.
    /// Does nothing when the source isn't added.
    fn begin(&self);

    /// Inserts `text` into the focused text field through the input method,
    /// selecting the source first if it isn't the current one (remembering
    /// what to go back to). Blocks for at most about two seconds; never call
    /// it on the main thread.
    fn insert(&self, text: &str) -> Result<Inserted, InsertFailure>;

    /// After a dictation or a "Paste Last", whatever its outcome: selects the
    /// remembered input source again if the DanyWhere source is still the
    /// current one (one the user chose meanwhile stays).
    fn end(&self);

    /// Installs or updates the copy in `~/Library/Input Methods`, registers
    /// it and opens System Settings where the user adds the source.
    fn set_up(&self) -> Result<(), String>;
}

/// The answer without an input method: never added.
#[derive(Debug, Default)]
pub struct NotAvailable;

impl InputMethod for NotAvailable {
    fn added(&self) -> bool {
        false
    }

    fn status(&self) -> InputMethodStatus {
        InputMethodStatus::default()
    }

    fn begin(&self) {}

    fn insert(&self, _text: &str) -> Result<Inserted, InsertFailure> {
        Err(InsertFailure::NotAdded)
    }

    fn end(&self) {}

    fn set_up(&self) -> Result<(), String> {
        Err("This build has no input method".into())
    }
}

static INSTALLED: OnceLock<Arc<dyn InputMethod>> = OnceLock::new();

/// Makes `input_method` the one [`current`] answers with, once, at startup.
pub fn install(input_method: Arc<dyn InputMethod>) {
    if INSTALLED.set(input_method).is_err() {
        log::warn!("The input method was installed twice; keeping the first");
    }
}

/// The input method the app installed, else [`NotAvailable`].
pub fn current() -> Arc<dyn InputMethod> {
    INSTALLED
        .get()
        .cloned()
        .unwrap_or_else(|| Arc::new(NotAvailable))
}
