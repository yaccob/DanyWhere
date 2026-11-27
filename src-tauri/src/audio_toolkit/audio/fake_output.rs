//! DanyWhere: fake output devices for the tests of feedback sounds and of
//! the list of output devices, built on cpal's `custom` host (enabled only
//! as a dev-dependency feature, so release builds never have it).
//!
//! A [`FakeOutput`] becomes an ordinary `cpal::Device` and [`FakeOutputs`]
//! an ordinary `cpal::Host`, so a test runs through rodio's and cpal's real
//! code; only the operating system is replaced. A fake device can be told to
//! misbehave the ways real ones do: refuse a configuration, offer something
//! other than it accepts, fail to open or start, lose its name, or report an
//! error while it plays.

use rodio::cpal::{
    self,
    platform::CustomHost,
    traits::{DeviceTrait, HostTrait, StreamTrait},
    BufferSize, Data, DeviceDescription, DeviceDescriptionBuilder, DeviceId, Error, ErrorKind,
    FrameCount, InputCallbackInfo, OutputCallbackInfo, OutputStreamTimestamp, SampleFormat,
    StreamConfig, StreamInstant, SupportedBufferSize, SupportedStreamConfig,
    SupportedStreamConfigRange,
};
use std::ops::RangeInclusive;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// What a device does while its stream plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Running {
    /// Asks for audio again and again, much faster than in real time.
    Pulls,
    /// Never asks for audio.
    Stalls,
    /// Asks for audio once, reports the error, then keeps asking.
    ReportsThenPulls(ErrorKind),
    /// Asks for audio once, reports the error, then never asks again, as a
    /// device that was unplugged.
    ReportsThenStalls(ErrorKind),
}

/// How a fake device behaves; tests set it up through [`FakeOutput`]'s
/// builder methods.
struct Behaviour {
    /// `None`: its name can't be read any more (the device has gone).
    name: Option<String>,
    /// What `default_output_config` offers; `None`: asking fails (the
    /// device has gone).
    offers: Option<SupportedStreamConfig>,
    /// A device for input only lists no output configurations, so cpal's
    /// `output_devices` leaves it out.
    input_only: bool,
    /// The buffer sizes a stream accepts, checked as cpal checks them on
    /// macOS (`coreaudio/macos/device.rs`); `None`: any.
    accepts_buffers: Option<RangeInclusive<FrameCount>>,
    /// The only sample rate a stream accepts; `None`: any.
    accepts_rate: Option<u32>,
    /// Building a stream fails with this.
    open_fails: Option<ErrorKind>,
    /// Starting a built stream fails with this.
    start_fails: Option<ErrorKind>,
    running: Running,
}

/// What a fake device has seen.
#[derive(Default)]
struct Seen {
    /// Every stream asked for, refused ones included.
    asked: Vec<StreamConfig>,
    open_streams: usize,
    peak: f32,
    errors_reported: usize,
    /// The first channel of every sample it got, of all its streams.
    heard: Vec<f32>,
}

/// One fake output device. Clones share the device.
#[derive(Clone)]
pub(crate) struct FakeOutput {
    id: String,
    behaviour: Arc<Mutex<Behaviour>>,
    seen: Arc<Mutex<Seen>>,
}

impl FakeOutput {
    /// A device named `name` that offers 44.1 kHz stereo f32 with any
    /// buffer size and plays whatever it gets.
    pub(crate) fn new(name: &str) -> Self {
        Self {
            id: name.to_string(),
            behaviour: Arc::new(Mutex::new(Behaviour {
                name: Some(name.to_string()),
                offers: Some(SupportedStreamConfig::new(
                    2,
                    44_100,
                    SupportedBufferSize::Unknown,
                    SampleFormat::F32,
                )),
                input_only: false,
                accepts_buffers: None,
                accepts_rate: None,
                open_fails: None,
                start_fails: None,
                running: Running::Pulls,
            })),
            seen: Arc::new(Mutex::new(Seen::default())),
        }
    }

    fn change(self, change: impl FnOnce(&mut Behaviour)) -> Self {
        change(&mut self.behaviour.lock().unwrap());
        self
    }

