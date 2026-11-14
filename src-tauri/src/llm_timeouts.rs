//! DanyWhere: how long DanyWhere waits for a post-processing provider.
//!
//! Handy bounds two waits with fixed values: the whole post-processing step
//! (60 s, retries included) and the request for a provider's model list
//! (15 s). The HTTP client itself only bounds connecting, so without them an
//! endpoint that accepts a request and never answers holds the dictation or
//! the model picker forever. DanyWhere keeps Handy's values as defaults and
//! lets the user change them under Advanced.

use crate::settings::{get_settings, write_settings, AppSettings};
use std::time::Duration;
use tauri::AppHandle;

/// The shortest time limit, in seconds. There is deliberately no 0 for "no
/// limit": that is the hang the limits exist to prevent.
pub const MIN_SECS: u64 = 5;
/// The longest time limit, in seconds.
pub const MAX_SECS: u64 = 600;

/// Handy's limit for the whole post-processing step.
pub fn default_post_process_timeout_secs() -> u64 {
    60
}

/// Handy's limit for the model list request.
pub fn default_model_list_timeout_secs() -> u64 {
    15
}

/// How long post-processing may take, retries included.
pub fn post_process_timeout(settings: &AppSettings) -> Duration {
    within_range(settings.post_process_timeout_secs)
}

/// How long the model list request may take.
pub fn model_list_timeout(settings: &AppSettings) -> Duration {
    within_range(settings.model_list_timeout_secs)
}

/// A value outside the range, which only a hand-edited settings file can
/// hold, counts as the nearest limit.
fn within_range(secs: u64) -> Duration {
    Duration::from_secs(secs.clamp(MIN_SECS, MAX_SECS))
}

/// Accepts whole seconds from [`MIN_SECS`] to [`MAX_SECS`].
fn check_range(secs: u64) -> Result<(), String> {
    if (MIN_SECS..=MAX_SECS).contains(&secs) {
        Ok(())
    } else {
        Err(format!(
            "A time limit must be {MIN_SECS} to {MAX_SECS} seconds, not {secs}"
        ))
    }
}

/// Sets the time limit for post-processing, in whole seconds.
#[tauri::command]
#[specta::specta]
pub fn change_post_process_timeout_setting(app: AppHandle, secs: u64) -> Result<(), String> {
    check_range(secs)?;
    let mut settings = get_settings(&app);
    settings.post_process_timeout_secs = secs;
    write_settings(&app, settings);
    Ok(())
}

/// Sets the time limit for the model list request, in whole seconds.
#[tauri::command]
#[specta::specta]
pub fn change_model_list_timeout_setting(app: AppHandle, secs: u64) -> Result<(), String> {
    check_range(secs)?;
    let mut settings = get_settings(&app);
    settings.model_list_timeout_secs = secs;
    write_settings(&app, settings);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::get_default_settings;

    #[test]
    fn settings_from_before_the_time_limits_get_handys_values() {
        let mut stored = serde_json::to_value(get_default_settings()).unwrap();
        let object = stored.as_object_mut().unwrap();
        object.remove("post_process_timeout_secs");
        object.remove("model_list_timeout_secs");

        let settings: AppSettings = serde_json::from_value(stored).unwrap();

        assert_eq!(settings.post_process_timeout_secs, 60);
        assert_eq!(settings.model_list_timeout_secs, 15);
        let fresh = get_default_settings();
        assert_eq!(fresh.post_process_timeout_secs, 60);
        assert_eq!(fresh.model_list_timeout_secs, 15);
    }

    #[test]
    fn the_time_limits_come_from_the_settings() {
        let mut settings = get_default_settings();
        settings.post_process_timeout_secs = 90;
        settings.model_list_timeout_secs = 30;

        assert_eq!(post_process_timeout(&settings), Duration::from_secs(90));
        assert_eq!(model_list_timeout(&settings), Duration::from_secs(30));
    }

    #[test]
    fn a_value_outside_the_range_counts_as_the_nearest_limit() {
        // Only a hand-edited settings file can hold one.
        let mut settings = get_default_settings();
        settings.post_process_timeout_secs = 0;
        settings.model_list_timeout_secs = 100_000;

        assert_eq!(post_process_timeout(&settings), Duration::from_secs(5));
        assert_eq!(model_list_timeout(&settings), Duration::from_secs(600));
    }

    #[test]
    fn the_settings_take_whole_seconds_from_5_to_600() {
        for secs in [MIN_SECS, 60, MAX_SECS] {
            assert!(check_range(secs).is_ok(), "{secs} s");
        }
        for secs in [0, MIN_SECS - 1, MAX_SECS + 1] {
            assert!(check_range(secs).is_err(), "{secs} s");
        }
    }
}
