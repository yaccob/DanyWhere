use crate::audio_feedback;
use crate::audio_toolkit::audio::{
    list_microphones, list_output_devices, MicrophoneDevice, OutputDeviceInfo,
};
use crate::managers::audio::{AudioRecordingManager, MicrophoneMode};
use crate::settings::{get_settings, write_settings};
use log::warn;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

#[cfg(target_os = "windows")]
use winreg::{
    enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE},
    RegKey, HKEY,
};

#[derive(Serialize, Type)]
pub struct CustomSounds {
    start: bool,
    stop: bool,
}

#[tauri::command]
#[specta::specta]
pub fn check_custom_sounds(app: AppHandle) -> CustomSounds {
    // DanyWhere: the files the custom theme plays, its own or those the
    // sound file names.
    CustomSounds {
        start: audio_feedback::custom_sound_exists(&app, audio_feedback::SoundType::Start),
        stop: audio_feedback::custom_sound_exists(&app, audio_feedback::SoundType::Stop),
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct AudioDevice {
    pub index: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum PermissionAccess {
    Allowed,
    Denied,
    Unknown,
}

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct WindowsMicrophonePermissionStatus {
    pub supported: bool,
    pub overall_access: PermissionAccess,
    pub device_access: PermissionAccess,
    pub app_access: PermissionAccess,
    pub desktop_app_access: PermissionAccess,
}

#[cfg(target_os = "windows")]
fn read_registry_permission_access(root_hkey: HKEY, path: &str) -> PermissionAccess {
    let root = RegKey::predef(root_hkey);
    let Ok(key) = root.open_subkey(path) else {
        return PermissionAccess::Unknown;
    };

    let Ok(value) = key.get_value::<String, _>("Value") else {
        return PermissionAccess::Unknown;
    };

    match value.to_ascii_lowercase().as_str() {
        "allow" => PermissionAccess::Allowed,
        "deny" => PermissionAccess::Denied,
        _ => PermissionAccess::Unknown,
    }
}

#[cfg(target_os = "windows")]
fn get_windows_microphone_permission_status_impl() -> WindowsMicrophonePermissionStatus {
    const MICROPHONE_PATH: &str =
        "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\microphone";
    const DESKTOP_APPS_PATH: &str =
        "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\microphone\\NonPackaged";

    let device_access = read_registry_permission_access(HKEY_LOCAL_MACHINE, MICROPHONE_PATH);
    let app_access = read_registry_permission_access(HKEY_CURRENT_USER, MICROPHONE_PATH);
    let desktop_app_access = read_registry_permission_access(HKEY_CURRENT_USER, DESKTOP_APPS_PATH);

    // Handy is a desktop app, so the NonPackaged key (desktop_app_access) is
    // the relevant permission scope. The UWP master key (app_access) can be
    // "deny" on systems with debloaters (e.g. O&O ShutUp10) without actually
    // blocking desktop app microphone access.
    let overall_access = if device_access == PermissionAccess::Denied {
        PermissionAccess::Denied
    } else if desktop_app_access == PermissionAccess::Denied {
        PermissionAccess::Denied
    } else if desktop_app_access == PermissionAccess::Allowed {
        PermissionAccess::Allowed
    } else if app_access == PermissionAccess::Denied {
        PermissionAccess::Denied
    } else if device_access == PermissionAccess::Allowed && app_access == PermissionAccess::Allowed
    {
        PermissionAccess::Allowed
    } else {
        PermissionAccess::Unknown
    };

    WindowsMicrophonePermissionStatus {
        supported: true,
        overall_access,
        device_access,
        app_access,
        desktop_app_access,
    }
}

#[tauri::command]
#[specta::specta]
pub fn get_windows_microphone_permission_status() -> WindowsMicrophonePermissionStatus {
    #[cfg(target_os = "windows")]
    {
        get_windows_microphone_permission_status_impl()
    }

    #[cfg(not(target_os = "windows"))]
    {
        WindowsMicrophonePermissionStatus {
            supported: false,
            overall_access: PermissionAccess::Unknown,
            device_access: PermissionAccess::Unknown,
            app_access: PermissionAccess::Unknown,
            desktop_app_access: PermissionAccess::Unknown,
        }
    }
}

#[tauri::command]
#[specta::specta]
pub fn open_microphone_privacy_settings() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        Command::new("cmd")
            .args(["/C", "start", "", "ms-settings:privacy-microphone"])
            .spawn()
            .map_err(|e| format!("Failed to open Windows microphone privacy settings: {}", e))?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
            .spawn()
            .map_err(|e| format!("Failed to open macOS microphone privacy settings: {}", e))?;
        Ok(())
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Err(
            "Opening microphone privacy settings is only supported on Windows and macOS"
                .to_string(),
        )
    }
}

#[tauri::command]
#[specta::specta]
pub async fn update_microphone_mode(app: AppHandle, always_on: bool) -> Result<(), String> {
    // Update settings (fast, stays inline)
    let mut settings = get_settings(&app);
    settings.always_on_microphone = always_on;
    write_settings(&app, settings);

    // Update the audio manager mode. update_mode can stop/start the cpal stream
    // (blocking CoreAudio) and takes the manager std mutexes — run it on a
    // blocking thread, NOT inline on the webview/main run loop (a slow device
    // open/close would freeze the UI).
    let rm = app.state::<Arc<AudioRecordingManager>>().inner().clone();
    let new_mode = if always_on {
        MicrophoneMode::AlwaysOn
    } else {
        MicrophoneMode::OnDemand
    };

    tokio::task::spawn_blocking(move || rm.update_mode(new_mode))
        .await
        .map_err(|e| format!("audio task join failed: {}", e))?
        .map_err(|e| format!("Failed to update microphone mode: {}", e))
}

#[tauri::command]
#[specta::specta]
pub fn get_microphone_mode(app: AppHandle) -> Result<bool, String> {
    let settings = get_settings(&app);
    Ok(settings.always_on_microphone)
}

#[tauri::command]
#[specta::specta]
pub async fn get_available_microphones() -> Result<Vec<AudioDevice>, String> {
    // cpal device enumeration can stall — run it off the webview/main run loop.
    tokio::task::spawn_blocking(|| {
        let devices =
            list_microphones().map_err(|e| format!("Failed to list audio devices: {}", e))?;
        Ok::<_, String>(microphone_choices(devices))
    })
    .await
    .map_err(|e| format!("audio task join failed: {}", e))?
}

/// The entry that stands for the system's default device, in both lists.
fn default_choice() -> AudioDevice {
    AudioDevice {
        index: "default".to_string(),
        name: "Default".to_string(),
        is_default: true,
    }
}

/// DanyWhere: the microphones the settings offer: the default first, then
/// every device but PulseAudio monitor sources, which record a speaker's
/// output, not a microphone.
fn microphone_choices(devices: Vec<MicrophoneDevice>) -> Vec<AudioDevice> {
    let mut result = vec![default_choice()];
    result.extend(
        devices
            .into_iter()
            .filter(|d| !d.is_monitor)
            .map(|d| AudioDevice {
                index: d.id,
                name: d.name,
                is_default: false, // The explicit default is handled separately
            }),
    );
    result
}

#[tauri::command]
#[specta::specta]
pub async fn set_selected_microphone(app: AppHandle, device_name: String) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.selected_microphone = if device_name == "default" {
        None
    } else {
        Some(device_name)
    };
    write_settings(&app, settings);

    // Update the audio manager to use the new device. update_selected_device
    // can restart the cpal stream (blocking CoreAudio) — run it on a blocking
    // thread, not inline on the webview/main run loop.
    let rm = app.state::<Arc<AudioRecordingManager>>().inner().clone();
    tokio::task::spawn_blocking(move || rm.update_selected_device())
        .await
        .map_err(|e| format!("audio task join failed: {}", e))?
        .map_err(|e| format!("Failed to update selected device: {}", e))
}

