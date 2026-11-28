//! DanyWhere: macOS's input sources (Text Input Sources, TIS), as far as the
//! input method needs them: whether its source is added, which source and
//! keyboard layout are current, selecting one, registering a bundle.

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use tauri::AppHandle;

/// What the input method asks of macOS's input sources (a fake in tests).
pub trait InputSources: Send + Sync {
    /// The source with this id is added (enabled) in System Settings.
    fn is_added(&self, id: &str) -> bool;
    /// The id of the current keyboard input source (a layout or an input
    /// method).
    fn current_source(&self) -> Option<String>;
    /// The id of the keyboard layout in use.
    fn current_layout(&self) -> Option<String>;
    /// Selects the added source with this id.
    fn select(&self, id: &str) -> Result<(), String>;
    /// Makes the bundle at `path` known to macOS (`TISRegisterInputSource`).
    fn register(&self, path: &Path) -> Result<(), String>;
    /// The id of the ASCII-capable keyboard layout macOS would use now.
    fn ascii_capable_layout(&self) -> Option<String>;
    /// Makes the application in front hand its focused text field to the
    /// current input source again, by taking the activation from it for a
    /// moment and giving it straight back. macOS doesn't always connect the
    /// focused field to a source a program selected: in Safari and TextEdit
    /// a field that had had the input method once often got no
    /// `activateServer` again until a key was pressed, while an activation
    /// of its application brought it every time (2026-11-24, macOS 27).
    /// Returns whether the activation went back to that application.
    fn reconnect_focused_field(&self) -> bool;
}

/// How long a caller waits for the main thread to run a TIS call.
const MAIN_THREAD_WAIT: Duration = Duration::from_secs(2);

/// The real input sources: Carbon's Text Input Sources, called on the main
/// thread (TIS requires it). A caller on another thread waits for the main
/// thread's answer, at most [`MAIN_THREAD_WAIT`]; on the main thread itself
/// the call runs inline.
pub struct CarbonInputSources {
    app: AppHandle,
}

impl CarbonInputSources {
    pub fn new(app: AppHandle) -> Self {
        CarbonInputSources { app }
    }

    fn on_main<T: Send + 'static>(
        &self,
        what: &str,
        call: impl FnOnce() -> T + Send + 'static,
    ) -> Option<T> {
        let (tx, rx) = mpsc::sync_channel(1);
        if let Err(e) = self.app.run_on_main_thread(move || {
            let _ = tx.send(call());
        }) {
            log::warn!("Input sources: {what} not run on the main thread: {e}");
            return None;
        }
        match rx.recv_timeout(MAIN_THREAD_WAIT) {
            Ok(answer) => Some(answer),
            Err(_) => {
                log::warn!("Input sources: the main thread didn't answer {what} in time");
                None
            }
        }
    }
}

impl InputSources for CarbonInputSources {
    fn is_added(&self, id: &str) -> bool {
        let id = id.to_string();
        self.on_main("is_added", move || tis::is_added(&id))
            .unwrap_or(false)
    }

    fn current_source(&self) -> Option<String> {
        self.on_main("current_source", tis::current_source)
            .flatten()
    }

    fn current_layout(&self) -> Option<String> {
        self.on_main("current_layout", tis::current_layout)
            .flatten()
    }

    fn select(&self, id: &str) -> Result<(), String> {
        let id = id.to_string();
        self.on_main("select", move || tis::select(&id))
            .unwrap_or_else(|| Err("the main thread didn't answer".into()))
    }

    fn register(&self, path: &Path) -> Result<(), String> {
        let path = path.to_path_buf();
        self.on_main("register", move || tis::register(&path))
            .unwrap_or_else(|| Err("the main thread didn't answer".into()))
    }

    fn ascii_capable_layout(&self) -> Option<String> {
        self.on_main("ascii_capable_layout", tis::ascii_capable_layout)
            .flatten()
    }

    fn reconnect_focused_field(&self) -> bool {
        let Some(front) = self
            .on_main("activate DanyWhere", activation::take_from_front)
            .flatten()
        else {
            return false;
        };
        // Off the main thread, so that it handles the activation meanwhile;
        // 120 ms brought the field back every time in the trial.
        std::thread::sleep(activation::HOLD);
        self.on_main("give the activation back", move || {
            activation::give_back(front)
        })
        .unwrap_or(false)
    }
}

/// Taking the activation from the application in front and giving it back
/// ([`InputSources::reconnect_focused_field`]); main thread only. While its
/// window is hidden DanyWhere is an accessory app (no Dock icon, no menu
/// bar), so all that shows is the front window losing its focus for a
/// moment.
mod activation {
    use std::time::Duration;