    /// Offers `rate` and `channels` in f32 with `buffers` as its format.
    pub(crate) fn offering(self, rate: u32, channels: u16, buffers: SupportedBufferSize) -> Self {
        self.change(|b| {
            b.offers = Some(SupportedStreamConfig::new(
                channels,
                rate,
                buffers,
                SampleFormat::F32,
            ))
        })
    }

    /// Accepts only buffer sizes in `range`, whatever it offers.
    pub(crate) fn accepting_buffers(self, range: RangeInclusive<FrameCount>) -> Self {
        self.change(|b| b.accepts_buffers = Some(range))
    }

    /// Accepts only `rate`, whatever it offers.
    pub(crate) fn accepting_only_rate(self, rate: u32) -> Self {
        self.change(|b| b.accepts_rate = Some(rate))
    }

    /// Its name can no longer be read.
    pub(crate) fn nameless(self) -> Self {
        self.change(|b| b.name = None)
    }

    /// Asking for its format fails, as for a device that has gone.
    pub(crate) fn without_format(self) -> Self {
        self.change(|b| b.offers = None)
    }

    /// A microphone, not an output.
    pub(crate) fn input_only(self) -> Self {
        self.change(|b| b.input_only = true)
    }

    /// Building a stream fails with `kind`.
    pub(crate) fn failing_to_open(self, kind: ErrorKind) -> Self {
        self.change(|b| b.open_fails = Some(kind))
    }

    /// Starting a built stream fails with `kind`.
    pub(crate) fn failing_to_start(self, kind: ErrorKind) -> Self {
        self.change(|b| b.start_fails = Some(kind))
    }

    /// Behaves as `running` says while it plays.
    pub(crate) fn running(self, running: Running) -> Self {
        self.change(|b| b.running = running)
    }

    /// Changes what it does while it plays, also in the middle of a sound.
    pub(crate) fn set_running(&self, running: Running) {
        self.behaviour.lock().unwrap().running = running;
    }

    /// The buffer size of every stream asked for, refused ones included.
    pub(crate) fn asked_buffers(&self) -> Vec<BufferSize> {
        self.seen
            .lock()
            .unwrap()
            .asked
            .iter()
            .map(|config| config.buffer_size)
            .collect()
    }

    /// The sample rate of every stream asked for.
    pub(crate) fn asked_rates(&self) -> Vec<u32> {
        self.seen
            .lock()
            .unwrap()
            .asked
            .iter()
            .map(|config| config.sample_rate)
            .collect()
    }

    /// Streams built and not yet dropped.
    pub(crate) fn open_streams(&self) -> usize {
        self.seen.lock().unwrap().open_streams
    }

    /// The loudest sample it got.
    pub(crate) fn peak(&self) -> f32 {
        self.seen.lock().unwrap().peak
    }

    /// The first channel of what it got, at the rate of the stream asked
    /// for last, from the first sample above silence to the last.
    pub(crate) fn heard(&self) -> (Vec<f32>, u32) {
        let seen = self.seen.lock().unwrap();
        let rate = seen.asked.last().map_or(0, |config| config.sample_rate);
        let loud = |s: &&f32| s.abs() > 1e-4;
        let start = seen.heard.iter().position(|s| loud(&s)).unwrap_or(0);
        let end = seen
            .heard
            .iter()
            .rposition(|s| loud(&s))
            .map_or(0, |i| i + 1);
        (seen.heard[start..end.max(start)].to_vec(), rate)
    }

    pub(crate) fn errors_reported(&self) -> usize {
        self.seen.lock().unwrap().errors_reported
    }

    fn name(&self) -> Option<String> {
        self.behaviour.lock().unwrap().name.clone()
    }

    /// Why a stream with `config` is refused, as cpal would say.
    fn refusal(&self, config: &StreamConfig, sample_format: SampleFormat) -> Option<Error> {
        let behaviour = self.behaviour.lock().unwrap();
        if let Some(kind) = behaviour.open_fails {
            return Some(Error::new(kind));
        }
        if sample_format != SampleFormat::F32 {
            return Some(Error::new(ErrorKind::UnsupportedConfig));
        }
        if let (BufferSize::Fixed(size), Some(range)) =
            (config.buffer_size, &behaviour.accepts_buffers)
        {
            if !range.contains(&size) {
                return Some(Error::with_message(
                    ErrorKind::UnsupportedConfig,
                    format!(
                        "Buffer size {size} is not in the supported range {}..={}",
                        range.start(),
                        range.end()
                    ),
                ));
            }
        }
        match behaviour.accepts_rate {
            Some(rate) if rate != config.sample_rate => {
                Some(Error::new(ErrorKind::UnsupportedConfig))
            }
            _ => None,
        }
    }
}

