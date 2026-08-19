use acelus_auth::chain::Error;
use acelus_auth::entitlement::Verifier;
use acelus_auth::microsoft::MicrosoftClient;
use acelus_auth::minecraft::MinecraftClient;
use acelus_auth::xbox::XboxClient;
use acelus_auth::{Authenticator, Secret};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::json;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod common;

use common::{signing_key, verifying_key};

const CLIENT_ID: &str = "00000000-0000-0000-0000-000000000000";
const UUID_COMPACT: &str = "986dec87b7ec47ff89ff033fdb95c4b5";
const UUID_DASHED: &str = "986dec87-b7ec-47ff-89ff-033fdb95c4b5";

fn entitlement_signature() -> String {
    let key = EncodingKey::from_rsa_pem(signing_key()).unwrap();
    jsonwebtoken::encode(
        &Header::new(Algorithm::RS256),
        &json!({"entitlements": [{"name": "product_minecraft"}]}),
        &key,
    )
    .unwrap()
}

fn authenticator(server: &MockServer) -> Authenticator {
    let http = reqwest::Client::new();
    let base = server.uri();

    Authenticator::new(
        MicrosoftClient::with_endpoints(
            http.clone(),
            CLIENT_ID,
            format!("{base}/oauth2/devicecode"),
            format!("{base}/oauth2/token"),
        ),
        XboxClient::with_endpoints(
            http.clone(),
            format!("{base}/user/authenticate"),
            format!("{base}/xsts/authorize"),
        ),
        MinecraftClient::with_base_url(http, &base),
        Verifier::from_pem(verifying_key()).unwrap(),
    )
}

async fn mock_xbox(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/user/authenticate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "Token": "xbl-token",
            "DisplayClaims": {"xui": [{"uhs": "1234567890"}]}
        })))
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/xsts/authorize"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "Token": "xsts-token",
            "DisplayClaims": {"xui": [{"uhs": "1234567890"}]}
        })))
        .mount(server)
        .await;
}

async fn mock_minecraft(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/authentication/login_with_xbox"))
        .and(body_string_contains("XBL3.0 x=1234567890;xsts-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "username": UUID_DASHED,
            "roles": [],
            "access_token": "minecraft-access-token",
            "token_type": "Bearer",
            "expires_in": 86400
        })))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/entitlements/mcstore"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [
                {"name": "product_minecraft", "signature": "item-jwt"},
                {"name": "game_minecraft", "signature": "item-jwt"}
            ],
            "signature": entitlement_signature(),
            "keyId": "1"
        })))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/minecraft/profile"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": UUID_COMPACT,
            "name": "HowDoesAuthWork",
            "skins": [{
                "id": "6a6e65e5-76dd-4c3c-a625-162924514568",
                "state": "ACTIVE",
                "url": "https://textures.minecraft.net/texture/skin",
                "variant": "CLASSIC"
            }],
            "capes": []
        })))
        .mount(server)
        .await;
}

fn tokens() -> acelus_auth::Tokens {
    acelus_auth::Tokens {
        access_token: Secret::new("microsoft-access-token"),
        refresh_token: Some(Secret::new("microsoft-refresh-token")),
        expires_in: 3600,
    }
}

#[tokio::test]
async fn the_whole_chain_produces_a_verified_account() {
    let server = MockServer::start().await;
    mock_xbox(&server).await;
    mock_minecraft(&server).await;

    let account = authenticator(&server).complete(tokens()).await.unwrap();

    assert_eq!(account.uuid, UUID_DASHED);
    assert_eq!(account.name, "HowDoesAuthWork");
    assert!(account.entitlement_verified);
    assert_eq!(account.minecraft_token.expose(), "minecraft-access-token");
    assert_eq!(
        account.skin_url.as_deref(),
        Some("https://textures.minecraft.net/texture/skin")
    );
    assert_eq!(
        account.refresh_token.as_ref().unwrap().expose(),
        "microsoft-refresh-token"
    );
}

#[tokio::test]
async fn an_account_without_an_xbox_profile_is_explained_rather_than_dumped() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/user/authenticate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "Token": "xbl-token",
            "DisplayClaims": {"xui": [{"uhs": "1234567890"}]}
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/xsts/authorize"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "Identity": "0",
            "XErr": 2148916233u64,
            "Message": "",
            "Redirect": "https://start.ui.xboxlive.com/CreateAccount"
        })))
        .mount(&server)
        .await;

    let error = authenticator(&server).complete(tokens()).await.unwrap_err();

    assert!(
        matches!(error, Error::Xbox(acelus_auth::xbox::Error::NoXboxAccount)),
        "expected a no-Xbox-account explanation, got {error:?}"
    );
}

#[tokio::test]
async fn a_child_account_is_told_it_needs_adding_to_a_family() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/user/authenticate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "Token": "xbl-token",
            "DisplayClaims": {"xui": [{"uhs": "1234567890"}]}
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/xsts/authorize"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({"XErr": 2148916238u64})))
        .mount(&server)
        .await;

    let error = authenticator(&server).complete(tokens()).await.unwrap_err();
    assert!(matches!(
        error,
        Error::Xbox(acelus_auth::xbox::Error::ChildAccount)
    ));
}

