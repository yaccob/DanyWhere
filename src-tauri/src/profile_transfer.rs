//! DanyWhere: exporting a profile to a YAML file and importing one.
//!
//! The file holds one profile (its name, values and applications), never the
//! global settings. The API key of the profile's post-processing provider
//! always travels with it, encrypted with a password chosen at export
//! ([`crate::profile_export`]); a profile that needs no key needs no password.
//! An import can always leave the key out, e.g. when the password is unknown.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Emitter};
use zeroize::Zeroizing;

use crate::profile_export::{
    self, EncryptedSecret, ExportError, ExportedApiKey, ProfileExportFile,
};
use crate::profiles::{self, Profile, ProfileValues};
use crate::settings::{self, AppSettings};

/// Export files are small; anything bigger is not one.
const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// What went wrong, as a code the frontend translates.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum TransferErrorCode {
    ProfileNotFound,
    /// The profile carries an API key, so the export needs a password.
    PasswordRequired,
    EmptyPassword,
    /// Wrong password, or the encrypted key in the file was changed.
    WrongPassword,
    /// Not a profile file, or one from a newer DanyWhere.
    UnsupportedFormat,
    /// A damaged or incomplete file.
    Malformed,
    /// The file could not be read or written.
    Io,
    Internal,
}

/// An error of an export or import: a code and, for the log, what only Rust
/// knows.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct TransferError {
    pub code: TransferErrorCode,
    pub detail: Option<String>,
}

impl TransferError {
    fn new(code: TransferErrorCode) -> Self {
        Self { code, detail: None }
    }

    fn with_detail(code: TransferErrorCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: Some(detail.into()),
        }
    }
}

impl From<ExportError> for TransferError {
    fn from(error: ExportError) -> Self {
        match error {
            ExportError::EmptyPassword => TransferError::new(TransferErrorCode::EmptyPassword),
            ExportError::WrongPassword => TransferError::new(TransferErrorCode::WrongPassword),
            ExportError::UnsupportedFormat(what) => {
                TransferError::with_detail(TransferErrorCode::UnsupportedFormat, what)
            }
            ExportError::Malformed(what) => {
                TransferError::with_detail(TransferErrorCode::Malformed, what)
            }
            ExportError::Internal(what) => {
                TransferError::with_detail(TransferErrorCode::Internal, what)
            }
        }
    }
}

/// What the import dialog needs to know about a file before asking for a
/// password.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct ImportPreview {
    /// The profile's name as stored in the file.
    pub name: String,
    /// Whether the file carries an encrypted API key.
    pub has_api_key: bool,
    /// The provider the key belongs to.
    pub provider_id: Option<String>,
}

/// What to do when the file's key differs from the key stored here for the
/// same provider.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum KeyConflictChoice {
    /// Import nothing and report the conflict, so the user can be asked.
    Ask,
    /// Store the file's key instead.
    Replace,
    /// Keep the stored key and import the profile.
    Keep,
}

/// What happened to the file's API key.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum ImportedKey {
    /// The file had none, or it was left out.
    None,
    /// No key was stored for the provider; the file's is now.
    Added,
    /// The stored key was replaced by the file's.
    Replaced,
    /// The stored key differs and was kept.
    Kept,
    /// The stored key is the same as the file's.
    Same,
}

/// The result of an import.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type)]
pub struct ImportResult {
    /// The new profile; `None` when the import stopped at a key conflict.
    pub profile: Option<Profile>,
    pub key: ImportedKey,
    /// Set when the file's key differs from the stored one and the choice was
    /// [`KeyConflictChoice::Ask`]; nothing was imported then.
    pub conflict_provider_id: Option<String>,
    /// Applications of the file's profile that another profile here is
    /// already used for, so the new profile doesn't get them.
    pub dropped_apps: Vec<String>,
}

/// A decrypted API key and its provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderKey {
    pub provider_id: String,
    pub key: String,
}

/// The API key that travels with `profile`: its provider's, while
/// post-processing is on and a key is stored for that provider.
pub fn key_to_export(settings: &AppSettings, profile: &Profile) -> Option<ProviderKey> {
    if !profile.values.post_process_enabled {
        return None;
    }
    let provider_id = &profile.values.post_process_provider_id;
    settings
        .post_process_api_keys
        .get(provider_id)
        .filter(|key| !key.trim().is_empty())
        .map(|key| ProviderKey {
            provider_id: provider_id.clone(),
            key: key.clone(),
        })
}