// cpal's `DeviceTrait` asks for these; a device is the same as another when
// their ids are.
impl PartialEq for FakeOutput {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for FakeOutput {}

impl std::hash::Hash for FakeOutput {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl std::fmt::Debug for FakeOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("FakeOutput").field(&self.id).finish()
    }
}

impl std::fmt::Display for FakeOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name().ok_or(std::fmt::Error)?)
    }
}

impl DeviceTrait for FakeOutput {
    type SupportedInputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;
    type SupportedOutputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;
    type Stream = FakeStream;

    fn description(&self) -> Result<DeviceDescription, Error> {
        match self.name() {
            Some(name) => Ok(DeviceDescriptionBuilder::new(name).build()),
            None => Err(Error::new(ErrorKind::DeviceNotAvailable)),
        }
    }

    fn id(&self) -> Result<DeviceId, Error> {
        Ok(DeviceId::new(cpal::HostId::Custom, &self.id))
    }

    fn supported_input_configs(&self) -> Result<Self::SupportedInputConfigs, Error> {
        Ok(Vec::new().into_iter())
    }

    fn supported_output_configs(&self) -> Result<Self::SupportedOutputConfigs, Error> {
        let behaviour = self.behaviour.lock().unwrap();
        if behaviour.input_only {
            return Ok(Vec::new().into_iter());
        }
        let offers = behaviour.offers.unwrap_or_else(|| {
            SupportedStreamConfig::new(2, 44_100, SupportedBufferSize::Unknown, SampleFormat::F32)
        });
        Ok(vec![SupportedStreamConfigRange::new(
            offers.channels(),
            offers.sample_rate(),
            offers.sample_rate(),
            *offers.buffer_size(),
            offers.sample_format(),
        )]
        .into_iter())
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, Error> {
        Err(Error::new(ErrorKind::UnsupportedConfig))
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, Error> {
        self.behaviour
            .lock()
            .unwrap()
            .offers
            .ok_or_else(|| Error::new(ErrorKind::DeviceNotAvailable))
    }

    fn build_input_stream_raw<D, E>(
        &self,
        _: StreamConfig,
        _: SampleFormat,
        _: D,
        _: E,
        _: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        Err(Error::new(ErrorKind::UnsupportedConfig))
    }

    fn build_output_stream_raw<D, E>(
        &self,
        config: StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        _: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        self.seen.lock().unwrap().asked.push(config);
        if let Some(refusal) = self.refusal(&config, sample_format) {
            return Err(refusal);
        }
        Ok(FakeStream::start(
            self.clone(),
            config,
            data_callback,
            error_callback,
        ))
    }
}

/// A fake device's stream: a thread that plays the device's audio thread.
pub(crate) struct FakeStream {
    device: FakeOutput,
    playing: Arc<AtomicBool>,
    exit: Arc<AtomicBool>,
    started: Instant,
    buffer: FrameCount,
    thread: Option<JoinHandle<()>>,
}

impl FakeStream {
    fn start<D, E>(
        device: FakeOutput,
        config: StreamConfig,
        mut data_callback: D,
        mut error_callback: E,
    ) -> Self
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        let buffer = match config.buffer_size {
            BufferSize::Fixed(size) => size,
            BufferSize::Default => 512,
        };
        let playing = Arc::new(AtomicBool::new(false));
        let exit = Arc::new(AtomicBool::new(false));
        let started = Instant::now();
        device.seen.lock().unwrap().open_streams += 1;
        let thread = std::thread::spawn({
            let device = device.clone();
            let playing = Arc::clone(&playing);
            let exit = Arc::clone(&exit);
            move || {
                let mut samples = vec![0.0f32; buffer as usize * config.channels as usize];
                let mut pulled = false;
                let mut reported = false;
                while !exit.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_micros(200));
                    if !playing.load(Ordering::Acquire) {
                        continue;
                    }
                    let running = device.behaviour.lock().unwrap().running;
                    let (report, pulls) = match running {
                        Running::Pulls => (None, true),
                        Running::Stalls => (None, false),
                        Running::ReportsThenPulls(kind) => (Some(kind), true),
                        Running::ReportsThenStalls(kind) => (Some(kind), !pulled),
                    };
                    if let Some(kind) = report {
                        if pulled && !reported {
                            reported = true;
                            device.seen.lock().unwrap().errors_reported += 1;
                            error_callback(Error::new(kind));
                            continue;
                        }
                    }
                    if !pulls {
                        continue;
                    }
                    // SAFETY: `samples` is a buffer of f32 samples of the
                    // length given, alive for the whole callback.
                    let mut data = unsafe {
                        Data::from_parts(
                            samples.as_mut_ptr().cast(),
                            samples.len(),
                            SampleFormat::F32,
                        )
                    };
                    let now = instant(started.elapsed());
                    data_callback(
                        &mut data,
                        &OutputCallbackInfo::new(OutputStreamTimestamp {
                            callback: now,
                            playback: now,
                        }),
                    );
                    pulled = true;
                    let peak = samples.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
                    let mut seen = device.seen.lock().unwrap();
                    seen.peak = seen.peak.max(peak);
                    let channels = usize::from(config.channels.max(1));
                    seen.heard.extend(samples.iter().step_by(channels));
                }
            }
        });
        Self {
            device,
            playing,
            exit,
            started,
            buffer,
            thread: Some(thread),
        }
    }
}

