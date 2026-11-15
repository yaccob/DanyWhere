//! DanyWhere: the user's theme file, `theme.css` next to `settings_store.json`.
//!
//! The built-in palette is compiled into the frontend bundle, so changing a
//! colour there needs a rebuild. This file lets the user restyle the app
//! without one: every webview loads it as a stylesheet after the built-in CSS
//! (src/lib/userTheme.ts) and reloads it when it is saved.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, SystemTime};

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

/// The theme file's name in the app's data folder.
pub const FILE_NAME: &str = "theme.css";

/// The event that carries the file's CSS (or `null`) to every webview after
/// it was saved or deleted; src/lib/loadUserTheme.ts listens for it.
pub const CHANGED_EVENT: &str = "user-theme-changed";

/// How often the file's size and modification time are checked. A webview
/// also reloads the file when its window gets the focus. No file watcher:
/// none of the dependencies brings one, and a stat call every two seconds
/// costs nothing next to a new crate.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// The built-in stylesheets whose tokens the template lists, in this order,
/// each with the comment that heads its part.
const BUILT_IN_SOURCES: &[(&str, &str)] = &[
    (
        "The app's colors, shadows and the onboarding's drawings",
        include_str!("../../src/styles/theme.css"),
    ),
    (
        "The wordmark at the top of the sidebar",
        include_str!("../../src/styles/danywhere-wordmark.css"),
    ),
];

const HEADER: &str = "\
/*
 * DanyWhere theme file
 *
 * This file restyles DanyWhere without a rebuild. It is plain CSS that every
 * window of the app loads after its built-in styles, so it can override the
 * variables below and add or change any other rule: any CSS works. When you
 * save it, every window reloads it within about two seconds, and at once
 * when you switch back to the app.
 *
 * Every variable below is commented out and shows its built-in default. To
 * change one, remove the comment markers around it and edit the value.
 * Variables you leave commented out keep following the app, so an update
 * can still change their defaults. The app never overwrites this file.
 *
 * A color that differs between the light and the dark theme comes as a
 * pair of variables, one starting with --light- and one with --dark-.
 * Change the pair: the --color- variable the app draws with picks the right
 * one by itself. A variable without a pair applies to both themes.
 *
 * A rule the browser engine can't read is skipped; the rest still applies.
 *
 * To go back to the built-in styles, delete this file. The app then runs
 * with its built-in styles and writes a fresh copy the next time it starts.
 */
";

/// The file that ships when it is missing: the header, then every token of
/// the built-in stylesheets with its default, commented out. Generated from
/// the stylesheets themselves, so it can't drift from them.
pub fn template() -> String {
    let mut out = String::from(HEADER);
    out.push_str("\n:root {\n");
    for (index, (heading, css)) in BUILT_IN_SOURCES.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        out.push_str(&format!("  /* {heading} */\n"));
        for (position, declaration) in root_declarations(css).iter().enumerate() {
            if declaration.picks_active_theme() {
                continue;
            }
            if declaration.blank_before && position > 0 {
                out.push('\n');
            }
            out.push_str(&format!("  /* {} */\n", declaration.text));
        }
    }
    out.push_str("}\n");
    out
}

/// A custom property of a stylesheet's first `:root` block.
struct Declaration {
    name: String,
    /// The declaration as written, `;` included, without comments.
    text: String,
    /// Whether an empty line sets it apart from the one before.
    blank_before: bool,
}

impl Declaration {
    /// `--color-text: var(--light-color-text)`: the line that picks the
    /// active theme's value. The template leaves it out, since overriding
    /// it would pin the app to one theme.
    fn picks_active_theme(&self) -> bool {
        let value = self.text[self.name.len()..]
            .trim_start_matches(':')
            .trim()
            .trim_end_matches(';')
            .trim();
        value == format!("var(--light-{})", &self.name[2..])
    }
}

