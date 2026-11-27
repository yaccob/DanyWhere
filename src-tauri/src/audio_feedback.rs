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

/// The events a feedback sound marks. DanyWhere: each has a pitch of its own,
/// rising from the first to the last (user, 2026-11-27): start E4,
/// instruction B4, stop E5, done G♯5, and the waiting sound as the done one;
/// a cancel falls below them all, to B3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundType {
    /// DanyWhere: a dictation was cancelled (Escape, the overlay's ✕, the
    /// tray's Cancel; `utils::cancel_current_operation`).
    Cancel,
    Start,
    /// DanyWhere: an instruction key cut the recording for a spoken
    /// instruction (`dictation_flow::instruction::begin`).
    Instruction,
    Stop,
    /// DanyWhere: the transcript was left on the clipboard, and the user has
    /// to paste it; "waiting" in the sound file.
    Clipboard,
    /// DanyWhere: a dictation is completely done, post-processing included,
    /// however it was delivered (`delivery::sound_after_dictation`).
    Done,
}

impl SoundType {
    /// Every event, in the order a dictation plays them and the cancel last,
    /// as the sound file lists them.
    pub const ALL: [SoundType; 6] = [
        SoundType::Start,
        SoundType::Instruction,
        SoundType::Stop,
        SoundType::Done,
        SoundType::Clipboard,
        SoundType::Cancel,
    ];

    /// The event's name in the sound file (`sound_file`).
    pub fn event_name(self) -> &'static str {
        match self {
            SoundType::Cancel => "cancel",
            SoundType::Start => "start",
            SoundType::Instruction => "instruction",
            SoundType::Stop => "stop",
            SoundType::Done => "done",
            SoundType::Clipboard => "waiting",
        }
    }
}

/// DanyWhere: the bundled sound `theme` plays for `sound_type`, in
/// `resources/`. The files are named after their pitch, not their role, so a
/// role can move to another pitch without a file changing its meaning;
/// `the_bundled_sounds_are_named_after_their_pitch` measures them. The custom
/// theme borrows Marimba's where it has no file of its own.
fn bundled(theme: SoundTheme, sound_type: SoundType) -> &'static str {
    match (theme, sound_type) {
        // A cancel a fourth below the start: Handy's own B3 stroke, and the
        // B4 pop an octave down.
        (SoundTheme::Pop, SoundType::Cancel) => "pop_b3.wav",
        (SoundTheme::Marimba | SoundTheme::Custom, SoundType::Cancel) => "marimba_b3.wav",
        (SoundTheme::Pop, SoundType::Start) => "pop_e4.wav",
        (SoundTheme::Pop, SoundType::Instruction) => "pop_b4.wav",
        (SoundTheme::Pop, SoundType::Stop) => "pop_e5.wav",
        // Two pops, D♯5 then G♯5: "done" was always two pops in this theme.
        (SoundTheme::Pop, SoundType::Done | SoundType::Clipboard) => "pop_double_gsharp5.wav",
        (SoundTheme::Marimba | SoundTheme::Custom, SoundType::Start) => "marimba_e4.wav",
        (SoundTheme::Marimba | SoundTheme::Custom, SoundType::Instruction) => "marimba_b4.wav",
        (SoundTheme::Marimba | SoundTheme::Custom, SoundType::Stop) => "marimba_e5.wav",
        // The waiting sound is the done sound: a step below it, it would be
        // the stop sound twice after a dictation with the done sound off.
        (SoundTheme::Marimba | SoundTheme::Custom, SoundType::Done | SoundType::Clipboard) => {
            "marimba_gsharp5.wav"
        }
    }
}

/// DanyWhere: the custom theme's own files for `sound_type` in the app data
/// folder, the first that exists playing, and whether Marimba's sound plays
/// when none does. A custom theme needs only a start and a stop sound; the
/// done sound is its clipboard sound without a file of its own (user,
/// 2026-11-15).
fn custom_files(sound_type: SoundType) -> (&'static [&'static str], bool) {
    match sound_type {
        SoundType::Start => (&["custom_start.wav"], false),
        SoundType::Stop => (&["custom_stop.wav"], false),
        SoundType::Instruction => (&["custom_instruction.wav"], true),
        SoundType::Done => (&["custom_done.wav", "custom_clipboard.wav"], true),
        SoundType::Clipboard => (&["custom_clipboard.wav"], true),
        SoundType::Cancel => (&["custom_cancel.wav"], true),
    }
}