/// Profile `id` as it is exported: the active one with the latest flat
/// values, and the default profile with its shown name instead of an empty
/// one.
pub fn profile_for_export(
    settings: &AppSettings,
    id: &str,
    default_name: &str,
) -> Result<Profile, TransferError> {
    let mut profile = settings
        .profiles
        .iter()
        .find(|profile| profile.id == id)
        .cloned()
        .ok_or_else(|| {
            TransferError::with_detail(TransferErrorCode::ProfileNotFound, id.to_string())
        })?;
    if profile.id == settings.active_profile_id {
        // The flat fields hold the active profile's latest values.
        profile.values = ProfileValues::from_settings(settings);
    }
    profile.name = profiles::display_name(&profile, default_name);
    Ok(profile)
}

/// Whether exporting profile `id` asks for a password.
pub fn needs_password(settings: &AppSettings, id: &str) -> Result<bool, TransferError> {
    let profile = profile_for_export(settings, id, "")?;
    Ok(key_to_export(settings, &profile).is_some())
}

/// The YAML of profile `id`, its key encrypted by `encrypt` with `password`.
fn build_export_with<F>(
    settings: &AppSettings,
    id: &str,
    default_name: &str,
    password: Option<&str>,
    encrypt: F,
) -> Result<String, TransferError>
where
    F: Fn(&str, &str) -> Result<EncryptedSecret, ExportError>,
{
    let profile = profile_for_export(settings, id, default_name)?;
    let api_key = match key_to_export(settings, &profile) {
        Some(key) => {
            let password =
                password.ok_or_else(|| TransferError::new(TransferErrorCode::PasswordRequired))?;
            Some(ExportedApiKey {
                provider_id: key.provider_id,
                secret: encrypt(password, &key.key)?,
            })
        }
        None => None,
    };
    Ok(profile_export::to_yaml(&ProfileExportFile::new(
        profile, api_key,
    ))?)
}

/// The YAML of profile `id`, its key encrypted with `password`.
pub fn build_export(
    settings: &AppSettings,
    id: &str,
    default_name: &str,
    password: Option<&str>,
) -> Result<String, TransferError> {
    build_export_with(
        settings,
        id,
        default_name,
        password,
        profile_export::encrypt_secret,
    )
}

/// What the import dialog shows before asking for a password.
pub fn preview(text: &str) -> Result<ImportPreview, TransferError> {
    let file: ProfileExportFile<Profile> = profile_export::from_yaml(text)?;
    if let Some(api_key) = &file.api_key {
        // An unknown algorithm is reported before anyone types a password.
        api_key.secret.check_supported()?;
    }
    Ok(ImportPreview {
        name: file.profile.name,
        has_api_key: file.api_key.is_some(),
        provider_id: file.api_key.map(|key| key.provider_id),
    })
}

/// Reads an export file. With a password its key is decrypted; without one
/// the key is left out.
pub fn read_import(
    text: &str,
    password: Option<&str>,
) -> Result<(Profile, Option<ProviderKey>), TransferError> {
    let file: ProfileExportFile<Profile> = profile_export::from_yaml(text)?;
    let key = match (file.api_key, password) {
        (Some(api_key), Some(password)) => Some(ProviderKey {
            key: profile_export::decrypt_secret(password, &api_key.secret)?,
            provider_id: api_key.provider_id,
        }),
        _ => None,
    };
    Ok((file.profile, key))
}

/// `wanted`, or `wanted (2)`, `wanted (3)`, … whichever is not in `taken`.
pub fn unique_name(wanted: &str, taken: &[String]) -> String {
    let wanted = wanted.trim();
    let is_taken = |name: &str| taken.iter().any(|t| t.trim() == name);
    if !is_taken(wanted) {
        return wanted.to_string();
    }
    (2..)
        .map(|n| format!("{wanted} ({n})"))
        .find(|name| !is_taken(name))
        .expect("an unused number")
}

