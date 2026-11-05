//! DanyWhere's model offer: which catalog models the app lists, in what order,
//! and where their files come from.
//!
//! `catalog/catalog.json` stays a verbatim copy of Handy's model package so
//! upstream syncs stay simple. This module narrows it to models that are
//! practical for dictation on a laptop and serves them from DanyWhere's own
//! mirror (GitHub releases of `yaccob/DanyWhere-models`), which also works on
//! networks that block Hugging Face. Hugging Face remains the fallback.
//!
//! The offer itself is data: `model_selection.toml`, compiled in.

use once_cell::sync::Lazy;
use serde::Deserialize;

#[derive(Deserialize)]
struct Selection {
    /// Release downloads of the mirror repository.
    mirror: String,
    /// Offered models, best first.
    offered: Vec<Offered>,
}

/// One offered catalog model.
#[derive(Deserialize)]
pub struct Offered {
    pub id: String,
    /// A general favourite (the onboarding's pick depends on the languages).
    pub recommended: bool,
}

static SELECTION: Lazy<Selection> = Lazy::new(|| {
    toml::from_str(include_str!("model_selection.toml"))
        .expect("bundled model_selection.toml is valid")
});

/// The offered models, best first.
pub fn offered() -> &'static [Offered] {
    &SELECTION.offered
}

/// Whether DanyWhere offers the catalog model `id`.
pub fn is_offered(id: &str) -> bool {
    offered().iter().any(|o| o.id == id)
}

/// `(rank, recommended)` of an offered model; rank 1 is the best.
pub fn rank(id: &str) -> Option<(u32, bool)> {
    offered()
        .iter()
        .position(|o| o.id == id)
        .map(|i| (i as u32 + 1, offered()[i].recommended))
}

/// Mirror URL of a model file. Each model revision is its own release
/// (`{id}-{first 12 chars of the revision}`), so a new revision never
/// replaces the bytes an older DanyWhere build expects.
pub fn mirror_url(id: &str, revision: &str, filename: &str) -> String {
    format!(
        "{}/{}/{filename}",
        SELECTION.mirror.trim_end_matches('/'),
        release_tag(id, revision)
    )
}

/// Base URL of the mirror's release downloads.
#[cfg(test)]
pub fn mirror_releases() -> &'static str {
    &SELECTION.mirror
}

/// Corrects the capabilities the engine reports for a loaded model with what
/// the catalog states for the same variant. The engine only sees the file;
/// for older files (e.g. the legacy Whisper `.bin`s) it guesses from the
/// architecture and can claim features the model was never trained for.
/// Variants the catalog does not know keep the engine's answer.
pub fn reconcile_capabilities(variant: &str, caps: &mut transcribe_cpp::Capabilities) {
    if let Some(known) = crate::catalog::known_capabilities(variant) {
        caps.supports_translate = known.translate;
        caps.supports_streaming = known.streaming;
        caps.supports_language_detect = known.language_detection;
    }
}

/// Release tag holding a model's files (see [`mirror_url`]).
fn release_tag(id: &str, revision: &str) -> String {
    format!("{id}-{}", &revision[..revision.len().min(12)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine_caps(translate: bool) -> transcribe_cpp::Capabilities {
        transcribe_cpp::Capabilities {
            native_sample_rate: 16000,
            languages: vec!["de".into(), "en".into()],
            translate_target_languages: Vec::new(),
            max_timestamp_kind: Default::default(),
            supports_language_detect: true,
            supports_translate: translate,
            supports_streaming: false,
            supports_spec_decode: false,
            max_audio_ms: 0,
        }
    }

    #[test]
    fn the_catalog_overrules_a_wrong_translate_claim() {
        // The engine claims translation for the legacy Whisper Turbo .bin; a
        // translate task then drops whole passages (reproduced 2026-10-24).
        let mut caps = engine_caps(true);
        reconcile_capabilities("whisper-large-v3-turbo", &mut caps);
        assert!(!caps.supports_translate);
    }

    #[test]
    fn catalogued_translation_support_is_kept() {
        let mut caps = engine_caps(true);
        reconcile_capabilities("whisper-medium", &mut caps);
        assert!(caps.supports_translate);
    }

    #[test]
    fn unknown_variants_keep_what_the_engine_reports() {
        let mut caps = engine_caps(true);
        let before = caps.clone();
        reconcile_capabilities("some-custom-model", &mut caps);
        assert_eq!(caps, before);
    }
}
