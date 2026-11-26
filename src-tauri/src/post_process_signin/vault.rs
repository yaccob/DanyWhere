//! DanyWhere: the sign-ins' secrets, kept out of the settings (lead's
//! decision, 2026-11-26): `get_app_settings` sends the settings to the
//! webview, which never needs a token. ChatGPT's session (tokens, account,
//! expiry) and the client id ChatGPT issued live in one JSON file of their
//! own in the app's data directory, [`FILE_NAME`], written atomically (a
//! temporary file renamed over it) and readable by the user only. The
//! settings keep only `chatgpt_signed_in` and the host id, which is no
//! secret.

use super::chatgpt::ChatGptSession;
use crate::post_process_setup::{ChatGptAccount, ChatGptSessionStored};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, PoisonError};
use tauri::AppHandle;

/// The file's name in the app's data directory.
pub const FILE_NAME: &str = "post_process_signin.json";

/// What the file holds.
#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Eq, Debug)]
#[serde(default)]
pub struct Secrets {
    /// The client id ChatGPT issued at the first registration; it stays
    /// when the session ends, so the next sign-in uses it.
    pub chatgpt_client_id: Option<String>,
    /// ChatGPT's session; `None` once signed out or expired. Its `Debug`
    /// hides the tokens.
    pub chatgpt_session: Option<ChatGptSessionStored>,
}

impl Secrets {
    /// The stored session, if it has a refresh token.
    fn live_session(&self) -> Option<&ChatGptSessionStored> {
        self.chatgpt_session
            .as_ref()
            .filter(|session| !session.refresh_token.is_empty())
    }

    /// The ChatGPT session, if one with a refresh token is stored.
    pub fn chatgpt(&self) -> Option<ChatGptSession> {
        self.live_session().map(ChatGptSession::from)
    }

    /// What the setup shows of the ChatGPT sign-in.
    pub fn chatgpt_account(&self) -> Option<ChatGptAccount> {
        self.live_session().map(|session| ChatGptAccount {
            email: session.email.clone(),
            plan: session.plan.clone(),
        })
    }
}

impl From<&ChatGptSession> for ChatGptSessionStored {
    fn from(session: &ChatGptSession) -> Self {
        ChatGptSessionStored {
            client_id: session.client_id.clone(),
            access_token: session.access_token.clone(),
            refresh_token: session.refresh_token.clone(),
            expires_at: session.expires_at,
            earliest_refresh_at: session.earliest_refresh_at,
            subject: Some(session.subject.clone()).filter(|subject| !subject.is_empty()),
            email: session.email.clone(),
            plan: session.plan.clone(),
        }
    }
}

impl From<&ChatGptSessionStored> for ChatGptSession {
    fn from(stored: &ChatGptSessionStored) -> Self {
        ChatGptSession {
            client_id: stored.client_id.clone(),
            access_token: stored.access_token.clone(),
            refresh_token: stored.refresh_token.clone(),
            expires_at: stored.expires_at,
            email: stored.email.clone(),
            plan: stored.plan.clone(),
            subject: stored.subject.clone().unwrap_or_default(),
            earliest_refresh_at: stored.earliest_refresh_at,
        }
    }
}

/// The file, read and written one change at a time.
pub struct Vault {
    path: PathBuf,
    lock: Mutex<()>,
}

impl Vault {
    pub fn new(path: PathBuf) -> Vault {
        Vault {
            path,
            lock: Mutex::new(()),
        }
    }

    #[cfg(test)]
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// What the file holds; nothing when it is missing or unreadable.
    pub fn load(&self) -> Secrets {
        let _held = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        self.read()
    }

    /// Changes what the file holds and writes it at once.
    pub fn update<R>(&self, change: impl FnOnce(&mut Secrets) -> R) -> Result<R, String> {
        let _held = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut secrets = self.read();
        let result = change(&mut secrets);
        self.write(&secrets)?;
        Ok(result)
    }

