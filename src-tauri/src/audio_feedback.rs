use crate::audio_toolkit::audio::output_device_name;
use crate::settings::SoundTheme;
use crate::settings::{self, AppSettings};
use crate::sound_queue::SoundQueue;
use log::{debug, error, warn};
use rodio::cpal::traits::HostTrait;
use rodio::DeviceSinkBuilder;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::thread;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundType {
    Start,
    Stop,
    /// DanyWhere: the transcript was left on the clipboard, and the user has
    /// to paste it.
    Clipboard,
}

/// The custom theme's optional clipboard sound, in the app data folder.
const CUSTOM_CLIPBOARD_SOUND: &str = "custom_clipboard.wav";

/// Where a feedback sound's file is.
#[derive(Debug, PartialEq, Eq)]
enum SoundLocation {
    /// Bundled with the app, relative to its resources.
    Resource(String),
    /// The user's own file in the app data folder (custom theme).
    AppData(String),
}

/// Finds the file for `sound_type` in `theme`. `custom_exists` tells whether
/// a file of the custom theme is present in the app data folder.
fn sound_location(
    theme: SoundTheme,
    sound_type: SoundType,
    custom_exists: impl Fn(&str) -> bool,
) -> SoundLocation {
    match (theme, sound_type) {
        (SoundTheme::Custom, SoundType::Start) => {
            SoundLocation::AppData("custom_start.wav".to_string())
        }
        (SoundTheme::Custom, SoundType::Stop) => {
            SoundLocation::AppData("custom_stop.wav".to_string())
        }
        // A custom theme needs only a start and a stop sound; without a
        // clipboard sound of its own it borrows the default theme's.
        (SoundTheme::Custom, SoundType::Clipboard) => {
            if custom_exists(CUSTOM_CLIPBOARD_SOUND) {
                SoundLocation::AppData(CUSTOM_CLIPBOARD_SOUND.to_string())
            } else {
                SoundLocation::Resource(SoundTheme::Marimba.to_clipboard_path())
            }
        }
        (theme, SoundType::Start) => SoundLocation::Resource(theme.to_start_path()),
        (theme, SoundType::Stop) => SoundLocation::Resource(theme.to_stop_path()),
        (theme, SoundType::Clipboard) => SoundLocation::Resource(theme.to_clipboard_path()),
    }
}

fn resolve_sound_path(
    app: &AppHandle,
    settings: &AppSettings,
    sound_type: SoundType,
) -> Option<PathBuf> {
    let custom_exists =
        |file: &str| crate::portable::resolve_app_data(app, file).is_ok_and(|path| path.exists());
    match sound_location(settings.sound_theme, sound_type, custom_exists) {
        SoundLocation::AppData(file) => crate::portable::resolve_app_data(app, &file).ok(),
        SoundLocation::Resource(file) => app
            .path()
            .resolve(&file, tauri::path::BaseDirectory::Resource)
            .ok(),
    }
}

pub fn play_feedback_sound(app: &AppHandle, sound_type: SoundType) {
    let settings = settings::get_settings(app);
    if !settings.audio_feedback {
        return;
    }
    if let Some(path) = resolve_sound_path(app, &settings, sound_type) {
        play_sound_async(app, path);
    }
}

pub fn play_feedback_sound_blocking(app: &AppHandle, sound_type: SoundType) {
    let settings = settings::get_settings(app);
    if !settings.audio_feedback {
        return;
    }
    if let Some(path) = resolve_sound_path(app, &settings, sound_type) {
        play_sound_blocking(app, &path);
    }
}

pub fn play_test_sound(app: &AppHandle, sound_type: SoundType) {
    let settings = settings::get_settings(app);
    if let Some(path) = resolve_sound_path(app, &settings, sound_type) {
        play_sound_blocking(app, &path);
    }
}

/// One feedback sound waiting to be played.
struct SoundJob {
    app: AppHandle,
    path: PathBuf,
}

/// DanyWhere: every feedback sound goes through one queue, so sounds never
/// overlap (see `sound_queue`). `None` if its thread could not be started;
/// sounds then play as in Handy.
fn sound_queue() -> Option<&'static SoundQueue<SoundJob>> {
    static QUEUE: OnceLock<Option<SoundQueue<SoundJob>>> = OnceLock::new();
    QUEUE
        .get_or_init(|| {
            SoundQueue::new(|job: SoundJob| play_sound_now(&job.app, &job.path))
                .map_err(|e| error!("Failed to start the feedback sound thread: {}", e))
                .ok()
        })
        .as_ref()
}

fn play_sound_async(app: &AppHandle, path: PathBuf) {
    let job = SoundJob {
        app: app.clone(),
        path,
    };
    let job = match sound_queue() {
        Some(queue) => match queue.enqueue(job) {
            Ok(()) => return,
            Err(job) => job,
        },
        None => job,
    };
    thread::spawn(move || play_sound_now(&job.app, &job.path));
}

