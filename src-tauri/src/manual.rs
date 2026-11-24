//! DanyWhere: the user manual in a window of its own.
//!
//! The manual is a static site (Astro Starlight) built into the frontend's
//! `public/manual/`, so it ships inside the app and is served from the app's
//! own origin at `/manual/…`, matching the installed version and readable
//! offline. English is the root locale (`/manual/<page>`), the others sit
//! under their language (`/manual/de/<page>`). Every page's address ends in
//! `/` (Starlight's `trailingSlash: "always"`), and Tauri serves the
//! page's `index.html` for it; for a file the build lacks it would serve the
//! app's own `index.html`, so such pages are never opened (`available_page`,
//! `checked_link`).
//!
//! The window is static content and gets no IPC: it is in no capability, so
//! every plugin command is refused, and `denies_ipc` keeps it from the app's
//! own commands, which Tauri would otherwise let any local page call. Its
//! links stay inside `/manual/`; a link leaving the app opens in the default
//! browser (`manual_link`). It follows Application Theme: each page reads
//! Starlight's key in the localStorage it shares with the main window, which
//! writes it (`src/lib/utils/theme.ts`), and a change reaches the page on
//! screen through `apply_theme`.

use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_opener::OpenerExt;

use crate::settings::{get_settings, Theme};

/// Opens the manual's window at `page` (a page's slug, see `manual_url`),
/// or its home page, in the app's interface language; a window already open
/// goes to that page and comes to the front.
// Async: building a window from a synchronous command deadlocks on Windows.
#[tauri::command]
#[specta::specta]
pub async fn open_manual(app: AppHandle, page: Option<String>) -> Result<(), String> {
    let settings = get_settings(&app);
    let language = interface_language(&settings.app_language, tauri_plugin_os::locale());
    let path = manual_url(&language, page.as_deref());
    // A page the build lacks would show the app's own index.html.
    let path = match asset_check(&app) {
        Some(exists) => {
            available_page(&path, exists).ok_or_else(|| "This build has no manual".to_string())?
        }
        None => path,
    };
    show(&app, &path, settings.theme).map_err(|e| format!("Failed to open the manual: {e}"))
}

/// The label of the manual's window.
pub const MANUAL_WINDOW: &str = "manual";

/// Where the manual lives on the app's origin.
const MANUAL_ROOT: &str = "/manual/";

/// The languages the manual has besides English, its root locale.
const MANUAL_LANGUAGES: [&str; 3] = ["de", "es", "fr"];

/// The window's size when it opens.
const WINDOW_SIZE: (f64, f64) = (1100.0, 800.0);
const MIN_WINDOW_SIZE: (f64, f64) = (480.0, 360.0);