/// A profile id no profile in `settings` has, like the ones `duplicate`
/// makes.
fn new_profile_id(settings: &AppSettings, now_millis: i64) -> String {
    let base = format!("profile_{now_millis}");
    let mut id = base.clone();
    let mut suffix = 1;
    while settings.profiles.iter().any(|profile| profile.id == id) {
        suffix += 1;
        id = format!("{base}_{suffix}");
    }
    id
}

/// Adds the imported `profile` to `settings` under a new id and a free name,
/// without switching to it, and stores its key as `choice` says.
pub fn apply_import(
    settings: &mut AppSettings,
    profile: Profile,
    key: Option<ProviderKey>,
    choice: KeyConflictChoice,
    default_name: &str,
    now_millis: i64,
) -> ImportResult {
    let mut profile = profile;
    // What happens to the key, decided before anything changes.
    let imported_key = match &key {
        None => ImportedKey::None,
        Some(key) => {
            let stored = settings
                .post_process_api_keys
                .get(&key.provider_id)
                .filter(|stored| !stored.trim().is_empty());
            match stored {
                None => ImportedKey::Added,
                Some(stored) if *stored == key.key => ImportedKey::Same,
                Some(_) => match choice {
                    KeyConflictChoice::Ask => {
                        return ImportResult {
                            profile: None,
                            key: ImportedKey::None,
                            conflict_provider_id: Some(key.provider_id.clone()),
                            dropped_apps: Vec::new(),
                        };
                    }
                    KeyConflictChoice::Replace => ImportedKey::Replaced,
                    KeyConflictChoice::Keep => ImportedKey::Kept,
                },
            }
        }
    };

    let taken: Vec<String> = settings
        .profiles
        .iter()
        .map(|existing| profiles::display_name(existing, default_name))
        .collect();
    let wanted = if profile.name.trim().is_empty() {
        default_name.to_string()
    } else {
        profile.name.clone()
    };
    profile.name = unique_name(&wanted, &taken);
    profile.id = new_profile_id(settings, now_millis);

    // An application belongs to at most one profile.
    let (kept, dropped): (Vec<String>, Vec<String>) = std::mem::take(&mut profile.apps)
        .into_iter()
        .partition(|app| {
            !settings
                .profiles
                .iter()
                .any(|existing| existing.apps.contains(app))
        });
    profile.apps = kept;

    if let (Some(key), ImportedKey::Added | ImportedKey::Replaced) = (key, imported_key) {
        settings
            .post_process_api_keys
            .insert(key.provider_id, key.key);
    }
    settings.profiles.push(profile.clone());
    ImportResult {
        profile: Some(profile),
        key: imported_key,
        conflict_provider_id: None,
        dropped_apps: dropped,
    }
}

/// `path` with `.yaml` appended when it has no extension.
pub fn with_yaml_extension(path: &str) -> String {
    if std::path::Path::new(path).extension().is_some() {
        path.to_string()
    } else {
        format!("{path}.yaml")
    }
}

fn io_error(error: std::io::Error) -> TransferError {
    TransferError::with_detail(TransferErrorCode::Io, error.to_string())
}

fn read_file(path: &str) -> Result<String, TransferError> {
    let size = std::fs::metadata(path).map_err(io_error)?.len();
    if size > MAX_FILE_BYTES {
        return Err(TransferError::with_detail(
            TransferErrorCode::Malformed,
            format!("{size} bytes is too big for a profile file"),
        ));
    }
    std::fs::read_to_string(path).map_err(io_error)
}

fn join_error(error: impl std::fmt::Display) -> TransferError {
    TransferError::with_detail(TransferErrorCode::Internal, error.to_string())
}

/// Whether exporting profile `id` asks for a password, i.e. whether it
/// carries an API key.
#[tauri::command]
#[specta::specta]
pub fn profile_export_needs_password(app: AppHandle, id: String) -> Result<bool, TransferError> {
    needs_password(&settings::get_settings(&app), &id)
}

