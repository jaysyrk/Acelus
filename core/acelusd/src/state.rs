use std::collections::HashMap;
use std::sync::Arc;

use acelus_auth::store::{now_seconds, AccountStore, StoredAccount};
use acelus_auth::xbox::XboxClient;
use acelus_auth::{Account, Authenticator, MicrosoftClient, MinecraftClient, Secret, Verifier};
use acelus_cas::Store;
use acelus_instance::descriptor::Instances;
use acelus_instance::java::JavaProvisioner;
use acelus_instance::{Installer, Paths, Resolver};
use acelus_launch::Session;
use acelus_meta::Environment;
use acelus_net::Fetcher;
use tokio::sync::Mutex;

pub const DEFAULT_CLIENT_ID_ENV: &str = "ACELUS_CLIENT_ID";

pub struct Daemon {
    pub paths: Paths,
    pub environment: Environment,
    pub resolver: Resolver,
    pub installer: Installer,
    pub java: JavaProvisioner,
    pub instances: Instances,
    pub accounts: AccountStore,
    pub authenticator: Option<Authenticator>,
    pub sessions: Mutex<HashMap<String, Session>>,
    pub pending_logins: Mutex<HashMap<String, acelus_auth::DeviceCode>>,
}

impl Daemon {
    pub fn open(paths: Paths) -> std::io::Result<Arc<Self>> {
        let fetcher = Fetcher::new().map_err(std::io::Error::other)?;
        let store = Store::open(paths.store()).map_err(std::io::Error::other)?;

        let authenticator = client_id().map(|client_id| {
            let http = fetcher.client().clone();
            Authenticator::new(
                MicrosoftClient::new(http.clone(), client_id),
                XboxClient::new(http.clone()),
                MinecraftClient::new(http),
                Verifier::mojang().expect("the embedded signing key parses"),
            )
        });

        Ok(Arc::new(Self {
            environment: Environment::current(),
            resolver: Resolver::new(fetcher.clone()),
            installer: Installer::new(fetcher.clone(), store, paths.clone()),
            java: JavaProvisioner::new(fetcher.clone(), paths.clone()),
            instances: Instances::new(paths.clone()),
            accounts: AccountStore::with_keyring(paths.cache().join("accounts.json")),
            authenticator,
            sessions: Mutex::new(HashMap::new()),
            pending_logins: Mutex::new(HashMap::new()),
            paths,
        }))
    }

    pub async fn usable_account(&self, uuid: Option<&str>) -> Result<Account, AccountProblem> {
        let registry = self
            .accounts
            .load()
            .map_err(|_| AccountProblem::Unreadable)?;

        let stored = match uuid {
            Some(uuid) => registry.find(uuid).cloned(),
            None => registry.active_account().cloned(),
        }
        .ok_or(AccountProblem::NoneSelected)?;

        let now = now_seconds();

        if stored.needs_refresh(now) {
            return self.renew(&stored).await;
        }

        let token = self
            .accounts
            .session_token(&stored.uuid)
            .ok()
            .flatten()
            .ok_or(AccountProblem::Expired)?;

        Ok(Account {
            uuid: stored.uuid,
            name: stored.name,
            minecraft_token: token,
            refresh_token: None,
            expires_in: stored.expires_at.saturating_sub(now),
            xuid: stored.xuid,
            entitlement_verified: stored.entitlement_verified,
            skin_url: stored.skin_url,
            cape_url: stored.cape_url,
        })
    }

    async fn renew(&self, stored: &StoredAccount) -> Result<Account, AccountProblem> {
        let authenticator = self
            .authenticator
            .as_ref()
            .ok_or(AccountProblem::NoClientId)?;

        let refresh: Secret = self
            .accounts
            .refresh_token(&stored.uuid)
            .ok()
            .flatten()
            .ok_or(AccountProblem::Expired)?;

        let account = authenticator
            .refresh(&refresh)
            .await
            .map_err(|error| AccountProblem::Refresh(error.to_string()))?;

        let _ = self.accounts.remember(&account, now_seconds());
        Ok(account)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountProblem {
    NoneSelected,
    Expired,
    Unreadable,
    NoClientId,
    Refresh(String),
}

impl AccountProblem {
    pub fn message(&self) -> String {
        match self {
            AccountProblem::NoneSelected => {
                "no account is signed in; run acelus login first".into()
            }
            AccountProblem::Expired => {
                "the stored session has expired and could not be renewed; sign in again".into()
            }
            AccountProblem::Unreadable => "the account registry could not be read".into(),
            AccountProblem::NoClientId => format!(
                "no Azure client id is configured; set {DEFAULT_CLIENT_ID_ENV} or see docs/AZURE_SETUP.md"
            ),
            AccountProblem::Refresh(detail) => {
                format!("the stored session could not be renewed: {detail}")
            }
        }
    }
}

pub fn client_id() -> Option<String> {
    std::env::var(DEFAULT_CLIENT_ID_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_account_problem_explains_itself_and_names_the_remedy() {
        assert!(AccountProblem::NoneSelected
            .message()
            .contains("acelus login"));
        assert!(AccountProblem::Expired.message().contains("sign in again"));
        assert!(AccountProblem::NoClientId
            .message()
            .contains("AZURE_SETUP.md"));
        assert!(AccountProblem::Refresh("upstream said no".into())
            .message()
            .contains("upstream said no"));
    }

    #[test]
    fn a_blank_client_id_counts_as_unset() {
        assert_eq!(
            std::env::var(DEFAULT_CLIENT_ID_ENV)
                .ok()
                .filter(|v| !v.trim().is_empty()),
            client_id()
        );
    }
}
