//! DanyWhere: installing and updating the copy of the input method in
//! `~/Library/Input Methods` (PROTOCOL.md, "Installing and updating"), and
//! what that needs of the system: ending the running input method, opening
//! System Settings.

use std::ffi::CString;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::{Duration, Instant};

/// The extended attribute Gatekeeper's quarantine uses.
pub const QUARANTINE: &str = "com.apple.quarantine";

/// What installing needs of the system (a fake in tests).
pub trait Host: Send + Sync {
    /// Ends the running processes with this bundle id.
    fn terminate(&self, bundle_id: &str);
    /// Asks a running System Settings to quit: it holds its list of input
    /// sources in memory, so only a new start reads them anew.
    fn quit_settings(&self) {}
    /// Removes System Settings' own cache of input sources
    /// ([`forget_keyboard_settings_cache`]).
    fn forget_settings_cache(&self) {}
    /// Opens System Settings at Keyboard, where the user adds the source.
    fn open_settings(&self) -> Result<(), String>;
}

/// The files of System Settings' own cache of input sources (its Keyboard
/// page), under the per-user cache folder. System Settings doesn't renew it
/// when an input method is registered, so a new source stays missing under
/// "+" until the cache is gone (found 2026-11-24, on macOS 27).
pub const KEYBOARD_SETTINGS_CACHE: [&str; 2] = [
    "com.apple.Keyboard-Settings.extension/com.apple.IntlDataCache.le",
    "com.apple.Keyboard-Settings.extension/com.apple.IntlDataCache.le.kbdx",
];

/// Removes [`KEYBOARD_SETTINGS_CACHE`] under `cache_dir` (the per-user cache
/// folder), so System Settings builds it anew and lists DanyWhere's source.
/// Returns how many files went; a missing file is no error.
pub fn forget_keyboard_settings_cache(cache_dir: &Path) -> io::Result<usize> {
    let mut removed = 0;
    for name in KEYBOARD_SETTINGS_CACHE {
        match fs::remove_file(cache_dir.join(name)) {
            Ok(()) => removed += 1,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(removed)
}

/// System Settings' Keyboard page: macOS 13 and later name it differently.
pub fn settings_url(macos_major: u64) -> &'static str {
    if macos_major >= 13 {
        "x-apple.systempreferences:com.apple.Keyboard-Settings.extension"
    } else {
        "x-apple.systempreferences:com.apple.preference.keyboard?InputSources"
    }
}

/// Whether the installed copy differs from the bundled input method: its
/// executable or its Info.plist (byte for byte), or it is missing.
pub fn differs(bundled: &Path, installed: &Path, executable: &str) -> bool {
    let same = |relative: &Path| match (
        fs::read(bundled.join(relative)),
        fs::read(installed.join(relative)),
    ) {
        (Ok(ours), Ok(theirs)) => ours == theirs,
        _ => false,
    };
    !(same(&Path::new("Contents/MacOS").join(executable)) && same(Path::new("Contents/Info.plist")))
}

/// Copies `bundled` to `installed`: into a temporary name next to it, then
/// in place of an old copy, without Gatekeeper's quarantine. Creates the
/// folder if needed and removes what an interrupted copy left behind.
pub fn copy_into_place(bundled: &Path, installed: &Path) -> io::Result<()> {
    let folder = installed
        .parent()
        .ok_or_else(|| io::Error::other("the copy's path has no folder"))?;
    let name = installed
        .file_name()
        .ok_or_else(|| io::Error::other("the copy's path has no name"))?
        .to_string_lossy()
        .into_owned();
    fs::create_dir_all(folder)?;
    remove_leftovers(folder, &name);

    // Names that don't end in `.app`, so macOS never takes them for one.
    let pid = std::process::id();
    let partial = folder.join(format!(".{name}.partial-{pid}"));
    let old = folder.join(format!(".{name}.old-{pid}"));
    if let Err(e) = copy_tree(bundled, &partial).and_then(|()| remove_quarantine(&partial)) {
        let _ = remove_any(&partial);
        return Err(e);
    }
    let had_old = fs::symlink_metadata(installed).is_ok();
    if had_old {
        if let Err(e) = fs::rename(installed, &old) {
            let _ = remove_any(&partial);
            return Err(e);
        }
    }
    if let Err(e) = fs::rename(&partial, installed) {
        if had_old {
            let _ = fs::rename(&old, installed);
        }
        let _ = remove_any(&partial);
        return Err(e);
    }
    if had_old {
        if let Err(e) = remove_any(&old) {
            log::warn!("Input method: the old copy wasn't removed: {e}");
        }
    }
    Ok(())
}

/// Removes what an interrupted copy of `name` left in `folder`.
fn remove_leftovers(folder: &Path, name: &str) {
    let partial = format!(".{name}.partial-");
    let old = format!(".{name}.old-");
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if file_name.starts_with(&partial) || file_name.starts_with(&old) {
            if let Err(e) = remove_any(&entry.path()) {
                log::warn!("Input method: a leftover copy wasn't removed: {e}");
            }
        }
    }
}