/// Where a feedback sound's file is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SoundLocation {
    /// Bundled with the app, relative to its resources.
    Resource(String),
    /// The user's own file in the app data folder (custom theme, sound file).
    AppData(String),
    /// DanyWhere: a file the sound file names by its full path.
    File(PathBuf),
}

fn resource(name: &str) -> SoundLocation {
    SoundLocation::Resource(format!("resources/{name}"))
}

/// The sound `theme` plays for `sound_type` without the sound file. `exists`
/// tells whether a file is there (the custom theme's own files).
fn default_location(
    theme: SoundTheme,
    sound_type: SoundType,
    exists: impl Fn(&SoundLocation) -> bool,
) -> SoundLocation {
    if theme != SoundTheme::Custom {
        return resource(bundled(theme, sound_type));
    }
    let (files, marimba) = custom_files(sound_type);
    files
        .iter()
        .map(|file| SoundLocation::AppData(file.to_string()))
        .find(|location| exists(location))
        .unwrap_or_else(|| {
            if marimba {
                resource(bundled(SoundTheme::Marimba, sound_type))
            } else {
                SoundLocation::AppData(files[0].to_string())
            }
        })
}

/// DanyWhere: how the sound file writes the sound `theme` plays for
/// `sound_type` without it, and what stands in when that file is missing.
pub(crate) fn default_written(
    theme: SoundTheme,
    sound_type: SoundType,
) -> (String, Option<String>) {
    let built_in = |theme| {
        format!(
            "{}{}",
            crate::sound_file::BUILT_IN_PREFIX,
            bundled(theme, sound_type)
        )
    };
    if theme != SoundTheme::Custom {
        return (built_in(theme), None);
    }
    let (files, marimba) = custom_files(sound_type);
    let mut instead: Vec<String> = files[1..].iter().map(|file| file.to_string()).collect();
    if marimba {
        instead.push(built_in(SoundTheme::Marimba));
    }
    let instead = (!instead.is_empty()).then(|| instead.join(", else "));
    (files[0].to_string(), instead)
}

/// DanyWhere: where the sound file's `file` points: `built-in:` among the
/// bundled sounds, `~/` under `home`, an absolute path as it is, anything
/// else in the app data folder, next to the sound file.
fn location_of(file: &crate::sound_file::FileRef, home: Option<&Path>) -> SoundLocation {
    use crate::sound_file::FileRef;
    match file {
        FileRef::BuiltIn(name) => resource(name),
        FileRef::Path(path) => match (path.strip_prefix("~/"), home) {
            (Some(rest), Some(home)) => SoundLocation::File(home.join(rest)),
            _ if Path::new(path).is_absolute() => SoundLocation::File(PathBuf::from(path)),
            _ => SoundLocation::AppData(path.clone()),
        },
    }
}

/// DanyWhere: what plays for an event: the file and how many semitones
/// higher.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SoundChoice {
    pub location: SoundLocation,
    pub semitones: f32,
}

/// DanyWhere: the sound `theme` plays for `sound_type`, as the sound file's
/// `entry` says, if it has one: its file, if that exists, else the built-in
/// sound unshifted, with the problem to log.
fn sound_choice(
    theme: SoundTheme,
    sound_type: SoundType,
    entry: Option<&crate::sound_file::Entry>,
    home: Option<&Path>,
    exists: impl Fn(&SoundLocation) -> bool,
) -> (SoundChoice, Option<String>) {
    let built_in = |exists| SoundChoice {
        location: default_location(theme, sound_type, exists),
        semitones: 0.0,
    };
    let Some(entry) = entry else {
        return (built_in(&exists), None);
    };
    let Some(file) = &entry.file else {
        return (
            SoundChoice {
                semitones: entry.semitones,
                ..built_in(&exists)
            },
            None,
        );
    };
    let location = location_of(file, home);
    if exists(&location) {
        return (
            SoundChoice {
                location,
                semitones: entry.semitones,
            },
            None,
        );
    }
    let named = match file {
        crate::sound_file::FileRef::BuiltIn(name) => {
            format!("{}{name}", crate::sound_file::BUILT_IN_PREFIX)
        }
        crate::sound_file::FileRef::Path(path) => path.clone(),
    };
    let problem = format!(
        "[{}] {}: \"{named}\" is not there",
        crate::sound_file::theme_name(theme),
        sound_type.event_name()
    );
    (built_in(&exists), Some(problem))
}