    use objc2::MainThreadMarker;
    use objc2_app_kit::NSWorkspace;
    use objc2_app_kit::{NSApplication, NSApplicationActivationOptions, NSRunningApplication};

    /// How long DanyWhere keeps the activation.
    pub const HOLD: Duration = Duration::from_millis(120);

    /// Activates DanyWhere and returns the pid of the application that was
    /// in front, or `None` when that is DanyWhere itself or unknown.
    pub fn take_from_front() -> Option<i32> {
        let mtm = MainThreadMarker::new()?;
        let front = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        let pid = front.processIdentifier();
        if pid <= 0 || pid as u32 == std::process::id() {
            return None;
        }
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
        Some(pid)
    }

    /// Activates the application with `pid` again.
    pub fn give_back(pid: i32) -> bool {
        let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) else {
            log::warn!("Input method: the application in front (pid {pid}) is gone");
            return false;
        };
        app.activateWithOptions(NSApplicationActivationOptions::empty())
    }
}

/// The Carbon calls themselves; main thread only.
mod tis {
    use std::ffi::c_void;
    use std::path::Path;

    use super::super::cf::{
        self, Boolean, CFArrayRef, CFDictionaryRef, CFStringRef, CFURLRef, Owned,
    };

    type TISInputSourceRef = *const c_void;
    type OSStatus = i32;

    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        fn TISCopyCurrentKeyboardInputSource() -> TISInputSourceRef;
        fn TISCopyCurrentKeyboardLayoutInputSource() -> TISInputSourceRef;
        fn TISCopyCurrentASCIICapableKeyboardLayoutInputSource() -> TISInputSourceRef;
        fn TISGetInputSourceProperty(source: TISInputSourceRef, key: CFStringRef) -> *const c_void;
        fn TISCreateInputSourceList(
            properties: CFDictionaryRef,
            include_all_installed: Boolean,
        ) -> CFArrayRef;
        fn TISSelectInputSource(source: TISInputSourceRef) -> OSStatus;
        fn TISRegisterInputSource(location: CFURLRef) -> OSStatus;
        static kTISPropertyInputSourceID: CFStringRef;
    }

    /// The id of a source someone else owns.
    fn id_of(source: TISInputSourceRef) -> Option<String> {
        // SAFETY: a valid source; the property is a CFString it owns.
        unsafe { cf::to_string(TISGetInputSourceProperty(source, kTISPropertyInputSourceID)) }
    }

    /// The id of the source a TISCopy… function returns.
    fn copied_id(copy: unsafe extern "C" fn() -> TISInputSourceRef) -> Option<String> {
        // SAFETY: the function follows the Copy rule; `Owned` releases.
        let source = unsafe { Owned::take(copy()) }?;
        id_of(source.as_ptr())
    }

    /// The enabled sources with this id (`TISCreateInputSourceList` lists
    /// only enabled ones without `include_all_installed`).
    fn enabled_with_id(id: &str) -> Option<Owned> {
        let value = cf::string(id)?;
        // SAFETY: the key is a constant of Carbon.
        let key = unsafe { kTISPropertyInputSourceID };
        let properties = cf::dictionary(&[(key, value.as_ptr())])?;
        // SAFETY: a valid dictionary; the list follows the Create rule.
        unsafe { Owned::take(TISCreateInputSourceList(properties.as_ptr(), 0)) }
    }

    pub fn is_added(id: &str) -> bool {
        enabled_with_id(id)
            // SAFETY: a valid array owned by `list`.
            .map(|list| !unsafe { cf::array_items(list.as_ptr()) }.is_empty())
            .unwrap_or(false)
    }

    pub fn current_source() -> Option<String> {
        copied_id(TISCopyCurrentKeyboardInputSource)
    }

    pub fn current_layout() -> Option<String> {
        copied_id(TISCopyCurrentKeyboardLayoutInputSource)
    }

    pub fn ascii_capable_layout() -> Option<String> {
        copied_id(TISCopyCurrentASCIICapableKeyboardLayoutInputSource)
    }

    pub fn select(id: &str) -> Result<(), String> {
        let list = enabled_with_id(id).ok_or_else(|| format!("{id} isn't added"))?;
        // SAFETY: a valid array owned by `list`, its items alive with it.
        let source = unsafe { cf::array_items(list.as_ptr()) }
            .first()
            .copied()
            .ok_or_else(|| format!("{id} isn't added"))?;
        // SAFETY: a valid source.
        let status = unsafe { TISSelectInputSource(source) };
        if status == 0 {
            Ok(())
        } else {
            Err(format!(
                "TISSelectInputSource({id}) failed: OSStatus {status}"
            ))
        }
    }

    pub fn register(path: &Path) -> Result<(), String> {
        let url = cf::file_url(path, true).ok_or("the bundle's path couldn't be passed on")?;
        // SAFETY: a valid file URL.
        let status = unsafe { TISRegisterInputSource(url.as_ptr()) };
        if status == 0 {
            log::info!("Input sources: registered {}", path.display());
            Ok(())
        } else {
            Err(format!("TISRegisterInputSource failed: OSStatus {status}"))
        }
    }
}