/// The custom properties of the first `:root` block of `css`, in order.
fn root_declarations(css: &str) -> Vec<Declaration> {
    // The text with every comment blanked out (newlines kept), so offsets
    // match the original.
    let mut masked = String::with_capacity(css.len());
    let mut in_comment = false;
    let mut chars = css.chars().peekable();
    while let Some(c) = chars.next() {
        if !in_comment && c == '/' && chars.peek() == Some(&'*') {
            in_comment = true;
            chars.next();
            masked.push_str("  ");
        } else if in_comment && c == '*' && chars.peek() == Some(&'/') {
            in_comment = false;
            chars.next();
            masked.push_str("  ");
        } else if in_comment && c != '\n' {
            masked.extend(std::iter::repeat_n(' ', c.len_utf8()));
        } else {
            masked.push(c);
        }
    }

    let Some(root) = masked.find(":root") else {
        return Vec::new();
    };
    let Some(open) = masked[root..].find('{').map(|i| root + i) else {
        return Vec::new();
    };
    let mut depth = 0usize;
    let mut found = Vec::new();
    let mut segment_start = open + 1;
    for (i, c) in masked[open..].char_indices() {
        let at = open + i;
        match c {
            '{' | '(' => depth += 1,
            '}' | ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
            }
            ';' if depth == 1 => {
                let segment = &masked[segment_start..at];
                if let Some(offset) = segment.find("--") {
                    let start = segment_start + offset;
                    let text = masked[start..=at]
                        .lines()
                        .map(str::trim_end)
                        .collect::<Vec<_>>()
                        .join("\n");
                    let name = text
                        .split(':')
                        .next()
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    // An empty line in the original gap, not one left by a
                    // blanked-out comment.
                    let gap: Vec<&str> = css[segment_start..start].split('\n').collect();
                    let blank_before =
                        gap.len() > 2 && gap[1..gap.len() - 1].iter().any(|l| l.trim().is_empty());
                    found.push(Declaration {
                        name,
                        text,
                        blank_before,
                    });
                }
                segment_start = at + 1;
            }
            _ => {}
        }
    }
    found
}

/// Creates the theme file from the template when it is missing, along with
/// its folder. Never touches an existing file. Returns whether it wrote one.
pub fn ensure_exists(path: &Path) -> io::Result<bool> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            file.write_all(template().as_bytes())?;
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error),
    }
}

/// What reading the theme file found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOutcome {
    /// The file's CSS.
    Css(String),
    /// No file: the app runs with the built-in styles.
    Missing,
    /// The file is there but can't be read, with the reason.
    Unreadable(String),
}

/// Reads the theme file. A stray byte that isn't UTF-8 becomes U+FFFD
/// instead of losing the whole file.
pub fn read(path: &Path) -> ReadOutcome {
    match fs::read(path) {
        Ok(bytes) => ReadOutcome::Css(String::from_utf8_lossy(&bytes).into_owned()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => ReadOutcome::Missing,
        Err(error) => ReadOutcome::Unreadable(error.to_string()),
    }
}

/// Turns a read into the CSS the webviews get, logging a read error once
/// until the file can be read (or is gone) again.
#[derive(Debug, Default)]
pub struct ErrorOnce {
    last: Option<String>,
    /// How many warnings were logged; for the tests.
    #[cfg_attr(not(test), allow(dead_code))]
    logged: usize,
}

impl ErrorOnce {
    /// The CSS to apply, or `None` for the built-in styles alone.
    pub fn css(&mut self, path: &Path, outcome: ReadOutcome) -> Option<String> {
        match outcome {
            ReadOutcome::Css(css) => {
                self.last = None;
                Some(css)
            }
            ReadOutcome::Missing => {
                self.last = None;
                None
            }
            ReadOutcome::Unreadable(reason) => {
                if self.last.as_deref() != Some(reason.as_str()) {
                    log::warn!(
                        "Can't read the theme file {}, using the built-in styles: {reason}",
                        path.display()
                    );
                    self.logged += 1;
                    self.last = Some(reason);
                }
                None
            }
        }
    }
}

/// What the cheap change check compares: size and modification time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    len: u64,
    modified: Option<SystemTime>,
}

impl Signature {
    /// The file's signature, or `None` when there is no file.
    pub fn of(path: &Path) -> Option<Signature> {
        let meta = fs::metadata(path).ok()?;
        Some(Signature {
            len: meta.len(),
            modified: meta.modified().ok(),
        })
    }
}

/// The theme file of this app (portable-aware, next to the settings).
pub fn path(app: &AppHandle) -> Result<PathBuf, tauri::Error> {
    crate::portable::resolve_app_data(app, FILE_NAME)
}

/// The app's theme file and its read errors, shared by the commands and the
/// change check.
pub struct ThemeFileState {
    path: Option<PathBuf>,
    errors: Mutex<ErrorOnce>,
}

impl ThemeFileState {
    fn css(&self) -> Option<String> {
        let path = self.path.as_ref()?;
        let mut errors = self.errors.lock().unwrap_or_else(|e| e.into_inner());
        errors.css(path, read(path))
    }
}