fn remove_any(path: &Path) -> io::Result<()> {
    if fs::symlink_metadata(path)?.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Copies files (with their permissions), folders and symbolic links.
fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(from)?;
    let kind = meta.file_type();
    if kind.is_symlink() {
        std::os::unix::fs::symlink(fs::read_link(from)?, to)
    } else if kind.is_dir() {
        fs::create_dir(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
        fs::set_permissions(to, meta.permissions())
    } else {
        fs::copy(from, to).map(|_| ())
    }
}

/// Removes the quarantine attribute from `path` and everything in it.
pub fn remove_quarantine(path: &Path) -> io::Result<()> {
    remove_xattr(path, QUARANTINE)?;
    if fs::symlink_metadata(path)?.is_dir() {
        for entry in fs::read_dir(path)? {
            remove_quarantine(&entry?.path())?;
        }
    }
    Ok(())
}

fn remove_xattr(path: &Path, name: &str) -> io::Result<()> {
    let path_c = CString::new(path.as_os_str().as_bytes())?;
    let name_c = CString::new(name)?;
    // SAFETY: two valid NUL-terminated strings.
    let result =
        unsafe { libc::removexattr(path_c.as_ptr(), name_c.as_ptr(), libc::XATTR_NOFOLLOW) };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ENOATTR) {
        Ok(())
    } else {
        Err(error)
    }
}

/// How long a running input method gets to quit before it is forced.
const TERMINATE_WAIT: Duration = Duration::from_secs(1);

/// System Settings (System Preferences before macOS 13).
const SYSTEM_SETTINGS_ID: &str = "com.apple.systempreferences";

/// How long a running System Settings gets to quit before it opens anew.
const SETTINGS_QUIT_WAIT: Duration = Duration::from_secs(2);

/// The real system.
pub struct MacHost;

