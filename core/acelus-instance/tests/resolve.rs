use acelus_instance::lock::{JavaSource, Role};
use acelus_instance::resolve::{Error, Resolver};
use acelus_meta::{Arch, Environment, Os};
use acelus_net::Fetcher;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const MODERN: &[u8] = include_bytes!("../../acelus-meta/tests/fixtures/26.2.json");
const LEGACY: &[u8] = include_bytes!("../../acelus-meta/tests/fixtures/1.12.2.json");
const JAVA_ALL: &[u8] = include_bytes!("../../acelus-meta/tests/fixtures/java_runtime_all.json");

fn sha1_hex(bytes: &[u8]) -> String {
    use sha1::Digest;
    hex::encode(sha1::Sha1::digest(bytes))
}

fn asset_index_body() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "objects": {
            "icons/icon_128x128.png": {
                "hash": "b62ca8ec10d07e6bf5ac8dae0c8c1d2e6a1e3356",
                "size": 9101
            }
        }
    }))
    .unwrap()
}

fn rewrite_asset_index(raw: &[u8], base: &str, body: &[u8]) -> Vec<u8> {
    let mut version: serde_json::Value = serde_json::from_slice(raw).unwrap();
    if let Some(index) = version.get_mut("assetIndex") {
        index["url"] = json!(format!("{base}/asset-index.json"));
        index["sha1"] = json!(sha1_hex(body));
        index["size"] = json!(body.len());
    }
    serde_json::to_vec(&version).unwrap()
}

struct Harness {
    server: MockServer,
    resolver: Resolver,
}

async fn harness(version_id: &str, raw: &[u8]) -> Harness {
    let server = MockServer::start().await;
    let base = server.uri();

    let index_body = asset_index_body();
    let version_body = rewrite_asset_index(raw, &base, &index_body);
    let version_sha1 = sha1_hex(&version_body);

    let manifest = json!({
        "latest": {"release": version_id, "snapshot": version_id},
        "versions": [{
            "id": version_id,
            "type": "release",
            "url": format!("{base}/version.json"),
            "time": "2026-01-01T00:00:00+00:00",
            "releaseTime": "2026-01-01T00:00:00+00:00",
            "sha1": version_sha1,
            "complianceLevel": 1
        }]
    });

    Mock::given(method("GET"))
        .and(path("/version_manifest.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(manifest))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/version.json"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(version_body))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/asset-index.json"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(index_body))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/java-all.json"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(JAVA_ALL))
        .mount(&server)
        .await;

    let fetcher = Fetcher::builder()
        .allow_any_host()
        .require_https(false)
        .build()
        .unwrap();

    let resolver = Resolver::with_endpoints(
        fetcher,
        format!("{base}/version_manifest.json"),
        format!("{base}/java-all.json"),
    );

    Harness { server, resolver }
}

fn linux() -> Environment {
    Environment::new(Os::Linux, Arch::X86_64)
}

#[tokio::test]
async fn a_modern_version_resolves_into_a_complete_plan() {
    let harness = harness("26.2", MODERN).await;
    let plan = harness.resolver.resolve("26.2", &linux()).await.unwrap();

    assert_eq!(plan.minecraft.id, "26.2");
    assert_eq!(plan.minecraft.main_class, "net.minecraft.client.main.Main");
    assert_eq!(plan.minecraft.asset_index_id.as_deref(), Some("32"));
    assert_eq!(plan.minecraft.manifest_sha1.len(), 40);

    assert_eq!(plan.artifacts_with_role(Role::Client).count(), 1);
    assert_eq!(plan.artifacts_with_role(Role::Library).count(), 68);
    assert_eq!(plan.artifacts_with_role(Role::Native).count(), 0);
    assert_eq!(plan.artifacts_with_role(Role::LoggingConfig).count(), 1);
    assert_eq!(plan.artifacts_with_role(Role::Asset).count(), 1);

    drop(harness.server);
}

#[tokio::test]
async fn every_planned_artifact_can_be_verified_before_use() {
    let harness = harness("26.2", MODERN).await;
    let plan = harness.resolver.resolve("26.2", &linux()).await.unwrap();

    for artifact in &plan.artifacts {
        let integrity = artifact.integrity();
        assert!(
            integrity.sha1.is_some(),
            "{} would be installed without a digest to check",
            artifact.path
        );
        assert!(integrity.size.is_some(), "{} has no size", artifact.path);
        assert!(
            !artifact.origin.is_empty(),
            "{} has nowhere to be fetched from",
            artifact.path
        );
    }

    drop(harness.server);
}

#[tokio::test]
async fn the_asset_index_is_pinned_as_an_artifact_so_assets_are_transitively_verified() {
    let harness = harness("26.2", MODERN).await;
    let plan = harness.resolver.resolve("26.2", &linux()).await.unwrap();

    let index = plan
        .artifacts_with_role(Role::Asset)
        .next()
        .expect("the asset index is pinned");

    assert_eq!(index.path, "assets/indexes/32.json");
    assert!(index.sha1.is_some());

    let parsed = plan.asset_index.expect("the index was fetched and parsed");
    assert_eq!(parsed.objects.len(), 1);

    drop(harness.server);
}

