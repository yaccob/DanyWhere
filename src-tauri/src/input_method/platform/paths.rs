//! DanyWhere: where the input method is: the one bundled in the app, its
//! installed copy and the socket of the link (PROTOCOL.md, "Names" and "The
//! link").

use std::path::{Path, PathBuf};

/// The custom Info.plist key that names the app an input method belongs to.
pub const APP_ID_KEY: &str = "DanyWhereAppID";

/// The longest socket path `sockaddr_un` takes (104 bytes with the NUL).
pub const MAX_SOCKET_PATH: usize = 103;

/// An input method bundle, as its Info.plist describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundledInputMethod {
    /// The bundle, e.g. `…/Contents/Helpers/DanyWhere Input.app`.
    pub path: PathBuf,
    /// `CFBundleIdentifier`, which is also its input source id.
    pub id: String,
    /// `CFBundleVersion`.
    pub version: String,
    /// The bundle's file name, e.g. `DanyWhere Input.app`.
    pub file_name: String,
    /// `CFBundleExecutable`, the file in `Contents/MacOS`.
    pub executable: String,
}

/// `Contents/Helpers` of the app whose executable is `exe`
/// (`…/Contents/MacOS/<exe>`).
pub fn helpers_dir(exe: &Path) -> Option<PathBuf> {
    let contents = exe.parent()?.parent()?;
    Some(contents.join("Helpers"))
}

/// The `*.app` in `helpers` whose `DanyWhereAppID` is `app_id`.
pub fn find_bundled(helpers: &Path, app_id: &str) -> Option<BundledInputMethod> {
    let mut bundles: Vec<PathBuf> = std::fs::read_dir(helpers)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "app"))
        .collect();
    bundles.sort();
    bundles
        .into_iter()
        .find_map(|path| read_bundle(&path).filter(|(owner, _)| owner == app_id))
        .map(|(_, bundle)| bundle)
}

/// A bundle's owning app id and description, from its Info.plist.
fn read_bundle(path: &Path) -> Option<(String, BundledInputMethod)> {
    let info = plist::Value::from_file(path.join("Contents/Info.plist")).ok()?;
    let info = info.as_dictionary()?;
    let text = |key: &str| {
        info.get(key)
            .and_then(plist::Value::as_string)
            .map(str::to_string)
    };
    let owner = text(APP_ID_KEY)?;
    let bundle = BundledInputMethod {
        path: path.to_path_buf(),
        id: text("CFBundleIdentifier")?,
        version: text("CFBundleVersion").unwrap_or_default(),
        file_name: path.file_name()?.to_str()?.to_string(),
        executable: text("CFBundleExecutable")?,
    };
    Some((owner, bundle))
}

/// `~/Library/Input Methods`.
pub fn input_methods_dir(home: &Path) -> PathBuf {
    home.join("Library/Input Methods")
}

/// Where the copy of a bundle with this file name is installed.
pub fn installed_path(home: &Path, file_name: &str) -> PathBuf {
    input_methods_dir(home).join(file_name)
}

/// The link's socket: `~/Library/Application Support/<app id>/input-method.sock`,
/// or, if that is longer than [`MAX_SOCKET_PATH`] bytes,
/// `<_CS_DARWIN_USER_TEMP_DIR><app id>.input-method.sock`.
pub fn socket_path(
    home: &Path,
    app_id: &str,
    user_temp_dir: impl FnOnce() -> Option<PathBuf>,
) -> PathBuf {
    let usual = home
        .join("Library/Application Support")
        .join(app_id)
        .join("input-method.sock");
    if usual.as_os_str().len() <= MAX_SOCKET_PATH {
        return usual;
    }
    let temp = user_temp_dir().unwrap_or_else(std::env::temp_dir);
    temp.join(format!("{app_id}.input-method.sock"))
}

/// `confstr(_CS_DARWIN_USER_TEMP_DIR)`: the per-user temporary folder.
pub fn darwin_user_temp_dir() -> Option<PathBuf> {
    confstr_dir(libc::_CS_DARWIN_USER_TEMP_DIR)
}

/// A folder `confstr` names, e.g. `_CS_DARWIN_USER_TEMP_DIR`.
fn confstr_dir(name: libc::c_int) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let mut buffer = vec![0u8; 1024];
    // SAFETY: the buffer is valid for its length; confstr writes at most
    // that many bytes, NUL included.
    let needed = unsafe { libc::confstr(name, buffer.as_mut_ptr().cast(), buffer.len()) };
    if needed == 0 || needed > buffer.len() {
        return None;
    }
    buffer.truncate(needed - 1);
    Some(PathBuf::from(std::ffi::OsString::from_vec(buffer)))
}

/// `confstr(_CS_DARWIN_USER_CACHE_DIR)`: the per-user cache folder.
pub fn darwin_user_cache_dir() -> Option<PathBuf> {
    confstr_dir(libc::_CS_DARWIN_USER_CACHE_DIR)
}

#[cfg(test)]
pub mod testing {
    use plist::{Dictionary, Value};
    use std::fs;
    use std::path::{Path, PathBuf};

