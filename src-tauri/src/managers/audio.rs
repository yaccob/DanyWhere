use crate::audio_toolkit::{
    audio::{AudioFrameCallback, LevelCallback, MicrophoneDevice, RecordingSections},
    vad::{
        frames_for_duration_ms, EarshotVad, SmoothedVad, VAD_OFFLINE_HANGOVER_MS, VAD_ONSET_MS,
        VAD_PREFILL_MS, VAD_STREAMING_HANGOVER_MS,
    },
    CaptureSink, CaptureVad, SileroVad, SpeechScorer, VadPolicy, VoiceActivityDetector,
};
use crate::dictation_microphone::MicrophoneChoice;
use crate::helpers::clamshell;
use crate::managers::transcription::StreamRouter;
use crate::notices::NoticeCode;
use crate::settings::{get_settings, write_settings, AppSettings, VadBackend};
use crate::utils;
use handy_recorder::{Channels, EndReason, ErrorKind, Recorder, RecorderConfig, StartError};
use log::{debug, error, info, trace, warn};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};

const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const SILERO_VAD_THRESHOLD: f32 = 0.3;
const EARSHOT_VAD_THRESHOLD: f32 = 0.5;

fn set_mute(mute: bool) {
    // Expected behavior:
    // - Windows: works on most systems using standard audio drivers.
    // - Linux: works on many systems (PipeWire, PulseAudio, ALSA),
    //   but some distros may lack the tools used.
    // - macOS: works on most standard setups via AppleScript.
    // If unsupported, fails silently.

    #[cfg(target_os = "windows")]
    {
        unsafe {
            use windows::Win32::{
                Media::Audio::{
                    eMultimedia, eRender, Endpoints::IAudioEndpointVolume, IMMDeviceEnumerator,
                    MMDeviceEnumerator,
                },
                System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED},
            };

            macro_rules! unwrap_or_return {
                ($expr:expr) => {
                    match $expr {
                        Ok(val) => val,
                        Err(_) => return,
                    }
                };
            }

            // Initialize the COM library for this thread.
            // If already initialized (e.g., by another library like Tauri), this does nothing.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

            let all_devices: IMMDeviceEnumerator =
                unwrap_or_return!(CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL));
            let default_device =
                unwrap_or_return!(all_devices.GetDefaultAudioEndpoint(eRender, eMultimedia));
            let volume_interface = unwrap_or_return!(
                default_device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
            );

            let _ = volume_interface.SetMute(mute, std::ptr::null());
        }
    }

    #[cfg(target_os = "linux")]
    {
        use std::process::Command;

        let mute_val = if mute { "1" } else { "0" };
        let amixer_state = if mute { "mute" } else { "unmute" };

        // Try multiple backends to increase compatibility
        // 1. PipeWire (wpctl)
        if Command::new("wpctl")
            .args(["set-mute", "@DEFAULT_AUDIO_SINK@", mute_val])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return;
        }

        // 2. PulseAudio (pactl)
        if Command::new("pactl")
            .args(["set-sink-mute", "@DEFAULT_SINK@", mute_val])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return;
        }

        // 3. ALSA (amixer)
        let _ = Command::new("amixer")
            .args(["set", "Master", amixer_state])
            .output();
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        let script = format!(
            "set volume output muted {}",
            if mute { "true" } else { "false" }
        );
        let _ = Command::new("osascript").args(["-e", &script]).output();
    }
}

/// Reads the current system output mute state, mirroring `set_mute`'s backends.
///
/// Returns `Some(true)`/`Some(false)` when the state could be determined, or
/// `None` when it couldn't (unsupported platform, missing CLI tools, or an
/// error). Callers treat `None` as "unknown" and fall back to unmuting on stop,
/// so we never strand the user's audio muted.
#[cfg(target_os = "windows")]
fn get_mute() -> Option<bool> {
    unsafe {
        use windows::Win32::{
            Media::Audio::{
                eMultimedia, eRender, Endpoints::IAudioEndpointVolume, IMMDeviceEnumerator,
                MMDeviceEnumerator,
            },
            System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED},
        };

        // Matches set_mute: no-op if COM is already initialized on this thread.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let all_devices: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let default_device = all_devices
            .GetDefaultAudioEndpoint(eRender, eMultimedia)
            .ok()?;
        let volume_interface = default_device
            .Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
            .ok()?;

        Some(volume_interface.GetMute().ok()?.as_bool())
    }
}

#[cfg(target_os = "linux")]
fn get_mute() -> Option<bool> {
    use std::process::Command;

    // 1. PipeWire (wpctl): prints "[MUTED]" in the volume line when muted.
    if let Ok(out) = Command::new("wpctl")
        .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
        .output()
    {
        if out.status.success() {
            return Some(String::from_utf8_lossy(&out.stdout).contains("[MUTED]"));
        }
    }

    // 2. PulseAudio (pactl): prints "Mute: yes" / "Mute: no".
    // Force LC_ALL=C so a localized system still emits the parseable English
    // "yes"/"no" instead of e.g. "ja"/"nein".
    if let Ok(out) = Command::new("pactl")
        .env("LC_ALL", "C")
        .args(["get-sink-mute", "@DEFAULT_SINK@"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).to_lowercase();
            if s.contains("yes") {
                return Some(true);
            }
            if s.contains("no") {
                return Some(false);
            }
        }
    }

    // 3. ALSA (amixer): prints "[off]" for muted channels, "[on]" otherwise.
    // LC_ALL=C keeps the "[on]"/"[off]" tokens stable across locales.
    if let Ok(out) = Command::new("amixer")
        .env("LC_ALL", "C")
        .args(["get", "Master"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            if s.contains("[off]") {
                return Some(true);
            }
            if s.contains("[on]") {
                return Some(false);
            }
        }
    }

    None
}

