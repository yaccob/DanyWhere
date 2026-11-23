//! DanyWhere: the app's input method on macOS, [`AppInputMethod`]: the
//! bundled input method, its installed copy, its input source and the link,
//! put together behind [`InputMethod`] (semantics in its doc comments and
//! PROTOCOL.md).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::install::Host;
use super::link::{ImLink, InsertAnswer, LinkError};
use super::paths::BundledInputMethod;
use super::sources::InputSources;
use crate::input_method::{InputMethod, InputMethodStatus, InsertFailure, Inserted};

/// How long an insertion waits for each step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    /// For the input method to link up after its source was selected.
    pub link: Duration,
    /// For a text field to have the input method.
    pub client: Duration,
    /// For the answer to `insert`.
    pub answer: Duration,
    /// For the whole insertion.
    pub total: Duration,
    /// The answer gets at least this long, even past `total`.
    pub min_answer: Duration,
    /// For the input method to quit before its copy is replaced.
    pub quit: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Timeouts {
            link: Duration::from_millis(1500),
            client: Duration::from_millis(1000),
            answer: Duration::from_millis(1500),
            total: Duration::from_millis(2500),
            min_answer: Duration::from_millis(500),
            quit: Duration::from_millis(1000),
        }
    }
}

/// The source and layout to go back to after a dictation.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Remembered {
    source: String,
    layout: Option<String>,
}

/// The operations that hold the DanyWhere source (`InputMethod::begin`), and
/// what to go back to when the last of them ends. Dictations and "Paste
/// Last" overlap: a recording starts while the previous dictation's text is
/// still being inserted.
#[derive(Debug, Default)]
struct Holds {
    /// Operations between their `begin` and their `end`.
    count: usize,
    remembered: Option<Remembered>,
}

/// Why the DanyWhere source isn't selected.
enum NotSelected {
    /// The current source can't be read (the main thread didn't answer in
    /// time): selecting its own then could leave it selected for good.
    CurrentUnknown,
    /// Selecting it failed.
    Failed,
}

/// The input method of this app on macOS.
pub struct AppInputMethod {
    bundled: Option<BundledInputMethod>,
    installed: PathBuf,
    sources: Arc<dyn InputSources>,
    link: Arc<dyn ImLink>,
    host: Arc<dyn Host>,
    timeouts: Timeouts,
    holds: Mutex<Holds>,
}

impl AppInputMethod {
    /// `bundled` is the input method in the app (`None`: this build has
    /// none), `installed` where its copy goes.
    pub fn new(
        bundled: Option<BundledInputMethod>,
        installed: PathBuf,
        sources: Arc<dyn InputSources>,
        link: Arc<dyn ImLink>,
        host: Arc<dyn Host>,
        timeouts: Timeouts,
    ) -> Self {
        AppInputMethod {
            bundled,
            installed,
            sources,
            link,
            host,
            timeouts,
            holds: Mutex::new(Holds::default()),
        }
    }

    /// A copy is in `~/Library/Input Methods`.
    pub fn is_installed(&self) -> bool {
        self.bundled.is_some() && self.installed.join("Contents/Info.plist").is_file()
    }

    /// The input source id of the DanyWhere source.
    fn id(&self) -> Option<&str> {
        self.bundled.as_ref().map(|bundled| bundled.id.as_str())
    }

    /// Whether the DanyWhere source is the current one.
    fn ours_is_current(&self, id: &str) -> bool {
        self.sources.current_source().as_deref() == Some(id)
    }

    /// The holds, locked: `begin`, `insert`'s selecting, `end` and undoing a
    /// leftover selection each run as one step.
    fn holds(&self) -> std::sync::MutexGuard<'_, Holds> {
        self.holds
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Makes the DanyWhere source the current one. When another is current,
    /// remembers it and its keyboard layout to go back to (the user's choice
    /// made meanwhile replaces what was remembered) and tells the input
    /// method. Selects nothing while the current source can't be read.
    fn select_ours(&self, holds: &mut Holds, id: &str) -> Result<(), NotSelected> {
        let Some(current) = self.sources.current_source() else {
            log::warn!("Input method: the current input source is unknown; not selecting its own");
            return Err(NotSelected::CurrentUnknown);
        };
        if current == id {
            return Ok(());
        }
        let layout = self.sources.current_layout();
        log::debug!("Input method: remembering {current} (layout {layout:?})");
        self.link.set_layout(layout.clone());
        self.link.set_restore(Some(current.clone()));
        holds.remembered = Some(Remembered {
            source: current,
            layout,
        });
        self.sources.select(id).map_err(|e| {
            log::warn!("Input method: selecting its source failed: {e}");
            NotSelected::Failed
        })
    }

