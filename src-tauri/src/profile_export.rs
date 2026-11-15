//! Profile export file: a profile as YAML, with the provider's API key
//! encrypted with a password.
//!
//! This module imports nothing from the rest of DanyWhere, so it can be
//! developed in a crate of its own and copied unchanged into `src-tauri/src/`.
//! It is generic over the profile type `P`.
//!
//! How the API key is protected:
//! - the password is stretched with Argon2id (version 0x13, RFC 9106) and a
//!   random 16-byte salt into a 32-byte key;
//! - that key encrypts the API key with XChaCha20-Poly1305 and a random 24-byte
//!   nonce; the 16-byte authentication tag is appended to the ciphertext;
//! - the file stores the names of both algorithms, the Argon2 parameters, salt,
//!   nonce and ciphertext (Base64). Decryption always uses the parameters from
//!   the file, so files written later with stronger parameters decrypt, and
//!   old files keep working when the defaults grow.
//!
//! A wrong password and tampered encrypted data both fail the authentication
//! tag; the two cannot be told apart and are reported as
//! [`ExportError::WrongPassword`].
//!
//! Reading a file ([`from_yaml`]) needs no password: the key stays encrypted
//! until [`decrypt_secret`], so a profile can always be imported without it.

use std::fmt;

use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

/// Value of [`ProfileExportFile::format`].
pub const FORMAT: &str = "danywhere-profile";
/// Highest file version this code reads and the version it writes.
pub const VERSION: u32 = 1;
/// Value of [`EncryptedSecret::kdf`].
pub const KDF_ARGON2ID: &str = "argon2id";
/// Value of [`EncryptedSecret::cipher`].
pub const CIPHER_XCHACHA20POLY1305: &str = "xchacha20poly1305";

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const KEY_LEN: usize = 32;
const TAG_LEN: usize = 16;

/// Associated data authenticated with every ciphertext. Changing it makes
/// existing files undecryptable.
const AAD: &[u8] = b"danywhere-profile-secret";

/// First lines of every written file.
const FILE_COMMENT: &str = "# DanyWhere profile export\n";
/// Added to [`FILE_COMMENT`] when the file carries an API key.
const KEY_COMMENT: &str = "# api_key.secret is the API key, encrypted with the export password\n";

/// Argon2id cost parameters, as stored in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    /// Memory in KiB.
    pub m_cost_kib: u32,
    /// Number of passes over the memory.
    pub t_cost: u32,
    /// Number of lanes.
    pub p_cost: u32,
}

impl KdfParams {
    /// Parameters for new exports: 384 MiB, 3 passes, 4 lanes.
    ///
    /// RFC 9106's second recommended option (64 MiB, t=3, p=4) with six times
    /// the memory. Measured 2026-11-15 on an Apple M1 Pro, release build,
    /// under heavy load: 0.63 s per derivation when repeated, 0.67-0.72 s for
    /// a single derivation in a fresh process, up to 1.8 s at the worst
    /// moments (64 MiB took 0.1 s, 256 MiB 0.43 s). Raising them later is
    /// safe, since every file names its own parameters.
    pub const DEFAULT: KdfParams = KdfParams {
        m_cost_kib: 384 * 1024,
        t_cost: 3,
        p_cost: 4,
    };

    /// Most memory a file may ask for (RFC 9106's first recommended option
    /// uses 2 GiB). Larger values are refused before any memory is allocated.
    pub const MAX_M_COST_KIB: u32 = 2 * 1024 * 1024;
    /// Most work (memory in KiB times passes) a file may ask for: about seven
    /// times the default, some 4.5 s on the machine the default was measured on.
    pub const MAX_WORK_KIB: u64 = 8 * 1024 * 1024;
    /// Most lanes a file may ask for.
    pub const MAX_P_COST: u32 = 16;

    /// Very low parameters that keep the test suites fast. Only for tests.
    #[cfg(test)]
    pub(crate) const FOR_TESTS: KdfParams = KdfParams {
        m_cost_kib: 64,
        t_cost: 1,
        p_cost: 1,
    };

