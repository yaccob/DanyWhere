//! DanyWhere: the link to the input method (PROTOCOL.md, "The link"): a Unix
//! domain stream socket the app listens on and the input method connects
//! to. Every connection's peer is checked before anything is read or
//! written; one connection at a time, a new verified one replaces the old.

use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::net::Shutdown;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use super::messages::{parse_line, to_line, FromInputMethod, ToInputMethod};

/// How long a write to the input method may block.
const WRITE_TIMEOUT: Duration = Duration::from_secs(1);
/// How often the listening thread looks up from waiting for a connection.
const POLL: Duration = Duration::from_millis(250);

/// Checks the process at the other end of a new connection.
pub trait PeerCheck: Send + Sync {
    /// `Ok` lets the connection in; `Err` says why not (for the log).
    fn verify(&self, stream: &UnixStream) -> Result<(), String>;
}

/// What the input method said in its `hello`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    pub version: String,
    pub pid: Option<i32>,
}

/// The input method's answer to an `insert`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertAnswer {
    /// Inserted into `client`'s text field.
    Inserted { client: Option<String> },
    /// No text field has the input method.
    NoClient,
    /// Another failure, with the input method's reason.
    Error(String),
}

/// Why a message could not go over the link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    /// No input method is connected.
    NotConnected,
    /// No answer in time.
    Timeout,
    /// The connection broke or was replaced before the answer came.
    Disconnected,
    /// Writing failed.
    Io(String),
}

/// The link as [`super::app::AppInputMethod`] uses it (a fake in its tests).
pub trait ImLink: Send + Sync {
    /// A verified input method is connected.
    fn connected(&self) -> bool;
    /// Waits up to `timeout` for a connection; whether there is one.
    fn wait_connected(&self, timeout: Duration) -> bool;
    /// Waits up to `timeout` for the connection to end; whether it did.
    fn wait_disconnected(&self, timeout: Duration) -> bool;
    /// The application whose text field has the input method, if any.
    fn active_client(&self) -> Option<String>;
    /// Waits up to `timeout` for a text field to have the input method.
    fn wait_active(&self, timeout: Duration) -> bool;
    /// The keyboard layout for typing (`layout`), sent now and to every
    /// later connection.
    fn set_layout(&self, source: Option<String>);
    /// The source to go back to if the link breaks (`restore`), sent now and
    /// to every later connection.
    fn set_restore(&self, source: Option<String>);
    /// Sends `insert` and waits up to `timeout` for its `inserted`.
    fn insert(&self, text: &str, timeout: Duration) -> Result<InsertAnswer, LinkError>;
    /// Asks the input method to quit; whether the message went out.
    fn quit(&self) -> bool;
}

/// What the link knows, shared by its threads.
#[derive(Default)]
struct State {
    /// The verified connection, if any.
    connection: Option<UnixStream>,
    /// Counts connections, so a replaced one's reader leaves the state alone.
    generation: u64,
    hello: Option<Hello>,
    active: Option<String>,
    /// Inserts waiting for their answer, by request id.
    pending: HashMap<u64, Option<InsertAnswer>>,
    next_id: u64,
    /// The standing `layout` and `restore`, sent to every new connection.
    layout: Option<String>,
    restore: Option<String>,
}

impl State {
    fn write(&mut self, message: &ToInputMethod) -> Result<(), LinkError> {
        let Some(stream) = self.connection.as_mut() else {
            return Err(LinkError::NotConnected);
        };
        stream
            .write_all(to_line(message).as_bytes())
            .map_err(|e| LinkError::Io(e.to_string()))
    }

    /// Sends a standing message if connected; logs a failure.
    fn write_if_connected(&mut self, message: &ToInputMethod) {
        if self.connection.is_some() {
            if let Err(e) = self.write(message) {
                log::warn!("Input method link: sending failed: {e:?}");
            }
        }
    }