#[tauri::command]
#[specta::specta]
pub fn get_selected_microphone(app: AppHandle) -> Result<String, String> {
    let settings = get_settings(&app);
    Ok(settings
        .selected_microphone
        .unwrap_or_else(|| "default".to_string()))
}

#[tauri::command]
#[specta::specta]
pub async fn get_available_output_devices() -> Result<Vec<AudioDevice>, String> {
    // cpal device enumeration can stall — run it off the webview/main run loop.
    tokio::task::spawn_blocking(|| {
        let devices =
            list_output_devices().map_err(|e| format!("Failed to list output devices: {}", e))?;
        Ok::<_, String>(output_device_choices(devices))
    })
    .await
    .map_err(|e| format!("audio task join failed: {}", e))?
}

/// DanyWhere: the output devices the settings offer: the default first,
/// then every device.
fn output_device_choices(devices: Vec<OutputDeviceInfo>) -> Vec<AudioDevice> {
    let mut result = vec![default_choice()];
    result.extend(devices.into_iter().map(|d| AudioDevice {
        index: d.index,
        name: d.name,
        is_default: false, // The explicit default is handled separately
    }));
    result
}

#[tauri::command]
#[specta::specta]
pub fn set_selected_output_device(app: AppHandle, device_name: String) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.selected_output_device = if device_name == "default" {
        None
    } else {
        Some(device_name)
    };
    write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn get_selected_output_device(app: AppHandle) -> Result<String, String> {
    let settings = get_settings(&app);
    Ok(settings
        .selected_output_device
        .unwrap_or_else(|| "default".to_string()))
}

