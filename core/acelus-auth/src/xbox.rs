use serde::Deserialize;
use serde_json::json;

use crate::secret::Secret;

pub const AUTHENTICATE_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
pub const AUTHORIZE_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
pub const MINECRAFT_RELYING_PARTY: &str = "rp://api.minecraftservices.com/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XboxToken {
    pub token: Secret,
    pub user_hash: String,
}

impl XboxToken {
    pub fn identity_token(&self) -> Secret {
        Secret::new(format!(
            "XBL3.0 x={};{}",
            self.user_hash,
            self.token.expose()
        ))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not reach Xbox Live")]
    Transport {
        #[source]
        source: reqwest::Error,
    },

    #[error("Xbox Live returned HTTP {status}")]
    Status { status: u16 },

    #[error("Xbox Live returned a response Acelus could not read")]
    Malformed {
        #[source]
        source: reqwest::Error,
    },

    #[error("Xbox Live returned no user hash, so the account cannot be identified")]
    MissingUserHash,

    #[error("this Microsoft account has no Xbox profile; sign in once at xbox.com to create one")]
    NoXboxAccount,

    #[error("this Xbox account is banned from Xbox Live")]
    Banned,

    #[error("Xbox Live is not available in this account's country or region")]
    RegionUnavailable,

    #[error("this Xbox account requires adult verification before it can be used")]
    AdultVerificationRequired,

    #[error("this is a child account and must be added to a family before it can sign in")]
    ChildAccount,

    #[error("Xbox Live refused the sign in with code {code}")]
    Refused { code: u64 },
}

impl Error {
    pub fn from_xerr(code: u64) -> Self {
        match code {
            2148916227 => Error::Banned,
            2148916233 => Error::NoXboxAccount,
            2148916235 => Error::RegionUnavailable,
            2148916236 | 2148916237 => Error::AdultVerificationRequired,
            2148916238 => Error::ChildAccount,
            other => Error::Refused { code: other },
        }
    }

    pub fn is_actionable_by_user(&self) -> bool {
        matches!(
            self,
            Error::NoXboxAccount
                | Error::ChildAccount
                | Error::AdultVerificationRequired
                | Error::RegionUnavailable
        )
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Deserialize)]
struct AuthResponse {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: DisplayClaims,
}

#[derive(Debug, Deserialize)]
struct DisplayClaims {
    #[serde(default)]
    xui: Vec<UserClaim>,
}

#[derive(Debug, Deserialize)]
struct UserClaim {
    #[serde(default)]
    uhs: Option<String>,
}

#[derive(Debug, Deserialize)]
struct XErrResponse {
    #[serde(rename = "XErr")]
    xerr: Option<u64>,
}

pub struct XboxClient {
    http: reqwest::Client,
    authenticate_url: String,
    authorize_url: String,
}

impl XboxClient {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            authenticate_url: AUTHENTICATE_URL.to_string(),
            authorize_url: AUTHORIZE_URL.to_string(),
        }
    }

    pub fn with_endpoints(
        http: reqwest::Client,
        authenticate_url: impl Into<String>,
        authorize_url: impl Into<String>,
    ) -> Self {
        Self {
            http,
            authenticate_url: authenticate_url.into(),
            authorize_url: authorize_url.into(),
        }
    }

    pub async fn authenticate(&self, microsoft_access_token: &Secret) -> Result<XboxToken> {
        let body = json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={}", microsoft_access_token.expose()),
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT",
        });

        self.post(&self.authenticate_url, body).await
    }

    pub async fn authorize(
        &self,
        xbox_token: &XboxToken,
        relying_party: &str,
    ) -> Result<XboxToken> {
        let body = json!({
            "Properties": {
                "SandboxId": "RETAIL",
                "UserTokens": [xbox_token.token.expose()],
            },
            "RelyingParty": relying_party,
            "TokenType": "JWT",
        });

        self.post(&self.authorize_url, body).await
    }

    pub async fn authorize_for_minecraft(&self, xbox_token: &XboxToken) -> Result<XboxToken> {
        self.authorize(xbox_token, MINECRAFT_RELYING_PARTY).await
    }

    async fn post(&self, url: &str, body: serde_json::Value) -> Result<XboxToken> {
        let response = self
            .http
            .post(url)
            .header(reqwest::header::ACCEPT, "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|source| Error::Transport { source })?;

        let status = response.status();
        if !status.is_success() {
            let payload = response.text().await.unwrap_or_default();
            if let Ok(XErrResponse { xerr: Some(code) }) =
                serde_json::from_str::<XErrResponse>(&payload)
            {
                return Err(Error::from_xerr(code));
            }
            return Err(Error::Status {
                status: status.as_u16(),
            });
        }

        let parsed: AuthResponse = response
            .json()
            .await
            .map_err(|source| Error::Malformed { source })?;

        let user_hash = parsed
            .display_claims
            .xui
            .into_iter()
            .find_map(|claim| claim.uhs)
            .ok_or(Error::MissingUserHash)?;

        Ok(XboxToken {
            token: Secret::new(parsed.token),
            user_hash,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documented_xerr_codes_map_to_explanations() {
        assert!(matches!(Error::from_xerr(2148916233), Error::NoXboxAccount));
        assert!(matches!(Error::from_xerr(2148916238), Error::ChildAccount));
        assert!(matches!(Error::from_xerr(2148916227), Error::Banned));
        assert!(matches!(
            Error::from_xerr(2148916235),
            Error::RegionUnavailable
        ));
        assert!(matches!(
            Error::from_xerr(2148916236),
            Error::AdultVerificationRequired
        ));
        assert!(matches!(
            Error::from_xerr(2148916237),
            Error::AdultVerificationRequired
        ));
    }

    #[test]
    fn an_unknown_code_is_surfaced_rather_than_swallowed() {
        match Error::from_xerr(2148916262) {
            Error::Refused { code } => assert_eq!(code, 2148916262),
            other => panic!("expected the raw code to survive, got {other:?}"),
        }
    }

    #[test]
    fn the_identity_token_uses_the_format_minecraft_expects() {
        let token = XboxToken {
            token: Secret::new("xsts-token-value"),
            user_hash: "1234567890".into(),
        };
        assert_eq!(
            token.identity_token().expose(),
            "XBL3.0 x=1234567890;xsts-token-value"
        );
    }

    #[test]
    fn errors_the_user_can_act_on_are_distinguished_from_ones_they_cannot() {
        assert!(Error::NoXboxAccount.is_actionable_by_user());
        assert!(Error::ChildAccount.is_actionable_by_user());
        assert!(!Error::Banned.is_actionable_by_user());
        assert!(!Error::Status { status: 500 }.is_actionable_by_user());
    }

    #[test]
    fn tokens_are_not_exposed_by_debug_formatting() {
        let token = XboxToken {
            token: Secret::new("xsts-token-value"),
            user_hash: "1234567890".into(),
        };
        let rendered = format!("{token:?}");
        assert!(!rendered.contains("xsts-token-value"));
        assert!(rendered.contains("1234567890"));
    }
}