/// The file of `location` on this computer.
fn location_path(app: &AppHandle, location: &SoundLocation) -> Option<PathBuf> {
    match location {
        SoundLocation::AppData(file) => crate::portable::resolve_app_data(app, file).ok(),
        SoundLocation::Resource(file) => app
            .path()
            .resolve(file, tauri::path::BaseDirectory::Resource)
            .ok(),
        SoundLocation::File(path) => Some(path.clone()),
    }
}

/// DanyWhere: the file a sound plays and how many semitones higher, as the
/// theme of `settings` and the sound file say.
struct ResolvedSound {
    path: PathBuf,
    semitones: f32,
}

fn resolve_sound(
    app: &AppHandle,
    settings: &AppSettings,
    sound_type: SoundType,
) -> Option<ResolvedSound> {
    let theme = settings.sound_theme;
    let file = crate::sound_file::current(app);
    let home = app.path().home_dir().ok();
    let exists =
        |location: &SoundLocation| location_path(app, location).is_some_and(|path| path.exists());
    let (choice, problem) = sound_choice(
        theme,
        sound_type,
        file.entry(theme, sound_type),
        home.as_deref(),
        exists,
    );
    if let Some(problem) = problem {
        crate::sound_file::warn(app, &problem);
    }
    Some(ResolvedSound {
        path: location_path(app, &choice.location)?,
        semitones: choice.semitones,
    })
}

/// DanyWhere: whether the custom theme's `sound_type` has a file (its own,
/// or the one the sound file names), for offering the theme at all.
pub fn custom_sound_exists(app: &AppHandle, sound_type: SoundType) -> bool {
    let mut settings = settings::get_settings(app);
    settings.sound_theme = SoundTheme::Custom;
    resolve_sound(app, &settings, sound_type).is_some_and(|sound| sound.path.exists())
}

/// DanyWhere: whether a dictation's feedback sounds play at all, as its
/// profile says.
pub fn feedback_on(app: &AppHandle) -> bool {
    crate::app_profiles::dictation_settings(app).audio_feedback
}

pub fn play_feedback_sound(app: &AppHandle, sound_type: SoundType) {
    // DanyWhere: as the dictation's profile says, if it has its own.
    let settings = crate::app_profiles::dictation_settings(app);
    if !settings.audio_feedback {
        return;
    }
    if let Some(sound) = resolve_sound(app, &settings, sound_type) {
        play_sound_async(sound.path, SoundOutput::of(&settings), sound.semitones);
    }
}

pub fn play_feedback_sound_blocking(app: &AppHandle, sound_type: SoundType) {
    // DanyWhere: as the dictation's profile says, if it has its own.
    let settings = crate::app_profiles::dictation_settings(app);
    if !settings.audio_feedback {
        return;
    }
    if let Some(sound) = resolve_sound(app, &settings, sound_type) {
        play_sound_blocking(&sound.path, SoundOutput::of(&settings), sound.semitones);
    }
}

pub fn play_test_sound(app: &AppHandle, sound_type: SoundType) {
    let settings = settings::get_settings(app);
    if let Some(sound) = resolve_sound(app, &settings, sound_type) {
        play_sound_blocking(&sound.path, SoundOutput::of(&settings), sound.semitones);
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
    /// DanyWhere: how many semitones higher it plays (sound file).
    semitones: f32,
}

/// DanyWhere: every feedback sound goes through one queue, so sounds never
/// overlap (see `sound_queue`). `None` if its thread could not be started;
/// sounds then play as in Handy.
fn sound_queue() -> Option<&'static SoundQueue<SoundJob>> {
    static QUEUE: OnceLock<Option<SoundQueue<SoundJob>>> = OnceLock::new();
    QUEUE
        .get_or_init(|| {
            SoundQueue::new(|job: SoundJob| play_sound_now(&job))
                .map_err(|e| error!("Failed to start the feedback sound thread: {}", e))
                .ok()
        })
        .as_ref()
}

fn play_sound_async(path: PathBuf, output: SoundOutput, semitones: f32) {
    let job = SoundJob {
        path,
        output,
        semitones,
    };
    let job = match sound_queue() {
        Some(queue) => match queue.enqueue(job) {
            Ok(()) => return,
            Err(job) => job,
        },
        None => job,
    };
    thread::spawn(move || play_sound_now(&job));
}

