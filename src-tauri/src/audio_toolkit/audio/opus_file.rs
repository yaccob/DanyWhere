//! History recordings as Ogg Opus: 16 kHz mono at 24 kbit/s in VoIP mode,
//! about 12 times smaller than 16-bit WAV with the same transcripts.

use anyhow::{anyhow, bail, Result};
use ogg::reading::PacketReader;
use ogg::writing::{PacketWriteEndInfo, PacketWriter};
use opusic_c::{Application, Bitrate, Channels, Decoder, Encoder, SampleRate};
use std::io::Cursor;
use std::path::Path;

/// Sample rate of every recording.
pub const OPUS_SAMPLE_RATE: u32 = 16_000;
/// Bit rate new recordings are encoded at.
pub const OPUS_BITRATE: u32 = 24_000;

/// 20 ms frames at 16 kHz.
const FRAME_SAMPLES: usize = 320;
/// Ogg Opus counts granule positions and pre-skip at 48 kHz.
const GRANULE_PER_SAMPLE: u64 = (48_000 / OPUS_SAMPLE_RATE) as u64;
/// Largest packet libopus produces for one frame; far above 24 kbit/s.
const MAX_PACKET_BYTES: usize = 4_000;
/// Room for the longest Opus packet (120 ms) at 16 kHz.
const MAX_DECODED_SAMPLES: usize = 1_920;
/// Any value works for a file with a single logical stream.
const STREAM_SERIAL: u32 = 0x4441_4e59;

fn codec_error(what: &str, code: opusic_c::ErrorCode) -> anyhow::Error {
    anyhow!("Opus {what} failed: {code:?}")
}

/// The OpusHead packet (RFC 7845 §5.1) for a mono stream.
fn opus_head(pre_skip: u16) -> Vec<u8> {
    let mut head = Vec::with_capacity(19);
    head.extend_from_slice(b"OpusHead");
    head.push(1); // version
    head.push(1); // channels
    head.extend_from_slice(&pre_skip.to_le_bytes());
    head.extend_from_slice(&OPUS_SAMPLE_RATE.to_le_bytes());
    head.extend_from_slice(&0i16.to_le_bytes()); // output gain
    head.push(0); // channel mapping family: mono/stereo
    head
}

/// The OpusTags packet (RFC 7845 §5.2) with no user comments.
fn opus_tags() -> Vec<u8> {
    let vendor = b"libopus";
    let mut tags = Vec::with_capacity(8 + 4 + vendor.len() + 4);
    tags.extend_from_slice(b"OpusTags");
    tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    tags.extend_from_slice(vendor);
    tags.extend_from_slice(&0u32.to_le_bytes());
    tags
}

/// Encode `samples` (16 kHz mono, -1.0..=1.0) as an Ogg Opus stream.
pub fn encode_opus(samples: &[f32]) -> Result<Vec<u8>> {
    let mut encoder = Encoder::new(Channels::Mono, SampleRate::Hz16000, Application::Voip)
        .map_err(|e| codec_error("encoder setup", e))?;
    encoder
        .set_bitrate(Bitrate::Value(OPUS_BITRATE))
        .map_err(|e| codec_error("bit rate setup", e))?;
    let look_ahead = encoder
        .get_look_ahead()
        .map_err(|e| codec_error("look-ahead query", e))? as usize;
    let pre_skip = look_ahead as u64 * GRANULE_PER_SAMPLE;

    let mut writer = PacketWriter::new(Vec::new());
    writer.write_packet(
        opus_head(u16::try_from(pre_skip)?),
        STREAM_SERIAL,
        PacketWriteEndInfo::EndPage,
        0,
    )?;
    writer.write_packet(opus_tags(), STREAM_SERIAL, PacketWriteEndInfo::EndPage, 0)?;

    // Encode the look-ahead's worth of silence past the end too, so the
    // decoder gets every sample out; at least one frame, so an empty
    // recording is still a complete stream.
    let padded = samples.len() + look_ahead;
    let frames = padded.div_ceil(FRAME_SAMPLES).max(1);
    let final_granule = pre_skip + samples.len() as u64 * GRANULE_PER_SAMPLE;
    let mut frame = [0.0f32; FRAME_SAMPLES];
    let mut packet = vec![0u8; MAX_PACKET_BYTES];
    for index in 0..frames {
        let start = index * FRAME_SAMPLES;
        for (i, slot) in frame.iter_mut().enumerate() {
            *slot = samples.get(start + i).map_or(0.0, |s| s.clamp(-1.0, 1.0));
        }
        let len = encoder
            .encode_float_to_slice(&frame, &mut packet)
            .map_err(|e| codec_error("encoding", e))?;
        let last = index + 1 == frames;
        let granule = if last {
            final_granule
        } else {
            ((index + 1) * FRAME_SAMPLES) as u64 * GRANULE_PER_SAMPLE
        };
        let end = if last {
            PacketWriteEndInfo::EndStream
        } else {
            PacketWriteEndInfo::NormalPacket
        };
        writer.write_packet(packet[..len].to_vec(), STREAM_SERIAL, end, granule)?;
    }

    Ok(writer.into_inner())
}