    /// Refuses parameters that would take too much memory or time, so a
    /// crafted file cannot exhaust the machine. Too small values are left to
    /// Argon2's own checks.
    fn check_limits(self) -> Result<(), ExportError> {
        let work = u64::from(self.m_cost_kib) * u64::from(self.t_cost);
        if self.m_cost_kib > Self::MAX_M_COST_KIB
            || self.p_cost > Self::MAX_P_COST
            || work > Self::MAX_WORK_KIB
        {
            return Err(ExportError::UnsupportedFormat(format!(
                "Argon2 parameters m_cost_kib={}, t_cost={}, p_cost={} exceed the limits",
                self.m_cost_kib, self.t_cost, self.p_cost
            )));
        }
        Ok(())
    }
}

// The defaults keep at least 64 MiB and stay within what this code reads.
const _: () = {
    let d = KdfParams::DEFAULT;
    assert!(d.m_cost_kib >= 64 * 1024);
    assert!(d.m_cost_kib <= KdfParams::MAX_M_COST_KIB);
    assert!(d.p_cost <= KdfParams::MAX_P_COST);
    assert!((d.m_cost_kib as u64) * (d.t_cost as u64) <= KdfParams::MAX_WORK_KIB);
};

/// An encrypted text with everything needed to decrypt it with the password.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct EncryptedSecret {
    /// Key derivation, `"argon2id"`.
    pub kdf: String,
    /// Argon2 memory in KiB.
    pub m_cost_kib: u32,
    /// Argon2 passes.
    pub t_cost: u32,
    /// Argon2 lanes.
    pub p_cost: u32,
    /// Argon2 salt, Base64.
    pub salt: String,
    /// Cipher, `"xchacha20poly1305"`.
    pub cipher: String,
    /// Cipher nonce, Base64.
    pub nonce: String,
    /// Ciphertext followed by the authentication tag, Base64.
    pub ciphertext: String,
}

/// Errors of encryption, decryption and reading or writing the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportError {
    /// The password is empty.
    EmptyPassword,
    /// The password is wrong, or the encrypted data was changed.
    WrongPassword,
    /// The file or the secret uses a format, version, algorithm or parameter
    /// this code does not support.
    UnsupportedFormat(String),
    /// The file or the secret is damaged or incomplete.
    Malformed(String),
    /// Something that does not depend on the input failed (no randomness
    /// from the operating system, YAML could not be written).
    Internal(String),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::EmptyPassword => write!(f, "the password is empty"),
            ExportError::WrongPassword => {
                write!(f, "wrong password, or the encrypted key was changed")
            }
            ExportError::UnsupportedFormat(what) => write!(f, "unsupported: {what}"),
            ExportError::Malformed(what) => write!(f, "damaged export file: {what}"),
            ExportError::Internal(what) => write!(f, "internal error: {what}"),
        }
    }
}

impl std::error::Error for ExportError {}

/// The API key that travels with an exported profile.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ExportedApiKey {
    /// The post-processing provider the key belongs to.
    pub provider_id: String,
    /// The key, encrypted with the export password.
    pub secret: EncryptedSecret,
}

/// The content of a profile export file.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ProfileExportFile<P> {
    /// Always [`FORMAT`].
    pub format: String,
    /// File version, [`VERSION`] when written by this code.
    pub version: u32,
    /// The exported profile.
    pub profile: P,
    /// The provider's API key, if the profile uses one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<ExportedApiKey>,
}

impl<P> ProfileExportFile<P> {
    /// A file of the current format and version.
    pub fn new(profile: P, api_key: Option<ExportedApiKey>) -> Self {
        Self {
            format: FORMAT.to_string(),
            version: VERSION,
            profile,
            api_key,
        }
    }
}

impl EncryptedSecret {
    /// Checks algorithms and parameters without the password, so an import
    /// can report an unsupported secret before asking for the password.
    pub fn check_supported(&self) -> Result<(), ExportError> {
        if self.kdf != KDF_ARGON2ID {
            return Err(ExportError::UnsupportedFormat(format!(
                "key derivation \"{}\"",
                self.kdf
            )));
        }
        if self.cipher != CIPHER_XCHACHA20POLY1305 {
            return Err(ExportError::UnsupportedFormat(format!(
                "cipher \"{}\"",
                self.cipher
            )));
        }
        self.params().check_limits()
    }

    fn params(&self) -> KdfParams {
        KdfParams {
            m_cost_kib: self.m_cost_kib,
            t_cost: self.t_cost,
            p_cost: self.p_cost,
        }
    }
}

