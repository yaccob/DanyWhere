//! DanyWhere: the effort of post-processing, i.e. how much the model thinks
//! before it answers. Chosen per profile next to the model, one value per
//! provider like the model (`post_process_efforts`); none stored means the
//! model's default, and then the request carries no new field.
//!
//! Which levels a model takes comes, in this order, from its provider's model
//! list (Anthropic `capabilities.effort`, OpenRouter `reasoning`, DeepSeek
//! `effort`; remembered while the app runs), from what the providers document
//! per model family (as of 2026-11-15), or, when neither says anything, the
//! common levels low/medium/high. An endpoint that refuses a level gets the
//! request again without it (`llm_client`), so a wrong guess never costs a
//! dictation.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use tauri::AppHandle;

use crate::settings::{
    self, AppSettings, PostProcessApiFormat, PostProcessProvider, APPLE_INTELLIGENCE_PROVIDER_ID,
};

/// Every level DanyWhere knows, from least to most effort. Providers name
/// them alike; each takes some of them.
pub const LEVELS: [&str; 7] = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];

/// Offered when nothing tells which levels a model takes.
const COMMON_LEVELS: [&str; 3] = ["low", "medium", "high"];

/// Where the offered levels come from.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum EffortSource {
    /// The provider's model list names them for this model.
    Listed,
    /// The provider's documentation names them for this model family.
    Documented,
    /// Nothing is known; the common levels, which the endpoint may refuse.
    Common,
}

/// What a model takes, as far as DanyWhere knows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModelEfforts {
    /// In [`LEVELS`] order; empty when the model takes no effort at all.
    pub levels: Vec<String>,
    /// Anthropic: the model thinks only when asked to (`thinking: adaptive`),
    /// which an effort should then ask for.
    pub adaptive_thinking: bool,
}

/// What the settings page offers for a provider's model.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct EffortOptions {
    /// In [`LEVELS`] order, without the model's default (always offered).
    pub levels: Vec<String>,
    pub source: EffortSource,
    /// Levels the endpoint refused for this model while the app runs.
    pub rejected: Vec<String>,
}

/// `levels` in [`LEVELS`] order, without unknown names and repeats.
fn ordered<I, S>(levels: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let given: Vec<String> = levels
        .into_iter()
        .map(|level| level.as_ref().trim().to_lowercase())
        .collect();
    LEVELS
        .iter()
        .filter(|level| given.iter().any(|g| g == *level))
        .map(|level| level.to_string())
        .collect()
}

fn efforts(levels: &[&str], adaptive_thinking: bool) -> ModelEfforts {
    ModelEfforts {
        levels: ordered(levels),
        adaptive_thinking,
    }
}

