//! Keyboard shortcut management module
//!
//! This module provides a unified interface for keyboard shortcuts with
//! multiple backend implementations:
//!
//! - `tauri`: Uses Tauri's built-in global-shortcut plugin
//! - `handy_keys`: Uses the handy-keys library for more control
//!
//! The active implementation is determined by the `keyboard_implementation`
//! setting and can be changed at runtime.
//!
//! DanyWhere: each backend, and "None", implements
//! `backend::ShortcutBackend`, and `backend::for_implementation` is the one
//! place that picks one. Several shortcuts are registered through the one
//! loop `backend::register_all`. Starting and switching the backend and the
//! cancel shortcut run against `lifecycle::ShortcutHost`, the shortcuts that
//! follow the profile through `profile_sync::Following`, so all of it is
//! tested without the app.

pub(crate) mod backend;
mod handler;
pub mod handy_keys;
mod lifecycle;
pub mod profile_sync;
pub mod tauri_impl;

use log::{debug, error, info, warn};
use serde::Serialize;
use specta::Type;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use crate::settings::APPLE_INTELLIGENCE_DEFAULT_MODEL_ID;
use crate::settings::{
    self, AutoSubmitKey, ChineseScript, ClipboardHandling, KeyboardImplementation, LLMPrompt,
    LanguageDetectionMode, OverlayPosition, OverlayStyle, PasteMethod, ShortcutBinding, SoundTheme,
    Theme, TypingTool, VadBackend, APPLE_INTELLIGENCE_PROVIDER_ID,
};
use crate::tray;

// Note: Commands are accessed via shortcut::handy_keys:: in lib.rs

/// Initialize shortcuts using the configured implementation
pub fn init_shortcuts(app: &AppHandle) {
    // DanyWhere: with the fallback to Tauri when handy-keys can't start
    // (`lifecycle::start`).
    lifecycle::start(&lifecycle::AppHost(app));

    // DanyWhere: the shortcuts of the profile that applies now, from here on
    // whenever another application comes to the front (`profile_sync`).
    profile_sync::sync_now(app);
    profile_sync::start_observer(app);
}

/// Whether the recording lifecycle currently wants the cancel shortcut.
/// Written synchronously by start/stop, so it always holds the latest request.
static CANCEL_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Whether the cancel shortcut is actually registered with the backend.
/// The lock also serializes reconciliation passes.
#[cfg(not(target_os = "linux"))]
static CANCEL_REGISTERED: std::sync::Mutex<bool> = std::sync::Mutex::new(false);

/// Register the cancel shortcut (called when recording starts)
pub fn register_cancel_shortcut(app: &AppHandle) {
    // Track recording lifecycle independently of the current implementation so
    // switching implementations mid-recording cannot leave stale fallback state.
    crate::secure_input::register_cancel_fallback(app);

    CANCEL_REQUESTED.store(true, Ordering::SeqCst);
    schedule_cancel_reconcile(app);
    // DanyWhere: the end shortcuts too, like Escape only while recording.
    profile_sync::set_recording(app, true);
}

/// Unregister the cancel shortcut (called when recording stops)
pub fn unregister_cancel_shortcut(app: &AppHandle) {
    crate::secure_input::unregister_cancel_fallback(app);

    CANCEL_REQUESTED.store(false, Ordering::SeqCst);
    schedule_cancel_reconcile(app);
    profile_sync::set_recording(app, false);
}

/// Apply the requested cancel state off the calling thread. Start and stop run
/// on the shortcut handler thread, which must not block on registration, so the
/// work is spawned. Spawned tasks may run in any order (a very short recording
/// can see its unregister task run before its register task), so each pass
/// applies the latest request instead of the action that scheduled it.
fn schedule_cancel_reconcile(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        reconcile_cancel_shortcut(&app);
    });
}

/// DanyWhere: runs `work` off the shortcut handler thread. Shortcut actions
/// run on the handy-keys manager thread, which also serves registrations, so
/// an action whose work registers or unregisters shortcuts, directly or
/// through a settings change, would wait for itself and every shortcut would
/// stop (2026-11-15, ⌃⌥A). The same rule as `schedule_cancel_reconcile`.
pub fn off_handler_thread(work: impl FnOnce() + Send + 'static) {
    std::thread::spawn(work);
}

fn reconcile_cancel_shortcut(app: &AppHandle) {
    // Cancel shortcut is disabled on Linux due to instability with dynamic shortcut registration
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        return;
    }

    #[cfg(not(target_os = "linux"))]
    {
        let mut registered = CANCEL_REGISTERED.lock().unwrap_or_else(|e| e.into_inner());
        let requested = CANCEL_REQUESTED.load(Ordering::SeqCst);
        if requested == *registered {
            return;
        }

        let settings = settings::get_settings(app);
        let Some(cancel_binding) = settings.bindings.get("cancel").cloned() else {
            return;
        };
        let backend = backend::for_implementation(app, settings.keyboard_implementation);
        lifecycle::reconcile_cancel(
            backend.as_ref(),
            &cancel_binding,
            requested,
            &mut registered,
        );
    }
}

/// Register a shortcut using the appropriate implementation
pub fn register_shortcut(app: &AppHandle, binding: ShortcutBinding) -> Result<(), String> {
    backend::active(app).register(&binding)
}

/// Unregister a shortcut using the appropriate implementation
pub fn unregister_shortcut(app: &AppHandle, binding: ShortcutBinding) -> Result<(), String> {
    backend::active(app).unregister(&binding)
}

// ============================================================================
// Binding Management Commands
// ============================================================================

