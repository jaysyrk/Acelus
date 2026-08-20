use std::time::Duration;

use serde::Deserialize;

use crate::secret::Secret;

pub const DEVICE_CODE_URL: &str =
    "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
pub const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
pub const SCOPE: &str = "XboxLive.signin offline_access";
pub const DEVICE_CODE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not reach the Microsoft identity service")]
    Transport {
        #[source]
        source: reqwest::Error,
    },

    #[error("the Microsoft identity service returned a response Acelus could not read")]
    Malformed {
        #[source]
        source: reqwest::Error,
    },

    #[error("the Microsoft identity service returned HTTP {status}: {error}")]
    Rejected { status: u16, error: String },

    #[error("the sign in code expired before it was used")]
    CodeExpired,

    #[error("the sign in was declined")]
    Declined,

    #[error(
        "Mojang has not approved this Azure application yet; \
         request approval at https://aka.ms/mce-reviewappid"
    )]
    ApplicationNotApproved,

    #[error("the stored credentials are no longer valid and the account must sign in again")]
    RefreshRejected,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    #[serde(default = "default_interval")]
    pub interval: u64,
    #[serde(default)]
    pub message: Option<String>,
}

fn default_interval() -> u64 {
    5
}

impl DeviceCode {
    pub fn poll_interval(&self) -> Duration {
        Duration::from_secs(self.interval.max(1))
    }

    pub fn expires_in(&self) -> Duration {
        Duration::from_secs(self.expires_in)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tokens {
    pub access_token: Secret,
    pub refresh_token: Option<Secret>,
    pub expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollOutcome {
    Pending,
    SlowDown,
}

pub struct MicrosoftClient {
    http: reqwest::Client,
    client_id: String,
    device_code_url: String,
    token_url: String,
}

impl MicrosoftClient {
    pub fn new(http: reqwest::Client, client_id: impl Into<String>) -> Self {
        Self {
            http,
            client_id: client_id.into(),
            device_code_url: DEVICE_CODE_URL.to_string(),
            token_url: TOKEN_URL.to_string(),
        }
    }

    pub fn with_endpoints(
        http: reqwest::Client,
        client_id: impl Into<String>,
        device_code_url: impl Into<String>,
        token_url: impl Into<String>,
    ) -> Self {
        Self {
            http,
            client_id: client_id.into(),
            device_code_url: device_code_url.into(),
            token_url: token_url.into(),
        }
    }

    pub async fn begin_device_code(&self) -> Result<DeviceCode> {
        let response = self
            .http
            .post(&self.device_code_url)
            .form(&[("client_id", self.client_id.as_str()), ("scope", SCOPE)])
            .send()
            .await
            .map_err(|source| Error::Transport { source })?;

        let status = response.status();
        if !status.is_success() {
            return Err(self.classify(status.as_u16(), &response.text().await.unwrap_or_default()));
        }

        response
            .json()
            .await
            .map_err(|source| Error::Malformed { source })
    }

    pub async fn poll_device_code(
        &self,
        device_code: &str,
    ) -> Result<std::result::Result<Tokens, PollOutcome>> {
        let response = self
            .http
            .post(&self.token_url)
            .form(&[
                ("grant_type", DEVICE_CODE_GRANT),
                ("client_id", self.client_id.as_str()),
                ("device_code", device_code),
            ])
            .send()
            .await
            .map_err(|source| Error::Transport { source })?;

        let status = response.status();
        if status.is_success() {
            return Ok(Ok(self.read_tokens(response).await?));
        }

        let body = response.text().await.unwrap_or_default();
        match serde_json::from_str::<ErrorResponse>(&body) {
            Ok(error) => match error.error.as_str() {
                "authorization_pending" => Ok(Err(PollOutcome::Pending)),
                "slow_down" => Ok(Err(PollOutcome::SlowDown)),
                "expired_token" | "code_expired" => Err(Error::CodeExpired),
                "authorization_declined" | "access_denied" => Err(Error::Declined),
                _ => Err(Error::Rejected {
                    status: status.as_u16(),
                    error: error.error_description.unwrap_or(error.error),
                }),
            },
            Err(_) => Err(self.classify(status.as_u16(), &body)),
        }
    }

    pub async fn refresh(&self, refresh_token: &Secret) -> Result<Tokens> {
        let response = self
            .http
            .post(&self.token_url)
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", self.client_id.as_str()),
                ("refresh_token", refresh_token.expose()),
                ("scope", SCOPE),
            ])
            .send()
            .await
            .map_err(|source| Error::Transport { source })?;

        let status = response.status();
        if status.is_success() {
            return self.read_tokens(response).await;
        }

        if status.as_u16() == 400 {
            return Err(Error::RefreshRejected);
        }

        Err(self.classify(status.as_u16(), &response.text().await.unwrap_or_default()))
    }

    async fn read_tokens(&self, response: reqwest::Response) -> Result<Tokens> {
        let parsed: TokenResponse = response
            .json()
            .await
            .map_err(|source| Error::Malformed { source })?;

        Ok(Tokens {
            access_token: Secret::new(parsed.access_token),
            refresh_token: parsed.refresh_token.map(Secret::new),
            expires_in: parsed.expires_in,
        })
    }

    fn classify(&self, status: u16, body: &str) -> Error {
        if status == 403 {
            return Error::ApplicationNotApproved;
        }

        match serde_json::from_str::<ErrorResponse>(body) {
            Ok(error) => Error::Rejected {
                status,
                error: error.error_description.unwrap_or(error.error),
            },
            Err(_) => Error::Rejected {
                status,
                error: body.chars().take(200).collect(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_code_without_an_interval_falls_back_to_five_seconds() {
        let code: DeviceCode = serde_json::from_str(
            r#"{"device_code":"d","user_code":"ABCD-EFGH","verification_uri":"https://microsoft.com/link","expires_in":900}"#,
        )
        .unwrap();
        assert_eq!(code.poll_interval(), Duration::from_secs(5));
        assert_eq!(code.expires_in(), Duration::from_secs(900));
    }

    #[test]
    fn a_zero_interval_is_raised_to_avoid_hammering_the_endpoint() {
        let code: DeviceCode = serde_json::from_str(
            r#"{"device_code":"d","user_code":"u","verification_uri":"https://e","expires_in":900,"interval":0}"#,
        )
        .unwrap();
        assert_eq!(code.poll_interval(), Duration::from_secs(1));
    }

    #[test]
    fn tokens_are_not_exposed_by_debug_formatting() {
        let tokens = Tokens {
            access_token: Secret::new("access-token-material"),
            refresh_token: Some(Secret::new("refresh-token-material")),
            expires_in: 3600,
        };
        let rendered = format!("{tokens:?}");
        assert!(!rendered.contains("access-token-material"));
        assert!(!rendered.contains("refresh-token-material"));
        assert!(rendered.contains("3600"));
    }
}