    fn apply(&mut self, message: FromInputMethod) {
        match message {
            FromInputMethod::Hello { version, pid } => {
                log::info!("Input method link: hello from version {version}, pid {pid:?}");
                self.hello = Some(Hello { version, pid });
            }
            FromInputMethod::Active { client } => {
                let client = client.unwrap_or_default();
                log::debug!("Input method link: active in {client}");
                self.active = Some(client);
            }
            FromInputMethod::Inactive { client } => {
                let client = client.unwrap_or_default();
                log::debug!("Input method link: inactive in {client}");
                if self.active.as_deref() == Some(client.as_str()) {
                    self.active = None;
                }
            }
            FromInputMethod::Inserted {
                id,
                ok,
                client,
                reason,
            } => {
                let answer = if ok {
                    InsertAnswer::Inserted { client }
                } else if reason.as_deref() == Some("no_client") {
                    InsertAnswer::NoClient
                } else {
                    InsertAnswer::Error(reason.unwrap_or_else(|| "error".into()))
                };
                match self.pending.get_mut(&id) {
                    Some(slot) => *slot = Some(answer),
                    None => log::debug!("Input method link: a late answer to #{id}"),
                }
            }
            FromInputMethod::Caught { id, code, mods } => {
                log::debug!("Input method link: caught #{id} (key {code}, modifiers {mods})");
            }
            FromInputMethod::Unknown => {
                log::debug!("Input method link: ignored a message of an unknown type");
            }
        }
    }

    /// The connection ended: forget what belonged to it.
    fn disconnect(&mut self) {
        if let Some(stream) = self.connection.take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        self.hello = None;
        self.active = None;
    }
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    stop: AtomicBool,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Waits up to `timeout` until `done`; the state and whether it came.
    fn wait_until(
        &self,
        timeout: Duration,
        mut done: impl FnMut(&State) -> bool,
    ) -> (MutexGuard<'_, State>, bool) {
        let deadline = Instant::now() + timeout;
        let mut state = self.lock();
        loop {
            if done(&state) {
                return (state, true);
            }
            let now = Instant::now();
            if now >= deadline {
                return (state, false);
            }
            state = self
                .changed
                .wait_timeout(state, deadline - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

/// The app's end of the link: the listening socket and the one connection.
pub struct Link {
    #[cfg_attr(not(test), allow(dead_code))]
    path: PathBuf,
    shared: Arc<Shared>,
}

impl Link {
    /// Listens on `path` on threads of its own, checking every `recheck`
    /// that the socket file is still there (binding again if not). A failed
    /// bind is logged and tried again at the next check.
    pub fn start(path: PathBuf, peer_check: Arc<dyn PeerCheck>, recheck: Duration) -> Arc<Link> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                next_id: 1,
                ..State::default()
            }),
            changed: Condvar::new(),
            stop: AtomicBool::new(false),
        });
        let bound = match bind(&path) {
            Ok(bound) => {
                log::info!("Input method link: listening at {}", path.display());
                Some(bound)
            }
            Err(e) => {
                log::warn!(
                    "Input method link: can't listen at {}: {e}; trying again later",
                    path.display()
                );
                None
            }
        };
        let server = Server {
            path: path.clone(),
            shared: shared.clone(),
            peer_check,
            recheck,
            last_refusal: None,
        };
        if let Err(e) = std::thread::Builder::new()
            .name("input-method-link".into())
            .spawn(move || server.run(bound))
        {
            log::error!("Input method link: no thread to listen on: {e}");
        }
        Arc::new(Link { path, shared })
    }

    /// The socket's path.
    #[cfg(test)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// What the connected input method said in its `hello`.
    #[cfg(test)]
    pub fn hello(&self) -> Option<Hello> {
        self.shared.lock().hello.clone()
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.lock().disconnect();
        self.shared.changed.notify_all();
    }
}

impl ImLink for Link {
    fn connected(&self) -> bool {
        self.shared.lock().connection.is_some()
    }