    /// Replaces the installed copy with the bundled one and registers it.
    /// A running input method is asked to quit first, else ended.
    fn install_copy(&self, bundled: &BundledInputMethod) -> Result<(), String> {
        if self.is_installed() {
            self.stop_running(bundled);
        }
        super::install::copy_into_place(&bundled.path, &self.installed)
            .map_err(|e| format!("Couldn't copy the input method: {e}"))?;
        log::info!(
            "Input method: copied version {} to {}",
            bundled.version,
            self.installed.display()
        );
        self.sources.register(&self.installed)
    }

    fn stop_running(&self, bundled: &BundledInputMethod) {
        if self.link.quit() {
            log::info!("Input method: asked the running input method to quit");
            if self.link.wait_disconnected(self.timeouts.quit) {
                return;
            }
            log::info!("Input method: it didn't quit in time");
        }
        self.host.terminate(&bundled.id);
    }

    /// At startup: replaces an installed copy that differs from the bundled
    /// one and registers it again. `Ok(true)` if it did; nothing is
    /// installed where no copy is.
    pub fn update_installed_copy(&self) -> Result<bool, String> {
        let Some(bundled) = &self.bundled else {
            return Ok(false);
        };
        if !self.is_installed()
            || !super::install::differs(&bundled.path, &self.installed, &bundled.executable)
        {
            return Ok(false);
        }
        log::info!("Input method: the installed copy is outdated; updating it");
        self.install_copy(bundled)?;
        Ok(true)
    }

    /// At startup: installs this input method with `install`
    /// (`input_method::install`) at once, asking macOS nothing, so whoever
    /// asks next gets it; then, on a thread of its own, updates an installed
    /// copy that differs ([`Self::update_installed_copy`]) and undoes a
    /// selection left over after a crash ([`Self::leave_leftover_selection`],
    /// which leaves a selection alone that a dictation begun meanwhile
    /// holds). The thread, for tests.
    pub fn install_then_prepare(
        self: Arc<Self>,
        install: impl FnOnce(Arc<dyn InputMethod>),
    ) -> Option<std::thread::JoinHandle<()>> {
        install(Arc::clone(&self) as Arc<dyn InputMethod>);
        let spawned = std::thread::Builder::new()
            .name("input-method-start".into())
            .spawn(move || {
                match self.update_installed_copy() {
                    Ok(true) => log::info!("Input method: the installed copy was updated"),
                    Ok(false) => {}
                    Err(e) => log::warn!("Input method: updating the installed copy failed: {e}"),
                }
                self.leave_leftover_selection();
                log::info!("Input method: ready ({:?})", self.status());
            });
        match spawned {
            Ok(handle) => Some(handle),
            Err(e) => {
                log::error!("Input method: no thread to start on: {e}");
                None
            }
        }
    }

    /// At startup: if the DanyWhere source is the current one (left over
    /// after a crash), selects an ASCII-capable keyboard layout. Whether it
    /// did. Nothing while an operation holds the source: the app installs
    /// its input method first, so a dictation may already have begun.
    pub fn leave_leftover_selection(&self) -> bool {
        let Some(id) = self.id() else {
            return false;
        };
        let holds = self.holds();
        if holds.count > 0 || !self.ours_is_current(id) {
            return false;
        }
        let Some(layout) = self.sources.ascii_capable_layout() else {
            log::warn!("Input method: left selected, but no keyboard layout to go back to");
            return false;
        };
        match self.sources.select(&layout) {
            Ok(()) => {
                log::info!("Input method: was left selected; selected {layout} again");
                true
            }
            Err(e) => {
                log::warn!("Input method: was left selected; selecting {layout} failed: {e}");
                false
            }
        }
    }
}

impl InputMethod for AppInputMethod {
    fn added(&self) -> bool {
        match self.id() {
            Some(id) => self.is_installed() && self.sources.is_added(id),
            None => false,
        }
    }

    fn status(&self) -> InputMethodStatus {
        if self.bundled.is_none() {
            return InputMethodStatus::default();
        }
        InputMethodStatus {
            available: true,
            installed: self.is_installed(),
            added: self.added(),
            connected: self.link.connected(),
        }
    }

    fn begin(&self) {
        // Counted whatever happens below: its `end` comes all the same.
        let mut holds = self.holds();
        holds.count += 1;
        log::debug!("Input method: {} operation(s) hold its source", holds.count);
        let Some(id) = self.id() else {
            return;
        };
        if !self.added() {
            return;
        }
        // Errors are logged; inserting tries again and falls back.
        let _ = self.select_ours(&mut holds, id);
    }

