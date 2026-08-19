use acelus_instance::loader::{Error, LoaderRequest, LoaderResolver};
use acelus_instance::lock::{JavaSource, LockMinecraft, Role};
use acelus_instance::plan::{Plan, PlannedArtifact, PlannedJava};
use acelus_meta::arguments::ResolvedArguments;
use acelus_meta::LoaderKind;
use acelus_net::Fetcher;
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const GAME_VERSION: &str = "1.21.4";
const ASM_SHA1: &str = "1111111111111111111111111111111111111111";

fn fabric_versions(maven: &str) -> Value {
    json!([
        {
            "loader": {
                "separator": ".",
                "build": 3,
                "maven": "net.fabricmc:fabric-loader:0.19.3",
                "version": "0.19.3",
                "sha1": "2222222222222222222222222222222222222222",
                "size": 1_536_000
            },
            "intermediary": {
                "maven": "net.fabricmc:intermediary:1.21.4",
                "version": "1.21.4",
                "sha1": "3333333333333333333333333333333333333333"
            },
            "launcherMeta": {
                "version": 2,
                "min_java_version": 21,
                "mainClass": { "client": "net.fabricmc.loader.impl.launch.knot.KnotClient" },
                "libraries": {
                    "common": [
                        {
                            "name": "org.ow2.asm:asm:9.10.1",
                            "url": maven,
                            "sha1": ASM_SHA1,
                            "size": 125_000
                        },
                        {
                            "name": "net.fabricmc:sponge-mixin:0.16.5",
                            "url": maven
                        }
                    ],
                    "client": []
                }
            }
        },
        {
            "loader": {
                "maven": "net.fabricmc:fabric-loader:0.19.2",
                "version": "0.19.2",
                "sha1": "4444444444444444444444444444444444444444"
            },
            "launcherMeta": {
                "version": 2,
                "mainClass": { "client": "net.fabricmc.loader.impl.launch.knot.KnotClient" },
                "libraries": { "common": [], "client": [] }
            }
        }
    ])
}

const MIXIN_SHA1: &str = "5555555555555555555555555555555555555555";
const MIXIN_PATH: &str = "/maven/net/fabricmc/sponge-mixin/0.16.5/sponge-mixin-0.16.5.jar.sha1";

