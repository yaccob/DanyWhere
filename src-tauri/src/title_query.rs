//! DanyWhere: which apps can tell their window title by Apple event, and how
//! to ask each, for a Mac without Accessibility (`window_capture`).
//!
//! An app's bundle tells it without sending the app anything. Its Info.plist
//! names a scripting definition (`OSAScriptingDefinition`: an `.sdef` in
//! `Contents/Resources`, which may include others, such as macOS's standard
//! suite), or it has `NSAppleScriptEnabled` with an old Cocoa `.scriptSuite`.
//! What the definition offers for a window decides the [`TitleQuery`]. That
//! takes parsing XML, so the answer is kept per bundle until its Info.plist
//! changes, and a dictation finds it in [`title_query`]'s cache (sidecar
//! notes `qa/2026-11-13_1145_fenstertitel-dynamisch.md` and
//! `qa/2026-11-17_2030_fenstertitel-dynamisch-umsetzung.md`).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::{Instant, SystemTime};

use quick_xml::events::{BytesStart, Event};

/// How an app is asked for the title of the window in front.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleQuery {
    /// iTerm2, by its bundle id: the name of the focused session.
    ITerm2,
    /// Chromium browsers (`active tab`): the active tab's title. With `mode`
    /// in the dictionary, nothing for a window that isn't "normal".
    ActiveTab { private_mode: bool },
    /// Safari-like browsers (`current tab`): the current tab's name.
    CurrentTab,
    /// Every other app with the standard suite: its front window's name.
    FrontWindow,
}

/// iTerm2's bundle id.
pub const ITERM2: &str = "com.googlecode.iterm2";

/// An application bundle: its id, its name and where it lies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
}

/// An app for the settings' list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleApp {
    pub id: String,
    pub name: String,
    pub running: bool,
}

/// Whether `bundle_id` may stand in an AppleScript: letters, digits, dots and
/// hyphens only, so it can't end the string it stands in and add commands.
pub fn valid_bundle_id(bundle_id: &str) -> bool {
    !bundle_id.is_empty()
        && bundle_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
}

/// The AppleScript that asks the app `bundle_id` for its title as `query`
/// says; `None` for an id that isn't plain ([`valid_bundle_id`]). A window
/// without a title answers "", and so does a window that isn't "normal" when
/// the app tells its mode (incognito, InPrivate).
pub fn script(query: TitleQuery, bundle_id: &str) -> Option<String> {
    if !valid_bundle_id(bundle_id) {
        return None;
    }
    let (guard, title) = match query {
        TitleQuery::ITerm2 => ("", "name of current session of current window"),
        TitleQuery::ActiveTab { private_mode: true } => (
            "if mode of front window is not \"normal\" then return \"\"\n",
            "title of active tab of front window",
        ),
        TitleQuery::ActiveTab {
            private_mode: false,
        } => ("", "title of active tab of front window"),
        TitleQuery::CurrentTab => ("", "name of current tab of front window"),
        TitleQuery::FrontWindow => ("", "name of front window"),
    };
    Some(format!(
        "tell application id \"{bundle_id}\"\n{guard}set theTitle to {title}\nend tell\n\
         if theTitle is missing value then return \"\"\nreturn theTitle"
    ))
}

/// Files read for one app's dictionary at most, the included ones counted.
const MAX_FILES: usize = 16;

/// A scripting definition larger than this is not read. The largest known,
/// Word's and Excel's, have about 840 KB.
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// What the files of the bundle at `bundle` say about asking the app for its
/// window title, without the cache. `None` when it can't be asked, or not
/// for a window's title.
pub fn detect(bundle: &Path) -> Option<TitleQuery> {
    let contents = bundle.join("Contents");
    let info = plist::Value::from_file(contents.join("Info.plist")).ok()?;
    let info = info.as_dictionary()?;
    let resources = contents.join("Resources");
    if let Some(name) = info
        .get("OSAScriptingDefinition")
        .and_then(plist::Value::as_string)
    {
        // "dynamic": the app makes its dictionary while it runs, so there is
        // nothing to read without asking it.
        if name == "dynamic" {
            return None;
        }
        if let Some(path) = definition_file(&resources, name) {
            let mut dictionary = Dictionary::default();
            dictionary.read(&path, &mut HashSet::new());
            return dictionary.title_query();
        }
    }
    // An old Cocoa app (TextEdit, Automator, Script Editor): its suites build
    // on Foundation's NSCoreSuite, whose window has the name ("title", `pnam`).
    // `NSAppleScriptEnabled` alone, without any scripting file (Chrome's web
    // app wrappers, OneDrive), doesn't count.
    (apple_script_enabled(info) && has_script_suite(&resources)).then_some(TitleQuery::FrontWindow)
}