#[cfg(target_os = "macos")]
fn get_mute() -> Option<bool> {
    use std::process::Command;

    let out = Command::new("osascript")
        .args(["-e", "output muted of (get volume settings)"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    match String::from_utf8_lossy(&out.stdout).trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn get_mute() -> Option<bool> {
    None
}

/// Restores the system mute state after our forced mute, given the state
/// captured just before we muted. We only ever need to unmute — and only when
/// the system was NOT already muted beforehand. If the prior state was muted,
/// we leave it muted (the user's own state). If it's unknown (`None`), we
/// default to unmuting so audio is never left stranded muted by us.
fn restore_mute(prev_muted: Option<bool>) {
    if prev_muted != Some(true) {
        set_mute(false);
    }
}

const WHISPER_SAMPLE_RATE: usize = 16000;

/// A recording shorter than a second, padded with silence to 1.25 s for the
/// models; an empty one stays empty.
fn padded_if_short(samples: Vec<f32>) -> Vec<f32> {
    if samples.len() < WHISPER_SAMPLE_RATE && !samples.is_empty() {
        let mut padded = samples;
        padded.resize(WHISPER_SAMPLE_RATE * 5 / 4, 0.0);
        padded
    } else {
        samples
    }
}

/* ──────────────────────────────────────────────────────────────── */

#[derive(Clone, Debug)]
pub enum RecordingState {
    Idle,
    Recording { binding_id: String },
    Stopping,
}

#[derive(Clone, Debug)]
pub enum MicrophoneMode {
    AlwaysOn,
    OnDemand,
}

/// Tracks our forced "mute while recording" so we can restore the user's audio
/// exactly as it was. `did_mute` is true while our mute is active; `prev_muted`
/// is the system mute state captured just before we muted, used to decide
/// whether to unmute on stop (so a system that was already muted stays muted).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct MuteState {
    did_mute: bool,
    prev_muted: Option<bool>,
    /// DanyWhere: our mute is lifted for a feedback sound in the middle of
    /// the recording (the instruction sound) and comes back after it.
    lifted: bool,
}

impl MuteState {
    /// DanyWhere: lifts our mute for a sound. `true`: restore the system's
    /// sound now.
    fn lift(&mut self) -> bool {
        if self.did_mute && !self.lifted {
            self.lifted = true;
            return true;
        }
        false
    }

    /// DanyWhere: puts a lifted mute back. `true`: mute now. Nothing once
    /// our mute ended meanwhile, so a recording that ended during the sound
    /// never leaves the system muted.
    fn put_back(&mut self) -> bool {
        if self.did_mute && self.lifted {
            self.lifted = false;
            return true;
        }
        false
    }

    /// Ends our mute. `true`: restore the system's sound now; not while it
    /// is lifted, since it is restored already.
    fn end(&mut self) -> bool {
        let restore = self.did_mute && !self.lifted;
        self.did_mute = false;
        self.lifted = false;
        restore
    }
}

/// The persisted microphone preference currently in effect. Clamshell and
/// regular selections are kept distinct so losing a clamshell-only device does
/// not erase the user's normal microphone preference.
enum DesiredMicrophone {
    Default,
    Selected(String),
    Clamshell(String),
}

/// Result of resolving the persisted preference to a recorder device ID.
/// `device: None` opens the system default. The unavailable name is populated
/// only when enumeration succeeded and confirmed that the user's regular
/// selected microphone is missing.
struct MicrophoneResolution {
    device: Option<String>,
    /// The ID came from `cached_device`, so it may be stale.
    from_cache: bool,
    unavailable_selected_microphone: Option<String>,
}

/// The VAD detector. A recording borrows it for its sink and `stop` hands it
/// back; a recording whose sink is lost loses it, and the next one reloads it.
enum VadSlot {
    Unloaded,
    Idle(CaptureVad),
    /// Lent to the active recording. Keeps the frame size, so a microphone
    /// opened meanwhile still gets VAD-sized chunks.
    Lent {
        frame_samples: usize,
    },
}

/// An open microphone: the stream is running whether or not it is recording.
struct OpenMicrophone {
    recorder: Recorder<CaptureSink>,
    /// Set when this recorder fails. Its own flag, so a failure handler that
    /// runs late can't mark a replacement recorder failed.
    failed: Arc<AtomicBool>,
}

impl OpenMicrophone {
    fn needs_reopen(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }
}

/// The notice for a recording that couldn't start.
pub fn recording_notice_code(error: &anyhow::Error) -> NoticeCode {
    match error
        .downcast_ref::<handy_recorder::Error>()
        .map(handy_recorder::Error::kind)
    {
        Some(ErrorKind::PermissionDenied) => NoticeCode::MicPermissionDenied,
        // A missing selected microphone falls back to the default, so this
        // means there is no input device at all.
        Some(ErrorKind::DeviceUnavailable) => NoticeCode::NoMicrophone,
        _ => NoticeCode::RecordingFailed,
    }
}

/* ──────────────────────────────────────────────────────────────── */

/// A fresh detector for `backend`, without Handy's smoothing: what a
/// recording's VAD wraps, and what scores audio for long-form chunking, so
/// every path uses the selected backend.
pub fn create_detector(
    backend: VadBackend,
    app_handle: &tauri::AppHandle,
) -> Result<Box<dyn SpeechScorer>, anyhow::Error> {
    Ok(match backend {
        VadBackend::Silero => {
            let vad_path = app_handle
                .path()
                .resolve(
                    "resources/models/silero-vad-v6.2-F32.gguf",
                    tauri::path::BaseDirectory::Resource,
                )
                .map_err(|e| anyhow::anyhow!("Failed to resolve VAD path: {e}"))?;
            Box::new(
                SileroVad::new(vad_path, SILERO_VAD_THRESHOLD)
                    .map_err(|e| anyhow::anyhow!("Failed to create SileroVad: {e}"))?,
            )
        }
        VadBackend::Earshot => Box::new(
            EarshotVad::new(EARSHOT_VAD_THRESHOLD)
                .map_err(|e| anyhow::anyhow!("Failed to create EarshotVad: {e}"))?,
        ),
    })
}

/// A detector for `backend` with a recording's smoothing (pre-roll, onset,
/// offline hangover): what keeps speech in a recording, and what cuts the
/// silence out of other audio the same way.
pub fn create_smoothed_detector(
    backend: VadBackend,
    app_handle: &tauri::AppHandle,
) -> Result<SmoothedVad, anyhow::Error> {
    Ok(smooth(create_detector(backend, app_handle)?))
}

/// DanyWhere: a recording's smoothing around `detector`.
fn smooth(detector: Box<dyn VoiceActivityDetector>) -> SmoothedVad {
    // Earshot uses 16 ms frames while Silero uses 32 ms. Convert the existing
    // time-based capture profile to each detector's frame size so selecting a
    // backend does not shorten pre-roll, onset, or post-speech audio.
    let frame_samples = detector.frame_samples();
    SmoothedVad::new(
        detector,
        frames_for_duration_ms(VAD_PREFILL_MS, frame_samples),
        frames_for_duration_ms(VAD_OFFLINE_HANGOVER_MS, frame_samples),
        frames_for_duration_ms(VAD_ONSET_MS, frame_samples),
    )
}

fn create_capture_vad(
    backend: VadBackend,
    app_handle: &tauri::AppHandle,
) -> Result<CaptureVad, anyhow::Error> {
    Ok(capture_vad(
        create_smoothed_detector(backend, app_handle)?,
        backend,
    ))
}

/// DanyWhere: the capture VAD around `smoothed_vad`, a detector of `backend`.
fn capture_vad(smoothed_vad: SmoothedVad, backend: VadBackend) -> CaptureVad {
    let frame_samples = smoothed_vad.frame_samples();
    let offline_hangover_frames = frames_for_duration_ms(VAD_OFFLINE_HANGOVER_MS, frame_samples);
    let streaming_hangover_frames =
        frames_for_duration_ms(VAD_STREAMING_HANGOVER_MS, frame_samples);

    info!(
        "Initialized {:?} VAD backend ({} samples/frame)",
        backend, frame_samples
    );

    CaptureVad::new(
        Box::new(smoothed_vad),
        offline_hangover_frames,
        streaming_hangover_frames,
    )
}

/// 16 kHz mono in VAD-frame chunks, from `device` (`None`: the system
/// default). `channel` picks one zero-based input channel; `None` averages
/// them all.
fn microphone_config(
    device: Option<String>,
    channel: Option<u16>,
    take_headset: bool,
    frames_per_chunk: usize,
) -> RecorderConfig {
    RecorderConfig {
        device,
        channels: channel.map_or(Channels::MixToMono, Channels::Only),
        frames_per_chunk: Some(frames_per_chunk),
        take_headset,
        ..RecorderConfig::speech()
    }
}

fn open_microphone(
    open: &OpenRecorder,
    config: RecorderConfig,
) -> Result<OpenMicrophone, handy_recorder::Error> {
    let failed = Arc::new(AtomicBool::new(false));
    let recorder = open(config, {
        let failed = Arc::clone(&failed);
        // handy-recorder logs the failure. The stream is rebuilt on the next
        // start; a recording in progress keeps the audio captured before it.
        Box::new(move |_| failed.store(true, Ordering::Release))
    })?;
    Ok(OpenMicrophone { recorder, failed })
}

/// Opens with the selected channel, or averages all channels if the device
/// doesn't have it (a different microphone than the one it was chosen on).
fn open_microphone_on_channel(
    open: &OpenRecorder,
    device: Option<String>,
    channel: Option<u16>,
    take_headset: bool,
    frames_per_chunk: usize,
) -> Result<OpenMicrophone, handy_recorder::Error> {
    match open_microphone(
        open,
        microphone_config(device.clone(), channel, take_headset, frames_per_chunk),
    ) {
        Err(error) if error.kind() == ErrorKind::InvalidChannel => {
            warn!("{error}; averaging all input channels instead");
            open_microphone(
                open,
                microphone_config(device, None, take_headset, frames_per_chunk),
            )
        }
        result => result,
    }
}

/// DanyWhere: what a recorder the manager opens calls when it fails.
pub type RecorderFailureHandler = Box<dyn FnOnce(handy_recorder::Error) + Send + 'static>;

/// DanyWhere: opens the microphone: `Recorder::open_with_failure_handler`
/// in the app, a fake microphone in tests (`handy_recorder::testing`).
pub type OpenRecorder = Arc<
    dyn Fn(
            RecorderConfig,
            RecorderFailureHandler,
        ) -> Result<Recorder<CaptureSink>, handy_recorder::Error>
        + Send
        + Sync,
>;

/// DanyWhere: what the manager takes from the app, as functions, so a test
/// can build one without it ([`AudioRecordingManager::with_host`]);
/// [`AudioRecordingManager::new`] wires them to the app.
#[derive(Clone)]
pub struct AudioHost {
    /// The stored settings (`get_settings`), read each time they are needed.
    pub settings: Arc<dyn Fn() -> AppSettings + Send + Sync>,
    /// Clears the selected microphone of this name from the settings after
    /// the default stood in for it, and tells the frontend.
    pub forget_microphone: Arc<dyn Fn(&str) + Send + Sync>,
    /// A recording's VAD for the backend.
    pub create_vad: Arc<dyn Fn(VadBackend) -> Result<CaptureVad, anyhow::Error> + Send + Sync>,
    pub open_recorder: OpenRecorder,
    /// The microphones there are now (`audio_toolkit::audio::list_microphones`),
    /// to find the selected or the clamshell microphone by its name.
    pub list_microphones:
        Arc<dyn Fn() -> Result<Vec<MicrophoneDevice>, anyhow::Error> + Send + Sync>,
    /// Whether this Mac's lid is closed (`clamshell::is_clamshell`); asked
    /// only while a clamshell microphone is set.
    pub is_clamshell: Arc<dyn Fn() -> bool + Send + Sync>,
    /// Spectrum levels for the overlay.
    pub on_levels: LevelCallback,
}

/// Keep persisted settings and the UI aligned with a successful runtime
/// fallback. Re-read first so recovery cannot clear a microphone the user
/// selected concurrently while the stream was being rebuilt.
fn forget_microphone(app: &tauri::AppHandle, unavailable_name: &str) {
    let mut settings = get_settings(app);
    if settings.selected_microphone.as_deref() != Some(unavailable_name) {
        return;
    }

    settings.selected_microphone = None;
    write_settings(app, settings);
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "selected_microphone",
            "value": "Default"
        }),
    );
}

/* ──────────────────────────────────────────────────────────────── */

/// DanyWhere: the first section of a recording cut in two
/// ([`AudioRecordingManager::split_recording`]).
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedSection {
    pub samples: Vec<f32>,
    /// The microphone sent only digital silence during the section.
    pub input_silent: bool,
    /// The detector heard a voice in it, not only the hangover of speech
    /// before (`audio_toolkit::SplitSection::heard_speech`).
    pub speech: bool,
}

/// One recording session's first-sample notification. Waiting on this never
/// blocks the shortcut coordinator: callers hand it to a dedicated worker.
pub struct RecordingReadiness {
    receiver: mpsc::Receiver<()>,
    generation: u64,
}

