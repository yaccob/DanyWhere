use crate::utils;
use log::{debug, warn};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use specta::Type;
use std::collections::HashMap;
use std::fmt;
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

pub const APPLE_INTELLIGENCE_PROVIDER_ID: &str = "apple_intelligence";
pub const APPLE_INTELLIGENCE_DEFAULT_MODEL_ID: &str = "Apple Intelligence";

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

// Custom deserializer to handle both old numeric format (1-5) and new string format ("trace", "debug", etc.)
impl<'de> Deserialize<'de> for LogLevel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct LogLevelVisitor;

        impl<'de> Visitor<'de> for LogLevelVisitor {
            type Value = LogLevel;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string or integer representing log level")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<LogLevel, E> {
                match value.to_lowercase().as_str() {
                    "trace" => Ok(LogLevel::Trace),
                    "debug" => Ok(LogLevel::Debug),
                    "info" => Ok(LogLevel::Info),
                    "warn" => Ok(LogLevel::Warn),
                    "error" => Ok(LogLevel::Error),
                    _ => Err(E::unknown_variant(
                        value,
                        &["trace", "debug", "info", "warn", "error"],
                    )),
                }
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<LogLevel, E> {
                match value {
                    1 => Ok(LogLevel::Trace),
                    2 => Ok(LogLevel::Debug),
                    3 => Ok(LogLevel::Info),
                    4 => Ok(LogLevel::Warn),
                    5 => Ok(LogLevel::Error),
                    _ => Err(E::invalid_value(de::Unexpected::Unsigned(value), &"1-5")),
                }
            }
        }

        deserializer.deserialize_any(LogLevelVisitor)
    }
}

impl From<LogLevel> for tauri_plugin_log::LogLevel {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Trace => tauri_plugin_log::LogLevel::Trace,
            LogLevel::Debug => tauri_plugin_log::LogLevel::Debug,
            LogLevel::Info => tauri_plugin_log::LogLevel::Info,
            LogLevel::Warn => tauri_plugin_log::LogLevel::Warn,
            LogLevel::Error => tauri_plugin_log::LogLevel::Error,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct ShortcutBinding {
    pub id: String,
    pub name: String,
    pub description: String,
    pub default_binding: String,
    pub current_binding: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type)]
pub struct LLMPrompt {
    pub id: String,
    pub name: String,
    pub prompt: String,
}

/// Request format a post-processing endpoint speaks. Built-in providers have a
/// fixed format; only the custom provider lets the user change it.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq, Type)]
#[serde(rename_all = "lowercase")]
pub enum PostProcessApiFormat {
    /// OpenAI-compatible Chat Completions (`/chat/completions`)
    #[default]
    OpenAi,
    /// Anthropic Messages (`/messages`)
    Anthropic,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type)]
pub struct PostProcessProvider {
    pub id: String,
    pub label: String,
    pub base_url: String,
    #[serde(default)]
    pub allow_base_url_edit: bool,
    #[serde(default)]
    pub models_endpoint: Option<String>,
    #[serde(default)]
    pub supports_structured_output: bool,
    #[serde(default)]
    pub api_format: PostProcessApiFormat,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "lowercase")]
pub enum OverlayPosition {
    Top,
    // `none` is retired: overlay visibility is owned by `OverlayStyle` now. The
    // alias keeps legacy stores (`"overlay_position": "none"`) deserializing
    // instead of failing the whole load; the one-time overlay migration reads the
    // raw stored string to recover the old "hidden" intent as `OverlayStyle::None`.
    #[serde(alias = "none")]
    Bottom,
}