fn supported(node: Option<&Value>) -> bool {
    node.and_then(|node| node.get("supported"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn string_list(node: &Value) -> Option<Vec<String>> {
    node.as_array().map(|items| {
        items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect()
    })
}

/// What a model list entry says about effort, if anything:
/// - Anthropic: `capabilities.effort.<level>.supported` and
///   `capabilities.thinking.types.adaptive.supported`
/// - OpenRouter: `reasoning.supported_efforts` (`null` = no restriction,
///   `mandatory` = can't be switched off); no `reasoning` object and no
///   "reasoning" in `supported_parameters` = no reasoning
/// - DeepSeek: `effort.supported_levels`
pub fn from_listing(entry: &Value) -> Option<ModelEfforts> {
    if let Some(capabilities) = entry.get("capabilities") {
        let effort = capabilities.get("effort")?;
        let adaptive_thinking = supported(
            capabilities
                .get("thinking")
                .and_then(|thinking| thinking.get("types"))
                .and_then(|types| types.get("adaptive")),
        );
        if !supported(Some(effort)) {
            return Some(ModelEfforts {
                levels: Vec::new(),
                adaptive_thinking,
            });
        }
        let levels = LEVELS
            .iter()
            .filter(|level| supported(effort.get(**level)))
            .map(|level| level.to_string())
            .collect();
        return Some(ModelEfforts {
            levels,
            adaptive_thinking,
        });
    }
    if let Some(parameters) = entry.get("supported_parameters") {
        let Some(reasoning) = entry.get("reasoning").filter(|r| r.is_object()) else {
            let takes_reasoning = string_list(parameters)
                .unwrap_or_default()
                .iter()
                .any(|p| p == "reasoning" || p == "reasoning_effort");
            // OpenRouter's router (openrouter/auto) decides per request.
            return (!takes_reasoning).then(ModelEfforts::default);
        };
        let mandatory = reasoning
            .get("mandatory")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut levels = match reasoning.get("supported_efforts").and_then(string_list) {
            Some(listed) => ordered(listed),
            None => ordered(LEVELS),
        };
        if mandatory {
            levels.retain(|level| level != "none");
        }
        return Some(ModelEfforts {
            levels,
            adaptive_thinking: false,
        });
    }
    let levels = entry
        .get("effort")
        .and_then(|effort| effort.get("supported_levels"))
        .and_then(string_list)?;
    Some(ModelEfforts {
        levels: ordered(levels),
        adaptive_thinking: false,
    })
}

/// Model list entries per endpoint and model (`llm_client::endpoint_key`).
fn listed() -> &'static Mutex<HashMap<String, ModelEfforts>> {
    static LISTED: OnceLock<Mutex<HashMap<String, ModelEfforts>>> = OnceLock::new();
    LISTED.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Remembers what a model list (`GET /models`, already parsed) says about
/// each model's effort, until the app quits.
pub fn remember_listing(provider: &PostProcessProvider, listing: &Value) {
    let Some(entries) = listing.get("data").and_then(Value::as_array) else {
        return;
    };
    let Ok(mut known) = listed().lock() else {
        return;
    };
    for entry in entries {
        let Some(id) = entry
            .get("id")
            .or_else(|| entry.get("name"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let key = crate::llm_client::endpoint_key(provider, id);
        match from_listing(entry) {
            Some(found) => {
                known.insert(key, found);
            }
            None => {
                known.remove(&key);
            }
        }
    }
}

fn listed_for(provider: &PostProcessProvider, model: &str) -> Option<ModelEfforts> {
    listed()
        .lock()
        .ok()?
        .get(&crate::llm_client::endpoint_key(provider, model))
        .cloned()
}

/// Anthropic's models by what the Models API reported on 2026-11-15
/// (`capabilities.effort`; tools/dev/anthropic-model-effort.py in the
/// sidecar). Matched anywhere in the id, so Bedrock's or Vertex's names and a
/// proxy's prefix match too. The 4.6 and later models think only on request
/// unless they are Claude 5 models; asking for adaptive thinking is the same
/// as the default on these, so every model that takes it gets it.
fn documented_anthropic(model: &str) -> Option<ModelEfforts> {
    const ALL: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
    const FAMILIES: [(&str, &[&str], bool); 13] = [
        ("claude-haiku-4-5", &[], false),
        ("claude-sonnet-4-5", &[], false),
        ("claude-opus-4-5", &["low", "medium", "high"], false),
        ("claude-opus-4-6", &["low", "medium", "high", "max"], true),
        ("claude-sonnet-4-6", &["low", "medium", "high", "max"], true),
        ("claude-opus-4-7", &ALL, true),
        ("claude-opus-4-8", &ALL, true),
        ("claude-fable-5", &ALL, true),
        ("claude-mythos-5", &ALL, true),
        ("claude-opus-5", &ALL, true),
        ("claude-sonnet-5", &ALL, true),
        ("claude-haiku-5", &ALL, true),
        ("claude-mythos-preview", &ALL, true),
    ];
    FAMILIES
        .iter()
        .find(|(family, _, _)| model.contains(family))
        .map(|(_, levels, adaptive)| efforts(levels, *adaptive))
}

/// OpenAI's model pages (developers.openai.com, 2026-11-15): each gpt-5.x
/// release names its own levels; o-series take low to high; gpt-4.x and
/// older don't reason.
fn documented_openai(model: &str) -> Option<ModelEfforts> {
    let model = model.strip_prefix("openai/").unwrap_or(model);
    let levels: &[&str] = if model.starts_with("gpt-5.5") || model.starts_with("gpt-5.2") {
        &["none", "low", "medium", "high", "xhigh"]
    } else if model.starts_with("gpt-5.1") {
        &["none", "low", "medium", "high"]
    } else if model.starts_with("gpt-5.") {
        // A release the documentation didn't name yet.
        return None;
    } else if model.starts_with("gpt-5") {
        &["minimal", "low", "medium", "high"]
    } else if ["o1", "o3", "o4"].iter().any(|o| model.starts_with(o)) {
        &["low", "medium", "high"]
    } else if ["gpt-4", "gpt-3.5", "chatgpt-4"]
        .iter()
        .any(|old| model.starts_with(old))
    {
        &[]
    } else {
        return None;
    };
    Some(efforts(levels, false))
}

/// Groq, Cerebras and Z.AI document levels for some families (2026-11-15);
/// they refuse others with 400, which the retry handles.
fn documented_other(provider_id: &str, model: &str) -> Option<ModelEfforts> {
    let levels: &[&str] = match provider_id {
        "groq" if model.contains("gpt-oss") => &["low", "medium", "high"],
        "groq" if model.contains("qwen3") || model.contains("qwen-3") => {
            &["none", "low", "medium", "high"]
        }
        "cerebras" if model.contains("gpt-oss") => &["low", "medium", "high"],
        "cerebras" if model.starts_with("zai-glm") => &["low", "high", "max"],
        "cerebras" if model.starts_with("qwen-3") => &["none", "low", "medium", "high"],
        "zai" if model.starts_with("glm-5.3") => &["low", "high", "max"],
        "zai" if model.starts_with("glm-5.2") => &LEVELS,
        _ => return None,
    };
    Some(efforts(levels, false))
}

/// What the provider documents for the model's family.
pub fn documented(provider: &PostProcessProvider, model: &str) -> Option<ModelEfforts> {
    let model = model.trim().to_lowercase();
    if provider.id == APPLE_INTELLIGENCE_PROVIDER_ID {
        return Some(ModelEfforts::default());
    }
    if provider.api_format == PostProcessApiFormat::Anthropic {
        return documented_anthropic(&model);
    }
    if provider
        .base_url
        .to_lowercase()
        .contains("api.deepseek.com")
    {
        // DeepSeek's guide: low, high and max (others are mapped to these).
        return Some(efforts(&["low", "high", "max"], false));
    }
    match provider.id.as_str() {
        "openai" => documented_openai(&model),
        // OpenRouter takes every level and maps it to the nearest one the
        // model has; its model list says more, once loaded.
        "openrouter" => Some(efforts(&LEVELS, false)),
        id => documented_other(id, &model),
    }
}

/// What is known about the model, and from where; `None` when nothing is.
pub fn known(provider: &PostProcessProvider, model: &str) -> Option<(ModelEfforts, EffortSource)> {
    if let Some(found) = listed_for(provider, model) {
        return Some((found, EffortSource::Listed));
    }
    documented(provider, model).map(|found| (found, EffortSource::Documented))
}

/// Whether an effort for this Anthropic-format model should also ask for
/// adaptive thinking.
pub fn wants_adaptive_thinking(provider: &PostProcessProvider, model: &str) -> bool {
    known(provider, model).is_some_and(|(found, _)| found.adaptive_thinking)
}

/// The levels the settings page offers for `model` of `provider`.
pub fn options(provider: &PostProcessProvider, model: &str) -> EffortOptions {
    let (levels, source) = match known(provider, model) {
        Some((found, source)) => (found.levels, source),
        None => (ordered(COMMON_LEVELS), EffortSource::Common),
    };
    EffortOptions {
        levels,
        source,
        rejected: crate::llm_client::rejected_efforts(provider, model),
    }
}

/// The effort stored for `provider_id`, if it is one DanyWhere knows.
pub fn stored(settings: &AppSettings, provider_id: &str) -> Option<String> {
    settings
        .post_process_efforts
        .get(provider_id)
        .map(|effort| effort.trim().to_lowercase())
        .filter(|effort| LEVELS.contains(&effort.as_str()))
}

/// The effort a request for `model` carries: the stored one, unless the
/// model is known not to take it (e.g. after a profile import), in which case
/// the settings page shows the model's default, and so the request is sent.
pub fn for_request(
    settings: &AppSettings,
    provider: &PostProcessProvider,
    model: &str,
) -> Option<String> {
    let effort = stored(settings, &provider.id)?;
    match known(provider, model) {
        Some((found, _)) if !found.levels.contains(&effort) => {
            log::info!("The model '{model}' doesn't take the effort '{effort}'; sending none");
            None
        }
        _ => Some(effort),
    }
}

/// Stores `effort` for `provider_id`; `None` or an empty one goes back to
/// the model's default.
pub fn set(
    settings: &mut AppSettings,
    provider_id: &str,
    effort: Option<&str>,
) -> Result<(), String> {
    match effort.map(str::trim).filter(|effort| !effort.is_empty()) {
        None => {
            settings.post_process_efforts.remove(provider_id);
        }
        Some(effort) => {
            let effort = effort.to_lowercase();
            if !LEVELS.contains(&effort.as_str()) {
                return Err(format!("Unknown effort '{effort}'"));
            }
            settings
                .post_process_efforts
                .insert(provider_id.to_string(), effort);
        }
    }
    Ok(())
}

/// After the provider's model changed: drops a stored effort the new model is
/// known not to take, so the choice shows the model's default again. Returns
/// whether it dropped one.
pub fn drop_if_unsupported(settings: &mut AppSettings, provider_id: &str) -> bool {
    let Some(effort) = stored(settings, provider_id) else {
        return false;
    };
    let model = settings
        .post_process_models
        .get(provider_id)
        .map(|model| model.trim().to_string())
        .unwrap_or_default();
    let takes_it = if model.is_empty() {
        false
    } else {
        match find_provider(settings, provider_id) {
            Ok(provider) => {
                known(provider, &model).is_none_or(|(found, _)| found.levels.contains(&effort))
            }
            Err(_) => true,
        }
    };
    if takes_it {
        return false;
    }
    log::info!("The model '{model}' doesn't take the effort '{effort}'; back to its default");
    settings.post_process_efforts.remove(provider_id);
    true
}

fn find_provider<'a>(
    settings: &'a AppSettings,
    provider_id: &str,
) -> Result<&'a PostProcessProvider, String> {
    settings
        .post_process_providers
        .iter()
        .find(|provider| provider.id == provider_id)
        .ok_or_else(|| format!("Provider '{provider_id}' not found"))
}

#[tauri::command]
#[specta::specta]
pub fn get_post_process_effort_options(
    app: AppHandle,
    provider_id: String,
    model: String,
) -> Result<EffortOptions, String> {
    let settings = settings::get_settings(&app);
    let provider = find_provider(&settings, &provider_id)?;
    Ok(options(provider, model.trim()))
}

#[tauri::command]
#[specta::specta]
pub fn change_post_process_effort_setting(
    app: AppHandle,
    provider_id: String,
    effort: Option<String>,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    find_provider(&settings, &provider_id)?;
    set(&mut settings, &provider_id, effort.as_deref())?;
    settings::write_settings(&app, settings);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider(id: &str, base_url: &str) -> PostProcessProvider {
        PostProcessProvider {
            id: id.to_string(),
            label: id.to_string(),
            base_url: base_url.to_string(),
            allow_base_url_edit: true,
            models_endpoint: None,
            supports_structured_output: false,
            api_format: if id == "anthropic" {
                PostProcessApiFormat::Anthropic
            } else {
                PostProcessApiFormat::OpenAi
            },
        }
    }

    fn levels(found: Option<ModelEfforts>) -> Option<Vec<String>> {
        found.map(|found| found.levels)
    }

    fn names(levels: &[&str]) -> Option<Vec<String>> {
        Some(levels.iter().map(|level| level.to_string()).collect())
    }

    fn anthropic_entry(id: &str, supported: &[&str], adaptive: bool) -> Value {
        let mut effort = json!({ "supported": !supported.is_empty() });
        for level in ["low", "medium", "high", "xhigh", "max"] {
            effort[level] = json!({ "supported": supported.contains(&level) });
        }
        json!({
            "id": id,
            "type": "model",
            "capabilities": {
                "effort": effort,
                "thinking": {
                    "supported": true,
                    "types": {
                        "adaptive": { "supported": adaptive },
                        "enabled": { "supported": !adaptive }
                    }
                }
            }
        })
    }

    #[test]
    fn the_anthropic_model_list_names_the_levels_and_adaptive_thinking() {
        let entry = anthropic_entry("claude-sonnet-4-6", &["low", "medium", "high", "max"], true);
        assert_eq!(
            from_listing(&entry),
            Some(ModelEfforts {
                levels: names(&["low", "medium", "high", "max"]).unwrap(),
                adaptive_thinking: true,
            })
        );
        // Older listings have no xhigh entry at all.
        let mut entry = anthropic_entry("claude-opus-4-5", &["low", "medium", "high"], false);
        entry["capabilities"]["effort"]
            .as_object_mut()
            .unwrap()
            .remove("xhigh");
        assert_eq!(
            from_listing(&entry),
            Some(ModelEfforts {
                levels: names(&["low", "medium", "high"]).unwrap(),
                adaptive_thinking: false,
            })
        );
    }

    #[test]
    fn an_anthropic_model_without_effort_takes_no_level() {
        let entry = anthropic_entry("claude-haiku-4-5", &[], false);
        assert_eq!(levels(from_listing(&entry)), Some(Vec::new()));
    }

    #[test]
    fn the_openrouter_model_list_names_the_levels_in_order() {
        let entry = json!({
            "id": "openai/gpt-5",
            "supported_parameters": ["reasoning", "include_reasoning", "reasoning_effort"],
            "reasoning": {
                "mandatory": true,
                "supported_efforts": ["high", "medium", "low", "minimal"],
                "default_effort": "medium"
            }
        });
        assert_eq!(
            levels(from_listing(&entry)),
            names(&["minimal", "low", "medium", "high"])
        );
    }

    #[test]
    fn openrouter_without_a_restriction_offers_every_level() {
        let entry = json!({
            "id": "anthropic/claude-haiku-4.5",
            "supported_parameters": ["reasoning", "include_reasoning"],
            "reasoning": { "mandatory": false }
        });
        assert_eq!(levels(from_listing(&entry)), names(&LEVELS));
        // Reasoning that can't be switched off has no "none".
        let entry = json!({
            "id": "some/model",
            "supported_parameters": ["reasoning"],
            "reasoning": { "mandatory": true, "supported_efforts": null }
        });
        assert_eq!(
            levels(from_listing(&entry)),
            names(&["minimal", "low", "medium", "high", "xhigh", "max"])
        );
    }

    #[test]
    fn an_openrouter_model_without_reasoning_takes_no_level() {
        let entry = json!({
            "id": "openai/gpt-4o-mini",
            "supported_parameters": ["max_tokens", "temperature"]
        });
        assert_eq!(levels(from_listing(&entry)), Some(Vec::new()));
        // OpenRouter's router decides per request: nothing is known.
        let entry = json!({
            "id": "openrouter/auto",
            "supported_parameters": ["reasoning", "reasoning_effort"]
        });
        assert_eq!(from_listing(&entry), None);
    }

    #[test]
    fn the_deepseek_model_list_names_its_levels() {
        let entry = json!({
            "id": "deepseek-chat",
            "effort": { "supported_levels": ["low", "high", "max"], "default_level": "high" }
        });
        assert_eq!(levels(from_listing(&entry)), names(&["low", "high", "max"]));
    }

    #[test]
    fn a_plain_model_list_says_nothing() {
        assert_eq!(
            from_listing(&json!({ "id": "gpt-4o", "object": "model" })),
            None
        );
        assert_eq!(from_listing(&json!("llama3")), None);
    }

    #[test]
    fn anthropic_levels_follow_what_the_models_api_reported() {
        let anthropic = provider("anthropic", "https://api.anthropic.com/v1");
        let all = names(&["low", "medium", "high", "xhigh", "max"]);
        for model in [
            "claude-fable-5-2",
            "claude-opus-5-5",
            "claude-opus-4-8",
            "claude-opus-4-7",
            "claude-sonnet-5-5",
            "claude-sonnet-5",
            "claude-haiku-5-5",
        ] {
            let found = documented(&anthropic, model);
            assert_eq!(levels(found.clone()), all, "{model}");
            assert!(found.unwrap().adaptive_thinking, "{model}");
        }
        for model in ["claude-sonnet-4-6", "claude-opus-4-6"] {
            assert_eq!(
                levels(documented(&anthropic, model)),
                names(&["low", "medium", "high", "max"]),
                "{model}"
            );
        }
        let opus_4_5 = documented(&anthropic, "claude-opus-4-5-20251101").unwrap();
        assert_eq!(opus_4_5.levels, names(&["low", "medium", "high"]).unwrap());
        assert!(!opus_4_5.adaptive_thinking);
        for model in ["claude-haiku-4-5", "claude-sonnet-4-5-20250929"] {
            assert_eq!(
                levels(documented(&anthropic, model)),
                Some(Vec::new()),
                "{model}"
            );
        }
        assert_eq!(documented(&anthropic, "claude-opus-7"), None);
    }

    #[test]
    fn a_custom_anthropic_endpoint_is_matched_by_the_model_name() {
        let mut custom = provider("custom", "https://example.com/anthropic/v1");
        custom.api_format = PostProcessApiFormat::Anthropic;
        assert_eq!(
            levels(documented(&custom, "us.anthropic.claude-sonnet-4-6")),
            names(&["low", "medium", "high", "max"])
        );
        assert_eq!(documented(&custom, "my-proxy-model"), None);
    }

    #[test]
    fn openai_levels_depend_on_the_model_family() {
        let openai = provider("openai", "https://api.openai.com/v1");
        assert_eq!(
            levels(documented(&openai, "gpt-5-mini")),
            names(&["minimal", "low", "medium", "high"])
        );
        assert_eq!(
            levels(documented(&openai, "gpt-5.1")),
            names(&["none", "low", "medium", "high"])
        );
        assert_eq!(
            levels(documented(&openai, "gpt-5.2-2026-01-15")),
            names(&["none", "low", "medium", "high", "xhigh"])
        );
        assert_eq!(
            levels(documented(&openai, "gpt-5.5")),
            names(&["none", "low", "medium", "high", "xhigh"])
        );
        assert_eq!(
            levels(documented(&openai, "o4-mini")),
            names(&["low", "medium", "high"])
        );
        for model in ["gpt-4o-mini", "gpt-4.1", "gpt-3.5-turbo"] {
            assert_eq!(
                levels(documented(&openai, model)),
                Some(Vec::new()),
                "{model}"
            );
        }
        // A family the documentation didn't name yet.
        assert_eq!(documented(&openai, "gpt-5.4"), None);
    }

    #[test]
    fn other_providers_document_some_families() {
        assert_eq!(
            levels(documented(
                &provider("groq", "https://api.groq.com/openai/v1"),
                "openai/gpt-oss-120b"
            )),
            names(&["low", "medium", "high"])
        );
        assert_eq!(
            levels(documented(
                &provider("cerebras", "https://api.cerebras.ai/v1"),
                "zai-glm-5.3-flash"
            )),
            names(&["low", "high", "max"])
        );
        assert_eq!(
            levels(documented(
                &provider("zai", "https://api.z.ai/api/paas/v4"),
                "glm-5.3"
            )),
            names(&["low", "high", "max"])
        );
        assert_eq!(
            levels(documented(
                &provider("custom", "https://api.deepseek.com"),
                "deepseek-chat"
            )),
            names(&["low", "high", "max"])
        );
        // OpenRouter takes every level and maps it to the nearest one.
        assert_eq!(
            levels(documented(
                &provider("openrouter", "https://openrouter.ai/api/v1"),
                "some/model"
            )),
            names(&LEVELS)
        );
        assert_eq!(
            documented(&provider("custom", "http://localhost:11434/v1"), "llama3"),
            None
        );
        assert_eq!(
            documented(
                &provider(
                    "bedrock_mantle",
                    "https://bedrock-mantle.us-east-1.api.aws/v1"
                ),
                "openai.gpt-oss-120b"
            ),
            None
        );
    }

    #[test]
    fn unknown_models_get_the_common_levels() {
        let options = options(
            &provider("custom", "http://127.0.0.1:9/effort-common"),
            "llama3",
        );
        assert_eq!(options.source, EffortSource::Common);
        assert_eq!(Some(options.levels), names(&COMMON_LEVELS));
        assert!(options.rejected.is_empty());
    }

    #[test]
    fn the_model_list_wins_over_the_documentation() {
        let anthropic = provider("anthropic", "http://127.0.0.1:9/effort-listed");
        let listing = json!({
            "data": [anthropic_entry("claude-sonnet-5-5", &["low", "high"], true)],
            "has_more": false
        });
        remember_listing(&anthropic, &listing);
        let options = options(&anthropic, "claude-sonnet-5-5");
        assert_eq!(options.source, EffortSource::Listed);
        assert_eq!(Some(options.levels), names(&["low", "high"]));
        assert!(wants_adaptive_thinking(&anthropic, "claude-sonnet-5-5"));

        let documented = super::options(&anthropic, "claude-sonnet-4-6");
        assert_eq!(documented.source, EffortSource::Documented);
    }

    #[test]
    fn the_options_name_levels_the_endpoint_refused() {
        let custom = provider("custom", "http://127.0.0.1:9/effort-refused");
        crate::llm_client::remember_effort_rejection(&custom, "llama3", "high");
        assert_eq!(Some(options(&custom, "llama3").rejected), names(&["high"]));
    }

    #[test]
    fn apple_intelligence_offers_no_level() {
        let apple = provider(APPLE_INTELLIGENCE_PROVIDER_ID, "apple-intelligence://local");
        let options = options(&apple, "Apple Intelligence");
        assert!(options.levels.is_empty());
        assert_ne!(options.source, EffortSource::Common);
    }

    fn settings_with(provider_id: &str, model: &str, effort: &str) -> AppSettings {
        let mut settings = settings::get_default_settings();
        settings
            .post_process_models
            .insert(provider_id.to_string(), model.to_string());
        settings
            .post_process_efforts
            .insert(provider_id.to_string(), effort.to_string());
        settings
    }

    #[test]
    fn the_stored_effort_is_per_provider_and_only_a_known_level() {
        let mut settings = settings_with("anthropic", "claude-sonnet-5-5", "xhigh");
        assert_eq!(stored(&settings, "anthropic").as_deref(), Some("xhigh"));
        assert_eq!(stored(&settings, "openai"), None);
        settings
            .post_process_efforts
            .insert("openai".to_string(), "turbo".to_string());
        assert_eq!(stored(&settings, "openai"), None);
    }

    #[test]
    fn a_request_carries_no_level_the_model_is_known_not_to_take() {
        let settings = settings_with("anthropic", "claude-sonnet-4-6", "xhigh");
        let anthropic = find_provider(&settings, "anthropic").unwrap().clone();
        // As the settings page shows it: the model's default.
        assert_eq!(
            for_request(&settings, &anthropic, "claude-sonnet-4-6"),
            None
        );
        assert_eq!(
            for_request(&settings, &anthropic, "claude-sonnet-5-5").as_deref(),
            Some("xhigh")
        );
        // Nothing known: the stored level goes, the endpoint may take it.
        let settings = settings_with("custom", "llama3", "xhigh");
        let custom = find_provider(&settings, "custom").unwrap().clone();
        assert_eq!(
            for_request(&settings, &custom, "llama3").as_deref(),
            Some("xhigh")
        );
    }

    #[test]
    fn setting_an_effort_checks_the_level_and_none_clears_it() {
        let mut settings = settings::get_default_settings();
        set(&mut settings, "anthropic", Some("xhigh")).unwrap();
        assert_eq!(stored(&settings, "anthropic").as_deref(), Some("xhigh"));
        assert!(set(&mut settings, "anthropic", Some("turbo")).is_err());
        assert_eq!(stored(&settings, "anthropic").as_deref(), Some("xhigh"));
        set(&mut settings, "anthropic", None).unwrap();
        assert!(!settings.post_process_efforts.contains_key("anthropic"));
        set(&mut settings, "anthropic", Some("high")).unwrap();
        set(&mut settings, "anthropic", Some(" ")).unwrap();
        assert!(!settings.post_process_efforts.contains_key("anthropic"));
    }

    #[test]
    fn a_new_model_drops_an_effort_it_is_known_not_to_take() {
        let mut settings = settings_with("anthropic", "claude-sonnet-4-6", "xhigh");
        assert!(drop_if_unsupported(&mut settings, "anthropic"));
        assert_eq!(stored(&settings, "anthropic"), None);

        let mut settings = settings_with("anthropic", "claude-sonnet-5-5", "xhigh");
        assert!(!drop_if_unsupported(&mut settings, "anthropic"));
        assert_eq!(stored(&settings, "anthropic").as_deref(), Some("xhigh"));

        // Nothing known about the model: the choice stays.
        let mut settings = settings_with("custom", "llama3", "xhigh");
        assert!(!drop_if_unsupported(&mut settings, "custom"));
        assert_eq!(stored(&settings, "custom").as_deref(), Some("xhigh"));

        // No model: nothing to take an effort.
        let mut settings = settings_with("anthropic", "", "high");
        assert!(drop_if_unsupported(&mut settings, "anthropic"));
    }
}
