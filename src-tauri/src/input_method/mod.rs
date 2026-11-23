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
    /// The text was sent, but the input method didn't confirm it in time or
    /// the link broke before its answer. The text may have arrived, so it
    /// is not inserted another way: a ⌘V after a late insertion would put
    /// it in twice.
    Unanswered,
    /// Another error; the detail is for the log.
    Failed(String),
}

impl InsertFailure {
    /// Whether the text may have arrived all the same, so that no other way
    /// of inserting it may follow.
    pub fn may_have_arrived(&self) -> bool {
        matches!(self, InsertFailure::Unanswered)
    }
}

/// What DanyWhere asks of its input method.
pub trait InputMethod: Send + Sync {
    /// Whether the user has added the DanyWhere input source and this build
    /// carries the input method. Cheap; asked at every decision.
    fn added(&self) -> bool;

    /// The state for the settings and the setup guide.
    fn status(&self) -> InputMethodStatus;

    /// Adds a hold on the DanyWhere source for one operation (a dictation
    /// whose text is to go through the input method, from its recording's
    /// start or right before its insertion; a "Paste Last"). Operations
    /// overlap: a recording starts while the previous dictation's text is
    /// still being inserted. While another source is current, remembers it
    /// and its keyboard layout, tells the input method the layout, and
    /// selects the DanyWhere source; with it current already, nothing more.
    /// Selects nothing when the source isn't added or the current source
    /// can't be read. Every `begin` is ended by exactly one [`Self::end`]
    /// (`delivery::InputSourceHold`).
    fn begin(&self);

    /// Inserts `text` into the focused text field through the input method,
    /// within a hold ([`Self::begin`]): selects the source first if it isn't
    /// the current one (remembering the one that is), never changes the
    /// holds. `NotSelected` when the current source can't be read. Blocks
    /// for at most about two seconds; never call it on the main thread.
    fn insert(&self, text: &str) -> Result<Inserted, InsertFailure>;

    /// Ends one hold ([`Self::begin`]), whatever its operation's outcome.
    /// Only when the last hold ends: selects the remembered input source
    /// again unless the user chose another one meanwhile (that one stays),
    /// and tells the input method to forget the layout and the source to go
    /// back to.
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

/// An input method for tests, without macOS: added or not, inserting as a
/// test says, and telling the test each call in order.
#[cfg(test)]
pub mod testing {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use super::{InputMethod, InputMethodStatus, InsertFailure, Inserted};

    /// One call of a [`FakeInputMethod`].
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum ImCall {
        Begin,
        /// The text it was asked to insert.
        Insert(String),
        End,
    }

    /// What an insertion that went through answers: the text went into
    /// TextEdit.
    pub fn inserted() -> Result<Inserted, InsertFailure> {
        Ok(Inserted {
            client: Some("com.apple.TextEdit".to_string()),
        })
    }

    /// Records every call, also those a real input method would only count
    /// (`begin` and `end` while the source isn't added), so a test sees a
    /// call that shouldn't have been made.
    pub struct FakeInputMethod {
        added: AtomicBool,
        insert: Mutex<Result<Inserted, InsertFailure>>,
        record: Box<dyn Fn(ImCall) + Send + Sync>,
    }

    impl FakeInputMethod {
        /// The source is `added` or not; an insertion while it is answers
        /// `insert`; each call goes to `record`.
        pub fn new(
            added: bool,
            insert: Result<Inserted, InsertFailure>,
            record: impl Fn(ImCall) + Send + Sync + 'static,
        ) -> Self {
            Self {
                added: AtomicBool::new(added),
                insert: Mutex::new(insert),
                record: Box::new(record),
            }
        }

        /// From now on the source answers as `added` (e.g. `false` as when
        /// the main thread doesn't answer in time).
        pub fn set_added(&self, added: bool) {
            self.added.store(added, Ordering::SeqCst);
        }

        /// A fake that keeps its calls in `calls`.
        pub fn keeping_calls_in(
            added: bool,
            insert: Result<Inserted, InsertFailure>,
            calls: &Arc<Mutex<Vec<ImCall>>>,
        ) -> Self {
            let calls = Arc::clone(calls);
            Self::new(added, insert, move |call| calls.lock().unwrap().push(call))
        }
    }

    impl InputMethod for FakeInputMethod {
        fn added(&self) -> bool {
            self.added.load(Ordering::SeqCst)
        }

        fn status(&self) -> InputMethodStatus {
            let added = self.added();
            InputMethodStatus {
                available: true,
                installed: added,
                added,
                connected: added,
            }
        }

        fn begin(&self) {
            (self.record)(ImCall::Begin);
        }

        fn insert(&self, text: &str) -> Result<Inserted, InsertFailure> {
            (self.record)(ImCall::Insert(text.to_string()));
            if !self.added() {
                return Err(InsertFailure::NotAdded);
            }
            self.insert.lock().unwrap().clone()
        }

        fn end(&self) {
            (self.record)(ImCall::End);
        }

        fn set_up(&self) -> Result<(), String> {
            Err("A fake input method sets nothing up".into())
        }
    }
}