/// Encrypts `plaintext` with `password`, using [`KdfParams::DEFAULT`].
pub fn encrypt_secret(password: &str, plaintext: &str) -> Result<EncryptedSecret, ExportError> {
    encrypt_secret_with(password, plaintext, KdfParams::DEFAULT)
}

/// Encrypts with lowered parameters, so tests elsewhere in the crate stay fast.
#[cfg(test)]
pub(crate) fn encrypt_secret_with_params(
    password: &str,
    plaintext: &str,
    params: KdfParams,
) -> Result<EncryptedSecret, ExportError> {
    encrypt_secret_with(password, plaintext, params)
}

fn encrypt_secret_with(
    password: &str,
    plaintext: &str,
    params: KdfParams,
) -> Result<EncryptedSecret, ExportError> {
    if password.is_empty() {
        return Err(ExportError::EmptyPassword);
    }
    // Never write a file this code would refuse to read.
    params.check_limits()?;

    let mut salt = [0u8; SALT_LEN];
    fill_random(&mut salt)?;
    let mut nonce = [0u8; NONCE_LEN];
    fill_random(&mut nonce)?;

    let key = derive_key(password, &salt, params).map_err(ExportError::Internal)?;
    let cipher = XChaCha20Poly1305::new_from_slice(&key[..])
        .map_err(|_| ExportError::Internal("wrong key length".to_string()))?;
    let ciphertext = cipher
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: plaintext.as_bytes(),
                aad: AAD,
            },
        )
        .map_err(|_| ExportError::Internal("encryption failed".to_string()))?;

    Ok(EncryptedSecret {
        kdf: KDF_ARGON2ID.to_string(),
        m_cost_kib: params.m_cost_kib,
        t_cost: params.t_cost,
        p_cost: params.p_cost,
        salt: BASE64.encode(salt),
        cipher: CIPHER_XCHACHA20POLY1305.to_string(),
        nonce: BASE64.encode(nonce),
        ciphertext: BASE64.encode(ciphertext),
    })
}

/// Decrypts `secret` with `password`, using the parameters stored in it.
pub fn decrypt_secret(password: &str, secret: &EncryptedSecret) -> Result<String, ExportError> {
    if password.is_empty() {
        return Err(ExportError::EmptyPassword);
    }
    secret.check_supported()?;

    let salt = decode_base64("salt", &secret.salt)?;
    let nonce: [u8; NONCE_LEN] = decode_base64("nonce", &secret.nonce)?
        .as_slice()
        .try_into()
        .map_err(|_| ExportError::Malformed(format!("the nonce must be {NONCE_LEN} bytes")))?;
    let ciphertext = decode_base64("ciphertext", &secret.ciphertext)?;
    if ciphertext.len() < TAG_LEN {
        return Err(ExportError::Malformed(
            "the ciphertext is shorter than its authentication tag".to_string(),
        ));
    }

    let key = derive_key(password, &salt, secret.params()).map_err(ExportError::Malformed)?;
    let cipher = XChaCha20Poly1305::new_from_slice(&key[..])
        .map_err(|_| ExportError::Internal("wrong key length".to_string()))?;
    let plaintext = cipher
        .decrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &ciphertext,
                aad: AAD,
            },
        )
        .map_err(|_| ExportError::WrongPassword)?;

    String::from_utf8(plaintext).map_err(|err| {
        let mut bytes = err.into_bytes();
        bytes.zeroize();
        ExportError::Malformed("the decrypted key is not UTF-8 text".to_string())
    })
}

/// Stretches the password into the cipher key. The key is wiped when dropped.
fn derive_key(
    password: &str,
    salt: &[u8],
    params: KdfParams,
) -> Result<Zeroizing<[u8; KEY_LEN]>, String> {
    let argon_params = Params::new(
        params.m_cost_kib,
        params.t_cost,
        params.p_cost,
        Some(KEY_LEN),
    )
    .map_err(|err| format!("invalid Argon2 parameters: {err}"))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon_params);
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    argon
        .hash_password_into(password.as_bytes(), salt, &mut key[..])
        .map_err(|err| format!("key derivation failed: {err}"))?;
    Ok(key)
}

fn fill_random(buf: &mut [u8]) -> Result<(), ExportError> {
    getrandom::fill(buf).map_err(|err| {
        ExportError::Internal(format!("no randomness from the operating system: {err}"))
    })
}