/// Shows the manual's page `path` (from `manual_url`): in the open window,
/// or in a new one.
fn show(app: &AppHandle, path: &str, theme: Theme) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(MANUAL_WINDOW) {
        // The main window's address is the app's origin on every platform
        // and in `tauri dev`; the manual's own may be an empty frame's.
        let base = app
            .get_webview_window("main")
            .and_then(|main| main.url().ok())
            .or_else(|| window.url().ok());
        match base.and_then(|base| base.join(path).ok()) {
            Some(url) => window.navigate(url)?,
            None => log::warn!("No address of the app to open the manual's {path} at"),
        }
        if window.is_minimized().unwrap_or(false) {
            window.unminimize()?;
        }
        window.show()?;
        return window.set_focus();
    }

    let dev_url = if tauri::is_dev() {
        app.config().build.dev_url.clone()
    } else {
        None
    };
    let origins = app_origins(cfg!(windows), dev_url.as_ref());
    let navigation_app = app.clone();
    let navigation_origins = origins.clone();
    let new_window_app = app.clone();

    let mut builder = WebviewWindowBuilder::new(app, MANUAL_WINDOW, WebviewUrl::App(path.into()))
        // Until the page's own title arrives.
        .title(app.package_info().name.clone())
        .inner_size(WINDOW_SIZE.0, WINDOW_SIZE.1)
        .min_inner_size(MIN_WINDOW_SIZE.0, MIN_WINDOW_SIZE.1)
        .resizable(true)
        // Shown once its title bar has the app's theme.
        .visible(false)
        .on_navigation(move |url| {
            let link = decide(&navigation_app, url, &navigation_origins);
            follow(&navigation_app, url, link)
        })
        .on_new_window(move |url, _features| {
            match decide(&new_window_app, &url, &origins) {
                // A page of the manual asked for a window of its own: it
                // shows in this one. Navigating from inside WebKit's
                // callback could re-enter it, so it happens afterwards.
                ManualLink::Stay => {
                    if let Some(window) = new_window_app.get_webview_window(MANUAL_WINDOW) {
                        tauri::async_runtime::spawn(async move {
                            if let Err(e) = window.navigate(url) {
                                log::warn!("Failed to show a manual page: {e}");
                            }
                        });
                    }
                }
                link => {
                    follow(&new_window_app, &url, link);
                }
            }
            NewWindowResponse::Deny
        })
        .on_document_title_changed(|window, title| {
            if let Err(e) = window.set_title(&title) {
                log::warn!("Failed to title the manual's window: {e}");
            }
        });

    // The main window's data directory in portable mode, so the two share
    // their storage (the theme, `src/lib/utils/theme.ts`).
    if let Some(data_dir) = crate::portable::data_dir() {
        builder = builder.data_directory(data_dir.join("webview"));
    }

    let window = builder.build()?;
    // The title bar in Application Theme, as the main window's.
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    crate::shortcut::apply_window_theme(app, theme);
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let _ = theme;
    window.show()?;
    window.set_focus()
}

/// `manual_link` for `url`, with the pages the build lacks blocked.
fn decide(app: &AppHandle, url: &Url, origins: &[Url]) -> ManualLink {
    let link = manual_link(url, origins);
    match asset_check(app) {
        Some(exists) => checked_link(url, link, Some(&exists as &dyn Fn(&str) -> bool)),
        None => link,
    }
}

/// What the window does with a navigation decided as `link`: whether it
/// loads it.
fn follow(app: &AppHandle, url: &Url, link: ManualLink) -> bool {
    match link {
        ManualLink::Stay => true,
        ManualLink::OpenInBrowser => {
            if let Err(e) = app.opener().open_url(url.as_str(), None::<&str>) {
                log::warn!("Failed to open {url} from the manual: {e}");
            }
            false
        }
        ManualLink::Block => {
            log::warn!("Kept the manual's window from loading {url}");
            false
        }
    }
}

/// The language the app's interface shows: the stored `app_language`, or
/// the system's locale while none is stored.
pub fn interface_language(stored: &str, system_locale: Option<String>) -> String {
    let stored = stored.trim();
    if !stored.is_empty() {
        return stored.to_string();
    }
    system_locale
        .filter(|locale| !locale.trim().is_empty())
        .unwrap_or_else(|| "en".to_string())
}

/// The path of a manual page on the app's origin, in the manual's language
/// for `app_language`. `page` is a page's slug such as
/// `how-it-works/start-and-end-shortcuts/`, optionally with `#anchor`; a
/// missing or invalid one gives the manual's home page.
pub fn manual_url(app_language: &str, page: Option<&str>) -> String {
    let mut url = MANUAL_ROOT.to_string();
    if let Some(language) = manual_language(app_language) {
        url.push_str(language);
        url.push('/');
    }
    if let Some(page) = page.and_then(page_path) {
        url.push_str(&page);
    }
    url
}

/// The manual's language for an interface language such as `de-AT`; none
/// for English and every language the manual lacks.
fn manual_language(app_language: &str) -> Option<&'static str> {
    let primary = app_language
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    MANUAL_LANGUAGES
        .into_iter()
        .find(|language| *language == primary)
}