/// Writes profile `id` to `path` (`.yaml` is appended when it has no
/// extension). The key derivation takes about a second, so it runs off the
/// main thread; the password is wiped afterwards.
#[tauri::command]
#[specta::specta]
pub async fn export_profile(
    app: AppHandle,
    id: String,
    path: String,
    password: Option<String>,
) -> Result<(), TransferError> {
    let password = password.map(Zeroizing::new);
    tauri::async_runtime::spawn_blocking(move || {
        let settings = settings::get_settings(&app);
        let default_name = profiles::default_profile_name(&settings);
        let yaml = build_export(
            &settings,
            &id,
            &default_name,
            password.as_deref().map(String::as_str),
        )?;
        let path = with_yaml_extension(&path);
        std::fs::write(&path, yaml).map_err(io_error)?;
        log::info!("Exported profile {id} to {path}");
        Ok(())
    })
    .await
    .map_err(join_error)?
}

/// What the import dialog shows about the file at `path`, before asking for
/// a password.
#[tauri::command]
#[specta::specta]
pub fn inspect_profile_import(path: String) -> Result<ImportPreview, TransferError> {
    preview(&read_file(&path)?)
}

/// Imports the profile at `path` as a new profile, without switching to it.
/// With a password the file's key is decrypted and stored as `key_conflict`
/// says; without one the key is left out. The password is wiped afterwards.
#[tauri::command]
#[specta::specta]
pub async fn import_profile(
    app: AppHandle,
    path: String,
    password: Option<String>,
    key_conflict: KeyConflictChoice,
) -> Result<ImportResult, TransferError> {
    let password = password.map(Zeroizing::new);
    tauri::async_runtime::spawn_blocking(move || {
        let text = read_file(&path)?;
        // The slow part first, so the settings are read and written in one go.
        let (profile, key) = read_import(&text, password.as_deref().map(String::as_str))?;
        let mut settings = settings::get_settings(&app);
        let default_name = profiles::default_profile_name(&settings);
        let now = chrono::Utc::now().timestamp_millis();
        let result = apply_import(
            &mut settings,
            profile,
            key,
            key_conflict,
            &default_name,
            now,
        );
        if let Some(profile) = &result.profile {
            log::info!(
                "Imported profile {} from {path} (key: {:?})",
                profile.id,
                result.key
            );
            settings::write_settings(&app, settings);
            let _ = app.emit(
                "settings-changed",
                serde_json::json!({ "setting": "profiles" }),
            );
            crate::tray::rebuild_tray_menu(&app);
            // Its prompts may still contain ${output}.
            crate::prompt_frame::prompts_changed(&app);
        }
        Ok(result)
    })
    .await
    .map_err(join_error)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile_export::{encrypt_secret_with_params, KdfParams};
    use crate::profiles::{ensure_profiles, ProfileValues};
    use crate::settings::LLMPrompt;

    const DEFAULT_NAME: &str = "Standard";
    const ANTHROPIC_KEY: &str = "sk-ant-test-0123456789abcdef";
    const OPENAI_KEY: &str = "sk-openai-test-fedcba9876543210";

    fn fast_encrypt(password: &str, plaintext: &str) -> Result<EncryptedSecret, ExportError> {
        encrypt_secret_with_params(password, plaintext, KdfParams::FOR_TESTS)
    }

    fn export(
        settings: &AppSettings,
        id: &str,
        password: Option<&str>,
    ) -> Result<String, TransferError> {
        build_export_with(settings, id, DEFAULT_NAME, password, fast_encrypt)
    }

    /// The default profile (active, Anthropic with a key, post-processing
    /// on) and "Work" (OpenAI, post-processing off), with keys stored for
    /// both providers.
    fn settings() -> AppSettings {
        let mut s = AppSettings {
            selected_model: "parakeet".to_string(),
            selected_language: "de".to_string(),
            post_process_enabled: true,
            post_process_provider_id: "anthropic".to_string(),
            post_process_prompts: vec![LLMPrompt {
                id: "p1".to_string(),
                name: "Diktat (DE/EN)".to_string(),
                prompt: "Clean it up.".to_string(),
            }],
            post_process_selected_prompt_id: Some("p1".to_string()),
            ..Default::default()
        };
        s.post_process_api_keys
            .insert("anthropic".to_string(), ANTHROPIC_KEY.to_string());
        s.post_process_api_keys
            .insert("openai".to_string(), OPENAI_KEY.to_string());
        ensure_profiles(&mut s);
        let mut work = ProfileValues::from_settings(&s);
        work.post_process_enabled = false;
        work.post_process_provider_id = "openai".to_string();
        s.profiles.push(Profile {
            id: "work".to_string(),
            name: "Work".to_string(),
            values: work,
            apps: vec!["com.apple.mail".to_string()],
        });
        s
    }

    fn default_profile(s: &AppSettings) -> Profile {
        s.profiles[0].clone()
    }

    #[test]
    fn the_key_of_the_profiles_provider_travels_while_post_processing_is_on() {
        let s = settings();
        assert_eq!(
            key_to_export(&s, &default_profile(&s)),
            Some(ProviderKey {
                provider_id: "anthropic".to_string(),
                key: ANTHROPIC_KEY.to_string(),
            })
        );
        // Post-processing off: no key, although one is stored for OpenAI.
        assert_eq!(key_to_export(&s, &s.profiles[1]), None);
    }

    #[test]
    fn no_key_travels_when_none_is_stored_for_the_provider() {
        let mut s = settings();
        s.post_process_api_keys
            .insert("anthropic".to_string(), "  ".to_string());
        assert_eq!(key_to_export(&s, &default_profile(&s)), None);
        s.post_process_api_keys.remove("anthropic");
        assert_eq!(key_to_export(&s, &default_profile(&s)), None);
    }

    #[test]
    fn only_a_profile_with_a_key_asks_for_a_password() {
        let s = settings();
        assert_eq!(needs_password(&s, "default"), Ok(true));
        assert_eq!(needs_password(&s, "work"), Ok(false));
        assert_eq!(
            needs_password(&s, "nope").map_err(|e| e.code),
            Err(TransferErrorCode::ProfileNotFound)
        );
    }

    #[test]
    fn a_profile_without_a_key_exports_without_a_password() {
        let s = settings();
        let yaml = export(&s, "work", None).expect("export");
        assert!(yaml.contains("Work"), "{yaml}");
        assert!(!yaml.contains("api_key"), "{yaml}");
        assert!(!yaml.contains(OPENAI_KEY));
    }

    #[test]
    fn a_profile_with_a_key_needs_the_password() {
        let s = settings();
        assert_eq!(
            export(&s, "default", None).map_err(|e| e.code),
            Err(TransferErrorCode::PasswordRequired)
        );
        assert_eq!(
            export(&s, "default", Some("")).map_err(|e| e.code),
            Err(TransferErrorCode::EmptyPassword)
        );
    }

    #[test]
    fn the_export_holds_the_encrypted_key_and_no_key_in_plain_text() {
        let s = settings();
        let yaml = export(&s, "default", Some("geheim")).expect("export");
        assert!(yaml.contains("api_key"), "{yaml}");
        assert!(yaml.contains("anthropic"), "{yaml}");
        assert!(yaml.contains("Diktat (DE/EN)"), "{yaml}");
        assert!(!yaml.contains(ANTHROPIC_KEY), "plain key in {yaml}");
        // No other provider's key, in any form.
        assert!(!yaml.contains(OPENAI_KEY), "other key in {yaml}");
        assert!(!yaml.contains("post_process_api_keys"), "{yaml}");
        assert!(!yaml.contains("sk-"), "{yaml}");
    }

    #[test]
    fn the_default_profile_is_exported_under_its_shown_name() {
        let s = settings();
        let profile = profile_for_export(&s, "default", DEFAULT_NAME).expect("profile");
        assert_eq!(profile.name, DEFAULT_NAME);
        let work = profile_for_export(&s, "work", DEFAULT_NAME).expect("profile");
        assert_eq!(work.name, "Work");
    }

    #[test]
    fn the_active_profile_is_exported_with_the_latest_flat_values() {
        let mut s = settings();
        // A change not yet copied into the profile.
        s.selected_language = "fr".to_string();
        let profile = profile_for_export(&s, "default", DEFAULT_NAME).expect("profile");
        assert_eq!(profile.values.selected_language, "fr");
        assert_eq!(
            profile_for_export(&s, "nope", DEFAULT_NAME).map_err(|e| e.code),
            Err(TransferErrorCode::ProfileNotFound)
        );
    }

    #[test]
    fn an_exported_profile_reads_back_with_its_key() {
        let s = settings();
        let yaml = export(&s, "default", Some("geheim")).expect("export");
        let (profile, key) = read_import(&yaml, Some("geheim")).expect("import");
        let original = profile_for_export(&s, "default", DEFAULT_NAME).unwrap();
        assert_eq!(profile.name, DEFAULT_NAME);
        assert_eq!(profile.values, original.values);
        assert_eq!(
            key,
            Some(ProviderKey {
                provider_id: "anthropic".to_string(),
                key: ANTHROPIC_KEY.to_string(),
            })
        );
    }

    #[test]
    fn a_wrong_password_is_reported_and_no_password_leaves_the_key_out() {
        let s = settings();
        let yaml = export(&s, "default", Some("geheim")).expect("export");
        assert_eq!(
            read_import(&yaml, Some("falsch")).map_err(|e| e.code),
            Err(TransferErrorCode::WrongPassword)
        );
        let (profile, key) = read_import(&yaml, None).expect("import without key");
        assert_eq!(profile.name, DEFAULT_NAME);
        assert_eq!(key, None);
    }

    #[test]
    fn the_preview_tells_whether_the_file_carries_a_key() {
        let s = settings();
        let with_key = export(&s, "default", Some("geheim")).unwrap();
        assert_eq!(
            preview(&with_key),
            Ok(ImportPreview {
                name: DEFAULT_NAME.to_string(),
                has_api_key: true,
                provider_id: Some("anthropic".to_string()),
            })
        );
        let without = export(&s, "work", None).unwrap();
        assert_eq!(
            preview(&without),
            Ok(ImportPreview {
                name: "Work".to_string(),
                has_api_key: false,
                provider_id: None,
            })
        );
    }

    #[test]
    fn other_files_are_refused_with_a_code() {
        assert_eq!(
            preview("format: something-else\nversion: 1\nprofile: {}\n").map_err(|e| e.code),
            Err(TransferErrorCode::UnsupportedFormat)
        );
        assert_eq!(
            preview("this is: [not: valid").map_err(|e| e.code),
            Err(TransferErrorCode::Malformed)
        );
        let s = settings();
        let newer = export(&s, "work", None)
            .unwrap()
            .replace("version: 1", "version: 99");
        assert_eq!(
            read_import(&newer, None).map_err(|e| e.code),
            Err(TransferErrorCode::UnsupportedFormat)
        );
    }

    #[test]
    fn a_taken_name_gets_a_number() {
        let taken = vec!["Work".to_string(), "Work (2)".to_string()];
        assert_eq!(unique_name("Work", &taken), "Work (3)");
        assert_eq!(unique_name("Mail", &taken), "Mail");
        assert_eq!(unique_name("  Mail ", &taken), "Mail");
    }

    fn imported(name: &str) -> Profile {
        let s = settings();
        let mut p = profile_for_export(&s, "work", DEFAULT_NAME).unwrap();
        p.id = "work".to_string();
        p.name = name.to_string();
        p.apps = Vec::new();
        p
    }

    #[test]
    fn an_import_adds_a_new_profile_without_switching_to_it() {
        let mut s = settings();
        let result = apply_import(
            &mut s,
            imported("Work"),
            None,
            KeyConflictChoice::Ask,
            DEFAULT_NAME,
            1000,
        );
        let profile = result.profile.expect("imported");
        assert_eq!(s.profiles.len(), 3);
        assert_eq!(s.profiles[2], profile);
        assert_ne!(profile.id, "work", "the file's id is not reused");
        assert_ne!(profile.id, "default");
        assert_eq!(profile.name, "Work (2)");
        assert_eq!(s.active_profile_id, "default");
        assert_eq!(result.key, ImportedKey::None);
    }

    #[test]
    fn two_imports_of_the_same_file_get_distinct_ids_and_names() {
        let mut s = settings();
        let a = apply_import(
            &mut s,
            imported("Mail"),
            None,
            KeyConflictChoice::Ask,
            DEFAULT_NAME,
            1000,
        );
        let b = apply_import(
            &mut s,
            imported("Mail"),
            None,
            KeyConflictChoice::Ask,
            DEFAULT_NAME,
            1000,
        );
        let (a, b) = (a.profile.unwrap(), b.profile.unwrap());
        assert_ne!(a.id, b.id);
        assert_eq!(a.name, "Mail");
        assert_eq!(b.name, "Mail (2)");
    }

    #[test]
    fn a_nameless_profile_is_imported_under_the_default_name_with_a_number() {
        let mut s = settings();
        let result = apply_import(
            &mut s,
            imported("  "),
            None,
            KeyConflictChoice::Ask,
            DEFAULT_NAME,
            1000,
        );
        assert_eq!(result.profile.unwrap().name, "Standard (2)");
    }

    fn key(value: &str) -> Option<ProviderKey> {
        Some(ProviderKey {
            provider_id: "anthropic".to_string(),
            key: value.to_string(),
        })
    }

    #[test]
    fn a_key_for_a_provider_without_one_is_added() {
        let mut s = settings();
        s.post_process_api_keys.remove("anthropic");
        let result = apply_import(
            &mut s,
            imported("Mail"),
            key("sk-new"),
            KeyConflictChoice::Ask,
            DEFAULT_NAME,
            1000,
        );
        assert_eq!(result.key, ImportedKey::Added);
        assert_eq!(
            s.post_process_api_keys.get("anthropic").map(String::as_str),
            Some("sk-new")
        );
    }

    #[test]
    fn the_same_key_is_left_as_it_is() {
        let mut s = settings();
        let result = apply_import(
            &mut s,
            imported("Mail"),
            key(ANTHROPIC_KEY),
            KeyConflictChoice::Ask,
            DEFAULT_NAME,
            1000,
        );
        assert_eq!(result.key, ImportedKey::Same);
        assert!(result.profile.is_some());
    }

    #[test]
    fn a_different_key_asks_first_and_imports_nothing() {
        let mut s = settings();
        let before = s.clone();
        let result = apply_import(
            &mut s,
            imported("Mail"),
            key("sk-other"),
            KeyConflictChoice::Ask,
            DEFAULT_NAME,
            1000,
        );
        assert_eq!(result.profile, None);
        assert_eq!(result.conflict_provider_id.as_deref(), Some("anthropic"));
        assert_eq!(s.profiles, before.profiles);
        assert_eq!(
            s.post_process_api_keys.get("anthropic").map(String::as_str),
            Some(ANTHROPIC_KEY)
        );
    }

    #[test]
    fn a_different_key_is_replaced_or_kept_as_chosen() {
        let mut s = settings();
        let replaced = apply_import(
            &mut s,
            imported("Mail"),
            key("sk-other"),
            KeyConflictChoice::Replace,
            DEFAULT_NAME,
            1000,
        );
        assert_eq!(replaced.key, ImportedKey::Replaced);
        assert!(replaced.profile.is_some());
        assert_eq!(
            s.post_process_api_keys.get("anthropic").map(String::as_str),
            Some("sk-other")
        );

        let mut s = settings();
        let kept = apply_import(
            &mut s,
            imported("Mail"),
            key("sk-other"),
            KeyConflictChoice::Keep,
            DEFAULT_NAME,
            1000,
        );
        assert_eq!(kept.key, ImportedKey::Kept);
        assert!(kept.profile.is_some());
        assert_eq!(
            s.post_process_api_keys.get("anthropic").map(String::as_str),
            Some(ANTHROPIC_KEY)
        );
    }

    #[test]
    fn applications_another_profile_has_are_dropped() {
        let mut s = settings();
        let mut profile = imported("Mail");
        profile.apps = vec![
            "com.apple.mail".to_string(),
            "com.apple.TextEdit".to_string(),
        ];
        let result = apply_import(
            &mut s,
            profile,
            None,
            KeyConflictChoice::Ask,
            DEFAULT_NAME,
            1000,
        );
        assert_eq!(result.dropped_apps, vec!["com.apple.mail".to_string()]);
        assert_eq!(
            result.profile.unwrap().apps,
            vec!["com.apple.TextEdit".to_string()]
        );
    }

    #[test]
    fn an_export_file_gets_the_yaml_extension() {
        assert_eq!(with_yaml_extension("/tmp/Work"), "/tmp/Work.yaml");
        assert_eq!(with_yaml_extension("/tmp/Work.yaml"), "/tmp/Work.yaml");
        assert_eq!(with_yaml_extension("/tmp/Work.yml"), "/tmp/Work.yml");
        assert_eq!(with_yaml_extension("/tmp/a.b/Work"), "/tmp/a.b/Work.yaml");
    }
}