#[tokio::test]
async fn an_unapproved_azure_application_is_named_as_the_cause() {
    let server = MockServer::start().await;
    mock_xbox(&server).await;

    Mock::given(method("POST"))
        .and(path("/authentication/login_with_xbox"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;

    let error = authenticator(&server).complete(tokens()).await.unwrap_err();

    assert!(
        matches!(
            error,
            Error::Minecraft(acelus_auth::minecraft::Error::ApplicationNotApproved)
        ),
        "a 403 during login must point at the Azure approval gate, got {error:?}"
    );
}

#[tokio::test]
async fn an_account_without_the_game_is_refused_before_a_profile_is_fetched() {
    let server = MockServer::start().await;
    mock_xbox(&server).await;

    Mock::given(method("POST"))
        .and(path("/authentication/login_with_xbox"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "username": UUID_DASHED,
            "access_token": "minecraft-access-token",
            "expires_in": 86400
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/entitlements/mcstore"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [],
            "signature": entitlement_signature(),
            "keyId": "1"
        })))
        .mount(&server)
        .await;

    let error = authenticator(&server).complete(tokens()).await.unwrap_err();

    assert!(
        matches!(
            error,
            Error::Entitlement(acelus_auth::entitlement::Error::NotEntitled)
        ),
        "expected the entitlement gate to refuse, got {error:?}"
    );
}

#[tokio::test]
async fn a_forged_entitlement_signature_is_refused() {
    let server = MockServer::start().await;
    mock_xbox(&server).await;

    Mock::given(method("POST"))
        .and(path("/authentication/login_with_xbox"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "username": UUID_DASHED,
            "access_token": "minecraft-access-token",
            "expires_in": 86400
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/entitlements/mcstore"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{"name": "product_minecraft"}],
            "signature": "eyJhbGciOiJub25lIn0.eyJlbnRpdGxlbWVudHMiOltdfQ.",
            "keyId": "1"
        })))
        .mount(&server)
        .await;

    let error = authenticator(&server).complete(tokens()).await.unwrap_err();

    assert!(
        matches!(
            error,
            Error::Entitlement(acelus_auth::entitlement::Error::InvalidSignature { .. })
        ),
        "a response claiming ownership without a genuine signature must be refused, got {error:?}"
    );
}

#[tokio::test]
async fn a_game_pass_account_without_a_profile_gets_a_specific_explanation() {
    let server = MockServer::start().await;
    mock_xbox(&server).await;

    Mock::given(method("POST"))
        .and(path("/authentication/login_with_xbox"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "username": UUID_DASHED,
            "access_token": "minecraft-access-token",
            "expires_in": 86400
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/entitlements/mcstore"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{"name": "product_game_pass_ultimate"}],
            "signature": entitlement_signature(),
            "keyId": "1"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/minecraft/profile"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "path": "/minecraft/profile",
            "error": "NOT_FOUND"
        })))
        .mount(&server)
        .await;

    let error = authenticator(&server).complete(tokens()).await.unwrap_err();

    assert!(
        matches!(
            error,
            Error::Minecraft(acelus_auth::minecraft::Error::NoProfile)
        ),
        "expected the Game Pass profile explanation, got {error:?}"
    );
}

#[tokio::test]
async fn an_expired_session_is_reported_as_expired_not_as_a_bare_status() {
    let server = MockServer::start().await;
    mock_xbox(&server).await;

    Mock::given(method("POST"))
        .and(path("/authentication/login_with_xbox"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "username": UUID_DASHED,
            "access_token": "minecraft-access-token",
            "expires_in": 86400
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/entitlements/mcstore"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let error = authenticator(&server).complete(tokens()).await.unwrap_err();
    assert!(matches!(
        error,
        Error::Minecraft(acelus_auth::minecraft::Error::SessionExpired)
    ));
}

#[tokio::test]
async fn a_device_code_is_requested_and_polled_until_the_user_approves() {
    let server = MockServer::start().await;
    mock_xbox(&server).await;
    mock_minecraft(&server).await;

    Mock::given(method("POST"))
        .and(path("/oauth2/devicecode"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "device_code": "device-code-value",
            "user_code": "ABCD-EFGH",
            "verification_uri": "https://www.microsoft.com/link",
            "expires_in": 900,
            "interval": 1
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/oauth2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "microsoft-access-token",
            "refresh_token": "microsoft-refresh-token",
            "expires_in": 3600,
            "token_type": "Bearer"
        })))
        .mount(&server)
        .await;

    let auth = authenticator(&server);
    let code = auth.begin_device_code().await.unwrap();

    assert_eq!(code.user_code, "ABCD-EFGH");
    assert_eq!(code.verification_uri, "https://www.microsoft.com/link");

    let account = auth.login_with_device_code(&code).await.unwrap();
    assert_eq!(account.name, "HowDoesAuthWork");
    assert!(account.entitlement_verified);
}

#[tokio::test]
async fn a_rejected_refresh_token_asks_the_account_to_sign_in_again() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/oauth2/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error": "invalid_grant"})))
        .mount(&server)
        .await;

    let error = authenticator(&server)
        .refresh(&Secret::new("stale-refresh-token"))
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        Error::Microsoft(acelus_auth::microsoft::Error::RefreshRejected)
    ));
}