#[derive(Serialize, Type)]
pub struct BindingResponse {
    success: bool,
    binding: Option<ShortcutBinding>,
    error: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub fn change_binding(
    app: AppHandle,
    id: String,
    binding: String,
) -> Result<BindingResponse, String> {
    // Reject empty bindings — every shortcut should have a value
    if binding.trim().is_empty() {
        return Err("Binding cannot be empty".to_string());
    }

    let mut settings = settings::get_settings(&app);

    // Get the binding to modify, or create it from defaults if it doesn't exist
    let binding_to_modify = match settings.bindings.get(&id) {
        Some(binding) => binding.clone(),
        None => {
            // Try to get the default binding for this id
            let default_settings = settings::get_default_settings();
            match default_settings.bindings.get(&id) {
                Some(default_binding) => {
                    warn!(
                        "Binding '{}' not found in settings, creating from defaults",
                        id
                    );
                    default_binding.clone()
                }
                None => {
                    let error_msg = format!("Binding with id '{}' not found in defaults", id);
                    warn!("change_binding error: {}", error_msg);
                    return Ok(BindingResponse {
                        success: false,
                        binding: None,
                        error: Some(error_msg),
                    });
                }
            }
        }
    };

    // If this is the cancel binding, just update the settings and return
    // It's managed dynamically, so we don't register/unregister here
    if id == "cancel" {
        if let Some(mut b) = settings.bindings.get(&id).cloned() {
            b.current_binding = binding;
            settings.bindings.insert(id.clone(), b.clone());
            settings::write_settings(&app, settings);
            crate::secure_input::reconcile_fallback(&app);
            return Ok(BindingResponse {
                success: true,
                binding: Some(b.clone()),
                error: None,
            });
        }
    }

    // DanyWhere: without Accessibility "Paste Last Transcript" is the "Insert
    // Last Dictation" service's shortcut, so it must be one a service can
    // have.
    if id == crate::insert_service::PASTE_LAST_BINDING {
        if let Err(e) = crate::insert_service::check_paste_last_change(
            &settings,
            &binding,
            crate::delivery::INSERT_SERVICE_AVAILABLE,
        ) {
            warn!("change_binding: refused {binding} for {id}: {e}");
            return Err(e);
        }
    }

    // Validate the new shortcut for the current keyboard implementation
    if let Err(e) = validate_shortcut_for_implementation(&binding, settings.keyboard_implementation)
    {
        warn!("change_binding validation error: {}", e);
        return Err(e);
    }

    // Create an updated binding
    let mut updated_binding = binding_to_modify.clone();
    updated_binding.current_binding = binding;
    let previous = settings
        .bindings
        .insert(id.clone(), updated_binding.clone());
    settings::write_settings(&app, settings.clone());

    // DanyWhere: the shortcuts follow the profile that applies now
    // (`profile_sync`), which unregisters the old keys and registers the new
    // ones where they apply. A change ends a capture in the settings, so the
    // others come back too.
    let failures = profile_sync::resume(&app);
    if let Some((_, e)) = failures.iter().find(|(failed, _)| *failed == id) {
        let error_msg = format!("Failed to register shortcut: {}", e);
        error!("change_binding error: {}", error_msg);
        match previous {
            Some(old) => settings.bindings.insert(id.clone(), old),
            None => settings.bindings.remove(&id),
        };
        settings::write_settings(&app, settings);
        profile_sync::sync_now(&app);
        return Ok(BindingResponse {
            success: false,
            binding: None,
            error: Some(error_msg),
        });
    }
    if !settings::registers_binding(&id, &settings, crate::delivery::INSERT_SERVICE_AVAILABLE) {
        info!(
            "{id}: {} goes to the app in front, not to DanyWhere.",
            updated_binding.current_binding
        );
    }

    // Synchronize any active Secure Input shadows.
    crate::secure_input::reconcile_fallback(&app);
    // DanyWhere: "Insert Last Dictation" carries "Paste Last Transcript"
    // (the start and hold shortcuts' services follow their own commands).
    if id == crate::insert_service::PASTE_LAST_BINDING {
        crate::insert_service::sync_dictation_service_shortcuts(&app);
    }

    // Return the updated binding
    Ok(BindingResponse {
        success: true,
        binding: Some(updated_binding),
        error: None,
    })
}

#[tauri::command]
#[specta::specta]
pub fn reset_binding(app: AppHandle, id: String) -> Result<BindingResponse, String> {
    let binding = settings::get_stored_binding(&settings::get_settings(&app), &id)?;
    change_binding(app, id, binding.default_binding)
}

/// Unregister every binding while the user is recording a new shortcut in
/// the UI, so no existing shortcut can fire — or swallow the keystrokes —
/// mid-capture. The "cancel" binding is untouched: it is managed dynamically
/// by the recording lifecycle.
pub fn suspend_all_shortcuts(app: &AppHandle) {
    // DanyWhere: what is registered is the profile's that applies now.
    profile_sync::suspend(app);
}

/// Re-register every binding from settings after shortcut recording ends.
/// Registering an already-registered shortcut fails cleanly in both
/// implementations, so this is idempotent and safe on every exit path.
pub fn resume_all_shortcuts(app: &AppHandle) {
    // DanyWhere: those of the profile that applies now (`profile_sync`).
    for (id, e) in profile_sync::resume(app) {
        debug!("resume_all_shortcuts: could not register '{}': {}", id, e);
    }
}

/// Temporarily unregister all bindings while the user is recording a
/// shortcut in the UI. This avoids firing actions while keys are recorded.
#[tauri::command]
#[specta::specta]
pub fn suspend_all_bindings(app: AppHandle) -> Result<(), String> {
    suspend_all_shortcuts(&app);
    Ok(())
}

/// Re-register all bindings after the user has finished recording.
#[tauri::command]
#[specta::specta]
pub fn resume_all_bindings(app: AppHandle) -> Result<(), String> {
    resume_all_shortcuts(&app);
    Ok(())
}

// ============================================================================
// Keyboard Implementation Switching
// ============================================================================

/// Result of changing keyboard implementation
#[derive(Serialize, Type)]
pub struct ImplementationChangeResult {
    pub success: bool,
    /// List of binding IDs that were reset to defaults due to incompatibility
    pub reset_bindings: Vec<String>,
}

/// Change the keyboard implementation with runtime switching.
/// This will unregister all shortcuts from the old implementation,
/// validate shortcuts for the new implementation (resetting invalid ones to defaults),
/// and register them with the new implementation.
#[tauri::command]
#[specta::specta]
pub fn change_keyboard_implementation_setting(
    app: AppHandle,
    implementation: String,
) -> Result<ImplementationChangeResult, String> {
    // DanyWhere: with the rollback to Tauri when handy-keys can't start
    // (`lifecycle::switch`).
    lifecycle::switch(&lifecycle::AppHost(&app), &implementation)
}

/// Get the current keyboard implementation
#[tauri::command]
#[specta::specta]
pub fn get_keyboard_implementation(app: AppHandle) -> String {
    let settings = settings::get_settings(&app);
    match settings.keyboard_implementation {
        KeyboardImplementation::Tauri => "tauri".to_string(),
        KeyboardImplementation::HandyKeys => "handy_keys".to_string(),
        KeyboardImplementation::None => "none".to_string(),
    }
}

// ============================================================================
// Validation Helpers
// ============================================================================

/// Validate a shortcut for a specific implementation
pub(crate) fn validate_shortcut_for_implementation(
    raw: &str,
    implementation: KeyboardImplementation,
) -> Result<(), String> {
    match implementation {
        KeyboardImplementation::Tauri => tauri_impl::validate_shortcut(raw),
        KeyboardImplementation::HandyKeys => handy_keys::validate_shortcut(raw),
        KeyboardImplementation::None => Ok(()),
    }
}

/// Parse a keyboard implementation string into the enum
fn parse_keyboard_implementation(s: &str) -> KeyboardImplementation {
    match s {
        "tauri" => KeyboardImplementation::Tauri,
        "handy_keys" => KeyboardImplementation::HandyKeys,
        "none" => KeyboardImplementation::None,
        other => {
            warn!(
                "Invalid keyboard implementation '{}', defaulting to tauri",
                other
            );
            KeyboardImplementation::Tauri
        }
    }
}

/// Configure `settings` to work without the macOS Accessibility permission.
///
/// Without it, handy-keys' event tap receives no key events and simulated
/// paste is blocked. The Tauri backend registers hotkeys through Carbon,
/// which needs no permission, and the transcript is left on the clipboard
/// for the user to paste. Shortcuts the Tauri backend can't register (e.g.
/// `fn`) are reset to their defaults. The factory end shortcuts that press a
/// key lose those ends, as nothing can press it
/// (`end_keys::drop_factory_key_presses`); a list of the user's own stays.
///
/// Returns the IDs of bindings that were reset to their defaults.
pub fn apply_no_accessibility_mode(settings: &mut settings::AppSettings) -> Vec<String> {
    settings.accessibility_skipped = true;
    settings.paste_method = PasteMethod::None;
    settings.clipboard_handling = ClipboardHandling::CopyToClipboard;
    settings.keyboard_implementation = KeyboardImplementation::Tauri;
    crate::end_keys::drop_factory_key_presses(&mut settings.end_keys);

    let defaults = settings::get_default_settings().bindings;
    // DanyWhere: the start and hold shortcuts, too (`start_keys`).
    let mut reset = crate::start_keys::reset_invalid_keys(settings, |keys| {
        validate_shortcut_for_implementation(keys, KeyboardImplementation::Tauri).is_ok()
    });
    for (id, binding) in settings.bindings.iter_mut() {
        let Some(default) = defaults.get(id) else {
            continue;
        };
        if validate_shortcut_for_implementation(
            &binding.current_binding,
            KeyboardImplementation::Tauri,
        )
        .is_err()
        {
            binding.current_binding = default.current_binding.clone();
            reset.push(id.clone());
        }
    }
    reset.sort();
    reset
}

// ============================================================================
// General Settings Commands
// ============================================================================

#[tauri::command]
#[specta::specta]
pub fn change_hold_threshold_ms_setting(app: AppHandle, ms: u64) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.hold_threshold_ms = ms;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_audio_feedback_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.audio_feedback = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_audio_feedback_volume_setting(app: AppHandle, volume: f32) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.audio_feedback_volume = volume;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_sound_theme_setting(app: AppHandle, theme: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    let parsed = match theme.as_str() {
        "marimba" => SoundTheme::Marimba,
        "pop" => SoundTheme::Pop,
        "custom" => SoundTheme::Custom,
        other => {
            warn!("Invalid sound theme '{}', defaulting to marimba", other);
            SoundTheme::Marimba
        }
    };
    settings.sound_theme = parsed;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_theme_setting(app: AppHandle, theme: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    let parsed = match theme.as_str() {
        "system" => Theme::System,
        "light" => Theme::Light,
        "dark" => Theme::Dark,
        other => {
            warn!("Invalid theme '{}', defaulting to system", other);
            Theme::System
        }
    };
    settings.theme = parsed;
    settings::write_settings(&app, settings);
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    apply_window_theme(&app, parsed);
    // Notify other webviews (the recording overlay) so they re-apply the palette
    // live — they set `data-theme` on their own document and can't see this one.
    let _ = app.emit("theme-changed", parsed);
    Ok(())
}

/// Applies the appearance setting to the native window chrome (title bar), which
/// CSS `data-theme` cannot reach. `System` clears the override so the window
/// follows the OS. Call this on startup and whenever the setting changes to keep
/// the title bar in sync with the in-app palette.
///
/// On Windows this themes the title bar only. On macOS `set_theme` sets
/// `NSApp.appearance` app-wide, which is what we want here: it darkens the title
/// bar and keeps the overlay in step. Linux is left to `data-theme` alone, since
/// its window theming is backend-dependent and unreliable.
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub fn apply_window_theme(app: &AppHandle, theme: Theme) {
    let window_theme = match theme {
        Theme::System => None,
        Theme::Light => Some(tauri::Theme::Light),
        Theme::Dark => Some(tauri::Theme::Dark),
    };
    if let Some(window) = app.get_webview_window("main") {
        if let Err(e) = window.set_theme(window_theme) {
            warn!("Failed to apply window theme: {}", e);
        }
    }
}

#[tauri::command]
#[specta::specta]
pub fn change_translate_to_english_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.translate_to_english = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_selected_language_setting(app: AppHandle, language: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.selected_language = language.clone();
    settings::write_settings(&app, settings);
    tray::update_tray_menu(&app);

    // The tray can change it too; keep the settings window in sync.
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "selected_language",
            "value": language
        }),
    );
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_spoken_languages_setting(
    app: AppHandle,
    languages: Vec<String>,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.spoken_languages = languages;
    settings::write_settings(&app, settings);
    tray::update_tray_menu(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_overlay_position_setting(app: AppHandle, position: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    let parsed = match position.as_str() {
        // "none" is retired (visibility is overlay_style now); fold legacy callers
        // onto Bottom rather than warn.
        "none" | "bottom" => OverlayPosition::Bottom,
        "top" => OverlayPosition::Top,
        other => {
            warn!("Invalid overlay position '{}', defaulting to bottom", other);
            OverlayPosition::Bottom
        }
    };
    settings.overlay_position = parsed;
    settings::write_settings(&app, settings);

    // Whether the overlay shows at all is owned by overlay_style now; position
    // only ever toggles Top/Bottom, so the enabled cache is untouched here.
    // Update overlay position without recreating window
    crate::utils::update_overlay_position(&app);

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_overlay_style_setting(app: AppHandle, style: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    let parsed = match style.as_str() {
        "none" => OverlayStyle::None,
        "minimal" => OverlayStyle::Minimal,
        "live" => OverlayStyle::Live,
        other => {
            warn!("Invalid overlay style '{}', defaulting to minimal", other);
            OverlayStyle::Minimal
        }
    };
    settings.overlay_style = parsed;
    settings::write_settings(&app, settings);

    // Keep the cached overlay-enabled flag in sync so emit_levels stops (or
    // resumes) emitting on the next audio callback.
    crate::overlay::update_overlay_enabled_cache(parsed != OverlayStyle::None);

    // Reposition in case the window needs to re-center for the new style.
    crate::utils::update_overlay_position(&app);

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_debug_mode_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.debug_mode = enabled;
    settings::write_settings(&app, settings);

    // Keep webview log streaming in sync: the live log viewer only exists in
    // debug mode, so logs are forwarded to the frontend only while it is on.
    crate::WEBVIEW_LOG_STREAMING.store(enabled, std::sync::atomic::Ordering::Relaxed);

    // Emit event to notify frontend of debug mode change
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "debug_mode",
            "value": enabled
        }),
    );

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_start_hidden_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.start_hidden = enabled;
    settings::write_settings(&app, settings);

    // Notify frontend
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "start_hidden",
            "value": enabled
        }),
    );

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_autostart_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.autostart_enabled = enabled;
    settings::write_settings(&app, settings);

    // Apply the autostart setting immediately
    crate::autostart::apply_autostart(&app, enabled);

    // Notify frontend
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "autostart_enabled",
            "value": enabled
        }),
    );

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_update_checks_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    if settings::update_checks_forced_disabled() {
        return Err(
            "Update checks are disabled by system configuration (HANDY_DISABLE_UPDATER)".into(),
        );
    }

    let mut settings = settings::get_settings(&app);
    settings.update_checks_enabled = enabled;
    settings::write_settings(&app, settings);

    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "update_checks_enabled",
            "value": enabled
        }),
    );

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_show_whats_new_on_update_setting(
    app: AppHandle,
    enabled: bool,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.show_whats_new_on_update = enabled;
    settings::write_settings(&app, settings);

    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "show_whats_new_on_update",
            "value": enabled
        }),
    );

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_whats_new_last_seen_version_setting(
    app: AppHandle,
    version: String,
) -> Result<(), String> {
    let version = version.trim().to_string();
    let mut settings = settings::get_settings(&app);
    settings.whats_new_last_seen_version = version.clone();
    settings::write_settings(&app, settings);

    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "whats_new_last_seen_version",
            "value": version
        }),
    );

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn update_custom_words(app: AppHandle, words: Vec<String>) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.custom_words = words;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_language_detection_setting(
    app: AppHandle,
    mode: LanguageDetectionMode,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.language_detection = mode;
    settings::write_settings(&app, settings);
    tray::update_tray_menu(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_language_detection_window_setting(app: AppHandle, secs: u32) -> Result<(), String> {
    let (min, max) = settings::LANGUAGE_DETECTION_WINDOW_SECS.into_inner();
    let mut settings = settings::get_settings(&app);
    settings.language_detection_window_secs = secs.clamp(min, max);
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_extra_recording_buffer_setting(app: AppHandle, ms: u64) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.extra_recording_buffer_ms = ms;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_paste_delay_ms_setting(app: AppHandle, ms: u64) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.paste_delay_ms = ms;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_paste_delay_after_ms_setting(app: AppHandle, ms: u64) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.paste_delay_after_ms = ms;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_reliable_paste_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.reliable_paste = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_paste_method_setting(app: AppHandle, method: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    let parsed = match method.as_str() {
        "ctrl_v" => PasteMethod::CtrlV,
        "direct" => PasteMethod::Direct,
        "none" => PasteMethod::None,
        "shift_insert" => PasteMethod::ShiftInsert,
        "ctrl_shift_v" => PasteMethod::CtrlShiftV,
        "external_script" => PasteMethod::ExternalScript,
        other => {
            warn!("Invalid paste method '{}', defaulting to ctrl_v", other);
            PasteMethod::CtrlV
        }
    };
    settings.paste_method = parsed;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn get_available_typing_tools() -> Vec<String> {
    #[cfg(target_os = "linux")]
    {
        crate::clipboard::get_available_typing_tools()
    }
    #[cfg(not(target_os = "linux"))]
    {
        vec!["auto".to_string()]
    }
}

#[tauri::command]
#[specta::specta]
pub fn change_typing_tool_setting(app: AppHandle, tool: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    let parsed = match tool.as_str() {
        "auto" => TypingTool::Auto,
        "wtype" => TypingTool::Wtype,
        "kwtype" => TypingTool::Kwtype,
        "dotool" => TypingTool::Dotool,
        "ydotool" => TypingTool::Ydotool,
        "xdotool" => TypingTool::Xdotool,
        other => {
            warn!("Invalid typing tool '{}', defaulting to auto", other);
            TypingTool::Auto
        }
    };
    settings.typing_tool = parsed;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_external_script_path_setting(
    app: AppHandle,
    path: Option<String>,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.external_script_path = path;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_clipboard_handling_setting(app: AppHandle, handling: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    let parsed = match handling.as_str() {
        "dont_modify" => ClipboardHandling::DontModify,
        "copy_to_clipboard" => ClipboardHandling::CopyToClipboard,
        other => {
            warn!(
                "Invalid clipboard handling '{}', defaulting to dont_modify",
                other
            );
            ClipboardHandling::DontModify
        }
    };
    settings.clipboard_handling = parsed;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_auto_submit_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.auto_submit = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_auto_submit_key_setting(app: AppHandle, key: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    let parsed = match key.as_str() {
        "enter" => AutoSubmitKey::Enter,
        "ctrl_enter" => AutoSubmitKey::CtrlEnter,
        "cmd_enter" => AutoSubmitKey::CmdEnter,
        other => {
            warn!("Invalid auto submit key '{}', defaulting to enter", other);
            AutoSubmitKey::Enter
        }
    };
    settings.auto_submit_key = parsed;
    settings::write_settings(&app, settings);
    Ok(())
}

/// Registers what follows the profile's values after `settings` changed:
/// DanyWhere: the shortcut that turns choosing the profile by application
/// on and off only while applications are assigned, the instruction
/// shortcuts only while post-processing can run (`instruction_keys::taken`);
/// all through the shortcuts that follow the profile (`profile_sync`), which
/// read the stored settings. (The switch "Post Processing" and its command
/// gave way to `post_process_mode` on 2026-11-26.)
pub fn sync_post_process_shortcuts(app: &AppHandle, settings: &settings::AppSettings) {
    let _ = settings;
    profile_sync::sync_now(app);
}

#[tauri::command]
#[specta::specta]
pub fn change_experimental_enabled_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.experimental_enabled = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_post_process_base_url_setting(
    app: AppHandle,
    provider_id: String,
    base_url: String,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    let label = settings
        .post_process_provider(&provider_id)
        .map(|provider| provider.label.clone())
        .ok_or_else(|| format!("Provider '{}' not found", provider_id))?;

    let provider = settings
        .post_process_provider_mut(&provider_id)
        .expect("Provider looked up above must exist");

    if provider.id != "custom" {
        return Err(format!(
            "Provider '{}' does not allow editing the base URL",
            label
        ));
    }

    provider.base_url = base_url;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_post_process_api_format_setting(
    app: AppHandle,
    provider_id: String,
    api_format: settings::PostProcessApiFormat,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    let provider = settings
        .post_process_provider_mut(&provider_id)
        .ok_or_else(|| format!("Provider '{}' not found", provider_id))?;

    if provider.id != "custom" {
        return Err(format!(
            "Provider '{}' does not allow changing the API format",
            provider.label
        ));
    }

    provider.api_format = api_format;
    settings::write_settings(&app, settings);
    Ok(())
}

/// Generic helper to validate provider exists
fn validate_provider_exists(
    settings: &settings::AppSettings,
    provider_id: &str,
) -> Result<(), String> {
    if !settings
        .post_process_providers
        .iter()
        .any(|provider| provider.id == provider_id)
    {
        return Err(format!("Provider '{}' not found", provider_id));
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_post_process_api_key_setting(
    app: AppHandle,
    provider_id: String,
    api_key: String,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    validate_provider_exists(&settings, &provider_id)?;
    settings.post_process_api_keys.insert(provider_id, api_key);
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_post_process_model_setting(
    app: AppHandle,
    provider_id: String,
    model: String,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    validate_provider_exists(&settings, &provider_id)?;
    settings
        .post_process_models
        .insert(provider_id.clone(), model);
    // DanyWhere: an effort the new model doesn't take shows "Model Default".
    crate::post_process_effort::drop_if_unsupported(&mut settings, &provider_id);
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn set_post_process_provider(app: AppHandle, provider_id: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    validate_provider_exists(&settings, &provider_id)?;
    settings.post_process_provider_id = provider_id;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn add_post_process_prompt(
    app: AppHandle,
    name: String,
    prompt: String,
) -> Result<LLMPrompt, String> {
    let mut settings = settings::get_settings(&app);

    // DanyWhere: into the library all profiles share (`prompt_library`):
    // under a name no other prompt has, at the end, active in no profile.
    let new_prompt = crate::prompt_library::add(
        &mut settings,
        &name,
        &prompt,
        chrono::Utc::now().timestamp_millis(),
    )?;
    settings::write_settings(&app, settings);
    // DanyWhere: the warning about prompts with old parts.
    crate::prompt_frame::prompts_changed(&app);

    Ok(new_prompt)
}

#[tauri::command]
#[specta::specta]
pub fn update_post_process_prompt(
    app: AppHandle,
    id: String,
    name: String,
    prompt: String,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);

    // DanyWhere: in every profile (`prompt_library`); a name another prompt
    // has is refused.
    crate::prompt_library::update(&mut settings, &id, &name, &prompt)?;
    settings::write_settings(&app, settings);
    // DanyWhere: the warning about prompts with old parts.
    crate::prompt_frame::prompts_changed(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn delete_post_process_prompt(app: AppHandle, id: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);

    // DanyWhere: out of the library and every profile's order
    // (`prompt_library`); never the last prompt. The window names the
    // profiles that have it active before it asks for this.
    crate::prompt_library::delete(&mut settings, &id)?;
    settings::write_settings(&app, settings);
    // DanyWhere: the warning about prompts with old parts.
    crate::prompt_frame::prompts_changed(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn fetch_post_process_models(
    app: AppHandle,
    provider_id: String,
) -> Result<Vec<String>, String> {
    let settings = settings::get_settings(&app);

    // Find the provider
    let provider = settings
        .post_process_providers
        .iter()
        .find(|p| p.id == provider_id)
        .ok_or_else(|| format!("Provider '{}' not found", provider_id))?;

    if provider.id == APPLE_INTELLIGENCE_PROVIDER_ID {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            return Ok(vec![APPLE_INTELLIGENCE_DEFAULT_MODEL_ID.to_string()]);
        }

        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        {
            return Err("Apple Intelligence is only available on Apple silicon Macs running macOS 15 or later.".to_string());
        }
    }

    // Get API key
    let api_key = settings
        .post_process_api_keys
        .get(&provider_id)
        .cloned()
        .unwrap_or_default();

    // Skip fetching if no API key for providers that typically need one
    if api_key.trim().is_empty() && model_list_needs_api_key(&provider.id) {
        return Err(format!(
            "API key is required for {}. Please add an API key to list available models.",
            provider.label
        ));
    }

    crate::llm_client::fetch_models(
        provider,
        api_key,
        crate::llm_timeouts::model_list_timeout(&settings),
    )
    .await
}

/// DanyWhere: whether listing a provider's models needs its API key. A local
/// server (custom) may need none, and OpenRouter's list is public, so it can
/// show before a key is entered.
fn model_list_needs_api_key(provider_id: &str) -> bool {
    !matches!(provider_id, "custom" | "openrouter")
}

#[tauri::command]
#[specta::specta]
pub fn set_post_process_selected_prompt(app: AppHandle, id: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);

    // DanyWhere: the prompt becomes the active profile's only active one
    // (`prompt_library`).
    crate::prompt_library::select_only(&mut settings, &id)?;
    settings::write_settings(&app, settings);
    crate::prompt_frame::prompts_changed(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_mute_while_recording_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.mute_while_recording = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_append_trailing_space_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.append_trailing_space = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_lazy_stream_close_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.lazy_stream_close = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_vad_enabled_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.vad_enabled = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_long_form_chunking_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.long_form_chunking = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn change_vad_backend_setting(app: AppHandle, backend: VadBackend) -> Result<(), String> {
    if settings::get_settings(&app).vad_backend == backend {
        return Ok(());
    }

    // Construct/swap the detector and, when necessary, reopen cpal away from
    // the webview thread. Persist only after the runtime change succeeds so a
    // rejected in-progress switch or failed microphone reopen rolls back cleanly.
    let manager = app
        .state::<std::sync::Arc<crate::managers::audio::AudioRecordingManager>>()
        .inner()
        .clone();
    tokio::task::spawn_blocking(move || manager.update_vad_backend(backend))
        .await
        .map_err(|e| format!("audio task join failed: {e}"))?
        .map_err(|e| format!("Failed to update VAD backend: {e}"))?;

    let mut current_settings = settings::get_settings(&app);
    current_settings.vad_backend = backend;
    settings::write_settings(&app, current_settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_filler_word_removal_enabled_setting(
    app: AppHandle,
    enabled: bool,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.filler_word_removal_enabled = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

/// DanyWhere: switches the word list check (`word_list_check`) on or off.
#[tauri::command]
#[specta::specta]
pub fn change_word_list_llm_check_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.word_list_llm_check = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_chinese_script_setting(app: AppHandle, script: ChineseScript) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.chinese_script = script;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_app_language_setting(app: AppHandle, language: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.app_language = language.clone();
    settings::write_settings(&app, settings);

    // Refresh the tray menu with the new language
    tray::update_tray_menu(&app);

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_show_tray_icon_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.show_tray_icon = enabled;
    settings::write_settings(&app, settings);

    // Apply change immediately
    tray::set_tray_visibility(&app, enabled);

    Ok(())
}

/// Save accelerator settings and make the next model use reload with them.
/// The currently running transcription, if any, keeps its existing engine.
fn save_accelerator_and_reload_next_use(app: &AppHandle, s: settings::AppSettings) {
    settings::write_settings(app, s);

    let tm = app.state::<std::sync::Arc<crate::managers::transcription::TranscriptionManager>>();
    tm.reload_model_on_next_use();
}

#[tauri::command]
#[specta::specta]
pub fn change_transcribe_accelerator_setting(
    app: AppHandle,
    accelerator: settings::TranscribeAcceleratorSetting,
) -> Result<(), String> {
    let mut s = settings::get_settings(&app);
    s.transcribe_accelerator = accelerator;
    save_accelerator_and_reload_next_use(&app, s);
    app.state::<std::sync::Arc<crate::managers::transcription::TranscriptionManager>>()
        .retry_transcribe_gpu("the transcribe.cpp accelerator setting changed");
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_transcribe_gpu_device(app: AppHandle, device: Option<String>) -> Result<(), String> {
    let mut s = settings::get_settings(&app);
    s.transcribe_gpu_device = device;
    save_accelerator_and_reload_next_use(&app, s);
    app.state::<std::sync::Arc<crate::managers::transcription::TranscriptionManager>>()
        .retry_transcribe_gpu("the transcribe.cpp GPU device setting changed");
    Ok(())
}

/// Return which accelerators and GPU devices are available for this build.
///
/// First-call cost is dominated by enumerating GPU devices through the
/// transcribe.cpp Metal/Vulkan backend, which loads dynamic libraries and
/// probes hardware. Run it on the blocking pool so the webview thread
/// stays responsive — see also the startup pre-warm in `lib.rs`.
#[tauri::command]
#[specta::specta]
pub async fn get_available_accelerators(
    app: AppHandle,
) -> crate::managers::transcription::AvailableAccelerators {
    tauri::async_runtime::spawn_blocking(move || {
        let tm =
            app.state::<std::sync::Arc<crate::managers::transcription::TranscriptionManager>>();
        crate::managers::transcription::get_available_accelerators(&tm)
    })
    .await
    .expect("get_available_accelerators panicked")
}

#[cfg(test)]
mod tests {
    use handy_keys::Hotkey;
    use tauri_plugin_global_shortcut::Shortcut;

    // DanyWhere: OpenRouter's model list is public, so the list can load
    // before a key is entered; every other provider answers 401 without one.
    #[test]
    fn openrouter_lists_its_models_without_a_key_the_others_need_one() {
        assert!(!super::model_list_needs_api_key("custom"));
        assert!(!super::model_list_needs_api_key("openrouter"));
        for id in [
            "openai",
            "anthropic",
            "groq",
            "cerebras",
            "zai",
            "bedrock_mantle",
        ] {
            assert!(super::model_list_needs_api_key(id), "{id}");
        }
    }

    /// A shortcut action runs on the handy-keys manager thread, which also
    /// serves registrations. Work that registers a shortcut and waits for the
    /// answer must not run on that thread, or it waits for itself: on
    /// 2026-11-15 ⌃⌥A (choose the profile by application) froze every shortcut.
    #[test]
    fn work_that_registers_does_not_wait_for_the_handler_thread() {
        use std::sync::mpsc;
        use std::time::Duration;

        let (command_tx, command_rx) = mpsc::channel::<mpsc::Sender<()>>();
        let (done_tx, done_rx) = mpsc::channel::<()>();
        // The manager thread: one shortcut event, then it serves commands.
        let manager = std::thread::spawn(move || {
            super::off_handler_thread(move || {
                let (reply_tx, reply_rx) = mpsc::channel();
                command_tx.send(reply_tx).expect("manager gone");
                if reply_rx.recv_timeout(Duration::from_secs(5)).is_ok() {
                    let _ = done_tx.send(());
                }
            });
            if let Ok(reply) = command_rx.recv_timeout(Duration::from_secs(8)) {
                let _ = reply.send(());
            }
        });
        let done = done_rx.recv_timeout(Duration::from_secs(4));
        manager.join().expect("manager thread panicked");
        assert!(
            done.is_ok(),
            "the action's registration waited for the handler thread it runs on"
        );
    }

    #[test]
    fn compound_shortcut_keys_parse_on_both_backends() {
        for key in [
            "scrolllock",
            "capslock",
            "numlock",
            "pageup",
            "pagedown",
            "printscreen",
        ] {
            assert!(key.parse::<Shortcut>().is_ok(), "Tauri rejected {key}");
            assert!(key.parse::<Hotkey>().is_ok(), "HandyKeys rejected {key}");
        }
    }

    #[test]
    fn has_a_default_shortcut_to_paste_the_last_transcript() {
        let settings = crate::settings::get_default_settings();
        let binding = settings
            .bindings
            .get("paste_last_transcript")
            .expect("default binding 'paste_last_transcript'");
        // Must work with both backends, incl. the one used without Accessibility.
        let raw = &binding.current_binding;
        assert_eq!(super::tauri_impl::validate_shortcut(raw), Ok(()));
        assert!(super::handy_keys::validate_shortcut(raw).is_ok());
    }

    /// Older stores get it too: `get_settings` fills in every default binding
    /// a store doesn't have yet.
    #[test]
    fn has_a_default_shortcut_to_switch_word_lists() {
        let settings = crate::settings::get_default_settings();
        let binding = settings
            .bindings
            .get("cycle_word_list")
            .expect("default binding 'cycle_word_list'");
        // Must work with both backends, incl. the one used without Accessibility.
        let raw = &binding.current_binding;
        assert_eq!(super::tauri_impl::validate_shortcut(raw), Ok(()));
        assert!(super::handy_keys::validate_shortcut(raw).is_ok());
        assert!(
            crate::actions::ACTION_MAP.contains_key("cycle_word_list"),
            "the shortcut has an action"
        );
    }

    /// DanyWhere: turns choosing the profile by application on and off.
    #[test]
    fn has_a_default_shortcut_to_toggle_choosing_the_profile_by_app() {
        let settings = crate::settings::get_default_settings();
        let binding = settings
            .bindings
            .get(crate::app_profiles::TOGGLE_BINDING)
            .expect("default binding 'toggle_profile_auto'");
        #[cfg(target_os = "macos")]
        assert_eq!(binding.default_binding, "ctrl+shift+a");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(binding.default_binding, "alt+shift+a");
        let raw = &binding.current_binding;
        assert_eq!(super::tauri_impl::validate_shortcut(raw), Ok(()));
        assert!(super::handy_keys::validate_shortcut(raw).is_ok());
        assert!(
            crate::actions::ACTION_MAP.contains_key(crate::app_profiles::TOGGLE_BINDING),
            "the shortcut has an action"
        );
    }

    #[test]
    fn no_two_default_shortcuts_are_the_same() {
        let settings = crate::settings::get_default_settings();
        let mut seen = std::collections::HashMap::new();
        for binding in settings.bindings.values() {
            if let Some(other) = seen.insert(binding.default_binding.clone(), binding.id.clone()) {
                panic!(
                    "'{}' and '{}' share the default {}",
                    other, binding.id, binding.default_binding
                );
            }
        }
    }

    mod no_accessibility_mode {
        use super::super::apply_no_accessibility_mode;
        use crate::settings::{
            get_default_settings, AppSettings, ClipboardHandling, KeyboardImplementation,
            PasteMethod,
        };

        /// A user coming from the default macOS setup: handy-keys backend,
        /// simulated paste, and a start shortcut and a shortcut only
        /// handy-keys can register.
        fn handy_keys_user() -> AppSettings {
            let mut settings = get_default_settings();
            settings.keyboard_implementation = KeyboardImplementation::HandyKeys;
            settings.paste_method = PasteMethod::CtrlV;
            settings.clipboard_handling = ClipboardHandling::DontModify;
            settings.start_keys[0].shortcut = "fn+space".into();
            settings
                .bindings
                .get_mut("cycle_word_list")
                .unwrap()
                .current_binding = "fn+l".into();
            settings
        }

        #[test]
        fn marks_accessibility_as_skipped() {
            let mut settings = handy_keys_user();
            apply_no_accessibility_mode(&mut settings);
            assert!(settings.accessibility_skipped);
        }

        #[test]
        fn delivers_text_via_clipboard_instead_of_simulated_paste() {
            let mut settings = handy_keys_user();
            apply_no_accessibility_mode(&mut settings);
            assert_eq!(settings.paste_method, PasteMethod::None);
            assert_eq!(
                settings.clipboard_handling,
                ClipboardHandling::CopyToClipboard
            );
        }

        #[test]
        fn switches_to_the_shortcut_backend_that_needs_no_event_tap() {
            let mut settings = handy_keys_user();
            apply_no_accessibility_mode(&mut settings);
            assert_eq!(
                settings.keyboard_implementation,
                KeyboardImplementation::Tauri
            );
        }

        #[test]
        fn resets_bindings_the_tauri_backend_cannot_register() {
            let mut settings = handy_keys_user();
            let reset = apply_no_accessibility_mode(&mut settings);
            let defaults = get_default_settings();
            assert_eq!(
                settings.bindings["cycle_word_list"].current_binding,
                defaults.bindings["cycle_word_list"].current_binding
            );
            assert_eq!(settings.start_keys, defaults.start_keys);
            assert_eq!(
                reset,
                vec!["cycle_word_list".to_string(), "start_key:start".to_string()]
            );
        }

        /// The factory ends whose "Append" presses a key need Accessibility
        /// to send it, with the input method too: without it they go, the
        /// others stay in their order.
        fn without_key_presses(ends: &[crate::end_keys::EndKey]) -> Vec<crate::end_keys::EndKey> {
            ends.iter()
                .filter(|end| {
                    crate::append_text::for_value(&end.append_text)
                        .keys
                        .is_empty()
                })
                .cloned()
                .collect()
        }

        #[cfg(target_os = "macos")]
        fn ids(ends: &[crate::end_keys::EndKey]) -> Vec<&str> {
            ends.iter().map(|end| end.id.as_str()).collect()
        }

        #[test]
        fn drops_the_factory_ends_that_press_a_key() {
            let mut settings = get_default_settings();
            let factory = settings.end_keys.clone();
            apply_no_accessibility_mode(&mut settings);
            assert_eq!(settings.end_keys, without_key_presses(&factory));
            #[cfg(target_os = "macos")]
            assert_eq!(
                ids(&settings.end_keys),
                [
                    "direct_space",
                    "direct_no_space",
                    "direct_paragraph",
                    "direct_line",
                ]
            );
        }

        #[test]
        fn the_factory_ends_left_without_accessibility_suit_its_backend() {
            let mut settings = get_default_settings();
            apply_no_accessibility_mode(&mut settings);
            for end in &settings.end_keys {
                assert_eq!(
                    super::super::validate_shortcut_for_implementation(
                        &end.shortcut,
                        KeyboardImplementation::Tauri,
                    ),
                    Ok(()),
                    "{}",
                    end.shortcut
                );
            }
        }

        /// The names don't make the list the user's: they are written in the
        /// language of the first start, which may not be the one in use
        /// now, and a typed name changes nothing an end does.
        #[test]
        fn drops_them_whatever_the_factory_ends_are_named() {
            let mut settings = get_default_settings();
            for end in settings.end_keys.iter_mut() {
                end.name = format!("Renamed {}", end.id);
            }
            let named = settings.end_keys.clone();
            apply_no_accessibility_mode(&mut settings);
            assert_eq!(settings.end_keys, without_key_presses(&named));
            #[cfg(target_os = "macos")]
            assert_eq!(settings.end_keys.len(), 4);
        }

        #[test]
        fn keeps_a_users_own_end_shortcuts_with_their_key_presses() {
            let mut settings = get_default_settings();
            settings.end_keys[0].append_text = "{Enter}".into();
            let own = settings.end_keys.clone();
            apply_no_accessibility_mode(&mut settings);
            assert_eq!(settings.end_keys, own);
        }

        /// Onboarding's "Continue without Accessibility" writes the values
        /// in use, which go to the defaults (`profile_layers::route_flat`)
        /// and on to every profile that follows them.
        #[test]
        fn the_defaults_and_the_profiles_following_them_lose_them_too() {
            let mut settings = get_default_settings();
            crate::profiles::ensure_profiles(&mut settings);
            crate::profiles::duplicate(
                &mut settings,
                crate::profiles::DEFAULT_PROFILE_ID,
                "Work",
                1_795_500_000_001,
            )
            .expect("duplicated");
            let factory = settings.end_keys.clone();
            apply_no_accessibility_mode(&mut settings);
            crate::profile_layers::route_flat(&mut settings);
            let expected = without_key_presses(&factory);
            assert_eq!(settings.end_keys, expected);
            assert_eq!(
                settings
                    .default_values
                    .as_ref()
                    .map(|values| &values.end_keys),
                Some(&expected)
            );
            assert_eq!(settings.profiles.len(), 1);
            assert_eq!(settings.profiles[0].values.end_keys, expected);
            #[cfg(target_os = "macos")]
            assert_eq!(expected.len(), 4);
        }

        #[test]
        fn keeps_bindings_the_tauri_backend_can_register() {
            let mut settings = handy_keys_user();
            settings.start_keys[0].shortcut = "ctrl+shift+d".into();
            settings
                .bindings
                .get_mut("cycle_word_list")
                .unwrap()
                .current_binding = "ctrl+shift+l".into();
            let reset = apply_no_accessibility_mode(&mut settings);
            assert_eq!(settings.start_keys[0].shortcut, "ctrl+shift+d");
            assert!(reset.is_empty());
        }
    }
}