    fn read(&self) -> Secrets {
        let text = match std::fs::read(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Secrets::default()
            }
            Err(error) => {
                log::error!("Cannot read the sign-in file: {error}");
                return Secrets::default();
            }
        };
        serde_json::from_slice(&text).unwrap_or_else(|error| {
            // serde's message names only where it broke, not the content.
            log::error!(
                "The sign-in file is unreadable (line {}, column {}); starting empty",
                error.line(),
                error.column()
            );
            Secrets::default()
        })
    }

    /// Writes `secrets` into a temporary file beside the file, readable by
    /// the user only, and renames it over the file.
    fn write(&self, secrets: &Secrets) -> Result<(), String> {
        use std::io::Write;
        let text = serde_json::to_vec_pretty(secrets)
            .map_err(|error| format!("Cannot encode the sign-in file: {error}"))?;
        let directory = self
            .path
            .parent()
            .ok_or_else(|| "The sign-in file has no directory".to_string())?;
        std::fs::create_dir_all(directory)
            .map_err(|error| format!("Cannot create the sign-in file's directory: {error}"))?;
        let temporary = directory.join(format!(".{FILE_NAME}.{}.tmp", std::process::id()));
        let _ = std::fs::remove_file(&temporary);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let written = options
            .open(&temporary)
            .and_then(|mut file| {
                file.write_all(&text)?;
                file.sync_all()
            })
            .and_then(|()| std::fs::rename(&temporary, &self.path));
        if let Err(error) = written {
            let _ = std::fs::remove_file(&temporary);
            return Err(format!("Cannot write the sign-in file: {error}"));
        }
        Ok(())
    }
}

/// The app's file, in its data directory.
pub fn for_app(app: &AppHandle) -> Result<&'static Vault, String> {
    static APP_VAULT: OnceLock<Vault> = OnceLock::new();
    if let Some(vault) = APP_VAULT.get() {
        return Ok(vault);
    }
    let directory = crate::portable::app_data_dir(app)
        .map_err(|error| format!("No app data directory for the sign-in: {error}"))?;
    Ok(APP_VAULT.get_or_init(|| Vault::new(directory.join(FILE_NAME))))
}