impl RecordingReadiness {
    pub fn wait(self) -> bool {
        self.receiver.recv().is_ok()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

#[derive(Clone)]
pub struct AudioRecordingManager {
    /// Never assign through this directly — route every write through
    /// `set_state()`, which keeps `recording_active` in sync.
    state: Arc<Mutex<RecordingState>>,
    mode: Arc<Mutex<MicrophoneMode>>,
    /// DanyWhere: the app's settings, microphone and VAD, or a test's.
    host: AudioHost,

    recorder: Arc<Mutex<Option<OpenMicrophone>>>,
    vad: Arc<Mutex<VadSlot>>,
    /// Zero-based input channel to record, or `None` to average them all.
    /// Held here rather than read from settings, which are persisted only
    /// after a channel change reopens successfully.
    selected_channel: Arc<Mutex<Option<u16>>>,
    /// Hold a Bluetooth headset on this Mac while recording. Held here for
    /// the same reason as `selected_channel`.
    airpods_mode: Arc<AtomicBool>,
    /// VAD-passed frames for live streaming, via a shared `StreamRouter`
    /// (captured directly, not via Tauri state — see its docs).
    on_audio: AudioFrameCallback,
    is_open: Arc<Mutex<bool>>,
    is_recording: Arc<Mutex<bool>>,
    mute_state: Arc<Mutex<MuteState>>,
    close_generation: Arc<AtomicU64>,
    cancel_generation: Arc<AtomicU64>,
    /// Lock-free mirror of "is the state in {Recording, Stopping}",
    /// maintained by `set_state()`. The hot-path `is_recording()` reads THIS
    /// instead of the std `state` mutex, so a UI poll can no longer deadlock
    /// the main/webview thread when a worker holds `state` across a slow
    /// CoreAudio open/close.
    recording_active: Arc<AtomicBool>,
    /// Invalidates asynchronous first-sample UI/chime work when a recording is
    /// stopped or cancelled. This prevents a slow device from producing a late
    /// "ready" indication for a session the user already ended.
    capture_generation: Arc<AtomicU64>,
    /// Resolution of a *named* microphone (selected or clamshell) to its
    /// recorder device ID, cached so on-demand recording starts skip an extra
    /// device enumeration. Keyed by the resolved name, so a settings change
    /// misses naturally; cleared when an open fails (device unplugged) so the
    /// retry re-enumerates. The system-default case is never cached — the
    /// recorder resolves the current default itself.
    cached_device: Arc<Mutex<Option<(String, String)>>>,
    /// The last recording was all-zero samples, not just audio without speech.
    last_input_silent: Arc<AtomicBool>,
    /// DanyWhere: the detector heard a voice in the last recording (its last
    /// section, after a cut): `RecordedSection::speech`.
    last_heard_speech: Arc<AtomicBool>,
    /// DanyWhere: what the open stream was opened with, so a recording whose
    /// profile has another microphone opens it again (`dictation_microphone`).
    opened_for: Arc<Mutex<Option<MicrophoneChoice>>>,
    /// DanyWhere: the running recording's sections, to cut it in two
    /// (`split_recording`); `None` while none runs.
    sections: Arc<Mutex<Option<RecordingSections>>>,
}

impl AudioRecordingManager {
    /* ---------- construction ------------------------------------------------ */

    /// The app's microphone: its settings, the selected device, the VAD
    /// model in its resources, levels to the overlay and frames to the
    /// live stream (`stream_router`).
    pub fn new(
        app: &tauri::AppHandle,
        stream_router: Arc<StreamRouter>,
    ) -> Result<Self, anyhow::Error> {
        let settings_app = app.clone();
        let forget_app = app.clone();
        let vad_app = app.clone();
        let levels_app = app.clone();
        let host = AudioHost {
            settings: Arc::new(move || get_settings(&settings_app)),
            forget_microphone: Arc::new(move |name: &str| forget_microphone(&forget_app, name)),
            create_vad: Arc::new(move |backend| create_capture_vad(backend, &vad_app)),
            open_recorder: Arc::new(|config, handler| {
                Recorder::open_with_failure_handler(config, handler)
            }),
            list_microphones: Arc::new(crate::audio_toolkit::audio::list_microphones),
            is_clamshell: Arc::new(|| clamshell::is_clamshell().unwrap_or(false)),
            on_levels: Arc::new(move |levels| utils::emit_levels(&levels_app, &levels)),
        };
        Self::with_host(host, Arc::new(move |frame| stream_router.feed(frame)))
    }

    /// DanyWhere: a manager without the app: `host` gives it settings,
    /// microphone and VAD, `on_audio` takes the frames for a live stream.
    pub fn with_host(host: AudioHost, on_audio: AudioFrameCallback) -> Result<Self, anyhow::Error> {
        let settings = (host.settings)();
        let mode = if settings.always_on_microphone {
            MicrophoneMode::AlwaysOn
        } else {
            MicrophoneMode::OnDemand
        };

        let manager = Self {
            state: Arc::new(Mutex::new(RecordingState::Idle)),
            mode: Arc::new(Mutex::new(mode.clone())),
            host,

            recorder: Arc::new(Mutex::new(None)),
            vad: Arc::new(Mutex::new(VadSlot::Unloaded)),
            selected_channel: Arc::new(Mutex::new(settings.selected_channel)),
            airpods_mode: Arc::new(AtomicBool::new(settings.airpods_mode)),
            on_audio,
            is_open: Arc::new(Mutex::new(false)),
            is_recording: Arc::new(Mutex::new(false)),
            mute_state: Arc::new(Mutex::new(MuteState::default())),
            close_generation: Arc::new(AtomicU64::new(0)),
            cancel_generation: Arc::new(AtomicU64::new(0)),
            recording_active: Arc::new(AtomicBool::new(false)),
            capture_generation: Arc::new(AtomicU64::new(0)),
            cached_device: Arc::new(Mutex::new(None)),
            last_input_silent: Arc::new(AtomicBool::new(false)),
            last_heard_speech: Arc::new(AtomicBool::new(false)),
            opened_for: Arc::new(Mutex::new(None)),
            sections: Arc::new(Mutex::new(None)),
        };

        // Always-on?  Open immediately.
        if matches!(mode, MicrophoneMode::AlwaysOn) {
            manager.start_microphone_stream()?;
        }

        Ok(manager)
    }

    /* ---------- helper methods --------------------------------------------- */

    /// The persisted microphone preference currently in effect. Only runs the
    /// clamshell probe (the host's `is_clamshell`, in the app an `ioreg`
    /// subprocess, ~10-20ms) when a clamshell microphone is actually
    /// configured.
    fn desired_microphone(&self, settings: &AppSettings) -> DesiredMicrophone {
        if let Some(clamshell_microphone) = &settings.clamshell_microphone {
            let clamshell_started = Instant::now();
            let is_clamshell = (self.host.is_clamshell)();
            debug!(
                "device resolve: clamshell_check={:?} (clamshell={})",
                clamshell_started.elapsed(),
                is_clamshell
            );
            if is_clamshell {
                return DesiredMicrophone::Clamshell(clamshell_microphone.clone());
            }
        }
        match &settings.selected_microphone {
            Some(name) => DesiredMicrophone::Selected(name.clone()),
            None => DesiredMicrophone::Default,
        }
    }

    pub fn invalidate_device_cache(&self) {
        *self.cached_device.lock().unwrap() = None;
    }

    /// DanyWhere: the settings a stream resolves its device with and the
    /// microphone it opens: the dictation's profile's while one applies,
    /// else the settings in use with the channel and headset option held
    /// here (`dictation_microphone::choose`).
    fn wanted_microphone(&self) -> (AppSettings, MicrophoneChoice) {
        crate::dictation_microphone::choose(
            &(self.host.settings)(),
            *self.selected_channel.lock().unwrap(),
            self.airpods_mode.load(Ordering::Acquire),
            crate::app_profiles::dictation_values().as_ref(),
        )
    }

    /// DanyWhere: takes the channel and headset option of the profile now
    /// in use (`profile_effects`) and opens a warm stream again when it was
    /// opened with another microphone. Refused while recording.
    pub fn use_profile_microphone(
        &self,
        channel: Option<u16>,
        take_headset: bool,
    ) -> Result<(), anyhow::Error> {
        let state = self.state.lock().unwrap();
        if !matches!(*state, RecordingState::Idle) {
            return Err(anyhow::anyhow!(
                "Cannot change the microphone while recording"
            ));
        }
        *self.selected_channel.lock().unwrap() = channel;
        self.airpods_mode.store(take_headset, Ordering::Release);
        self.invalidate_device_cache();
        let was_open = *self.is_open.lock().unwrap();
        if was_open {
            self.close_generation.fetch_add(1, Ordering::SeqCst);
            // Reopens only when the stream has another microphone.
            self.start_microphone_stream()?;
        }
        drop(state);
        Ok(())
    }

    fn resolve_microphone_device(&self, settings: &AppSettings) -> MicrophoneResolution {
        let desired = self.desired_microphone(settings);
        let (device_name, selected_microphone) = match desired {
            DesiredMicrophone::Default => {
                debug!("device resolve: no mic configured -> system default");
                return MicrophoneResolution {
                    device: None,
                    from_cache: false,
                    unavailable_selected_microphone: None,
                };
            }
            DesiredMicrophone::Selected(name) => (name.clone(), Some(name)),
            DesiredMicrophone::Clamshell(name) => (name, None),
        };

        // Cache hit: skip the enumeration. A stale ID (device unplugged)
        // fails at open, where the caller invalidates and retries fresh.
        if let Some((cached_name, id)) = self.cached_device.lock().unwrap().as_ref() {
            if *cached_name == device_name {
                debug!("device resolve: cache hit for '{}'", device_name);
                return MicrophoneResolution {
                    device: Some(id.clone()),
                    from_cache: true,
                    unavailable_selected_microphone: None,
                };
            }
        }

        // Only report a selected microphone as unavailable when enumeration
        // itself succeeded. A backend enumeration error may be transient and
        // must not erase the user's persisted preference.
        let enumerate_started = Instant::now();
        let (device, enumeration_succeeded) = match (self.host.list_microphones)() {
            Ok(devices) => (
                devices
                    .into_iter()
                    .find(|d| d.name == device_name)
                    .map(|d| d.id),
                true,
            ),
            Err(e) => {
                debug!("Failed to list devices, using default: {}", e);
                (None, false)
            }
        };
        debug!(
            "device resolve: enumerate={:?} (found={})",
            enumerate_started.elapsed(),
            device.is_some()
        );
        if let Some(id) = &device {
            *self.cached_device.lock().unwrap() = Some((device_name, id.clone()));
        }

        let unavailable_selected_microphone = if enumeration_succeeded && device.is_none() {
            selected_microphone
        } else {
            None
        };
        MicrophoneResolution {
            device,
            from_cache: false,
            unavailable_selected_microphone,
        }
    }

    /// Keep persisted settings and the UI aligned with a successful runtime
    /// fallback (the host's `forget_microphone`).
    fn persist_default_microphone_after_fallback(&self, unavailable_name: &str) {
        (self.host.forget_microphone)(unavailable_name);
    }

    fn schedule_lazy_close(&self) {
        let gen = self.close_generation.fetch_add(1, Ordering::SeqCst) + 1;
        // DanyWhere: a clone shares every piece of state with this manager.
        let rm = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(STREAM_IDLE_TIMEOUT);
            // Hold state lock across the check AND close to serialize against
            // try_start_recording, preventing a race where the stream is closed
            // under an active recording.
            let state = rm.state.lock().unwrap();
            if rm.close_generation.load(Ordering::SeqCst) == gen
                && matches!(*state, RecordingState::Idle)
            {
                // stop_microphone_stream does not acquire the state lock,
                // so holding it here is safe (no deadlock).
                info!(
                    "Closing idle microphone stream after {:?}",
                    STREAM_IDLE_TIMEOUT
                );
                rm.stop_microphone_stream();
            }
        });
    }

    /* ---------- VAD --------------------------------------------------------- */

    pub fn preload_vad(&self) -> Result<(), anyhow::Error> {
        let mut vad = self.vad.lock().unwrap();
        if matches!(*vad, VadSlot::Unloaded) {
            let settings = (self.host.settings)();
            *vad = VadSlot::Idle((self.host.create_vad)(settings.vad_backend)?);
        }
        Ok(())
    }

    /// Samples per VAD frame: the chunk size the microphone is opened with.
    fn vad_frame_samples(&self) -> Result<usize, anyhow::Error> {
        self.preload_vad()?;
        match &*self.vad.lock().unwrap() {
            VadSlot::Idle(vad) => Ok(vad.frame_samples()),
            VadSlot::Lent { frame_samples } => Ok(*frame_samples),
            VadSlot::Unloaded => unreachable!("preload_vad just loaded it"),
        }
    }

    /// Lends the detector to a recording.
    /// Lends the detector to a new recording. Called only while idle.
    fn lend_vad(&self) -> Result<CaptureVad, anyhow::Error> {
        {
            let mut slot = self.vad.lock().unwrap();
            // No recording is active, so a lent detector went with a lost sink.
            if matches!(*slot, VadSlot::Lent { .. }) {
                warn!("The last recording did not return its VAD detector; reloading it");
                *slot = VadSlot::Unloaded;
            }
        }
        self.preload_vad()?;
        let mut slot = self.vad.lock().unwrap();
        match std::mem::replace(&mut *slot, VadSlot::Unloaded) {
            VadSlot::Idle(vad) => {
                *slot = VadSlot::Lent {
                    frame_samples: vad.frame_samples(),
                };
                Ok(vad)
            }
            other => {
                *slot = other;
                Err(anyhow::anyhow!("VAD is not available"))
            }
        }
    }

    fn return_vad(&self, vad: CaptureVad) {
        *self.vad.lock().unwrap() = VadSlot::Idle(vad);
    }

    /* ---------- microphone life-cycle -------------------------------------- */

    /// Applies mute if mute_while_recording is enabled and stream is open.
    /// Snapshots the system's prior mute state first so `remove_mute` can
    /// restore it instead of unconditionally unmuting.
    pub fn apply_mute(&self) {
        // DanyWhere: the dictation's profile's switch, if it has its own
        // (as `app_profiles::dictation_settings`).
        let settings = crate::app_profiles::with_values(
            &(self.host.settings)(),
            crate::app_profiles::dictation_values().as_ref(),
        );
        if !settings.mute_while_recording {
            return;
        }

        // Lock order: is_open before mute_state (matches stop_microphone_stream).
        let is_open = self.is_open.lock().unwrap();
        let mut mute_guard = self.mute_state.lock().unwrap();
        // Already muted this session — don't re-snapshot, or a duplicate/late
        // apply would overwrite prev_muted with our own forced-muted state and
        // strand audio muted on stop.
        if mute_guard.did_mute {
            return;
        }
        if *is_open {
            mute_guard.prev_muted = get_mute();
            set_mute(true);
            mute_guard.did_mute = true;
            mute_guard.lifted = false;
            debug!("Mute applied (prev_muted={:?})", mute_guard.prev_muted);
        }
    }

    /// Removes mute if it was applied, restoring the system's prior mute state
    /// (a system already muted before recording stays muted).
    pub fn remove_mute(&self) {
        let mut mute_guard = self.mute_state.lock().unwrap();
        if mute_guard.end() {
            restore_mute(mute_guard.prev_muted);
            debug!(
                "Mute removed (restored prev_muted={:?})",
                mute_guard.prev_muted
            );
        }
    }

    /// DanyWhere: lifts the mute while recording for a feedback sound in
    /// the middle of the recording, as the start sound plays before the
    /// mute; [`put_mute_back`](Self::put_mute_back) after it.
    pub fn lift_mute(&self) {
        let mut mute_guard = self.mute_state.lock().unwrap();
        if mute_guard.lift() {
            restore_mute(mute_guard.prev_muted);
            debug!("Mute lifted for a feedback sound");
        }
    }

    /// DanyWhere: mutes again after [`lift_mute`](Self::lift_mute), unless
    /// the recording's mute ended meanwhile.
    pub fn put_mute_back(&self) {
        let mut mute_guard = self.mute_state.lock().unwrap();
        if mute_guard.put_back() {
            set_mute(true);
            debug!("Mute put back after a feedback sound");
        }
    }

