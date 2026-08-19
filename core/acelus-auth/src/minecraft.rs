use serde::Deserialize;
use serde_json::json;

use crate::entitlement::EntitlementsResponse;
use crate::secret::Secret;

pub const LOGIN_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
pub const ENTITLEMENTS_URL: &str = "https://api.minecraftservices.com/entitlements/mcstore";
pub const PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not reach the Minecraft services API")]
    Transport {
        #[source]
        source: reqwest::Error,
    },

    #[error("the Minecraft services API returned a response Acelus could not read")]
    Malformed {
        #[source]
        source: reqwest::Error,
    },

    #[error(
        "the Minecraft services API refused this application; \
         the Azure app is most likely still awaiting Mojang's approval, see docs/AZURE_SETUP.md"
    )]
    ApplicationNotApproved,

    #[error("the Minecraft session has expired and must be renewed")]
    SessionExpired,

    #[error(
        "this account has no Minecraft: Java Edition profile; \
         Game Pass accounts must sign in to the official launcher once to create one"
    )]
    NoProfile,

    #[error("the Minecraft services API returned HTTP {status}")]
    Status { status: u16 },
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub username: String,
    pub access_token: Secret,
    pub expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct LoginResponse {
    username: String,
    access_token: String,
    #[serde(default)]
    expires_in: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub skins: Vec<Texture>,
    #[serde(default)]
    pub capes: Vec<Texture>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Texture {
    pub id: String,
    pub state: String,
    pub url: String,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub alias: Option<String>,
}

impl Profile {
    pub fn active_skin(&self) -> Option<&Texture> {
        self.skins.iter().find(|t| t.state == "ACTIVE")
    }

    pub fn active_cape(&self) -> Option<&Texture> {
        self.capes.iter().find(|t| t.state == "ACTIVE")
    }

    pub fn dashed_uuid(&self) -> String {
        if self.id.len() != 32 || self.id.contains('-') {
            return self.id.clone();
        }
        format!(
            "{}-{}-{}-{}-{}",
            &self.id[0..8],
            &self.id[8..12],
            &self.id[12..16],
            &self.id[16..20],
            &self.id[20..32]
        )
    }
}

pub struct MinecraftClient {
    http: reqwest::Client,
    login_url: String,
    entitlements_url: String,
    profile_url: String,
}

impl MinecraftClient {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            login_url: LOGIN_URL.to_string(),
            entitlements_url: ENTITLEMENTS_URL.to_string(),
            profile_url: PROFILE_URL.to_string(),
        }
    }

    pub fn with_base_url(http: reqwest::Client, base: &str) -> Self {
        let base = base.trim_end_matches('/');
        Self {
            http,
            login_url: format!("{base}/authentication/login_with_xbox"),
            entitlements_url: format!("{base}/entitlements/mcstore"),
            profile_url: format!("{base}/minecraft/profile"),
        }
    }

    pub async fn login_with_xbox(&self, identity_token: &Secret) -> Result<Session> {
        let response = self
            .http
            .post(&self.login_url)
            .header(reqwest::header::ACCEPT, "application/json")
            .json(&json!({ "identityToken": identity_token.expose() }))
            .send()
            .await
            .map_err(|source| Error::Transport { source })?;

        let response = self.check(response)?;
        let parsed: LoginResponse = response
            .json()
            .await
            .map_err(|source| Error::Malformed { source })?;

        Ok(Session {
            username: parsed.username,
            access_token: Secret::new(parsed.access_token),
            expires_in: parsed.expires_in,
        })
    }

    pub async fn entitlements(&self, access_token: &Secret) -> Result<EntitlementsResponse> {
        let response = self
            .authorized_get(&self.entitlements_url, access_token)
            .await?;

        response
            .json()
            .await
            .map_err(|source| Error::Malformed { source })
    }

    pub async fn profile(&self, access_token: &Secret) -> Result<Profile> {
        let response = self
            .http
            .get(&self.profile_url)
            .bearer_auth(access_token.expose())
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|source| Error::Transport { source })?;

        if response.status().as_u16() == 404 {
            return Err(Error::NoProfile);
        }

        let response = self.check(response)?;
        response
            .json()
            .await
            .map_err(|source| Error::Malformed { source })
    }

    async fn authorized_get(&self, url: &str, access_token: &Secret) -> Result<reqwest::Response> {
        let response = self
            .http
            .get(url)
            .bearer_auth(access_token.expose())
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|source| Error::Transport { source })?;

        self.check(response)
    }

    fn check(&self, response: reqwest::Response) -> Result<reqwest::Response> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }

        Err(match status.as_u16() {
            401 => Error::SessionExpired,
            403 => Error::ApplicationNotApproved,
            other => Error::Status { status: other },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(id: &str) -> Profile {
        Profile {
            id: id.to_string(),
            name: "HowDoesAuthWork".into(),
            skins: Vec::new(),
            capes: Vec::new(),
        }
    }

    #[test]
    fn a_compact_uuid_gains_its_dashes() {
        assert_eq!(
            profile("986dec87b7ec47ff89ff033fdb95c4b5").dashed_uuid(),
            "986dec87-b7ec-47ff-89ff-033fdb95c4b5"
        );
    }

    #[test]
    fn an_already_dashed_uuid_is_left_alone() {
        let dashed = "986dec87-b7ec-47ff-89ff-033fdb95c4b5";
        assert_eq!(profile(dashed).dashed_uuid(), dashed);
    }

    #[test]
    fn the_active_texture_is_picked_out_of_several() {
        let profile: Profile = serde_json::from_str(
            r#"{"id":"986dec87b7ec47ff89ff033fdb95c4b5","name":"n","skins":[
                {"id":"a","state":"INACTIVE","url":"https://old","variant":"CLASSIC"},
                {"id":"b","state":"ACTIVE","url":"https://current","variant":"SLIM"}
            ],"capes":[{"id":"c","state":"ACTIVE","url":"https://cape","alias":"Migrator"}]}"#,
        )
        .unwrap();

        assert_eq!(profile.active_skin().unwrap().url, "https://current");
        assert_eq!(
            profile.active_skin().unwrap().variant.as_deref(),
            Some("SLIM")
        );
        assert_eq!(
            profile.active_cape().unwrap().alias.as_deref(),
            Some("Migrator")
        );
    }

    #[test]
    fn a_profile_without_textures_reports_none() {
        let profile = profile("986dec87b7ec47ff89ff033fdb95c4b5");
        assert!(profile.active_skin().is_none());
        assert!(profile.active_cape().is_none());
    }

    #[test]
    fn session_tokens_are_not_exposed_by_debug_formatting() {
        let session = Session {
            username: "986dec87-b7ec-47ff-89ff-033fdb95c4b5".into(),
            access_token: Secret::new("minecraft-access-token"),
            expires_in: 86400,
        };
        let rendered = format!("{session:?}");
        assert!(!rendered.contains("minecraft-access-token"));
        assert!(rendered.contains("86400"));
    }
}
