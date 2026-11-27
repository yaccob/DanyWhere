//! DanyWhere: the user's sound file, `sounds.toml` next to
//! `settings_store.json`.
//!
//! The feedback sounds are bundled with the app, so changing one needed a
//! rebuild. This file names, per sound theme and event, the sound to play
//! instead and can shift its pitch (sidecar note
//! `qa/2026-11-27_2115_vier-toene-und-tondatei.md`). It is read at every
//! sound, so a saved change applies to the next one. The app writes it with
//! every entry commented out when it is missing and never overwrites it; the
//! Debug page opens it.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use crate::audio_feedback::SoundType;
use crate::settings::SoundTheme;

/// The sound file's name in the app's data folder.
pub const FILE_NAME: &str = "sounds.toml";

/// What names a sound bundled with the app: `built-in:marimba_e5.wav`.
pub const BUILT_IN_PREFIX: &str = "built-in:";

/// How far `semitones` may shift a sound, either way: two octaves.
pub const MAX_SEMITONES: f64 = 24.0;

/// The sound themes, in the file's order.
pub const THEMES: [SoundTheme; 3] = [SoundTheme::Marimba, SoundTheme::Pop, SoundTheme::Custom];

/// The file an entry names, as written.
#[derive(Debug, Clone, PartialEq)]
pub enum FileRef {
    /// `built-in:<name>`: a sound bundled with the app.
    BuiltIn(String),
    /// A path: relative to the sound file's folder, under the home folder
    /// with `~/`, or absolute.
    Path(String),
}

/// What the file says for one event of one theme.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Entry {
    /// The sound to play; `None`: the built-in one.
    pub file: Option<FileRef>,
    /// How many semitones higher it plays (lower when negative).
    pub semitones: f32,
}

/// The entries of a sound file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SoundFile {
    entries: Vec<(SoundTheme, SoundType, Entry)>,
}

impl SoundFile {
    /// The entry for `sound_type` in `theme`, if the file has one.
    pub fn entry(&self, theme: SoundTheme, sound_type: SoundType) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|(t, s, _)| *t == theme && *s == sound_type)
            .map(|(_, _, entry)| entry)
    }
}

/// A theme's section name in the file.
pub fn theme_name(theme: SoundTheme) -> &'static str {
    match theme {
        SoundTheme::Marimba => "marimba",
        SoundTheme::Pop => "pop",
        SoundTheme::Custom => "custom",
    }
}

/// Reads a sound file's text: the entries it could read, and one line for
/// each thing it couldn't, which then plays the built-in sound.
pub fn parse(text: &str) -> (SoundFile, Vec<String>) {
    let mut file = SoundFile::default();
    let mut problems = Vec::new();
    let table: toml::Table = match text.parse() {
        Ok(table) => table,
        Err(error) => {
            let why = error
                .to_string()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            return (file, vec![format!("can't be read as TOML: {why}")]);
        }
    };
    for (section, value) in &table {
        let Some(theme) = THEMES
            .into_iter()
            .find(|theme| theme_name(*theme) == section)
        else {
            problems.push(format!(
                "[{section}]: no such sound theme (marimba, pop, custom)"
            ));
            continue;
        };
        let Some(events) = value.as_table() else {
            problems.push(format!("{section}: must be a section, [{section}]"));
            continue;
        };
        for (event, value) in events {
            let Some(sound_type) = SoundType::ALL
                .into_iter()
                .find(|sound_type| sound_type.event_name() == event)
            else {
                let known: Vec<&str> = SoundType::ALL.map(SoundType::event_name).to_vec();
                problems.push(format!(
                    "[{section}] {event}: no such event ({})",
                    known.join(", ")
                ));
                continue;
            };
            match entry(value) {
                Ok(entry) => file.entries.push((theme, sound_type, entry)),
                Err(why) => problems.push(format!("[{section}] {event}: {why}")),
            }
        }
    }
    (file, problems)
}