/// A page's slug as it goes after the language: lowercase letters, digits
/// and `-` in segments separated by single `/`, ending in `/`, and an
/// optional `#anchor` of the same letters. Anything else is none.
fn page_path(page: &str) -> Option<String> {
    let slug = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-';
    let (path, anchor) = match page.split_once('#') {
        Some((path, anchor)) => (path, Some(anchor)),
        None => (page, None),
    };
    if !path.chars().all(|c| slug(c) || c == '/') || path.starts_with('/') || path.contains("//") {
        return None;
    }
    if let Some(anchor) = anchor {
        if anchor.is_empty() || !anchor.chars().all(slug) {
            return None;
        }
    }
    let mut out = path.to_string();
    if !out.is_empty() && !out.ends_with('/') {
        out.push('/');
    }
    if let Some(anchor) = anchor {
        out.push('#');
        out.push_str(anchor);
    }
    Some(out)
}

/// What happens to a navigation inside the manual's window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualLink {
    /// A page of the manual: the window goes there.
    Stay,
    /// A link leaving the app (a website, an e-mail address): the default
    /// browser or mail program opens it, the window stays where it is.
    OpenInBrowser,
    /// Anything else: nothing happens.
    Block,
}

/// Decides a navigation of the manual's window to `url`. `app_origins` are
/// the origins the app's own pages are served from (`app_origins`).
pub fn manual_link(url: &Url, app_origins: &[Url]) -> ManualLink {
    if app_origins.iter().any(|origin| same_origin(url, origin)) {
        // The URL is parsed, so `/manual/../index.html` is `/index.html`.
        let path = url.path();
        return if path == "/manual" || path.starts_with(MANUAL_ROOT) {
            ManualLink::Stay
        } else {
            ManualLink::Block
        };
    }
    match url.scheme() {
        "http" | "https" | "mailto" => ManualLink::OpenInBrowser,
        // An empty frame loads nothing.
        "about" if matches!(url.path(), "blank" | "srcdoc") => ManualLink::Stay,
        _ => ManualLink::Block,
    }
}

/// Scheme, host and port compared one by one: the `url` crate gives a
/// custom scheme such as `tauri:` an opaque origin, never equal to another.
fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str() == b.host_str()
        && a.port_or_known_default() == b.port_or_known_default()
}

/// The origins the app's pages are served from: `tauri://localhost` on
/// macOS and Linux, `http://tauri.localhost` on Windows, and the Vite dev
/// server while running `tauri dev`.
pub fn app_origins(windows: bool, dev_url: Option<&Url>) -> Vec<Url> {
    let own = if windows {
        "http://tauri.localhost"
    } else {
        "tauri://localhost"
    };
    Url::parse(own)
        .into_iter()
        .chain(dev_url.cloned())
        .collect()
}

/// Whether a command call from the webview `label` is refused. The manual
/// is static content and needs none; Tauri checks the app's own commands
/// against the capabilities only when the app has an ACL manifest of them,
/// which DanyWhere has not, so the window would otherwise reach them all.
pub fn denies_ipc(label: &str) -> bool {
    label == MANUAL_WINDOW
}

/// Whether closing the window `label` closes it for good. The app's own
/// windows only hide, and on macOS the app leaves the Dock with them; the
/// manual just closes, and the next `open_manual` opens it anew.
pub fn closes_for_good(label: &str) -> bool {
    label == MANUAL_WINDOW
}

/// Whether the build has a file for the address path `path`, as Tauri looks
/// it up (`path`, `path.html`, `path/index.html`) but without its last
/// resort, the app's own `index.html`. `exists` answers for an asset key
/// such as `/manual/de/index.html`.
pub fn served(path: &str, exists: impl Fn(&str) -> bool) -> bool {
    // As Tauri's `get_asset`: one trailing slash off, percent-decoded.
    let path = path.strip_suffix('/').unwrap_or(path);
    let path = percent_decode(path);
    let key = path.strip_prefix('/').unwrap_or(&path);
    if key.is_empty() {
        return exists("/index.html");
    }
    exists(&format!("/{key}"))
        || exists(&format!("/{key}.html"))
        || exists(&format!("/{key}/index.html"))
}

