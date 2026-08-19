use acelus_auth::entitlement::{EntitlementItem, EntitlementsResponse, Error, Verifier};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::json;

mod common;

use common::{signing_key, verifying_key};

fn sign(claims: serde_json::Value) -> String {
    let key = EncodingKey::from_rsa_pem(signing_key()).expect("test signing key loads");
    jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, &key).expect("token is signed")
}

fn owned_response(signature: String) -> EntitlementsResponse {
    EntitlementsResponse {
        items: vec![
            EntitlementItem {
                name: "product_minecraft".into(),
                signature: None,
            },
            EntitlementItem {
                name: "game_minecraft".into(),
                signature: None,
            },
        ],
        signature: Some(signature),
        key_id: Some("1".into()),
    }
}

#[test]
fn a_correctly_signed_entitlement_verifies() {
    let verifier = Verifier::from_pem(verifying_key()).unwrap();
    let token = sign(json!({
        "entitlements": [{"name": "product_minecraft"}],
        "signerId": "2535416239104446"
    }));

    let verified = verifier.verify(&owned_response(token)).unwrap();

    assert!(verified.names.contains(&"product_minecraft".to_string()));
    assert_eq!(verified.claims["signerId"], "2535416239104446");
}

#[test]
fn a_token_signed_by_the_wrong_key_is_rejected() {
    let verifier = Verifier::mojang().unwrap();
    let token = sign(json!({"entitlements": [{"name": "product_minecraft"}]}));

    assert!(
        matches!(
            verifier.verify(&owned_response(token)),
            Err(Error::InvalidSignature { .. })
        ),
        "a token signed by a key that is not Mojang's must not verify"
    );
}

#[test]
fn tampering_with_the_payload_invalidates_the_signature() {
    let verifier = Verifier::from_pem(verifying_key()).unwrap();
    let token = sign(json!({"entitlements": [{"name": "product_dungeons"}]}));

    let mut parts: Vec<&str> = token.split('.').collect();
    let forged_payload = jsonwebtoken::encode(
        &Header::new(Algorithm::RS256),
        &json!({"entitlements": [{"name": "product_minecraft"}]}),
        &EncodingKey::from_rsa_pem(signing_key()).unwrap(),
    )
    .unwrap();
    let forged_payload_segment = forged_payload.split('.').nth(1).unwrap().to_string();
    parts[1] = &forged_payload_segment;
    let forged = parts.join(".");

    assert!(
        matches!(
            verifier.verify(&owned_response(forged)),
            Err(Error::InvalidSignature { .. })
        ),
        "swapping the payload while keeping the original signature must be detected"
    );
}

#[test]
fn an_unsigned_token_is_rejected_even_with_a_valid_payload() {
    let verifier = Verifier::from_pem(verifying_key()).unwrap();

    let header = base64_url(br#"{"alg":"none","typ":"JWT"}"#);
    let payload = base64_url(br#"{"entitlements":[{"name":"product_minecraft"}]}"#);
    let unsigned = format!("{header}.{payload}.");

    assert!(
        matches!(
            verifier.verify(&owned_response(unsigned)),
            Err(Error::InvalidSignature { .. })
        ),
        "the alg=none downgrade must not be accepted"
    );
}

#[test]
fn a_valid_signature_does_not_grant_ownership_the_account_lacks() {
    let verifier = Verifier::from_pem(verifying_key()).unwrap();
    let token = sign(json!({"entitlements": []}));

    let response = EntitlementsResponse {
        items: vec![EntitlementItem {
            name: "product_dungeons".into(),
            signature: None,
        }],
        signature: Some(token),
        key_id: Some("1".into()),
    };

    assert!(
        matches!(verifier.verify(&response), Err(Error::NotEntitled)),
        "a genuine signature over a response without Java Edition must still refuse"
    );
}

fn base64_url(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}
