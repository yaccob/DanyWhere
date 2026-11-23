use crate::audio_toolkit::audio::{output_device_name, OutputHost, SystemOutputHost};
use crate::settings::SoundTheme;
use crate::settings::{self, AppSettings};
use crate::sound_queue::SoundQueue;
use log::{debug, error, warn};
use rodio::cpal::{traits::DeviceTrait, BufferSize, FrameCount, SupportedBufferSize};
use rodio::{
    cpal, Decoder, DeviceSinkBuilder, DeviceSinkError, MixerDeviceSink, PlayError, Player, Source,
};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundType {
    Start,
    Stop,
    /// DanyWhere: the transcript was left on the clipboard, and the user has
    /// to paste it.
    Clipboard,
    /// DanyWhere: a dictation is completely done, post-processing included,
    /// however it was delivered (`delivery::sound_after_dictation`).
    Done,
}

/// The custom theme's optional clipboard sound, in the app data folder.
const CUSTOM_CLIPBOARD_SOUND: &str = "custom_clipboard.wav";

/// The custom theme's optional done sound, in the app data folder.
const CUSTOM_DONE_SOUND: &str = "custom_done.wav";

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
        (SoundTheme::Custom, SoundType::Done) if custom_exists(CUSTOM_DONE_SOUND) => {
            SoundLocation::AppData(CUSTOM_DONE_SOUND.to_string())
        }
        // A custom theme needs only a start and a stop sound; without a
        // clipboard sound of its own it borrows the default theme's. The done
        // sound is the high clipboard sound (user, 2026-11-15), so without a
        // file of its own it is the theme's clipboard sound.
        (SoundTheme::Custom, SoundType::Clipboard | SoundType::Done) => {
            if custom_exists(CUSTOM_CLIPBOARD_SOUND) {
                SoundLocation::AppData(CUSTOM_CLIPBOARD_SOUND.to_string())
            } else {
                SoundLocation::Resource(SoundTheme::Marimba.to_clipboard_path())
            }
        }
        (theme, SoundType::Start) => SoundLocation::Resource(theme.to_start_path()),
        (theme, SoundType::Stop) => SoundLocation::Resource(theme.to_stop_path()),
        (theme, SoundType::Clipboard | SoundType::Done) => {
            SoundLocation::Resource(theme.to_clipboard_path())
        }
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
    // DanyWhere: as the dictation's profile says, if it has its own.
    let settings = crate::app_profiles::dictation_settings(app);
    if !settings.audio_feedback {
        return;
    }
    if let Some(path) = resolve_sound_path(app, &settings, sound_type) {
        play_sound_async(path, SoundOutput::of(&settings));
    }
}

pub fn play_feedback_sound_blocking(app: &AppHandle, sound_type: SoundType) {
    // DanyWhere: as the dictation's profile says, if it has its own.
    let settings = crate::app_profiles::dictation_settings(app);
    if !settings.audio_feedback {
        return;
    }
    if let Some(path) = resolve_sound_path(app, &settings, sound_type) {
        play_sound_blocking(&path, SoundOutput::of(&settings));
    }
}

pub fn play_test_sound(app: &AppHandle, sound_type: SoundType) {
    let settings = settings::get_settings(app);
    if let Some(path) = resolve_sound_path(app, &settings, sound_type) {
        play_sound_blocking(&path, SoundOutput::of(&settings));
    }
}

/// DanyWhere: the device and volume a sound plays with, taken when it is
/// asked for, so a dictation's sound follows its profile also when it plays
/// after the dictation ended.
#[derive(Debug, Clone, PartialEq)]
struct SoundOutput {
    device: Option<String>,
    volume: f32,
}

impl SoundOutput {
    fn of(settings: &AppSettings) -> Self {
        Self {
            device: settings.selected_output_device.clone(),
            volume: settings.audio_feedback_volume,
        }
    }
}

/// One feedback sound waiting to be played.
struct SoundJob {
    path: PathBuf,
    output: SoundOutput,
}