/// `%XX` sequences of an address path as the bytes they stand for.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = (bytes[i] == b'%')
            .then(|| bytes.get(i + 1..i + 3))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The page to open for `path` from `manual_url`: itself if the build has
/// it, else its language's home page, else the English one; none if the
/// build has no manual at all.
pub fn available_page(path: &str, exists: impl Fn(&str) -> bool) -> Option<String> {
    let page = path.split('#').next().unwrap_or_default();
    if served(page, &exists) {
        return Some(path.to_string());
    }
    let language_home = MANUAL_LANGUAGES
        .into_iter()
        .map(|language| format!("{MANUAL_ROOT}{language}/"))
        .find(|home| page.starts_with(home.as_str()));
    language_home
        .into_iter()
        .chain([MANUAL_ROOT.to_string()])
        .find(|home| served(home, &exists))
}

/// `link` for `url`, with a page of the manual the build lacks blocked:
/// Tauri would show the app's own `index.html` for it. `exists` is none
/// while the assets are not embedded (`tauri dev`).
pub fn checked_link(
    url: &Url,
    link: ManualLink,
    exists: Option<&dyn Fn(&str) -> bool>,
) -> ManualLink {
    match (link, exists) {
        (ManualLink::Stay, Some(exists))
            if url.scheme() != "about" && !served(url.path(), exists) =>
        {
            ManualLink::Block
        }
        _ => link,
    }
}

/// Whether the app's embedded files hold `key` (`/manual/de/index.html`).
/// Looked up one by one: `AssetResolver::get` answers a missing file with
/// the app's own `index.html`.
fn has_asset(app: &AppHandle, key: &str) -> bool {
    app.asset_resolver().iter().any(|(asset, _)| asset == key)
}

/// `has_asset` for `checked_link`: none while the assets are not embedded.
fn asset_check(app: &AppHandle) -> Option<impl Fn(&str) -> bool + '_> {
    (!tauri::is_dev()).then_some(move |key: &str| has_asset(app, key))
}

/// Brings the open manual's page into `theme` at once. The next page reads
/// it from localStorage, where the main window put it; the page on screen
/// read it before, so it is told here.
pub fn apply_theme(app: &AppHandle, theme: Theme) {
    if let Some(window) = app.get_webview_window(MANUAL_WINDOW) {
        if let Err(e) = window.eval(theme_script(theme)) {
            log::warn!("Failed to bring the manual into the app's theme: {e}");
        }
    }
}

/// Starlight's stored theme for Application Theme: "light", "dark", or
/// empty for the system's appearance.
pub fn starlight_theme(theme: Theme) -> &'static str {
    match theme {
        Theme::System => "",
        Theme::Light => "light",
        Theme::Dark => "dark",
    }
}