#[tauri::command]
#[specta::specta]
pub async fn play_test_sound(app: AppHandle, sound_type: String) {
    let Some(sound) = test_sound(&sound_type) else {
        warn!("Unknown sound type: {}", sound_type);
        return;
    };
    audio_feedback::play_test_sound(&app, sound);
}

/// The sound the Sound page's preview asks for by its name.
fn test_sound(name: &str) -> Option<audio_feedback::SoundType> {
    match name {
        "start" => Some(audio_feedback::SoundType::Start),
        "instruction" => Some(audio_feedback::SoundType::Instruction),
        "stop" => Some(audio_feedback::SoundType::Stop),
        "clipboard" => Some(audio_feedback::SoundType::Clipboard),
        "done" => Some(audio_feedback::SoundType::Done),
        _ => None,
    }
}

#[tauri::command]
#[specta::specta]
pub fn set_clamshell_microphone(app: AppHandle, device_name: String) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.clamshell_microphone = if device_name == "default" {
        None
    } else {
        Some(device_name)
    };
    write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn get_clamshell_microphone(app: AppHandle) -> Result<String, String> {
    let settings = get_settings(&app);
    Ok(settings
        .clamshell_microphone
        .unwrap_or_else(|| "default".to_string()))
}

#[tauri::command]
#[specta::specta]
pub fn is_recording(app: AppHandle) -> bool {
    let audio_manager = app.state::<Arc<AudioRecordingManager>>();
    audio_manager.is_recording()
}

#[tauri::command]
#[specta::specta]
pub async fn get_microphone_channels(device_name: String) -> Result<u16, String> {
    // Device enumeration can stall, so keep it off the webview/main run loop.
    tokio::task::spawn_blocking(move || {
        let devices =
            list_microphones().map_err(|e| format!("Failed to list audio devices: {e}"))?;
        Ok(microphone_channels(devices, &device_name))
    })
    .await
    .map_err(|e| format!("audio task join failed: {e}"))?
}

/// DanyWhere: the channels of the microphone named `device_name` among
/// `devices` ("default": the system's default device); 1 when it isn't
/// there or its channels are unknown.
fn microphone_channels(devices: Vec<MicrophoneDevice>, device_name: &str) -> u16 {
    let device = if device_name.eq_ignore_ascii_case("default") {
        devices.into_iter().find(|device| device.is_default)
    } else {
        devices
            .into_iter()
            .find(|device| device.name == device_name)
    };
    // `channels` is unknown only on ALSA, where reading it opens the device.
    device.and_then(|device| device.channels).unwrap_or(1)
}

