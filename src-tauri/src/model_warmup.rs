//! DanyWhere: loads a post-processing model that runs on this computer
//! (Ollama, LM Studio, llama-server …) when a dictation starts, so that its
//! cold start falls into the speaking time instead of coming after it.
//!
//! A local server unloads an idle model after a while (Ollama: 5 min), and
//! loading a 12B model again took 31 s on an M1 Pro. A tiny request at the
//! start of the recording has the server load it while the user speaks.
//! The request runs on its own, never holds up the recording, and its errors
//! only go to the log.

use crate::settings::{AppSettings, PostProcessProvider, APPLE_INTELLIGENCE_PROVIDER_ID};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// How long a warm-up may take: loading a large model from disk can take
/// well over half a minute.
const WARMUP_TIME_LIMIT: Duration = Duration::from_secs(180);

/// One warm-up at a time; a dictation started while one runs needs no other.
static IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// Whether `base_url` points at this computer: `localhost` or a loopback
/// address (`127.0.0.0/8`, `::1`). Anything else, including an address in
/// the local network, counts as leaving the computer.
pub fn is_local_base_url(base_url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(base_url.trim()) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    // An IPv6 host comes in brackets: `[::1]`.
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(address) = host.parse::<std::net::IpAddr>() {
        return address.is_loopback();
    }
    host.trim_end_matches('.').eq_ignore_ascii_case("localhost")
}

/// What a warm-up loads: the provider, its model and its key (no `Debug`,
/// so the key can't end up in a log line).
#[derive(Clone)]
pub struct Warmup {
    pub provider: PostProcessProvider,
    pub model: String,
    pub api_key: String,
}

/// The warm-up a dictation needs: one when it post-processes (`post_process`,
/// as its profile's mode expects, `dictation_start::plan_start`) and the
/// profile may (not "never") with a provider on this computer that has a
/// model.
pub fn warmup_for(settings: &AppSettings, post_process: bool) -> Option<Warmup> {
    if !post_process || !settings.post_process_mode.allows() {
        return None;
    }
    let provider = settings.active_post_process_provider()?;
    // Apple Intelligence runs in this process, with nothing to load.
    if provider.id == APPLE_INTELLIGENCE_PROVIDER_ID || !is_local_base_url(&provider.base_url) {
        return None;
    }
    let model = settings.post_process_models.get(&provider.id)?;
    if model.trim().is_empty() {
        return None;
    }
    Some(Warmup {
        provider: provider.clone(),
        model: model.clone(),
        api_key: settings
            .post_process_api_keys
            .get(&provider.id)
            .cloned()
            .unwrap_or_default(),
    })
}