    pub fn start_microphone_stream(&self) -> Result<(), anyhow::Error> {
        let mut open_flag = self.is_open.lock().unwrap();
        // DanyWhere: the microphone of the dictation's profile, if it has
        // its own (`dictation_microphone`).
        let (settings, wanted) = self.wanted_microphone();
        if *open_flag {
            // `is_open` only records that we opened a stream, not that it is
            // still running. If the recorder has since failed (mic unplugged,
            // stalled, permission revoked), rebuild it before the next
            // recording instead of handing the caller a failed recorder.
            let needs_reopen = self
                .recorder
                .lock()
                .unwrap()
                .as_ref()
                .is_none_or(OpenMicrophone::needs_reopen);
            // DanyWhere: or it was opened with another profile's microphone.
            let other_microphone = self.opened_for.lock().unwrap().as_ref() != Some(&wanted);

            if !needs_reopen && !other_microphone {
                // trace, not debug: with the aliveness check in
                // try_start_recording this now fires on every keypress in
                // always-on mode.
                trace!("Microphone stream already active");
                return Ok(());
            }

            if needs_reopen {
                warn!("Microphone stream failed; reopening");
            } else {
                info!("Opening the microphone of the dictation's profile");
            }

            // Torn down inline rather than via stop_microphone_stream(), which
            // takes the `is_open` lock we are already holding.
            {
                let mut mute_guard = self.mute_state.lock().unwrap();
                if mute_guard.end() {
                    restore_mute(mute_guard.prev_muted);
                }
            }
            let failed = self.recorder.lock().unwrap().take();
            if let Some(microphone) = failed {
                self.close_microphone(microphone);
            }
            *open_flag = false;
            self.invalidate_device_cache();
            // Fall through to the same fresh resolution and fallback path used
            // when an on-demand stream opens after its device was unplugged.
        }

        let start_time = Instant::now();

        // Don't mute immediately - caller will handle muting after audio feedback.
        // The previous stream restored audio on close, so did_mute should already
        // be false here; if it somehow isn't, restore rather than just clearing the
        // flag, which would strand system audio muted.
        {
            let mut mute_guard = self.mute_state.lock().unwrap();
            if mute_guard.end() {
                restore_mute(mute_guard.prev_muted);
            }
        }

        // Get the selected device from settings, considering clamshell mode.
        // No pre-flight enumeration here: when nothing is configured the
        // recorder resolves the system default itself, and a machine with no
        // input devices at all fails inside open() with `DeviceUnavailable`.
        let resolve_started = Instant::now();
        let mut resolution = self.resolve_microphone_device(&settings);
        let resolve_elapsed = resolve_started.elapsed();

        // Ensure VAD is loaded if it wasn't for whatever reason; its frame
        // size is the chunk size.
        let vad_started = Instant::now();
        let frames_per_chunk = self.vad_frame_samples()?;
        let vad_elapsed = vad_started.elapsed();

        let (channel, take_headset) = (wanted.channel, wanted.take_headset);
        let open_started = Instant::now();
        let microphone = match open_microphone_on_channel(
            &self.host.open_recorder,
            resolution.device.clone(),
            channel,
            take_headset,
            frames_per_chunk,
        ) {
            Err(first_err) if resolution.from_cache => {
                // The cached device ID may have gone stale (unplugged).
                // Re-resolve from a fresh enumeration and retry once before
                // surfacing the error.
                warn!("Recorder open failed ({first_err}); re-resolving device and retrying once");
                self.invalidate_device_cache();
                resolution = self.resolve_microphone_device(&settings);
                open_microphone_on_channel(
                    &self.host.open_recorder,
                    resolution.device.clone(),
                    channel,
                    take_headset,
                    frames_per_chunk,
                )
            }
            result => result,
        }?;
        debug!(
            "mic stream breakdown: device_resolve={:?} vad_ensure={:?} open={:?}",
            resolve_elapsed,
            vad_elapsed,
            open_started.elapsed()
        );
        *self.recorder.lock().unwrap() = Some(microphone);
        *self.opened_for.lock().unwrap() = Some(wanted);

        *open_flag = true;
        if let Some(unavailable_name) = resolution.unavailable_selected_microphone {
            // Do this only after the default stream opened successfully. A
            // failed fallback must not erase the user's microphone preference.
            self.persist_default_microphone_after_fallback(&unavailable_name);
        }
        // The stream is running, but the device may not be delivering audio
        // yet (Bluetooth can take seconds); a recording's readiness waits for
        // its first chunk.
        info!(
            "Microphone stream initialized in {:?}",
            start_time.elapsed()
        );
        Ok(())
    }

    pub fn stop_microphone_stream(&self) {
        let mut open_flag = self.is_open.lock().unwrap();
        if !*open_flag {
            return;
        }

        {
            let mut mute_guard = self.mute_state.lock().unwrap();
            if mute_guard.end() {
                restore_mute(mute_guard.prev_muted);
            }
        }

        let microphone = self.recorder.lock().unwrap().take();
        if let Some(microphone) = microphone {
            self.close_microphone(microphone);
        }

        *open_flag = false;
        debug!("Microphone stream stopped");
    }

    /// Closes a microphone, first discarding any recording on it (its
    /// detector comes back).
    fn close_microphone(&self, microphone: OpenMicrophone) {
        let mut is_recording = self.is_recording.lock().unwrap();
        if *is_recording {
            let _ = self.collect_recording(&microphone);
            *is_recording = false;
        }
        if let Err(e) = microphone.recorder.close() {
            warn!("Closing the microphone failed: {e}");
        }
    }

    /// Stops the recording on `microphone`, returning its audio and taking
    /// back the detector it borrowed.
    fn collect_recording(&self, microphone: &OpenMicrophone) -> Vec<f32> {
        match microphone.recorder.stop() {
            Ok(stopped) => {
                match &stopped.end_reason {
                    EndReason::StopCalled => {}
                    EndReason::RecorderFailed(error) => {
                        microphone.failed.store(true, Ordering::Release);
                        warn!("Recording cut short by a microphone failure; keeping the audio before it: {error}");
                    }
                    EndReason::SinkPanicked(message) => {
                        error!("Recording cut short; audio processing panicked: {message}");
                    }
                }
                if stopped.dropped_frames > 0 {
                    warn!(
                        "Recording completed after dropping {} microphone frames",
                        stopped.dropped_frames
                    );
                }
                let captured = stopped.sink.finish();
                self.return_vad(captured.vad);
                self.last_input_silent
                    .store(captured.input_silent, Ordering::Release);
                self.last_heard_speech
                    .store(captured.heard_speech, Ordering::Release);
                captured.samples
            }
            Err(e) => {
                // The recording and its detector are lost with the sink.
                error!("Failed to stop recording: {e}");
                self.last_input_silent.store(false, Ordering::Release);
                self.last_heard_speech.store(false, Ordering::Release);
                if e.kind() != ErrorKind::NotRecording {
                    microphone.failed.store(true, Ordering::Release);
                }
                *self.vad.lock().unwrap() = VadSlot::Unloaded;
                Vec::new()
            }
        }
    }

    /* ---------- mode switching --------------------------------------------- */

    pub fn update_mode(&self, new_mode: MicrophoneMode) -> Result<(), anyhow::Error> {
        let cur_mode = self.mode.lock().unwrap().clone();

        match (cur_mode, &new_mode) {
            (MicrophoneMode::AlwaysOn, MicrophoneMode::OnDemand) => {
                if matches!(*self.state.lock().unwrap(), RecordingState::Idle) {
                    self.close_generation.fetch_add(1, Ordering::SeqCst);
                    self.stop_microphone_stream();
                }
            }
            (MicrophoneMode::OnDemand, MicrophoneMode::AlwaysOn) => {
                self.close_generation.fetch_add(1, Ordering::SeqCst);
                self.start_microphone_stream()?;
            }
            _ => {}
        }

        *self.mode.lock().unwrap() = new_mode;
        Ok(())
    }

    /* ---------- recording --------------------------------------------------- */

    /// The one place `state` is written. Derives `recording_active` (the
    /// lock-free mirror read by `is_recording()`) from the new value itself,
    /// so the two can never drift: a new `RecordingState` variant only needs
    /// its active-set membership decided here, once.
    fn set_state(&self, guard: &mut RecordingState, new_state: RecordingState) {
        *guard = new_state;
        self.recording_active.store(
            matches!(
                *guard,
                RecordingState::Recording { .. } | RecordingState::Stopping
            ),
            Ordering::SeqCst,
        );
    }

    pub fn try_start_recording(
        &self,
        binding_id: &str,
        vad_policy: VadPolicy,
    ) -> Result<RecordingReadiness, anyhow::Error> {
        let mut state = self.state.lock().unwrap();
        if !matches!(*state, RecordingState::Idle) {
            return Err(anyhow::anyhow!("Already recording"));
        }

        // Cancel any pending lazy close (no-op in always-on mode, where
        // closes are never scheduled).
        self.close_generation.fetch_add(1, Ordering::SeqCst);

        // A recorder can fail just before this start, ahead of its failure
        // handler; the start then reports it. Reopen and try once more.
        let mut reopened = false;
        loop {
            // Opens the stream in on-demand mode. In always-on mode the stream
            // is normally already open and this is a cheap aliveness check —
            // but if the recorder failed (device disconnect), it rebuilds the
            // stream instead of leaving every subsequent start wedged.
            if let Err(e) = self.start_microphone_stream() {
                error!("Failed to open microphone stream: {e}");
                return Err(e);
            }

            let (ready_tx, receiver) = mpsc::channel();
            let sink = CaptureSink::new(
                self.lend_vad()?,
                vad_policy,
                Arc::clone(&self.host.on_levels),
                Arc::clone(&self.on_audio),
                ready_tx,
            );

            let recorder = self.recorder.lock().unwrap();
            let Some(microphone) = recorder.as_ref() else {
                self.return_vad(sink.into_vad());
                return Err(anyhow::anyhow!("Recorder not available"));
            };
            // DanyWhere: what cuts this recording for a spoken instruction.
            let sections = sink.sections();
            match microphone.recorder.start(sink) {
                Ok(()) => {
                    let generation = self.capture_generation.fetch_add(1, Ordering::AcqRel) + 1;
                    *self.sections.lock().unwrap() = Some(sections);
                    *self.is_recording.lock().unwrap() = true;
                    self.set_state(
                        &mut state,
                        RecordingState::Recording {
                            binding_id: binding_id.to_string(),
                        },
                    );
                    debug!("Recording requested for binding {binding_id}");
                    return Ok(RecordingReadiness {
                        receiver,
                        generation,
                    });
                }
                Err(StartError { error, sink }) => {
                    self.return_vad(sink.into_vad());
                    if reopened || error.kind() == ErrorKind::AlreadyRecording {
                        return Err(anyhow::Error::new(error).context("Failed to start recorder"));
                    }
                    warn!("Microphone failed before this recording ({error}); reopening");
                    microphone.failed.store(true, Ordering::Release);
                    reopened = true;
                }
            }
        }
    }

    /// Replace the VAD implementation while idle. If the microphone stream is
    /// currently warm (always-on or lazy-close mode), reopen it with the new
    /// detector's chunk size before reporting success. A failed reopen restores
    /// the previous detector so the persisted setting can remain unchanged.
    pub fn update_vad_backend(&self, backend: VadBackend) -> Result<(), anyhow::Error> {
        let state = self.state.lock().unwrap();
        if !matches!(*state, RecordingState::Idle) {
            return Err(anyhow::anyhow!(
                "Cannot change the VAD backend while recording"
            ));
        }

        let replacement = (self.host.create_vad)(backend)?;
        let was_open = *self.is_open.lock().unwrap();

        // Invalidate any delayed close before swapping the detector.
        self.close_generation.fetch_add(1, Ordering::SeqCst);
        if was_open {
            self.stop_microphone_stream();
        }

        let previous =
            std::mem::replace(&mut *self.vad.lock().unwrap(), VadSlot::Idle(replacement));
        if was_open {
            if let Err(change_error) = self.start_microphone_stream() {
                *self.vad.lock().unwrap() = previous;

                if let Err(rollback_error) = self.start_microphone_stream() {
                    error!(
                        "Failed to restore microphone stream after VAD backend change failed: {rollback_error}"
                    );
                }
                return Err(anyhow::anyhow!(
                    "Failed to reopen microphone with {:?} VAD: {change_error}",
                    backend
                ));
            }
        }

        info!("VAD backend changed to {:?}", backend);
        drop(state);
        Ok(())
    }