#[tauri::command]
#[specta::specta]
pub async fn set_selected_channel(app: AppHandle, channel: Option<u16>) -> Result<(), String> {
    // Restarting cpal can block, so keep it off the webview/main run loop. Apply
    // the runtime change before persisting it so a rejected active-recording
    // change does not become effective on the next launch.
    let manager = app.state::<Arc<AudioRecordingManager>>().inner().clone();
    tokio::task::spawn_blocking(move || manager.update_selected_channel(channel))
        .await
        .map_err(|e| format!("audio task join failed: {e}"))?
        .map_err(|e| format!("Failed to update channel selection: {e}"))?;

    let mut settings = get_settings(&app);
    settings.selected_channel = channel;
    write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn change_airpods_mode_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    // Same ordering as `set_selected_channel`: reopen off the main run loop,
    // persist only once the reopen succeeds.
    let manager = app.state::<Arc<AudioRecordingManager>>().inner().clone();
    tokio::task::spawn_blocking(move || manager.update_airpods_mode(enabled))
        .await
        .map_err(|e| format!("audio task join failed: {e}"))?
        .map_err(|e| format!("Failed to update AirPods mode: {e}"))?;

    let mut settings = get_settings(&app);
    settings.airpods_mode = enabled;
    write_settings(&app, settings);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_preview_plays_every_event_by_its_name() {
        use audio_feedback::SoundType;
        assert_eq!(
            ["start", "instruction", "stop", "done", "clipboard"].map(test_sound),
            [
                Some(SoundType::Start),
                Some(SoundType::Instruction),
                Some(SoundType::Stop),
                Some(SoundType::Done),
                Some(SoundType::Clipboard),
            ]
        );
        assert_eq!(test_sound("finish"), None);
    }

    // DanyWhere: what the settings offer from the lists of devices.

    fn microphone(
        id: &str,
        name: &str,
        is_default: bool,
        channels: Option<u16>,
    ) -> MicrophoneDevice {
        MicrophoneDevice {
            id: id.to_string(),
            name: name.to_string(),
            is_default,
            channels,
            is_monitor: false,
        }
    }

    fn microphones() -> Vec<MicrophoneDevice> {
        vec![
            microphone("built-in", "MacBook Pro Microphone", true, Some(1)),
            microphone("usb-7", "USB Mic", false, Some(4)),
        ]
    }

    /// The choices as (index, name, is the default).
    fn offered(choices: Vec<AudioDevice>) -> Vec<(String, String, bool)> {
        choices
            .into_iter()
            .map(|choice| (choice.index, choice.name, choice.is_default))
            .collect()
    }

    fn choice(index: &str, name: &str, is_default: bool) -> (String, String, bool) {
        (index.to_string(), name.to_string(), is_default)
    }

    #[test]
    fn the_microphones_offered_start_with_the_default_and_leave_out_monitors() {
        let mut devices = microphones();
        devices.insert(
            1,
            MicrophoneDevice {
                is_monitor: true,
                ..microphone("monitor-1", "Monitor of Built-in Audio", false, Some(2))
            },
        );
        assert_eq!(
            offered(microphone_choices(devices)),
            [
                choice("default", "Default", true),
                choice("built-in", "MacBook Pro Microphone", false),
                choice("usb-7", "USB Mic", false),
            ]
        );
    }

    #[test]
    fn without_microphones_only_the_default_is_offered() {
        assert_eq!(
            offered(microphone_choices(Vec::new())),
            [choice("default", "Default", true)]
        );
    }

    #[test]
    fn a_microphone_has_the_channels_the_list_gives_it() {
        assert_eq!(microphone_channels(microphones(), "USB Mic"), 4);
        // The default is the system's default device, whatever its name.
        assert_eq!(microphone_channels(microphones(), "default"), 1);
        assert_eq!(microphone_channels(microphones(), "Default"), 1);
    }

    #[test]
    fn unknown_channels_and_missing_microphones_count_as_one_channel() {
        // ALSA can't tell without opening the device.
        let alsa = vec![microphone("hw:1", "USB Mic", true, None)];
        assert_eq!(microphone_channels(alsa.clone(), "USB Mic"), 1);
        assert_eq!(microphone_channels(alsa, "default"), 1);
        assert_eq!(microphone_channels(microphones(), "AirPods Pro"), 1);
        let without_default = vec![microphone("usb-7", "USB Mic", false, Some(4))];
        assert_eq!(microphone_channels(without_default, "default"), 1);
    }

    #[test]
    fn the_outputs_offered_start_with_the_default_and_mark_no_other() {
        let devices = vec![
            OutputDeviceInfo {
                index: "0".to_string(),
                name: "MacBook Pro Speakers".to_string(),
                is_default: true,
            },
            OutputDeviceInfo {
                index: "1".to_string(),
                name: "USB Headset".to_string(),
                is_default: false,
            },
        ];
        assert_eq!(
            offered(output_device_choices(devices)),
            [
                choice("default", "Default", true),
                choice("0", "MacBook Pro Speakers", false),
                choice("1", "USB Headset", false),
            ]
        );
    }
}