/// DanyWhere: every feedback sound goes through one queue, so sounds never
/// overlap (see `sound_queue`). `None` if its thread could not be started;
/// sounds then play as in Handy.
fn sound_queue() -> Option<&'static SoundQueue<SoundJob>> {
    static QUEUE: OnceLock<Option<SoundQueue<SoundJob>>> = OnceLock::new();
    QUEUE
        .get_or_init(|| {
            SoundQueue::new(|job: SoundJob| play_sound_now(&job.path, &job.output))
                .map_err(|e| error!("Failed to start the feedback sound thread: {}", e))
                .ok()
        })
        .as_ref()
}

fn play_sound_async(path: PathBuf, output: SoundOutput) {
    let job = SoundJob { path, output };
    let job = match sound_queue() {
        Some(queue) => match queue.enqueue(job) {
            Ok(()) => return,
            Err(job) => job,
        },
        None => job,
    };
    thread::spawn(move || play_sound_now(&job.path, &job.output));
}

fn play_sound_blocking(path: &Path, output: SoundOutput) {
    let job = SoundJob {
        path: path.to_path_buf(),
        output,
    };
    let job = match sound_queue() {
        Some(queue) => match queue.play_and_wait(job) {
            Ok(()) => return,
            Err(job) => job,
        },
        None => job,
    };
    play_sound_now(&job.path, &job.output);
}

fn play_sound_now(path: &Path, output: &SoundOutput) {
    match play_audio_file_on(
        &SystemOutputHost,
        path,
        output.device.as_deref(),
        output.volume,
        SoundTimeLimit::FEEDBACK,
    ) {
        Ok(SoundEnd::Played) => {}
        Ok(SoundEnd::GaveUp { waited }) => warn!(
            "Gave up on sound '{}' after {} ms: the output device stopped taking it",
            path.display(),
            waited.as_millis()
        ),
        Err(e) => error!("{}", failure_line(path, e.as_ref())),
    }
}

/// DanyWhere: the log line of a sound that failed, with the reason: rodio's
/// errors name only their kind ("Error opening the stream with the OS") and
/// keep the device's reason as their source.
fn failure_line(path: &Path, error: &dyn std::error::Error) -> String {
    format!(
        "Failed to play sound '{}': {}",
        path.display(),
        error_chain(error)
    )
}

/// DanyWhere: an error and every cause behind it, on one line. `{:#}`
/// prints the causes only for `anyhow` errors, not for a `dyn Error`.
fn error_chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

/// DanyWhere: how a sound ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SoundEnd {
    /// The device took all of it.
    Played,
    /// The device stopped taking it (unplugged, Bluetooth gone), so it was
    /// stopped after `waited`.
    GaveUp { waited: Duration },
}

/// DanyWhere: how long a sound may take before it is given up. rodio's
/// `sleep_until_end` waits without a limit, so a device that goes silent in
/// the middle of a sound held the thread of the feedback sounds, every sound
/// after it and a caller that waits for its sound (the start sound before a
/// recording), and with it the start of every later dictation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SoundTimeLimit {
    /// Beyond the sound's own length.
    grace: Duration,
    /// For a file that doesn't tell its length.
    unknown_length: Duration,
}

impl SoundTimeLimit {
    /// A sound's length plus 1 s: rodio's buffer (about 50 ms) and the start
    /// of a Bluetooth device fit in it many times over. 5 s for a file
    /// without a length: longer than any feedback sound (the bundled ones
    /// last under 1 s), short enough not to hold a dictation's start long.
    const FEEDBACK: Self = Self {
        grace: Duration::from_secs(1),
        unknown_length: Duration::from_secs(5),
    };

    /// How long a sound of `length` may take.
    fn for_length(self, length: Option<Duration>) -> Duration {
        length.map_or(self.unknown_length, |length| length + self.grace)
    }
}

/// DanyWhere: how often `wait_for_end` looks whether a sound has ended.
const SOUND_END_CHECK: Duration = Duration::from_millis(5);

/// DanyWhere: waits until `player` has played its sound, at most
/// `time_limit`, and stops it then. rodio offers no `sleep_until_end` with a
/// time limit, so this looks every few milliseconds, as `Player::empty`
/// tells the moment `sleep_until_end` would return.
fn wait_for_end(player: &Player, time_limit: Duration) -> SoundEnd {
    let started = Instant::now();
    while !player.empty() {
        let waited = started.elapsed();
        if waited >= time_limit {
            player.stop();
            return SoundEnd::GaveUp { waited };
        }
        thread::sleep(SOUND_END_CHECK.min(time_limit - waited));
    }
    SoundEnd::Played
}