fn decode_base64(field: &str, value: &str) -> Result<Vec<u8>, ExportError> {
    BASE64
        .decode(value)
        .map_err(|_| ExportError::Malformed(format!("{field} is not valid Base64")))
}

/// Writes the file as YAML.
pub fn to_yaml<P: Serialize>(file: &ProfileExportFile<P>) -> Result<String, ExportError> {
    let body = serde_saphyr::to_string(file)
        .map_err(|err| ExportError::Internal(format!("cannot write YAML: {err}")))?;
    let mut text = String::from(FILE_COMMENT);
    if file.api_key.is_some() {
        text.push_str(KEY_COMMENT);
    }
    text.push_str(&body);
    Ok(text)
}

/// The fields read before the rest, so a newer file is reported as such even
/// if its body no longer fits the profile type.
#[derive(Deserialize)]
struct FileHeader {
    format: String,
    version: u32,
}

/// Reads a file written by [`to_yaml`]. Needs no password: the key stays
/// encrypted until [`decrypt_secret`], so a profile can always be imported
/// without its key.
pub fn from_yaml<P: DeserializeOwned>(text: &str) -> Result<ProfileExportFile<P>, ExportError> {
    let header: FileHeader = serde_saphyr::from_str(text)
        .map_err(|err| ExportError::Malformed(format!("not a profile export file: {err}")))?;
    if header.format != FORMAT {
        return Err(ExportError::UnsupportedFormat(format!(
            "\"{}\" is not a DanyWhere profile",
            header.format
        )));
    }
    if header.version == 0 || header.version > VERSION {
        return Err(ExportError::UnsupportedFormat(format!(
            "file version {} (this DanyWhere reads version {VERSION})",
            header.version
        )));
    }
    serde_saphyr::from_str(text).map_err(|err| ExportError::Malformed(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const PASSWORD: &str = "correct horse battery staple";
    /// A dummy key, never a real one.
    const API_KEY: &str = "sk-dummy-0123456789-abcdefghijklmnopqrstuvwxyz-DUMMY";

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
    #[serde(rename_all = "snake_case")]
    enum Detection {
        Auto,
        Fixed,
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
    struct Prompt {
        id: String,
        name: String,
        prompt: String,
    }

    /// Shaped like DanyWhere's profile: nested structs, enums, maps,
    /// options, and strings that YAML might misread.
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
    struct TestProfile {
        id: String,
        name: String,
        detection: Detection,
        window_secs: u32,
        enabled: bool,
        words: Vec<String>,
        models: HashMap<String, String>,
        prompts: Vec<Prompt>,
        selected_prompt: Option<String>,
        filler_words: Option<Vec<String>>,
        apps: Vec<String>,
    }

    fn sample_profile() -> TestProfile {
        let mut models = HashMap::new();
        models.insert("anthropic".to_string(), "claude-model-x".to_string());
        models.insert("openai".to_string(), "gpt-model-y".to_string());
        TestProfile {
            id: "profile-1".to_string(),
            name: "Büro: Diktat #1 – \"Entwürfe\"".to_string(),
            detection: Detection::Auto,
            window_secs: 30,
            enabled: true,
            words: vec![
                "yes".to_string(),
                "no".to_string(),
                "null".to_string(),
                "~".to_string(),
                "0123".to_string(),
                "1e3".to_string(),
                "true".to_string(),
                "  leading spaces".to_string(),
                "key: value".to_string(),
                "- dash".to_string(),
                "Grüße 😀".to_string(),
                "!tag".to_string(),
                "&anchor".to_string(),
                "*alias".to_string(),
                String::new(),
            ],
            models,
            prompts: vec![
                Prompt {
                    id: "p1".to_string(),
                    name: "Diktat (DE/EN)".to_string(),
                    prompt: "Correct the transcript.\n\n- Keep the language.\n- Fix: punctuation\n\ttab line\nlast line without newline".to_string(),
                },
                Prompt {
                    id: "p2".to_string(),
                    name: "Trailing newline".to_string(),
                    prompt: "Line one\nLine two\n".to_string(),
                },
            ],
            selected_prompt: Some("p1".to_string()),
            filler_words: None,
            apps: vec!["com.apple.TextEdit".to_string()],
        }
    }

    fn fast_secret(password: &str, plaintext: &str) -> EncryptedSecret {
        encrypt_secret_with_params(password, plaintext, KdfParams::FOR_TESTS)
            .expect("encryption with test parameters")
    }

    fn file_with_key() -> ProfileExportFile<TestProfile> {
        ProfileExportFile::new(
            sample_profile(),
            Some(ExportedApiKey {
                provider_id: "anthropic".to_string(),
                secret: fast_secret(PASSWORD, API_KEY),
            }),
        )
    }

    fn decoded(b64: &str) -> Vec<u8> {
        BASE64.decode(b64).expect("valid Base64")
    }

    fn flip_first_bit(b64: &str) -> String {
        let mut bytes = decoded(b64);
        bytes[0] ^= 0x01;
        BASE64.encode(bytes)
    }

    // ---- encryption -------------------------------------------------------

    #[test]
    fn secret_round_trip() {
        let secret = fast_secret(PASSWORD, API_KEY);
        assert_eq!(decrypt_secret(PASSWORD, &secret).unwrap(), API_KEY);
    }

    #[test]
    fn secret_round_trip_with_unicode_password_and_text() {
        let password = "Pässwörd ß 🔑 ünd Leerzeichen ";
        let text = "schlüssel-äöü-€-😀";
        let secret = fast_secret(password, text);
        assert_eq!(decrypt_secret(password, &secret).unwrap(), text);
    }

    #[test]
    fn default_parameters_round_trip() {
        let secret = encrypt_secret(PASSWORD, API_KEY).unwrap();
        assert_eq!(secret.kdf, KDF_ARGON2ID);
        assert_eq!(secret.cipher, CIPHER_XCHACHA20POLY1305);
        assert_eq!(secret.m_cost_kib, KdfParams::DEFAULT.m_cost_kib);
        assert_eq!(secret.t_cost, KdfParams::DEFAULT.t_cost);
        assert_eq!(secret.p_cost, KdfParams::DEFAULT.p_cost);
        assert_eq!(decrypt_secret(PASSWORD, &secret).unwrap(), API_KEY);
    }

    #[test]
    fn wrong_password_gives_wrong_password() {
        let secret = fast_secret(PASSWORD, API_KEY);
        assert_eq!(
            decrypt_secret("correct horse battery stapler", &secret),
            Err(ExportError::WrongPassword)
        );
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let mut secret = fast_secret(PASSWORD, API_KEY);
        secret.ciphertext = flip_first_bit(&secret.ciphertext);
        assert_eq!(
            decrypt_secret(PASSWORD, &secret),
            Err(ExportError::WrongPassword)
        );
    }

    #[test]
    fn tampering_any_field_is_rejected() {
        let original = fast_secret(PASSWORD, API_KEY);
        let mut variants: Vec<(&str, EncryptedSecret)> = Vec::new();

        let mut s = original.clone();
        s.salt = flip_first_bit(&s.salt);
        variants.push(("salt", s));

        let mut s = original.clone();
        s.nonce = flip_first_bit(&s.nonce);
        variants.push(("nonce", s));

        let mut s = original.clone();
        let mut ct = decoded(&s.ciphertext);
        let last = ct.len() - 1;
        ct[last] ^= 0x80;
        s.ciphertext = BASE64.encode(ct);
        variants.push(("tag", s));

        let mut s = original.clone();
        let mut ct = decoded(&s.ciphertext);
        ct.truncate(ct.len() - 1);
        s.ciphertext = BASE64.encode(ct);
        variants.push(("truncated ciphertext", s));

        let mut s = original.clone();
        s.ciphertext = BASE64.encode([0u8; 4]);
        variants.push(("ciphertext shorter than the tag", s));

        let mut s = original.clone();
        s.m_cost_kib += 8;
        variants.push(("m_cost_kib", s));

        let mut s = original.clone();
        s.t_cost += 1;
        variants.push(("t_cost", s));

        let mut s = original.clone();
        s.p_cost += 1;
        s.m_cost_kib = s.m_cost_kib.max(8 * s.p_cost);
        variants.push(("p_cost", s));

        for (what, secret) in variants {
            let result = decrypt_secret(PASSWORD, &secret);
            assert!(result.is_err(), "tampered {what} must fail, got {result:?}");
        }
    }

    #[test]
    fn invalid_base64_is_malformed() {
        let original = fast_secret(PASSWORD, API_KEY);
        for field in ["salt", "nonce", "ciphertext"] {
            let mut s = original.clone();
            let bad = "not base64 !!".to_string();
            match field {
                "salt" => s.salt = bad,
                "nonce" => s.nonce = bad,
                _ => s.ciphertext = bad,
            }
            assert!(
                matches!(decrypt_secret(PASSWORD, &s), Err(ExportError::Malformed(_))),
                "{field}"
            );
        }
    }

    #[test]
    fn wrong_nonce_length_is_malformed() {
        let mut secret = fast_secret(PASSWORD, API_KEY);
        secret.nonce = BASE64.encode([0u8; 12]);
        assert!(matches!(
            decrypt_secret(PASSWORD, &secret),
            Err(ExportError::Malformed(_))
        ));
    }

    #[test]
    fn two_encryptions_of_the_same_text_differ() {
        let a = fast_secret(PASSWORD, API_KEY);
        let b = fast_secret(PASSWORD, API_KEY);
        assert_ne!(a.salt, b.salt, "salt");
        assert_ne!(a.nonce, b.nonce, "nonce");
        assert_ne!(a.ciphertext, b.ciphertext, "ciphertext");
        assert_eq!(decrypt_secret(PASSWORD, &a).unwrap(), API_KEY);
        assert_eq!(decrypt_secret(PASSWORD, &b).unwrap(), API_KEY);
    }

    #[test]
    fn salt_nonce_and_tag_have_the_specified_lengths() {
        let secret = fast_secret(PASSWORD, API_KEY);
        assert_eq!(decoded(&secret.salt).len(), 16);
        assert_eq!(decoded(&secret.nonce).len(), 24);
        assert_eq!(decoded(&secret.ciphertext).len(), API_KEY.len() + 16);
    }

    #[test]
    fn empty_password_is_rejected() {
        assert_eq!(encrypt_secret("", API_KEY), Err(ExportError::EmptyPassword));
        assert_eq!(
            encrypt_secret_with_params("", API_KEY, KdfParams::FOR_TESTS),
            Err(ExportError::EmptyPassword)
        );
        let secret = fast_secret(PASSWORD, API_KEY);
        assert_eq!(decrypt_secret("", &secret), Err(ExportError::EmptyPassword));
    }

    #[test]
    fn unknown_kdf_is_unsupported() {
        let mut secret = fast_secret(PASSWORD, API_KEY);
        secret.kdf = "scrypt".to_string();
        assert!(matches!(
            secret.check_supported(),
            Err(ExportError::UnsupportedFormat(_))
        ));
        assert!(matches!(
            decrypt_secret(PASSWORD, &secret),
            Err(ExportError::UnsupportedFormat(_))
        ));
    }

    #[test]
    fn unknown_cipher_is_unsupported() {
        let mut secret = fast_secret(PASSWORD, API_KEY);
        secret.cipher = "aes-256-gcm".to_string();
        assert!(matches!(
            secret.check_supported(),
            Err(ExportError::UnsupportedFormat(_))
        ));
        assert!(matches!(
            decrypt_secret(PASSWORD, &secret),
            Err(ExportError::UnsupportedFormat(_))
        ));
    }

    #[test]
    fn parameters_beyond_the_limits_are_unsupported_without_deriving() {
        // Would need 4 TiB of memory if it were derived.
        let mut secret = fast_secret(PASSWORD, API_KEY);
        secret.m_cost_kib = u32::MAX;
        assert!(matches!(
            secret.check_supported(),
            Err(ExportError::UnsupportedFormat(_))
        ));
        assert!(matches!(
            decrypt_secret(PASSWORD, &secret),
            Err(ExportError::UnsupportedFormat(_))
        ));
    }

    #[test]
    fn supported_secret_passes_the_check() {
        let secret = fast_secret(PASSWORD, API_KEY);
        assert_eq!(secret.check_supported(), Ok(()));
    }

    #[test]
    fn weaker_and_stronger_parameters_from_the_file_both_decrypt() {
        let weak = fast_secret(PASSWORD, API_KEY);
        let stronger = encrypt_secret_with_params(
            PASSWORD,
            API_KEY,
            KdfParams {
                m_cost_kib: 512,
                t_cost: 2,
                p_cost: 2,
            },
        )
        .unwrap();
        assert_eq!(stronger.m_cost_kib, 512);
        assert_eq!(stronger.t_cost, 2);
        assert_eq!(stronger.p_cost, 2);
        assert_eq!(decrypt_secret(PASSWORD, &weak).unwrap(), API_KEY);
        assert_eq!(decrypt_secret(PASSWORD, &stronger).unwrap(), API_KEY);
    }

    // ---- file -------------------------------------------------------------

    #[test]
    fn new_file_has_the_current_format_and_version() {
        let file = ProfileExportFile::new(sample_profile(), None);
        assert_eq!(file.format, FORMAT);
        assert_eq!(file.version, VERSION);
        assert_eq!(file.profile, sample_profile());
        assert_eq!(file.api_key, None);
    }

    #[test]
    fn file_round_trip_with_key() {
        let file = file_with_key();
        let yaml = to_yaml(&file).unwrap();
        let back: ProfileExportFile<TestProfile> = from_yaml(&yaml).unwrap();
        assert_eq!(back, file);
        let key = back.api_key.expect("key present");
        assert_eq!(key.provider_id, "anthropic");
        assert_eq!(decrypt_secret(PASSWORD, &key.secret).unwrap(), API_KEY);
    }

    #[test]
    fn file_without_key_round_trip_omits_api_key() {
        let file = ProfileExportFile::new(sample_profile(), None);
        let yaml = to_yaml(&file).unwrap();
        assert!(!yaml.contains("api_key"), "{yaml}");
        let back: ProfileExportFile<TestProfile> = from_yaml(&yaml).unwrap();
        assert_eq!(back, file);
        assert_eq!(back.api_key, None);
    }

    #[test]
    fn import_without_password_reads_the_profile() {
        // The file is read without a password; the key stays encrypted and
        // can simply be left out ("import without key").
        let yaml = to_yaml(&file_with_key()).unwrap();
        let back: ProfileExportFile<TestProfile> = from_yaml(&yaml).unwrap();
        assert_eq!(back.profile, sample_profile());
        assert!(back.api_key.is_some());
    }

    #[test]
    fn import_without_key_works_even_if_the_secret_is_unsupported() {
        let yaml = to_yaml(&file_with_key()).unwrap();
        assert!(yaml.contains("kdf: argon2id"), "{yaml}");
        let yaml = yaml.replace("kdf: argon2id", "kdf: scrypt");
        let back: ProfileExportFile<TestProfile> = from_yaml(&yaml).unwrap();
        assert_eq!(back.profile, sample_profile());
        let secret = back.api_key.unwrap().secret;
        assert!(matches!(
            decrypt_secret(PASSWORD, &secret),
            Err(ExportError::UnsupportedFormat(_))
        ));
    }

    #[test]
    fn higher_or_zero_version_is_unsupported() {
        let yaml = to_yaml(&file_with_key()).unwrap();
        assert!(yaml.contains("version: 1\n"), "{yaml}");
        for version in ["version: 2\n", "version: 0\n", "version: 4000000000\n"] {
            let changed = yaml.replace("version: 1\n", version);
            assert!(
                matches!(
                    from_yaml::<TestProfile>(&changed),
                    Err(ExportError::UnsupportedFormat(_))
                ),
                "{version}"
            );
        }
    }

    #[test]
    fn higher_version_with_a_different_body_is_unsupported_not_malformed() {
        let yaml = "format: danywhere-profile\nversion: 2\nprofiles:\n  - something: new\n";
        assert!(matches!(
            from_yaml::<TestProfile>(yaml),
            Err(ExportError::UnsupportedFormat(_))
        ));
    }

    #[test]
    fn unknown_format_is_unsupported() {
        let yaml = to_yaml(&file_with_key()).unwrap();
        let changed = yaml.replace("format: danywhere-profile", "format: handy-profile");
        assert!(matches!(
            from_yaml::<TestProfile>(&changed),
            Err(ExportError::UnsupportedFormat(_))
        ));
    }

    #[test]
    fn malformed_yaml_is_malformed() {
        for text in [
            "",
            "format: [unclosed",
            "just a string",
            "version: 1\n",
            "format: danywhere-profile\nversion: 1\n",
            "format: danywhere-profile\nversion: one\nprofile: {}\n",
            "format: danywhere-profile\nversion: 1\nprofile:\n  id: only-an-id\n",
        ] {
            assert!(
                matches!(
                    from_yaml::<TestProfile>(text),
                    Err(ExportError::Malformed(_))
                ),
                "{text:?} gave {:?}",
                from_yaml::<TestProfile>(text).map(|_| ())
            );
        }
    }

    #[test]
    fn yaml_is_human_readable() {
        let file = file_with_key();
        let yaml = to_yaml(&file).unwrap();
        let secret = &file.api_key.as_ref().unwrap().secret;
        for expected in [
            "format: danywhere-profile\n".to_string(),
            "version: 1\n".to_string(),
            "provider_id: anthropic\n".to_string(),
            "kdf: argon2id\n".to_string(),
            format!("m_cost_kib: {}\n", secret.m_cost_kib),
            format!("t_cost: {}\n", secret.t_cost),
            format!("p_cost: {}\n", secret.p_cost),
            "cipher: xchacha20poly1305\n".to_string(),
            "salt: ".to_string(),
            "nonce: ".to_string(),
            "ciphertext: ".to_string(),
        ] {
            assert!(yaml.contains(&expected), "missing {expected:?} in\n{yaml}");
        }
        // Base64 values appear as plain strings, without binary tags.
        for value in [&secret.salt, &secret.nonce, &secret.ciphertext] {
            assert!(yaml.contains(value.as_str()), "{value} not in\n{yaml}");
        }
        assert!(!yaml.contains("!!"), "no YAML tags:\n{yaml}");
        assert!(!yaml.contains(": !"), "no YAML tags:\n{yaml}");
        // Only line breaks and the tab that the sample prompt itself contains.
        assert!(
            yaml.chars()
                .all(|c| c == '\n' || c == '\t' || !c.is_control()),
            "no other control characters:\n{yaml:?}"
        );
    }

    #[test]
    fn plaintext_key_never_appears_in_the_yaml() {
        let file = file_with_key();
        let yaml = to_yaml(&file).unwrap();
        assert!(!yaml.contains(API_KEY));
        assert!(!yaml.contains("0123456789-abcdefghijklmnopqrstuvwxyz"));
        assert!(!yaml.contains(&BASE64.encode(API_KEY)));
        assert!(!format!("{file:?}").contains(API_KEY));
    }

    #[test]
    fn tricky_strings_survive_the_round_trip() {
        let file = ProfileExportFile::new(sample_profile(), None);
        let yaml = to_yaml(&file).unwrap();
        let back: ProfileExportFile<TestProfile> = from_yaml(&yaml).unwrap();
        assert_eq!(back.profile.words, sample_profile().words, "\n{yaml}");
        assert_eq!(back.profile.prompts, sample_profile().prompts, "\n{yaml}");
        assert_eq!(back.profile.name, sample_profile().name, "\n{yaml}");
    }

    #[test]
    fn long_and_multiline_strings_survive_the_round_trip() {
        let long_sentence = "This prompt is a single long line with  double spaces, \
             a tab\there, and   more words so that it is much longer than eighty \
             characters, followed by a trailing space ";
        let texts: Vec<String> = vec![
            long_sentence.to_string(),
            format!("   {long_sentence}"),
            format!("{long_sentence}{long_sentence}"),
            "line one\n  indented second\n\ttab third\n".to_string(),
            "  leading spaces on the first line\nsecond".to_string(),
            "trailing spaces   \nnext   ".to_string(),
            "\n\nstarts with newlines".to_string(),
            "ends with many newlines\n\n\n".to_string(),
            "windows\r\nline endings\r\n".to_string(),
            "only\n".to_string(),
            "\n".to_string(),
            "a\n---\nb\n...\nc".to_string(),
            "# not a comment\nvalue # still text".to_string(),
            "x".repeat(300),
            "ünïcödé ".repeat(20),
        ];
        let mut profile = sample_profile();
        profile.words = texts.clone();
        profile.prompts = texts
            .iter()
            .enumerate()
            .map(|(i, text)| Prompt {
                id: format!("p{i}"),
                name: format!("prompt {i}"),
                prompt: text.clone(),
            })
            .collect();
        let file = ProfileExportFile::new(profile.clone(), None);
        let yaml = to_yaml(&file).unwrap();
        let back: ProfileExportFile<TestProfile> = from_yaml(&yaml).unwrap();
        for (i, text) in texts.iter().enumerate() {
            assert_eq!(&back.profile.words[i], text, "word {i} in\n{yaml}");
            assert_eq!(
                &back.profile.prompts[i].prompt, text,
                "prompt {i} in\n{yaml}"
            );
        }
        assert_eq!(back.profile, profile);
    }
}