/// Decode an Ogg Opus stream written by [`encode_opus`] into 16 kHz mono
/// samples, exactly as many as were encoded.
pub fn decode_opus(bytes: &[u8]) -> Result<Vec<f32>> {
    let mut reader = PacketReader::new(Cursor::new(bytes));
    let head = reader
        .read_packet()?
        .ok_or_else(|| anyhow!("Not an Ogg Opus stream: no packets"))?;
    let head = head.data;
    if head.len() < 19 || &head[..8] != b"OpusHead" {
        bail!("Not an Ogg Opus stream: no OpusHead packet");
    }
    if head[9] != 1 {
        bail!("Unsupported Opus recording with {} channels", head[9]);
    }
    let pre_skip = u64::from(u16::from_le_bytes([head[10], head[11]]));

    let tags = reader
        .read_packet()?
        .ok_or_else(|| anyhow!("Ogg Opus stream ends after its header"))?;
    if !tags.data.starts_with(b"OpusTags") {
        bail!("Not an Ogg Opus stream: no OpusTags packet");
    }

    let mut decoder = Decoder::new(Channels::Mono, SampleRate::Hz16000)
        .map_err(|e| codec_error("decoder setup", e))?;
    let mut decoded = Vec::new();
    let mut buffer = [0.0f32; MAX_DECODED_SAMPLES];
    let mut final_granule = None;
    while let Some(packet) = reader.read_packet()? {
        let n = decoder
            .decode_float_to_slice(&packet.data, &mut buffer, false)
            .map_err(|e| codec_error("decoding", e))?;
        decoded.extend_from_slice(&buffer[..n]);
        if packet.last_in_page() {
            final_granule = Some(packet.absgp_page());
        }
        if packet.last_in_stream() {
            break;
        }
    }

    let skip = (pre_skip / GRANULE_PER_SAMPLE) as usize;
    let available = decoded.len().saturating_sub(skip);
    let length = match final_granule {
        Some(granule) => {
            ((granule.saturating_sub(pre_skip) / GRANULE_PER_SAMPLE) as usize).min(available)
        }
        None => available,
    };
    Ok(decoded
        .into_iter()
        .skip(skip)
        .take(length)
        .map(|s| s.clamp(-1.0, 1.0))
        .collect())
}

/// Write `samples` to `path` as Ogg Opus.
pub fn save_opus_file<P: AsRef<Path>>(path: P, samples: &[f32]) -> Result<()> {
    std::fs::write(path.as_ref(), encode_opus(samples)?)?;
    Ok(())
}