/// Writes the theme file if it is missing and starts checking it for
/// changes. Called once at startup, before the first window loads; a file
/// deleted later is not written again until the next start.
pub fn init(app: &AppHandle) {
    let path = match path(app) {
        Ok(path) => path,
        Err(error) => {
            log::warn!("No folder for the theme file, using the built-in styles: {error}");
            app.manage(ThemeFileState {
                path: None,
                errors: Mutex::default(),
            });
            return;
        }
    };
    match ensure_exists(&path) {
        Ok(true) => log::info!("Wrote the theme file {}", path.display()),
        Ok(false) => log::debug!("Theme file {}", path.display()),
        Err(error) => log::warn!("Can't write the theme file {}: {error}", path.display()),
    }
    let start = Signature::of(&path);
    app.manage(ThemeFileState {
        path: Some(path.clone()),
        errors: Mutex::default(),
    });
    let handle = app.clone();
    let spawned = thread::Builder::new()
        .name("theme-file".into())
        .spawn(move || watch(handle, path, start));
    if let Err(error) = spawned {
        log::warn!("Can't check the theme file for changes: {error}");
    }
}

/// Sends the file's CSS to every webview whenever its size or modification
/// time changes, including when it is deleted or comes back.
fn watch(app: AppHandle, path: PathBuf, mut last: Option<Signature>) {
    loop {
        thread::sleep(POLL_INTERVAL);
        let now = Signature::of(&path);
        if now == last {
            continue;
        }
        last = now;
        let css = app.state::<ThemeFileState>().css();
        if let Err(error) = app.emit(CHANGED_EVENT, css) {
            log::warn!("Can't send the theme file to the windows: {error}");
        }
    }
}

/// The theme file's CSS, or `null` while it is missing or unreadable (the
/// built-in styles alone). Never an error: a bad file must not break the UI.
#[tauri::command]
#[specta::specta]
pub fn get_user_theme(app: AppHandle) -> Option<String> {
    app.try_state::<ThemeFileState>()?.css()
}