    fn insert(&self, text: &str) -> Result<Inserted, InsertFailure> {
        let started = Instant::now();
        let Some(id) = self.id() else {
            return Err(InsertFailure::NotAdded);
        };
        if !self.added() {
            return Err(InsertFailure::NotAdded);
        }
        // The user may have chosen another source since `begin`.
        if self.select_ours(&mut self.holds(), id).is_err() {
            return Err(InsertFailure::NotSelected);
        }
        let left =
            |limit: Duration| limit.min(self.timeouts.total.saturating_sub(started.elapsed()));
        if !self.link.wait_connected(left(self.timeouts.link)) {
            log::info!("Input method: it didn't link up in time");
            return Err(InsertFailure::NotConnected);
        }
        if self.link.active_client().is_none() {
            let wait = left(self.timeouts.client).min(
                self.timeouts
                    .total
                    .saturating_sub(started.elapsed())
                    .saturating_sub(self.timeouts.min_answer),
            );
            if !self.link.wait_active(wait) {
                log::debug!("Input method: no text field reported; asking anyway");
            }
        }
        let answer_time = left(self.timeouts.answer).max(self.timeouts.min_answer);
        let result = match self.link.insert(text, answer_time) {
            Ok(InsertAnswer::Inserted { client }) => Ok(Inserted { client }),
            Ok(InsertAnswer::NoClient) => Err(InsertFailure::NoClient),
            Ok(InsertAnswer::Error(reason)) => Err(InsertFailure::Failed(reason)),
            Err(LinkError::NotConnected) => Err(InsertFailure::NotConnected),
            // Sent: the text may have arrived all the same.
            Err(LinkError::Timeout | LinkError::Disconnected) => Err(InsertFailure::Unanswered),
            Err(LinkError::Io(e)) => Err(InsertFailure::Failed(format!("sending failed: {e}"))),
        };
        match &result {
            Ok(inserted) => log::info!(
                "Input method: inserted into {} in {} ms",
                inserted.client.as_deref().unwrap_or("an unknown app"),
                started.elapsed().as_millis()
            ),
            Err(failure) => log::info!(
                "Input method: not inserted ({failure:?}) after {} ms",
                started.elapsed().as_millis()
            ),
        }
        result
    }

    fn end(&self) {
        let mut holds = self.holds();
        if holds.count == 0 {
            log::warn!("Input method: an end without its begin; ignored");
            return;
        }
        holds.count -= 1;
        if holds.count > 0 {
            log::debug!(
                "Input method: {} operation(s) still hold its source",
                holds.count
            );
            return;
        }
        let Some(id) = self.id() else {
            return;
        };
        if let Some(remembered) = holds.remembered.take() {
            match self.sources.current_source() {
                Some(current) if current != id => {
                    log::debug!("Input method: another source was chosen meanwhile; it stays");
                }
                // DanyWhere's, or unknown (the main thread didn't answer):
                // back to the one from before rather than leaving its own
                // selected.
                _ => match self.sources.select(&remembered.source) {
                    Ok(()) => log::debug!("Input method: selected {} again", remembered.source),
                    Err(e) => log::warn!(
                        "Input method: selecting {} again failed: {e}",
                        remembered.source
                    ),
                },
            }
        }
        self.link.set_restore(None);
        self.link.set_layout(None);
    }