/// What Starlight's own theme menu does on a page: store the value, show it
/// in the menu, and set the `data-theme` its styles follow.
pub fn theme_script(theme: Theme) -> String {
    format!(
        r#"(() => {{
  const theme = "{}";
  try {{ localStorage.setItem("starlight-theme", theme); }} catch {{}}
  document.documentElement.dataset.theme =
    theme || (matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark");
  document.querySelectorAll("starlight-theme-select select").forEach((select) => {{
    select.value = theme || "auto";
  }});
}})();"#,
        starlight_theme(theme)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const START_AND_END: &str = "how-it-works/start-and-end-shortcuts/";

    // ---- interface_language ------------------------------------------------

    #[test]
    fn the_stored_interface_language_wins() {
        assert_eq!(interface_language("de", Some("fr-FR".into())), "de");
        assert_eq!(interface_language("es-419", None), "es-419");
    }

    #[test]
    fn without_a_stored_language_the_systems_locale_counts() {
        assert_eq!(interface_language("", Some("fr_FR".into())), "fr_FR");
        assert_eq!(interface_language("  ", Some("de-AT".into())), "de-AT");
    }

    #[test]
    fn without_either_it_is_english() {
        assert_eq!(interface_language("", None), "en");
    }

    // ---- manual_url: languages -----------------------------------------------

    #[test]
    fn english_is_the_root_locale() {
        assert_eq!(manual_url("en", None), "/manual/");
        assert_eq!(
            manual_url("en", Some(START_AND_END)),
            "/manual/how-it-works/start-and-end-shortcuts/"
        );
    }

    #[test]
    fn german_spanish_and_french_have_a_prefix_of_their_own() {
        assert_eq!(manual_url("de", None), "/manual/de/");
        assert_eq!(
            manual_url("de", Some(START_AND_END)),
            "/manual/de/how-it-works/start-and-end-shortcuts/"
        );
        assert_eq!(
            manual_url("es", Some(START_AND_END)),
            "/manual/es/how-it-works/start-and-end-shortcuts/"
        );
        assert_eq!(
            manual_url("fr", Some(START_AND_END)),
            "/manual/fr/how-it-works/start-and-end-shortcuts/"
        );
    }

    #[test]
    fn a_regional_locale_takes_its_languages_manual() {
        assert_eq!(manual_url("de-AT", None), "/manual/de/");
        assert_eq!(manual_url("de_CH", None), "/manual/de/");
        assert_eq!(manual_url("es-419", None), "/manual/es/");
        assert_eq!(manual_url("fr-CA", None), "/manual/fr/");
        assert_eq!(manual_url("DE", None), "/manual/de/");
        assert_eq!(manual_url("en-GB", None), "/manual/");
    }

    #[test]
    fn every_other_language_gets_english() {
        for language in ["ja", "zh-TW", "pt-BR", "it", "ru", "", "dex", "d"] {
            assert_eq!(
                manual_url(language, Some(START_AND_END)),
                "/manual/how-it-works/start-and-end-shortcuts/",
                "{language:?}"
            );
        }
    }

    // ---- manual_url: pages ---------------------------------------------------

    #[test]
    fn no_page_or_an_empty_one_is_the_home_page() {
        assert_eq!(manual_url("de", None), "/manual/de/");
        assert_eq!(manual_url("de", Some("")), "/manual/de/");
    }

    #[test]
    fn a_page_gets_its_trailing_slash() {
        assert_eq!(
            manual_url("en", Some("how-it-works/start-and-end-shortcuts")),
            "/manual/how-it-works/start-and-end-shortcuts/"
        );
    }

    #[test]
    fn an_anchor_is_kept() {
        assert_eq!(
            manual_url(
                "fr",
                Some("how-it-works/dictation-from-start-to-end/#the-end")
            ),
            "/manual/fr/how-it-works/dictation-from-start-to-end/#the-end"
        );
        assert_eq!(
            manual_url("en", Some("how-it-works/start-and-end-shortcuts#end-2")),
            "/manual/how-it-works/start-and-end-shortcuts/#end-2"
        );
    }

    #[test]
    fn an_invalid_page_opens_the_home_page() {
        for page in [
            "../secret/",
            "how-it-works/../../etc/passwd",
            "./how-it-works/",
            "/how-it-works/",
            "//evil.example/",
            "how-it-works//start/",
            "https://evil.example/",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "How-It-Works/",
            "how it works/",
            "how-it-works/?x=1",
            "%2e%2e/",
            "how-it-works\\start",
            "einführung/",
            "how-it-works/#",
            "how-it-works/#a#b",
            "how-it-works/#Anchor",
            "how-it-works/#an chor",
            "how-it-works/#a/b",
        ] {
            assert_eq!(manual_url("de", Some(page)), "/manual/de/", "{page:?}");
        }
    }

    // ---- manual_link ---------------------------------------------------------

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    fn mac() -> Vec<Url> {
        app_origins(false, None)
    }

    fn windows() -> Vec<Url> {
        app_origins(true, None)
    }

    fn dev() -> Vec<Url> {
        app_origins(false, Some(&url("http://localhost:1420")))
    }

    #[test]
    fn the_apps_origin_per_platform() {
        assert_eq!(mac(), vec![url("tauri://localhost")]);
        assert_eq!(windows(), vec![url("http://tauri.localhost")]);
        assert_eq!(
            dev(),
            vec![url("tauri://localhost"), url("http://localhost:1420")]
        );
    }

    #[test]
    fn a_page_of_the_manual_stays_in_the_window() {
        for link in [
            "tauri://localhost/manual/",
            "tauri://localhost/manual",
            "tauri://localhost/manual/de/how-it-works/start-and-end-shortcuts/",
            "tauri://localhost/manual/how-it-works/start-and-end-shortcuts/#end",
            "tauri://localhost/manual/_astro/index.css",
        ] {
            assert_eq!(manual_link(&url(link), &mac()), ManualLink::Stay, "{link}");
        }
        assert_eq!(
            manual_link(&url("http://tauri.localhost/manual/fr/"), &windows()),
            ManualLink::Stay
        );
        assert_eq!(
            manual_link(&url("http://localhost:1420/manual/es/"), &dev()),
            ManualLink::Stay
        );
    }

    #[test]
    fn the_apps_own_pages_outside_the_manual_are_blocked() {
        for link in [
            "tauri://localhost/",
            "tauri://localhost/index.html",
            "tauri://localhost/manualx/",
            "tauri://localhost/manual/../index.html",
        ] {
            assert_eq!(manual_link(&url(link), &mac()), ManualLink::Block, "{link}");
        }
        assert_eq!(
            manual_link(&url("http://tauri.localhost/"), &windows()),
            ManualLink::Block
        );
        assert_eq!(
            manual_link(&url("http://localhost:1420/"), &dev()),
            ManualLink::Block
        );
    }

    #[test]
    fn links_leaving_the_app_open_in_the_browser() {
        for link in [
            "https://github.com/yaccob/DanyWhere",
            "http://example.com/manual/",
            "mailto:someone@example.com",
            // Another platform's origin is not this app's.
            "http://tauri.localhost/manual/",
            // A look-alike host is somewhere else.
            "https://tauri.localhost.example.com/manual/",
            // The dev server only counts while running `tauri dev`.
            "http://localhost:1420/manual/",
        ] {
            assert_eq!(
                manual_link(&url(link), &mac()),
                ManualLink::OpenInBrowser,
                "{link}"
            );
        }
        assert_eq!(
            manual_link(&url("tauri://localhost/manual/"), &windows()),
            ManualLink::Block
        );
        assert_eq!(
            manual_link(&url("http://localhost:1421/manual/"), &dev()),
            ManualLink::OpenInBrowser
        );
    }

    #[test]
    fn an_empty_frame_may_load() {
        assert_eq!(manual_link(&url("about:blank"), &mac()), ManualLink::Stay);
        assert_eq!(manual_link(&url("about:srcdoc"), &mac()), ManualLink::Stay);
    }

    #[test]
    fn other_schemes_are_blocked() {
        for link in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,hi",
            "ftp://example.com/",
            "tel:+431234",
            "about:config",
            "asset://localhost/etc/passwd",
            "ipc://localhost/open_manual",
        ] {
            assert_eq!(manual_link(&url(link), &mac()), ManualLink::Block, "{link}");
        }
    }

    // ---- denies_ipc ----------------------------------------------------------

    #[test]
    fn only_the_manual_is_refused_the_apps_commands() {
        assert!(denies_ipc(MANUAL_WINDOW));
        assert!(!denies_ipc("main"));
        assert!(!denies_ipc("recording_overlay"));
    }

    // ---- closing -------------------------------------------------------------

    #[test]
    fn the_manual_closes_and_the_app_stays_in_the_dock() {
        assert!(closes_for_good(MANUAL_WINDOW));
        assert!(!closes_for_good("main"));
        assert!(!closes_for_good("recording_overlay"));
    }

    // ---- pages the build has ---------------------------------------------------

    /// A build with the English and German manual and part of the French.
    const BUILT: &[&str] = &[
        "/index.html",
        "/manual/index.html",
        "/manual/how-it-works/start-and-end-shortcuts/index.html",
        "/manual/de/index.html",
        "/manual/de/how-it-works/start-and-end-shortcuts/index.html",
        "/manual/de/übersicht/index.html",
        "/manual/fr/index.html",
        "/manual/_astro/print.css",
    ];

    fn built(key: &str) -> bool {
        BUILT.contains(&key)
    }

    #[test]
    fn the_app_serves_a_manual_file_only_when_the_build_has_it() {
        for path in [
            "/manual/",
            "/manual",
            "/manual/de/how-it-works/start-and-end-shortcuts/",
            "/manual/how-it-works/start-and-end-shortcuts",
            "/manual/_astro/print.css",
            "/manual/de/%C3%BCbersicht/",
        ] {
            assert!(served(path, built), "{path}");
        }
        for path in [
            "/manual/de/how-it-works/missing/",
            "/manual/es/",
            "/manual/_astro/missing.css",
            "/manual/de/%C3%BC/",
        ] {
            assert!(!served(path, built), "{path}");
        }
    }

    #[test]
    fn a_page_the_build_has_opens_as_asked() {
        for path in [
            "/manual/de/how-it-works/start-and-end-shortcuts/#the-end",
            "/manual/how-it-works/start-and-end-shortcuts/",
            "/manual/fr/",
        ] {
            assert_eq!(available_page(path, built).as_deref(), Some(path));
        }
    }

    #[test]
    fn a_page_the_build_lacks_opens_its_languages_home_page() {
        assert_eq!(
            available_page("/manual/fr/how-it-works/start-and-end-shortcuts/", built).as_deref(),
            Some("/manual/fr/")
        );
        assert_eq!(
            available_page("/manual/how-it-works/missing/#x", built).as_deref(),
            Some("/manual/")
        );
    }

    #[test]
    fn a_language_the_build_lacks_opens_the_english_home_page() {
        assert_eq!(
            available_page("/manual/es/how-it-works/start-and-end-shortcuts/", built).as_deref(),
            Some("/manual/")
        );
        assert_eq!(
            available_page("/manual/es/", built).as_deref(),
            Some("/manual/")
        );
    }

    #[test]
    fn without_a_manual_in_the_build_nothing_opens() {
        assert_eq!(
            available_page("/manual/de/", |key| key == "/index.html"),
            None
        );
    }

    #[test]
    fn a_link_to_a_page_the_build_lacks_is_blocked() {
        let exists: &dyn Fn(&str) -> bool = &built;
        let check = |link: &str| {
            let link = url(link);
            checked_link(&link, manual_link(&link, &mac()), Some(exists))
        };
        assert_eq!(
            check("tauri://localhost/manual/de/how-it-works/start-and-end-shortcuts/"),
            ManualLink::Stay
        );
        assert_eq!(
            check("tauri://localhost/manual/de/missing/"),
            ManualLink::Block
        );
        assert_eq!(check("about:blank"), ManualLink::Stay);
        assert_eq!(check("https://example.com/"), ManualLink::OpenInBrowser);
        assert_eq!(check("tauri://localhost/"), ManualLink::Block);
    }

    #[test]
    fn under_tauri_dev_every_page_may_load() {
        let link = url("http://localhost:1420/manual/de/missing/");
        assert_eq!(
            checked_link(&link, manual_link(&link, &dev()), None),
            ManualLink::Stay
        );
    }

    // ---- the theme -----------------------------------------------------------

    #[test]
    fn starlight_stores_light_dark_or_nothing_for_the_system() {
        assert_eq!(starlight_theme(Theme::Light), "light");
        assert_eq!(starlight_theme(Theme::Dark), "dark");
        assert_eq!(starlight_theme(Theme::System), "");
    }

    #[test]
    fn the_open_page_gets_the_value_starlight_stores() {
        for theme in [Theme::Light, Theme::Dark, Theme::System] {
            let script = theme_script(theme);
            assert!(
                script.contains(&format!("const theme = \"{}\";", starlight_theme(theme))),
                "{theme:?}: {script}"
            );
            assert!(script.contains("localStorage.setItem(\"starlight-theme\", theme)"));
            assert!(script.contains("document.documentElement.dataset.theme"));
            assert!(script.contains("starlight-theme-select select"));
        }
    }
}