    fn wait_connected(&self, timeout: Duration) -> bool {
        self.shared
            .wait_until(timeout, |state| state.connection.is_some())
            .1
    }

    fn wait_disconnected(&self, timeout: Duration) -> bool {
        self.shared
            .wait_until(timeout, |state| state.connection.is_none())
            .1
    }

    fn active_client(&self) -> Option<String> {
        self.shared.lock().active.clone()
    }

    fn wait_active(&self, timeout: Duration) -> bool {
        self.shared
            .wait_until(timeout, |state| {
                state.connection.is_some() && state.active.is_some()
            })
            .1
    }

    fn set_layout(&self, source: Option<String>) {
        let mut state = self.shared.lock();
        state.layout = source.clone();
        state.write_if_connected(&ToInputMethod::Layout { source });
    }

    fn set_restore(&self, source: Option<String>) {
        let mut state = self.shared.lock();
        state.restore = source.clone();
        state.write_if_connected(&ToInputMethod::Restore { source });
    }

    fn insert(&self, text: &str, timeout: Duration) -> Result<InsertAnswer, LinkError> {
        let (id, generation) = {
            let mut state = self.shared.lock();
            if state.connection.is_none() {
                return Err(LinkError::NotConnected);
            }
            let id = state.next_id;
            state.next_id += 1;
            state.write(&ToInputMethod::Insert {
                id,
                text: text.to_string(),
            })?;
            state.pending.insert(id, None);
            (id, state.generation)
        };
        log::debug!(
            "Input method link: insert #{id} ({} characters)",
            text.chars().count()
        );
        let (mut state, _) = self.shared.wait_until(timeout, |state| {
            state.generation != generation
                || state.connection.is_none()
                || matches!(state.pending.get(&id), Some(Some(_)))
        });
        match state.pending.remove(&id).flatten() {
            Some(answer) => Ok(answer),
            None if state.generation != generation || state.connection.is_none() => {
                Err(LinkError::Disconnected)
            }
            None => Err(LinkError::Timeout),
        }
    }

    fn quit(&self) -> bool {
        let mut state = self.shared.lock();
        state.connection.is_some() && state.write(&ToInputMethod::Quit).is_ok()
    }
}