/// Opens the theme file in the system's default app for CSS files, writing
/// it first if it is missing (Debug page).
#[tauri::command]
#[specta::specta]
pub fn open_theme_file(app: AppHandle) -> Result<(), String> {
    let path = path(&app).map_err(|e| format!("Failed to find the theme file: {e}"))?;
    ensure_exists(&path).map_err(|e| format!("Failed to write the theme file: {e}"))?;
    app.opener()
        .open_path(path.to_string_lossy().to_string(), None::<String>)
        .map_err(|e| format!("Failed to open the theme file: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The CSS with its comments removed.
    fn strip_comments(css: &str) -> String {
        let mut out = String::new();
        let mut rest = css;
        while let Some(start) = rest.find("/*") {
            out.push_str(&rest[..start]);
            match rest[start + 2..].find("*/") {
                Some(end) => rest = &rest[start + 2 + end + 2..],
                None => return out,
            }
        }
        out.push_str(rest);
        out
    }

    /// The comments of a CSS text, whitespace collapsed.
    fn comments(css: &str) -> Vec<String> {
        let mut found = Vec::new();
        let mut rest = css;
        while let Some(start) = rest.find("/*") {
            let Some(end) = rest[start + 2..].find("*/") else {
                break;
            };
            let body = &rest[start + 2..start + 2 + end];
            found.push(body.split_whitespace().collect::<Vec<_>>().join(" "));
            rest = &rest[start + 2 + end + 2..];
        }
        found
    }

    /// The custom properties of the first `:root` block, value on one line.
    fn root_tokens(css: &str) -> Vec<(String, String)> {
        let css = strip_comments(css);
        let start = css.find(":root {").expect("a :root block");
        let open = start + ":root ".len();
        let mut depth = 0;
        let mut close = open;
        for (i, c) in css[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        close = open + i;
                        break;
                    }
                }
                _ => {}
            }
        }
        css[open + 1..close]
            .split(';')
            .filter_map(|part| {
                let (name, value) = part.split_once(':')?;
                let name = name.trim();
                name.starts_with("--").then(|| {
                    (
                        name.to_string(),
                        value.split_whitespace().collect::<Vec<_>>().join(" "),
                    )
                })
            })
            .collect()
    }

    /// `--color-text: var(--light-color-text)`: the line that picks the
    /// active theme's value, which a user override would pin to one theme.
    fn is_active_mapping(name: &str, value: &str) -> bool {
        value == format!("var(--light-{})", &name[2..])
    }

    #[test]
    fn template_lists_every_built_in_token_with_its_default_commented_out() {
        let listed = comments(&template());
        let mut checked = 0;
        for (source, css) in BUILT_IN_SOURCES {
            for (name, value) in root_tokens(css) {
                if is_active_mapping(&name, &value) {
                    continue;
                }
                let line = format!("{name}: {value};");
                assert!(
                    listed.contains(&line),
                    "{source}: the template lacks `/* {line} */`"
                );
                checked += 1;
            }
        }
        assert!(checked > 60, "only {checked} tokens found in the sources");
    }

    #[test]
    fn template_has_light_and_dark_default_of_every_pair() {
        let listed = comments(&template());
        let theme = root_tokens(BUILT_IN_SOURCES[0].1);
        for (name, _) in theme.iter().filter(|(n, _)| n.starts_with("--light-")) {
            let dark = format!("--dark-{}", &name["--light-".len()..]);
            if !theme.iter().any(|(n, _)| *n == dark) {
                continue;
            }
            for wanted in [name, &dark] {
                assert!(
                    listed.iter().any(|c| c.starts_with(&format!("{wanted}:"))),
                    "the template lacks {wanted}"
                );
            }
        }
    }

    #[test]
    fn template_overrides_nothing() {
        let code: String = strip_comments(&template())
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        assert_eq!(code, ":root{}", "everything but the block is commented out");
    }

    #[test]
    fn template_leaves_out_the_lines_that_pick_the_active_theme() {
        let listed = comments(&template());
        assert!(
            !listed
                .iter()
                .any(|c| c.starts_with("--color-text: var(--light-color-text)")),
            "overriding --color-text would pin one theme"
        );
    }

    #[test]
    fn template_header_says_what_the_file_is_and_how_to_go_back() {
        let text = template();
        let header = comments(&text).into_iter().next().unwrap_or_default();
        for needed in ["DanyWhere", "reload", "any CSS", "delete this file"] {
            assert!(
                header.to_lowercase().contains(&needed.to_lowercase()),
                "the header lacks {needed:?}: {header}"
            );
        }
        assert!(text.is_ascii(), "the header is plain English");
    }

    #[test]
    fn creates_the_missing_file_with_the_template() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not-yet").join(FILE_NAME);
        assert!(ensure_exists(&path).unwrap(), "reports that it wrote one");
        assert_eq!(fs::read_to_string(&path).unwrap(), template());
    }

    #[test]
    fn never_overwrites_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        fs::write(&path, ":root { --light-color-text: red; }").unwrap();
        assert!(!ensure_exists(&path).unwrap(), "reports that it wrote none");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            ":root { --light-color-text: red; }"
        );
    }

    #[test]
    fn a_saved_file_gives_its_css() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        fs::write(&path, "body { color: red; }").unwrap();
        let mut errors = ErrorOnce::default();
        assert_eq!(
            errors.css(&path, read(&path)),
            Some("body { color: red; }".to_string())
        );
    }

    #[test]
    fn stray_bytes_do_not_lose_the_rest_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        fs::write(&path, b"a { color: red; } /* \xff */").unwrap();
        match read(&path) {
            ReadOutcome::Css(css) => assert!(css.starts_with("a { color: red; }")),
            other => panic!("expected CSS, got {other:?}"),
        }
    }

    #[test]
    fn a_missing_file_gives_the_built_in_styles_without_a_warning() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        assert_eq!(read(&path), ReadOutcome::Missing);
        let mut errors = ErrorOnce::default();
        assert_eq!(errors.css(&path, read(&path)), None);
        assert_eq!(errors.logged, 0);
    }

    #[test]
    fn an_unreadable_file_gives_the_built_in_styles_and_warns_once() {
        let dir = tempfile::tempdir().unwrap();
        // A folder where the file should be: there, but not readable as one.
        let path = dir.path().join(FILE_NAME);
        fs::create_dir(&path).unwrap();
        assert!(matches!(read(&path), ReadOutcome::Unreadable(_)));
        let mut errors = ErrorOnce::default();
        assert_eq!(errors.css(&path, read(&path)), None);
        assert_eq!(errors.css(&path, read(&path)), None);
        assert_eq!(errors.logged, 1, "the same error is logged once");
    }

    #[test]
    fn warns_again_after_the_file_was_readable_in_between() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        let mut errors = ErrorOnce::default();
        let broken = ReadOutcome::Unreadable("Permission denied".to_string());
        errors.css(&path, broken.clone());
        errors.css(&path, ReadOutcome::Css(String::new()));
        errors.css(&path, broken);
        assert_eq!(errors.logged, 2);
    }

    #[test]
    fn the_signature_changes_when_the_file_is_saved_or_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        assert_eq!(Signature::of(&path), None);
        fs::write(&path, "a {}").unwrap();
        let first = Signature::of(&path);
        assert!(first.is_some());
        fs::write(&path, "a { color: red; }").unwrap();
        assert_ne!(Signature::of(&path), first);
        fs::remove_file(&path).unwrap();
        assert_eq!(Signature::of(&path), None);
    }
}