/// The file `OSAScriptingDefinition` names: as written, or with `.sdef`
/// added (Messages names "Messages").
fn definition_file(resources: &Path, name: &str) -> Option<PathBuf> {
    let named = resources.join(name);
    if named.is_file() {
        return Some(named);
    }
    let with_extension = resources.join(format!("{name}.sdef"));
    with_extension.is_file().then_some(with_extension)
}

/// `NSAppleScriptEnabled`, a boolean or a string such as "YES".
fn apple_script_enabled(info: &plist::Dictionary) -> bool {
    match info.get("NSAppleScriptEnabled") {
        Some(plist::Value::Boolean(enabled)) => *enabled,
        Some(plist::Value::String(text)) => matches!(
            text.trim().to_ascii_lowercase().as_str(),
            "yes" | "true" | "1"
        ),
        _ => false,
    }
}

/// Whether the bundle has an old Cocoa scripting suite (`*.scriptSuite`).
fn has_script_suite(resources: &Path) -> bool {
    fs::read_dir(resources).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "scriptSuite")
        })
    })
}

/// A class of a scripting definition: its properties with their types, and
/// the classes it inherits from.
#[derive(Debug, Default)]
struct ClassInfo {
    properties: HashMap<String, Option<String>>,
    inherits: HashSet<String>,
}

/// The classes of a scripting definition and of the files it includes, the
/// definitions of a class (class, class extensions) merged.
#[derive(Debug, Default)]
struct Dictionary {
    classes: HashMap<String, ClassInfo>,
}

/// What one file of a scripting definition holds.
#[derive(Debug, Default)]
struct DefinitionFile {
    classes: Vec<(String, ClassInfo)>,
    includes: Vec<String>,
}

impl Dictionary {
    /// Adds the file at `path` and the files it includes, each read once.
    fn read(&mut self, path: &Path, seen: &mut HashSet<PathBuf>) {
        let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if seen.len() >= MAX_FILES || !seen.insert(key) {
            return;
        }
        let Some(file) = read_text(path).and_then(|text| parse_definition(&text)) else {
            log::debug!("Could not read the scripting definition {}", path.display());
            return;
        };
        for (name, class) in file.classes {
            let merged = self.classes.entry(name).or_default();
            merged.properties.extend(class.properties);
            merged.inherits.extend(class.inherits);
        }
        for href in file.includes {
            if let Some(included) = include_path(&href, path) {
                self.read(&included, seen);
            }
        }
    }

    /// The properties of `class` and of the classes it inherits from.
    fn properties(&self, class: &str) -> HashMap<&str, Option<&str>> {
        let mut properties = HashMap::new();
        let mut pending = vec![class];
        let mut visited = HashSet::new();
        while let Some(name) = pending.pop() {
            if !visited.insert(name) {
                continue;
            }
            if let Some(info) = self.classes.get(name) {
                for (property, kind) in &info.properties {
                    properties
                        .entry(property.as_str())
                        .or_insert(kind.as_deref());
                }
                pending.extend(info.inherits.iter().map(String::as_str));
            }
        }
        properties
    }

    /// The properties of the class a window's `property` points to, by its
    /// type ("tab" when it names none).
    fn tab_properties(
        &self,
        window: &HashMap<&str, Option<&str>>,
        property: &str,
    ) -> HashMap<&str, Option<&str>> {
        match window.get(property) {
            Some(kind) => self.properties(kind.unwrap_or("tab")),
            None => HashMap::new(),
        }
    }

    fn title_query(&self) -> Option<TitleQuery> {
        let window = self.properties("window");
        if self
            .tab_properties(&window, "active tab")
            .contains_key("title")
        {
            return Some(TitleQuery::ActiveTab {
                private_mode: window.contains_key("mode"),
            });
        }
        if self
            .tab_properties(&window, "current tab")
            .contains_key("name")
        {
            return Some(TitleQuery::CurrentTab);
        }
        window
            .contains_key("name")
            .then_some(TitleQuery::FrontWindow)
    }
}

/// A file's text, unless it is too large to be a scripting definition.
fn read_text(path: &Path) -> Option<String> {
    if fs::metadata(path).ok()?.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    Some(text.strip_prefix('\u{feff}').unwrap_or(&text).to_string())
}