impl Host for MacHost {
    fn terminate(&self, bundle_id: &str) {
        use objc2_app_kit::NSRunningApplication;
        use objc2_foundation::NSString;

        let running: Vec<_> = NSRunningApplication::runningApplicationsWithBundleIdentifier(
            &NSString::from_str(bundle_id),
        )
        .iter()
        .collect();
        if running.is_empty() {
            log::debug!("Input method: no running process of {bundle_id}");
            return;
        }
        log::info!(
            "Input method: asking {} running process(es) of {bundle_id} to quit",
            running.len()
        );
        for app in &running {
            app.terminate();
        }
        let alive = |app: &NSRunningApplication| {
            let pid = app.processIdentifier();
            // SAFETY: signal 0 only checks that the process exists.
            pid > 0 && unsafe { libc::kill(pid, 0) } == 0
        };
        let deadline = Instant::now() + TERMINATE_WAIT;
        while running.iter().any(|app| alive(app)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        for app in running.iter().filter(|app| alive(app)) {
            log::info!(
                "Input method: forcing pid {} to quit",
                app.processIdentifier()
            );
            app.forceTerminate();
        }
    }

    fn quit_settings(&self) {
        use objc2_app_kit::NSRunningApplication;
        use objc2_foundation::NSString;

        let running: Vec<_> = NSRunningApplication::runningApplicationsWithBundleIdentifier(
            &NSString::from_str(SYSTEM_SETTINGS_ID),
        )
        .iter()
        .collect();
        if running.is_empty() {
            return;
        }
        log::info!("Input method: asking System Settings to quit, so it reads the sources anew");
        for app in &running {
            app.terminate();
        }
        // Only asked, never forced; it opens again either way.
        let deadline = Instant::now() + SETTINGS_QUIT_WAIT;
        while running.iter().any(|app| !app.isTerminated()) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn forget_settings_cache(&self) {
        let Some(cache_dir) = super::paths::darwin_user_cache_dir() else {
            log::warn!("Input method: no per-user cache folder; System Settings keeps its cache");
            return;
        };
        match forget_keyboard_settings_cache(&cache_dir) {
            Ok(removed) => log::info!(
                "Input method: removed {removed} file(s) of System Settings' cache of input sources"
            ),
            Err(e) => log::warn!(
                "Input method: System Settings' cache of input sources wasn't removed: {e}"
            ),
        }
    }

    fn open_settings(&self) -> Result<(), String> {
        let version = objc2_foundation::NSProcessInfo::processInfo().operatingSystemVersion();
        let url = settings_url(version.majorVersion as u64);
        log::info!("Input method: opening System Settings at {url}");
        let status = std::process::Command::new("/usr/bin/open")
            .arg(url)
            .status()
            .map_err(|e| format!("Couldn't open System Settings: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("Couldn't open System Settings ({status})"))
        }
    }
}

#[cfg(test)]
pub mod testing {
    use super::Host;
    use std::sync::Mutex;

    /// The system, played by the test: records terminations, openings and,
    /// in order, what was done to System Settings.
    #[derive(Default)]
    pub struct FakeHost {
        pub terminated: Mutex<Vec<String>>,
        pub settings_opened: Mutex<usize>,
        pub settings_steps: Mutex<Vec<&'static str>>,
    }

    impl FakeHost {
        pub fn terminated(&self) -> Vec<String> {
            self.terminated.lock().unwrap().clone()
        }

        pub fn settings_opened(&self) -> usize {
            *self.settings_opened.lock().unwrap()
        }

        /// "forget cache" and "open", in the order they happened.
        pub fn settings_steps(&self) -> Vec<&'static str> {
            self.settings_steps.lock().unwrap().clone()
        }
    }

    impl Host for FakeHost {
        fn terminate(&self, bundle_id: &str) {
            self.terminated.lock().unwrap().push(bundle_id.into());
        }

        fn quit_settings(&self) {
            self.settings_steps.lock().unwrap().push("quit");
        }

        fn forget_settings_cache(&self) {
            self.settings_steps.lock().unwrap().push("forget cache");
        }

        fn open_settings(&self) -> Result<(), String> {
            *self.settings_opened.lock().unwrap() += 1;
            self.settings_steps.lock().unwrap().push("open");
            Ok(())
        }
    }

    /// Sets an extended attribute on `path` (not following a symlink).
    pub fn set_xattr(path: &std::path::Path, name: &str, value: &[u8]) {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let p = CString::new(path.as_os_str().as_bytes()).unwrap();
        let n = CString::new(name).unwrap();
        // SAFETY: valid NUL-terminated strings and a buffer of `value.len()`.
        let r = unsafe {
            libc::setxattr(
                p.as_ptr(),
                n.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                libc::XATTR_NOFOLLOW,
            )
        };
        assert_eq!(r, 0, "setxattr {path:?}");
    }

    /// Whether `path` has the extended attribute `name`.
    pub fn has_xattr(path: &std::path::Path, name: &str) -> bool {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let p = CString::new(path.as_os_str().as_bytes()).unwrap();
        let n = CString::new(name).unwrap();
        // SAFETY: a size query (null buffer) on valid NUL-terminated strings.
        let r = unsafe {
            libc::getxattr(
                p.as_ptr(),
                n.as_ptr(),
                std::ptr::null_mut(),
                0,
                0,
                libc::XATTR_NOFOLLOW,
            )
        };
        r >= 0
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{has_xattr, set_xattr};
    use super::*;
    use crate::input_method::platform::paths::testing::fake_bundle;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    const IM: &str = "com.yaccob.danywhere.inputmethod.DanyWhere";
    const NAME: &str = "DanyWhere Input.app";

    fn bundled(dir: &Path, exe: &[u8], version: &str) -> PathBuf {
        fake_bundle(
            &dir.join("Helpers"),
            NAME,
            IM,
            Some("com.yaccob.danywhere"),
            version,
            exe,
        )
    }

    fn input_methods(dir: &Path) -> PathBuf {
        dir.join("home/Library/Input Methods")
    }

    /// Every file and folder under `root`, relative, sorted.
    fn tree(root: &Path) -> Vec<String> {
        let mut out = Vec::new();
        fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                out.push(path.strip_prefix(root).unwrap().display().to_string());
                if fs::symlink_metadata(&path).unwrap().is_dir() {
                    walk(root, &path, out);
                }
            }
        }
        walk(root, root, &mut out);
        out.sort();
        out
    }

    #[test]
    fn system_settings_cache_of_input_sources_is_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let page = dir.path().join("com.apple.Keyboard-Settings.extension");
        fs::create_dir_all(&page).unwrap();
        for name in [
            "com.apple.IntlDataCache.le",
            "com.apple.IntlDataCache.le.kbdx",
            "com.apple.Keyboard-Settings.extension",
            "com.apple.scriptmanager2.le.cache",
        ] {
            fs::write(page.join(name), b"cache").unwrap();
        }
        assert_eq!(forget_keyboard_settings_cache(dir.path()).unwrap(), 2);
        assert!(!page.join("com.apple.IntlDataCache.le").exists());
        assert!(!page.join("com.apple.IntlDataCache.le.kbdx").exists());
        // Only the cache of input sources goes.
        assert!(page.join("com.apple.Keyboard-Settings.extension").exists());
        assert!(page.join("com.apple.scriptmanager2.le.cache").exists());
        // Gone already, or never there: nothing to do, no error.
        assert_eq!(forget_keyboard_settings_cache(dir.path()).unwrap(), 0);
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(forget_keyboard_settings_cache(empty.path()).unwrap(), 0);
    }

