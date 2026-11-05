// Re-export all audio components
mod decode;
mod device;
mod opus_file;
mod recording;
mod sink;
mod utils;
mod visualizer;

pub use decode::decode_audio_file;
pub use device::{list_output_devices, output_device_name, OutputDeviceInfo};
pub use opus_file::{read_opus_samples, save_opus_file};
pub use recording::{
    read_recording_samples, recording_file_name, save_recording, verify_recording,
    write_playback_wav, RECORDING_EXTENSION,
};
pub use sink::{
    AudioFrameCallback, CaptureSink, CaptureVad, CapturedAudio, LevelCallback, VadPolicy,
};
pub use utils::{read_wav_samples, save_wav_file, verify_wav_file};
pub use visualizer::AudioVisualiser;