/// The classes, properties and includes of one scripting definition file;
/// `None` when it isn't well-formed XML.
fn parse_definition(text: &str) -> Option<DefinitionFile> {
    let mut reader = quick_xml::Reader::from_str(text);
    let decoder = reader.decoder();
    let attribute = |element: &BytesStart, name: &[u8]| -> Option<String> {
        element
            .try_get_attribute(name)
            .ok()??
            .decode_and_unescape_value(decoder)
            .ok()
            .map(|value| value.into_owned())
    };
    let mut file = DefinitionFile::default();
    // Per open element, for a class or class extension its index in
    // `file.classes`.
    let mut open: Vec<Option<usize>> = Vec::new();
    loop {
        let event = reader.read_event().ok()?;
        let (element, empty) = match &event {
            Event::Start(element) => (element, false),
            Event::Empty(element) => (element, true),
            Event::End(_) => {
                open.pop();
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        let class = match element.local_name().as_ref() {
            b"class" => attribute(element, b"name").map(|name| {
                let mut class = ClassInfo::default();
                class.inherits.extend(attribute(element, b"inherits"));
                file.classes.push((name, class));
                file.classes.len() - 1
            }),
            b"class-extension" => attribute(element, b"extends").map(|name| {
                file.classes.push((name, ClassInfo::default()));
                file.classes.len() - 1
            }),
            b"property" => {
                if let (Some(Some(index)), Some(name)) = (open.last(), attribute(element, b"name"))
                {
                    file.classes[*index]
                        .1
                        .properties
                        .insert(name, attribute(element, b"type"));
                }
                None
            }
            b"include" => {
                file.includes.extend(attribute(element, b"href"));
                None
            }
            _ => None,
        };
        if !empty {
            open.push(class);
        }
    }
    Some(file)
}

/// The file an `xi:include` names, relative to the file `from` it stands in:
/// `file://localhost/…`, `file:///…`, an absolute or a relative path.
fn include_path(href: &str, from: &Path) -> Option<PathBuf> {
    let path = if let Some(rest) = href.strip_prefix("file://localhost") {
        rest
    } else if let Some(rest) = href.strip_prefix("file://") {
        rest
    } else if href.contains("://") {
        return None;
    } else {
        href
    };
    let path = PathBuf::from(percent_decoded(path));
    if path.is_absolute() {
        Some(path)
    } else {
        Some(from.parent()?.join(path))
    }
}

/// `text` with `%20` and the like decoded.
fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if let Some(byte) = text
                .get(index + 1..index + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
            {
                decoded.push(byte);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

/// Per bundle path: the time its Info.plist was written, and what [`detect`]
/// found then.
type CacheEntries = HashMap<PathBuf, (Option<SystemTime>, Option<TitleQuery>)>;

/// What [`detect`] found per bundle, kept until the bundle's Info.plist
/// changes, as it does with every update.
#[derive(Default)]
pub struct QueryCache {
    entries: Mutex<CacheEntries>,
}

impl QueryCache {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, CacheEntries> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// How to ask the app `bundle_id`, whose bundle is at `bundle`, for its
    /// window title; `None` when it can't be asked. iTerm2 has a rule of its
    /// own; every other app is read from its files once per version.
    pub fn query_for(&self, bundle_id: &str, bundle: &Path) -> Option<TitleQuery> {
        if bundle_id == ITERM2 {
            return Some(TitleQuery::ITerm2);
        }
        let stamp = fs::metadata(bundle.join("Contents/Info.plist"))
            .and_then(|metadata| metadata.modified())
            .ok();
        if let Some((cached_stamp, query)) = self.lock().get(bundle) {
            if *cached_stamp == stamp {
                return *query;
            }
        }
        let started = Instant::now();
        let query = detect(bundle);
        log::trace!(
            "Read in {:?} how to ask {} for a window title: {query:?}",
            started.elapsed(),
            bundle.display()
        );
        self.lock().insert(bundle.to_path_buf(), (stamp, query));
        query
    }
}

static CACHE: LazyLock<QueryCache> = LazyLock::new(QueryCache::new);

/// How to ask the app `bundle_id`, whose bundle is at `bundle`, for its
/// window title, from the cache; `None` when it can't be asked.
pub fn title_query(bundle_id: &str, bundle: &Path) -> Option<TitleQuery> {
    CACHE.query_for(bundle_id, bundle)
}

/// The apps the settings list for window titles: the running ones, then the
/// installed ones that aren't running, each by name; only apps `query` finds
/// a way to ask, each id once, never `own_id` (DanyWhere itself).
pub fn title_apps(
    running: Vec<Bundle>,
    installed: Vec<Bundle>,
    own_id: &str,
    query: impl Fn(&str, &Path) -> Option<TitleQuery>,
) -> Vec<TitleApp> {
    let mut seen = HashSet::new();
    let mut apps: Vec<TitleApp> = running
        .into_iter()
        .map(|bundle| (bundle, true))
        .chain(installed.into_iter().map(|bundle| (bundle, false)))
        .filter(|(bundle, _)| {
            bundle.id != own_id
                && valid_bundle_id(&bundle.id)
                && seen.insert(bundle.id.clone())
                && query(&bundle.id, &bundle.path).is_some()
        })
        .map(|(bundle, running)| TitleApp {
            id: bundle.id,
            name: bundle.name,
            running,
        })
        .collect();
    apps.sort_by(|a, b| {
        b.running
            .cmp(&a.running)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    apps
}

#[cfg(test)]
mod tests {
    use super::*;
    use plist::{Dictionary as PlistDictionary, Value};
    use std::fs;
    use std::time::{Duration, Instant, SystemTime};

    /// A fake app bundle `<dir>/<name>.app` with these Info.plist keys and
    /// these files in `Contents/Resources`.
    fn bundle(
        dir: &Path,
        name: &str,
        info: &[(&str, Value)],
        resources: &[(&str, &str)],
    ) -> PathBuf {
        let app = dir.join(format!("{name}.app"));
        let resources_dir = app.join("Contents/Resources");
        fs::create_dir_all(&resources_dir).unwrap();
        let mut dictionary = PlistDictionary::new();
        dictionary.insert(
            "CFBundleIdentifier".to_string(),
            Value::String(format!("com.example.{name}")),
        );
        for (key, value) in info {
            dictionary.insert(key.to_string(), value.clone());
        }
        Value::Dictionary(dictionary)
            .to_file_xml(app.join("Contents/Info.plist"))
            .unwrap();
        for (file, text) in resources {
            fs::write(resources_dir.join(file), text).unwrap();
        }
        app
    }

    /// A bundle whose Info.plist names `App.sdef` with this content.
    fn sdef_app(dir: &Path, name: &str, sdef: &str) -> PathBuf {
        bundle(
            dir,
            name,
            &[
                ("OSAScriptingDefinition", Value::String("App.sdef".into())),
                ("NSAppleScriptEnabled", Value::Boolean(true)),
            ],
            &[("App.sdef", sdef)],
        )
    }

    /// A scripting definition around `body`, as apps write them.
    fn sdef(body: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE dictionary SYSTEM "file://localhost/System/Library/DTDs/sdef.dtd">
<dictionary xmlns:xi="http://www.w3.org/2003/XInclude" title="Test Terminology">
{body}
</dictionary>
"#
        )
    }

    /// What counts of macOS's standard suite (`CocoaStandard.sdef`).
    const STANDARD_SUITE: &str = r#"<suite name="Standard Suite" code="????" description="Common classes and commands for all applications.">
  <class name="application" code="capp" description="The application's top-level scripting object.">
    <property name="name" code="pnam" type="text" access="r" description="The name of the application."/>
    <element type="window" access="r"><cocoa key="orderedWindows"/></element>
  </class>
  <class name="window" code="cwin" description="A window.">
    <cocoa class="NSWindow"/>
    <property name="name" code="pnam" type="text" access="r" description="The title of the window.">
      <cocoa key="title"/>
    </property>
    <property name="index" code="pidx" type="integer" description="The index of the window."/>
  </class>
</suite>"#;

    /// A Chromium browser's suite (Chrome's `scripting.sdef`), with `MODE`
    /// standing for the `mode` property or nothing.
    const CHROMIUM_SUITE: &str = r#"<suite name="Chromium Suite" code="CrSu" description="Common classes and commands for Chrome.">
  <class name="window" code="cwin" description="A window.">
    <cocoa class="BrowserCrWindow"/>
    <property name="name" code="pnam" type="text" access="r" description="The full title of the window."/>
    <property name="active tab" code="acTa" type="tab" access="r" description="Returns the currently selected tab"/>
    MODE
  </class>
  <class name="tab" code="CTab" description="A browser tab.">
    <property name="title" code="pnam" type="text" access="r" description="The title of the tab."/>
    <property name="URL" code="URL " type="text" description="The url visible to the user."/>
  </class>
</suite>"#;

    const MODE: &str = r#"<property name="mode" code="mode" type="text" description="Represents the mode of the window which can be &apos;normal&apos; or &apos;incognito&apos;."><cocoa key="mode"/></property>"#;

    /// Safari's own suite; it takes the window's name from the standard one.
    const SAFARI_SUITE: &str = r#"<suite name="Safari suite" code="sfri" description="Safari specific classes">
  <class-extension extends="window" description="A Safari window.">
    <property name="current tab" code="cTab" type="tab" description="The current tab."/>
  </class-extension>
  <class name="tab" code="bTab" description="A Safari window tab.">
    <property name="name" code="pnam" type="text" access="r" description="The name of the tab."/>
    <property name="URL" code="pURL" type="text" description="The current URL of the tab."/>
  </class>
</suite>"#;

    /// An `xi:include` of the file at `href`.
    fn include(href: &str) -> String {
        format!(r#"<xi:include href="{href}" xpointer="xpointer(/dictionary/suite)"/>"#)
    }

    /// The standard suite in a file of its own, as `/System/Library/
    /// ScriptingDefinitions/CocoaStandard.sdef` is.
    fn standard_file(dir: &Path) -> PathBuf {
        let path = dir.join("CocoaStandard.sdef");
        fs::write(&path, sdef(STANDARD_SUITE)).unwrap();
        path
    }

    #[test]
    fn a_chromium_dictionary_with_mode_leaves_private_windows_out() {
        let dir = tempfile::tempdir().unwrap();
        let app = sdef_app(
            dir.path(),
            "Edge",
            &sdef(&CHROMIUM_SUITE.replace("MODE", MODE)),
        );
        assert_eq!(
            detect(&app),
            Some(TitleQuery::ActiveTab { private_mode: true })
        );
    }

    #[test]
    fn a_chromium_dictionary_without_mode_reads_every_window() {
        let dir = tempfile::tempdir().unwrap();
        let app = sdef_app(
            dir.path(),
            "Browser",
            &sdef(&CHROMIUM_SUITE.replace("MODE", "")),
        );
        assert_eq!(
            detect(&app),
            Some(TitleQuery::ActiveTab {
                private_mode: false
            })
        );
    }

    /// Safari's tab property comes from its own suite, the window's name from
    /// the included standard suite.
    #[test]
    fn a_safari_like_dictionary_reads_the_current_tab() {
        let dir = tempfile::tempdir().unwrap();
        let standard = standard_file(dir.path());
        let body = format!(
            "{}\n{SAFARI_SUITE}",
            include(&format!("file://localhost{}", standard.display()))
        );
        let app = sdef_app(dir.path(), "Safari", &sdef(&body));
        assert_eq!(detect(&app), Some(TitleQuery::CurrentTab));
    }

    #[test]
    fn the_standard_suite_gives_the_front_windows_name() {
        let dir = tempfile::tempdir().unwrap();
        let app = sdef_app(dir.path(), "Notes", &sdef(STANDARD_SUITE));
        assert_eq!(detect(&app), Some(TitleQuery::FrontWindow));
    }

    #[test]
    fn the_standard_suite_counts_when_included_with_localhost() {
        let dir = tempfile::tempdir().unwrap();
        let standard = standard_file(dir.path());
        let app = sdef_app(
            dir.path(),
            "Mail",
            &sdef(&include(&format!("file://localhost{}", standard.display()))),
        );
        assert_eq!(detect(&app), Some(TitleQuery::FrontWindow));
    }

    #[test]
    fn the_standard_suite_counts_when_included_as_a_plain_file_url() {
        let dir = tempfile::tempdir().unwrap();
        let standard = standard_file(dir.path());
        let app = sdef_app(
            dir.path(),
            "Pages",
            &sdef(&include(&format!("file://{}", standard.display()))),
        );
        assert_eq!(detect(&app), Some(TitleQuery::FrontWindow));
    }

    /// A relative `href` is read next to the dictionary that names it, and a
    /// space written as `%20` is a space.
    #[test]
    fn a_relative_include_is_read_next_to_the_dictionary() {
        let dir = tempfile::tempdir().unwrap();
        let app = bundle(
            dir.path(),
            "Keynote",
            &[("OSAScriptingDefinition", Value::String("App.sdef".into()))],
            &[
                ("App.sdef", &sdef(&include("Standard%20Suite.sdef"))),
                ("Standard Suite.sdef", &sdef(STANDARD_SUITE)),
            ],
        );
        assert_eq!(detect(&app), Some(TitleQuery::FrontWindow));
    }

    /// Apps include the standard suite from macOS itself, in both forms.
    #[test]
    fn the_standard_suite_of_macos_has_the_window_name() {
        let dir = tempfile::tempdir().unwrap();
        for (n, href) in [
            "file://localhost/System/Library/ScriptingDefinitions/CocoaStandard.sdef",
            "file:///System/Library/ScriptingDefinitions/CocoaStandard.sdef",
        ]
        .into_iter()
        .enumerate()
        {
            let app = sdef_app(
                &dir.path().join(n.to_string()),
                "Reminders",
                &sdef(&include(href)),
            );
            assert_eq!(detect(&app), Some(TitleQuery::FrontWindow), "{href}");
        }
    }

    /// Music, TV, Photos: a dictionary, but no window with a name.
    #[test]
    fn a_dictionary_without_a_window_name_gives_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let music = sdef_app(
            dir.path(),
            "Music",
            &sdef(
                r#"<suite name="Music Suite" code="hook">
  <class name="application" code="capp"><property name="name" code="pnam" type="text"/></class>
  <class name="track" code="cTrk"><property name="name" code="pnam" type="text"/></class>
  <class name="window" code="cwin"><property name="index" code="pidx" type="integer"/></class>
</suite>"#,
            ),
        );
        assert_eq!(detect(&music), None);
    }

    #[test]
    fn an_app_without_a_dictionary_gives_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let app = bundle(dir.path(), "Teams", &[], &[]);
        assert_eq!(detect(&app), None);
        assert_eq!(detect(&dir.path().join("Missing.app")), None);
    }

    /// Chrome's web-app wrappers, OneDrive, GarageBand: only
    /// `NSAppleScriptEnabled`, no scripting file at all.
    #[test]
    fn a_web_app_wrapper_with_only_apple_script_enabled_gives_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let app = bundle(
            dir.path(),
            "YouTube",
            &[("NSAppleScriptEnabled", Value::Boolean(true))],
            &[("app.icns", "")],
        );
        assert_eq!(detect(&app), None);
    }

    /// TextEdit, Automator, Script Editor: `NSAppleScriptEnabled` (here as
    /// the string "YES") and an old Cocoa `.scriptSuite`, whose classes build
    /// on Foundation's `NSCoreSuite`; that one has the window's name.
    #[test]
    fn an_old_cocoa_script_suite_gives_the_front_windows_name() {
        let dir = tempfile::tempdir().unwrap();
        let app = bundle(
            dir.path(),
            "TextEdit",
            &[("NSAppleScriptEnabled", Value::String("YES".into()))],
            &[
                ("TextEdit.scriptSuite", "{}"),
                ("TextEdit.scriptTerminology", "{}"),
            ],
        );
        assert_eq!(detect(&app), Some(TitleQuery::FrontWindow));
        let disabled = bundle(
            dir.path(),
            "Disabled",
            &[("NSAppleScriptEnabled", Value::String("NO".into()))],
            &[("Disabled.scriptSuite", "{}")],
        );
        assert_eq!(detect(&disabled), None);
    }

    /// Messages names its dictionary "Messages"; the file is `Messages.sdef`.
    #[test]
    fn a_dictionary_named_without_its_extension_is_found() {
        let dir = tempfile::tempdir().unwrap();
        let app = bundle(
            dir.path(),
            "Messages",
            &[("OSAScriptingDefinition", Value::String("Messages".into()))],
            &[("Messages.sdef", &sdef(STANDARD_SUITE))],
        );
        assert_eq!(detect(&app), Some(TitleQuery::FrontWindow));
    }

    /// "dynamic" means the app makes its dictionary when it runs: nothing to
    /// read without asking it.
    #[test]
    fn a_dynamic_dictionary_gives_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let app = bundle(
            dir.path(),
            "Dynamic",
            &[
                ("OSAScriptingDefinition", Value::String("dynamic".into())),
                ("NSAppleScriptEnabled", Value::Boolean(true)),
            ],
            &[],
        );
        assert_eq!(detect(&app), None);
    }

    /// A window class can take its name from a class it inherits from; Word
    /// even has a window inheriting from window. Files including each other
    /// are read once.
    #[test]
    fn an_inherited_window_name_counts_and_cycles_end() {
        let dir = tempfile::tempdir().unwrap();
        let app = bundle(
            dir.path(),
            "Word",
            &[("OSAScriptingDefinition", Value::String("A.sdef".into()))],
            &[
                (
                    "A.sdef",
                    &sdef(&format!(
                        r#"{}<suite name="A" code="aaaa">
  <class name="item" code="cobj"><property name="name" code="pnam" type="text"/></class>
  <class name="window" code="cwin" inherits="item"><property name="index" code="pidx" type="integer"/></class>
  <class name="window" code="cwin" inherits="window"><property name="caption" code="1065" type="text"/></class>
</suite>"#,
                        include("B.sdef")
                    )),
                ),
                ("B.sdef", &sdef(&include("A.sdef"))),
            ],
        );
        let started = Instant::now();
        assert_eq!(detect(&app), Some(TitleQuery::FrontWindow));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    /// A dictionary that isn't well-formed counts for nothing, even the part
    /// before the error.
    #[test]
    fn a_broken_dictionary_gives_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let app = sdef_app(
            dir.path(),
            "Broken",
            r#"<dictionary><suite name="S" code="ssss"><class name="window" code="cwin"><property name="name" code="pnam" type="text"/></suite></dictionary>"#,
        );
        assert_eq!(detect(&app), None);
    }

    /// Some apps ship their Info.plist in binary form.
    #[test]
    fn a_binary_info_plist_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let app = sdef_app(dir.path(), "Binary", &sdef(STANDARD_SUITE));
        let info = app.join("Contents/Info.plist");
        let value = Value::from_file(&info).unwrap();
        value.to_file_binary(&info).unwrap();
        assert_eq!(detect(&app), Some(TitleQuery::FrontWindow));
    }

    /// iTerm2's session name beats its window's name, whatever its dictionary.
    #[test]
    fn iterm2_keeps_its_own_rule() {
        let dir = tempfile::tempdir().unwrap();
        let app = sdef_app(dir.path(), "iTerm", &sdef(STANDARD_SUITE));
        let cache = QueryCache::new();
        assert_eq!(cache.query_for(ITERM2, &app), Some(TitleQuery::ITerm2));
        assert_eq!(
            cache.query_for("com.example.iTerm", &app),
            Some(TitleQuery::FrontWindow)
        );
    }

    fn set_modified(path: &Path, time: SystemTime) {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(time)
            .unwrap();
    }

    /// The cache answers without reading the dictionary again until the
    /// bundle's Info.plist changes, as it does with every update.
    #[test]
    fn the_cache_reads_a_bundle_again_only_when_its_info_plist_changes() {
        let dir = tempfile::tempdir().unwrap();
        let app = sdef_app(dir.path(), "Updated", &sdef(STANDARD_SUITE));
        let info = app.join("Contents/Info.plist");
        let before = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        set_modified(&info, before);
        let cache = QueryCache::new();
        let id = "com.example.Updated";
        assert_eq!(cache.query_for(id, &app), Some(TitleQuery::FrontWindow));

        fs::write(
            app.join("Contents/Resources/App.sdef"),
            sdef(&CHROMIUM_SUITE.replace("MODE", "")),
        )
        .unwrap();
        set_modified(&info, before);
        assert_eq!(
            cache.query_for(id, &app),
            Some(TitleQuery::FrontWindow),
            "an unchanged Info.plist: the cached answer"
        );

        set_modified(&info, before + Duration::from_secs(60));
        assert_eq!(
            cache.query_for(id, &app),
            Some(TitleQuery::ActiveTab {
                private_mode: false
            }),
            "an updated bundle is read again"
        );
    }

    /// Only ids of letters, digits, dots and hyphens go into an AppleScript,
    /// so nothing from a bundle can end the string and add commands.
    #[test]
    fn only_plain_bundle_ids_get_into_a_script() {
        for id in [
            "com.google.Chrome",
            "com.microsoft.edgemac",
            "org.chromium.Chromium",
            "com.apple.iWork.Pages",
            "TeXShop",
            "com.example.app-2",
        ] {
            assert!(valid_bundle_id(id), "{id}");
            assert!(script(TitleQuery::FrontWindow, id).is_some(), "{id}");
        }
        for id in [
            "",
            r#"com.evil" to do shell script "touch /tmp/x"#,
            "com.example\nend tell",
            "com example",
            "com.example_app",
            "cöm.example",
            r"com.example\",
        ] {
            assert!(!valid_bundle_id(id), "{id:?}");
            assert_eq!(script(TitleQuery::FrontWindow, id), None, "{id:?}");
        }
    }

    #[test]
    fn each_kind_asks_for_its_own_title() {
        let iterm = script(TitleQuery::ITerm2, ITERM2).unwrap();
        assert!(iterm.contains(r#"tell application id "com.googlecode.iterm2""#));
        assert!(iterm.contains("name of current session of current window"));

        let edge = script(
            TitleQuery::ActiveTab { private_mode: true },
            "com.microsoft.edgemac",
        )
        .unwrap();
        assert!(edge.contains(r#"tell application id "com.microsoft.edgemac""#));
        assert!(edge.contains(r#"if mode of front window is not "normal" then return """#));
        assert!(edge.contains("title of active tab of front window"));

        let chromium = script(
            TitleQuery::ActiveTab {
                private_mode: false,
            },
            "org.chromium.Chromium",
        )
        .unwrap();
        assert!(chromium.contains("title of active tab of front window"));
        assert!(!chromium.contains("mode"));

        let safari = script(TitleQuery::CurrentTab, "com.apple.Safari").unwrap();
        assert!(safari.contains("name of current tab of front window"));

        let mail = script(TitleQuery::FrontWindow, "com.apple.mail").unwrap();
        assert!(mail.contains(r#"tell application id "com.apple.mail""#));
        assert!(mail.contains("name of front window"));
        assert!(!mail.contains("tab"));

        for script in [iterm, edge, chromium, safari, mail] {
            assert!(
                script.contains(r#"if theTitle is missing value then return """#),
                "a window without a title answers nothing: {script}"
            );
        }
    }

    fn app(id: &str, name: &str) -> Bundle {
        Bundle {
            id: id.to_string(),
            name: name.to_string(),
            path: PathBuf::from(format!("/Applications/{name}.app")),
        }
    }

    /// Running apps first, then installed ones that aren't running, each by
    /// name; only apps with a way to ask, never DanyWhere itself, each once.
    #[test]
    fn the_list_has_running_apps_first_then_installed_ones_by_name() {
        let running = vec![
            app("com.apple.Terminal", "Terminal"),
            app("com.example.Plain", "Plain"),
            app("com.yaccob.danywhere", "DanyWhere"),
            app("com.microsoft.edgemac", "Microsoft Edge"),
        ];
        let installed = vec![
            app("com.apple.Safari", "safari"),
            app("com.microsoft.edgemac", "Microsoft Edge"),
            app("com.apple.mail", "Mail"),
            app(r#"com.evil" to beep"#, "Evil"),
            app("com.apple.Music", "Music"),
            app("com.yaccob.danywhere", "DanyWhere"),
        ];
        let query = |id: &str, _: &Path| match id {
            "com.example.Plain" | "com.apple.Music" => None,
            "com.microsoft.edgemac" => Some(TitleQuery::ActiveTab { private_mode: true }),
            _ => Some(TitleQuery::FrontWindow),
        };
        let apps = title_apps(running, installed, "com.yaccob.danywhere", query);
        let shown: Vec<(&str, bool)> = apps
            .iter()
            .map(|app| (app.id.as_str(), app.running))
            .collect();
        assert_eq!(
            shown,
            vec![
                ("com.microsoft.edgemac", true),
                ("com.apple.Terminal", true),
                ("com.apple.mail", false),
                ("com.apple.Safari", false),
            ]
        );
    }

    /// Not a check: what this Mac's apps offer, to compare with the sidecar's
    /// scan. `cargo test --lib title_query -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_what_the_apps_on_this_mac_offer() {
        let mut folders = vec![
            PathBuf::from("/Applications"),
            PathBuf::from("/Applications/Utilities"),
            PathBuf::from("/System/Applications"),
            PathBuf::from("/System/Applications/Utilities"),
        ];
        if let Some(home) = std::env::var_os("HOME") {
            folders.push(PathBuf::from(home).join("Applications"));
        }
        let mut found = 0;
        let mut bundles = 0;
        let all = Instant::now();
        for folder in folders {
            let Ok(entries) = fs::read_dir(&folder) else {
                continue;
            };
            let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
            paths.sort();
            for path in paths {
                if path.extension().is_none_or(|ext| ext != "app") {
                    continue;
                }
                bundles += 1;
                let started = Instant::now();
                let query = detect(&path);
                let took = started.elapsed();
                if let Some(query) = query {
                    found += 1;
                    println!("{:<45} {:?} ({took:?})", path.display(), query);
                }
            }
        }
        println!(
            "{found} of {bundles} bundles can tell a title; all in {:?}",
            all.elapsed()
        );
    }
}