    #[test]
    fn system_settings_opens_where_sources_are_added() {
        assert_eq!(
            settings_url(13),
            "x-apple.systempreferences:com.apple.Keyboard-Settings.extension"
        );
        assert_eq!(
            settings_url(26),
            "x-apple.systempreferences:com.apple.Keyboard-Settings.extension"
        );
        assert_eq!(
            settings_url(12),
            "x-apple.systempreferences:com.apple.preference.keyboard?InputSources"
        );
        assert_eq!(
            settings_url(10),
            "x-apple.systempreferences:com.apple.preference.keyboard?InputSources"
        );
    }

    #[test]
    fn a_missing_copy_differs() {
        let dir = tempfile::tempdir().unwrap();
        let source = bundled(dir.path(), b"v1", "0.3.3");
        let installed = input_methods(dir.path()).join(NAME);
        assert!(differs(&source, &installed, "DanyWhereInput"));
    }

    #[test]
    fn the_same_executable_and_info_plist_dont_differ() {
        let dir = tempfile::tempdir().unwrap();
        let source = bundled(dir.path(), b"v1", "0.3.3");
        let installed = fake_bundle(
            &input_methods(dir.path()),
            NAME,
            IM,
            Some("com.yaccob.danywhere"),
            "0.3.3",
            b"v1",
        );
        fs::write(installed.join("Contents/Resources/icon.tiff"), b"other").unwrap();
        assert!(
            !differs(&source, &installed, "DanyWhereInput"),
            "resources don't count"
        );
    }

    #[test]
    fn another_executable_differs() {
        let dir = tempfile::tempdir().unwrap();
        let source = bundled(dir.path(), b"v2", "0.3.3");
        let installed = fake_bundle(
            &input_methods(dir.path()),
            NAME,
            IM,
            Some("com.yaccob.danywhere"),
            "0.3.3",
            b"v1",
        );
        assert!(differs(&source, &installed, "DanyWhereInput"));
    }

    #[test]
    fn another_info_plist_differs() {
        let dir = tempfile::tempdir().unwrap();
        let source = bundled(dir.path(), b"v1", "0.3.4");
        let installed = fake_bundle(
            &input_methods(dir.path()),
            NAME,
            IM,
            Some("com.yaccob.danywhere"),
            "0.3.3",
            b"v1",
        );
        assert!(differs(&source, &installed, "DanyWhereInput"));
    }

    #[test]
    fn the_copy_is_the_whole_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let source = bundled(dir.path(), b"v1", "0.3.3");
        std::os::unix::fs::symlink("icon.tiff", source.join("Contents/Resources/link.tiff"))
            .unwrap();
        let installed = input_methods(dir.path()).join(NAME);

