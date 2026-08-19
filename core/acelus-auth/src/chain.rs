use std::time::Duration;

use crate::entitlement::{self, Verifier};
use crate::microsoft::{self, DeviceCode, MicrosoftClient, PollOutcome, Tokens};
use crate::minecraft::{self, MinecraftClient, Profile};
use crate::secret::Secret;
use crate::xbox::{self, XboxClient};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Microsoft(#[from] microsoft::Error),

    #[error(transparent)]
    Xbox(#[from] xbox::Error),

    #[error(transparent)]
    Minecraft(#[from] minecraft::Error),

    #[error(transparent)]
    Entitlement(#[from] entitlement::Error),

    #[error("the sign in did not complete before the code expired")]
    TimedOut,

    #[error("no stored credentials for this account; it must sign in again")]
    NoStoredCredentials,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub uuid: String,
    pub name: String,
    pub minecraft_token: Secret,
    pub refresh_token: Option<Secret>,
    pub expires_in: u64,
    pub entitlement_verified: bool,
    pub skin_url: Option<String>,
    pub cape_url: Option<String>,
}

impl Account {
    pub fn from_parts(profile: Profile, session: minecraft::Session, tokens: &Tokens) -> Self {
        Self {
            uuid: profile.dashed_uuid(),
            name: profile.name.clone(),
            minecraft_token: session.access_token,
            refresh_token: tokens.refresh_token.clone(),
            expires_in: session.expires_in,
            entitlement_verified: true,
            skin_url: profile.active_skin().map(|t| t.url.clone()),
            cape_url: profile.active_cape().map(|t| t.url.clone()),
        }
    }
}

pub struct Authenticator {
    microsoft: MicrosoftClient,
    xbox: XboxClient,
    minecraft: MinecraftClient,
    verifier: Verifier,
}

impl Authenticator {
    pub fn new(
        microsoft: MicrosoftClient,
        xbox: XboxClient,
        minecraft: MinecraftClient,
        verifier: Verifier,
    ) -> Self {
        Self {
            microsoft,
            xbox,
            minecraft,
            verifier,
        }
    }

    pub fn microsoft(&self) -> &MicrosoftClient {
        &self.microsoft
    }

    pub async fn begin_device_code(&self) -> Result<DeviceCode> {
        Ok(self.microsoft.begin_device_code().await?)
    }

    pub async fn wait_for_device_code(&self, code: &DeviceCode) -> Result<Tokens> {
        let deadline = std::time::Instant::now() + code.expires_in();
        let mut interval = code.poll_interval();

        loop {
            if std::time::Instant::now() >= deadline {
                return Err(Error::TimedOut);
            }

            tokio::time::sleep(interval).await;

            match self.microsoft.poll_device_code(&code.device_code).await? {
                Ok(tokens) => return Ok(tokens),
                Err(PollOutcome::Pending) => {}
                Err(PollOutcome::SlowDown) => interval += Duration::from_secs(5),
            }
        }
    }

    pub async fn complete(&self, tokens: Tokens) -> Result<Account> {
        let xbox_token = self.xbox.authenticate(&tokens.access_token).await?;
        let xsts_token = self.xbox.authorize_for_minecraft(&xbox_token).await?;

        let session = self
            .minecraft
            .login_with_xbox(&xsts_token.identity_token())
            .await?;

        let entitlements = self.minecraft.entitlements(&session.access_token).await?;
        self.verifier.verify(&entitlements)?;

        let profile = self.minecraft.profile(&session.access_token).await?;

        Ok(Account::from_parts(profile, session, &tokens))
    }

    pub async fn login_with_device_code(&self, code: &DeviceCode) -> Result<Account> {
        let tokens = self.wait_for_device_code(code).await?;
        self.complete(tokens).await
    }

    pub async fn refresh(&self, refresh_token: &Secret) -> Result<Account> {
        let tokens = self.microsoft.refresh(refresh_token).await?;
        self.complete(tokens).await
    }
}

impl std::fmt::Debug for Authenticator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Authenticator").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_credentials_are_not_exposed_by_debug_formatting() {
        let account = Account {
            uuid: "986dec87-b7ec-47ff-89ff-033fdb95c4b5".into(),
            name: "HowDoesAuthWork".into(),
            minecraft_token: Secret::new("minecraft-access-token"),
            refresh_token: Some(Secret::new("microsoft-refresh-token")),
            expires_in: 86400,
            entitlement_verified: true,
            skin_url: None,
            cape_url: None,
        };

        let rendered = format!("{account:?}");
        assert!(!rendered.contains("minecraft-access-token"));
        assert!(!rendered.contains("microsoft-refresh-token"));
        assert!(rendered.contains("HowDoesAuthWork"));
    }
}