/// Starts the warm-up for a dictation that begins now, if it needs one.
pub fn start(settings: &AppSettings, post_process: bool) {
    let Some(warmup) = warmup_for(settings, post_process) else {
        return;
    };
    if IN_FLIGHT.swap(true, Ordering::SeqCst) {
        log::debug!("A warm-up of the post-processing model is already running");
        return;
    }
    log::info!(
        "Warming up the local post-processing model '{}' ({})",
        warmup.model,
        warmup.provider.id
    );
    tauri::async_runtime::spawn(async move {
        let started = Instant::now();
        match crate::llm_client::send_warmup(
            &warmup.provider,
            &warmup.api_key,
            &warmup.model,
            WARMUP_TIME_LIMIT,
        )
        .await
        {
            Ok(()) => log::info!(
                "Local post-processing model '{}' is ready after {:?}",
                warmup.model,
                started.elapsed()
            ),
            Err(error) => log::warn!(
                "Warming up the local post-processing model '{}' failed after {:?}: {}",
                warmup.model,
                started.elapsed(),
                error
            ),
        }
        IN_FLIGHT.store(false, Ordering::SeqCst);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::get_default_settings;

    #[test]
    fn localhost_and_loopback_addresses_are_on_this_computer() {
        for url in [
            "http://localhost:11434/v1",
            "http://LocalHost:1234/v1/",
            "http://127.0.0.1:1234/v1",
            "http://127.0.0.2:8080",
            "http://[::1]:8080/v1",
            "https://localhost/v1",
            "  http://localhost:11434/v1  ",
        ] {
            assert!(is_local_base_url(url), "{url} should be local");
        }
    }

    #[test]
    fn every_other_address_leaves_this_computer() {
        for url in [
            "https://api.openai.com/v1",
            "https://api.anthropic.com/v1",
            "http://192.168.1.10:11434/v1",
            "http://10.0.0.1:1234/v1",
            "http://localhost.example.com/v1",
            "http://example.com/localhost",
            "http://127.0.0.1.example.com/v1",
            "apple-intelligence://local",
            "localhost:11434",
            "not a url",
            "",
        ] {
            assert!(!is_local_base_url(url), "{url} should not be local");
        }
    }

    /// Default settings with post-processing on, the custom provider at
    /// `base_url` and `model` chosen for it.
    fn custom_settings(base_url: &str, model: &str) -> AppSettings {
        let mut settings = get_default_settings();
        settings.post_process_mode = crate::settings::PostProcessMode::Always;
        settings.post_process_provider_id = "custom".to_string();
        settings
            .post_process_provider_mut("custom")
            .expect("the custom provider exists")
            .base_url = base_url.to_string();
        settings
            .post_process_models
            .insert("custom".to_string(), model.to_string());
        settings
    }

    #[test]
    fn a_post_processing_dictation_warms_up_a_model_on_this_computer() {
        let settings = custom_settings("http://localhost:11434/v1", "gemma4:12b");
        let warmup = warmup_for(&settings, true).expect("a warm-up");
        assert_eq!(warmup.provider.id, "custom");
        assert_eq!(warmup.provider.base_url, "http://localhost:11434/v1");
        assert_eq!(warmup.model, "gemma4:12b");
    }

    #[test]
    fn the_warmup_carries_the_providers_key() {
        let mut settings = custom_settings("http://127.0.0.1:1234/v1", "local-model");
        settings
            .post_process_api_keys
            .insert("custom".to_string(), "lm-studio-key".to_string());
        let warmup = warmup_for(&settings, true).expect("a warm-up");
        assert_eq!(warmup.api_key, "lm-studio-key");
    }

    #[test]
    fn a_dictation_without_post_processing_warms_up_nothing() {
        let settings = custom_settings("http://localhost:11434/v1", "gemma4:12b");
        assert!(warmup_for(&settings, false).is_none());

        let mut switched_off = settings;
        switched_off.post_process_mode = crate::settings::PostProcessMode::Never;
        assert!(warmup_for(&switched_off, true).is_none());
    }

    #[test]
    fn a_profile_that_never_post_processes_warms_up_nothing() {
        let mut settings = custom_settings("http://localhost:11434/v1", "gemma4:12b");
        settings.post_process_mode = crate::settings::PostProcessMode::Never;
        assert!(warmup_for(&settings, true).is_none());
        settings.post_process_mode = crate::settings::PostProcessMode::WithInstructionKey;
        assert!(warmup_for(&settings, true).is_some());
    }

    #[test]
    fn a_provider_in_the_cloud_is_never_warmed_up() {
        let mut settings = custom_settings("https://my-server.example.com/v1", "gemma4:12b");
        assert!(warmup_for(&settings, true).is_none());

        settings.post_process_provider_id = "openai".to_string();
        settings
            .post_process_models
            .insert("openai".to_string(), "gpt-5-mini".to_string());
        assert!(warmup_for(&settings, true).is_none());
    }

    #[test]
    fn a_local_provider_without_a_model_is_not_warmed_up() {
        assert!(warmup_for(&custom_settings("http://localhost:11434/v1", ""), true).is_none());
        assert!(warmup_for(&custom_settings("http://localhost:11434/v1", "  "), true).is_none());
    }

    #[test]
    fn apple_intelligence_is_not_warmed_up() {
        let mut settings = custom_settings("http://localhost:11434/v1", "gemma4:12b");
        settings.post_process_provider_id = APPLE_INTELLIGENCE_PROVIDER_ID.to_string();
        settings
            .post_process_models
            .insert(APPLE_INTELLIGENCE_PROVIDER_ID.to_string(), "0".to_string());
        assert!(warmup_for(&settings, true).is_none());
    }
}