/// One event's entry: a file in quotes, or a table of `file` and
/// `semitones`.
fn entry(value: &toml::Value) -> Result<Entry, String> {
    match value {
        toml::Value::String(written) => Ok(Entry {
            file: Some(file_ref(written)?),
            semitones: 0.0,
        }),
        toml::Value::Table(table) => {
            let mut entry = Entry::default();
            for (key, value) in table {
                match key.as_str() {
                    "file" => {
                        let written = value.as_str().ok_or("the file must be written in quotes")?;
                        entry.file = Some(file_ref(written)?);
                    }
                    "semitones" => {
                        let semitones = value
                            .as_float()
                            .or_else(|| value.as_integer().map(|n| n as f64))
                            .ok_or("semitones must be a number")?;
                        if !(semitones.is_finite() && semitones.abs() <= MAX_SEMITONES) {
                            return Err(format!(
                                "semitones must be between -24 and 24, not {semitones}"
                            ));
                        }
                        entry.semitones = semitones as f32;
                    }
                    other => {
                        return Err(format!("no such key '{other}' (file, semitones)"));
                    }
                }
            }
            Ok(entry)
        }
        _ => Err(
            "must be a file in quotes, or a table such as { file = \"my.wav\", semitones = 2 }"
                .to_string(),
        ),
    }
}

/// The file an entry names: `built-in:` and the name of a bundled sound,
/// or a path.
fn file_ref(written: &str) -> Result<FileRef, String> {
    match written.strip_prefix(BUILT_IN_PREFIX) {
        Some(name) => {
            let plain = !name.is_empty() && !name.starts_with('.') && !name.contains(['/', '\\']);
            if plain {
                Ok(FileRef::BuiltIn(name.to_string()))
            } else {
                Err(format!(
                    "\"{written}\": a built-in sound is named without a folder, e.g. built-in:marimba_e5.wav"
                ))
            }
        }
        None if written.trim().is_empty() => Err("the file is empty".to_string()),
        None => Ok(FileRef::Path(written.to_string())),
    }
}

const HEADER: &str = "\
# DanyWhere sound file
#
# This file changes DanyWhere's feedback sounds without a rebuild. Each
# sound theme has a section below, and each event in it names the sound it
# plays. DanyWhere reads this file at every sound, so a saved change applies
# to the next one, without a restart.
#
# Every entry below is commented out and shows the built-in sound. To change
# one, remove the \"# \" in front of it and edit it. Entries left commented
# out keep following the app, so an update can still change them. The app
# never overwrites this file; to go back to the built-in sounds, delete it,
# and the app writes a fresh copy at its next start.
#
# The events, in the order a dictation plays them, each higher than the one
# before, and the cancel lowest of all:
#   start        the recording starts
#   instruction  an instruction shortcut switches to the spoken instruction
#   stop         the recording ends
#   done         the dictation is done, post-processing included, and its
#                text delivered (with Sound When Done on)
#   waiting      the text waits on the clipboard for you to paste it
#   cancel       a dictation is cancelled (Escape, the overlay's X, the
#                tray's Cancel)
#
# An entry is a file, or a table with a file and a pitch shift:
#   stop = \"my-stop.wav\"
#   stop = { file = \"my-stop.wav\", semitones = -2 }
#   stop = { semitones = 2 }      # the built-in sound, a whole tone higher
# A file is WAV, FLAC, MP3 or Ogg Vorbis. A relative path starts in this
# file's folder, \"~/\" in your home folder, and \"built-in:\" names a sound
# that comes with the app. The built-in sounds are named after their pitch:
# marimba_gsharp5.wav plays G#5. \"semitones\" shifts the pitch as a faster
# or slower playback would: 12 is an octave higher and half as long, -12 an
# octave lower and twice as long; from -24 to 24, fractions allowed.
#
# Which theme plays is a setting of the defaults and of each profile (Sound
# Theme, under Advanced in debug mode); out of the box it is Marimba. Audio
# Feedback switches all of the sounds on and off.
#
# Whatever can't be read here, and a file that isn't there, plays the
# built-in sound instead, and the log says why.
";

