use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

pub const MOJANG_ENTITLEMENT_KEY: &[u8] = include_bytes!("mojang_entitlement_key.pem");

pub const JAVA_EDITION_ENTITLEMENTS: &[&str] = &[
    "product_minecraft",
    "game_minecraft",
    "product_game_pass_pc",
    "product_game_pass_ultimate",
];

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the entitlement signing key could not be parsed")]
    Key {
        #[source]
        source: jsonwebtoken::errors::Error,
    },

    #[error("the entitlement response carried no signature to verify")]
    MissingSignature,

    #[error(
        "the entitlement signature is not valid for Mojang's signing key; \
         the response was altered in transit, or Mojang has rotated the key"
    )]
    InvalidSignature {
        #[source]
        source: jsonwebtoken::errors::Error,
    },

    #[error("this account holds no Minecraft: Java Edition entitlement")]
    NotEntitled,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitlementItem {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementsResponse {
    #[serde(default)]
    pub items: Vec<EntitlementItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
}

impl EntitlementsResponse {
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.items.iter().map(|item| item.name.as_str())
    }

    pub fn claims_java_edition(&self) -> bool {
        self.names()
            .any(|name| JAVA_EDITION_ENTITLEMENTS.contains(&name))
    }
}

#[derive(Debug, Clone)]
pub struct VerifiedEntitlements {
    pub names: Vec<String>,
    pub claims: serde_json::Value,
}

pub struct Verifier {
    key: DecodingKey,
    validation: Validation,
}

impl Verifier {
    pub fn mojang() -> Result<Self> {
        Self::from_pem(MOJANG_ENTITLEMENT_KEY)
    }

    pub fn from_pem(pem: &[u8]) -> Result<Self> {
        let key = DecodingKey::from_rsa_pem(pem).map_err(|source| Error::Key { source })?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.required_spec_claims.clear();
        validation.validate_exp = false;
        validation.validate_nbf = false;
        validation.validate_aud = false;

        Ok(Self { key, validation })
    }

    pub fn decode(&self, token: &str) -> Result<serde_json::Value> {
        jsonwebtoken::decode::<serde_json::Value>(token, &self.key, &self.validation)
            .map(|data| data.claims)
            .map_err(|source| Error::InvalidSignature { source })
    }

    pub fn verify(&self, response: &EntitlementsResponse) -> Result<VerifiedEntitlements> {
        let signature = response
            .signature
            .as_deref()
            .ok_or(Error::MissingSignature)?;

        let claims = self.decode(signature)?;

        if !response.claims_java_edition() {
            return Err(Error::NotEntitled);
        }

        Ok(VerifiedEntitlements {
            names: response.names().map(str::to_owned).collect(),
            claims,
        })
    }
}

impl std::fmt::Debug for Verifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Verifier").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mojangs_signing_key_is_a_usable_rsa_key() {
        Verifier::mojang().expect("the embedded key parses");
    }

    #[test]
    fn a_response_listing_the_java_edition_product_claims_ownership() {
        let response: EntitlementsResponse = serde_json::from_str(
            r#"{"items":[{"name":"product_minecraft"},{"name":"game_minecraft"}],"signature":"x","keyId":"1"}"#,
        )
        .unwrap();
        assert!(response.claims_java_edition());
    }

    #[test]
    fn a_game_pass_subscription_claims_ownership() {
        let response: EntitlementsResponse =
            serde_json::from_str(r#"{"items":[{"name":"product_game_pass_ultimate"}]}"#).unwrap();
        assert!(response.claims_java_edition());
    }

    #[test]
    fn an_empty_response_claims_nothing() {
        let response: EntitlementsResponse =
            serde_json::from_str(r#"{"items":[],"signature":"x","keyId":"1"}"#).unwrap();
        assert!(!response.claims_java_edition());
    }

    #[test]
    fn entitlements_for_other_games_do_not_grant_java_edition() {
        let response: EntitlementsResponse = serde_json::from_str(
            r#"{"items":[{"name":"product_dungeons"},{"name":"game_legends"},{"name":"product_minecraft_bedrock"}]}"#,
        )
        .unwrap();
        assert!(!response.claims_java_edition());
    }

    #[test]
    fn verification_fails_when_no_signature_is_present() {
        let verifier = Verifier::mojang().unwrap();
        let response = EntitlementsResponse {
            items: vec![EntitlementItem {
                name: "product_minecraft".into(),
                signature: None,
            }],
            signature: None,
            key_id: None,
        };
        assert!(matches!(
            verifier.verify(&response),
            Err(Error::MissingSignature)
        ));
    }

    #[test]
    fn a_signature_not_made_by_mojang_is_rejected() {
        let verifier = Verifier::mojang().unwrap();
        let response = EntitlementsResponse {
            items: vec![EntitlementItem {
                name: "product_minecraft".into(),
                signature: None,
            }],
            signature: Some("not.a.jwt".into()),
            key_id: Some("1".into()),
        };
        assert!(matches!(
            verifier.verify(&response),
            Err(Error::InvalidSignature { .. })
        ));
    }
}