/// Binds a fresh socket at `path`: its folder `0700`, a stale file removed,
/// the socket `0600`. The listener and the socket file's inode.
fn bind(path: &Path) -> io::Result<(UnixListener, u64)> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other("the socket path has no folder"))?;
    fs::create_dir_all(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    match fs::remove_file(path) {
        Ok(()) => log::debug!("Input method link: removed a stale socket file"),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    let inode = fs::symlink_metadata(path)?.ino();
    Ok((listener, inode))
}

/// The socket file this link bound is still at `path`.
fn still_bound(path: &Path, inode: u64) -> bool {
    fs::symlink_metadata(path)
        .map(|meta| meta.file_type().is_socket() && meta.ino() == inode)
        .unwrap_or(false)
}

/// Waits up to `timeout` for a connection to accept.
fn readable(listener: &UnixListener, timeout: Duration) -> bool {
    let mut poll = libc::pollfd {
        fd: listener.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let millis = libc::c_int::try_from(timeout.as_millis()).unwrap_or(libc::c_int::MAX);
    // SAFETY: one valid pollfd for the duration of the call.
    let ready = unsafe { libc::poll(&mut poll, 1, millis) };
    ready > 0 && poll.revents & libc::POLLIN != 0
}

/// The listening thread.
struct Server {
    path: PathBuf,
    shared: Arc<Shared>,
    peer_check: Arc<dyn PeerCheck>,
    recheck: Duration,
    /// Why the last connection was refused: the same reason again (the
    /// input method retries every 2 s) is logged only once.
    last_refusal: Option<String>,
}

impl Server {
    fn run(mut self, mut bound: Option<(UnixListener, u64)>) {
        let mut last_check = Instant::now();
        let mut bind_failing = bound.is_none();
        while !self.shared.stop.load(Ordering::SeqCst) {
            match &bound {
                Some((listener, _)) => {
                    if readable(listener, POLL.min(self.recheck)) {
                        match listener.accept() {
                            Ok((stream, _)) => self.take(stream),
                            Err(e) => log::debug!("Input method link: accept failed: {e}"),
                        }
                    }
                }
                None => std::thread::sleep(POLL.min(self.recheck)),
            }
            if last_check.elapsed() < self.recheck {
                continue;
            }
            last_check = Instant::now();
            if bound
                .as_ref()
                .is_some_and(|(_, inode)| still_bound(&self.path, *inode))
            {
                continue;
            }
            if bound.take().is_some() {
                log::info!("Input method link: the socket file is gone; binding again");
            }
            match bind(&self.path) {
                Ok(fresh) => {
                    log::info!("Input method link: listening at {}", self.path.display());
                    bound = Some(fresh);
                    bind_failing = false;
                }
                Err(e) => {
                    if !bind_failing {
                        log::warn!(
                            "Input method link: can't listen at {}: {e}",
                            self.path.display()
                        );
                    }
                    bind_failing = true;
                }
            }
        }
    }

    /// A new connection: checked, then it replaces the old one.
    fn take(&mut self, stream: UnixStream) {
        let _ = stream.set_nonblocking(false);
        if let Err(reason) = self.peer_check.verify(&stream) {
            if self.last_refusal.as_ref() == Some(&reason) {
                log::debug!("Input method link: refused a connection: {reason}");
            } else {
                log::warn!("Input method link: refused a connection: {reason}");
                self.last_refusal = Some(reason);
            }
            let _ = stream.shutdown(Shutdown::Both);
            return;
        }
        self.last_refusal = None;
        let reader = match stream.try_clone() {
            Ok(reader) => reader,
            Err(e) => {
                log::warn!("Input method link: connection not usable: {e}");
                return;
            }
        };
        let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
        let generation = {
            let mut state = self.shared.lock();
            if state.connection.is_some() {
                log::info!("Input method link: a new connection replaces the old one");
            }
            state.disconnect();
            state.generation += 1;
            state.connection = Some(stream);
            let layout = ToInputMethod::Layout {
                source: state.layout.clone(),
            };
            let restore = ToInputMethod::Restore {
                source: state.restore.clone(),
            };
            state.write_if_connected(&layout);
            state.write_if_connected(&restore);
            self.shared.changed.notify_all();
            state.generation
        };
        log::info!("Input method link: the input method connected");
        let shared = self.shared.clone();
        if let Err(e) = std::thread::Builder::new()
            .name("input-method-link-reader".into())
            .spawn(move || read_loop(shared, reader, generation))
        {
            log::error!("Input method link: no thread to read on: {e}");
            self.shared.lock().disconnect();
        }
    }
}

/// Reads the input method's messages until its connection ends.
fn read_loop(shared: Arc<Shared>, stream: UnixStream, generation: u64) {
    let mut reader = BufReader::new(stream);
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&line);
        let Some(message) = parse_line(&text) else {
            if !text.trim().is_empty() {
                log::debug!("Input method link: ignored a line that is no message");
            }
            continue;
        };
        let mut state = shared.lock();
        if state.generation != generation {
            return;
        }
        state.apply(message);
        shared.changed.notify_all();
    }
    let mut state = shared.lock();
    if state.generation == generation && state.connection.is_some() {
        state.disconnect();
        shared.changed.notify_all();
        log::info!("Input method link: the input method disconnected");
    }
}

#[cfg(test)]
pub mod testing {
    use super::PeerCheck;
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Lets every peer in.
    pub struct AllowAll;

    impl PeerCheck for AllowAll {
        fn verify(&self, _stream: &UnixStream) -> Result<(), String> {
            Ok(())
        }
    }

    /// Lets the first `n` peers in, refuses the rest.
    pub struct AllowFirst(pub usize, pub AtomicUsize);