    /// A fake input method bundle `<dir>/<file_name>` with these Info.plist
    /// values and an executable with `exe_bytes`.
    pub fn fake_bundle(
        dir: &Path,
        file_name: &str,
        id: &str,
        app_id: Option<&str>,
        version: &str,
        exe_bytes: &[u8],
    ) -> PathBuf {
        let bundle = dir.join(file_name);
        fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        fs::create_dir_all(bundle.join("Contents/Resources")).unwrap();
        let mut info = Dictionary::new();
        info.insert("CFBundleIdentifier".into(), Value::String(id.into()));
        info.insert("CFBundleVersion".into(), Value::String(version.into()));
        info.insert(
            "CFBundleExecutable".into(),
            Value::String("DanyWhereInput".into()),
        );
        if let Some(app_id) = app_id {
            info.insert(super::APP_ID_KEY.into(), Value::String(app_id.into()));
        }
        Value::Dictionary(info)
            .to_file_xml(bundle.join("Contents/Info.plist"))
            .unwrap();
        let exe = bundle.join("Contents/MacOS/DanyWhereInput");
        fs::write(&exe, exe_bytes).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(bundle.join("Contents/Resources/icon.tiff"), b"icon").unwrap();
        bundle
    }
}

#[cfg(test)]
mod tests {
    use super::testing::fake_bundle;
    use super::*;
    use std::fs;

    const APP: &str = "com.yaccob.danywhere";
    const IM: &str = "com.yaccob.danywhere.inputmethod.DanyWhere";

    #[test]
    fn the_helpers_sit_next_to_the_executables_folder() {
        assert_eq!(
            helpers_dir(Path::new(
                "/Applications/DanyWhere.app/Contents/MacOS/danywhere"
            )),
            Some(PathBuf::from(
                "/Applications/DanyWhere.app/Contents/Helpers"
            ))
        );
    }

    #[test]
    fn the_bundled_input_method_is_the_one_for_this_app() {
        let dir = tempfile::tempdir().unwrap();
        let helpers = dir.path().join("Helpers");
        fake_bundle(
            &helpers,
            "DanyWhere Dev Input.app",
            "com.pais.handy.dev.inputmethod.DanyWhere",
            Some("com.pais.handy.dev"),
            "0.3.3",
            b"dev",
        );
        let ours = fake_bundle(
            &helpers,
            "DanyWhere Input.app",
            IM,
            Some(APP),
            "0.3.3",
            b"release",
        );
        fake_bundle(&helpers, "Other.app", "x.y", None, "1", b"other");

        let found = find_bundled(&helpers, APP).expect("found");
        assert_eq!(
            found,
            BundledInputMethod {
                path: ours,
                id: IM.into(),
                version: "0.3.3".into(),
                file_name: "DanyWhere Input.app".into(),
                executable: "DanyWhereInput".into(),
            }
        );
    }

    #[test]
    fn a_build_without_one_has_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(find_bundled(&dir.path().join("Helpers"), APP), None);
        let helpers = dir.path().join("Helpers");
        fake_bundle(
            &helpers,
            "DanyWhere Dev Input.app",
            "com.pais.handy.dev.inputmethod.DanyWhere",
            Some("com.pais.handy.dev"),
            "0.3.3",
            b"dev",
        );
        fs::write(helpers.join("notes.txt"), b"x").unwrap();
        assert_eq!(find_bundled(&helpers, APP), None);
    }

    #[test]
    fn the_copy_goes_into_input_methods() {
        let home = Path::new("/Users/someone");
        assert_eq!(
            input_methods_dir(home),
            PathBuf::from("/Users/someone/Library/Input Methods")
        );
        assert_eq!(
            installed_path(home, "DanyWhere Input.app"),
            PathBuf::from("/Users/someone/Library/Input Methods/DanyWhere Input.app")
        );
    }

    #[test]
    fn the_socket_is_in_application_support() {
        let path = socket_path(Path::new("/Users/someone"), APP, || panic!("not needed"));
        assert_eq!(
            path,
            PathBuf::from(
                "/Users/someone/Library/Application Support/com.yaccob.danywhere/input-method.sock"
            )
        );
    }

    #[test]
    fn a_socket_path_of_103_bytes_stays() {
        // "/Library/Application Support/" + id + "/input-method.sock" is 47 bytes
        // plus the id; a home of 103 - 47 - id bytes gives exactly 103.
        let fixed = "/Library/Application Support//input-method.sock".len() + APP.len();
        let home = format!("/{}", "h".repeat(MAX_SOCKET_PATH - fixed - 1));
        let path = socket_path(Path::new(&home), APP, || panic!("not needed"));
        assert_eq!(path.as_os_str().len(), MAX_SOCKET_PATH);
        assert!(path.starts_with(&home));
    }

    #[test]
    fn a_longer_one_goes_into_the_user_temp_dir() {
        let fixed = "/Library/Application Support//input-method.sock".len() + APP.len();
        let home = format!("/{}", "h".repeat(MAX_SOCKET_PATH - fixed));
        let path = socket_path(Path::new(&home), APP, || {
            Some(PathBuf::from("/var/folders/ab/xyz/T/"))
        });
        assert_eq!(
            path,
            PathBuf::from("/var/folders/ab/xyz/T/com.yaccob.danywhere.input-method.sock")
        );
    }

    #[test]
    fn the_user_temp_dir_is_known() {
        let dir = darwin_user_temp_dir().expect("confstr answers");
        assert!(dir.is_absolute(), "{dir:?}");
        assert!(dir.is_dir(), "{dir:?}");
    }

    #[test]
    fn the_user_cache_dir_is_known() {
        let dir = darwin_user_cache_dir().expect("confstr answers");
        assert!(dir.is_absolute(), "{dir:?}");
        assert!(dir.is_dir(), "{dir:?}");
        assert_ne!(
            Some(dir),
            darwin_user_temp_dir(),
            "the cache, not the temp folder"
        );
    }
}