#[tokio::test]
async fn a_legacy_version_plans_natives_for_extraction() {
    let harness = harness("1.12.2", LEGACY).await;
    let plan = harness.resolver.resolve("1.12.2", &linux()).await.unwrap();

    assert_eq!(plan.artifacts_with_role(Role::Native).count(), 3);

    for native in plan.artifacts_with_role(Role::Native) {
        assert!(native.path.starts_with("natives/"));
        assert_eq!(
            native.extract.as_deref(),
            Some(&["META-INF/".to_string()][..])
        );
    }

    assert_eq!(plan.arguments.jvm.last().unwrap(), "${classpath}");

    drop(harness.server);
}

#[tokio::test]
async fn a_modern_version_resolves_its_java_runtime_from_mojang() {
    let harness = harness("26.2", MODERN).await;
    let plan = harness.resolver.resolve("26.2", &linux()).await.unwrap();

    assert_eq!(plan.java.source, JavaSource::Mojang);
    assert_eq!(plan.java.component.as_deref(), Some("java-runtime-epsilon"));
    assert_eq!(plan.java.major_version, 25);
    assert!(plan.java.manifest_url.is_some());
    assert_eq!(plan.java.manifest_sha1.as_ref().unwrap().len(), 40);

    drop(harness.server);
}

#[tokio::test]
async fn a_platform_mojang_does_not_ship_for_falls_back_to_adoptium() {
    let harness = harness("26.2", MODERN).await;
    let arm_linux = Environment::new(Os::Linux, Arch::Arm64);

    let plan = harness.resolver.resolve("26.2", &arm_linux).await.unwrap();

    assert_eq!(
        plan.java.source,
        JavaSource::Adoptium,
        "Linux on ARM has no Mojang runtime and must fall back"
    );
    assert!(plan.java.manifest_url.is_none());

    drop(harness.server);
}

#[tokio::test]
async fn an_old_version_on_apple_silicon_falls_back_to_adoptium() {
    let harness = harness("1.12.2", LEGACY).await;
    let apple_silicon = Environment::new(Os::MacOs, Arch::Arm64);

    let plan = harness
        .resolver
        .resolve("1.12.2", &apple_silicon)
        .await
        .unwrap();

    assert_eq!(plan.java.component.as_deref(), Some("jre-legacy"));
    assert_eq!(
        plan.java.source,
        JavaSource::Adoptium,
        "Mojang ships no legacy runtime for Apple Silicon"
    );

    drop(harness.server);
}

#[tokio::test]
async fn an_unknown_version_is_named_rather_than_guessed_at() {
    let harness = harness("26.2", MODERN).await;

    let error = harness
        .resolver
        .resolve("1.2.3-does-not-exist", &linux())
        .await
        .unwrap_err();

    match error {
        Error::UnknownVersion { id } => assert_eq!(id, "1.2.3-does-not-exist"),
        other => panic!("expected an unknown version error, got {other:?}"),
    }

    drop(harness.server);
}

#[tokio::test]
async fn tampered_version_metadata_is_rejected_by_its_manifest_digest() {
    let server = MockServer::start().await;
    let base = server.uri();

    let manifest = json!({
        "latest": {"release": "26.2", "snapshot": "26.2"},
        "versions": [{
            "id": "26.2",
            "type": "release",
            "url": format!("{base}/version.json"),
            "time": "2026-01-01T00:00:00+00:00",
            "releaseTime": "2026-01-01T00:00:00+00:00",
            "sha1": "0".repeat(40),
            "complianceLevel": 1
        }]
    });

    Mock::given(method("GET"))
        .and(path("/version_manifest.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(manifest))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/version.json"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(MODERN))
        .mount(&server)
        .await;

    let resolver = Resolver::with_endpoints(
        Fetcher::builder()
            .allow_any_host()
            .require_https(false)
            .attempts(1)
            .build()
            .unwrap(),
        format!("{base}/version_manifest.json"),
        format!("{base}/java-all.json"),
    );

    let error = resolver.resolve("26.2", &linux()).await.unwrap_err();
    assert!(
        matches!(error, Error::Fetch { .. }),
        "metadata whose digest disagrees with the manifest must be refused, got {error:?}"
    );
}

#[tokio::test]
async fn planned_download_size_accounts_for_assets_as_well_as_artifacts() {
    let harness = harness("26.2", MODERN).await;
    let plan = harness.resolver.resolve("26.2", &linux()).await.unwrap();

    let artifacts: u64 = plan.artifacts.iter().filter_map(|a| a.size).sum();
    assert_eq!(plan.total_download_size(), artifacts + 9101);

    drop(harness.server);
}
