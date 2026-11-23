//! DanyWhere: the macOS side of the input method: the link to it, its
//! input source, installing and updating it
//! (`src-tauri/macos/input-method/PROTOCOL.md`).
//!
//! - [`messages`]: the protocol's messages, one JSON object per line.
//! - [`paths`]: the bundled input method, its installed copy, the socket.
//! - [`link`]: the socket server and its one connection; [`peer_check`]
//!   checks who connects by code signature.
//! - [`sources`]: macOS's input sources (TIS), on the main thread.
//! - [`install`]: copying, updating, ending the old process, System Settings.
//! - [`app`]: [`app::AppInputMethod`], all of it behind `InputMethod`.
//!
//! [`start`] puts it together at startup.

pub mod app;
mod cf;
pub mod install;
pub mod link;
pub mod messages;
pub mod paths;
pub mod peer_check;
pub mod sources;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Manager};

use app::{AppInputMethod, Timeouts};
use install::MacHost;
use link::Link;
use paths::BundledInputMethod;
use peer_check::CodeSignaturePeerCheck;
use sources::CarbonInputSources;

/// How often the link checks that its socket file is still there.
const LINK_RECHECK: Duration = Duration::from_secs(60);

/// At startup, after the app handle exists: if the app carries an input
/// method, starts the link, updates an installed copy that differs, undoes
/// a selection left over after a crash, and installs it as
/// `input_method::current()`, all on a thread of its own. A build without
/// one installs nothing, so `current()` stays `NotAvailable`.
pub fn start(app: &AppHandle) {
    let app_id = app.config().identifier.clone();
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|exe| paths::helpers_dir(&exe))
        .and_then(|helpers| paths::find_bundled(&helpers, &app_id));
    let Some(bundled) = bundled else {
        log::info!("Input method: this build carries none; inserting as before");
        return;
    };
    let home = match app.path().home_dir() {
        Ok(home) => home,
        Err(e) => {
            log::warn!("Input method: no home folder ({e}); not started");
            return;
        }
    };
    log::info!(
        "Input method: bundled {} version {} ({})",
        bundled.id,
        bundled.version,
        bundled.file_name
    );
    let app = app.clone();
    if let Err(e) = std::thread::Builder::new()
        .name("input-method-start".into())
        .spawn(move || start_in_background(app, app_id, home, bundled))
    {
        log::error!("Input method: no thread to start on: {e}");
    }
}

fn start_in_background(app: AppHandle, app_id: String, home: PathBuf, bundled: BundledInputMethod) {
    let socket = paths::socket_path(&home, &app_id, paths::darwin_user_temp_dir);
    let peer_check = Arc::new(CodeSignaturePeerCheck::for_peer(&bundled.id));
    let link = Link::start(socket, peer_check, LINK_RECHECK);
    let installed = paths::installed_path(&home, &bundled.file_name);
    let input_method = AppInputMethod::new(
        Some(bundled),
        installed,
        Arc::new(CarbonInputSources::new(app)),
        link,
        Arc::new(MacHost),
        Timeouts::default(),
    );
    match input_method.update_installed_copy() {
        Ok(true) => log::info!("Input method: the installed copy was updated"),
        Ok(false) => {}
        Err(e) => log::warn!("Input method: updating the installed copy failed: {e}"),
    }
    input_method.leave_leftover_selection();
    log::info!(
        "Input method: ready ({:?})",
        crate::input_method::InputMethod::status(&input_method)
    );
    crate::input_method::install(Arc::new(input_method));
}