fn play_sound_blocking(app: &AppHandle, path: &Path) {
    let job = SoundJob {
        app: app.clone(),
        path: path.to_path_buf(),
    };
    let job = match sound_queue() {
        Some(queue) => match queue.play_and_wait(job) {
            Ok(()) => return,
            Err(job) => job,
        },
        None => job,
    };
    play_sound_now(&job.app, &job.path);
}

fn play_sound_now(app: &AppHandle, path: &Path) {
    if let Err(e) = play_sound_at_path(app, path) {
        error!("Failed to play sound '{}': {}", path.display(), e);
    }
}

fn play_sound_at_path(app: &AppHandle, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let settings = settings::get_settings(app);
    let volume = settings.audio_feedback_volume;
    let selected_device = settings.selected_output_device.clone();
    play_audio_file(path, selected_device, volume)
}

fn play_audio_file(
    path: &std::path::Path,
    selected_device: Option<String>,
    volume: f32,
) -> Result<(), Box<dyn std::error::Error>> {
    let sink_builder = if let Some(device_name) = selected_device {
        if device_name == "Default" {
            debug!("Using default device");
            DeviceSinkBuilder::from_default_device()?
        } else {
            let host = crate::audio_toolkit::get_cpal_host();
            let found_device = host
                .output_devices()?
                .find(|device| output_device_name(device).as_deref() == Some(&device_name));

            match found_device {
                Some(device) => DeviceSinkBuilder::from_device(device)?,
                None => {
                    warn!("Device '{}' not found, using default device", device_name);
                    DeviceSinkBuilder::from_default_device()?
                }
            }
        }
    } else {
        debug!("Using default device");
        DeviceSinkBuilder::from_default_device()?
    };

    let device_sink = sink_builder.open_stream()?;
    let mixer = device_sink.mixer();

    let file = File::open(path)?;
    let buf_reader = BufReader::new(file);

    let player = rodio::play(mixer, buf_reader)?;
    player.set_volume(volume);
    player.sleep_until_end();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{sound_location, SoundLocation, SoundType};
    use crate::settings::SoundTheme;
    use std::path::Path;

    fn resource(path: &str) -> SoundLocation {
        SoundLocation::Resource(path.to_string())
    }

    fn app_data(file: &str) -> SoundLocation {
        SoundLocation::AppData(file.to_string())
    }

    const NO_CUSTOM_FILES: fn(&str) -> bool = |_| false;

    #[test]
    fn each_theme_has_its_own_clipboard_sound() {
        assert_eq!(
            sound_location(SoundTheme::Marimba, SoundType::Clipboard, NO_CUSTOM_FILES),
            resource("resources/marimba_clipboard.wav")
        );
        assert_eq!(
            sound_location(SoundTheme::Pop, SoundType::Clipboard, NO_CUSTOM_FILES),
            resource("resources/pop_clipboard.wav")
        );
    }

    #[test]
    fn the_start_and_stop_sounds_stay_where_they_were() {
        assert_eq!(
            sound_location(SoundTheme::Marimba, SoundType::Start, NO_CUSTOM_FILES),
            resource("resources/marimba_start.wav")
        );
        assert_eq!(
            sound_location(SoundTheme::Pop, SoundType::Stop, NO_CUSTOM_FILES),
            resource("resources/pop_stop.wav")
        );
        assert_eq!(
            sound_location(SoundTheme::Custom, SoundType::Start, NO_CUSTOM_FILES),
            app_data("custom_start.wav")
        );
        assert_eq!(
            sound_location(SoundTheme::Custom, SoundType::Stop, NO_CUSTOM_FILES),
            app_data("custom_stop.wav")
        );
    }

    #[test]
    fn the_custom_theme_uses_its_own_clipboard_sound_when_there_is_one() {
        assert_eq!(
            sound_location(SoundTheme::Custom, SoundType::Clipboard, |file| {
                file == "custom_clipboard.wav"
            }),
            app_data("custom_clipboard.wav")
        );
    }

    #[test]
    fn the_custom_theme_falls_back_to_the_marimba_clipboard_sound() {
        assert_eq!(
            sound_location(SoundTheme::Custom, SoundType::Clipboard, NO_CUSTOM_FILES),
            resource("resources/marimba_clipboard.wav")
        );
    }

    #[test]
    fn the_bundled_clipboard_sounds_are_short_wav_files() {
        for theme in [SoundTheme::Marimba, SoundTheme::Pop] {
            let SoundLocation::Resource(relative) =
                sound_location(theme, SoundType::Clipboard, NO_CUSTOM_FILES)
            else {
                panic!("{theme:?}: the clipboard sound is not bundled");
            };
            let file = Path::new(env!("CARGO_MANIFEST_DIR")).join(&relative);
            let reader =
                hound::WavReader::open(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
            let seconds = reader.duration() as f32 / reader.spec().sample_rate as f32;
            assert!(
                (0.1..1.0).contains(&seconds),
                "{relative} lasts {seconds} s"
            );
        }
    }
}