/// DanyWhere: the device a sound plays on: the one named `selected_device`
/// among `outputs`, else their default. A list that can't be read fails the
/// sound; it doesn't fall back.
fn feedback_device(
    outputs: &impl OutputHost,
    selected_device: Option<&str>,
) -> Result<cpal::Device, Box<dyn std::error::Error>> {
    // As rodio's `DeviceSinkBuilder::from_default_device`.
    let default_device = || outputs.default_device().ok_or(DeviceSinkError::NoDevice);
    match selected_device {
        None | Some("Default") => {
            debug!("Using default device");
            Ok(default_device()?)
        }
        Some(device_name) => {
            let found_device = outputs
                .all_devices()?
                .find(|device| output_device_name(device).as_deref() == Some(device_name));
            match found_device {
                Some(device) => Ok(device),
                None => {
                    warn!("Device '{}' not found, using default device", device_name);
                    Ok(default_device()?)
                }
            }
        }
    }
}

/// DanyWhere: the buffer rodio's `DeviceSinkBuilder::from_device` asks every
/// device for: about 50 ms at `sample_rate`, rounded to the nearest power of
/// two (rodio's own `nearest_multiple_of_two` is private).
fn rodio_buffer_frames(sample_rate: u32) -> FrameCount {
    let frames = sample_rate / (1000 / 50);
    if frames <= 1 {
        return 1;
    }
    let above = frames.next_power_of_two();
    let below = above >> 1;
    if frames - below <= above - frames {
        below
    } else {
        above
    }
}

/// DanyWhere: `rodio_frames` fitted into the buffers a device `offered`.
/// cpal refuses a buffer outside that range on macOS and Linux, so a Bose QC
/// Ultra, which takes 14 to 1024 frames only, refused rodio's 2048 for every
/// sound (Handy's #2524). Without a range, or with one that can't be right
/// (empty, or 0 frames at most), rodio's buffer as it is.
fn fitted_buffer(rodio_frames: FrameCount, offered: &SupportedBufferSize) -> BufferSize {
    match *offered {
        SupportedBufferSize::Range { min, max } if min <= max && max > 0 => {
            BufferSize::Fixed(rodio_frames.clamp(min.max(1), max))
        }
        _ => BufferSize::Fixed(rodio_frames),
    }
}

/// DanyWhere: opens `device` as rodio's `DeviceSinkBuilder::from_device`
/// would, in the format the device offers, with the buffer `fitted_buffer`
/// gives. A device that refuses to build that stream is asked once more,
/// with the system's default buffer. Handy's fix for #2525 asks every device
/// for the system's default; DanyWhere keeps rodio's buffer of about 50 ms
/// wherever a device takes it, as rodio chose it against playback that
/// breaks on systems whose default is set wrong.
fn open_sink(device: cpal::Device) -> Result<MixerDeviceSink, DeviceSinkError> {
    let offered = device
        .default_output_config()
        .map_err(DeviceSinkError::DefaultSinkConfigError)?;
    let open_with = |buffer: BufferSize| {
        DeviceSinkBuilder::default()
            .with_device(device.clone())
            .with_supported_config(&offered)
            .with_buffer_size(buffer)
            .open_stream()
    };
    let buffer = fitted_buffer(
        rodio_buffer_frames(offered.sample_rate()),
        offered.buffer_size(),
    );
    match open_with(buffer) {
        Err(DeviceSinkError::BuildError(refused)) => {
            warn!(
                "The output device refused a stream with a buffer of {:?} ({}); \
                 trying the system's default buffer",
                buffer,
                error_chain(&refused)
            );
            open_with(BufferSize::Default).map_err(|again| {
                warn!(
                    "The output device refused the system's default buffer too: {}",
                    error_chain(&again)
                );
                DeviceSinkError::BuildError(refused)
            })
        }
        opened => opened,
    }
}