fn instant(elapsed: Duration) -> StreamInstant {
    StreamInstant::new(elapsed.as_secs(), elapsed.subsec_nanos())
}

impl StreamTrait for FakeStream {
    fn play(&self) -> Result<(), Error> {
        if let Some(kind) = self.device.behaviour.lock().unwrap().start_fails {
            return Err(Error::new(kind));
        }
        self.playing.store(true, Ordering::Release);
        Ok(())
    }

    fn pause(&self) -> Result<(), Error> {
        self.playing.store(false, Ordering::Release);
        Ok(())
    }

    fn now(&self) -> StreamInstant {
        instant(self.started.elapsed())
    }

    fn buffer_size(&self) -> Result<FrameCount, Error> {
        Ok(self.buffer)
    }
}

impl Drop for FakeStream {
    fn drop(&mut self) {
        self.exit.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        self.device.seen.lock().unwrap().open_streams -= 1;
    }
}

/// A fake host with fake output devices. Clones share the count of listings.
#[derive(Clone)]
pub(crate) struct FakeOutputs {
    devices: Vec<FakeOutput>,
    default: Option<FakeOutput>,
    list_fails: Option<ErrorKind>,
    listed: Arc<AtomicUsize>,
}

impl FakeOutputs {
    /// A host with `devices`, in this order, and no default device.
    pub(crate) fn new(devices: &[&FakeOutput]) -> Self {
        Self {
            devices: devices.iter().map(|&device| device.clone()).collect(),
            default: None,
            list_fails: None,
            listed: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// `device` is the system's default output.
    pub(crate) fn with_default(mut self, device: &FakeOutput) -> Self {
        self.default = Some(device.clone());
        self
    }

    /// Listing the devices fails with `kind`.
    pub(crate) fn failing_to_list(mut self, kind: ErrorKind) -> Self {
        self.list_fails = Some(kind);
        self
    }

    /// How often the devices were listed.
    pub(crate) fn times_listed(&self) -> usize {
        self.listed.load(Ordering::Acquire)
    }

    /// This host as cpal's own type, as `cpal::default_host` would give it.
    pub(crate) fn host(&self) -> cpal::Host {
        cpal::Host::from(CustomHost::from_host(self.clone()))
    }
}

impl HostTrait for FakeOutputs {
    type Device = FakeOutput;
    type Devices = std::vec::IntoIter<FakeOutput>;

    fn is_available() -> bool {
        true
    }

    fn devices(&self) -> Result<Self::Devices, Error> {
        self.listed.fetch_add(1, Ordering::AcqRel);
        match self.list_fails {
            Some(kind) => Err(Error::new(kind)),
            None => Ok(self.devices.clone().into_iter()),
        }
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        None
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        self.default.clone()
    }
}