    pub fn update_selected_device(&self) -> Result<(), anyhow::Error> {
        // Device settings changed; re-enumerate the device and restart capture.
        self.invalidate_device_cache();
        let was_open = *self.is_open.lock().unwrap();
        if was_open {
            self.close_generation.fetch_add(1, Ordering::SeqCst);
            self.stop_microphone_stream();
            self.start_microphone_stream()?;
        }
        Ok(())
    }

    pub fn update_selected_channel(
        &self,
        selected_channel: Option<u16>,
    ) -> Result<(), anyhow::Error> {
        // Serialize against recording start/stop. Restarting an active capture
        // would discard its samples and leave the manager's recording state out
        // of sync with the new recorder.
        let state = self.state.lock().unwrap();
        if !matches!(*state, RecordingState::Idle) {
            return Err(anyhow::anyhow!(
                "Cannot change the input channel while recording"
            ));
        }

        let was_open = *self.is_open.lock().unwrap();
        if was_open {
            self.close_generation.fetch_add(1, Ordering::SeqCst);
            self.stop_microphone_stream();
        }
        let previous_channel = std::mem::replace(
            &mut *self.selected_channel.lock().unwrap(),
            selected_channel,
        );
        if was_open {
            if let Err(error) = self.start_microphone_stream() {
                *self.selected_channel.lock().unwrap() = previous_channel;
                return Err(error);
            }
        }
        drop(state);
        Ok(())
    }

    /// Takes effect on the next open, so a warm stream is reopened.
    pub fn update_airpods_mode(&self, enabled: bool) -> Result<(), anyhow::Error> {
        let state = self.state.lock().unwrap();
        if !matches!(*state, RecordingState::Idle) {
            return Err(anyhow::anyhow!(
                "Cannot change AirPods mode while recording"
            ));
        }

        let was_open = *self.is_open.lock().unwrap();
        if was_open {
            self.close_generation.fetch_add(1, Ordering::SeqCst);
            self.stop_microphone_stream();
        }
        let previous = self.airpods_mode.swap(enabled, Ordering::AcqRel);
        if was_open {
            if let Err(error) = self.start_microphone_stream() {
                self.airpods_mode.store(previous, Ordering::Release);
                return Err(error);
            }
        }
        drop(state);
        Ok(())
    }

    /// Invalidate pending first-sample UI and audio-feedback work immediately.
    /// Called at the beginning of stop, before the slower capture drain starts.
    pub fn invalidate_recording_readiness(&self) {
        self.capture_generation.fetch_add(1, Ordering::AcqRel);
    }

    pub fn is_recording_readiness_current(&self, generation: u64) -> bool {
        self.capture_generation.load(Ordering::Acquire) == generation
    }

    pub fn cancel_generation(&self) -> u64 {
        self.cancel_generation.load(Ordering::Acquire)
    }

    /// Whether the last stopped recording's device sent only digital silence.
    pub fn last_input_silent(&self) -> bool {
        self.last_input_silent.load(Ordering::Acquire)
    }

    /// DanyWhere: whether the detector heard a voice in the last stopped
    /// recording, its last section after a cut (`RecordedSection::speech`).
    pub fn last_heard_speech(&self) -> bool {
        self.last_heard_speech.load(Ordering::Acquire)
    }

    pub fn was_cancelled_since(&self, generation: u64) -> bool {
        self.cancel_generation.load(Ordering::Acquire) != generation
    }

    pub fn stop_recording(&self, binding_id: &str, cancel_generation: u64) -> Option<Vec<f32>> {
        self.invalidate_recording_readiness();
        let mut state = self.state.lock().unwrap();

        match *state {
            RecordingState::Recording {
                binding_id: ref active,
            } if active == binding_id => {
                self.set_state(&mut state, RecordingState::Stopping);
                self.sections.lock().unwrap().take();
                drop(state);

                // Optionally keep recording for a bit longer to capture trailing audio.
                // This is only the explicit user setting; streaming VAD must not add
                // hidden post-release capture time.
                let settings = (self.host.settings)();
                let buffer_ms = settings.extra_recording_buffer_ms;
                if buffer_ms > 0 {
                    debug!(
                        "Extra recording buffer: sleeping {}ms before stopping",
                        buffer_ms
                    );
                    let started = Instant::now();
                    let buffer = Duration::from_millis(buffer_ms);
                    while started.elapsed() < buffer {
                        if self.was_cancelled_since(cancel_generation) {
                            debug!("Recording stop cancelled during extra buffer");
                            break;
                        }
                        let remaining = buffer.saturating_sub(started.elapsed());
                        std::thread::sleep(remaining.min(Duration::from_millis(25)));
                    }
                }

                let samples = match self.recorder.lock().unwrap().as_ref() {
                    Some(microphone) => self.collect_recording(microphone),
                    None => {
                        error!("Recorder not available");
                        self.last_input_silent.store(false, Ordering::Release);
                        self.last_heard_speech.store(false, Ordering::Release);
                        Vec::new()
                    }
                };

                *self.is_recording.lock().unwrap() = false;
                self.set_state(&mut self.state.lock().unwrap(), RecordingState::Idle);

                // In on-demand mode, close the mic (lazily if the setting is enabled)
                if matches!(*self.mode.lock().unwrap(), MicrophoneMode::OnDemand) {
                    if (self.host.settings)().lazy_stream_close {
                        self.schedule_lazy_close();
                    } else {
                        self.stop_microphone_stream();
                    }
                }

                if self.was_cancelled_since(cancel_generation) {
                    debug!("Recording stop cancelled; discarding captured samples");
                    return None;
                }

                Some(padded_if_short(samples))
            }
            _ => None,
        }
    }
    /// DanyWhere: cuts the recording of `binding_id` in two while the
    /// microphone runs, for the spoken instruction: hands over the first
    /// section (padded like a short recording at `stop_recording`) and goes
    /// on recording into a second one, which `stop_recording` returns. No
    /// stop sound, no change to the output's mute, no gap: the cut is made
    /// in the sink between two frames, so every sample lands in exactly one
    /// section. From the cut on, frames no longer go to the live stream.
    /// `None`: no recording of `binding_id` runs, or it was cut already.
    ///
    /// A cut in the sink rather than a stop and a new start of the
    /// recorder: between `stop` and `start` handy-recorder pauses the
    /// stream and discards what arrives while idle, and it pads the last
    /// chunk of a stop with zeros, so the start of the instruction would be
    /// lost.
    pub fn split_recording(&self, binding_id: &str) -> Option<RecordedSection> {
        // Under the state lock, so a stop or cancel can't run meanwhile.
        let state = self.state.lock().unwrap();
        if !matches!(&*state, RecordingState::Recording { binding_id: active } if active == binding_id)
        {
            return None;
        }
        let sections = self.sections.lock().unwrap().clone()?;
        let first = sections.split()?;
        drop(state);
        debug!(
            "Recording for binding {binding_id} cut while recording: {} samples before the cut",
            first.samples.len()
        );
        Some(RecordedSection {
            samples: padded_if_short(first.samples),
            input_silent: first.input_silent,
            speech: first.heard_speech,
        })
    }

    pub fn is_recording(&self) -> bool {
        // Lock-free: mirrors the `state` {Recording, Stopping} membership via
        // an atomic maintained by `set_state()`. Polled from the webview/main
        // thread, so it MUST NOT take the `state` mutex (a worker can hold it
        // across a slow CoreAudio open/close → main-thread deadlock / UI
        // freeze).
        self.recording_active.load(Ordering::SeqCst)
    }

    /// Cancel any ongoing recording without returning audio samples
    pub fn cancel_recording(&self) {
        self.invalidate_recording_readiness();
        self.cancel_generation.fetch_add(1, Ordering::AcqRel);
        let mut state = self.state.lock().unwrap();

        match *state {
            RecordingState::Recording { .. } => {
                self.set_state(&mut state, RecordingState::Idle);
                self.sections.lock().unwrap().take();
                drop(state);

                if let Some(microphone) = self.recorder.lock().unwrap().as_ref() {
                    let _ = self.collect_recording(microphone); // Discard the result
                }

                *self.is_recording.lock().unwrap() = false;

                // In on-demand mode, close the mic (lazily if the setting is enabled)
                if matches!(*self.mode.lock().unwrap(), MicrophoneMode::OnDemand) {
                    if (self.host.settings)().lazy_stream_close {
                        self.schedule_lazy_close();
                    } else {
                        self.stop_microphone_stream();
                    }
                }
            }
            RecordingState::Stopping => {
                debug!("Cancellation requested while recording is stopping");
            }
            RecordingState::Idle => {}
        }
    }
}

/// DanyWhere: a manager over handy-recorder's fake microphone, for the tests
/// here and those of the start of a dictation (`dictation_start`).
#[cfg(test)]
pub(crate) mod fake_microphone {
    use super::*;
    use handy_recorder::testing::FakeMic;

    /// A device at the rate and channels a recording delivers, so its
    /// samples come through unchanged.
    pub(crate) fn mic() -> FakeMic {
        FakeMic::new(WHISPER_SAMPLE_RATE as u32, 1)
    }

    /// Settings with nothing that reaches past the fake microphone: the
    /// default device, opened on demand and closed at once, the system's
    /// sound left alone, Earshot as the VAD (no model file).
    pub(crate) fn settings() -> AppSettings {
        let mut settings = crate::settings::get_default_settings();
        settings.selected_microphone = None;
        settings.clamshell_microphone = None;
        settings.always_on_microphone = false;
        settings.lazy_stream_close = false;
        settings.mute_while_recording = false;
        settings.extra_recording_buffer_ms = 0;
        settings.vad_backend = VadBackend::Earshot;
        settings
    }

    /// A manager recording from `mic`, with `settings`.
    pub(crate) fn manager(mic: &FakeMic, settings: AppSettings) -> AudioRecordingManager {
        AudioRecordingManager::with_host(host(mic, settings), Arc::new(|_| {}))
            .expect("a manager over the fake microphone")
    }

    /// The host of [`manager`]: `mic` is the system default and the only
    /// device opened; no microphone is listed, and the lid is open.
    pub(crate) fn host(mic: &FakeMic, settings: AppSettings) -> AudioHost {
        let mic = mic.clone();
        AudioHost {
            settings: Arc::new(move || settings.clone()),
            forget_microphone: Arc::new(|_| {}),
            create_vad: Arc::new(|backend| {
                let detector = EarshotVad::new(EARSHOT_VAD_THRESHOLD)
                    .map_err(|e| anyhow::anyhow!("Failed to create EarshotVad: {e}"))?;
                Ok(capture_vad(smooth(Box::new(detector)), backend))
            }),
            open_recorder: Arc::new(move |config, handler| {
                mic.open(config, Some(handler), Duration::from_secs(2))
            }),
            list_microphones: Arc::new(|| Ok(Vec::new())),
            is_clamshell: Arc::new(|| false),
            on_levels: Arc::new(|_| {}),
        }
    }

    /// A microphone plugged in: how the list shows it, and the fake behind it.
    type Plugged = (MicrophoneDevice, FakeMic);

    /// An open asked for: the device (`None`: the default) and channels.
    type Opened = (Option<String>, Channels);