fn play_sound_blocking(path: &Path, output: SoundOutput, semitones: f32) {
    let job = SoundJob {
        path: path.to_path_buf(),
        output,
        semitones,
    };
    let job = match sound_queue() {
        Some(queue) => match queue.play_and_wait(job) {
            Ok(()) => return,
            Err(job) => job,
        },
        None => job,
    };
    play_sound_now(&job);
}

fn play_sound_now(job: &SoundJob) {
    let path = &job.path;
    match play_audio_file_on(
        &SystemOutputHost,
        path,
        job.output.device.as_deref(),
        job.output.volume,
        SoundTimeLimit::FEEDBACK,
        job.semitones,
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
    semitones: f32,
) -> Result<SoundEnd, Box<dyn std::error::Error>> {
    let device = feedback_device(outputs, selected_device)?;
    let device_sink = open_sink(device)?;
    let mixer = device_sink.mixer();

    let file = File::open(path)?;
    // DanyWhere: the sound file's pitch shift, as a faster or slower
    // playback: rodio's `speed` raises the rate the samples are read at,
    // so the pitch rises and the sound gets shorter (its length too, which
    // the time limit takes). A factor of 1 changes nothing.
    let sound = Decoder::new(BufReader::new(file))
        .map_err(PlayError::from)?
        .speed(2f32.powf(semitones / 12.0));
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
        bundled, default_location, error_chain, failure_line, fitted_buffer, play_audio_file_on,
        sound_choice, SoundChoice, SoundEnd, SoundLocation, SoundQueue, SoundTimeLimit, SoundType,
    };
    use crate::audio_toolkit::audio::fake_output::{FakeOutput, FakeOutputs, Running};
    use crate::settings::SoundTheme;
    use crate::sound_file::{Entry, FileRef};
    use rodio::cpal::{BufferSize, ErrorKind, SupportedBufferSize};
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    // DanyWhere: feedback sounds on fake output devices (`fake_output`).

    /// The bundled start sound, a fraction of a second long.
    fn sound() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join(bundled(SoundTheme::Marimba, SoundType::Start))
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
        semitones: f32,
    ) -> mpsc::Receiver<Result<SoundEnd, String>> {
        let outputs = outputs.clone();
        let selected = selected.map(str::to_string);
        let (done, result) = mpsc::channel();
        std::thread::spawn(move || {
            let played = play_audio_file_on(
                &outputs.host(),
                &sound,
                selected.as_deref(),
                volume,
                limit,
                semitones,
            )
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
        play_file_in_background(
            outputs,
            sound(),
            selected,
            volume,
            SoundTimeLimit::FEEDBACK,
            0.0,
        )
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
            0.0,
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
        let result = play_file_in_background(&outputs, tone, None, 1.0, QUICK_LIMIT, 0.0);
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
            let _ = play_audio_file_on(&outputs.host(), &tone, None, 1.0, QUICK_LIMIT, 0.0);
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

    /// A sound shifted by `semitones`, played on a fake device at 44.1 kHz:
    /// the strongest frequency the device got, and how long it lasted.
    fn heard_shifted(semitones: f32) -> (f64, Duration) {
        let dir = tempfile::tempdir().unwrap();
        let tone = short_tone(dir.path(), 300);
        let device =
            speakers().offering(44_100, 2, SupportedBufferSize::Range { min: 14, max: 4096 });
        let outputs = FakeOutputs::new(&[&device]).with_default(&device);
        let played = play_file_in_background(&outputs, tone, None, 1.0, QUICK_LIMIT, semitones)
            .recv_timeout(Duration::from_secs(5))
            .expect("the sound ends");
        assert_eq!(played, PLAYED);
        let (heard, rate) = device.heard();
        let samples: Vec<f64> = heard.iter().map(|&s| f64::from(s)).collect();
        let length = Duration::from_secs_f64(samples.len() as f64 / f64::from(rate));
        (strongest_frequency(&samples, rate), length)
    }

    #[test]
    fn a_shifted_sound_reaches_the_device_higher_and_shorter() {
        // DanyWhere: `semitones` in the sound file, played like a faster or
        // slower playback.
        let (plain, plain_length) = heard_shifted(0.0);
        assert!((plain / 440.0 - 1.0).abs() < 0.03, "{plain} Hz");
        let (octave_up, up_length) = heard_shifted(12.0);
        assert!((octave_up / 880.0 - 1.0).abs() < 0.03, "{octave_up} Hz");
        let (fifth_down, down_length) = heard_shifted(-7.0);
        let fifth = 440.0 * 2f64.powf(-7.0 / 12.0);
        assert!((fifth_down / fifth - 1.0).abs() < 0.03, "{fifth_down} Hz");
        let ratio = |a: Duration, b: Duration| a.as_secs_f64() / b.as_secs_f64();
        assert!(
            (ratio(up_length, plain_length) - 0.5).abs() < 0.1,
            "{up_length:?} of {plain_length:?}"
        );
        assert!(
            (ratio(down_length, plain_length) - 1.5).abs() < 0.1,
            "{down_length:?} of {plain_length:?}"
        );
    }

    fn resource(name: &str) -> SoundLocation {
        SoundLocation::Resource(format!("resources/{name}"))
    }

    fn app_data(file: &str) -> SoundLocation {
        SoundLocation::AppData(file.to_string())
    }

    const NO_FILES: fn(&SoundLocation) -> bool = |_| false;

    /// Only the custom files named are in the app data folder.
    fn custom_files_there(files: &'static [&'static str]) -> impl Fn(&SoundLocation) -> bool {
        move |location| matches!(location, SoundLocation::AppData(file) if files.contains(&file.as_str()))
    }

    /// The bundled sounds of a theme for start, instruction, stop, done and
    /// waiting.
    fn bundled_set(theme: SoundTheme) -> Vec<SoundLocation> {
        SoundType::ALL
            .into_iter()
            .map(|sound_type| default_location(theme, sound_type, NO_FILES))
            .collect()
    }

    #[test]
    fn marimba_rises_from_e4_over_b4_and_e5_to_g_sharp_5() {
        // The user's wish (2026-11-27): one fixed pitch per event.
        assert_eq!(
            bundled_set(SoundTheme::Marimba),
            [
                resource("marimba_e4.wav"),
                resource("marimba_b4.wav"),
                resource("marimba_e5.wav"),
                resource("marimba_gsharp5.wav"),
                resource("marimba_gsharp5.wav"),
                resource("marimba_b3.wav"),
            ]
        );
    }

    #[test]
    fn pop_rises_the_same_way_and_ends_with_its_two_pops() {
        assert_eq!(
            bundled_set(SoundTheme::Pop),
            [
                resource("pop_e4.wav"),
                resource("pop_b4.wav"),
                resource("pop_e5.wav"),
                resource("pop_double_gsharp5.wav"),
                resource("pop_double_gsharp5.wav"),
                resource("pop_b3.wav"),
            ]
        );
    }

    /// A cancel falls below the start (user, 2026-11-27: "H3 … oder E3"):
    /// B3, a fourth below E4, at least a whole tone below it.
    fn assert_cancel_lowest(theme: SoundTheme) {
        let pitch = |sound_type| {
            let name = bundled(theme, sound_type);
            let (samples, rate) = read_bundled(name);
            (name, strongest_frequency(&samples, rate))
        };
        let (cancel, cancel_hz) = pitch(SoundType::Cancel);
        let (start, start_hz) = pitch(SoundType::Start);
        assert!(
            cancel_hz * 2f64.powf(2.0 / 12.0) < start_hz,
            "{theme:?}: {cancel} ({cancel_hz:.0} Hz) is not a whole tone below {start} ({start_hz:.0} Hz)"
        );
    }

    #[test]
    fn the_marimba_cancel_sound_is_below_the_start_sound() {
        assert_cancel_lowest(SoundTheme::Marimba);
    }

    #[test]
    fn the_pop_cancel_sound_is_below_the_start_sound() {
        assert_cancel_lowest(SoundTheme::Pop);
    }

    #[test]
    fn the_custom_theme_plays_its_own_files() {
        let all = custom_files_there(&[
            "custom_start.wav",
            "custom_instruction.wav",
            "custom_stop.wav",
            "custom_done.wav",
            "custom_clipboard.wav",
            "custom_cancel.wav",
        ]);
        let played: Vec<_> = SoundType::ALL
            .into_iter()
            .map(|sound_type| default_location(SoundTheme::Custom, sound_type, &all))
            .collect();
        assert_eq!(
            played,
            [
                app_data("custom_start.wav"),
                app_data("custom_instruction.wav"),
                app_data("custom_stop.wav"),
                app_data("custom_done.wav"),
                app_data("custom_clipboard.wav"),
                app_data("custom_cancel.wav"),
            ]
        );
    }

    #[test]
    fn without_files_of_its_own_the_custom_theme_borrows_marimbas_sounds() {
        // Start and stop stay its own: without them the theme isn't offered.
        let played: Vec<_> = SoundType::ALL
            .into_iter()
            .map(|sound_type| default_location(SoundTheme::Custom, sound_type, NO_FILES))
            .collect();
        assert_eq!(
            played,
            [
                app_data("custom_start.wav"),
                resource("marimba_b4.wav"),
                app_data("custom_stop.wav"),
                resource("marimba_gsharp5.wav"),
                resource("marimba_gsharp5.wav"),
                resource("marimba_b3.wav"),
            ]
        );
    }

    #[test]
    fn the_custom_done_sound_is_its_clipboard_sound_without_a_file_of_its_own() {
        let clipboard_only = custom_files_there(&["custom_clipboard.wav"]);
        assert_eq!(
            default_location(SoundTheme::Custom, SoundType::Done, &clipboard_only),
            app_data("custom_clipboard.wav")
        );
        let done_only = custom_files_there(&["custom_done.wav"]);
        assert_eq!(
            default_location(SoundTheme::Custom, SoundType::Clipboard, &done_only),
            resource("marimba_gsharp5.wav"),
            "the clipboard sound never takes the done sound"
        );
    }

    /// What the sound file's `entry` makes `theme` play for `sound_type`,
    /// with `there` the files that exist and `/Users/me` the home folder.
    fn chosen(
        theme: SoundTheme,
        sound_type: SoundType,
        entry: Entry,
        there: impl Fn(&SoundLocation) -> bool,
    ) -> (SoundChoice, Option<String>) {
        sound_choice(
            theme,
            sound_type,
            Some(&entry),
            Some(Path::new("/Users/me")),
            there,
        )
    }

    fn file(path: &str) -> Entry {
        Entry {
            file: Some(FileRef::Path(path.to_string())),
            semitones: 0.0,
        }
    }

    const ALL_THERE: fn(&SoundLocation) -> bool = |_| true;

    #[test]
    fn the_sound_file_names_a_file_next_to_it_in_the_home_folder_or_anywhere() {
        for (written, location) in [
            ("my-stop.wav", app_data("my-stop.wav")),
            ("sounds/my-stop.wav", app_data("sounds/my-stop.wav")),
            (
                "~/Sounds/stop.flac",
                SoundLocation::File(PathBuf::from("/Users/me/Sounds/stop.flac")),
            ),
            (
                "/Library/Sounds/stop.wav",
                SoundLocation::File(PathBuf::from("/Library/Sounds/stop.wav")),
            ),
        ] {
            let (choice, problem) = chosen(
                SoundTheme::Marimba,
                SoundType::Stop,
                file(written),
                ALL_THERE,
            );
            assert_eq!(choice.location, location, "{written}");
            assert_eq!(choice.semitones, 0.0);
            assert_eq!(problem, None);
        }
    }

    #[test]
    fn the_sound_file_can_name_a_built_in_sound_for_another_event() {
        let entry = Entry {
            file: Some(FileRef::BuiltIn("marimba_b4.wav".to_string())),
            semitones: -12.0,
        };
        let (choice, problem) = chosen(SoundTheme::Pop, SoundType::Done, entry, ALL_THERE);
        assert_eq!(
            choice,
            SoundChoice {
                location: resource("marimba_b4.wav"),
                semitones: -12.0
            }
        );
        assert_eq!(problem, None);
    }

    #[test]
    fn semitones_alone_shift_the_built_in_sound() {
        let entry = Entry {
            file: None,
            semitones: 2.0,
        };
        let (choice, problem) = chosen(SoundTheme::Marimba, SoundType::Stop, entry, ALL_THERE);
        assert_eq!(
            choice,
            SoundChoice {
                location: resource("marimba_e5.wav"),
                semitones: 2.0
            }
        );
        assert_eq!(problem, None);
    }

    #[test]
    fn a_missing_file_plays_the_built_in_sound_unshifted_and_says_so() {
        let entry = Entry {
            file: Some(FileRef::Path("gone.wav".to_string())),
            semitones: 3.0,
        };
        let only_bundled =
            |location: &SoundLocation| matches!(location, SoundLocation::Resource(_));
        let (choice, problem) = chosen(
            SoundTheme::Marimba,
            SoundType::Instruction,
            entry,
            only_bundled,
        );
        assert_eq!(
            choice,
            SoundChoice {
                location: resource("marimba_b4.wav"),
                semitones: 0.0
            }
        );
        let problem = problem.expect("a problem to log");
        assert!(problem.contains("[marimba] instruction"), "{problem}");
        assert!(problem.contains("gone.wav"), "{problem}");
    }

    #[test]
    fn without_an_entry_the_built_in_sound_plays() {
        let (choice, problem) = sound_choice(
            SoundTheme::Pop,
            SoundType::Instruction,
            None,
            None,
            ALL_THERE,
        );
        assert_eq!(
            choice,
            SoundChoice {
                location: resource("pop_b4.wav"),
                semitones: 0.0
            }
        );
        assert_eq!(problem, None);
    }

    #[test]
    fn the_sound_file_writes_each_built_in_sound_as_the_app_finds_it() {
        use crate::sound_file::{BUILT_IN_PREFIX, THEMES};
        for theme in THEMES {
            for sound_type in SoundType::ALL {
                let (written, instead) = super::default_written(theme, sound_type);
                let expected = match written.strip_prefix(BUILT_IN_PREFIX) {
                    Some(name) => resource(name),
                    None => app_data(&written),
                };
                let all_custom_files_there =
                    |location: &SoundLocation| matches!(location, SoundLocation::AppData(_));
                assert_eq!(
                    default_location(theme, sound_type, all_custom_files_there),
                    expected,
                    "{theme:?} {sound_type:?}"
                );
                if let Some(instead) = instead {
                    let SoundLocation::Resource(fallback) =
                        default_location(theme, sound_type, NO_FILES)
                    else {
                        panic!("{theme:?} {sound_type:?}: no bundled sound stands in");
                    };
                    let fallback = fallback.trim_start_matches("resources/");
                    assert!(instead.ends_with(fallback), "{instead} vs {fallback}");
                }
            }
        }
    }

    /// Every bundled sound of the themes, once each.
    fn all_bundled() -> Vec<String> {
        let mut names: Vec<String> = [SoundTheme::Marimba, SoundTheme::Pop]
            .into_iter()
            .flat_map(|theme| {
                SoundType::ALL.map(|sound_type| bundled(theme, sound_type).to_string())
            })
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// A bundled sound: its samples mixed to mono between -1 and 1, and its
    /// sample rate.
    fn read_bundled(name: &str) -> (Vec<f64>, u32) {
        let file = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join(name);
        let mut reader =
            hound::WavReader::open(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        let spec = reader.spec();
        assert_eq!(
            (spec.sample_format, spec.bits_per_sample),
            (hound::SampleFormat::Int, 16),
            "{name}"
        );
        let channels = usize::from(spec.channels);
        let samples: Vec<i16> = reader
            .samples::<i16>()
            .map(|sample| sample.unwrap_or_else(|e| panic!("{name}: {e}")))
            .collect();
        let mono = samples
            .chunks(channels)
            .map(|frame| {
                frame.iter().map(|&s| f64::from(s)).sum::<f64>() / channels as f64 / 32768.0
            })
            .collect();
        (mono, spec.sample_rate)
    }

    #[test]
    fn the_bundled_sounds_are_short_wav_files() {
        for name in all_bundled() {
            let (samples, rate) = read_bundled(&name);
            let seconds = samples.len() as f64 / f64::from(rate);
            assert!((0.1..1.0).contains(&seconds), "{name} lasts {seconds} s");
        }
    }

    /// The strongest frequency of a sound just after its attack: a Hann
    /// window of 8192 samples from 10 ms after its loudest sample, scanned
    /// with the Goertzel algorithm in quarter semitones from 100 Hz to 5 kHz.
    fn strongest_frequency(samples: &[f64], rate: u32) -> f64 {
        const LENGTH: usize = 8192;
        let loudest = (0..samples.len())
            .max_by(|&a, &b| samples[a].abs().total_cmp(&samples[b].abs()))
            .expect("the sound has samples");
        let start = (loudest + rate as usize / 100).min(samples.len().saturating_sub(LENGTH));
        let window = &samples[start..samples.len().min(start + LENGTH)];
        let last = (window.len() - 1) as f64;
        let windowed: Vec<f64> = window
            .iter()
            .enumerate()
            .map(|(i, x)| x * (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / last).cos()))
            .collect();
        let power = |freq: f64| {
            let coeff = 2.0 * (std::f64::consts::TAU * freq / f64::from(rate)).cos();
            let (mut s1, mut s2) = (0.0, 0.0);
            for x in &windowed {
                let s0 = x + coeff * s1 - s2;
                s2 = s1;
                s1 = s0;
            }
            s1 * s1 + s2 * s2 - coeff * s1 * s2
        };
        let mut strongest = (0.0, 0.0);
        let mut freq: f64 = 100.0;
        while freq < 5000.0 {
            let p = power(freq);
            if p > strongest.0 {
                strongest = (p, freq);
            }
            freq *= 2f64.powf(1.0 / 48.0);
        }
        strongest.1
    }

    /// The frequency of the note a bundled sound's name ends in:
    /// `marimba_gsharp5.wav` → G♯5, 830.6 Hz.
    fn pitch_of_name(name: &str) -> f64 {
        let note = name
            .trim_end_matches(".wav")
            .rsplit('_')
            .next()
            .expect("a note at the end of the name");
        let (letter, rest) = note.split_at(1);
        let (sharp, octave) = match rest.strip_prefix("sharp") {
            Some(octave) => (1, octave),
            None => (0, rest),
        };
        let semitone = match letter {
            "c" => 0,
            "d" => 2,
            "e" => 4,
            "f" => 5,
            "g" => 7,
            "a" => 9,
            "b" => 11,
            _ => panic!("{name}: no note"),
        } + sharp;
        let octave: i32 = octave
            .parse()
            .unwrap_or_else(|_| panic!("{name}: no octave"));
        let midi = 12 * (octave + 1) + semitone;
        440.0 * 2f64.powf(f64::from(midi - 69) / 12.0)
    }

    #[test]
    fn the_bundled_sounds_are_named_after_their_pitch() {
        for name in all_bundled() {
            let (samples, rate) = read_bundled(&name);
            let measured = strongest_frequency(&samples, rate);
            let named = pitch_of_name(&name);
            let cents = 1200.0 * (measured / named).log2();
            assert!(
                cents.abs() <= 50.0,
                "{name}: {measured:.0} Hz, {cents:+.0} cents from {named:.0} Hz"
            );
        }
    }

    /// The pitch shows how far a dictation has got (user, 2026-11-24 and
    /// 2026-11-27): start, instruction, stop and done each a step higher,
    /// at least a whole tone, so it can be heard.
    fn assert_rising(theme: SoundTheme) {
        let pitches: Vec<(&str, f64)> = [
            SoundType::Start,
            SoundType::Instruction,
            SoundType::Stop,
            SoundType::Done,
        ]
        .into_iter()
        .map(|sound_type| {
            let name = bundled(theme, sound_type);
            let (samples, rate) = read_bundled(name);
            (name, strongest_frequency(&samples, rate))
        })
        .collect();
        for pair in pitches.windows(2) {
            let [(lower, lower_hz), (higher, higher_hz)] = pair else {
                unreachable!()
            };
            assert!(
                *higher_hz > lower_hz * 2f64.powf(2.0 / 12.0),
                "{theme:?}: {higher} ({higher_hz:.0} Hz) is not a whole tone above \
                 {lower} ({lower_hz:.0} Hz); all: {pitches:?}"
            );
        }
    }

    #[test]
    fn the_marimba_sounds_rise_from_start_to_instruction_to_stop_to_done() {
        assert_rising(SoundTheme::Marimba);
    }

    #[test]
    fn the_pop_sounds_rise_from_start_to_instruction_to_stop_to_done() {
        assert_rising(SoundTheme::Pop);
    }

    /// The start sound rings while the recording already runs and the user
    /// begins to speak, and muting the other sounds waits for its end
    /// (`dictation_start`), so it stays as short as Handy's.
    #[test]
    fn the_bundled_start_sounds_last_at_most_six_tenths_of_a_second() {
        for theme in [SoundTheme::Marimba, SoundTheme::Pop] {
            let name = bundled(theme, SoundType::Start);
            let (samples, rate) = read_bundled(name);
            let seconds = samples.len() as f64 / f64::from(rate);
            assert!(seconds <= 0.6, "{name} lasts {seconds:.2} s");
        }
    }
}