/// DanyWhere: plays `path` at `volume` on the device `feedback_device`
/// picks among `outputs`, and returns once the device has taken all of it
/// or the time `limit` allows for it has passed.
fn play_audio_file_on(
    outputs: &impl OutputHost,
    path: &Path,
    selected_device: Option<&str>,
    volume: f32,
    limit: SoundTimeLimit,
) -> Result<SoundEnd, Box<dyn std::error::Error>> {
    let device = feedback_device(outputs, selected_device)?;
    let device_sink = open_sink(device)?;
    let mixer = device_sink.mixer();

    let file = File::open(path)?;
    let sound = Decoder::new(BufReader::new(file)).map_err(PlayError::from)?;
    let time_limit = limit.for_length(sound.total_duration());

    // As rodio's `play`, which hides the sound's length.
    let player = Player::connect_new(mixer);
    player.append(sound);
    player.set_volume(volume);

    // The sink and its stream close when they're dropped here, also after
    // a sound given up.
    Ok(wait_for_end(&player, time_limit))
}

#[cfg(test)]
mod tests {
    use super::{
        error_chain, failure_line, fitted_buffer, play_audio_file_on, sound_location, SoundEnd,
        SoundLocation, SoundQueue, SoundTimeLimit, SoundType,
    };
    use crate::audio_toolkit::audio::fake_output::{FakeOutput, FakeOutputs, Running};
    use crate::settings::SoundTheme;
    use rodio::cpal::{BufferSize, ErrorKind, SupportedBufferSize};
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    // DanyWhere: feedback sounds on fake output devices (`fake_output`).

