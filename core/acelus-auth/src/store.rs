use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::chain::Account;
use crate::secret::Secret;

pub const KEYRING_SERVICE: &str = "acelus";
pub const REFRESH_TOKEN_KEY: &str = "refresh";
pub const SESSION_TOKEN_KEY: &str = "session";
pub const REFRESH_MARGIN_SECONDS: u64 = 300;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the account registry at {path} could not be read or written")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("the account registry at {path} is not valid JSON")]
    Corrupt {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("the operating system credential store refused the request")]
    Secrets {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("no account is stored with the id {uuid}")]
    UnknownAccount { uuid: String },
}

pub type Result<T> = std::result::Result<T, Error>;

pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<Secret>>;
    fn set(&self, key: &str, value: &Secret) -> Result<()>;
    fn delete(&self, key: &str) -> Result<()>;
}

#[derive(Debug, Default)]
pub struct MemorySecrets {
    entries: Mutex<HashMap<String, String>>,
}

impl MemorySecrets {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemorySecrets {
    fn get(&self, key: &str) -> Result<Option<Secret>> {
        Ok(self
            .entries
            .lock()
            .expect("the secret map is never poisoned")
            .get(key)
            .map(Secret::new))
    }

    fn set(&self, key: &str, value: &Secret) -> Result<()> {
        self.entries
            .lock()
            .expect("the secret map is never poisoned")
            .insert(key.to_string(), value.expose().to_string());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<()> {
        self.entries
            .lock()
            .expect("the secret map is never poisoned")
            .remove(key);
        Ok(())
    }
}

pub struct KeyringSecrets {
    service: String,
}

impl KeyringSecrets {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(&self, key: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(&self.service, key).map_err(|source| Error::Secrets {
            source: Box::new(source),
        })
    }
}

impl Default for KeyringSecrets {
    fn default() -> Self {
        Self::new(KEYRING_SERVICE)
    }
}

impl SecretStore for KeyringSecrets {
    fn get(&self, key: &str) -> Result<Option<Secret>> {
        match self.entry(key)?.get_password() {
            Ok(value) => Ok(Some(Secret::new(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(source) => Err(Error::Secrets {
                source: Box::new(source),
            }),
        }
    }

    fn set(&self, key: &str, value: &Secret) -> Result<()> {
        self.entry(key)?
            .set_password(value.expose())
            .map_err(|source| Error::Secrets {
                source: Box::new(source),
            })
    }

    fn delete(&self, key: &str) -> Result<()> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(source) => Err(Error::Secrets {
                source: Box::new(source),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredAccount {
    pub uuid: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xuid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skin_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cape_url: Option<String>,
    pub entitlement_verified: bool,
    pub expires_at: u64,
}

impl StoredAccount {
    pub fn is_expired(&self, now: u64) -> bool {
        self.expires_at <= now
    }

    pub fn needs_refresh(&self, now: u64) -> bool {
        self.expires_at <= now.saturating_add(REFRESH_MARGIN_SECONDS)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Registry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    #[serde(default)]
    pub accounts: Vec<StoredAccount>,
}

impl Registry {
    pub fn find(&self, uuid: &str) -> Option<&StoredAccount> {
        self.accounts.iter().find(|account| account.uuid == uuid)
    }

    pub fn active_account(&self) -> Option<&StoredAccount> {
        self.active.as_deref().and_then(|uuid| self.find(uuid))
    }
}

pub struct AccountStore {
    path: PathBuf,
    secrets: Box<dyn SecretStore>,
}

impl AccountStore {
    pub fn new(path: impl Into<PathBuf>, secrets: Box<dyn SecretStore>) -> Self {
        Self {
            path: path.into(),
            secrets,
        }
    }

    pub fn with_keyring(path: impl Into<PathBuf>) -> Self {
        Self::new(path, Box::new(KeyringSecrets::default()))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<Registry> {
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|source| Error::Corrupt {
                path: self.path.clone(),
                source,
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Registry::default()),
            Err(source) => Err(Error::Io {
                path: self.path.clone(),
                source,
            }),
        }
    }

    fn save(&self, registry: &Registry) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| Error::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let json = serde_json::to_vec_pretty(registry).map_err(|source| Error::Corrupt {
            path: self.path.clone(),
            source,
        })?;

        std::fs::write(&self.path, json).map_err(|source| Error::Io {
            path: self.path.clone(),
            source,
        })
    }

    pub fn remember(&self, account: &Account, now: u64) -> Result<StoredAccount> {
        let stored = StoredAccount {
            uuid: account.uuid.clone(),
            name: account.name.clone(),
            xuid: account.xuid.clone(),
            skin_url: account.skin_url.clone(),
            cape_url: account.cape_url.clone(),
            entitlement_verified: account.entitlement_verified,
            expires_at: now.saturating_add(account.expires_in),
        };

        if let Some(refresh) = account.refresh_token.as_ref() {
            self.secrets
                .set(&key(&account.uuid, REFRESH_TOKEN_KEY), refresh)?;
        }
        self.secrets.set(
            &key(&account.uuid, SESSION_TOKEN_KEY),
            &account.minecraft_token,
        )?;

        let mut registry = self.load()?;
        registry.accounts.retain(|held| held.uuid != stored.uuid);
        registry.accounts.push(stored.clone());
        if registry.active.is_none() {
            registry.active = Some(stored.uuid.clone());
        }
        self.save(&registry)?;

        Ok(stored)
    }

    pub fn refresh_token(&self, uuid: &str) -> Result<Option<Secret>> {
        self.secrets.get(&key(uuid, REFRESH_TOKEN_KEY))
    }

    pub fn session_token(&self, uuid: &str) -> Result<Option<Secret>> {
        self.secrets.get(&key(uuid, SESSION_TOKEN_KEY))
    }

    pub fn set_active(&self, uuid: &str) -> Result<()> {
        let mut registry = self.load()?;
        if registry.find(uuid).is_none() {
            return Err(Error::UnknownAccount {
                uuid: uuid.to_string(),
            });
        }
        registry.active = Some(uuid.to_string());
        self.save(&registry)
    }

    pub fn forget(&self, uuid: &str) -> Result<()> {
        let mut registry = self.load()?;
        if registry.find(uuid).is_none() {
            return Err(Error::UnknownAccount {
                uuid: uuid.to_string(),
            });
        }

        registry.accounts.retain(|account| account.uuid != uuid);
        if registry.active.as_deref() == Some(uuid) {
            registry.active = registry
                .accounts
                .first()
                .map(|account| account.uuid.clone());
        }
        self.save(&registry)?;

        self.secrets.delete(&key(uuid, REFRESH_TOKEN_KEY))?;
        self.secrets.delete(&key(uuid, SESSION_TOKEN_KEY))?;

        Ok(())
    }
}

fn key(uuid: &str, kind: &str) -> String {
    format!("{uuid}:{kind}")
}

pub fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_account_is_due_for_refresh_before_it_actually_expires() {
        let account = StoredAccount {
            uuid: "u".into(),
            name: "n".into(),
            xuid: None,
            skin_url: None,
            cape_url: None,
            entitlement_verified: true,
            expires_at: 1000,
        };

        assert!(!account.is_expired(500));
        assert!(!account.needs_refresh(500));

        assert!(
            account.needs_refresh(1000 - REFRESH_MARGIN_SECONDS),
            "a token about to expire should be renewed before it is used"
        );
        assert!(!account.is_expired(1000 - REFRESH_MARGIN_SECONDS));

        assert!(account.is_expired(1000));
        assert!(account.is_expired(2000));
    }

    #[test]
    fn secrets_are_namespaced_per_account_and_kind() {
        assert_eq!(key("uuid-a", REFRESH_TOKEN_KEY), "uuid-a:refresh");
        assert_eq!(key("uuid-a", SESSION_TOKEN_KEY), "uuid-a:session");
        assert_ne!(
            key("uuid-a", REFRESH_TOKEN_KEY),
            key("uuid-b", REFRESH_TOKEN_KEY)
        );
    }

    #[test]
    fn the_in_memory_secret_store_round_trips() {
        let secrets = MemorySecrets::new();
        assert_eq!(secrets.get("missing").unwrap(), None);

        secrets.set("k", &Secret::new("value")).unwrap();
        assert_eq!(secrets.get("k").unwrap().unwrap().expose(), "value");

        secrets.delete("k").unwrap();
        assert_eq!(secrets.get("k").unwrap(), None);
    }
}