    impl AllowFirst {
        pub fn new(n: usize) -> Self {
            AllowFirst(n, AtomicUsize::new(0))
        }
    }

    impl PeerCheck for AllowFirst {
        fn verify(&self, _stream: &UnixStream) -> Result<(), String> {
            if self.1.fetch_add(1, Ordering::SeqCst) < self.0 {
                Ok(())
            } else {
                Err("not the expected input method".into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{AllowAll, AllowFirst};
    use super::*;
    use serde_json::{json, Value};
    use std::fs;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    use std::time::Instant;

    const WAIT: Duration = Duration::from_secs(3);

    fn wait_until(mut condition: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline {
            if condition() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        condition()
    }

    /// The input method's end, played by the test.
    struct FakeIm {
        reader: BufReader<UnixStream>,
        writer: UnixStream,
    }

    impl FakeIm {
        fn connect(path: &Path) -> FakeIm {
            let mut stream = None;
            assert!(
                wait_until(|| {
                    stream = UnixStream::connect(path).ok();
                    stream.is_some()
                }),
                "could not connect to {path:?}"
            );
            let stream = stream.unwrap();
            stream.set_read_timeout(Some(WAIT)).unwrap();
            FakeIm {
                reader: BufReader::new(stream.try_clone().unwrap()),
                writer: stream,
            }
        }

        /// Connects, reads the standing `layout` and `restore` and says hello.
        fn greet(link: &Link, version: &str, pid: i32) -> FakeIm {
            let mut im = FakeIm::connect(link.path());
            assert_eq!(im.read()["type"], "layout");
            assert_eq!(im.read()["type"], "restore");
            im.send(&json!({"type": "hello", "version": version, "pid": pid}).to_string());
            let expected = Hello {
                version: version.into(),
                pid: Some(pid),
            };
            assert!(wait_until(|| link.hello().as_ref() == Some(&expected)));
            im
        }

        fn send(&mut self, line: &str) {
            self.writer.write_all(line.as_bytes()).unwrap();
            self.writer.write_all(b"\n").unwrap();
        }

        fn read(&mut self) -> Value {
            let mut line = String::new();
            let n = self.reader.read_line(&mut line).expect("a message in time");
            assert!(n > 0, "the link closed");
            serde_json::from_str(line.trim_end()).expect("JSON")
        }

        /// What came until the app closed this connection; `None` if it
        /// didn't close it in time.
        fn closed(&mut self) -> Option<Vec<u8>> {
            let mut rest = Vec::new();
            self.reader.read_to_end(&mut rest).ok().map(|_| rest)
        }
    }

    fn start(dir: &Path) -> Arc<Link> {
        Link::start(
            dir.join("app/input-method.sock"),
            Arc::new(AllowAll),
            Duration::from_secs(60),
        )
    }

    #[test]
    fn it_listens_in_a_private_folder() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app/input-method.sock");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"stale").unwrap();

        let link = Link::start(path.clone(), Arc::new(AllowAll), Duration::from_secs(60));

        assert!(wait_until(|| fs::symlink_metadata(&path)
            .map(|meta| meta.file_type().is_socket())
            .unwrap_or(false)));
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        assert_eq!(mode(&path), 0o600);
        assert!(!link.connected());
    }

    #[test]
    fn hello_is_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let link = start(dir.path());
        let _im = FakeIm::greet(&link, "0.3.3", 123);
        assert!(link.connected());
        assert!(link.wait_connected(Duration::ZERO));
    }

    #[test]
    fn active_and_inactive_track_the_client() {
        let dir = tempfile::tempdir().unwrap();
        let link = start(dir.path());
        let mut im = FakeIm::greet(&link, "0.3.3", 1);
        assert_eq!(link.active_client(), None);
        assert!(!link.wait_active(Duration::from_millis(50)));

        im.send(r#"{"type":"active","client":"com.apple.TextEdit"}"#);
        assert!(link.wait_active(WAIT));
        assert_eq!(link.active_client().as_deref(), Some("com.apple.TextEdit"));

        im.send(r#"{"type":"inactive","client":"com.apple.Safari"}"#);
        im.send(r#"{"type":"active","client":"com.apple.Notes"}"#);
        assert!(wait_until(
            || link.active_client().as_deref() == Some("com.apple.Notes")
        ));
        im.send(r#"{"type":"inactive","client":"com.apple.TextEdit"}"#);
        im.send(r#"{"type":"hello","version":"0.3.3","pid":2}"#);
        assert!(wait_until(|| link.hello().and_then(|h| h.pid) == Some(2)));
        assert_eq!(
            link.active_client().as_deref(),
            Some("com.apple.Notes"),
            "another app's inactive leaves the active one"
        );
        im.send(r#"{"type":"inactive","client":"com.apple.Notes"}"#);
        assert!(wait_until(|| link.active_client().is_none()));
    }

    /// The fake input method answers the next `insert` with `answer`
    /// (its `id` filled in) and hands back what it received.
    fn answer_next_insert(mut im: FakeIm, answer: Value) -> std::thread::JoinHandle<Value> {
        std::thread::spawn(move || {
            let request = im.read();
            let mut answer = answer;
            answer["id"] = request["id"].clone();
            im.send(&answer.to_string());
            // Keep the connection until the test has its answer.
            let _ = im.read_line_or_eof();
            request
        })
    }

    impl FakeIm {
        fn read_line_or_eof(&mut self) -> Option<String> {
            let mut line = String::new();
            match self.reader.read_line(&mut line) {
                Ok(0) | Err(_) => None,
                Ok(_) => Some(line),
            }
        }
    }

    #[test]
    fn an_insert_gets_its_answer() {
        let dir = tempfile::tempdir().unwrap();
        let link = start(dir.path());
        let im = FakeIm::greet(&link, "0.3.3", 1);
        let fake = answer_next_insert(
            im,
            json!({"type": "inserted", "ok": true, "client": "com.apple.TextEdit"}),
        );

        let answer = link.insert("Hallo Welt", WAIT);

        assert_eq!(
            answer,
            Ok(InsertAnswer::Inserted {
                client: Some("com.apple.TextEdit".into())
            })
        );
        drop(link);
        let request = fake.join().unwrap();
        assert_eq!(request["type"], "insert");
        assert_eq!(request["text"], "Hallo Welt");
        assert!(request["id"].as_u64().is_some());
    }

    #[test]
    fn requests_get_new_ids() {
        let dir = tempfile::tempdir().unwrap();
        let link = start(dir.path());
        let im = FakeIm::greet(&link, "0.3.3", 1);
        let first = answer_next_insert(im, json!({"type": "inserted", "ok": true}));
        assert!(link.insert("a", WAIT).is_ok());
        // The fake still holds the connection; a new one replaces it.
        let im = FakeIm::greet(&link, "0.3.3", 2);
        let second = answer_next_insert(im, json!({"type": "inserted", "ok": true}));
        assert!(link.insert("b", WAIT).is_ok());
        drop(link);
        let (a, b) = (first.join().unwrap(), second.join().unwrap());
        assert_ne!(a["id"], b["id"]);
    }

    #[test]
    fn no_text_field_is_no_client() {
        let dir = tempfile::tempdir().unwrap();
        let link = start(dir.path());
        let im = FakeIm::greet(&link, "0.3.3", 1);
        let _fake = answer_next_insert(
            im,
            json!({"type": "inserted", "ok": false, "reason": "no_client"}),
        );
        assert_eq!(link.insert("x", WAIT), Ok(InsertAnswer::NoClient));
    }

    #[test]
    fn another_failure_keeps_its_reason() {
        let dir = tempfile::tempdir().unwrap();
        let link = start(dir.path());
        let im = FakeIm::greet(&link, "0.3.3", 1);
        let _fake = answer_next_insert(
            im,
            json!({"type": "inserted", "ok": false, "reason": "error"}),
        );
        assert_eq!(
            link.insert("x", WAIT),
            Ok(InsertAnswer::Error("error".into()))
        );
    }

    #[test]
    fn an_insert_without_an_answer_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let link = start(dir.path());
        let mut im = FakeIm::greet(&link, "0.3.3", 1);
        let started = Instant::now();
        assert_eq!(
            link.insert("x", Duration::from_millis(300)),
            Err(LinkError::Timeout)
        );
        let took = started.elapsed();
        assert!(took >= Duration::from_millis(300), "{took:?}");
        assert!(took < Duration::from_millis(1500), "{took:?}");
        assert_eq!(im.read()["type"], "insert");
        // A late answer changes nothing.
        im.send(r#"{"type":"inserted","id":1,"ok":true}"#);
        assert!(link.connected());
    }

    #[test]
    fn an_insert_without_a_connection_fails_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let link = start(dir.path());
        let started = Instant::now();
        assert_eq!(link.insert("x", WAIT), Err(LinkError::NotConnected));
        assert!(started.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn a_pending_insert_ends_when_the_link_breaks() {
        let dir = tempfile::tempdir().unwrap();
        let link = start(dir.path());
        let mut im = FakeIm::greet(&link, "0.3.3", 1);
        let breaker = std::thread::spawn(move || {
            assert_eq!(im.read()["type"], "insert");
            drop(im);
        });
        let started = Instant::now();
        assert_eq!(link.insert("x", WAIT), Err(LinkError::Disconnected));
        assert!(started.elapsed() < Duration::from_secs(2));
        breaker.join().unwrap();
    }

    #[test]
    fn a_closed_connection_is_noticed_and_a_new_one_taken() {
        let dir = tempfile::tempdir().unwrap();
        let link = start(dir.path());
        let im = FakeIm::greet(&link, "0.3.3", 1);
        drop(im);
        assert!(link.wait_disconnected(WAIT));
        assert!(!link.connected());
        assert_eq!(link.hello(), None);

        let _im = FakeIm::greet(&link, "0.3.4", 2);
        assert!(link.connected());
        assert_eq!(
            link.hello(),
            Some(Hello {
                version: "0.3.4".into(),
                pid: Some(2)
            })
        );
    }

    #[test]
    fn a_second_connection_replaces_the_first() {
        let dir = tempfile::tempdir().unwrap();
        let link = start(dir.path());
        let mut first = FakeIm::greet(&link, "0.3.3", 1);
        first.send(r#"{"type":"active","client":"com.apple.TextEdit"}"#);
        assert!(link.wait_active(WAIT));

        let second = FakeIm::greet(&link, "0.3.3", 2);

        assert!(first.closed().is_some(), "the first connection is closed");
        assert!(link.connected());
        assert_eq!(link.active_client(), None, "the new one has no client yet");
        let fake = answer_next_insert(second, json!({"type": "inserted", "ok": true}));
        assert_eq!(
            link.insert("x", WAIT),
            Ok(InsertAnswer::Inserted { client: None })
        );
        drop(link);
        assert_eq!(fake.join().unwrap()["text"], "x");
    }

    #[test]
    fn unknown_messages_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let link = start(dir.path());
        let mut im = FakeIm::connect(link.path());
        im.read();
        im.read();
        im.send(r#"{"type":"stretch","x":1}"#);
        im.send("not json at all");
        im.send("");
        im.send(r#"{"type":"hello","version":"0.9.0","pid":7,"new":true}"#);
        assert!(wait_until(|| link.hello().is_some()));
        assert_eq!(link.hello().unwrap().version, "0.9.0");
        assert!(link.connected());
    }

    #[test]
    fn a_refused_peer_is_closed_and_the_link_kept() {
        let dir = tempfile::tempdir().unwrap();
        let link = Link::start(
            dir.path().join("app/input-method.sock"),
            Arc::new(AllowFirst::new(1)),
            Duration::from_secs(60),
        );
        let first = FakeIm::greet(&link, "0.3.3", 1);

        let mut intruder = FakeIm::connect(link.path());
        assert_eq!(
            intruder.closed(),
            Some(Vec::new()),
            "the refused peer is closed without a word"
        );

        assert!(link.connected());
        assert_eq!(link.hello().and_then(|h| h.pid), Some(1));
        let fake = answer_next_insert(first, json!({"type": "inserted", "ok": true}));
        assert!(link.insert("x", WAIT).is_ok());
        drop(link);
        fake.join().unwrap();
    }

    #[test]
    fn a_refused_peer_alone_is_no_connection() {
        let dir = tempfile::tempdir().unwrap();
        let link = Link::start(
            dir.path().join("app/input-method.sock"),
            Arc::new(AllowFirst::new(0)),
            Duration::from_secs(60),
        );
        let mut intruder = FakeIm::connect(link.path());
        assert_eq!(intruder.closed(), Some(Vec::new()));
        assert!(!link.connected());
    }

    #[test]
    fn layout_and_restore_reach_the_input_method_now_and_later() {
        let dir = tempfile::tempdir().unwrap();
        let link = start(dir.path());
        let mut im = FakeIm::connect(link.path());
        assert_eq!(im.read(), json!({"type": "layout", "source": null}));
        assert_eq!(im.read(), json!({"type": "restore", "source": null}));

        link.set_layout(Some("com.apple.keylayout.German".into()));
        link.set_restore(Some("com.apple.keylayout.German".into()));
        assert_eq!(
            im.read(),
            json!({"type": "layout", "source": "com.apple.keylayout.German"})
        );
        assert_eq!(
            im.read(),
            json!({"type": "restore", "source": "com.apple.keylayout.German"})
        );

        // A later connection (the input method started again) gets them too.
        drop(im);
        assert!(link.wait_disconnected(WAIT));
        let mut again = FakeIm::connect(link.path());
        assert_eq!(
            again.read(),
            json!({"type": "layout", "source": "com.apple.keylayout.German"})
        );
        assert_eq!(
            again.read(),
            json!({"type": "restore", "source": "com.apple.keylayout.German"})
        );

        link.set_restore(None);
        link.set_layout(None);
        assert_eq!(again.read(), json!({"type": "restore", "source": null}));
        assert_eq!(again.read(), json!({"type": "layout", "source": null}));
    }

    #[test]
    fn quit_is_sent() {
        let dir = tempfile::tempdir().unwrap();
        let link = start(dir.path());
        assert!(!link.quit(), "nothing to send it to");
        let mut im = FakeIm::greet(&link, "0.3.3", 1);
        assert!(link.quit());
        assert_eq!(im.read(), json!({"type": "quit"}));
    }

    #[test]
    fn it_binds_again_when_the_file_vanishes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app/input-method.sock");
        let link = Link::start(path.clone(), Arc::new(AllowAll), Duration::from_millis(100));
        let _first = FakeIm::greet(&link, "0.3.3", 1);

        fs::remove_file(&path).unwrap();
        assert!(wait_until(|| path.exists()), "bound again");

        let _second = FakeIm::greet(&link, "0.3.3", 2);
        assert!(link.connected());
    }

    #[test]
    fn a_failed_bind_is_tried_again() {
        let dir = tempfile::tempdir().unwrap();
        // A file where the folder should be: binding fails until it's gone.
        let blocker = dir.path().join("app");
        fs::write(&blocker, b"in the way").unwrap();
        let path = blocker.join("input-method.sock");
        let link = Link::start(path.clone(), Arc::new(AllowAll), Duration::from_millis(100));
        std::thread::sleep(Duration::from_millis(250));
        assert!(!path.exists());

        fs::remove_file(&blocker).unwrap();
        assert!(wait_until(|| path.exists()), "bound at a later check");
        let _im = FakeIm::greet(&link, "0.3.3", 1);
    }
}