    /// The bundled start sound, a fraction of a second long.
    fn sound() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/marimba_start.wav")
    }

    /// A 440 Hz tone of `ms` milliseconds at half the full scale, as a 44.1
    /// kHz stereo WAV file in `dir`.
    fn short_tone(dir: &Path, ms: u32) -> PathBuf {
        let path = dir.join("tone.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut wav = hound::WavWriter::create(&path, spec).expect("the tone is written");
        for frame in 0..44_100 * ms / 1000 {
            let phase = std::f32::consts::TAU * 440.0 * frame as f32 / 44_100.0;
            let sample = (0.5 * phase.sin() * f32::from(i16::MAX)) as i16;
            wav.write_sample(sample).expect("the tone is written");
            wav.write_sample(sample).expect("the tone is written");
        }
        wav.finalize().expect("the tone is written");
        path
    }

    /// A time limit short enough for a test: a sound's length plus 100 ms.
    const QUICK_LIMIT: SoundTimeLimit = SoundTimeLimit {
        grace: Duration::from_millis(100),
        unknown_length: Duration::from_secs(30),
    };

    /// Plays `sound` on `outputs` within `limit` on a thread of its own, so a
    /// device that never lets it end can't hang the tests.
    fn play_file_in_background(
        outputs: &FakeOutputs,
        sound: PathBuf,
        selected: Option<&str>,
        volume: f32,
        limit: SoundTimeLimit,
    ) -> mpsc::Receiver<Result<SoundEnd, String>> {
        let outputs = outputs.clone();
        let selected = selected.map(str::to_string);
        let (done, result) = mpsc::channel();
        std::thread::spawn(move || {
            let played =
                play_audio_file_on(&outputs.host(), &sound, selected.as_deref(), volume, limit)
                    .map_err(|e| error_chain(e.as_ref()));
            let _ = done.send(played);
        });
        result
    }

    /// Plays the start sound on `outputs` on a thread of its own, within the
    /// app's own time limit.
    fn play_in_background(
        outputs: &FakeOutputs,
        selected: Option<&str>,
        volume: f32,
    ) -> mpsc::Receiver<Result<SoundEnd, String>> {
        play_file_in_background(outputs, sound(), selected, volume, SoundTimeLimit::FEEDBACK)
    }

    const PLAYED: Result<SoundEnd, String> = Ok(SoundEnd::Played);

    /// Plays the start sound on `outputs` and tells how it ended.
    fn play(outputs: &FakeOutputs, selected: Option<&str>) -> Result<SoundEnd, String> {
        play_in_background(outputs, selected, 1.0)
            .recv_timeout(Duration::from_secs(5))
            .expect("the sound ends")
    }

    fn speakers() -> FakeOutput {
        FakeOutput::new("MacBook Pro Speakers")
    }

    fn headset() -> FakeOutput {
        FakeOutput::new("USB Headset")
    }

    #[test]
    fn without_a_selected_device_the_sound_plays_on_the_default() {
        let (speakers, headset) = (speakers(), headset());
        let outputs = FakeOutputs::new(&[&speakers, &headset]).with_default(&speakers);
        assert_eq!(play(&outputs, None), PLAYED);
        assert!(
            speakers.peak() > 0.01,
            "the sound reached the default device"
        );
        assert_eq!(speakers.open_streams(), 0, "its stream closed after it");
        assert!(headset.asked_buffers().is_empty());
        assert_eq!(outputs.times_listed(), 0, "the default needs no list");
    }

    #[test]
    fn the_device_named_default_is_the_default_without_listing_devices() {
        let (speakers, headset) = (speakers(), headset());
        let outputs = FakeOutputs::new(&[&speakers, &headset]).with_default(&speakers);
        assert_eq!(play(&outputs, Some("Default")), PLAYED);
        assert!(speakers.peak() > 0.01);
        assert_eq!(outputs.times_listed(), 0);
    }

    #[test]
    fn a_selected_device_is_found_by_its_name() {
        let (speakers, headset) = (speakers(), headset());
        let outputs = FakeOutputs::new(&[&speakers, &headset]).with_default(&speakers);
        assert_eq!(play(&outputs, Some("USB Headset")), PLAYED);
        assert!(headset.peak() > 0.01, "the sound reached the headset");
        assert!(
            speakers.asked_buffers().is_empty(),
            "the default stayed idle"
        );
        assert_eq!(outputs.times_listed(), 1);
    }

    #[test]
    fn a_missing_selected_device_falls_back_to_the_default() {
        let (speakers, headset) = (speakers(), headset());
        let outputs = FakeOutputs::new(&[&speakers, &headset]).with_default(&speakers);
        assert_eq!(play(&outputs, Some("AirPods Pro")), PLAYED);
        assert!(speakers.peak() > 0.01);
        assert!(headset.asked_buffers().is_empty());
    }

    #[test]
    fn a_device_whose_name_can_no_longer_be_read_is_not_chosen() {
        let speakers = speakers();
        let gone = FakeOutput::new("AirPods Pro").nameless();
        let outputs = FakeOutputs::new(&[&speakers, &gone]).with_default(&speakers);
        assert_eq!(play(&outputs, Some("AirPods Pro")), PLAYED);
        assert!(speakers.peak() > 0.01, "the default stood in");
        assert!(gone.asked_buffers().is_empty());
    }

    #[test]
    fn without_any_output_device_the_sound_fails() {
        let outputs = FakeOutputs::new(&[]);
        let no_device = Err("Could not find any output device".to_string());
        assert_eq!(play(&outputs, None), no_device);
        assert_eq!(play(&outputs, Some("USB Headset")), no_device);
    }

    #[test]
    fn when_the_devices_cannot_be_listed_the_sound_fails_without_the_default() {
        let (speakers, headset) = (speakers(), headset());
        let outputs = FakeOutputs::new(&[&speakers, &headset])
            .with_default(&speakers)
            .failing_to_list(ErrorKind::HostUnavailable);
        assert_eq!(
            play(&outputs, Some("USB Headset")),
            Err(ErrorKind::HostUnavailable.to_string())
        );
        assert!(speakers.asked_buffers().is_empty(), "no fallback today");
    }

    #[test]
    fn a_device_that_has_gone_when_asked_for_its_format_fails_the_sound() {
        let speakers = speakers().without_format();
        let outputs = FakeOutputs::new(&[&speakers]).with_default(&speakers);
        assert_eq!(
            play(&outputs, None),
            Err(format!(
                "Failed to get the config for the given device: {}",
                ErrorKind::DeviceNotAvailable
            ))
        );
        assert!(speakers.asked_buffers().is_empty());
    }

    #[test]
    fn a_sound_asks_the_device_for_rodios_buffer_of_about_50_ms() {
        // What DanyWhere's fix for Handy's #2525 keeps for every device that
        // takes it; Handy's own fix asks every device for the system's
        // default.
        for (rate, frames) in [(44_100, 2048), (48_000, 2048), (96_000, 4096)] {
            let device = FakeOutput::new("Studio Display Speakers").offering(
                rate,
                2,
                SupportedBufferSize::Range { min: 14, max: 4096 },
            );
            let outputs = FakeOutputs::new(&[&device]).with_default(&device);
            assert_eq!(play(&outputs, None), PLAYED, "{rate} Hz");
            assert_eq!(
                device.asked_buffers(),
                [BufferSize::Fixed(frames)],
                "{rate} Hz"
            );
        }
    }

    /// Handy's issue #2524: a Bose QC Ultra on macOS takes buffers of 14 to
    /// 1024 frames only, and says so.
    fn bose() -> FakeOutput {
        FakeOutput::new("Bose QC Ultra Headphones")
            .offering(44_100, 2, SupportedBufferSize::Range { min: 14, max: 1024 })
            .accepting_buffers(14..=1024)
    }

    #[test]
    fn a_device_that_takes_only_short_buffers_plays_with_the_largest_it_offers() {
        // Handy's #2525: rodio's 2048 frames failed every sound on it.
        let bose = bose();
        let outputs = FakeOutputs::new(&[&bose]).with_default(&bose);
        assert_eq!(play(&outputs, Some("Bose QC Ultra Headphones")), PLAYED);
        assert_eq!(bose.asked_buffers(), [BufferSize::Fixed(1024)]);
        assert!(bose.peak() > 0.01, "the sound reached the headphones");
        assert_eq!(bose.open_streams(), 0);
    }

    #[test]
    fn a_device_that_takes_only_long_buffers_plays_with_the_shortest_it_offers() {
        let device = FakeOutput::new("Conference Speaker")
            .offering(
                44_100,
                2,
                SupportedBufferSize::Range {
                    min: 4096,
                    max: 8192,
                },
            )
            .accepting_buffers(4096..=8192);
        let outputs = FakeOutputs::new(&[&device]).with_default(&device);
        assert_eq!(play(&outputs, None), PLAYED);
        assert_eq!(device.asked_buffers(), [BufferSize::Fixed(4096)]);
    }

    #[test]
    fn a_device_that_refuses_the_buffer_it_offers_plays_with_the_system_default() {
        let device = FakeOutput::new("Lying Headset")
            .offering(44_100, 2, SupportedBufferSize::Range { min: 14, max: 4096 })
            .accepting_buffers(14..=1024);
        let outputs = FakeOutputs::new(&[&device]).with_default(&device);
        assert_eq!(play(&outputs, None), PLAYED);
        assert_eq!(
            device.asked_buffers(),
            [BufferSize::Fixed(2048), BufferSize::Default],
            "asked once more, and only once"
        );
        assert!(device.peak() > 0.01);
        assert_eq!(device.open_streams(), 0);
    }

    #[test]
    fn a_device_without_a_buffer_range_gets_rodios_buffer() {
        let device = speakers().offering(48_000, 2, SupportedBufferSize::Unknown);
        let outputs = FakeOutputs::new(&[&device]).with_default(&device);
        assert_eq!(play(&outputs, None), PLAYED);
        assert_eq!(device.asked_buffers(), [BufferSize::Fixed(2048)]);
    }

    #[test]
    fn rodios_buffer_is_fitted_into_a_range_that_makes_sense_only() {
        let range = |min, max| SupportedBufferSize::Range { min, max };
        assert_eq!(
            fitted_buffer(2048, &range(14, 1024)),
            BufferSize::Fixed(1024)
        );
        assert_eq!(
            fitted_buffer(2048, &range(4096, 8192)),
            BufferSize::Fixed(4096)
        );
        assert_eq!(
            fitted_buffer(2048, &range(14, 4096)),
            BufferSize::Fixed(2048)
        );
        assert_eq!(
            fitted_buffer(2048, &range(1024, 14)),
            BufferSize::Fixed(2048)
        );
        assert_eq!(fitted_buffer(2048, &range(0, 0)), BufferSize::Fixed(2048));
        assert_eq!(fitted_buffer(2048, &range(0, 512)), BufferSize::Fixed(512));
        assert_eq!(
            fitted_buffer(2048, &SupportedBufferSize::Unknown),
            BufferSize::Fixed(2048)
        );
    }

    #[test]
    fn a_device_that_offers_a_rate_it_does_not_take_fails_the_sound() {
        let device = speakers()
            .offering(44_100, 2, SupportedBufferSize::Unknown)
            .accepting_only_rate(48_000);
        let outputs = FakeOutputs::new(&[&device]).with_default(&device);
        assert_eq!(
            play(&outputs, None),
            Err(format!(
                "Error opening the stream with the OS: {}",
                ErrorKind::UnsupportedConfig
            ))
        );
        assert_eq!(
            device.asked_rates(),
            [44_100, 44_100],
            "the system's buffer is tried once more, and no other rate"
        );
    }

    #[test]
    fn a_device_that_cannot_be_opened_fails_the_sound_with_its_first_refusal() {
        let device = speakers().failing_to_open(ErrorKind::DeviceBusy);
        let outputs = FakeOutputs::new(&[&device]).with_default(&device);
        assert_eq!(
            play(&outputs, None),
            Err(format!(
                "Error opening the stream with the OS: {}",
                ErrorKind::DeviceBusy
            ))
        );
        assert_eq!(
            device.asked_buffers(),
            [BufferSize::Fixed(2048), BufferSize::Default]
        );
    }

    #[test]
    fn the_log_line_of_a_failed_sound_tells_why() {
        let device = speakers().failing_to_open(ErrorKind::DeviceBusy);
        let outputs = FakeOutputs::new(&[&device]).with_default(&device);
        let error = play_audio_file_on(
            &outputs.host(),
            &sound(),
            None,
            1.0,
            SoundTimeLimit::FEEDBACK,
        )
        .expect_err("the sound fails");
        assert_eq!(
            failure_line(Path::new("/sounds/start.wav"), error.as_ref()),
            format!(
                "Failed to play sound '/sounds/start.wav': \
                 Error opening the stream with the OS: {}",
                ErrorKind::DeviceBusy
            )
        );
    }

    #[test]
    fn a_device_that_cannot_start_fails_the_sound_and_closes_its_stream() {
        let device = speakers().failing_to_start(ErrorKind::PermissionDenied);
        let outputs = FakeOutputs::new(&[&device]).with_default(&device);
        assert_eq!(
            play(&outputs, None),
            Err(format!(
                "Could not start playing the stream: {}",
                ErrorKind::PermissionDenied
            ))
        );
        assert_eq!(
            device.asked_buffers().len(),
            1,
            "a stream it took is not asked for again with another buffer"
        );
        assert_eq!(device.open_streams(), 0);
    }

    #[test]
    fn an_error_while_playing_does_not_end_the_sound() {
        let device = speakers().running(Running::ReportsThenPulls(ErrorKind::Xrun));
        let outputs = FakeOutputs::new(&[&device]).with_default(&device);
        assert_eq!(play(&outputs, None), PLAYED);
        assert_eq!(device.errors_reported(), 1);
        assert!(device.peak() > 0.01, "the sound played on");
    }

    /// A headset that is unplugged in the middle of a sound: it asks for
    /// audio once, reports the error and goes silent.
    fn unplugged_headset() -> FakeOutput {
        headset().running(Running::ReportsThenStalls(ErrorKind::DeviceNotAvailable))
    }

    #[test]
    fn a_device_that_goes_silent_after_an_error_has_its_sound_given_up() {
        // rodio's `sleep_until_end` waited without a time limit, until the
        // device asked for audio again.
        let dir = tempfile::tempdir().unwrap();
        let tone = short_tone(dir.path(), 50);
        let headset = unplugged_headset();
        let outputs = FakeOutputs::new(&[&headset]).with_default(&headset);
        let started = Instant::now();
        let result = play_file_in_background(&outputs, tone, None, 1.0, QUICK_LIMIT);
        let ended = result
            .recv_timeout(Duration::from_secs(2))
            .expect("the sound is given up within 2 s");
        let took = started.elapsed();
        let Ok(SoundEnd::GaveUp { waited }) = ended else {
            panic!("{ended:?}");
        };
        assert_eq!(headset.errors_reported(), 1);
        assert!(
            waited >= Duration::from_millis(150),
            "the whole sound and its grace: {waited:?}"
        );
        assert!(took < Duration::from_secs(1), "{took:?}");
        assert_eq!(headset.open_streams(), 0, "its stream is closed");
    }

    #[test]
    fn a_device_that_goes_silent_holds_neither_later_sounds_nor_a_caller_that_waits() {
        // Every feedback sound plays on one thread (`sound_queue`), and the
        // start sound before a recording waits for its end
        // (`play_feedback_sound_blocking`).
        let dir = tempfile::tempdir().unwrap();
        let tone = short_tone(dir.path(), 50);
        let (headset, speakers) = (unplugged_headset(), speakers());
        let queue = SoundQueue::new(|(outputs, tone): (FakeOutputs, PathBuf)| {
            let _ = play_audio_file_on(&outputs.host(), &tone, None, 1.0, QUICK_LIMIT);
        })
        .unwrap();
        let on_headset = FakeOutputs::new(&[&headset]).with_default(&headset);
        let on_speakers = FakeOutputs::new(&[&speakers]).with_default(&speakers);
        assert!(queue.enqueue((on_headset, tone.clone())).is_ok());
        let (played, waited) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = played.send(queue.play_and_wait((on_speakers, tone)).is_ok());
        });
        assert_eq!(
            waited.recv_timeout(Duration::from_secs(2)),
            Ok(true),
            "the caller got its sound within 2 s"
        );
        assert_eq!(headset.errors_reported(), 1);
        assert!(speakers.peak() > 0.01, "the next sound played");
    }

    #[test]
    fn a_sound_may_take_its_length_and_a_second_or_five_seconds_without_one() {
        let limit = SoundTimeLimit::FEEDBACK;
        assert_eq!(
            limit.for_length(Some(Duration::from_millis(580))),
            Duration::from_millis(1580)
        );
        assert_eq!(limit.for_length(None), Duration::from_secs(5));
    }

    /// The loudest sample a device gets of the start sound at `volume`. The
    /// device waits before it asks for audio: rodio reads the volume at the
    /// sound's first sample, and the volume is set just after it is queued.
    fn peak_at(volume: f32) -> f32 {
        let device = speakers().running(Running::Stalls);
        let outputs = FakeOutputs::new(&[&device]).with_default(&device);
        let result = play_in_background(&outputs, None, volume);
        std::thread::sleep(Duration::from_millis(200));
        device.set_running(Running::Pulls);
        assert_eq!(
            result
                .recv_timeout(Duration::from_secs(5))
                .expect("the sound ends"),
            PLAYED
        );
        device.peak()
    }

    #[test]
    fn the_device_gets_the_sound_at_the_volume_asked_for() {
        let full = peak_at(1.0);
        let quarter = peak_at(0.25);
        assert!(full > 0.01, "{full}");
        assert!((quarter / full - 0.25).abs() < 0.01, "{quarter} of {full}");
    }

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
    fn the_done_sound_is_the_themes_high_clipboard_sound() {
        // The user's wish (2026-11-15): the high E that ends every dictation
        // without Accessibility.
        assert_eq!(
            sound_location(SoundTheme::Marimba, SoundType::Done, NO_CUSTOM_FILES),
            resource("resources/marimba_clipboard.wav")
        );
        assert_eq!(
            sound_location(SoundTheme::Pop, SoundType::Done, NO_CUSTOM_FILES),
            resource("resources/pop_clipboard.wav")
        );
    }

    #[test]
    fn the_custom_theme_uses_its_own_done_sound_when_there_is_one() {
        assert_eq!(
            sound_location(SoundTheme::Custom, SoundType::Done, |file| {
                file == "custom_done.wav" || file == "custom_clipboard.wav"
            }),
            app_data("custom_done.wav")
        );
    }

    #[test]
    fn without_a_done_sound_the_custom_theme_uses_its_clipboard_sound() {
        assert_eq!(
            sound_location(SoundTheme::Custom, SoundType::Done, |file| {
                file == "custom_clipboard.wav"
            }),
            app_data("custom_clipboard.wav")
        );
        assert_eq!(
            sound_location(SoundTheme::Custom, SoundType::Done, NO_CUSTOM_FILES),
            resource("resources/marimba_clipboard.wav")
        );
    }

    #[test]
    fn the_clipboard_sound_never_takes_the_custom_done_sound() {
        assert_eq!(
            sound_location(SoundTheme::Custom, SoundType::Clipboard, |file| {
                file == "custom_done.wav"
            }),
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