/// The file that ships when it is missing: the header, then every event of
/// every theme with its built-in sound, commented out. Written from the
/// table the app plays by (`audio_feedback::default_written`), so it can't
/// drift from it.
pub fn template() -> String {
    let mut out = String::from(HEADER);
    for theme in THEMES {
        out.push_str(&format!("\n[{}]\n", theme_name(theme)));
        if theme == SoundTheme::Custom {
            out.push_str(
                "# Your own sounds. The theme is offered once its start and stop sounds\n\
                 # are there.\n",
            );
        }
        for sound_type in SoundType::ALL {
            let (written, instead) = crate::audio_feedback::default_written(theme, sound_type);
            let line = format!("# {:<11} = \"{written}\"", sound_type.event_name());
            match instead {
                Some(instead) => out.push_str(&format!("{line}   # without it: {instead}\n")),
                None => out.push_str(&format!("{line}\n")),
            }
        }
    }
    out
}

/// Creates the sound file from the template when it is missing, along with
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

/// Logs each problem of the sound file once, not at every sound, until the
/// file's text changes.
#[derive(Debug, Default)]
pub struct ProblemLog {
    state: Mutex<(String, BTreeSet<String>)>,
    /// How many warnings were logged; for the tests.
    #[cfg_attr(not(test), allow(dead_code))]
    logged: Mutex<usize>,
}

impl ProblemLog {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new((String::new(), BTreeSet::new())),
            logged: Mutex::new(0),
        }
    }

    /// The file now reads `text`: a problem of another text is logged again.
    fn read(&self, text: &str) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.0 != text {
            state.0 = text.to_string();
            state.1.clear();
        }
    }

    /// Logs `problem` of the file at `path`, unless it was logged for the
    /// text read last.
    fn warn(&self, path: &Path, problem: &str) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.1.insert(problem.to_string()) {
            log::warn!(
                "Sound file {}: {problem}; the built-in sound plays",
                path.display()
            );
            *self.logged.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        }
    }
}

/// The problems of the app's sound file.
static PROBLEMS: ProblemLog = ProblemLog::new();

/// Reads the sound file at `path`; problems go to `log`. No file, or one
/// that can't be read, has no entries.
fn read_from(path: &Path, log: &ProblemLog) -> SoundFile {
    let text = match fs::read(path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            log.read("");
            return SoundFile::default();
        }
        Err(error) => {
            log.read("");
            log.warn(path, &format!("can't be read: {error}"));
            return SoundFile::default();
        }
    };
    log.read(&text);
    let (file, problems) = parse(&text);
    for problem in problems {
        log.warn(path, &problem);
    }
    file
}

/// The sound file of this app (portable-aware, next to the settings).
pub fn path(app: &AppHandle) -> Result<PathBuf, tauri::Error> {
    crate::portable::resolve_app_data(app, FILE_NAME)
}

/// The app's sound file as it is now.
pub fn current(app: &AppHandle) -> SoundFile {
    match path(app) {
        Ok(path) => read_from(&path, &PROBLEMS),
        Err(_) => SoundFile::default(),
    }
}

/// Logs a problem found while following the sound file (a file it names is
/// missing), once until the file changes.
pub fn warn(app: &AppHandle, problem: &str) {
    if let Ok(path) = path(app) {
        PROBLEMS.warn(&path, problem);
    }
}