    /// Several fake microphones behind one host, as on a Mac with its own
    /// microphone and others plugged in: each with an id and a name, one of
    /// them the system default. Opening an id opens that fake microphone;
    /// an id that isn't plugged in fails as a missing device does. Writes
    /// down every open, every listing, every question about the lid and
    /// every microphone forgotten. Clones share everything.
    #[derive(Clone)]
    pub(crate) struct Microphones {
        default: FakeMic,
        plugged: Arc<Mutex<Vec<Plugged>>>,
        list_fails: Arc<AtomicBool>,
        lid_closed: Arc<AtomicBool>,
        opened: Arc<Mutex<Vec<Opened>>>,
        forgotten: Arc<Mutex<Vec<String>>>,
        listed: Arc<AtomicU64>,
        lid_asked: Arc<AtomicU64>,
    }

    impl Microphones {
        /// The system default microphone, listed with `id` and `name`.
        pub(crate) fn with_default(id: &str, name: &str) -> Self {
            let default = mic();
            let microphones = Self {
                default: default.clone(),
                plugged: Arc::new(Mutex::new(Vec::new())),
                list_fails: Arc::new(AtomicBool::new(false)),
                lid_closed: Arc::new(AtomicBool::new(false)),
                opened: Arc::new(Mutex::new(Vec::new())),
                forgotten: Arc::new(Mutex::new(Vec::new())),
                listed: Arc::new(AtomicU64::new(0)),
                lid_asked: Arc::new(AtomicU64::new(0)),
            };
            microphones
                .plugged
                .lock()
                .unwrap()
                .push((listed(id, name, true, 1), default));
            microphones
        }

        pub(crate) fn default_mic(&self) -> FakeMic {
            self.default.clone()
        }

        /// Plugs in a microphone with one channel, which the list says has
        /// `listed_channels`.
        pub(crate) fn plug(&self, id: &str, name: &str, listed_channels: u16) -> FakeMic {
            let plugged = mic();
            self.plugged
                .lock()
                .unwrap()
                .push((listed(id, name, false, listed_channels), plugged.clone()));
            plugged
        }

        /// Unplugs the microphone named `name`: it leaves the list, and its
        /// id no longer opens.
        pub(crate) fn unplug(&self, name: &str) {
            self.plugged
                .lock()
                .unwrap()
                .retain(|(device, _)| device.name != name);
        }

        pub(crate) fn fail_listing(&self) {
            self.list_fails.store(true, Ordering::Release);
        }

        pub(crate) fn close_lid(&self) {
            self.lid_closed.store(true, Ordering::Release);
        }

        /// The device id every open asked for (`None`: the default).
        pub(crate) fn opened(&self) -> Vec<Option<String>> {
            let opened = self.opened.lock().unwrap();
            opened.iter().map(|(device, _)| device.clone()).collect()
        }

        /// The channels every open asked for.
        pub(crate) fn channels_asked(&self) -> Vec<Channels> {
            let opened = self.opened.lock().unwrap();
            opened.iter().map(|(_, channels)| *channels).collect()
        }

        /// The selected microphones the default stood in for.
        pub(crate) fn forgotten(&self) -> Vec<String> {
            self.forgotten.lock().unwrap().clone()
        }

        pub(crate) fn times_listed(&self) -> u64 {
            self.listed.load(Ordering::Acquire)
        }

        pub(crate) fn times_lid_asked(&self) -> u64 {
            self.lid_asked.load(Ordering::Acquire)
        }

        /// A manager over these microphones, with `settings`.
        pub(crate) fn manager(&self, settings: AppSettings) -> AudioRecordingManager {
            let mut host = host(&self.default, settings);
            let this = self.clone();
            host.open_recorder = Arc::new(move |config, handler| this.open(config, handler));
            let this = self.clone();
            host.list_microphones = Arc::new(move || this.list());
            let this = self.clone();
            host.is_clamshell = Arc::new(move || {
                this.lid_asked.fetch_add(1, Ordering::AcqRel);
                this.lid_closed.load(Ordering::Acquire)
            });
            let this = self.clone();
            host.forget_microphone =
                Arc::new(move |name| this.forgotten.lock().unwrap().push(name.to_string()));
            AudioRecordingManager::with_host(host, Arc::new(|_| {}))
                .expect("a manager over the fake microphones")
        }

        fn open(
            &self,
            config: RecorderConfig,
            handler: RecorderFailureHandler,
        ) -> Result<Recorder<CaptureSink>, handy_recorder::Error> {
            self.opened
                .lock()
                .unwrap()
                .push((config.device.clone(), config.channels));
            let plugged = match &config.device {
                None => Some(self.default.clone()),
                Some(id) => self
                    .plugged
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|(device, _)| device.id == *id)
                    .map(|(_, plugged)| plugged.clone()),
            };
            match plugged {
                Some(plugged) => plugged.open(
                    RecorderConfig {
                        device: None,
                        ..config
                    },
                    Some(handler),
                    Duration::from_secs(2),
                ),
                // A fake microphone knows only its own id, so any other
                // fails as a device that isn't there.
                None => mic().open(config, Some(handler), Duration::from_secs(2)),
            }
        }

