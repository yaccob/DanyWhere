use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone, Default)]
#[command(name = "danywhere", about = "DanyWhere - Speech to Text")]
pub struct CliArgs {
    /// Start with the main window hidden
    #[arg(long)]
    pub start_hidden: bool,

    /// Disable the system tray icon
    #[arg(long)]
    pub no_tray: bool,

    /// Toggle transcription on/off (sent to running instance)
    #[arg(long)]
    pub toggle_transcription: bool,

    /// Toggle transcription with post-processing on/off (sent to running instance)
    #[arg(long)]
    pub toggle_post_process: bool,

    /// Toggle transcription with post-processing and context from earlier
    /// dictations in the same window (sent to running instance)
    #[arg(long)]
    pub toggle_post_process_with_context: bool,

    /// Cancel the current operation (sent to running instance)
    #[arg(long)]
    pub cancel: bool,

    /// Enable debug mode with verbose logging
    #[arg(long)]
    pub debug: bool,

    /// Transcribe this WAV (16 kHz mono) headlessly and exit. Runs the same
    /// batch transcription path as the app — no mic, no silence removal, no
    /// download (the model must already be installed). Long audio is still
    /// chunked at pauses, as in the app.
    #[arg(short = 'f', long, value_name = "WAV")]
    pub transcribe_file: Option<PathBuf>,

    /// Model id to load for --transcribe-file (default: the selected model).
    #[arg(long)]
    pub model: Option<String>,

    /// Hard-select the compute device for --transcribe-file by its registry
    /// index (see --list-devices). Omit to use the persisted accelerator
    /// setting. transcribe-cpp (whisper-family) models only.
    #[arg(long, value_name = "N")]
    pub device_index: Option<usize>,

    /// List the transcribe-cpp compute devices (with indices) and exit.
    #[arg(long)]
    pub list_devices: bool,

    /// List the available models (with ids) and exit. Pass an id to --model.
    /// Honors --json for machine-readable output.
    #[arg(long)]
    pub list_models: bool,

    /// Repeat the transcription N times (best_ms reports the fastest run).
    #[arg(long, value_name = "N")]
    pub repeat: Option<usize>,

    /// Emit --transcribe-file results as JSON.
    #[arg(long)]
    pub json: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// The executable name the bundles ship: `mainBinaryName` from
    /// tauri.conf.json, or the crate's binary name when it is not set.
    fn shipped_binary_name() -> String {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
            .expect("tauri.conf.json is valid JSON");
        conf.get("mainBinaryName")
            .and_then(|name| name.as_str())
            .unwrap_or(env!("CARGO_PKG_NAME"))
            .to_string()
    }

    #[test]
    fn shipped_binary_has_danywheres_own_name() {
        // Named after the crate, DanyWhere's process is "handy": Activity
        // Monitor, pkill and UI scripting can't tell it apart from Handy.
        assert_eq!(shipped_binary_name(), "danywhere");
    }

    #[test]
    fn cli_names_the_shipped_binary() {
        assert_eq!(CliArgs::command().get_name(), shipped_binary_name());
    }
}