/// Writes the sound file if it is missing. Called once at startup; a file
/// deleted later is written again at the next start or when the Debug page
/// opens it.
pub fn init(app: &AppHandle) {
    let path = match path(app) {
        Ok(path) => path,
        Err(error) => {
            log::warn!("No folder for the sound file, using the built-in sounds: {error}");
            return;
        }
    };
    match ensure_exists(&path) {
        Ok(true) => log::info!("Wrote the sound file {}", path.display()),
        Ok(false) => log::debug!("Sound file {}", path.display()),
        Err(error) => log::warn!("Can't write the sound file {}: {error}", path.display()),
    }
}

/// Opens the sound file in the system's default app for it, writing it
/// first if it is missing (Debug page).
#[tauri::command]
#[specta::specta]
pub fn open_sound_file(app: AppHandle) -> Result<(), String> {
    let path = path(&app).map_err(|e| format!("Failed to find the sound file: {e}"))?;
    ensure_exists(&path).map_err(|e| format!("Failed to write the sound file: {e}"))?;
    app.opener()
        .open_path(path.to_string_lossy().to_string(), None::<String>)
        .map_err(|e| format!("Failed to open the sound file: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_feedback::default_written;

    fn parsed(text: &str) -> SoundFile {
        let (file, problems) = parse(text);
        assert_eq!(problems, Vec::<String>::new(), "{text}");
        file
    }

    #[test]
    fn an_entry_is_a_file_or_a_table_of_a_file_and_semitones() {
        let file = parsed(
            "[marimba]\n\
             stop = \"my-stop.wav\"\n\
             done = { file = \"built-in:pop_e5.wav\", semitones = -2 }\n\
             instruction = { semitones = 1.5 }\n\
             [custom]\n\
             start = \"~/Sounds/start.flac\"\n",
        );
        assert_eq!(
            file.entry(SoundTheme::Marimba, SoundType::Stop),
            Some(&Entry {
                file: Some(FileRef::Path("my-stop.wav".into())),
                semitones: 0.0
            })
        );
        assert_eq!(
            file.entry(SoundTheme::Marimba, SoundType::Done),
            Some(&Entry {
                file: Some(FileRef::BuiltIn("pop_e5.wav".into())),
                semitones: -2.0
            })
        );
        assert_eq!(
            file.entry(SoundTheme::Marimba, SoundType::Instruction),
            Some(&Entry {
                file: None,
                semitones: 1.5
            })
        );
        assert_eq!(
            file.entry(SoundTheme::Custom, SoundType::Start),
            Some(&Entry {
                file: Some(FileRef::Path("~/Sounds/start.flac".into())),
                semitones: 0.0
            })
        );
        assert_eq!(file.entry(SoundTheme::Marimba, SoundType::Start), None);
        assert_eq!(file.entry(SoundTheme::Pop, SoundType::Stop), None);
    }

    #[test]
    fn the_waiting_sound_is_the_clipboard_sound() {
        let file = parsed("[pop]\nwaiting = \"wait.wav\"\n");
        assert!(file.entry(SoundTheme::Pop, SoundType::Clipboard).is_some());
        assert!(file.entry(SoundTheme::Pop, SoundType::Done).is_none());
    }

    #[test]
    fn what_cant_be_read_is_named_and_the_rest_still_applies() {
        let (file, found) = parse(
            "[marimba]\n\
             stop = \"ok.wav\"\n\
             finish = \"x.wav\"\n\
             done = { file = \"x.wav\", pitch = 2 }\n\
             start = { semitones = 25 }\n\
             instruction = 3\n\
             waiting = \"\"\n\
             [theremin]\n\
             start = \"x.wav\"\n\
             [pop]\n\
             stop = \"built-in:../../secret.wav\"\n",
        );
        assert!(file.entry(SoundTheme::Marimba, SoundType::Stop).is_some());
        for (theme, sound_type) in [
            (SoundTheme::Marimba, SoundType::Done),
            (SoundTheme::Marimba, SoundType::Start),
            (SoundTheme::Marimba, SoundType::Instruction),
            (SoundTheme::Marimba, SoundType::Clipboard),
            (SoundTheme::Pop, SoundType::Stop),
        ] {
            assert_eq!(
                file.entry(theme, sound_type),
                None,
                "{theme:?} {sound_type:?}"
            );
        }
        let joined = found.join("\n");
        for part in [
            "[marimba] finish",
            "[marimba] done",
            "pitch",
            "[marimba] start",
            "-24 and 24",
            "[marimba] instruction",
            "[marimba] waiting",
            "[theremin]",
            "[pop] stop",
        ] {
            assert!(joined.contains(part), "{part:?} missing in:\n{joined}");
        }
        assert_eq!(found.len(), 7, "{joined}");
    }

    #[test]
    fn a_file_that_isnt_toml_has_no_entries_and_says_why() {
        let (file, found) = parse("[marimba\nstop = ");
        assert_eq!(file, SoundFile::default());
        assert_eq!(found.len(), 1, "{found:?}");
    }

    #[test]
    fn integer_and_fractional_semitones_are_taken() {
        let file = parsed("[pop]\nstop = { semitones = -12 }\ndone = { semitones = 0.25 }\n");
        assert_eq!(
            file.entry(SoundTheme::Pop, SoundType::Stop)
                .map(|e| e.semitones),
            Some(-12.0)
        );
        assert_eq!(
            file.entry(SoundTheme::Pop, SoundType::Done)
                .map(|e| e.semitones),
            Some(0.25)
        );
    }

    /// The template's commented-out entries, uncommented.
    fn uncommented(template: &str) -> String {
        template
            .lines()
            .map(|line| match line.strip_prefix("# ") {
                Some(rest) if rest.contains(" = ") && !rest.starts_with(' ') => rest,
                _ => line,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_template_changes_nothing() {
        assert_eq!(parsed(&template()), SoundFile::default());
    }

    #[test]
    fn the_template_lists_every_event_of_every_theme_with_its_built_in_sound() {
        let file = parsed(&uncommented(&template()));
        for theme in THEMES {
            for sound_type in SoundType::ALL {
                let (written, _) = default_written(theme, sound_type);
                let expected = match written.strip_prefix(BUILT_IN_PREFIX) {
                    Some(name) => FileRef::BuiltIn(name.to_string()),
                    None => FileRef::Path(written.clone()),
                };
                assert_eq!(
                    file.entry(theme, sound_type),
                    Some(&Entry {
                        file: Some(expected),
                        semitones: 0.0
                    }),
                    "{theme:?} {sound_type:?}"
                );
            }
        }
    }

    #[test]
    fn the_template_explains_the_events_and_the_pitch_shift() {
        let text = template();
        for word in [
            "start",
            "instruction",
            "stop",
            "done",
            "waiting",
            "a dictation is cancelled",
            "semitones",
            BUILT_IN_PREFIX,
            "[marimba]",
            "[pop]",
            "[custom]",
        ] {
            assert!(text.contains(word), "{word:?}");
        }
    }

    #[test]
    fn the_file_is_written_once_and_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join(FILE_NAME);
        assert!(ensure_exists(&path).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), template());
        fs::write(&path, "[pop]\n").unwrap();
        assert!(!ensure_exists(&path).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), "[pop]\n");
    }

    #[test]
    fn a_problem_is_logged_once_until_the_file_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        let log = ProblemLog::new();
        let logged = || *log.logged.lock().unwrap();

        fs::write(&path, "[marimba]\nfinish = \"x.wav\"\n").unwrap();
        read_from(&path, &log);
        read_from(&path, &log);
        assert_eq!(logged(), 1, "once for the same text");

        fs::write(&path, "[marimba]\nfinish = \"x.wav\"\n# edited\n").unwrap();
        read_from(&path, &log);
        assert_eq!(logged(), 2, "again once the file changed");

        fs::remove_file(&path).unwrap();
        assert_eq!(read_from(&path, &log), SoundFile::default());
        assert_eq!(logged(), 2, "no file, no warning");
    }
}