/// Read an Ogg Opus file written by [`save_opus_file`].
pub fn read_opus_samples<P: AsRef<Path>>(path: P) -> Result<Vec<f32>> {
    decode_opus(&std::fs::read(path.as_ref())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `seconds` of a sine at `hz` with the given amplitude.
    fn sine(hz: f32, seconds: f32, amplitude: f32) -> Vec<f32> {
        let n = (seconds * OPUS_SAMPLE_RATE as f32) as usize;
        (0..n)
            .map(|i| {
                amplitude
                    * (2.0 * std::f32::consts::PI * hz * i as f32 / OPUS_SAMPLE_RATE as f32).sin()
            })
            .collect()
    }

    /// Speech-like test signal: a few harmonics whose loudness changes
    /// like syllables, with pauses.
    fn speech_like(seconds: f32) -> Vec<f32> {
        let n = (seconds * OPUS_SAMPLE_RATE as f32) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / OPUS_SAMPLE_RATE as f32;
                let envelope = (2.0 * std::f32::consts::PI * 3.0 * t).sin().max(0.0);
                let tone: f32 = [140.0, 280.0, 420.0, 900.0, 2400.0]
                    .iter()
                    .enumerate()
                    .map(|(k, hz)| (2.0 * std::f32::consts::PI * hz * t).sin() / (k as f32 + 1.0))
                    .sum();
                0.3 * envelope * tone
            })
            .collect()
    }

    /// Frequency of a pure tone from its zero crossings.
    fn dominant_hz(samples: &[f32]) -> f32 {
        let crossings = samples
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count();
        crossings as f32 / 2.0 / (samples.len() as f32 / OPUS_SAMPLE_RATE as f32)
    }

    #[test]
    fn writes_an_ogg_opus_stream_for_16_khz_mono() {
        let bytes = encode_opus(&sine(440.0, 1.0, 0.5)).unwrap();
        assert_eq!(&bytes[..4], b"OggS", "starts with an Ogg page");
        let head = bytes
            .windows(8)
            .position(|w| w == b"OpusHead")
            .expect("has an OpusHead packet");
        assert_eq!(bytes[head + 9], 1, "one channel");
        let input_rate = u32::from_le_bytes([
            bytes[head + 12],
            bytes[head + 13],
            bytes[head + 14],
            bytes[head + 15],
        ]);
        assert_eq!(input_rate, OPUS_SAMPLE_RATE);
        assert!(
            bytes.windows(8).any(|w| w == b"OpusTags"),
            "has an OpusTags packet"
        );
    }

    #[test]
    fn decoding_returns_exactly_the_encoded_length() {
        for seconds in [0.013, 0.02, 1.0, 3.337] {
            let samples = speech_like(seconds);
            let decoded = decode_opus(&encode_opus(&samples).unwrap()).unwrap();
            assert_eq!(decoded.len(), samples.len(), "{seconds} s");
        }
    }

    #[test]
    fn an_empty_recording_stays_empty() {
        let decoded = decode_opus(&encode_opus(&[]).unwrap()).unwrap();
        assert!(decoded.is_empty());
    }

    #[test]
    fn keeps_the_pitch_so_the_sample_rate_is_right() {
        let decoded = decode_opus(&encode_opus(&sine(440.0, 2.0, 0.5)).unwrap()).unwrap();
        // Skip the codec's first frames while it settles.
        let hz = dominant_hz(&decoded[1600..]);
        assert!((hz - 440.0).abs() < 440.0 * 0.02, "decoded at {hz} Hz");
    }

    #[test]
    fn does_not_clip_loud_input() {
        let decoded = decode_opus(&encode_opus(&sine(300.0, 1.0, 0.99)).unwrap()).unwrap();
        let peak = decoded.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak <= 1.0, "peak {peak}");
        assert!(peak > 0.8, "still loud: peak {peak}");
    }

    #[test]
    fn is_far_smaller_than_wav() {
        let samples = speech_like(10.0);
        let wav_bytes = samples.len() * 2;
        let opus_bytes = encode_opus(&samples).unwrap().len();
        // 24 kbit/s is 3 kB per second; WAV is 32 kB per second.
        assert!(
            opus_bytes * 8 < wav_bytes,
            "{opus_bytes} bytes of Opus for {wav_bytes} bytes of WAV"
        );
    }

    #[test]
    fn rejects_data_that_is_not_ogg_opus() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("a.wav");
        super::super::utils::save_wav_file(&wav, &sine(440.0, 0.5, 0.5)).unwrap();
        assert!(decode_opus(&std::fs::read(&wav).unwrap()).is_err());
        assert!(decode_opus(b"OggS but nothing else").is_err());
    }

    #[test]
    fn saves_and_reads_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.opus");
        let samples = speech_like(1.5);
        save_opus_file(&path, &samples).unwrap();
        assert_eq!(read_opus_samples(&path).unwrap().len(), samples.len());
    }
}