        copy_into_place(&source, &installed).unwrap();

        assert_eq!(tree(&installed), tree(&source));
        assert!(!differs(&source, &installed, "DanyWhereInput"));
        let exe = installed.join("Contents/MacOS/DanyWhereInput");
        assert_eq!(
            fs::metadata(&exe).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert_eq!(
            fs::read_link(installed.join("Contents/Resources/link.tiff")).unwrap(),
            PathBuf::from("icon.tiff")
        );
        assert_eq!(
            fs::read_dir(input_methods(dir.path())).unwrap().count(),
            1,
            "nothing else is left in Input Methods: {:?}",
            tree(&input_methods(dir.path()))
        );
    }

    #[test]
    fn an_old_copy_is_replaced_entirely() {
        let dir = tempfile::tempdir().unwrap();
        let source = bundled(dir.path(), b"v2", "0.3.4");
        let installed = fake_bundle(
            &input_methods(dir.path()),
            NAME,
            IM,
            Some("com.yaccob.danywhere"),
            "0.3.3",
            b"v1",
        );
        fs::write(installed.join("Contents/Resources/old.txt"), b"old").unwrap();
        fs::write(input_methods(dir.path()).join("Other Input.app"), b"x").unwrap();

        copy_into_place(&source, &installed).unwrap();

        assert_eq!(tree(&installed), tree(&source));
        assert_eq!(
            fs::read(installed.join("Contents/MacOS/DanyWhereInput")).unwrap(),
            b"v2"
        );
        let mut left: Vec<String> = fs::read_dir(input_methods(dir.path()))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, vec![NAME.to_string(), "Other Input.app".to_string()]);
    }

    #[test]
    fn what_an_interrupted_copy_left_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let source = bundled(dir.path(), b"v1", "0.3.3");
        let folder = input_methods(dir.path());
        fs::create_dir_all(folder.join(format!(".{NAME}.partial-12345/Contents"))).unwrap();
        fs::create_dir_all(folder.join(format!(".{NAME}.old-12345/Contents"))).unwrap();

        copy_into_place(&source, &folder.join(NAME)).unwrap();

        let left: Vec<String> = fs::read_dir(&folder)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, vec![NAME.to_string()]);
    }

    #[test]
    fn the_copy_carries_no_quarantine() {
        let dir = tempfile::tempdir().unwrap();
        let source = bundled(dir.path(), b"v1", "0.3.3");
        let value = b"0083;00000000;Safari;";
        set_xattr(&source, QUARANTINE, value);
        set_xattr(&source.join("Contents/Info.plist"), QUARANTINE, value);
        set_xattr(
            &source.join("Contents/MacOS/DanyWhereInput"),
            QUARANTINE,
            value,
        );
        set_xattr(
            &source.join("Contents/MacOS/DanyWhereInput"),
            "com.example.keep",
            b"1",
        );
        let installed = input_methods(dir.path()).join(NAME);

        copy_into_place(&source, &installed).unwrap();

        let mut quarantined: Vec<String> = tree(&installed)
            .into_iter()
            .filter(|rel| has_xattr(&installed.join(rel), QUARANTINE))
            .collect();
        if has_xattr(&installed, QUARANTINE) {
            quarantined.push(".".into());
        }
        assert!(quarantined.is_empty(), "quarantined: {quarantined:?}");
        assert!(has_xattr(&source.join("Contents/Info.plist"), QUARANTINE));
    }

    #[test]
    fn quarantine_is_removed_recursively() {
        let dir = tempfile::tempdir().unwrap();
        let source = bundled(dir.path(), b"v1", "0.3.3");
        set_xattr(&source, QUARANTINE, b"q");
        set_xattr(
            &source.join("Contents/MacOS/DanyWhereInput"),
            QUARANTINE,
            b"q",
        );
        set_xattr(
            &source.join("Contents/MacOS/DanyWhereInput"),
            "com.example.keep",
            b"1",
        );

        remove_quarantine(&source).unwrap();

        assert!(!has_xattr(&source, QUARANTINE));
        assert!(!has_xattr(
            &source.join("Contents/MacOS/DanyWhereInput"),
            QUARANTINE
        ));
        assert!(has_xattr(
            &source.join("Contents/MacOS/DanyWhereInput"),
            "com.example.keep"
        ));
    }
}