/// What the setup shows of the app's ChatGPT sign-in.
pub fn chatgpt_account(app: &AppHandle) -> Option<ChatGptAccount> {
    for_app(app).ok()?.load().chatgpt_account()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault_in(dir: &tempfile::TempDir) -> Vault {
        Vault::new(dir.path().join(FILE_NAME))
    }

    fn session() -> ChatGptSession {
        ChatGptSession {
            client_id: "app_client_1795300000001".to_string(),
            access_token: "chatgpt-access-secret".to_string(),
            refresh_token: "chatgpt-refresh-secret".to_string(),
            expires_at: 1_795_400_000,
            email: Some("someone@example.com".to_string()),
            plan: Some("plus".to_string()),
            subject: "user-subject-1".to_string(),
            earliest_refresh_at: Some(1_795_399_700),
        }
    }

    #[test]
    fn a_missing_file_holds_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(vault_in(&dir).load(), Secrets::default());
    }

    #[test]
    fn a_change_is_written_at_once_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let vault = vault_in(&dir);
        vault
            .update(|secrets| {
                secrets.chatgpt_client_id = Some("app_client_1".to_string());
                secrets.chatgpt_session = Some((&session()).into());
            })
            .unwrap();
        // A new reader of the same file sees it.
        let again = vault_in(&dir).load();
        assert_eq!(again.chatgpt_client_id.as_deref(), Some("app_client_1"));
        let read = again.chatgpt().expect("the session");
        assert_eq!(read.access_token, "chatgpt-access-secret");
        assert_eq!(read.refresh_token, "chatgpt-refresh-secret");
        let text = std::fs::read_to_string(vault.path()).unwrap();
        assert!(text.contains("chatgpt-refresh-secret"));
    }

    #[test]
    fn the_file_is_replaced_whole_and_no_temporary_file_stays() {
        let dir = tempfile::tempdir().unwrap();
        let vault = vault_in(&dir);
        vault
            .update(|secrets| secrets.chatgpt_client_id = Some("one".to_string()))
            .unwrap();
        vault
            .update(|secrets| secrets.chatgpt_client_id = Some("two".to_string()))
            .unwrap();
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec![FILE_NAME.to_string()]);
        assert_eq!(vault.load().chatgpt_client_id.as_deref(), Some("two"));
    }

    #[cfg(unix)]
    #[test]
    fn only_the_user_may_read_the_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let vault = vault_in(&dir);
        vault
            .update(|secrets| secrets.chatgpt_client_id = Some("x".to_string()))
            .unwrap();
        let mode = std::fs::metadata(vault.path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "{mode:o}");
    }

    #[test]
    fn a_broken_file_holds_nothing_and_is_replaced_by_the_next_change() {
        let dir = tempfile::tempdir().unwrap();
        let vault = vault_in(&dir);
        std::fs::write(vault.path(), "{not json").unwrap();
        assert_eq!(vault.load(), Secrets::default());
        vault
            .update(|secrets| secrets.chatgpt_client_id = Some("new".to_string()))
            .unwrap();
        assert_eq!(vault.load().chatgpt_client_id.as_deref(), Some("new"));
    }

    #[test]
    fn changes_from_two_threads_both_land() {
        let dir = tempfile::tempdir().unwrap();
        let vault = std::sync::Arc::new(vault_in(&dir));
        let other = vault.clone();
        let thread = std::thread::spawn(move || {
            other
                .update(|secrets| secrets.chatgpt_client_id = Some("client".to_string()))
                .unwrap();
        });
        vault
            .update(|secrets| secrets.chatgpt_session = Some((&session()).into()))
            .unwrap();
        thread.join().unwrap();
        let both = vault.load();
        assert_eq!(both.chatgpt_client_id.as_deref(), Some("client"));
        assert!(both.chatgpt_session.is_some());
    }

    #[test]
    fn the_stored_session_keeps_every_field_and_an_empty_subject_is_none() {
        let stored: ChatGptSessionStored = (&session()).into();
        assert_eq!(stored.client_id, "app_client_1795300000001");
        assert_eq!(stored.subject.as_deref(), Some("user-subject-1"));
        assert_eq!(stored.earliest_refresh_at, Some(1_795_399_700));
        assert_eq!(stored.email.as_deref(), Some("someone@example.com"));
        assert_eq!(stored.plan.as_deref(), Some("plus"));
        let back: ChatGptSession = (&stored).into();
        assert_eq!(back.subject, "user-subject-1");
        assert_eq!(back.expires_at, 1_795_400_000);
        assert_eq!(back.access_token, "chatgpt-access-secret");
        let mut without = session();
        without.subject = String::new();
        let stored: ChatGptSessionStored = (&without).into();
        assert_eq!(stored.subject, None);
    }

    #[test]
    fn a_session_without_refresh_token_is_no_sign_in() {
        let mut stored: ChatGptSessionStored = (&session()).into();
        let secrets = Secrets {
            chatgpt_client_id: None,
            chatgpt_session: Some(stored.clone()),
        };
        assert!(secrets.chatgpt().is_some());
        stored.refresh_token.clear();
        let secrets = Secrets {
            chatgpt_client_id: None,
            chatgpt_session: Some(stored),
        };
        assert!(secrets.chatgpt().is_none());
        assert!(secrets.chatgpt_account().is_none());
    }

    #[test]
    fn the_account_shows_email_and_plan_only() {
        let secrets = Secrets {
            chatgpt_client_id: Some("c".to_string()),
            chatgpt_session: Some((&session()).into()),
        };
        assert_eq!(
            secrets.chatgpt_account(),
            Some(ChatGptAccount {
                email: Some("someone@example.com".to_string()),
                plan: Some("plus".to_string()),
            })
        );
        let dump = format!("{secrets:?}");
        assert!(!dump.contains("chatgpt-access-secret"), "{dump}");
        assert!(!dump.contains("chatgpt-refresh-secret"), "{dump}");
    }
}