async fn server(sidecar: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    let maven = format!("{}/maven/", server.uri());

    Mock::given(method("GET"))
        .and(path(format!("/v2/versions/loader/{GAME_VERSION}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(fabric_versions(&maven)))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path(MIXIN_PATH))
        .respond_with(sidecar)
        .mount(&server)
        .await;

    server
}

fn resolver(server: &MockServer) -> LoaderResolver {
    let fetcher = Fetcher::builder()
        .allow_any_host()
        .require_https(false)
        .build()
        .expect("the test fetcher builds");
    LoaderResolver::with_endpoints(
        fetcher,
        format!("{}/v2", server.uri()),
        format!("{}/v3", server.uri()),
    )
}

fn vanilla_plan() -> Plan {
    Plan {
        minecraft: LockMinecraft {
            id: GAME_VERSION.into(),
            release_type: "release".into(),
            manifest_sha1: "a".repeat(40),
            asset_index_id: Some("19".into()),
            main_class: "net.minecraft.client.main.Main".into(),
            compliance_level: Some(1),
        },
        loader: None,
        java: PlannedJava {
            component: Some("java-runtime-delta".into()),
            major_version: 17,
            source: JavaSource::Mojang,
            manifest_url: None,
            manifest_sha1: None,
        },
        artifacts: vec![
            PlannedArtifact::new("client.jar", Role::Client, "https://example/client.jar"),
            PlannedArtifact::new(
                "libraries/org/ow2/asm/asm/9.7/asm-9.7.jar",
                Role::Library,
                "https://libraries.minecraft.net/org/ow2/asm/asm/9.7/asm-9.7.jar",
            ),
            PlannedArtifact::new(
                "libraries/com/google/guava/guava/33.3.1-jre/guava-33.3.1-jre.jar",
                Role::Library,
                "https://libraries.minecraft.net/guava.jar",
            ),
            PlannedArtifact::new(
                "natives/jars/lwjgl.jar",
                Role::Native,
                "https://example/n.jar",
            ),
        ],
        arguments: ResolvedArguments::default(),
        asset_index: None,
    }
}

fn sha1_body(digest: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_string(digest.to_string())
}

#[tokio::test]
async fn the_newest_published_build_is_taken_when_none_is_pinned() {
    let server = server(sha1_body(MIXIN_SHA1)).await;
    let profile = resolver(&server)
        .resolve(&LoaderRequest::latest(LoaderKind::Fabric), GAME_VERSION)
        .await
        .unwrap();

    assert_eq!(profile.loader_version, "0.19.3");
    assert_eq!(profile.intermediary_version.as_deref(), Some("1.21.4"));
    assert_eq!(
        profile.main_class,
        "net.fabricmc.loader.impl.launch.knot.KnotClient"
    );
    assert_eq!(profile.min_java_version, Some(21));
}

#[tokio::test]
async fn a_pinned_build_is_honoured_and_an_unpublished_one_is_refused() {
    let server = server(sha1_body(MIXIN_SHA1)).await;
    let resolver = resolver(&server);

    let pinned = resolver
        .resolve(
            &LoaderRequest {
                kind: LoaderKind::Fabric,
                version: Some("0.19.2".into()),
            },
            GAME_VERSION,
        )
        .await
        .unwrap();
    assert_eq!(pinned.loader_version, "0.19.2");

    let missing = resolver
        .resolve(
            &LoaderRequest {
                kind: LoaderKind::Fabric,
                version: Some("0.0.1".into()),
            },
            GAME_VERSION,
        )
        .await;
    assert!(matches!(missing, Err(Error::Metadata(_))));
}

#[tokio::test]
async fn applying_a_loader_rewrites_the_plan_it_is_merged_into() {
    let server = server(sha1_body(MIXIN_SHA1)).await;
    let resolver = resolver(&server);

    let profile = resolver
        .resolve(&LoaderRequest::latest(LoaderKind::Fabric), GAME_VERSION)
        .await
        .unwrap();

    let mut plan = vanilla_plan();
    resolver.apply(&mut plan, &profile).await.unwrap();

    assert_eq!(
        plan.minecraft.main_class,
        "net.fabricmc.loader.impl.launch.knot.KnotClient"
    );

    let loader = plan.loader.as_ref().expect("the plan records its loader");
    assert_eq!(loader.kind, LoaderKind::Fabric);
    assert_eq!(loader.version, "0.19.3");
    assert_eq!(loader.intermediary_version.as_deref(), Some("1.21.4"));

    assert_eq!(
        plan.java.major_version, 21,
        "a loader that needs a newer Java must raise the runtime, never lower it"
    );

    let paths: Vec<&str> = plan.artifacts.iter().map(|a| a.path.as_str()).collect();
    assert!(paths.contains(&"libraries/net/fabricmc/fabric-loader/0.19.3/fabric-loader-0.19.3.jar"));
    assert!(paths.contains(&"libraries/org/ow2/asm/asm/9.10.1/asm-9.10.1.jar"));
    assert!(
        !paths.contains(&"libraries/org/ow2/asm/asm/9.7/asm-9.7.jar"),
        "the loader's asm supersedes the vanilla one, or both land on the classpath"
    );
    assert!(paths.contains(&"libraries/com/google/guava/guava/33.3.1-jre/guava-33.3.1-jre.jar"));
    assert!(paths.contains(&"natives/jars/lwjgl.jar"));

    let mixin = plan
        .artifacts
        .iter()
        .find(|a| a.path.contains("sponge-mixin"))
        .expect("the mixin library is planned");
    assert_eq!(
        mixin.sha1.as_deref(),
        Some(MIXIN_SHA1),
        "a library published without a hash must be pinned from its maven sidecar"
    );

    let first_library = plan
        .artifacts
        .iter()
        .position(|a| a.role == Role::Library)
        .unwrap();
    assert!(
        plan.artifacts[first_library].path.contains("fabric-loader"),
        "the loader must precede the vanilla libraries it overrides"
    );
}

#[tokio::test]
async fn a_library_whose_checksum_cannot_be_read_is_never_installed() {
    for sidecar in [
        ResponseTemplate::new(404),
        ResponseTemplate::new(200).set_body_string("<!doctype html><title>404</title>"),
    ] {
        let server = server(sidecar).await;
        let resolver = resolver(&server);
        let profile = resolver
            .resolve(&LoaderRequest::latest(LoaderKind::Fabric), GAME_VERSION)
            .await
            .unwrap();

        let mut plan = vanilla_plan();
        assert!(
            matches!(
                resolver.apply(&mut plan, &profile).await,
                Err(Error::NoChecksum { .. })
            ),
            "an unverifiable jar must stop the install, not be fetched on trust"
        );
    }
}
