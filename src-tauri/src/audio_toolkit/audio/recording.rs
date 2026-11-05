//! The files History keeps: new recordings are Ogg Opus, older ones may
//! still be WAV until they are converted. Everything that reads a recording
//! goes through here, so both formats keep working.

use anyhow::{anyhow, bail, Result};
use std::path::{Path, PathBuf};

use super::opus_file::{read_opus_samples, save_opus_file};
use super::utils::{read_wav_samples, save_wav_file};

/// Extension of new recordings.
pub const RECORDING_EXTENSION: &str = "opus";

/// File name of a new recording with the given stem (`handy-<time>`).
pub fn recording_file_name(stem: &str) -> String {
    format!("{stem}.{RECORDING_EXTENSION}")
}

fn is_opus(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(RECORDING_EXTENSION))
}

/// Write a new recording.
pub fn save_recording<P: AsRef<Path>>(path: P, samples: &[f32]) -> Result<()> {
    save_opus_file(path, samples)
}

/// Read a recording in either format, as 16 kHz mono samples.
pub fn read_recording_samples<P: AsRef<Path>>(path: P) -> Result<Vec<f32>> {
    let path = path.as_ref();
    if is_opus(path) {
        read_opus_samples(path)
    } else {
        read_wav_samples(path)
    }
}

/// Check a written recording by reading it back.
pub fn verify_recording<P: AsRef<Path>>(path: P, expected_samples: usize) -> Result<()> {
    let actual = read_recording_samples(path)?.len();
    if actual != expected_samples {
        bail!(
            "Recording sample count mismatch: expected {}, got {}",
            expected_samples,
            actual
        );
    }
    Ok(())
}

/// A WAV the history player can play: the recording itself when it is a
/// WAV, otherwise a decoded copy in `cache_dir`. Copies older than a minute
/// are removed on the way: the player reads a copy right after asking.
pub fn write_playback_wav(recording: &Path, cache_dir: &Path) -> Result<PathBuf> {
    if !is_opus(recording) {
        return Ok(recording.to_path_buf());
    }
    std::fs::create_dir_all(cache_dir)?;
    remove_stale_playback_copies(cache_dir, std::time::Duration::from_secs(60));

    let stem = recording
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow!("Recording has no file name: {:?}", recording))?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let wav = cache_dir.join(format!("{stem}-{nanos}.wav"));
    save_wav_file(&wav, &read_opus_samples(recording)?)?;
    Ok(wav)
}

fn remove_stale_playback_copies(cache_dir: &Path, max_age: std::time::Duration) {
    let Ok(entries) = std::fs::read_dir(cache_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > max_age);
        if stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::opus_file::save_opus_file;
    use super::*;

    fn tone(n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| 0.4 * (i as f32 * 0.07).sin())
            .collect::<Vec<_>>()
    }

    #[test]
    fn new_recordings_are_opus_files() {
        assert_eq!(recording_file_name("handy-1"), "handy-1.opus");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(recording_file_name("handy-1"));
        save_recording(&path, &tone(16_000)).unwrap();
        assert_eq!(&std::fs::read(&path).unwrap()[..4], b"OggS");
    }

    #[test]
    fn reads_opus_and_wav_recordings() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("old.wav");
        let opus = dir.path().join("new.opus");
        save_wav_file(&wav, &tone(8_000)).unwrap();
        save_opus_file(&opus, &tone(12_345)).unwrap();
        assert_eq!(read_recording_samples(&wav).unwrap().len(), 8_000);
        assert_eq!(read_recording_samples(&opus).unwrap().len(), 12_345);
    }

    #[test]
    fn verifying_checks_the_length() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(recording_file_name("handy-2"));
        save_recording(&path, &tone(4_000)).unwrap();
        assert!(verify_recording(&path, 4_000).is_ok());
        assert!(verify_recording(&path, 3_999).is_err());
    }

    #[test]
    fn plays_an_opus_recording_as_a_decoded_wav() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("playback");
        let opus = dir.path().join("handy-3.opus");
        save_opus_file(&opus, &tone(10_000)).unwrap();

        let wav = write_playback_wav(&opus, &cache).unwrap();
        assert!(wav.starts_with(&cache), "{wav:?} is in the cache");
        assert_eq!(wav.extension().and_then(|e| e.to_str()), Some("wav"));
        assert_eq!(read_wav_samples(&wav).unwrap().len(), 10_000);
    }

    #[test]
    fn plays_a_wav_recording_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("handy-4.wav");
        save_wav_file(&wav, &tone(1_000)).unwrap();
        let played = write_playback_wav(&wav, &dir.path().join("playback")).unwrap();
        assert_eq!(played, wav);
    }
}