#[cfg(test)]
pub mod testing {
    use super::InputSources;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    type OnReconnect = Box<dyn Fn() + Send + Sync>;
    type OnSelect = Box<dyn Fn(&str) + Send + Sync>;

    /// macOS's input sources, played by the test: a list of added sources,
    /// the current one and every selection and registration.
    #[derive(Default)]
    pub struct FakeSources {
        pub state: Mutex<FakeState>,
        /// What the application in front does when its activation goes
        /// round (`reconnect_focused_field`), e.g. a text field reports.
        pub on_reconnect: Mutex<Option<OnReconnect>>,
        /// What macOS does after a source was selected, e.g. starting the
        /// input method.
        pub on_select: Mutex<Option<OnSelect>>,
    }

    #[derive(Default)]
    pub struct FakeState {
        pub added: Vec<String>,
        pub current: String,
        /// The layout in use: the last keyboard layout selected.
        pub layout: String,
        pub ascii: String,
        pub selected: Vec<String>,
        pub registered: Vec<PathBuf>,
        pub select_fails: bool,
        /// The current source and layout can't be read, as when the main
        /// thread doesn't answer in time.
        pub current_unknown: bool,
        /// How often the activation went round (`reconnect_focused_field`).
        pub reconnects: usize,
    }

    impl FakeSources {
        /// `added` sources, `current` (a keyboard layout) the current one.
        pub fn new(added: &[&str], current: &str) -> Self {
            FakeSources {
                state: Mutex::new(FakeState {
                    added: added.iter().map(|id| id.to_string()).collect(),
                    current: current.into(),
                    layout: current.into(),
                    ascii: "com.apple.keylayout.ABC".into(),
                    ..FakeState::default()
                }),
                on_reconnect: Mutex::new(None),
                on_select: Mutex::new(None),
            }
        }

        /// The user picks `id` in the input menu.
        pub fn user_selects(&self, id: &str) {
            let mut state = self.state.lock().unwrap();
            state.current = id.into();
            if id.contains(".keylayout.") {
                state.layout = id.into();
            }
        }

        pub fn selected(&self) -> Vec<String> {
            self.state.lock().unwrap().selected.clone()
        }

        pub fn current(&self) -> String {
            self.state.lock().unwrap().current.clone()
        }

        pub fn registered(&self) -> Vec<PathBuf> {
            self.state.lock().unwrap().registered.clone()
        }

        /// From now on, when the activation goes round, `then` runs.
        pub fn when_reconnected(&self, then: impl Fn() + Send + Sync + 'static) {
            *self.on_reconnect.lock().unwrap() = Some(Box::new(then));
        }

        pub fn reconnects(&self) -> usize {
            self.state.lock().unwrap().reconnects
        }

        /// From now on, after each selection, `then` runs with its id.
        pub fn when_selected(&self, then: impl Fn(&str) + Send + Sync + 'static) {
            *self.on_select.lock().unwrap() = Some(Box::new(then));
        }
    }

    impl InputSources for FakeSources {
        fn is_added(&self, id: &str) -> bool {
            self.state.lock().unwrap().added.iter().any(|a| a == id)
        }

        fn current_source(&self) -> Option<String> {
            let state = self.state.lock().unwrap();
            (!state.current_unknown).then(|| state.current.clone())
        }

        fn current_layout(&self) -> Option<String> {
            let state = self.state.lock().unwrap();
            (!state.current_unknown).then(|| state.layout.clone())
        }

        fn select(&self, id: &str) -> Result<(), String> {
            {
                let mut state = self.state.lock().unwrap();
                if state.select_fails || !state.added.iter().any(|a| a == id) {
                    return Err(format!("{id} can't be selected"));
                }
                state.selected.push(id.into());
                state.current = id.into();
                if id.contains(".keylayout.") {
                    state.layout = id.into();
                }
            }
            if let Some(then) = self.on_select.lock().unwrap().as_ref() {
                then(id);
            }
            Ok(())
        }

        fn register(&self, path: &Path) -> Result<(), String> {
            self.state.lock().unwrap().registered.push(path.into());
            Ok(())
        }

        fn ascii_capable_layout(&self) -> Option<String> {
            Some(self.state.lock().unwrap().ascii.clone())
        }

        fn reconnect_focused_field(&self) -> bool {
            self.state.lock().unwrap().reconnects += 1;
            if let Some(then) = self.on_reconnect.lock().unwrap().as_ref() {
                then();
            }
            true
        }
    }
}