/// Which recording overlay to display. `Minimal` and `Live` share one base
/// (the pill); `Live` grows into the panel that shows live transcription text.
/// `None` hides the overlay entirely. Decoupled from whether the model runs in
/// streaming mode (that is driven purely by model capability).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "lowercase")]
pub enum OverlayStyle {
    None,
    Minimal,
    Live,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModelUnloadTimeout {
    Never,
    Immediately,
    Min2,
    #[default]
    Min5,
    Min10,
    Min15,
    Hour1,
    Sec15, // Debug mode only
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum PasteMethod {
    CtrlV,
    Direct,
    None,
    ShiftInsert,
    CtrlShiftV,
    ExternalScript,
}

/// How the transcribe shortcut's key events drive a recording.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutActivation {
    /// Press to start, press again to stop.
    Toggle,
    /// Hold to record, release to stop.
    PushToTalk,
    /// Hold to record and release to stop, or tap to keep recording until the
    /// next press. Which one it was is decided by how long the key was held
    /// (`hold_threshold_ms`).
    #[default]
    HoldOrToggle,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardHandling {
    #[default]
    DontModify,
    CopyToClipboard,
}

/// DanyWhere: what an "end dictation" service does when it is called while
/// no dictation is recording (e.g. DanyWhere wasn't running and macOS
/// started it to answer the service). It never inserts an earlier dictation
/// (user, 2026-11-12).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum DictationServiceWhenIdle {
    /// Start a recording for the shortcut, so its next press ends and
    /// inserts it ("noch besser", the user's choice as the default).
    #[default]
    StartRecording,
    DoNothing,
}

/// Script applied to Mandarin and Cantonese output. Traditional carries its region
/// (Taiwan vs Hong Kong forms, e.g. 裡/裏) rather than guessing it.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChineseScript {
    /// Keep whatever script the model produced.
    #[default]
    AsTranscribed,
    Simplified,
    /// Traditional, Taiwan forms. Also reads legacy `traditional` (migrated on load).
    #[serde(alias = "traditional")]
    TraditionalTw,
    /// Traditional, in Hong Kong's standard forms.
    TraditionalHk,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum AutoSubmitKey {
    #[default]
    Enter,
    CtrlEnter,
    CmdEnter,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum RecordingRetentionPeriod {
    Never,
    PreserveLimit,
    Days3,
    Weeks2,
    Months3,
}

/// How much space History's recordings may take. Over it, the oldest
/// recordings that are not starred are deleted; their transcripts stay.
/// Replaces [`RecordingRetentionPeriod`] and `history_limit`, which are kept
/// only so older stores still parse and migrate.
///
/// The names are spelled out because serde's and specta's `snake_case`
/// disagree on digits (`mb100` vs `mb_100`), and the frontend sends what
/// specta generates.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
pub enum RecordingStorageLimit {
    #[serde(rename = "mb_100")]
    Mb100,
    #[default]
    #[serde(rename = "mb_500")]
    Mb500,
    #[serde(rename = "gb_1")]
    Gb1,
    #[serde(rename = "gb_5")]
    Gb5,
    #[serde(rename = "unlimited")]
    Unlimited,
}

impl RecordingStorageLimit {
    /// The limit in bytes (decimal units, as Finder and Explorer count them);
    /// `None` when unlimited.
    pub fn bytes(self) -> Option<u64> {
        const MB: u64 = 1_000_000;
        match self {
            Self::Mb100 => Some(100 * MB),
            Self::Mb500 => Some(500 * MB),
            Self::Gb1 => Some(1_000 * MB),
            Self::Gb5 => Some(5_000 * MB),
            Self::Unlimited => None,
        }
    }

    /// The limit an upgrading user gets for the retention they had chosen:
    /// "never delete" stays unlimited, every other choice gets the default.
    pub fn for_retention(period: RecordingRetentionPeriod) -> Self {
        match period {
            RecordingRetentionPeriod::Never => Self::Unlimited,
            _ => Self::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum KeyboardImplementation {
    Tauri,
    HandyKeys,
    /// No built-in global shortcuts; trigger Handy via CLI flags or signals
    /// (e.g. from a Wayland compositor keybinding).
    None,
}

impl Default for KeyboardImplementation {
    fn default() -> Self {
        #[cfg(target_os = "linux")]
        return KeyboardImplementation::Tauri;
        #[cfg(not(target_os = "linux"))]
        return KeyboardImplementation::HandyKeys;
    }
}

impl Default for PasteMethod {
    fn default() -> Self {
        // Default to CtrlV for macOS and Windows. On Linux, Shift+Insert pastes in
        // terminals and GUI apps alike (Handy also puts the text on the primary
        // selection, which terminals read), and unlike Direct typing it doesn't
        // depend on the keyboard layout.
        #[cfg(target_os = "linux")]
        return PasteMethod::ShiftInsert;
        #[cfg(not(target_os = "linux"))]
        return PasteMethod::CtrlV;
    }
}

impl ModelUnloadTimeout {
    pub fn to_minutes(self) -> Option<u64> {
        match self {
            ModelUnloadTimeout::Never => None,
            ModelUnloadTimeout::Immediately => Some(0), // Special case for immediate unloading
            ModelUnloadTimeout::Min2 => Some(2),
            ModelUnloadTimeout::Min5 => Some(5),
            ModelUnloadTimeout::Min10 => Some(10),
            ModelUnloadTimeout::Min15 => Some(15),
            ModelUnloadTimeout::Hour1 => Some(60),
            ModelUnloadTimeout::Sec15 => Some(0), // Special case for debug - handled separately
        }
    }

    pub fn to_seconds(self) -> Option<u64> {
        match self {
            ModelUnloadTimeout::Never => None,
            ModelUnloadTimeout::Immediately => Some(0), // Special case for immediate unloading
            ModelUnloadTimeout::Sec15 => Some(15),
            _ => self.to_minutes().map(|m| m * 60),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum SoundTheme {
    Marimba,
    Pop,
    Custom,
}

impl SoundTheme {
    fn as_str(&self) -> &'static str {
        match self {
            SoundTheme::Marimba => "marimba",
            SoundTheme::Pop => "pop",
            SoundTheme::Custom => "custom",
        }
    }

    pub fn to_start_path(self) -> String {
        format!("resources/{}_start.wav", self.as_str())
    }

    pub fn to_stop_path(self) -> String {
        format!("resources/{}_stop.wav", self.as_str())
    }

    /// DanyWhere: the sound for a transcript left on the clipboard.
    pub fn to_clipboard_path(self) -> String {
        format!("resources/{}_clipboard.wav", self.as_str())
    }
}

/// UI appearance mode. `System` follows the OS `prefers-color-scheme`; `Light`
/// and `Dark` force one of the two palettes Handy already ships.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum TypingTool {
    #[default]
    Auto,
    Wtype,
    Kwtype,
    Dotool,
    Ydotool,
    Xdotool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum TranscribeAcceleratorSetting {
    #[default]
    Auto,
    Cpu,
    Gpu,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum VadBackend {
    #[default]
    Silero,
    Earshot,
}

/// When Handy's spoken-language detector picks the language for Auto.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum LanguageDetectionMode {
    /// Only for models that can't detect the language themselves.
    #[default]
    WhenNeeded,
    /// For every model except streaming ones, restricted to the languages
    /// the person speaks.
    Always,
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub(crate) struct SecretMap(HashMap<String, String>);

impl fmt::Debug for SecretMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let redacted: HashMap<&String, &str> = self
            .0
            .iter()
            .map(|(k, v)| (k, if v.is_empty() { "" } else { "[REDACTED]" }))
            .collect();
        redacted.fmt(f)
    }
}

impl std::ops::Deref for SecretMap {
    type Target = HashMap<String, String>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for SecretMap {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/* still handy for composing the initial JSON in the store ------------- */
/// The container-level `serde(default)` (backed by the `Default` impl below)
/// guarantees every field — including ones added in the future — falls back to
/// its `get_default_settings()` value when missing from a stored settings
/// object, so a partial store can never fail the whole load (#1619).
/// Field-level defaults below take precedence where present.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
#[serde(default)]
pub struct AppSettings {
    /// Internal settings schema marker for one-time migrations. Fresh installs
    /// start at the current version; existing stores missing this key are
    /// treated as version 0 and migrated forward.
    #[serde(default = "default_settings_schema_version")]
    pub settings_schema_version: u32,
    /// Defaults to empty on partial stores; the load path merges in the
    /// default bindings for any missing keys before the settings are used.
    #[serde(default)]
    pub bindings: HashMap<String, ShortcutBinding>,
    /// Replaces the pre-0.10 `push_to_talk` bool; stores missing this key are
    /// migrated from it in `apply_settings_migrations`.
    #[serde(default)]
    pub shortcut_activation: ShortcutActivation,
    /// Hold-or-toggle only: a press held at least this long is push-to-talk,
    /// anything shorter is a tap that locks recording on.
    #[serde(default = "default_hold_threshold_ms")]
    pub hold_threshold_ms: u64,
    #[serde(default)]
    pub audio_feedback: bool,
    #[serde(default = "default_audio_feedback_volume")]
    pub audio_feedback_volume: f32,
    #[serde(default = "default_sound_theme")]
    pub sound_theme: SoundTheme,
    /// DanyWhere: a sound once a dictation is completely done, post-processing
    /// included, however it was delivered (user, 2026-11-15). Plays only
    /// while `audio_feedback` is on. See `delivery::sound_after_dictation`.
    #[serde(default = "default_done_sound")]
    pub done_sound: bool,
    #[serde(default = "default_start_hidden")]
    pub start_hidden: bool,
    #[serde(default = "default_autostart_enabled")]
    pub autostart_enabled: bool,
    #[serde(default = "default_update_checks_enabled")]
    pub update_checks_enabled: bool,
    #[serde(default = "default_show_whats_new_on_update")]
    pub show_whats_new_on_update: bool,
    /// The app version whose What's New the user has already seen. Fresh installs
    /// default to the current version (nothing is "new" to them). Existing users
    /// upgrading from before this key existed are blanked by the migration so they
    /// see the current release's notes — see `apply_settings_migrations`.
    #[serde(default = "default_whats_new_last_seen_version")]
    pub whats_new_last_seen_version: String,
    #[serde(default = "default_model")]
    pub selected_model: String,
    #[serde(default)]
    pub onboarding_completed: bool,
    /// The user chose to run without the macOS Accessibility permission
    /// (e.g. it is blocked by device management). Startup then no longer
    /// sends them back to the permission screen. See
    /// `shortcut::apply_no_accessibility_mode`.
    #[serde(default)]
    pub accessibility_skipped: bool,
    /// DanyWhere gave the "Insert Last Dictation" service its default
    /// shortcut once (or found one set); it never sets it again, so a change
    /// in System Settings stays. See `insert_service`.
    #[serde(default)]
    pub insert_service_shortcut_set: bool,
    /// DanyWhere: without Accessibility, the dictation shortcut's second press
    /// ends the dictation and inserts it once it's ready, through the "end
    /// dictation" services (user, 2026-11-12). See `dictation_key_inserts`.
    #[serde(default = "default_dictation_key_inserts")]
    pub dictation_key_inserts: bool,
    /// DanyWhere: what the dictation shortcuts' services do when no dictation
    /// is recording. See `DictationServiceWhenIdle`.
    #[serde(default)]
    pub dictation_service_when_idle: DictationServiceWhenIdle,
    /// DanyWhere: when DanyWhere last gave the "end dictation" services a
    /// shortcut, in seconds since the Unix epoch. Apps launched before it
    /// don't know the shortcut until they restart. See `dictation_key_inserts`.
    #[serde(default)]
    pub dictation_service_shortcuts_written_at: Option<u64>,
    #[serde(default = "default_always_on_microphone")]
    pub always_on_microphone: bool,
    #[serde(default)]
    pub selected_microphone: Option<String>,
    /// Which input channel to use on the selected microphone device.
    /// None means "average all channels" (original behavior).
    #[serde(default)]
    pub selected_channel: Option<u16>,
    #[serde(default)]
    pub clamshell_microphone: Option<String>,
    #[serde(default)]
    pub selected_output_device: Option<String>,
    #[serde(default = "default_translate_to_english")]
    pub translate_to_english: bool,
    #[serde(default = "default_selected_language")]
    pub selected_language: String,
    /// Every language the person dictates in (codes, e.g. ["en", "zh"]),
    /// picked during onboarding. Empty until then.
    #[serde(default)]
    pub spoken_languages: Vec<String>,
    #[serde(default = "default_overlay_position")]
    pub overlay_position: OverlayPosition,
    #[serde(default = "default_debug_mode")]
    pub debug_mode: bool,
    #[serde(default = "default_log_level")]
    pub log_level: LogLevel,
    /// The word list every dictation gets (see `word_lists`); a profile
    /// value (`profiles`), the same in every profile until one differs.
    #[serde(default)]
    pub custom_words: Vec<String>,
    /// Named lists that can be switched on in addition to the global one.
    #[serde(default)]
    pub word_lists: Vec<crate::word_lists::WordList>,
    /// The additional list in use; `None`: only the global list.
    #[serde(default)]
    pub active_word_list_id: Option<String>,
    /// DanyWhere: earlier dictations from the same window as context for
    /// post-processing (`dictation_context`).
    #[serde(default)]
    pub dictation_context: crate::dictation_context::DictationContextSettings,
    #[serde(default)]
    pub model_unload_timeout: ModelUnloadTimeout,
    #[serde(default = "default_word_correction_threshold")]
    pub word_correction_threshold: f64,
    #[serde(default = "default_history_limit")]
    pub history_limit: usize,
    #[serde(default = "default_recording_retention_period")]
    pub recording_retention_period: RecordingRetentionPeriod,
    /// Migrated from `recording_retention_period` in `apply_settings_migrations`.
    #[serde(default)]
    pub recording_storage_limit: RecordingStorageLimit,
    #[serde(default)]
    pub paste_method: PasteMethod,
    #[serde(default)]
    pub clipboard_handling: ClipboardHandling,
    #[serde(default = "default_auto_submit")]
    pub auto_submit: bool,
    #[serde(default)]
    pub auto_submit_key: AutoSubmitKey,
    #[serde(default = "default_post_process_enabled")]
    pub post_process_enabled: bool,
    #[serde(default = "default_post_process_provider_id")]
    pub post_process_provider_id: String,
    #[serde(default = "default_post_process_providers")]
    pub post_process_providers: Vec<PostProcessProvider>,
    #[serde(default = "default_post_process_api_keys")]
    pub post_process_api_keys: SecretMap,
    #[serde(default = "default_post_process_models")]
    pub post_process_models: HashMap<String, String>,
    /// DanyWhere: the effort per provider (`post_process_effort`); none
    /// stored = the model's default.
    #[serde(default)]
    pub post_process_efforts: HashMap<String, String>,
    #[serde(default = "default_post_process_prompts")]
    pub post_process_prompts: Vec<LLMPrompt>,
    #[serde(default)]
    pub post_process_selected_prompt_id: Option<String>,
    /// DanyWhere: time limit for the whole post-processing step, retries
    /// included, in seconds (Handy: a fixed 60 s; see `llm_timeouts`).
    #[serde(default = "crate::llm_timeouts::default_post_process_timeout_secs")]
    pub post_process_timeout_secs: u64,
    /// DanyWhere: time limit for a provider's model list, in seconds (Handy:
    /// a fixed 15 s).
    #[serde(default = "crate::llm_timeouts::default_model_list_timeout_secs")]
    pub model_list_timeout_secs: u64,
    #[serde(default)]
    pub mute_while_recording: bool,
    #[serde(default)]
    pub append_trailing_space: bool,
    /// DanyWhere: what every dictation gets appended, text and key presses at
    /// the end (`append_text`). Replaces `append_trailing_space` and
    /// `auto_submit`, which stay only for Handy merges and the migration.
    #[serde(default)]
    pub append_text: String,
    #[serde(default = "default_app_language")]
    pub app_language: String,
    #[serde(default = "default_theme")]
    pub theme: Theme,
    #[serde(default)]
    pub experimental_enabled: bool,
    #[serde(default)]
    pub lazy_stream_close: bool,
    /// Experimental, macOS only: move a Bluetooth headset (AirPods) to this
    /// Mac while recording, so its microphone does not return silence while
    /// another Apple device is playing through it.
    #[serde(default)]
    pub airpods_mode: bool,
    #[serde(default)]
    pub keyboard_implementation: KeyboardImplementation,
    #[serde(default = "default_show_tray_icon")]
    pub show_tray_icon: bool,
    #[serde(default = "default_paste_delay_ms")]
    pub paste_delay_ms: u64,
    /// How long the "on the clipboard" notice stays visible, in milliseconds;
    /// 0 switches it off. Independent of the recording overlay style.
    #[serde(default = "default_clipboard_notice_ms")]
    pub clipboard_notice_ms: u64,
    #[serde(default = "default_paste_delay_after_ms")]
    pub paste_delay_after_ms: u64,
    /// Debug-gated ("beta") receipt-sequenced paste: restore the clipboard only
    /// after the target app actually reads the transcript, instead of after a
    /// fixed delay. See `paste_tx`. macOS and Windows only.
    #[serde(default)]
    pub reliable_paste: bool,
    #[serde(default = "default_typing_tool")]
    pub typing_tool: TypingTool,
    #[serde(default)]
    pub external_script_path: Option<String>,
    #[serde(default = "default_filler_word_removal_enabled")]
    pub filler_word_removal_enabled: bool,
    #[serde(default)]
    pub custom_filler_words: Option<Vec<String>>,
    /// Fresh installs default from the OS locale; existing stores are migrated
    /// in `apply_settings_migrations`.
    #[serde(default)]
    pub chinese_script: ChineseScript,
    #[serde(default)]
    pub transcribe_accelerator: TranscribeAcceleratorSetting,
    /// Stable transcribe.cpp device selector. This is derived from the backend's
    /// `device_id` when available (or its name for backends such as Metal),
    /// never from the process-local device registry index.
    #[serde(
        default = "default_transcribe_gpu_device",
        deserialize_with = "deserialize_transcribe_gpu_device"
    )]
    pub transcribe_gpu_device: Option<String>,
    #[serde(default)]
    pub extra_recording_buffer_ms: u64,
    #[serde(default = "default_vad_enabled")]
    pub vad_enabled: bool,
    /// Experimental detector implementation. Silero remains the stable default.
    #[serde(default)]
    pub vad_backend: VadBackend,
    /// Decode long audio in chunks cut at pauses, for models whose accuracy
    /// degrades on long audio. Independent of `vad_enabled`, which only
    /// decides whether silence is dropped from recordings.
    #[serde(default = "default_long_form_chunking")]
    pub long_form_chunking: bool,
    #[serde(default)]
    pub language_detection: LanguageDetectionMode,
    /// Seconds of speech the language detector hears (debug). Read through
    /// [`AppSettings::language_detection_window_samples`], which clamps it.
    #[serde(default = "default_language_detection_window_secs")]
    pub language_detection_window_secs: u32,
    /// Which recording overlay to show: None / Minimal / Live. Streaming mode is
    /// not gated on this — that follows model capability. Migrated from the old
    /// `overlay_position` (position `none` → style `None`).
    #[serde(default = "default_overlay_style")]
    pub overlay_style: OverlayStyle,
    /// DanyWhere: the profiles (`profiles`). The flat fields above hold the
    /// active profile's values.
    #[serde(default)]
    pub profiles: Vec<crate::profiles::Profile>,
    #[serde(default)]
    pub active_profile_id: String,
    /// DanyWhere: the defaults ("Grundeinstellungen"): the values every
    /// profile's coupled fields follow and that apply while no profile is in
    /// use (`profile_layers`). `None` only in a store from before them, which
    /// `profiles::ensure_profiles` migrates; read them through
    /// `profiles::default_values`.
    #[serde(default)]
    pub default_values: Option<crate::profiles::ProfileValues>,
    /// DanyWhere: the active profile's own word list, which always applies
    /// in it, before `custom_words`.
    #[serde(default)]
    pub profile_words: Vec<String>,
    /// DanyWhere: whether an application's profile is used for dictations
    /// into it. Not used yet.
    #[serde(default = "default_profile_auto_by_app")]
    pub profile_auto_by_app: bool,
    /// DanyWhere: the active profile's switch "Send Context": a dictation
    /// with post-processing sends earlier dictations of the same window as
    /// context (`send_context`).
    #[serde(default)]
    pub send_context: bool,
    /// DanyWhere: the height in pixels a History text box may grow to before
    /// it scrolls, as dragged at its grip, for the newest entry and for the
    /// others; `None` = the default lines (`history_box`).
    #[serde(default)]
    pub history_box_height_newest: Option<u32>,
    #[serde(default)]
    pub history_box_height_other: Option<u32>,
}

fn default_profile_auto_by_app() -> bool {
    true
}

fn default_model() -> String {
    "".to_string()
}

const CURRENT_SETTINGS_SCHEMA_VERSION: u32 = 3;

fn default_settings_schema_version() -> u32 {
    CURRENT_SETTINGS_SCHEMA_VERSION
}

fn default_hold_threshold_ms() -> u64 {
    300
}

fn default_always_on_microphone() -> bool {
    false
}

fn default_dictation_key_inserts() -> bool {
    true
}

fn default_translate_to_english() -> bool {
    false
}

fn default_start_hidden() -> bool {
    false
}

fn default_autostart_enabled() -> bool {
    false
}

fn default_update_checks_enabled() -> bool {
    true
}

fn default_show_whats_new_on_update() -> bool {
    true
}

fn default_whats_new_last_seen_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn default_selected_language() -> String {
    "auto".to_string()
}

fn default_overlay_position() -> OverlayPosition {
    // Position only matters when the overlay is shown; whether it shows at all is
    // `overlay_style` (Linux defaults that to None). So a single default suffices.
    OverlayPosition::Bottom
}

fn default_overlay_style() -> OverlayStyle {
    // Linux hides the overlay by default; other platforms show the live overlay.
    // Position is independent and only selects top vs. bottom placement.
    #[cfg(target_os = "linux")]
    return OverlayStyle::None;
    #[cfg(not(target_os = "linux"))]
    return OverlayStyle::Live;
}

fn default_vad_enabled() -> bool {
    true
}

fn default_long_form_chunking() -> bool {
    true
}

fn default_language_detection_window_secs() -> u32 {
    5
}

fn default_filler_word_removal_enabled() -> bool {
    true
}

fn default_chinese_script() -> ChineseScript {
    crate::system_languages::suggested_chinese_script(
        crate::system_languages::system_languages()
            .iter()
            .map(String::as_str),
    )
    .unwrap_or_default()
}

fn default_debug_mode() -> bool {
    false
}

fn default_log_level() -> LogLevel {
    LogLevel::Debug
}

fn default_word_correction_threshold() -> f64 {
    0.18
}

fn default_paste_delay_ms() -> u64 {
    60
}

fn default_clipboard_notice_ms() -> u64 {
    2500
}

fn default_paste_delay_after_ms() -> u64 {
    // Linux apps fetch the pasted text from Handy only when they handle the
    // key, so restoring the old clipboard too soon pastes the old content
    // (#502). Only new installs get this; existing settings keep their value.
    #[cfg(target_os = "linux")]
    return 500;
    #[cfg(not(target_os = "linux"))]
    return 60;
}

fn default_auto_submit() -> bool {
    false
}

fn default_history_limit() -> usize {
    5
}

fn default_recording_retention_period() -> RecordingRetentionPeriod {
    RecordingRetentionPeriod::PreserveLimit
}

fn default_audio_feedback_volume() -> f32 {
    1.0
}

fn default_sound_theme() -> SoundTheme {
    SoundTheme::Marimba
}

fn default_done_sound() -> bool {
    true
}

fn default_theme() -> Theme {
    Theme::System
}

fn default_post_process_enabled() -> bool {
    false
}

fn default_app_language() -> String {
    tauri_plugin_os::locale()
        .map(|l| l.replace('_', "-"))
        .unwrap_or_else(|| "en".to_string())
}

fn default_show_tray_icon() -> bool {
    true
}

fn default_post_process_provider_id() -> String {
    "openai".to_string()
}

fn default_post_process_providers() -> Vec<PostProcessProvider> {
    let mut providers = vec![
        PostProcessProvider {
            id: "openai".to_string(),
            label: "OpenAI".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            allow_base_url_edit: false,
            models_endpoint: Some("/models".to_string()),
            supports_structured_output: true,
            api_format: PostProcessApiFormat::OpenAi,
        },
        PostProcessProvider {
            id: "zai".to_string(),
            label: "Z.AI".to_string(),
            base_url: "https://api.z.ai/api/paas/v4".to_string(),
            allow_base_url_edit: false,
            models_endpoint: Some("/models".to_string()),
            supports_structured_output: true,
            api_format: PostProcessApiFormat::OpenAi,
        },
        PostProcessProvider {
            id: "openrouter".to_string(),
            label: "OpenRouter".to_string(),
            base_url: "https://openrouter.ai/api/v1".to_string(),
            allow_base_url_edit: false,
            models_endpoint: Some("/models".to_string()),
            supports_structured_output: true,
            api_format: PostProcessApiFormat::OpenAi,
        },
        PostProcessProvider {
            id: "anthropic".to_string(),
            label: "Anthropic".to_string(),
            base_url: "https://api.anthropic.com/v1".to_string(),
            allow_base_url_edit: false,
            models_endpoint: Some("/models".to_string()),
            supports_structured_output: false,
            api_format: PostProcessApiFormat::Anthropic,
        },
        PostProcessProvider {
            id: "groq".to_string(),
            label: "Groq".to_string(),
            base_url: "https://api.groq.com/openai/v1".to_string(),
            allow_base_url_edit: false,
            models_endpoint: Some("/models".to_string()),
            supports_structured_output: false,
            api_format: PostProcessApiFormat::OpenAi,
        },
        PostProcessProvider {
            id: "cerebras".to_string(),
            label: "Cerebras".to_string(),
            base_url: "https://api.cerebras.ai/v1".to_string(),
            allow_base_url_edit: false,
            models_endpoint: Some("/models".to_string()),
            supports_structured_output: true,
            api_format: PostProcessApiFormat::OpenAi,
        },
    ];

    // Note: We always include Apple Intelligence on macOS ARM64 without checking availability
    // at startup. The availability check is deferred to when the user actually tries to use it
    // (in actions.rs). This prevents crashes on macOS 26.x beta where accessing
    // SystemLanguageModel.default during early app initialization causes SIGABRT.
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        providers.push(PostProcessProvider {
            id: APPLE_INTELLIGENCE_PROVIDER_ID.to_string(),
            label: "Apple Intelligence".to_string(),
            base_url: "apple-intelligence://local".to_string(),
            allow_base_url_edit: false,
            models_endpoint: None,
            supports_structured_output: true,
            api_format: PostProcessApiFormat::OpenAi,
        });
    }

    // AWS Bedrock via Mantle (OpenAI-compatible endpoint)
    providers.push(PostProcessProvider {
        id: "bedrock_mantle".to_string(),
        label: "AWS Bedrock (Mantle)".to_string(),
        base_url: "https://bedrock-mantle.us-east-1.api.aws/v1".to_string(),
        allow_base_url_edit: false,
        models_endpoint: Some("/models".to_string()),
        supports_structured_output: true,
        api_format: PostProcessApiFormat::OpenAi,
    });

    // Custom provider always comes last
    providers.push(PostProcessProvider {
        id: "custom".to_string(),
        label: "Custom".to_string(),
        base_url: "http://localhost:11434/v1".to_string(),
        allow_base_url_edit: true,
        models_endpoint: Some("/models".to_string()),
        supports_structured_output: false,
        api_format: PostProcessApiFormat::OpenAi,
    });

    providers
}

fn default_post_process_api_keys() -> SecretMap {
    let mut map = HashMap::new();
    for provider in default_post_process_providers() {
        map.insert(provider.id, String::new());
    }
    SecretMap(map)
}

fn default_model_for_provider(provider_id: &str) -> String {
    if provider_id == APPLE_INTELLIGENCE_PROVIDER_ID {
        return APPLE_INTELLIGENCE_DEFAULT_MODEL_ID.to_string();
    }
    String::new()
}

fn default_post_process_models() -> HashMap<String, String> {
    let mut map = HashMap::new();
    for provider in default_post_process_providers() {
        map.insert(
            provider.id.clone(),
            default_model_for_provider(&provider.id),
        );
    }
    map
}

fn default_post_process_prompts() -> Vec<LLMPrompt> {
    vec![LLMPrompt {
        id: "default_improve_transcriptions".to_string(),
        name: "Improve Transcriptions".to_string(),
        // DanyWhere: Handy's text without what the fixed frame says; Handy's
        // own versions are in `prompt_frame::HANDY_IMPROVE_TRANSCRIPTIONS`.
        prompt: crate::prompt_frame::IMPROVE_TRANSCRIPTIONS.to_string(),
    }]
}

fn default_transcribe_gpu_device() -> Option<String> {
    None // automatic device selection
}

/// Accept the 0.1-era integer registry index long enough for the schema
/// migration to clear it. Device indices are process-local in transcribe.cpp
/// 0.2 and must never be carried across launches.
fn deserialize_transcribe_gpu_device<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    match Option::<serde_json::Value>::deserialize(deserializer)? {
        None => Ok(None),
        Some(serde_json::Value::String(value)) => Ok(Some(value)),
        Some(serde_json::Value::Number(_)) => Ok(None),
        Some(_) => Err(de::Error::custom(
            "transcribe GPU device must be a string, integer, or null",
        )),
    }
}

fn default_typing_tool() -> TypingTool {
    TypingTool::Auto
}

fn ensure_post_process_defaults(settings: &mut AppSettings) -> bool {
    let mut changed = false;
    for provider in default_post_process_providers() {
        // Use match to do a single lookup - either sync existing or add new
        match settings
            .post_process_providers
            .iter_mut()
            .find(|p| p.id == provider.id)
        {
            Some(existing) => {
                // Sync supports_structured_output field for existing providers (migration)
                if existing.supports_structured_output != provider.supports_structured_output {
                    debug!(
                        "Updating supports_structured_output for provider '{}' from {} to {}",
                        provider.id,
                        existing.supports_structured_output,
                        provider.supports_structured_output
                    );
                    existing.supports_structured_output = provider.supports_structured_output;
                    changed = true;
                }

                // Built-in providers have a fixed API format (migration); the
                // custom provider's format is a user choice and is left alone.
                if provider.id != "custom" && existing.api_format != provider.api_format {
                    debug!(
                        "Updating api_format for provider '{}' from {:?} to {:?}",
                        provider.id, existing.api_format, provider.api_format
                    );
                    existing.api_format = provider.api_format;
                    changed = true;
                }
            }
            None => {
                // Provider doesn't exist, add it
                settings.post_process_providers.push(provider.clone());
                changed = true;
            }
        }

        if !settings.post_process_api_keys.contains_key(&provider.id) {
            settings
                .post_process_api_keys
                .insert(provider.id.clone(), String::new());
            changed = true;
        }

        let default_model = default_model_for_provider(&provider.id);
        match settings.post_process_models.get_mut(&provider.id) {
            Some(existing) => {
                if existing.is_empty() && !default_model.is_empty() {
                    *existing = default_model.clone();
                    changed = true;
                }
            }
            None => {
                settings
                    .post_process_models
                    .insert(provider.id.clone(), default_model);
                changed = true;
            }
        }
    }

    // DanyWhere: Handy's untouched default prompt becomes the one without
    // what the fixed frame says (`prompt_frame`).
    if crate::prompt_frame::replace_untouched_handy_default(&mut settings.post_process_prompts) {
        changed = true;
    }

    changed
}

/// DanyWhere: [`ensure_post_process_defaults`] for the flat values and
/// every profile, since the providers, their keys and models are profile
/// values: a provider a new version brings reaches every profile.
fn ensure_post_process_defaults_everywhere(settings: &mut AppSettings) -> bool {
    let flat = ensure_post_process_defaults(settings);
    let profiles = crate::profiles::ensure_in_every_profile(settings, ensure_post_process_defaults);
    flat || profiles
}

pub const SETTINGS_STORE_PATH: &str = "settings_store.json";

pub fn get_default_settings() -> AppSettings {
    // DanyWhere: ⌃ instead of ⌥ on macOS (user, 2026-11-13). Without
    // Accessibility a dictation shortcut's second press inserts through a
    // service, and a service shortcut with ⌥ but no ⌃ or ⌘ never fires (⌥
    // types a character); the others follow so all shortcuts stay alike.
    // ⌃Space is also macOS's "select the previous input source", see ROADMAP.
    #[cfg(target_os = "windows")]
    let default_shortcut = "ctrl+space";
    #[cfg(target_os = "macos")]
    let default_shortcut = "ctrl+space";
    #[cfg(target_os = "linux")]
    let default_shortcut = "ctrl+alt+space";
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let default_shortcut = "alt+space";

    let mut bindings = HashMap::new();
    bindings.insert(
        "transcribe".to_string(),
        ShortcutBinding {
            id: "transcribe".to_string(),
            name: "Transcribe".to_string(),
            description: "Converts your speech into text.".to_string(),
            default_binding: default_shortcut.to_string(),
            current_binding: default_shortcut.to_string(),
        },
    );
    #[cfg(target_os = "windows")]
    let default_post_process_shortcut = "ctrl+shift+space";
    #[cfg(target_os = "macos")]
    let default_post_process_shortcut = "ctrl+shift+space";
    #[cfg(target_os = "linux")]
    let default_post_process_shortcut = "ctrl+alt+shift+space";
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let default_post_process_shortcut = "alt+shift+space";

    bindings.insert(
        "transcribe_with_post_process".to_string(),
        ShortcutBinding {
            id: "transcribe_with_post_process".to_string(),
            name: "Transcribe with Post-Processing".to_string(),
            description: "Converts your speech into text and applies AI post-processing."
                .to_string(),
            default_binding: default_post_process_shortcut.to_string(),
            current_binding: default_post_process_shortcut.to_string(),
        },
    );
    #[cfg(target_os = "macos")]
    let default_paste_last_shortcut = "ctrl+shift+v";
    #[cfg(not(target_os = "macos"))]
    let default_paste_last_shortcut = "alt+shift+v";

    bindings.insert(
        "paste_last_transcript".to_string(),
        ShortcutBinding {
            id: "paste_last_transcript".to_string(),
            name: "Paste Last Transcript".to_string(),
            description: "Pastes the most recent transcript again.".to_string(),
            default_binding: default_paste_last_shortcut.to_string(),
            current_binding: default_paste_last_shortcut.to_string(),
        },
    );
    // DanyWhere: puts the last transcript on the clipboard; during a
    // dictation it ends it onto the clipboard (user, 2026-11-14: "⌃⇧V
    // fügt ein, ⌃⇧C kopiert", with Accessibility and without). C next to V,
    // with the same modifiers as paste last.
    #[cfg(target_os = "macos")]
    let default_copy_last_shortcut = "ctrl+shift+c";
    #[cfg(not(target_os = "macos"))]
    let default_copy_last_shortcut = "alt+shift+c";

    bindings.insert(
        "copy_last_transcript".to_string(),
        ShortcutBinding {
            id: "copy_last_transcript".to_string(),
            name: "Copy Last Transcript".to_string(),
            description: "Puts the most recent transcript on the clipboard.".to_string(),
            default_binding: default_copy_last_shortcut.to_string(),
            current_binding: default_copy_last_shortcut.to_string(),
        },
    );
    // DanyWhere: steps through the additional word lists. L for "list";
    // Control/Alt+Shift like the paste-last shortcut. Not W: on German Mac
    // keyboards Option+Shift+W types the opening quote „ (when it was ⌥).
    #[cfg(target_os = "macos")]
    let default_cycle_word_list_shortcut = "ctrl+shift+l";
    #[cfg(not(target_os = "macos"))]
    let default_cycle_word_list_shortcut = "alt+shift+l";

    bindings.insert(
        "cycle_word_list".to_string(),
        ShortcutBinding {
            id: "cycle_word_list".to_string(),
            name: "Switch Word List".to_string(),
            description: "Switches on the next additional word list.".to_string(),
            default_binding: default_cycle_word_list_shortcut.to_string(),
            current_binding: default_cycle_word_list_shortcut.to_string(),
        },
    );
    // DanyWhere: turns choosing the profile by application on and off
    // (`app_profiles`). A for "application"; Control/Alt+Shift like the
    // word list shortcut. Registered only while applications are assigned.
    #[cfg(target_os = "macos")]
    let default_toggle_profile_auto_shortcut = "ctrl+shift+a";
    #[cfg(not(target_os = "macos"))]
    let default_toggle_profile_auto_shortcut = "alt+shift+a";

    bindings.insert(
        crate::app_profiles::TOGGLE_BINDING.to_string(),
        ShortcutBinding {
            id: crate::app_profiles::TOGGLE_BINDING.to_string(),
            name: "Choose Profile by Application".to_string(),
            description: "Turns choosing the profile by application on or off.".to_string(),
            default_binding: default_toggle_profile_auto_shortcut.to_string(),
            current_binding: default_toggle_profile_auto_shortcut.to_string(),
        },
    );
    // DanyWhere: turns "Send Context" on and off for the profile a dictation
    // into the app in front uses (`send_context`). K for "Kontext"/context
    // (C is Copy Last Transcript); Control/Alt+Shift like the profile
    // toggle. Registered only while some dictation can post-process.
    #[cfg(target_os = "macos")]
    let default_toggle_send_context_shortcut = "ctrl+shift+x";
    #[cfg(not(target_os = "macos"))]
    let default_toggle_send_context_shortcut = "alt+shift+x";

    bindings.insert(
        crate::send_context::TOGGLE_BINDING.to_string(),
        ShortcutBinding {
            id: crate::send_context::TOGGLE_BINDING.to_string(),
            name: "Send Context".to_string(),
            description:
                "Turns sending context with post-processing on or off for the profile a dictation into the app in front uses."
                    .to_string(),
            default_binding: default_toggle_send_context_shortcut.to_string(),
            current_binding: default_toggle_send_context_shortcut.to_string(),
        },
    );
    bindings.insert(
        "cancel".to_string(),
        ShortcutBinding {
            id: "cancel".to_string(),
            name: "Cancel".to_string(),
            description: "Cancels the current recording.".to_string(),
            default_binding: "escape".to_string(),
            current_binding: "escape".to_string(),
        },
    );

    AppSettings {
        settings_schema_version: default_settings_schema_version(),
        bindings,
        shortcut_activation: ShortcutActivation::default(),
        hold_threshold_ms: default_hold_threshold_ms(),
        audio_feedback: false,
        audio_feedback_volume: default_audio_feedback_volume(),
        sound_theme: default_sound_theme(),
        done_sound: default_done_sound(),
        start_hidden: default_start_hidden(),
        autostart_enabled: default_autostart_enabled(),
        update_checks_enabled: default_update_checks_enabled(),
        show_whats_new_on_update: default_show_whats_new_on_update(),
        whats_new_last_seen_version: default_whats_new_last_seen_version(),
        selected_model: "".to_string(),
        onboarding_completed: false,
        accessibility_skipped: false,
        insert_service_shortcut_set: false,
        dictation_key_inserts: default_dictation_key_inserts(),
        dictation_service_when_idle: DictationServiceWhenIdle::default(),
        dictation_service_shortcuts_written_at: None,
        always_on_microphone: false,
        spoken_languages: Vec::new(),
        selected_microphone: None,
        selected_channel: None,
        clamshell_microphone: None,
        selected_output_device: None,
        translate_to_english: false,
        selected_language: "auto".to_string(),
        overlay_position: default_overlay_position(),
        debug_mode: false,
        log_level: default_log_level(),
        custom_words: Vec::new(),
        word_lists: Vec::new(),
        active_word_list_id: None,
        dictation_context: Default::default(),
        model_unload_timeout: ModelUnloadTimeout::default(),
        word_correction_threshold: default_word_correction_threshold(),
        history_limit: default_history_limit(),
        recording_retention_period: default_recording_retention_period(),
        recording_storage_limit: RecordingStorageLimit::default(),
        paste_method: PasteMethod::default(),
        clipboard_handling: ClipboardHandling::default(),
        auto_submit: default_auto_submit(),
        auto_submit_key: AutoSubmitKey::default(),
        post_process_enabled: default_post_process_enabled(),
        post_process_provider_id: default_post_process_provider_id(),
        post_process_providers: default_post_process_providers(),
        post_process_api_keys: default_post_process_api_keys(),
        post_process_models: default_post_process_models(),
        post_process_efforts: HashMap::new(),
        post_process_prompts: default_post_process_prompts(),
        post_process_selected_prompt_id: None,
        post_process_timeout_secs: crate::llm_timeouts::default_post_process_timeout_secs(),
        model_list_timeout_secs: crate::llm_timeouts::default_model_list_timeout_secs(),
        mute_while_recording: false,
        append_trailing_space: false,
        append_text: String::new(),
        app_language: default_app_language(),
        theme: default_theme(),
        experimental_enabled: false,
        lazy_stream_close: false,
        airpods_mode: false,
        keyboard_implementation: KeyboardImplementation::default(),
        show_tray_icon: default_show_tray_icon(),
        paste_delay_ms: default_paste_delay_ms(),
        clipboard_notice_ms: default_clipboard_notice_ms(),
        paste_delay_after_ms: default_paste_delay_after_ms(),
        reliable_paste: false,
        typing_tool: default_typing_tool(),
        external_script_path: None,
        filler_word_removal_enabled: default_filler_word_removal_enabled(),
        custom_filler_words: None,
        chinese_script: default_chinese_script(),
        transcribe_accelerator: TranscribeAcceleratorSetting::default(),
        transcribe_gpu_device: default_transcribe_gpu_device(),
        extra_recording_buffer_ms: 0,
        vad_enabled: default_vad_enabled(),
        vad_backend: VadBackend::default(),
        long_form_chunking: default_long_form_chunking(),
        language_detection: LanguageDetectionMode::default(),
        language_detection_window_secs: default_language_detection_window_secs(),
        overlay_style: default_overlay_style(),
        profiles: Vec::new(),
        active_profile_id: String::new(),
        default_values: None,
        profile_words: Vec::new(),
        profile_auto_by_app: default_profile_auto_by_app(),
        send_context: false,
        history_box_height_newest: None,
        history_box_height_other: None,
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        get_default_settings()
    }
}

/// The language detector's window bounds: below 2 s it guesses, and it only
/// scores the first 30 s of what it's given.
pub const LANGUAGE_DETECTION_WINDOW_SECS: std::ops::RangeInclusive<u32> = 2..=30;

impl AppSettings {
    /// Samples (16 kHz) of speech the language detector hears.
    pub fn language_detection_window_samples(&self) -> usize {
        let (min, max) = LANGUAGE_DETECTION_WINDOW_SECS.into_inner();
        self.language_detection_window_secs.clamp(min, max) as usize * 16_000
    }

    pub fn active_post_process_provider(&self) -> Option<&PostProcessProvider> {
        self.post_process_providers
            .iter()
            .find(|provider| provider.id == self.post_process_provider_id)
    }

    pub fn post_process_provider(&self, provider_id: &str) -> Option<&PostProcessProvider> {
        self.post_process_providers
            .iter()
            .find(|provider| provider.id == provider_id)
    }

    pub fn post_process_provider_mut(
        &mut self,
        provider_id: &str,
    ) -> Option<&mut PostProcessProvider> {
        self.post_process_providers
            .iter_mut()
            .find(|provider| provider.id == provider_id)
    }
}

/// Startup entry point. Same load-or-create/salvage/migrate behavior as
/// `get_settings`; kept as a named alias for call-site clarity, plus a
/// one-time debug dump of the loaded settings.
pub fn load_or_create_app_settings(app: &AppHandle) -> AppSettings {
    let settings = get_settings(app);
    debug!("Loaded settings: {:?}", settings);
    settings
}

/// DanyWhere: the shortcuts that post-process, which are registered only
/// while post-processing is switched on, and the switch "Send Context",
/// which does nothing without it.
pub fn binding_needs_post_processing(id: &str) -> bool {
    id == "transcribe_with_post_process" || id == crate::send_context::TOGGLE_BINDING
}

/// DanyWhere: whether DanyWhere registers the shortcut `id` as its own.
/// Not the post-processing ones while post-processing is off, and not
/// "Paste Last Transcript" while it is the shortcut of the "Insert Last
/// Dictation" service (`insert_service::paste_last_is_service_shortcut`).
/// `service_available` is `delivery::INSERT_SERVICE_AVAILABLE`, passed in
/// so tests cover every platform. The cancel shortcut is registered only
/// while recording, by its callers' own rule.
pub fn registers_binding(id: &str, settings: &AppSettings, service_available: bool) -> bool {
    // DanyWhere: also while a profile with applications post-processes, and
    // the profile toggle only while applications are assigned.
    if binding_needs_post_processing(id)
        && !crate::app_profiles::post_processing_reachable(settings)
    {
        return false;
    }
    if id == crate::app_profiles::TOGGLE_BINDING && !crate::app_profiles::has_assignments(settings)
    {
        return false;
    }
    if id == crate::insert_service::PASTE_LAST_BINDING {
        let binding = settings
            .bindings
            .get(id)
            .map(|b| b.current_binding.clone())
            .or_else(|| {
                get_default_settings()
                    .bindings
                    .get(id)
                    .map(|b| b.current_binding.clone())
            })
            .unwrap_or_default();
        return !crate::insert_service::paste_last_is_service_shortcut(
            settings.accessibility_skipped,
            &binding,
            service_available,
        );
    }
    true
}

pub fn get_settings(app: &AppHandle) -> AppSettings {
    let store = app
        .store(crate::portable::store_path(SETTINGS_STORE_PATH))
        .expect("Failed to initialize store");

    // Settings reads also persist one-time migrations. Migration helpers are
    // idempotent, so this converges after the first read of an older store.
    let mut settings = if let Some(settings_value) = store.get("settings") {
        let (mut settings, mut updated) =
            match serde_json::from_value::<AppSettings>(settings_value.clone()) {
                Ok(settings) => (settings, false),
                Err(e) => {
                    warn!("Failed to parse stored settings ({e}); salvaging valid fields");
                    (salvage_settings(&settings_value), true)
                }
            };

        if apply_settings_migrations(&mut settings, &settings_value) {
            updated = true;
        }

        // Merge in any bindings added since this store was written.
        for (key, value) in get_default_settings().bindings {
            if let std::collections::hash_map::Entry::Vacant(entry) = settings.bindings.entry(key) {
                debug!("Adding missing binding: {}", entry.key());
                entry.insert(value);
                updated = true;
            }
        }

        if updated {
            store.set("settings", serde_json::to_value(&settings).unwrap());
        }

        settings
    } else {
        let default_settings = get_default_settings();
        store.set("settings", serde_json::to_value(&default_settings).unwrap());
        default_settings
    };

    if ensure_post_process_defaults_everywhere(&mut settings) {
        store.set("settings", serde_json::to_value(&settings).unwrap());
    }

    // DanyWhere: the defaults on a new installation or the switch from the
    // profile "Standard", and the latest flat values where they belong.
    if crate::profiles::ensure_profiles(&mut settings) {
        store.set("settings", serde_json::to_value(&settings).unwrap());
    }

    settings
}

/// Rebuilds settings from a store value that failed to deserialize as a whole.
/// Every stored field that is individually valid is kept; only broken values
/// (e.g. an enum variant written by a newer or older version) fall back to
/// their default. This means one bad field can never reset the rest of the
/// user's configuration (#1619).
fn salvage_settings(stored: &serde_json::Value) -> AppSettings {
    let Some(stored_map) = stored.as_object() else {
        warn!("Stored settings are not a JSON object; falling back to defaults");
        return get_default_settings();
    };

    let mut merged = serde_json::to_value(get_default_settings())
        .expect("default settings serialize to a JSON object");

    for (key, value) in stored_map {
        let previous = merged
            .as_object_mut()
            .expect("merged settings stay an object")
            .insert(key.clone(), value.clone());
        if serde_json::from_value::<AppSettings>(merged.clone()).is_err() {
            // Log only the key: values may hold secrets (e.g. API keys).
            warn!("Dropping invalid settings field '{key}', keeping its default");
            let map = merged
                .as_object_mut()
                .expect("merged settings stay an object");
            match previous {
                Some(previous) => map.insert(key.clone(), previous),
                None => map.remove(key),
            };
        }
    }

    serde_json::from_value(merged).unwrap_or_else(|e| {
        warn!("Failed to reassemble salvaged settings ({e}); falling back to defaults");
        get_default_settings()
    })
}

fn apply_settings_migrations(
    settings: &mut AppSettings,
    settings_value: &serde_json::Value,
) -> bool {
    let mut updated = false;

    // One-time onboarding migration: users with an explicit selected model have
    // already made it through model selection. Users who merely have compatible
    // files on disk should still see onboarding.
    if settings_value.get("onboarding_completed").is_none() {
        settings.onboarding_completed = !settings.selected_model.is_empty();
        updated = true;
    }

    // One-time What's New migration: migrations only run on an existing store
    // (fresh installs stamp the current version via get_default_settings). A
    // missing key here means a user upgrading from before it existed — blank it
    // so they see the current release's What's New, mirroring the onboarding
    // migration's explicit first-run-vs-upgrade decision.
    if settings_value.get("whats_new_last_seen_version").is_none() {
        settings.whats_new_last_seen_version = String::new();
        updated = true;
    }

    // One-time shortcut activation migration (only while the new key is
    // absent): the retired `push_to_talk` bool maps onto the two legacy modes so
    // upgrading users keep exactly the behavior they had. Only fresh installs
    // get the hold-or-toggle default.
    if settings_value.get("shortcut_activation").is_none() {
        if let Some(push_to_talk) = settings_value.get("push_to_talk").and_then(|v| v.as_bool()) {
            settings.shortcut_activation = if push_to_talk {
                ShortcutActivation::PushToTalk
            } else {
                ShortcutActivation::Toggle
            };
            updated = true;
        }
    }

    // One-time storage limit migration (only while the new key is absent):
    // the count and age options gave way to a size limit for recordings.
    if settings_value.get("recording_storage_limit").is_none() {
        settings.recording_storage_limit =
            RecordingStorageLimit::for_retention(settings.recording_retention_period);
        updated = true;
    }

    // One-time spoken languages backfill for users onboarded before it was asked.
    // The guess always includes English, so it never decides a transcription
    // (`effective_language` only uses a lone language). Must run before the
    // Chinese script migrations: it reads `zh-Hans`/`zh-Hant`, and they read it.
    if settings.onboarding_completed && settings.spoken_languages.is_empty() {
        settings.spoken_languages = guessed_spoken_languages(settings, settings_value);
        updated = true;
    }

    // One-time Chinese script migration: the script used to be chosen through
    // `zh-Hans`/`zh-Hant` language intents. Split those into the recognition
    // language and the script setting; every other upgrading user keeps the
    // unconverted output they had. Only fresh installs get the locale default.
    if settings_value.get("chinese_script").is_none() {
        settings.chinese_script = match settings.selected_language.as_str() {
            "zh-Hans" => ChineseScript::Simplified,
            "zh-Hant" => ChineseScript::TraditionalTw,
            _ => ChineseScript::AsTranscribed,
        };
        if settings.chinese_script != ChineseScript::AsTranscribed {
            settings.selected_language = "zh".to_string();
        }
        updated = true;
    }

    // One-time Traditional split: `traditional` took its regional forms from
    // what was spoken (Hong Kong's for Cantonese, Taiwan's for Mandarin).
    // Keep Hong Kong's for people who speak Cantonese or whose OS's first
    // Traditional Chinese language is Hong Kong's or Macau's. People who
    // dictated both now get one region's forms for both.
    if settings_value
        .get("chinese_script")
        .and_then(|v| v.as_str())
        == Some("traditional")
    {
        let hong_kong = settings.spoken_languages.iter().any(|l| l == "yue")
            || os_languages().iter().find_map(|tag| {
                match crate::chinese_script::chinese_script_for_locale(tag) {
                    Some(s @ (ChineseScript::TraditionalTw | ChineseScript::TraditionalHk)) => {
                        Some(s)
                    }
                    _ => None,
                }
            }) == Some(ChineseScript::TraditionalHk);
        settings.chinese_script = if hong_kong {
            ChineseScript::TraditionalHk
        } else {
            ChineseScript::TraditionalTw
        };
        updated = true;
    }

    let stored_schema_version = settings_value
        .get("settings_schema_version")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if stored_schema_version < 1 {
        // Before schema 1 this was a UI ordinal. Preserve the original safety
        // migration: a positive selection was ambiguous even in 0.1.
        let had_positive_legacy_selection = settings_value
            .get("transcribe_gpu_device")
            .and_then(|value| value.as_i64())
            .is_some_and(|value| value > 0);
        if had_positive_legacy_selection {
            settings.transcribe_accelerator = TranscribeAcceleratorSetting::Auto;
        }
    }
    if stored_schema_version < 2 {
        // transcribe.cpp 0.2 replaced integer registry indices with opaque
        // process-local handles. Clear every old index once.
        settings.transcribe_gpu_device = default_transcribe_gpu_device();
        settings.settings_schema_version = CURRENT_SETTINGS_SCHEMA_VERSION;
        updated = true;
    }
    if stored_schema_version < 3 {
        // Linux's default clipboard restore delay went from 60ms to 500ms.
        // Someone not pasting through the clipboard has never used the delay,
        // so give them the new default for when they switch to Shift+Insert.
        // People already pasting through the clipboard keep their value.
        #[cfg(target_os = "linux")]
        if settings.paste_delay_after_ms == 60
            && !matches!(
                settings.paste_method,
                PasteMethod::CtrlV | PasteMethod::CtrlShiftV | PasteMethod::ShiftInsert
            )
        {
            settings.paste_delay_after_ms = default_paste_delay_after_ms();
        }
        settings.settings_schema_version = CURRENT_SETTINGS_SCHEMA_VERSION;
        updated = true;
    }

    // The generic GPU choice was removed in favor of Auto or an exact device.
    // Normalize settings created by builds that exposed that short-lived option.
    if settings.transcribe_accelerator == TranscribeAcceleratorSetting::Gpu
        && settings.transcribe_gpu_device.is_none()
    {
        settings.transcribe_accelerator = TranscribeAcceleratorSetting::Auto;
        updated = true;
    }

    // One-time overlay migration (only while the new key is absent): the retired
    // overlay_position `none` meant "hide the overlay" → OverlayStyle::None; any
    // other position had it visible → Live. The position enum no longer has a
    // `none` variant (legacy "none" deserializes to Bottom via a serde alias), so
    // read the raw stored string to recover the old intent.
    if settings_value.get("overlay_style").is_none() {
        let was_hidden = settings_value
            .get("overlay_position")
            .and_then(|v| v.as_str())
            == Some("none");
        settings.overlay_style = if was_hidden {
            OverlayStyle::None
        } else {
            OverlayStyle::Live
        };
        updated = true;
    }

    // DanyWhere: a stored profile without a profile value gets the flat
    // value it had (contract V5), after the migrations of the flat values
    // above and before those of a profile's own below, which win.
    if crate::profiles::fill_missing_values_from_flat(settings, settings_value) {
        updated = true;
    }
    // DanyWhere: the same for the defaults (`profile_layers`).
    if crate::profile_layers::fill_missing_defaults_from_flat(settings, settings_value) {
        updated = true;
    }

    // DanyWhere: one-time "Append" migration from the trailing space and
    // auto submit, for the settings and every stored profile.
    if crate::append_text::migrate(settings, settings_value) {
        updated = true;
    }

    // DanyWhere: the context shortcut gave way to "Send Context".
    if crate::send_context::drop_retired_binding(settings) {
        updated = true;
    }

    updated
}

/// Likely spoken languages from existing settings and the OS; always includes English.
fn guessed_spoken_languages(
    settings: &AppSettings,
    settings_value: &serde_json::Value,
) -> Vec<String> {
    // Read the stored value: a missing key may hold the locale default, not a choice.
    let picked_script = settings_value
        .get("chinese_script")
        .and_then(|v| v.as_str())
        .is_some_and(|script| script != "as_transcribed");
    let intent = settings.selected_language.as_str();
    // A script says Chinese, unless the language they picked already does.
    let picked_chinese = crate::chinese_script::chinese_script_for_locale(intent).is_some();
    let tags = [
        (intent != "auto").then_some(intent),
        (picked_script && !picked_chinese).then_some("zh"),
        Some(settings.app_language.as_str()),
    ]
    .into_iter()
    .flatten()
    .chain(os_languages().iter().map(String::as_str));
    crate::system_languages::suggested_languages(tags)
}

/// The OS's languages; empty under test so migrations are deterministic.
fn os_languages() -> &'static [String] {
    if cfg!(test) {
        &[]
    } else {
        crate::system_languages::system_languages()
    }
}

/// How this build was packaged, as far as the updater is concerned. Mirrors
/// the Linux cases of `tauri::utils::platform::BundleType`; `Other` covers
/// every other stamped type and `None` (at the call site) a binary the bundler
/// did not stamp, such as a source, AUR or other distro build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
enum BundleKind {
    AppImage,
    Deb,
    Rpm,
    Other,
}

/// Whether the updater plugin can install an update on Linux. It runs
/// `dpkg -i` for a deb build, `rpm -U` for an rpm build, and otherwise swaps
/// the running AppImage. A build with no bundle type, or one whose tool is
/// missing (the deb repacked for Arch), would only fail after a password
/// prompt. Pure so it can be tested on any OS.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn linux_self_update_possible(kind: Option<BundleKind>, has_tool: impl Fn(&str) -> bool) -> bool {
    match kind {
        Some(BundleKind::AppImage) => true,
        Some(BundleKind::Deb) => has_tool("dpkg"),
        Some(BundleKind::Rpm) => has_tool("rpm"),
        Some(BundleKind::Other) | None => false,
    }
}

#[cfg(target_os = "linux")]
fn self_update_possible() -> bool {
    use std::os::unix::fs::PermissionsExt;
    use tauri::utils::config::BundleType;
    use tauri::utils::platform::bundle_type;

    let kind = bundle_type().map(|b| match b {
        BundleType::AppImage => BundleKind::AppImage,
        BundleType::Deb => BundleKind::Deb,
        BundleType::Rpm => BundleKind::Rpm,
        _ => BundleKind::Other,
    });
    let has_tool = |name: &str| {
        let path = std::env::var_os("PATH").unwrap_or_default();
        std::env::split_paths(&path)
            .chain(["/usr/bin", "/bin", "/usr/sbin", "/sbin"].map(std::path::PathBuf::from))
            .any(|dir| {
                std::fs::metadata(dir.join(name))
                    .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                    .unwrap_or(false)
            })
    };
    linux_self_update_possible(kind, has_tool)
}

#[cfg(not(target_os = "linux"))]
fn self_update_possible() -> bool {
    true
}

/// Update checks are forced off (without touching the persisted setting) when
/// `HANDY_DISABLE_UPDATER` is set — e.g. by the Nix package, since self-update
/// can't work against an immutable /nix/store install — or when this build
/// cannot install its own update (see `linux_self_update_possible`).
pub fn update_checks_forced_disabled() -> bool {
    use std::sync::OnceLock;
    static IS_UPDATER_DISABLED: OnceLock<bool> = OnceLock::new();
    *IS_UPDATER_DISABLED
        .get_or_init(|| utils::env_flag_enabled("HANDY_DISABLE_UPDATER") || !self_update_possible())
}

/// Effective updater state: the user's stored preference, overridden to `false`
/// while `HANDY_DISABLE_UPDATER` is set. Callers deciding whether to actually
/// check for updates must use this rather than reading `update_checks_enabled`
/// directly, so the forced-off state never leaks into the persisted setting.
pub fn update_checks_effectively_enabled(settings: &AppSettings) -> bool {
    settings.update_checks_enabled && !update_checks_forced_disabled()
}

pub fn write_settings(app: &AppHandle, settings: AppSettings) {
    let store = app
        .store(crate::portable::store_path(SETTINGS_STORE_PATH))
        .expect("Failed to initialize store");

    // DanyWhere: a change of the flat values goes where its value comes
    // from: the defaults or the profile in use (`profile_layers`).
    let mut settings = settings;
    let _ = crate::profile_layers::route_flat(&mut settings);

    store.set("settings", serde_json::to_value(&settings).unwrap());
}

pub fn get_bindings(app: &AppHandle) -> HashMap<String, ShortcutBinding> {
    let settings = get_settings(app);

    settings.bindings
}

pub fn get_stored_binding(settings: &AppSettings, id: &str) -> Result<ShortcutBinding, String> {
    settings
        .bindings
        .get(id)
        .cloned()
        .ok_or_else(|| format!("Binding with id '{}' not found", id))
}

pub fn get_recording_storage_limit(app: &AppHandle) -> RecordingStorageLimit {
    get_settings(app).recording_storage_limit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_post_processing_shortcuts_need_post_processing() {
        assert!(binding_needs_post_processing(
            "transcribe_with_post_process"
        ));
        // DanyWhere: the switch "Send Context" does nothing without it.
        assert!(binding_needs_post_processing(
            crate::send_context::TOGGLE_BINDING
        ));
        assert!(!binding_needs_post_processing("transcribe_with_context"));
        assert!(!binding_needs_post_processing("transcribe"));
        assert!(!binding_needs_post_processing("paste_last_transcript"));
    }

    fn without_accessibility() -> AppSettings {
        let mut settings = get_default_settings();
        crate::shortcut::apply_no_accessibility_mode(&mut settings);
        settings
    }

    fn with_paste_last(mut settings: AppSettings, keys: &str) -> AppSettings {
        settings
            .bindings
            .get_mut("paste_last_transcript")
            .unwrap()
            .current_binding = keys.to_string();
        settings
    }

    #[test]
    fn without_accessibility_danywhere_leaves_paste_last_to_the_service() {
        let settings = with_paste_last(without_accessibility(), "ctrl+shift+v");
        assert!(!registers_binding("paste_last_transcript", &settings, true));
        // The others stay DanyWhere's, the copy shortcut included.
        assert!(registers_binding("copy_last_transcript", &settings, true));
        assert!(registers_binding("transcribe", &settings, true));
        assert!(registers_binding("cycle_word_list", &settings, true));
    }

    #[test]
    fn paste_last_stays_registered_where_the_service_cant_take_it() {
        let settings = with_paste_last(get_default_settings(), "ctrl+shift+v");
        assert!(registers_binding("paste_last_transcript", &settings, true));
        let settings = with_paste_last(without_accessibility(), "ctrl+shift+v");
        assert!(registers_binding("paste_last_transcript", &settings, false));
        let settings = with_paste_last(without_accessibility(), "option+shift+v");
        assert!(registers_binding("paste_last_transcript", &settings, true));
    }

    /// DanyWhere: the defaults (in use, post-processing off) and "mail"
    /// (post-processing on) assigned to Mail.
    fn with_mail_profile() -> AppSettings {
        let mut settings = get_default_settings();
        settings.post_process_enabled = false;
        crate::profiles::ensure_profiles(&mut settings);
        let mut mail = crate::profiles::profile_for_test(&settings, "mail", "Mail", |values| {
            values.post_process_enabled = true;
        });
        mail.apps = vec!["com.apple.mail".to_string()];
        settings.profiles.push(mail);
        settings
    }

    #[test]
    fn the_profile_toggle_is_registered_only_while_apps_are_assigned() {
        let toggle = crate::app_profiles::TOGGLE_BINDING;
        let mut settings = with_mail_profile();
        assert!(registers_binding(toggle, &settings, true));
        crate::app_profiles::set_apps(&mut settings, "mail", Vec::new()).unwrap();
        assert!(!registers_binding(toggle, &settings, true));
    }

    #[test]
    fn an_app_profile_with_post_processing_keeps_its_shortcuts_registered() {
        let mut settings = with_mail_profile();
        assert!(registers_binding(
            "transcribe_with_post_process",
            &settings,
            true
        ));
        crate::app_profiles::toggle_auto(&mut settings);
        assert!(!registers_binding(
            "transcribe_with_post_process",
            &settings,
            true
        ));
    }

    #[test]
    fn the_post_processing_shortcuts_are_registered_only_while_it_is_on() {
        let mut settings = get_default_settings();
        settings.post_process_enabled = false;
        assert!(!registers_binding(
            "transcribe_with_post_process",
            &settings,
            true
        ));
        assert!(!registers_binding(
            crate::send_context::TOGGLE_BINDING,
            &settings,
            true
        ));
        settings.post_process_enabled = true;
        assert!(registers_binding(
            "transcribe_with_post_process",
            &settings,
            true
        ));
        assert!(registers_binding(
            crate::send_context::TOGGLE_BINDING,
            &settings,
            true
        ));
    }

    #[test]
    fn copy_last_transcript_has_a_shortcut_next_to_paste_last() {
        // User, 2026-11-14: ⌃⇧V pastes and ⌃⇧C copies, with Accessibility
        // or without; Alt+Shift elsewhere, like paste last.
        let defaults = get_default_settings();
        let copy = defaults.bindings.get("copy_last_transcript");
        let expected = if cfg!(target_os = "macos") {
            "ctrl+shift+c"
        } else {
            "alt+shift+c"
        };
        assert_eq!(copy.map(|b| b.default_binding.as_str()), Some(expected));
        assert_eq!(copy.map(|b| b.current_binding.as_str()), Some(expected));
        assert_eq!(copy.map(|b| b.name.as_str()), Some("Copy Last Transcript"));
        let others = defaults
            .bindings
            .values()
            .filter(|b| b.id != "copy_last_transcript" && b.default_binding == expected)
            .count();
        assert_eq!(others, 0, "{expected} is taken");
    }

    #[test]
    fn stored_binding_returns_the_requested_binding() {
        let settings = get_default_settings();

        let result = get_stored_binding(&settings, "transcribe");

        assert_eq!(result.unwrap().id, "transcribe");
    }

    #[test]
    fn unknown_stored_binding_returns_an_error() {
        let settings = get_default_settings();

        let result = get_stored_binding(&settings, "unknown");

        assert_eq!(result.unwrap_err(), "Binding with id 'unknown' not found");
    }

    fn default_settings_json() -> serde_json::Value {
        serde_json::to_value(get_default_settings()).unwrap()
    }

    /// Every field must survive a partial store: a missing key must never fail
    /// the whole-settings parse (#1619). `json!({})` is the extreme case.
    #[test]
    fn empty_store_parses_with_defaults() {
        let settings: AppSettings = serde_json::from_value(serde_json::json!({}))
            .expect("all AppSettings fields need serde defaults");
        assert_eq!(
            settings.shortcut_activation,
            ShortcutActivation::HoldOrToggle
        );
        assert_eq!(settings.hold_threshold_ms, default_hold_threshold_ms());
        assert!(!settings.audio_feedback);
        assert!(settings.filler_word_removal_enabled);
        // Bindings default to empty; the load path merges the real defaults in.
        assert!(settings.bindings.is_empty());
    }

    /// DanyWhere: the done sound is on for new installations and for every
    /// store written before it existed.
    #[test]
    fn a_store_without_the_done_sound_turns_it_on() {
        let settings: AppSettings = serde_json::from_value(serde_json::json!({}))
            .expect("all AppSettings fields need serde defaults");
        assert!(settings.done_sound);
        assert!(get_default_settings().done_sound);
    }

    /// Frozen snapshot of a real v0.9.0-era settings store, as written to
    /// disk. This pins backwards compatibility: it must always parse strictly
    /// (no salvage). Schema migrations may then rewrite fields whose native
    /// meaning changed.
    ///
    /// If a schema change breaks this test, do NOT just update the fixture —
    /// it stands in for the stores on users' machines. Add a
    /// `#[serde(alias)]`/`#[serde(other)]` or a one-time migration in
    /// `apply_settings_migrations` so old values keep loading, and only extend
    /// the fixture alongside that.
    #[test]
    fn frozen_v0_9_store_parses_strictly_then_migrates_device_index() {
        // Note "log_level": 2 — the legacy numeric format, kept deliberately.
        let stored: serde_json::Value = serde_json::from_str(
            r##"{
            "settings_schema_version": 1,
            "bindings": {
                "transcribe": {
                    "id": "transcribe",
                    "name": "Transcribe",
                    "description": "Converts your speech into text.",
                    "default_binding": "option+space",
                    "current_binding": "f13"
                },
                "transcribe_with_post_process": {
                    "id": "transcribe_with_post_process",
                    "name": "Transcribe with Post-Processing",
                    "description": "Converts your speech into text and applies AI post-processing.",
                    "default_binding": "option+shift+space",
                    "current_binding": "option+shift+space"
                },
                "cancel": {
                    "id": "cancel",
                    "name": "Cancel",
                    "description": "Cancels the current recording.",
                    "default_binding": "escape",
                    "current_binding": "escape"
                }
            },
            "push_to_talk": false,
            "audio_feedback": true,
            "audio_feedback_volume": 0.8,
            "sound_theme": "pop",
            "start_hidden": false,
            "autostart_enabled": true,
            "update_checks_enabled": true,
            "show_whats_new_on_update": true,
            "whats_new_last_seen_version": "0.9.0",
            "selected_model": "whisper-large-v3-turbo",
            "onboarding_completed": true,
            "always_on_microphone": false,
            "selected_microphone": "MacBook Pro Microphone",
            "clamshell_microphone": null,
            "selected_output_device": null,
            "translate_to_english": false,
            "selected_language": "en",
            "overlay_position": "bottom",
            "debug_mode": false,
            "log_level": 2,
            "custom_words": ["Handy", "cjpais"],
            "model_unload_timeout": "min5",
            "word_correction_threshold": 0.18,
            "history_limit": 5,
            "recording_retention_period": "preserve_limit",
            "paste_method": "ctrl_v",
            "clipboard_handling": "dont_modify",
            "auto_submit": false,
            "auto_submit_key": "enter",
            "post_process_enabled": false,
            "post_process_provider_id": "openai",
            "post_process_providers": [
                {
                    "id": "openai",
                    "label": "OpenAI",
                    "base_url": "https://api.openai.com/v1",
                    "allow_base_url_edit": false,
                    "models_endpoint": null,
                    "supports_structured_output": true
                }
            ],
            "post_process_api_keys": { "openai": "" },
            "post_process_models": { "openai": "gpt-4o-mini" },
            "post_process_prompts": [
                { "id": "default", "name": "Default", "prompt": "Clean up the transcript." }
            ],
            "post_process_selected_prompt_id": null,
            "mute_while_recording": false,
            "append_trailing_space": false,
            "app_language": "en",
            "experimental_enabled": false,
            "lazy_stream_close": false,
            "keyboard_implementation": "handy_keys",
            "show_tray_icon": true,
            "paste_delay_ms": 60,
            "typing_tool": "auto",
            "external_script_path": null,
            "custom_filler_words": null,
            "transcribe_accelerator": "gpu",
            "ort_accelerator": "auto",
            "transcribe_gpu_device": 0,
            "extra_recording_buffer_ms": 0,
            "vad_enabled": true,
            "overlay_style": "live"
        }"##,
        )
        .expect("fixture is valid JSON");

        let mut settings: AppSettings = serde_json::from_value(stored.clone())
            .expect("a stored v0.9.0 settings object must keep parsing strictly");

        assert_eq!(settings.selected_model, "whisper-large-v3-turbo");
        assert!(!settings.accessibility_skipped);
        // DanyWhere: no effort stored yet = every model's default.
        assert!(settings.post_process_efforts.is_empty());
        assert_eq!(settings.clipboard_notice_ms, 2500);
        assert_eq!(settings.bindings["transcribe"].current_binding, "f13");
        assert_eq!(settings.log_level, LogLevel::Debug);
        assert_eq!(settings.sound_theme, SoundTheme::Pop);
        assert!(settings.filler_word_removal_enabled);
        assert_eq!(settings.vad_backend, VadBackend::Silero);
        assert!(
            settings.long_form_chunking,
            "chunking is on for settings saved before it existed"
        );

        // The 0.1 integer device index is cleared once for transcribe.cpp 0.2.
        // Without an exact device, the retired generic GPU choice becomes Auto.
        assert!(apply_settings_migrations(&mut settings, &stored));
        assert_eq!(
            settings.settings_schema_version,
            CURRENT_SETTINGS_SCHEMA_VERSION
        );
        assert_eq!(
            settings.transcribe_accelerator,
            TranscribeAcceleratorSetting::Auto
        );
        // The retired push_to_talk bool (false in this fixture) becomes the
        // matching legacy mode rather than the new hold-or-toggle default.
        assert_eq!(settings.shortcut_activation, ShortcutActivation::Toggle);
        assert_eq!(settings.transcribe_gpu_device, None);
    }

    #[test]
    fn salvage_preserves_valid_fields_when_one_value_is_invalid() {
        let mut stored = default_settings_json();
        let map = stored.as_object_mut().unwrap();
        map.insert(
            "selected_model".into(),
            serde_json::json!("parakeet-tdt-0.6b-v3"),
        );
        map.insert("onboarding_completed".into(), serde_json::json!(true));
        // An enum variant this build doesn't know, e.g. written by a newer
        // version before a downgrade.
        map.insert("sound_theme".into(), serde_json::json!("theremin"));
        stored["bindings"]["transcribe"]["current_binding"] = serde_json::json!("f13");

        // Precondition: this is exactly the whole-store parse failure from
        // #1619 that used to reset everything to defaults.
        assert!(serde_json::from_value::<AppSettings>(stored.clone()).is_err());

        let salvaged = salvage_settings(&stored);
        assert_eq!(salvaged.selected_model, "parakeet-tdt-0.6b-v3");
        assert!(salvaged.onboarding_completed);
        assert_eq!(salvaged.bindings["transcribe"].current_binding, "f13");
        assert_eq!(salvaged.sound_theme, default_sound_theme());
    }

    #[test]
    fn salvage_drops_only_wrong_typed_fields() {
        let mut stored = default_settings_json();
        let map = stored.as_object_mut().unwrap();
        map.insert("paste_delay_ms".into(), serde_json::json!("sixty"));
        map.insert("sound_theme".into(), serde_json::json!(42));
        map.insert("custom_words".into(), serde_json::json!(["handy"]));

        assert!(serde_json::from_value::<AppSettings>(stored.clone()).is_err());

        let salvaged = salvage_settings(&stored);
        assert_eq!(salvaged.paste_delay_ms, default_paste_delay_ms());
        assert_eq!(salvaged.sound_theme, default_sound_theme());
        assert_eq!(salvaged.custom_words, vec!["handy".to_string()]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn default_shortcuts_do_not_conflict_with_ibus_input_method_switch() {
        let defaults = get_default_settings();
        assert_eq!(
            defaults.bindings["transcribe"].current_binding,
            "ctrl+alt+space"
        );
        assert_eq!(
            defaults.bindings["transcribe_with_post_process"].current_binding,
            "ctrl+alt+shift+space",
        );
    }

    #[test]
    fn salvage_of_poisoned_bindings_keeps_other_fields() {
        let mut stored = default_settings_json();
        let map = stored.as_object_mut().unwrap();
        // One malformed entry poisons the whole bindings map, but must not
        // take the rest of the settings down with it.
        map.insert(
            "bindings".into(),
            serde_json::json!({ "transcribe": { "id": 42 } }),
        );
        map.insert("selected_model".into(), serde_json::json!("whisper-small"));

        assert!(serde_json::from_value::<AppSettings>(stored.clone()).is_err());

        let salvaged = salvage_settings(&stored);
        assert_eq!(salvaged.selected_model, "whisper-small");
        let defaults = get_default_settings();
        assert_eq!(
            salvaged.bindings["transcribe"].current_binding,
            defaults.bindings["transcribe"].current_binding
        );
    }

    #[test]
    fn salvage_tolerates_unknown_keys() {
        let mut stored = default_settings_json();
        let map = stored.as_object_mut().unwrap();
        map.insert(
            "field_from_the_future".into(),
            serde_json::json!({ "nested": true }),
        );
        map.insert("selected_model".into(), serde_json::json!("kept"));
        map.insert("sound_theme".into(), serde_json::json!("theremin"));

        let salvaged = salvage_settings(&stored);
        assert_eq!(salvaged.selected_model, "kept");
        assert_eq!(salvaged.sound_theme, default_sound_theme());
    }

    #[test]
    fn salvage_of_non_object_store_falls_back_to_defaults() {
        for stored in [
            serde_json::json!("corrupt"),
            serde_json::json!(null),
            serde_json::json!([1, 2, 3]),
        ] {
            let salvaged = salvage_settings(&stored);
            assert_eq!(
                serde_json::to_value(&salvaged).unwrap(),
                default_settings_json()
            );
        }
    }

    #[test]
    fn default_settings_disable_auto_submit() {
        let settings = get_default_settings();
        assert!(!settings.auto_submit);
        assert_eq!(settings.auto_submit_key, AutoSubmitKey::Enter);
        assert_eq!(
            settings.settings_schema_version,
            CURRENT_SETTINGS_SCHEMA_VERSION
        );
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn default_overlay_style_is_live_when_overlay_defaults_on() {
        let settings = get_default_settings();
        assert_eq!(settings.overlay_style, OverlayStyle::Live);
    }

    #[test]
    fn overlay_migration_keeps_disabled_overlay_off() {
        let mut settings = get_default_settings();

        // Legacy store: overlay was hidden via the retired position "none".
        let raw = serde_json::json!({
            "selected_model": "",
            "overlay_position": "none"
        });

        assert!(apply_settings_migrations(&mut settings, &raw));
        assert_eq!(settings.overlay_style, OverlayStyle::None);
    }

    /// DanyWhere: the trailing space and auto submit become the "Append"
    /// field when the store is read (`append_text::migrate`).
    #[test]
    fn append_migration_runs_when_the_store_is_read() {
        let raw = serde_json::json!({
            "append_trailing_space": true,
            "auto_submit": true,
            "auto_submit_key": "ctrl_enter"
        });
        let mut settings: AppSettings = serde_json::from_value(raw.clone()).expect("parses");
        assert!(apply_settings_migrations(&mut settings, &raw));
        assert_eq!(settings.append_text, r"\s{Ctrl+Enter}");
        // The defaults are made from the migrated flat values.
        crate::profiles::ensure_profiles(&mut settings);
        assert_eq!(
            crate::profiles::default_values(&settings).append_text,
            r"\s{Ctrl+Enter}"
        );
    }

    /// DanyWhere: a store written on another system (or by hand) gets this
    /// system's key names in "Append" when it is read, in the flat value and
    /// in every profile, so none of them turns invalid by moving.
    #[test]
    fn reading_the_store_gives_append_the_systems_key_names() {
        use crate::append_text::{localize, parse, Append, AppendKey, KeyPlatform};
        // Every system's name of the key, so at least two are foreign here.
        let written = "{Cmd+Enter}{Win+Enter}{Super+Enter}{Return}";
        let mut settings = get_default_settings();
        settings.append_text = written.to_string();
        crate::profiles::ensure_profiles(&mut settings);
        let work = crate::profiles::profile_for_test(&settings, "work", "Work", |_| {});
        settings.profiles.push(work);
        let raw = serde_json::to_value(&settings).expect("serializes");
        let mut settings: AppSettings = serde_json::from_value(raw.clone()).expect("parses");
        assert!(apply_settings_migrations(&mut settings, &raw));
        let defaults = settings.default_values.clone().expect("defaults");
        for value in [
            &settings.append_text,
            &settings.profiles[0].values.append_text,
            &defaults.append_text,
        ] {
            // Only this system's names are left.
            assert_eq!(&localize(value, KeyPlatform::current()), value);
            assert_eq!(
                Append::from_parts(&parse(value)).keys,
                vec![
                    AppendKey::CmdEnter,
                    AppendKey::CmdEnter,
                    AppendKey::CmdEnter,
                    AppendKey::Enter
                ],
                "{value}"
            );
        }

        // Once stored, the next read changes nothing.
        let raw = serde_json::to_value(&settings).expect("serializes");
        let mut again: AppSettings = serde_json::from_value(raw.clone()).expect("parses");
        assert!(!apply_settings_migrations(&mut again, &raw));
    }

    #[test]
    fn legacy_none_overlay_position_deserializes_to_bottom() {
        // A persisted "none" must not fail the whole settings load; the serde
        // alias folds it onto Bottom (visibility is owned by overlay_style).
        let raw = serde_json::json!({ "overlay_position": "none" });
        let position: OverlayPosition =
            serde_json::from_value(raw.get("overlay_position").unwrap().clone())
                .expect("legacy \"none\" should deserialize, not error");
        assert_eq!(position, OverlayPosition::Bottom);
    }

    #[test]
    fn overlay_migration_promotes_enabled_overlay_to_live() {
        let mut settings = get_default_settings();
        settings.overlay_position = OverlayPosition::Top;
        settings.overlay_style = OverlayStyle::Minimal;

        let raw = serde_json::json!({
            "selected_model": "",
            "overlay_position": "top"
        });

        assert!(apply_settings_migrations(&mut settings, &raw));
        assert_eq!(settings.overlay_style, OverlayStyle::Live);
        assert_eq!(settings.overlay_position, OverlayPosition::Top);
    }

    #[test]
    fn chinese_script_migration_only_carries_over_legacy_intents() {
        for (intent, language, script) in [
            ("zh-Hans", "zh", ChineseScript::Simplified),
            ("zh-Hant", "zh", ChineseScript::TraditionalTw),
            ("auto", "auto", ChineseScript::AsTranscribed),
        ] {
            let mut settings = get_default_settings();
            settings.selected_language = intent.to_string();
            settings.chinese_script = ChineseScript::TraditionalTw;
            let raw = serde_json::json!({ "selected_language": intent });

            assert!(apply_settings_migrations(&mut settings, &raw));
            assert_eq!(settings.selected_language, language);
            assert_eq!(settings.chinese_script, script);
        }
    }

    /// Spoken languages backfilled for an onboarded upgrader with these stored
    /// settings (merged over the defaults, as a stored file deserializes).
    fn backfilled(stored: serde_json::Value) -> AppSettings {
        let mut raw = serde_json::json!({
            "onboarding_completed": true,
            "selected_language": "auto",
            "app_language": "en"
        });
        raw.as_object_mut()
            .unwrap()
            .extend(stored.as_object().unwrap().clone());
        let mut settings: AppSettings = serde_json::from_value(raw.clone()).unwrap();
        apply_settings_migrations(&mut settings, &raw);
        settings
    }

    #[test]
    fn fresh_installs_have_only_the_global_word_list() {
        let defaults = get_default_settings();
        assert!(defaults.word_lists.is_empty());
        assert_eq!(defaults.active_word_list_id, None);
    }

    #[test]
    fn an_older_store_keeps_its_words_as_the_global_list() {
        let settings = backfilled(serde_json::json!({
            "custom_words": ["Sayuri", "DanyWhere"]
        }));
        assert_eq!(settings.custom_words, vec!["Sayuri", "DanyWhere"]);
        assert!(settings.word_lists.is_empty());
        assert_eq!(settings.active_word_list_id, None);
    }

    #[test]
    fn word_lists_round_trip_through_the_store() {
        let settings = backfilled(serde_json::json!({
            "custom_words": ["Sayuri"],
            "word_lists": [{ "id": "w1", "name": "Work", "words": ["Jira"] }],
            "active_word_list_id": "w1"
        }));
        assert_eq!(settings.word_lists.len(), 1);
        assert_eq!(settings.word_lists[0].name, "Work");
        assert_eq!(settings.active_word_list_id.as_deref(), Some("w1"));
    }

    #[test]
    fn recording_storage_limits_in_bytes() {
        assert_eq!(RecordingStorageLimit::Mb100.bytes(), Some(100_000_000));
        assert_eq!(RecordingStorageLimit::Mb500.bytes(), Some(500_000_000));
        assert_eq!(RecordingStorageLimit::Gb1.bytes(), Some(1_000_000_000));
        assert_eq!(RecordingStorageLimit::Gb5.bytes(), Some(5_000_000_000));
        assert_eq!(RecordingStorageLimit::Unlimited.bytes(), None);
    }

    #[test]
    fn fresh_installs_keep_500_mb_of_recordings() {
        assert_eq!(
            get_default_settings().recording_storage_limit,
            RecordingStorageLimit::Mb500
        );
        let parsed: AppSettings = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(parsed.recording_storage_limit, RecordingStorageLimit::Mb500);
    }

    #[test]
    fn retention_migrates_to_a_storage_limit() {
        for (period, limit) in [
            ("never", RecordingStorageLimit::Unlimited),
            ("preserve_limit", RecordingStorageLimit::Mb500),
            ("days3", RecordingStorageLimit::Mb500),
            ("weeks2", RecordingStorageLimit::Mb500),
            ("months3", RecordingStorageLimit::Mb500),
        ] {
            let settings = backfilled(serde_json::json!({
                "recording_retention_period": period
            }));
            assert_eq!(settings.recording_storage_limit, limit, "{period}");
        }
        // A store from before either key existed had the count limit.
        assert_eq!(
            backfilled(serde_json::json!({})).recording_storage_limit,
            RecordingStorageLimit::Mb500
        );
    }

    #[test]
    fn a_chosen_storage_limit_is_kept() {
        let settings = backfilled(serde_json::json!({
            "recording_retention_period": "never",
            "recording_storage_limit": "gb_1"
        }));
        assert_eq!(settings.recording_storage_limit, RecordingStorageLimit::Gb1);
    }

    /// The frontend sends the names specta generates in bindings.ts.
    #[test]
    fn storage_limits_use_the_names_the_frontend_sends() {
        for (limit, name) in [
            (RecordingStorageLimit::Mb100, "mb_100"),
            (RecordingStorageLimit::Mb500, "mb_500"),
            (RecordingStorageLimit::Gb1, "gb_1"),
            (RecordingStorageLimit::Gb5, "gb_5"),
            (RecordingStorageLimit::Unlimited, "unlimited"),
        ] {
            assert_eq!(serde_json::to_value(limit).unwrap(), name);
            assert_eq!(
                serde_json::from_value::<RecordingStorageLimit>(name.into()).unwrap(),
                limit
            );
        }
    }

    #[test]
    fn spoken_languages_backfill_guesses_with_english_included() {
        for (stored, spoken) in [
            (
                serde_json::json!({ "selected_language": "de" }),
                vec!["de", "en"],
            ),
            (serde_json::json!({ "selected_language": "en" }), vec!["en"]),
            (
                serde_json::json!({ "selected_language": "zh-Hant" }),
                vec!["zh", "en"],
            ),
            (
                serde_json::json!({ "chinese_script": "traditional" }),
                vec!["zh", "en"],
            ),
            (
                serde_json::json!({ "chinese_script": "as_transcribed" }),
                vec!["en"],
            ),
            // A picked language leads, the app's follows.
            (
                serde_json::json!({ "selected_language": "fr", "app_language": "de" }),
                vec!["fr", "de", "en"],
            ),
        ] {
            assert_eq!(
                backfilled(stored.clone()).spoken_languages,
                spoken,
                "{stored}"
            );
        }
    }

    #[test]
    fn spoken_languages_backfill_keeps_cantonese_on_hong_kong_forms() {
        let settings = backfilled(serde_json::json!({
            "selected_language": "yue",
            "chinese_script": "traditional"
        }));

        assert_eq!(settings.spoken_languages, vec!["yue", "en"]);
        assert_eq!(settings.chinese_script, ChineseScript::TraditionalHk);
    }

    #[test]
    fn spoken_languages_backfill_leaves_chosen_and_unasked_alone() {
        let chosen = backfilled(serde_json::json!({
            "selected_language": "de",
            "spoken_languages": ["ja"]
        }));
        assert_eq!(chosen.spoken_languages, vec!["ja"]);

        let unasked = backfilled(serde_json::json!({
            "onboarding_completed": false,
            "selected_language": "de"
        }));
        assert!(unasked.spoken_languages.is_empty());
    }

    #[test]
    fn shortcut_activation_migration_maps_push_to_talk_true() {
        let mut settings = get_default_settings();
        let raw = serde_json::json!({
            "selected_model": "",
            "push_to_talk": true
        });

        assert!(apply_settings_migrations(&mut settings, &raw));
        assert_eq!(settings.shortcut_activation, ShortcutActivation::PushToTalk);
    }

    #[test]
    fn shortcut_activation_migration_maps_push_to_talk_false() {
        let mut settings = get_default_settings();
        let raw = serde_json::json!({
            "selected_model": "",
            "push_to_talk": false
        });

        assert!(apply_settings_migrations(&mut settings, &raw));
        assert_eq!(settings.shortcut_activation, ShortcutActivation::Toggle);
    }

    #[test]
    fn shortcut_activation_migration_respects_explicit_new_key() {
        let mut settings = get_default_settings();
        settings.shortcut_activation = ShortcutActivation::HoldOrToggle;
        let raw = serde_json::json!({
            "selected_model": "",
            "push_to_talk": true,
            "shortcut_activation": "hold_or_toggle"
        });

        apply_settings_migrations(&mut settings, &raw);
        assert_eq!(
            settings.shortcut_activation,
            ShortcutActivation::HoldOrToggle
        );
    }

    #[test]
    fn shortcut_activation_defaults_to_hold_or_toggle_without_legacy_key() {
        let mut settings = get_default_settings();
        let raw = serde_json::json!({ "selected_model": "" });

        apply_settings_migrations(&mut settings, &raw);
        assert_eq!(
            settings.shortcut_activation,
            ShortcutActivation::HoldOrToggle
        );
    }

    #[test]
    fn gpu_device_migration_resets_legacy_positive_selection_to_auto() {
        let mut settings = get_default_settings();
        settings.transcribe_accelerator = TranscribeAcceleratorSetting::Gpu;

        let raw = serde_json::json!({
            "transcribe_accelerator": "gpu",
            "transcribe_gpu_device": 2
        });

        assert!(apply_settings_migrations(&mut settings, &raw));
        assert_eq!(
            settings.transcribe_accelerator,
            TranscribeAcceleratorSetting::Auto
        );
        assert_eq!(settings.transcribe_gpu_device, None);
        assert_eq!(
            settings.settings_schema_version,
            CURRENT_SETTINGS_SCHEMA_VERSION
        );
    }

    #[test]
    fn gpu_device_migration_maps_v1_automatic_gpu_to_auto() {
        let raw = serde_json::json!({
            "settings_schema_version": 1,
            "transcribe_accelerator": "gpu",
            "transcribe_gpu_device": 2
        });
        let mut settings: AppSettings = serde_json::from_value(raw.clone()).unwrap();

        assert!(apply_settings_migrations(&mut settings, &raw));
        assert_eq!(
            settings.transcribe_accelerator,
            TranscribeAcceleratorSetting::Auto
        );
        assert_eq!(settings.transcribe_gpu_device, None);
    }

    #[test]
    fn gpu_device_migration_maps_current_automatic_gpu_to_auto() {
        let raw = serde_json::json!({
            "settings_schema_version": CURRENT_SETTINGS_SCHEMA_VERSION,
            "onboarding_completed": false,
            "whats_new_last_seen_version": default_whats_new_last_seen_version(),
            "overlay_style": "live",
            "transcribe_accelerator": "gpu",
            "transcribe_gpu_device": null
        });
        let mut settings: AppSettings = serde_json::from_value(raw.clone()).unwrap();

        assert!(apply_settings_migrations(&mut settings, &raw));
        assert_eq!(
            settings.transcribe_accelerator,
            TranscribeAcceleratorSetting::Auto
        );
        assert_eq!(settings.transcribe_gpu_device, None);
    }

    #[test]
    fn gpu_device_migration_keeps_current_stable_selection() {
        let mut settings = get_default_settings();
        settings.transcribe_accelerator = TranscribeAcceleratorSetting::Gpu;
        settings.transcribe_gpu_device = Some("[\"vulkan\",\"id\",\"0000:01:00.0\"]".into());

        let raw = serde_json::json!({
            "settings_schema_version": CURRENT_SETTINGS_SCHEMA_VERSION,
            "onboarding_completed": false,
            "whats_new_last_seen_version": default_whats_new_last_seen_version(),
            "overlay_style": "live",
            "chinese_script": "as_transcribed",
            "recording_storage_limit": "mb_500",
            "transcribe_accelerator": "gpu",
            "transcribe_gpu_device": settings.transcribe_gpu_device
        });

        assert!(!apply_settings_migrations(&mut settings, &raw));
        assert_eq!(
            settings.transcribe_gpu_device.as_deref(),
            Some("[\"vulkan\",\"id\",\"0000:01:00.0\"]")
        );
    }

    #[test]
    fn post_process_defaults_sync_builtin_api_format_but_keep_custom() {
        let mut settings = get_default_settings();
        for provider in settings.post_process_providers.iter_mut() {
            match provider.id.as_str() {
                // Settings saved before the built-in provider moved to Messages
                "anthropic" => provider.api_format = PostProcessApiFormat::OpenAi,
                "custom" => provider.api_format = PostProcessApiFormat::Anthropic,
                _ => {}
            }
        }

        assert!(ensure_post_process_defaults(&mut settings));

        let format_of = |id: &str| settings.post_process_provider(id).unwrap().api_format;
        assert_eq!(format_of("anthropic"), PostProcessApiFormat::Anthropic);
        assert_eq!(format_of("custom"), PostProcessApiFormat::Anthropic);
    }

    /// DanyWhere: the fixed prompt frame (`prompt_frame`).
    #[test]
    fn handys_untouched_default_prompt_is_replaced_when_settings_load() {
        let mut settings = get_default_settings();
        settings.post_process_prompts[0].prompt =
            crate::prompt_frame::HANDY_IMPROVE_TRANSCRIPTIONS[2].to_string();

        assert!(ensure_post_process_defaults(&mut settings));
        assert_eq!(
            settings.post_process_prompts[0].prompt,
            crate::prompt_frame::IMPROVE_TRANSCRIPTIONS
        );
        assert!(!ensure_post_process_defaults(&mut settings));
    }

    #[test]
    fn debug_output_redacts_api_keys() {
        let mut settings = get_default_settings();
        settings
            .post_process_api_keys
            .insert("openai".to_string(), "sk-proj-secret-key-12345".to_string());
        settings.post_process_api_keys.insert(
            "anthropic".to_string(),
            "sk-ant-secret-key-67890".to_string(),
        );
        settings
            .post_process_api_keys
            .insert("empty_provider".to_string(), "".to_string());

        let debug_output = format!("{:?}", settings);

        assert!(!debug_output.contains("sk-proj-secret-key-12345"));
        assert!(!debug_output.contains("sk-ant-secret-key-67890"));
        assert!(debug_output.contains("[REDACTED]"));
    }

    // ---- DanyWhere: profile values (P6) -----------------------------------

    /// A store from before the newer profile values: flat ones for them, a
    /// second profile without them, which still has Handy's trailing space.
    fn stored_before_new_profile_values() -> serde_json::Value {
        serde_json::json!({
            "selected_microphone": "USB Mic",
            "overlay_position": "top",
            "overlay_style": "minimal",
            "custom_words": ["Sayuri"],
            "append_text": "{Enter}",
            "active_profile_id": "default",
            "profiles": [
                { "id": "default", "name": "", "values": { "append_text": "{Enter}" } },
                { "id": "old", "name": "Old",
                  "values": { "selected_language": "fr", "append_trailing_space": true } }
            ]
        })
    }

    #[test]
    fn a_stored_profile_gets_the_flat_values_it_lacks_when_loaded() {
        let raw = stored_before_new_profile_values();
        let mut settings: AppSettings = serde_json::from_value(raw.clone()).expect("parses");
        assert!(apply_settings_migrations(&mut settings, &raw));
        let old = serde_json::to_value(&settings.profiles[1].values).expect("serializes");
        assert_eq!(old["selected_microphone"], "USB Mic");
        assert_eq!(old["overlay_position"], "top");
        assert_eq!(old["overlay_style"], "minimal");
        assert_eq!(old["custom_words"], serde_json::json!(["Sayuri"]));
        assert_eq!(old["selected_language"], "fr");
        // A migration of its own still wins over the flat value: the
        // profile's trailing space, not the flat "Append" text.
        assert_eq!(old["append_text"], r"\s");
    }

    #[test]
    fn a_new_built_in_provider_reaches_every_profile() {
        let mut settings = get_default_settings();
        crate::profiles::ensure_profiles(&mut settings);
        let mut other = crate::profiles::profile_for_test(&settings, "other", "Other", |_| {});
        other.values = crate::profiles::testing::values_with(
            &other.values,
            serde_json::json!({
                "post_process_providers": [{
                    "id": "openai",
                    "label": "OpenAI",
                    "base_url": "https://api.openai.com/v1",
                }],
                "post_process_api_keys": { "openai": "sk-other" },
            }),
        );
        settings.profiles.push(other);
        assert!(ensure_post_process_defaults_everywhere(&mut settings));
        let values = serde_json::to_value(&settings.profiles[0].values).expect("serializes");
        let ids: Vec<&str> = values["post_process_providers"]
            .as_array()
            .expect("providers")
            .iter()
            .filter_map(|provider| provider["id"].as_str())
            .collect();
        for provider in default_post_process_providers() {
            assert!(ids.contains(&provider.id.as_str()), "{}", provider.id);
        }
        assert_eq!(values["post_process_api_keys"]["openai"], "sk-other");
        assert_eq!(values["post_process_api_keys"]["anthropic"], "");
        assert!(!ensure_post_process_defaults_everywhere(&mut settings));
    }

    #[test]
    fn secret_map_debug_redacts_values() {
        let map = SecretMap(HashMap::from([("key".into(), "secret".into())]));
        let out = format!("{:?}", map);
        assert!(!out.contains("secret"));
        assert!(out.contains("[REDACTED]"));
    }

    #[test]
    fn linux_self_update_possible_depends_on_bundle_and_tool() {
        let all = |_: &str| true;
        let none = |_: &str| false;
        let only = |want: &'static str| move |name: &str| name == want;

        assert!(linux_self_update_possible(Some(BundleKind::AppImage), none));
        assert!(linux_self_update_possible(Some(BundleKind::Deb), all));
        assert!(linux_self_update_possible(
            Some(BundleKind::Deb),
            only("dpkg")
        ));
        assert!(!linux_self_update_possible(Some(BundleKind::Deb), none));
        assert!(!linux_self_update_possible(
            Some(BundleKind::Deb),
            only("rpm")
        ));
        assert!(linux_self_update_possible(
            Some(BundleKind::Rpm),
            only("rpm")
        ));
        assert!(!linux_self_update_possible(Some(BundleKind::Rpm), none));
        assert!(!linux_self_update_possible(
            Some(BundleKind::Rpm),
            only("dpkg")
        ));
        assert!(!linux_self_update_possible(Some(BundleKind::Other), all));
        assert!(!linux_self_update_possible(None, all));
    }
}