    fn set_up(&self) -> Result<(), String> {
        let Some(bundled) = &self.bundled else {
            return Err("This build has no input method".into());
        };
        if !self.is_installed()
            || super::install::differs(&bundled.path, &self.installed, &bundled.executable)
        {
            self.install_copy(bundled)?;
        } else {
            self.sources.register(&self.installed)?;
        }
        self.host.open_settings()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_method::platform::install::testing::FakeHost;
    use crate::input_method::platform::paths::testing::fake_bundle;
    use crate::input_method::platform::paths::{self, find_bundled};
    use crate::input_method::platform::sources::testing::FakeSources;
    use std::fs;

    const APP: &str = "com.yaccob.danywhere";
    const OURS: &str = "com.yaccob.danywhere.inputmethod.DanyWhere";
    const GERMAN: &str = "com.apple.keylayout.German";
    const FRENCH: &str = "com.apple.keylayout.French";
    const NAME: &str = "DanyWhere Input.app";

    /// The link, played by the test: logs what it was asked.
    struct FakeLink {
        state: Mutex<LinkState>,
    }

    struct LinkState {
        connected: bool,
        active: Option<String>,
        /// A text field gets the input method this long into `wait_active`.
        active_after: Option<Duration>,
        answer: Result<InsertAnswer, LinkError>,
        quit_disconnects: bool,
        log: Vec<String>,
    }

    impl FakeLink {
        fn new(connected: bool) -> Self {
            FakeLink {
                state: Mutex::new(LinkState {
                    connected,
                    active: connected.then(|| "com.apple.TextEdit".to_string()),
                    active_after: None,
                    answer: Ok(InsertAnswer::Inserted {
                        client: Some("com.apple.TextEdit".into()),
                    }),
                    quit_disconnects: true,
                    log: Vec::new(),
                }),
            }
        }

        fn log(&self) -> Vec<String> {
            self.state.lock().unwrap().log.clone()
        }

        fn answer_with(&self, answer: Result<InsertAnswer, LinkError>) {
            self.state.lock().unwrap().answer = answer;
        }
    }

    fn shown(source: &Option<String>) -> String {
        source.clone().unwrap_or_else(|| "null".into())
    }

    impl ImLink for FakeLink {
        fn connected(&self) -> bool {
            self.state.lock().unwrap().connected
        }

        fn wait_connected(&self, timeout: Duration) -> bool {
            if self.connected() {
                return true;
            }
            std::thread::sleep(timeout);
            false
        }

        fn wait_disconnected(&self, timeout: Duration) -> bool {
            if !self.connected() {
                return true;
            }
            std::thread::sleep(timeout);
            false
        }

        fn active_client(&self) -> Option<String> {
            self.state.lock().unwrap().active.clone()
        }

        fn wait_active(&self, timeout: Duration) -> bool {
            let after = {
                let mut state = self.state.lock().unwrap();
                state.log.push("wait for a text field".into());
                if state.active.is_some() {
                    return true;
                }
                state.active_after
            };
            match after {
                Some(after) if after <= timeout => {
                    std::thread::sleep(after);
                    self.state.lock().unwrap().active = Some("com.apple.Notes".into());
                    true
                }
                _ => {
                    std::thread::sleep(timeout);
                    false
                }
            }
        }

        fn set_layout(&self, source: Option<String>) {
            let mut state = self.state.lock().unwrap();
            state.log.push(format!("layout {}", shown(&source)));
        }

        fn set_restore(&self, source: Option<String>) {
            let mut state = self.state.lock().unwrap();
            state.log.push(format!("restore {}", shown(&source)));
        }

        fn insert(&self, text: &str, timeout: Duration) -> Result<InsertAnswer, LinkError> {
            let answer = {
                let mut state = self.state.lock().unwrap();
                state.log.push(format!("insert {text}"));
                if !state.connected {
                    return Err(LinkError::NotConnected);
                }
                state.answer.clone()
            };
            if answer == Err(LinkError::Timeout) {
                std::thread::sleep(timeout);
            }
            answer
        }

        fn quit(&self) -> bool {
            let mut state = self.state.lock().unwrap();
            if !state.connected {
                return false;
            }
            state.log.push("quit".into());
            if state.quit_disconnects {
                state.connected = false;
            }
            true
        }
    }

    fn short() -> Timeouts {
        Timeouts {
            link: Duration::from_millis(200),
            client: Duration::from_millis(150),
            answer: Duration::from_millis(300),
            total: Duration::from_millis(400),
            min_answer: Duration::from_millis(100),
            quit: Duration::from_millis(100),
        }
    }

    struct Setup {
        _dir: tempfile::TempDir,
        bundled: BundledInputMethod,
        installed: PathBuf,
        sources: Arc<FakeSources>,
        link: Arc<FakeLink>,
        host: Arc<FakeHost>,
        im: Arc<AppInputMethod>,
    }

    /// An app with a bundled input method; `installed` puts a copy (the
    /// same) in place, `added` adds its source, `connected` links it up.
    fn setup(installed: bool, added: bool, connected: bool) -> Setup {
        let dir = tempfile::tempdir().unwrap();
        let helpers = dir.path().join("DanyWhere.app/Contents/Helpers");
        fake_bundle(&helpers, NAME, OURS, Some(APP), "0.3.3", b"v1");
        let bundled = find_bundled(&helpers, APP).expect("bundled");
        let home = dir.path().join("home");
        let installed_path = paths::installed_path(&home, NAME);
        if installed {
            fake_bundle(
                &paths::input_methods_dir(&home),
                NAME,
                OURS,
                Some(APP),
                "0.3.3",
                b"v1",
            );
        }
        let added_ids: Vec<&str> = if added {
            vec![OURS, GERMAN, FRENCH]
        } else {
            vec![GERMAN, FRENCH]
        };
        let sources = Arc::new(FakeSources::new(&added_ids, GERMAN));
        let link = Arc::new(FakeLink::new(connected));
        let host = Arc::new(FakeHost::default());
        let im = Arc::new(AppInputMethod::new(
            Some(bundled.clone()),
            installed_path.clone(),
            sources.clone(),
            link.clone(),
            host.clone(),
            short(),
        ));
        Setup {
            _dir: dir,
            bundled,
            installed: installed_path,
            sources,
            link,
            host,
            im,
        }
    }

    fn ready() -> Setup {
        setup(true, true, true)
    }

    // --- added and status ---

    #[test]
    fn added_needs_the_bundle_the_copy_and_the_source() {
        assert!(ready().im.added());
        assert!(!setup(true, false, true).im.added(), "source not added");
        assert!(!setup(false, true, true).im.added(), "no copy installed");

        let s = ready();
        let without_bundle = AppInputMethod::new(
            None,
            s.installed.clone(),
            s.sources.clone(),
            s.link.clone(),
            s.host.clone(),
            short(),
        );
        assert!(!without_bundle.added(), "this build has no input method");
    }

    #[test]
    fn status_reports_each_part() {
        assert_eq!(
            ready().im.status(),
            InputMethodStatus {
                available: true,
                installed: true,
                added: true,
                connected: true
            }
        );
        assert_eq!(
            setup(true, false, false).im.status(),
            InputMethodStatus {
                available: true,
                installed: true,
                added: false,
                connected: false
            }
        );
        assert_eq!(
            setup(false, false, false).im.status(),
            InputMethodStatus {
                available: true,
                installed: false,
                added: false,
                connected: false
            }
        );
        let s = ready();
        let without_bundle = AppInputMethod::new(
            None,
            PathBuf::new(),
            s.sources.clone(),
            Arc::new(FakeLink::new(false)),
            s.host.clone(),
            short(),
        );
        assert_eq!(without_bundle.status(), InputMethodStatus::default());
    }

    // --- not added: rule 1 doesn't apply ---

    #[test]
    fn without_the_source_nothing_is_selected() {
        let s = setup(true, false, true);
        s.im.begin();
        assert_eq!(s.im.insert("Hallo"), Err(InsertFailure::NotAdded));
        s.im.end();
        assert!(s.sources.selected().is_empty());
        assert!(
            !s.link.log().iter().any(|line| line.starts_with("insert")),
            "{:?}",
            s.link.log()
        );
    }

    // --- begin, insert, end ---

    #[test]
    fn begin_selects_ours_and_end_goes_back() {
        let s = ready();
        s.im.begin();
        assert_eq!(s.sources.current(), OURS);
        assert_eq!(
            s.link.log(),
            vec![format!("layout {GERMAN}"), format!("restore {GERMAN}")]
        );

        s.im.end();

        assert_eq!(s.sources.selected(), vec![OURS, GERMAN]);
        assert_eq!(s.sources.current(), GERMAN);
        assert_eq!(
            s.link.log()[2..],
            ["restore null".to_string(), "layout null".to_string()]
        );
    }

    #[test]
    fn the_layout_sent_is_the_one_in_use() {
        let s = ready();
        // An input method of another language is current; its layout is
        // the one typing used.
        s.sources
            .state
            .lock()
            .unwrap()
            .added
            .push("com.apple.inputmethod.Kotoeri".into());
        s.sources.user_selects("com.apple.inputmethod.Kotoeri");
        s.im.begin();
        assert_eq!(
            s.link.log(),
            vec![
                format!("layout {GERMAN}"),
                "restore com.apple.inputmethod.Kotoeri".to_string()
            ]
        );
        s.im.end();
        assert_eq!(s.sources.current(), "com.apple.inputmethod.Kotoeri");
    }

    #[test]
    fn begin_with_ours_already_current_remembers_nothing() {
        let s = ready();
        s.sources.user_selects(OURS);
        s.im.begin();
        s.im.end();
        assert!(
            s.sources.selected().is_empty(),
            "{:?}",
            s.sources.selected()
        );
        assert_eq!(s.sources.current(), OURS, "the user's choice stays");
    }

    #[test]
    fn a_source_the_user_chose_meanwhile_stays() {
        let s = ready();
        s.im.begin();
        s.sources.user_selects(FRENCH);
        s.im.end();
        assert_eq!(s.sources.selected(), vec![OURS]);
        assert_eq!(s.sources.current(), FRENCH);
    }

    #[test]
    fn insert_after_begin_goes_straight_through() {
        let s = ready();
        s.im.begin();
        assert_eq!(
            s.im.insert("Hallo Welt"),
            Ok(Inserted {
                client: Some("com.apple.TextEdit".into())
            })
        );
        assert_eq!(s.sources.selected(), vec![OURS], "selected once");
        assert!(s.link.log().contains(&"insert Hallo Welt".to_string()));
        s.im.end();
        assert_eq!(s.sources.current(), GERMAN);
    }

    #[test]
    fn insert_selects_ours_where_begin_could_not_and_end_goes_back() {
        let s = ready();
        s.sources.state.lock().unwrap().select_fails = true;
        s.im.begin();
        assert!(s.sources.selected().is_empty());
        s.sources.state.lock().unwrap().select_fails = false;
        assert!(s.im.insert("Paste Last").is_ok());
        assert_eq!(s.sources.selected(), vec![OURS]);
        assert_eq!(
            s.link.log()[..2],
            [format!("layout {GERMAN}"), format!("restore {GERMAN}")]
        );
        s.im.end();
        assert_eq!(s.sources.selected(), vec![OURS, GERMAN]);
    }

    #[test]
    fn insert_after_the_user_switched_returns_to_their_choice() {
        let s = ready();
        s.im.begin();
        s.sources.user_selects(FRENCH);
        assert!(s.im.insert("x").is_ok());
        s.im.end();
        assert_eq!(s.sources.selected(), vec![OURS, OURS, FRENCH]);
        assert_eq!(s.sources.current(), FRENCH);
    }

    #[test]
    fn an_end_without_its_begin_changes_nothing() {
        let s = ready();
        s.im.end();
        assert!(s.sources.selected().is_empty());
        assert!(s.link.log().is_empty(), "{:?}", s.link.log());
        s.im.begin();
        s.im.end();
        s.im.end();
        assert_eq!(
            s.sources.selected(),
            vec![OURS, GERMAN],
            "forgotten after the first end"
        );
        assert_eq!(
            s.link.log(),
            vec![
                format!("layout {GERMAN}"),
                format!("restore {GERMAN}"),
                "restore null".to_string(),
                "layout null".to_string()
            ]
        );
    }

    // --- holds: one per operation, the last one goes back ---

    #[test]
    fn overlapping_holds_go_back_only_when_the_last_ends() {
        let s = ready();
        // A dictation's recording, then the next one's while the first
        // one's text is still being inserted.
        s.im.begin();
        s.im.begin();
        assert_eq!(s.sources.selected(), vec![OURS], "selected once");
        s.im.end();
        assert_eq!(s.sources.current(), OURS, "the second recording keeps it");
        assert!(
            !s.link.log().contains(&"restore null".to_string()),
            "{:?}",
            s.link.log()
        );
        s.im.end();
        assert_eq!(s.sources.selected(), vec![OURS, GERMAN]);
        assert_eq!(s.sources.current(), GERMAN);
        assert_eq!(
            s.link.log()[2..],
            ["restore null".to_string(), "layout null".to_string()]
        );
    }

    #[test]
    fn three_overlapping_operations_go_back_once_at_the_end() {
        let s = ready();
        s.im.begin(); // dictation A
        s.im.begin(); // dictation B records while A's text goes in
        assert!(s.im.insert("A").is_ok());
        s.im.end(); // A is in
        s.im.begin(); // "Paste Last" during B's recording
        assert!(s.im.insert("Paste Last").is_ok());
        s.im.end();
        assert_eq!(s.sources.current(), OURS, "B still records");
        assert!(s.im.insert("B").is_ok());
        s.im.end(); // B is in
        assert_eq!(s.sources.current(), GERMAN);
        assert_eq!(
            s.sources.selected(),
            vec![OURS, GERMAN],
            "selected once, back once"
        );
    }

    #[test]
    fn inserting_changes_no_hold() {
        let s = ready();
        s.im.begin();
        assert!(s.im.insert("x").is_ok());
        assert!(s.im.insert("y").is_ok());
        s.im.end();
        assert_eq!(s.sources.current(), GERMAN, "one end for one begin");
    }

    // --- the current source can't be read (the main thread didn't answer) ---

    fn current_unknown(s: &Setup, unknown: bool) {
        s.sources.state.lock().unwrap().current_unknown = unknown;
    }

    #[test]
    fn begin_selects_nothing_while_the_current_source_is_unknown() {
        let s = ready();
        current_unknown(&s, true);
        s.im.begin();
        assert!(
            s.sources.selected().is_empty(),
            "{:?}",
            s.sources.selected()
        );
        assert!(s.link.log().is_empty(), "{:?}", s.link.log());
        s.im.end();
        assert!(s.sources.selected().is_empty());
        assert_eq!(s.sources.current(), GERMAN);
    }

    #[test]
    fn insert_falls_back_while_the_current_source_is_unknown() {
        let s = ready();
        current_unknown(&s, true);
        s.im.begin();
        assert_eq!(s.im.insert("x"), Err(InsertFailure::NotSelected));
        assert!(
            s.sources.selected().is_empty(),
            "{:?}",
            s.sources.selected()
        );
        assert!(
            !s.link.log().iter().any(|l| l.starts_with("insert")),
            "{:?}",
            s.link.log()
        );
        s.im.end();
        assert_eq!(s.sources.current(), GERMAN);
    }

    #[test]
    fn a_hold_that_began_unknown_remembers_once_the_source_is_known() {
        let s = ready();
        current_unknown(&s, true);
        s.im.begin();
        current_unknown(&s, false);
        assert!(s.im.insert("x").is_ok());
        s.im.end();
        assert_eq!(s.sources.selected(), vec![OURS, GERMAN]);
    }

    #[test]
    fn the_last_end_goes_back_when_the_current_source_cant_be_read() {
        // Better back to the source from before than DanyWhere's left
        // selected.
        let s = ready();
        s.im.begin();
        current_unknown(&s, true);
        s.im.end();
        assert_eq!(s.sources.selected(), vec![OURS, GERMAN]);
    }

    #[test]
    fn a_source_that_cant_be_selected() {
        let s = ready();
        s.sources.state.lock().unwrap().select_fails = true;
        assert_eq!(s.im.insert("x"), Err(InsertFailure::NotSelected));
        assert!(!s.link.log().iter().any(|l| l.starts_with("insert")));
    }

    #[test]
    fn no_link_in_time_is_not_connected() {
        let s = setup(true, true, false);
        let started = Instant::now();
        assert_eq!(s.im.insert("x"), Err(InsertFailure::NotConnected));
        let took = started.elapsed();
        assert!(took >= short().link, "{took:?}");
        assert!(took < short().link + Duration::from_millis(150), "{took:?}");
    }

    #[test]
    fn it_waits_for_a_text_field_before_sending() {
        let s = ready();
        {
            let mut state = s.link.state.lock().unwrap();
            state.active = None;
            state.active_after = Some(Duration::from_millis(50));
        }
        s.link.answer_with(Ok(InsertAnswer::Inserted {
            client: Some("com.apple.Notes".into()),
        }));
        assert_eq!(
            s.im.insert("x"),
            Ok(Inserted {
                client: Some("com.apple.Notes".into())
            })
        );
        let log = s.link.log();
        let waited = log.iter().position(|l| l == "wait for a text field");
        let sent = log.iter().position(|l| l == "insert x");
        assert!(waited.is_some() && waited < sent, "{log:?}");
    }

    #[test]
    fn without_a_text_field_it_still_asks_the_input_method() {
        let s = ready();
        s.link.state.lock().unwrap().active = None;
        s.link.answer_with(Ok(InsertAnswer::NoClient));
        assert_eq!(s.im.insert("x"), Err(InsertFailure::NoClient));
        assert!(s.link.log().contains(&"insert x".to_string()));
    }

    #[test]
    fn failures_of_the_input_method_are_failures() {
        let s = ready();
        s.link.answer_with(Ok(InsertAnswer::Error("error".into())));
        assert!(matches!(s.im.insert("x"), Err(InsertFailure::Failed(_))));
    }

    #[test]
    fn a_sent_insertion_without_an_answer_may_have_arrived() {
        let s = ready();
        s.link.answer_with(Err(LinkError::Disconnected));
        assert_eq!(s.im.insert("x"), Err(InsertFailure::Unanswered));
        s.link.answer_with(Err(LinkError::Timeout));
        assert_eq!(s.im.insert("x"), Err(InsertFailure::Unanswered));
    }

    #[test]
    fn an_insertion_takes_at_most_the_total_time() {
        let s = ready();
        {
            let mut state = s.link.state.lock().unwrap();
            state.active = None;
            state.answer = Err(LinkError::Timeout);
        }
        let started = Instant::now();
        assert!(s.im.insert("x").is_err());
        let took = started.elapsed();
        assert!(
            took >= short().total - Duration::from_millis(20),
            "{took:?}"
        );
        assert!(
            took < short().total + Duration::from_millis(150),
            "{took:?}"
        );
    }

    // --- installing ---

    #[test]
    fn set_up_installs_registers_and_opens_settings() {
        let s = setup(false, false, false);
        s.im.set_up().expect("set up");
        assert!(s.installed.join("Contents/Info.plist").is_file());
        assert!(s.im.is_installed());
        assert_eq!(s.sources.registered(), vec![s.installed.clone()]);
        assert_eq!(s.host.settings_opened(), 1);
        assert!(s.host.terminated().is_empty(), "nothing to end");
    }

    #[test]
    fn set_up_with_the_same_copy_only_registers_and_opens_settings() {
        let s = setup(true, false, true);
        s.im.set_up().expect("set up");
        assert_eq!(s.sources.registered(), vec![s.installed.clone()]);
        assert_eq!(s.host.settings_opened(), 1);
        assert!(!s.link.log().contains(&"quit".to_string()));
    }

    #[test]
    fn set_up_replaces_an_outdated_copy_after_asking_it_to_quit() {
        let s = setup(true, false, true);
        fs::write(s.installed.join("Contents/MacOS/DanyWhereInput"), b"old").unwrap();
        s.im.set_up().expect("set up");
        assert!(s.link.log().contains(&"quit".to_string()));
        assert!(s.host.terminated().is_empty(), "it quit by itself");
        assert_eq!(
            fs::read(s.installed.join("Contents/MacOS/DanyWhereInput")).unwrap(),
            b"v1"
        );
    }

    #[test]
    fn an_unlinked_old_copy_is_ended_by_its_bundle_id() {
        let s = setup(true, false, false);
        fs::write(s.installed.join("Contents/MacOS/DanyWhereInput"), b"old").unwrap();
        s.im.set_up().expect("set up");
        assert_eq!(s.host.terminated(), vec![OURS.to_string()]);
    }

    #[test]
    fn one_that_doesnt_quit_in_time_is_ended() {
        let s = setup(true, false, true);
        s.link.state.lock().unwrap().quit_disconnects = false;
        fs::write(s.installed.join("Contents/MacOS/DanyWhereInput"), b"old").unwrap();
        s.im.set_up().expect("set up");
        assert!(s.link.log().contains(&"quit".to_string()));
        assert_eq!(s.host.terminated(), vec![OURS.to_string()]);
    }

    #[test]
    fn set_up_without_a_bundled_input_method_fails() {
        let s = ready();
        let without_bundle = AppInputMethod::new(
            None,
            PathBuf::new(),
            s.sources.clone(),
            s.link.clone(),
            s.host.clone(),
            short(),
        );
        assert!(without_bundle.set_up().is_err());
        assert_eq!(s.host.settings_opened(), 0);
    }

    // --- startup ---

    #[test]
    fn at_start_an_outdated_copy_is_updated() {
        let s = setup(true, true, false);
        fs::write(s.installed.join("Contents/Info.plist"), b"old").unwrap();
        assert_eq!(s.im.update_installed_copy(), Ok(true));
        assert!(!crate::input_method::platform::install::differs(
            &s.bundled.path,
            &s.installed,
            &s.bundled.executable
        ));
        assert_eq!(s.sources.registered(), vec![s.installed.clone()]);
        assert_eq!(s.host.settings_opened(), 0, "no settings at start");
    }

    #[test]
    fn at_start_the_same_copy_stays() {
        let s = ready();
        assert_eq!(s.im.update_installed_copy(), Ok(false));
        assert!(s.sources.registered().is_empty());
    }

    #[test]
    fn at_start_nothing_is_installed_where_no_copy_is() {
        let s = setup(false, false, false);
        assert_eq!(s.im.update_installed_copy(), Ok(false));
        assert!(!s.installed.exists());
        assert!(s.sources.registered().is_empty());
    }

    #[test]
    fn a_selection_left_over_after_a_crash_is_undone() {
        let s = ready();
        s.sources.user_selects(OURS);
        s.sources
            .state
            .lock()
            .unwrap()
            .added
            .push("com.apple.keylayout.ABC".into());
        assert!(s.im.leave_leftover_selection());
        assert_eq!(s.sources.current(), "com.apple.keylayout.ABC");

        let s = ready();
        assert!(!s.im.leave_leftover_selection());
        assert!(s.sources.selected().is_empty());
    }

    /// Starts `s`'s input method as the app does at startup and waits for
    /// the work after installing it. Whether `install` had been called when
    /// it returned, and whether the copy was still outdated then.
    fn install_then_prepare(s: &Setup) -> Option<bool> {
        let seen: Arc<Mutex<Option<bool>>> = Arc::default();
        let prepared = {
            let seen = Arc::clone(&seen);
            let (bundled, installed) = (s.bundled.clone(), s.installed.clone());
            Arc::clone(&s.im).install_then_prepare(move |input_method| {
                assert!(input_method.status().available);
                *seen.lock().unwrap() = Some(crate::input_method::platform::install::differs(
                    &bundled.path,
                    &installed,
                    &bundled.executable,
                ));
            })
        };
        let installed_at_once = *seen.lock().unwrap();
        prepared.expect("a thread").join().expect("it finished");
        installed_at_once
    }

    #[test]
    fn at_start_it_is_installed_at_once_and_the_copy_updated_afterwards() {
        // An outdated copy whose input method doesn't quit in time: the
        // update waits for it, but the settings may ask right away.
        let s = setup(true, true, true);
        s.link.state.lock().unwrap().quit_disconnects = false;
        fs::write(s.installed.join("Contents/MacOS/DanyWhereInput"), b"old").unwrap();
        let installed_at_once = install_then_prepare(&s);
        assert_eq!(
            installed_at_once,
            Some(true),
            "installed before it returned, with the copy not yet updated"
        );
        assert!(!crate::input_method::platform::install::differs(
            &s.bundled.path,
            &s.installed,
            &s.bundled.executable
        ));
        assert_eq!(s.host.terminated(), vec![OURS.to_string()]);
    }

    #[test]
    fn at_start_a_leftover_selection_is_undone_after_installing() {
        let s = ready();
        s.sources.user_selects(OURS);
        s.sources
            .state
            .lock()
            .unwrap()
            .added
            .push("com.apple.keylayout.ABC".into());
        let installed_at_once = install_then_prepare(&s);
        assert!(installed_at_once.is_some());
        assert_eq!(s.sources.current(), "com.apple.keylayout.ABC");
    }

    #[test]
    fn a_selection_a_running_dictation_holds_is_no_leftover() {
        // The app installs its input method before this runs, so a
        // dictation may already hold the source, even one the user had
        // selected by hand.
        for chosen_by_hand in [false, true] {
            let s = ready();
            s.sources
                .state
                .lock()
                .unwrap()
                .added
                .push("com.apple.keylayout.ABC".into());
            if chosen_by_hand {
                s.sources.user_selects(OURS);
            }
            s.im.begin();
            assert!(!s.im.leave_leftover_selection(), "by hand {chosen_by_hand}");
            assert_eq!(s.sources.current(), OURS, "by hand {chosen_by_hand}");
        }
    }
}