        fn list(&self) -> Result<Vec<MicrophoneDevice>, anyhow::Error> {
            self.listed.fetch_add(1, Ordering::AcqRel);
            if self.list_fails.load(Ordering::Acquire) {
                return Err(anyhow::anyhow!("the fake microphones can't be listed"));
            }
            let plugged = self.plugged.lock().unwrap();
            Ok(plugged.iter().map(|(device, _)| device.clone()).collect())
        }
    }

    fn listed(id: &str, name: &str, is_default: bool, channels: u16) -> MicrophoneDevice {
        MicrophoneDevice {
            id: id.to_string(),
            name: name.to_string(),
            is_default,
            channels: Some(channels),
            is_monitor: false,
        }
    }

    /// `len` samples that are neither silence nor all alike.
    pub(crate) fn speech(len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| (i % 1000) as f32 / 1000.0 - 0.4995)
            .collect()
    }

    /// Delivers `samples` in blocks of 10 ms, as the device's audio thread
    /// would.
    pub(crate) fn play(mic: &FakeMic, samples: &[f32]) {
        for block in samples.chunks(160) {
            assert!(mic.push(block), "the fake microphone is streaming");
        }
    }

    /// Stops `binding`'s recording on `rm`, playing the audio thread's part:
    /// a stop asks the stream to pause and waits for the next callback,
    /// which belongs to the recording. An empty block is one that adds no
    /// audio. Returns what `stop_recording` gives back.
    pub(crate) fn stop(
        rm: &AudioRecordingManager,
        mic: &FakeMic,
        binding: &str,
    ) -> Option<Vec<f32>> {
        let stopped = AtomicBool::new(false);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                while !stopped.load(Ordering::Acquire) {
                    mic.push(&[]);
                    std::thread::sleep(Duration::from_millis(1));
                }
            });
            let samples = rm.stop_recording(binding, rm.cancel_generation());
            stopped.store(true, Ordering::Release);
            samples
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use handy_recorder::testing::{FakeError, FakeMic};

    // DanyWhere: lifting the mute while recording for the instruction
    // sound, as pure steps of `MuteState`; the manager runs them under one
    // lock, so an end of the recording between lifting and putting back
    // decides in one order or the other, never in between.

    /// Our mute is on; the system was not muted before.
    fn muted() -> MuteState {
        MuteState {
            did_mute: true,
            prev_muted: Some(false),
            lifted: false,
        }
    }

    #[test]
    fn a_lifted_mute_restores_the_sound_and_comes_back_after_it() {
        let mut state = muted();
        assert!(state.lift(), "the sound is restored for the sound");
        assert!(!state.lift(), "lifted once");
        assert!(state.put_back(), "muted again after it");
        assert!(!state.put_back(), "put back once");
        assert!(state.end(), "the end restores the sound");
    }

    #[test]
    fn a_recording_that_ends_during_the_sound_leaves_the_system_unmuted() {
        let mut state = muted();
        assert!(state.lift());
        assert!(
            !state.end(),
            "the sound is restored already; nothing to do at the end"
        );
        assert!(!state.put_back(), "no mute once the recording ended");
        assert!(!state.did_mute);
    }

    #[test]
    fn without_our_mute_nothing_is_lifted_or_put_back() {
        let mut state = MuteState::default();
        assert!(!state.lift());
        assert!(!state.put_back());
        assert!(!state.end());
    }

    #[test]
    fn a_new_mute_forgets_a_lift_of_the_recording_before() {
        let mut state = muted();
        assert!(state.lift());
        assert!(!state.end());
        // The next recording mutes (`apply_mute`); the old sound's put back
        // comes late.
        state.did_mute = true;
        state.lifted = false;
        assert!(!state.put_back(), "the new mute is not touched");
        assert!(state.end());
    }

    // DanyWhere: the manager over a fake microphone (`AudioHost`).

    #[test]
    fn a_recording_gives_back_what_the_microphone_sent() {
        let mic = fake_microphone::mic();
        let rm = fake_microphone::manager(&mic, fake_microphone::settings());
        let readiness = rm
            .try_start_recording("transcribe", VadPolicy::Disabled)
            .expect("the recording starts");
        assert!(rm.is_recording());
        let speech = fake_microphone::speech(24_000);
        fake_microphone::play(&mic, &speech);
        assert!(readiness.wait(), "the first audio makes it ready");

        assert_eq!(fake_microphone::stop(&rm, &mic, "transcribe"), Some(speech));
        assert!(!rm.is_recording());
        // On demand, the microphone closes with the recording.
        assert!(!mic.is_streaming());
    }

    #[test]
    fn a_microphone_without_access_fails_the_start_with_its_notice() {
        let mic = fake_microphone::mic();
        mic.fail_next_open(FakeError::PermissionDenied);
        let rm = fake_microphone::manager(&mic, fake_microphone::settings());
        let error = rm
            .try_start_recording("transcribe", VadPolicy::Disabled)
            .err()
            .expect("the start fails");
        assert_eq!(
            recording_notice_code(&error),
            NoticeCode::MicPermissionDenied
        );
        assert!(!rm.is_recording());
    }

    #[test]
    fn a_stop_before_any_audio_ends_the_wait_for_readiness() {
        let mic = fake_microphone::mic();
        let rm = fake_microphone::manager(&mic, fake_microphone::settings());
        let readiness = rm
            .try_start_recording("transcribe", VadPolicy::Disabled)
            .expect("the recording starts");
        assert_eq!(
            fake_microphone::stop(&rm, &mic, "transcribe"),
            Some(Vec::new())
        );
        assert!(!readiness.wait(), "no audio came, so it never got ready");
    }

    /// A recorder error, without touching a device: the request is rejected
    /// before one is opened.
    fn recorder_error() -> handy_recorder::Error {
        let config = RecorderConfig {
            sample_rate: Some(0),
            ..RecorderConfig::speech()
        };
        match Recorder::<CaptureSink>::open(config) {
            Err(error) => error,
            Ok(_) => panic!("a 0 Hz output rate should be rejected"),
        }
    }

    #[test]
    fn recorder_errors_are_classified_through_added_context() {
        let error = recorder_error();
        assert_eq!(error.kind(), ErrorKind::UnsupportedFormat);

        let wrapped = anyhow::Error::new(error).context("Failed to start recorder");
        assert_eq!(
            wrapped
                .downcast_ref::<handy_recorder::Error>()
                .map(handy_recorder::Error::kind),
            Some(ErrorKind::UnsupportedFormat)
        );
        assert_eq!(recording_notice_code(&wrapped), NoticeCode::RecordingFailed);
    }

    #[test]
    fn errors_from_outside_the_recorder_are_unknown() {
        assert_eq!(
            recording_notice_code(&anyhow::anyhow!("Already recording")),
            NoticeCode::RecordingFailed
        );
    }

    #[test]
    fn microphone_config_maps_the_selected_channel() {
        let all = microphone_config(None, None, false, 480);
        assert_eq!(all.channels, Channels::MixToMono);
        assert_eq!(all.sample_rate, Some(16_000));
        assert_eq!(all.frames_per_chunk, Some(480));

        let second = microphone_config(Some("id".into()), Some(1), false, 256);
        assert_eq!(second.channels, Channels::Only(1));
        assert_eq!(second.device.as_deref(), Some("id"));
        assert_eq!(second.frames_per_chunk, Some(256));
    }

    #[test]
    fn microphone_config_takes_the_headset_only_in_airpods_mode() {
        assert!(!microphone_config(None, None, false, 480).take_headset);
        assert!(microphone_config(None, None, true, 480).take_headset);
    }

    // DanyWhere: which microphone a recording opens, among several fake
    // ones (`fake_microphone::Microphones`).

    const BUILT_IN: &str = "MacBook Pro Microphone";
    const USB: &str = "USB Mic";
    const DESK: &str = "Desk Mic";

    /// The Mac's own microphone as the default, and a USB microphone.
    fn built_in_and_usb() -> (fake_microphone::Microphones, FakeMic, FakeMic) {
        let microphones = fake_microphone::Microphones::with_default("built-in", BUILT_IN);
        let usb = microphones.plug("usb-7", USB, 1);
        let built_in = microphones.default_mic();
        (microphones, built_in, usb)
    }

    fn selecting(microphone: Option<&str>) -> AppSettings {
        let mut settings = fake_microphone::settings();
        settings.selected_microphone = microphone.map(str::to_string);
        settings
    }

    fn ids(expected: &[Option<&str>]) -> Vec<Option<String>> {
        expected.iter().map(|id| id.map(str::to_string)).collect()
    }

    fn start(rm: &AudioRecordingManager) {
        rm.try_start_recording("transcribe", VadPolicy::Disabled)
            .expect("the recording starts");
    }

    /// Sends speech from `mic` and stops the recording: whether what came
    /// out is what `mic` sent.
    fn finish(rm: &AudioRecordingManager, mic: &FakeMic) -> bool {
        let speech = fake_microphone::speech(24_000);
        fake_microphone::play(mic, &speech);
        fake_microphone::stop(rm, mic, "transcribe") == Some(speech)
    }

    #[test]
    fn a_selected_microphone_is_opened_by_its_id() {
        let (microphones, _, usb) = built_in_and_usb();
        let rm = microphones.manager(selecting(Some(USB)));
        start(&rm);
        assert_eq!(microphones.opened(), ids(&[Some("usb-7")]));
        assert!(
            finish(&rm, &usb),
            "the recording came from the USB microphone"
        );
        assert!(microphones.forgotten().is_empty());
    }

    #[test]
    fn a_missing_selected_microphone_records_from_the_default_and_is_forgotten() {
        let (microphones, built_in, _) = built_in_and_usb();
        microphones.unplug(USB);
        let rm = microphones.manager(selecting(Some(USB)));
        start(&rm);
        assert_eq!(microphones.times_listed(), 1);
        assert_eq!(microphones.opened(), ids(&[None]));
        assert!(finish(&rm, &built_in));
        assert_eq!(microphones.forgotten(), [USB]);
    }

    #[test]
    fn a_failing_default_keeps_the_missing_microphone_selected() {
        let (microphones, built_in, _) = built_in_and_usb();
        microphones.unplug(USB);
        built_in.fail_next_open(FakeError::DeviceNotAvailable);
        let rm = microphones.manager(selecting(Some(USB)));
        let error = rm
            .try_start_recording("transcribe", VadPolicy::Disabled)
            .err()
            .expect("the start fails");
        assert_eq!(recording_notice_code(&error), NoticeCode::NoMicrophone);
        assert_eq!(microphones.times_listed(), 1);
        assert!(
            microphones.forgotten().is_empty(),
            "a fallback that failed keeps the choice"
        );
    }

    #[test]
    fn a_list_that_fails_records_from_the_default_and_keeps_the_selection() {
        let (microphones, built_in, _) = built_in_and_usb();
        microphones.fail_listing();
        let rm = microphones.manager(selecting(Some(USB)));
        start(&rm);
        assert_eq!(microphones.times_listed(), 1);
        assert_eq!(microphones.opened(), ids(&[None]));
        assert!(finish(&rm, &built_in));
        assert!(
            microphones.forgotten().is_empty(),
            "the list may fail only for a moment"
        );
    }

    #[test]
    fn the_clamshell_microphone_is_used_while_the_lid_is_closed() {
        let (microphones, _, _) = built_in_and_usb();
        let desk = microphones.plug("desk-3", DESK, 1);
        microphones.close_lid();
        let mut settings = selecting(Some(USB));
        settings.clamshell_microphone = Some(DESK.to_string());
        let rm = microphones.manager(settings);
        start(&rm);
        assert_eq!(microphones.opened(), ids(&[Some("desk-3")]));
        assert!(finish(&rm, &desk));
    }

    #[test]
    fn with_the_lid_open_the_selected_microphone_is_used() {
        let (microphones, _, usb) = built_in_and_usb();
        microphones.plug("desk-3", DESK, 1);
        let mut settings = selecting(Some(USB));
        settings.clamshell_microphone = Some(DESK.to_string());
        let rm = microphones.manager(settings);
        start(&rm);
        assert_eq!(microphones.times_lid_asked(), 1);
        assert_eq!(microphones.opened(), ids(&[Some("usb-7")]));
        assert!(finish(&rm, &usb));
    }

    #[test]
    fn the_lid_is_asked_about_only_with_a_clamshell_microphone_set() {
        let (microphones, _, usb) = built_in_and_usb();
        microphones.close_lid();
        let rm = microphones.manager(selecting(Some(USB)));
        start(&rm);
        assert_eq!(microphones.opened(), ids(&[Some("usb-7")]));
        assert_eq!(microphones.times_lid_asked(), 0);
        assert!(finish(&rm, &usb));
    }

    #[test]
    fn a_missing_clamshell_microphone_records_from_the_default_not_the_selected_one() {
        // Today's behaviour: with the lid closed and the clamshell
        // microphone gone, the system default stands in, though the
        // selected microphone is there. Both choices are kept.
        let (microphones, built_in, _) = built_in_and_usb();
        microphones.close_lid();
        let mut settings = selecting(Some(USB));
        settings.clamshell_microphone = Some(DESK.to_string());
        let rm = microphones.manager(settings);
        start(&rm);
        assert_eq!(microphones.times_listed(), 1);
        assert_eq!(microphones.opened(), ids(&[None]));
        assert!(finish(&rm, &built_in));
        assert!(microphones.forgotten().is_empty());
    }

    #[test]
    fn a_found_microphone_opens_again_without_a_new_list() {
        let (microphones, _, usb) = built_in_and_usb();
        let rm = microphones.manager(selecting(Some(USB)));
        for _ in 0..2 {
            start(&rm);
            assert!(finish(&rm, &usb));
        }
        assert_eq!(microphones.opened(), ids(&[Some("usb-7"), Some("usb-7")]));
        assert_eq!(microphones.times_listed(), 1);
    }

    #[test]
    fn an_unplugged_microphone_is_looked_up_again_and_the_default_stands_in() {
        let (microphones, built_in, usb) = built_in_and_usb();
        let rm = microphones.manager(selecting(Some(USB)));
        start(&rm);
        assert!(finish(&rm, &usb));
        microphones.unplug(USB);
        start(&rm);
        // The id found before fails, so it lists again and takes the default.
        assert_eq!(
            microphones.opened(),
            ids(&[Some("usb-7"), Some("usb-7"), None])
        );
        assert_eq!(microphones.times_listed(), 2);
        assert!(finish(&rm, &built_in));
        assert_eq!(microphones.forgotten(), [USB]);
    }

    #[test]
    fn a_microphone_without_the_selected_channel_records_all_channels_mixed() {
        // The list says two channels, the device takes one: the second
        // channel, chosen there, isn't there when the stream opens.
        let microphones = fake_microphone::Microphones::with_default("built-in", BUILT_IN);
        let usb = microphones.plug("usb-7", USB, 2);
        let mut settings = selecting(Some(USB));
        settings.selected_channel = Some(1);
        let rm = microphones.manager(settings);
        start(&rm);
        assert_eq!(microphones.opened(), ids(&[Some("usb-7"), Some("usb-7")]));
        assert_eq!(
            microphones.channels_asked(),
            [Channels::Only(1), Channels::MixToMono]
        );
        assert!(finish(&rm, &usb));
    }

    #[test]
    fn a_microphone_that_fails_while_recording_keeps_the_audio_before_it_and_opens_again() {
        let (microphones, _, usb) = built_in_and_usb();
        let mut settings = selecting(Some(USB));
        settings.always_on_microphone = true;
        let rm = microphones.manager(settings);
        assert_eq!(
            microphones.opened(),
            ids(&[Some("usb-7")]),
            "always on: opened at once"
        );
        start(&rm);
        let speech = fake_microphone::speech(24_000);
        fake_microphone::play(&usb, &speech);
        assert!(usb.report_error(FakeError::DeviceNotAvailable));
        assert_eq!(fake_microphone::stop(&rm, &usb, "transcribe"), Some(speech));
        start(&rm);
        assert_eq!(
            microphones.opened(),
            ids(&[Some("usb-7"), Some("usb-7")]),
            "the failed stream opened again"
        );
        assert!(finish(&rm, &usb));
    }

    // DanyWhere: cutting a recording in two while the microphone runs
    // (`split_recording`), for the spoken instruction.

    mod split {
        use super::*;
        use crate::audio_toolkit::vad::VadFrame;
        use std::sync::atomic::AtomicUsize;

        /// Samples per chunk: the frame of the fake host's detector
        /// (Earshot), which the microphone is opened with.
        const CHUNK: usize = 256;
        const BINDING: &str = "transcribe";

        /// The `i`-th sample of a ramp: every value differs, none is zero,
        /// each is exact in f32.
        fn ramp_at(i: usize) -> f32 {
            (i + 1) as f32 / 65_536.0
        }

        fn ramp(range: std::ops::Range<usize>) -> Vec<f32> {
            range.map(ramp_at).collect()
        }

        /// A manager over `host` whose live feed is written down.
        fn feeding(host: AudioHost) -> (AudioRecordingManager, Arc<Mutex<Vec<f32>>>) {
            let fed = Arc::new(Mutex::new(Vec::new()));
            let feed = Arc::clone(&fed);
            let rm = AudioRecordingManager::with_host(
                host,
                Arc::new(move |frame: &[f32]| feed.lock().unwrap().extend_from_slice(frame)),
            )
            .expect("a manager over the fake microphone");
            (rm, fed)
        }

        /// A manager over `mic` with the test settings, its live feed
        /// written down.
        fn manager(mic: &FakeMic) -> (AudioRecordingManager, Arc<Mutex<Vec<f32>>>) {
            feeding(fake_microphone::host(mic, fake_microphone::settings()))
        }

        fn wait_until(what: &str, done: impl Fn() -> bool) {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !done() {
                assert!(Instant::now() < deadline, "timed out waiting: {what}");
                std::thread::sleep(Duration::from_millis(1));
            }
        }

        /// Waits until the live feed has `samples` samples: the sink has
        /// had them (without VAD every sample is fed).
        fn wait_fed(fed: &Arc<Mutex<Vec<f32>>>, samples: usize) {
            wait_until("the sink got the audio", || {
                fed.lock().unwrap().len() >= samples
            });
        }

        fn start(rm: &AudioRecordingManager, policy: VadPolicy) -> RecordingReadiness {
            rm.try_start_recording(BINDING, policy)
                .expect("the recording starts")
        }

        #[test]
        fn the_split_gives_the_first_section_and_the_stop_the_second() {
            let mic = fake_microphone::mic();
            let (rm, fed) = manager(&mic);
            start(&rm, VadPolicy::Disabled);
            let first_len = CHUNK * 70;
            fake_microphone::play(&mic, &ramp(0..first_len));
            wait_fed(&fed, first_len);

            let first = rm.split_recording(BINDING).expect("the recording is cut");
            assert_eq!(first.samples, ramp(0..first_len));
            assert!(!first.input_silent);
            assert!(rm.is_recording(), "the recording goes on");

            let total = first_len + 17_000;
            fake_microphone::play(&mic, &ramp(first_len..total));
            assert_eq!(
                fake_microphone::stop(&rm, &mic, BINDING),
                Some(ramp(first_len..total))
            );
        }

        #[test]
        fn no_sample_is_lost_or_doubled_while_the_microphone_sends_through_the_cut() {
            let mic = fake_microphone::mic();
            let (rm, fed) = manager(&mic);
            start(&rm, VadPolicy::Disabled);
            let split_done = Arc::new(AtomicBool::new(false));

            // The device's audio thread: 10 ms blocks, on and on, until well
            // after the cut.
            let pusher = {
                let mic = mic.clone();
                let split_done = Arc::clone(&split_done);
                std::thread::spawn(move || {
                    let (mut sent, mut after_cut) = (0, 0);
                    while after_cut < 120 && sent < 60_000 {
                        assert!(mic.push(&ramp(sent..sent + 160)));
                        sent += 160;
                        if split_done.load(Ordering::Acquire) {
                            after_cut += 1;
                        }
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    sent
                })
            };
            wait_fed(&fed, 16_000);
            let first = rm.split_recording(BINDING).expect("the recording is cut");
            split_done.store(true, Ordering::Release);
            let sent = pusher.join().expect("the audio thread");
            let second = fake_microphone::stop(&rm, &mic, BINDING).expect("the second section");

            assert!(first.samples.len() >= 16_000 && second.len() >= 16_000);
            let mut both = first.samples.clone();
            both.extend_from_slice(&second);
            assert_eq!(
                both.len(),
                sent,
                "{} + {} samples",
                first.samples.len(),
                second.len()
            );
            assert!(
                both == ramp(0..sent),
                "the two sections are the ramp without a gap or a repeat"
            );
        }

        /// Speech where a frame's mean level is above 0.1, under a
        /// recording's real smoothing (pre-roll, onset, hangover); counts
        /// the frames it was given.
        struct LevelVad {
            frames: Arc<AtomicUsize>,
        }

        impl VoiceActivityDetector for LevelVad {
            fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> anyhow::Result<VadFrame<'a>> {
                self.frames.fetch_add(1, Ordering::AcqRel);
                let level = frame.iter().map(|s| s.abs()).sum::<f32>() / frame.len() as f32;
                Ok(if level > 0.1 {
                    VadFrame::Speech(frame)
                } else {
                    VadFrame::Noise
                })
            }

            fn frame_samples(&self) -> usize {
                CHUNK
            }
        }

        /// A manager over `mic` whose recordings run through [`LevelVad`],
        /// and the count of frames it saw.
        fn with_level_vad(
            mic: &FakeMic,
        ) -> (
            AudioRecordingManager,
            Arc<Mutex<Vec<f32>>>,
            Arc<AtomicUsize>,
        ) {
            let frames = Arc::new(AtomicUsize::new(0));
            let mut host = fake_microphone::host(mic, fake_microphone::settings());
            let seen = Arc::clone(&frames);
            host.create_vad = Arc::new(move |backend| {
                Ok(capture_vad(
                    smooth(Box::new(LevelVad {
                        frames: Arc::clone(&seen),
                    })),
                    backend,
                ))
            });
            let (rm, fed) = feeding(host);
            (rm, fed, frames)
        }

        /// Three times a quarter second of speech-like sound, each followed
        /// by two seconds of silence: longer than VAD's pre-roll and
        /// hangover together, so some of it is dropped.
        fn speech_and_pauses() -> Vec<f32> {
            (0..3 * PERIOD)
                .map(|i| {
                    if i % PERIOD < 4_000 {
                        0.5 * (((i % 100) as f32 / 50.0) - 1.0)
                    } else {
                        0.0
                    }
                })
                .collect()
        }

        const PERIOD: usize = 36_000;

        /// Plays `audio` and waits, every half second of it, until the
        /// detector had it: the device's ring holds two seconds.
        fn play_through(mic: &FakeMic, audio: &[f32], frames: &AtomicUsize, before: usize) {
            for (i, part) in audio.chunks(8_000).enumerate() {
                fake_microphone::play(mic, part);
                let played = before + i * 8_000 + part.len();
                wait_until("the detector had the audio", || {
                    frames.load(Ordering::Acquire) >= played / CHUNK
                });
            }
        }

        #[test]
        fn with_vad_the_two_sections_hold_what_one_recording_would() {
            let audio = speech_and_pauses();

            let mic = fake_microphone::mic();
            let (rm, _, frames) = with_level_vad(&mic);
            start(&rm, VadPolicy::Offline);
            play_through(&mic, &audio, &frames, 0);
            let whole = fake_microphone::stop(&rm, &mic, BINDING).expect("the recording");
            assert!(
                whole.len() >= WHISPER_SAMPLE_RATE && whole.len() < audio.len(),
                "VAD kept some and dropped some: {} of {}",
                whole.len(),
                audio.len()
            );

            // Cut while the speech's onset is still pending, in the middle
            // of speech, and in the hangover after it.
            for cut_at in [PERIOD + 700, PERIOD + 2_000, PERIOD + 6_000] {
                let mic = fake_microphone::mic();
                let (rm, fed, frames) = with_level_vad(&mic);
                start(&rm, VadPolicy::Offline);
                play_through(&mic, &audio[..cut_at], &frames, 0);
                let first = rm.split_recording(BINDING).expect("the recording is cut");
                play_through(&mic, &audio[cut_at..], &frames, cut_at);
                let second = fake_microphone::stop(&rm, &mic, BINDING).expect("the second");

                // A section shorter than a second comes padded; what VAD
                // let through before the cut is what the live feed got.
                let kept = fed.lock().unwrap().clone();
                assert!(!kept.is_empty(), "cut at {cut_at}: the text kept speech");
                assert_eq!(first.samples[..kept.len()], kept[..], "cut at {cut_at}");
                assert!(first.samples[kept.len()..].iter().all(|&s| s == 0.0));
                let mut both = kept;
                both.extend_from_slice(&second);
                assert_eq!(both, whole, "cut at {cut_at}");
            }
        }

        #[test]
        fn the_live_stream_gets_only_the_first_section() {
            let mic = fake_microphone::mic();
            let (rm, fed) = manager(&mic);
            start(&rm, VadPolicy::Disabled);
            fake_microphone::play(&mic, &ramp(0..CHUNK * 70));
            wait_fed(&fed, CHUNK * 70);
            let first = rm.split_recording(BINDING).expect("the recording is cut");
            fake_microphone::play(&mic, &ramp(CHUNK * 70..CHUNK * 150));
            let second = fake_microphone::stop(&rm, &mic, BINDING).expect("the second");

            assert_eq!(second.len(), CHUNK * 80);
            assert_eq!(*fed.lock().unwrap(), first.samples);
        }

        #[test]
        fn the_cut_leaves_the_microphone_open_the_mute_alone_and_the_start_current() {
            let mic = fake_microphone::mic();
            let (rm, fed) = manager(&mic);
            let readiness = start(&rm, VadPolicy::Disabled);
            fake_microphone::play(&mic, &ramp(0..CHUNK * 70));
            wait_fed(&fed, CHUNK * 70);
            // As if the output had been muted for the recording; a system
            // muted before stays muted, so nothing here touches the Mac.
            *rm.mute_state.lock().unwrap() = MuteState {
                did_mute: true,
                prev_muted: Some(true),
                ..Default::default()
            };

            assert!(rm.split_recording(BINDING).is_some());
            let mute = *rm.mute_state.lock().unwrap();
            assert!(mute.did_mute, "the output stays muted");
            assert_eq!(mute.prev_muted, Some(true));
            assert!(mic.is_streaming(), "the microphone stays open");
            assert!(rm.is_recording());
            assert!(
                rm.is_recording_readiness_current(readiness.generation()),
                "the recording that started is still the one running"
            );
            fake_microphone::play(&mic, &ramp(0..CHUNK));
            assert!(fake_microphone::stop(&rm, &mic, BINDING).is_some());
        }

        #[test]
        fn cancelling_after_the_cut_discards_the_second_section() {
            let mic = fake_microphone::mic();
            let (rm, fed) = manager(&mic);
            start(&rm, VadPolicy::Disabled);
            fake_microphone::play(&mic, &ramp(0..CHUNK * 70));
            wait_fed(&fed, CHUNK * 70);
            assert!(rm.split_recording(BINDING).is_some());
            fake_microphone::play(&mic, &ramp(0..CHUNK * 10));

            rm.cancel_recording();
            assert!(!rm.is_recording());
            assert_eq!(rm.stop_recording(BINDING, rm.cancel_generation()), None);
            assert!(!mic.is_streaming(), "on demand, the microphone closes");
        }

        #[test]
        fn a_second_section_without_audio_stops_empty() {
            let mic = fake_microphone::mic();
            let (rm, fed) = manager(&mic);
            start(&rm, VadPolicy::Disabled);
            fake_microphone::play(&mic, &ramp(0..CHUNK * 70));
            wait_fed(&fed, CHUNK * 70);
            assert!(rm.split_recording(BINDING).is_some());
            assert_eq!(fake_microphone::stop(&rm, &mic, BINDING), Some(Vec::new()));
        }

        #[test]
        fn only_a_running_recording_of_that_shortcut_is_cut_and_only_once() {
            let mic = fake_microphone::mic();
            let (rm, fed) = manager(&mic);
            assert_eq!(rm.split_recording(BINDING), None, "nothing records");

            start(&rm, VadPolicy::Disabled);
            fake_microphone::play(&mic, &ramp(0..CHUNK * 70));
            wait_fed(&fed, CHUNK * 70);
            assert_eq!(rm.split_recording("other"), None, "another shortcut's");
            assert!(rm.split_recording(BINDING).is_some());
            assert_eq!(rm.split_recording(BINDING), None, "cut once already");
            assert!(fake_microphone::stop(&rm, &mic, BINDING).is_some());
            assert_eq!(rm.split_recording(BINDING), None, "stopped");
        }

        #[test]
        fn a_short_first_section_is_padded_as_a_short_recording_is() {
            let mic = fake_microphone::mic();
            let (rm, fed) = manager(&mic);
            start(&rm, VadPolicy::Disabled);
            fake_microphone::play(&mic, &ramp(0..CHUNK * 20));
            wait_fed(&fed, CHUNK * 20);
            let first = rm.split_recording(BINDING).expect("the recording is cut");
            assert_eq!(first.samples.len(), WHISPER_SAMPLE_RATE * 5 / 4);
            assert_eq!(first.samples[..CHUNK * 20], ramp(0..CHUNK * 20)[..]);
            assert!(first.samples[CHUNK * 20..].iter().all(|&s| s == 0.0));
            assert!(fake_microphone::stop(&rm, &mic, BINDING).is_some());
        }

        #[test]
        fn each_section_reports_digital_silence_of_its_own() {
            let mic = fake_microphone::mic();
            let (rm, fed) = manager(&mic);
            start(&rm, VadPolicy::Disabled);
            fake_microphone::play(&mic, &vec![0.0; CHUNK * 70]);
            wait_fed(&fed, CHUNK * 70);
            let first = rm.split_recording(BINDING).expect("the recording is cut");
            assert!(first.input_silent, "the text had only zeros");

            fake_microphone::play(&mic, &ramp(0..CHUNK * 70));
            assert!(fake_microphone::stop(&rm, &mic, BINDING).is_some());
            assert!(!rm.last_input_silent(), "the instruction had sound");
        }
    }
}
