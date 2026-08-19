use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use acelus_instance::java::{Error, JavaProvisioner, MARKER_FILE};
use acelus_instance::paths::Paths;
use acelus_instance::plan::PlannedJava;
use acelus_instance::JavaSource;
use acelus_meta::Os;
use acelus_net::Fetcher;
use serde_json::json;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

fn sha1_hex(bytes: &[u8]) -> String {
    use sha1::Digest;
    hex::encode(sha1::Sha1::digest(bytes))
}

struct Blobs {
    bodies: HashMap<String, Vec<u8>>,
    hits: Arc<AtomicUsize>,
}

impl Respond for Blobs {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        self.hits.fetch_add(1, Ordering::SeqCst);
        match self.bodies.get(request.url.path()) {
            Some(body) => ResponseTemplate::new(200).set_body_bytes(body.clone()),
            None => ResponseTemplate::new(404),
        }
    }
}

struct Harness {
    _server: MockServer,
    provisioner: JavaProvisioner,
    paths: Paths,
    java: PlannedJava,
    blob_hits: Arc<AtomicUsize>,
    _dir: tempfile::TempDir,
}

async fn harness() -> Harness {
    let server = MockServer::start().await;
    let base = server.uri();
    let dir = tempfile::tempdir().unwrap();

    let java_binary = b"#!/bin/sh\nexit 0\n".to_vec();
    let library = b"shared object payload".to_vec();
    let licence = b"licence text".to_vec();

    let mut bodies = HashMap::new();
    bodies.insert("/blob/java".to_string(), java_binary.clone());
    bodies.insert("/blob/library".to_string(), library.clone());
    bodies.insert("/blob/licence".to_string(), licence.clone());

    let listing = json!({
        "files": {
            "bin": {"type": "directory"},
            "lib": {"type": "directory"},
            "legal/java.base": {"type": "directory"},
            "bin/java": {
                "type": "file",
                "executable": true,
                "downloads": {"raw": {
                    "sha1": sha1_hex(&java_binary),
                    "size": java_binary.len(),
                    "url": format!("{base}/blob/java")
                }}
            },
            "lib/libjvm.so": {
                "type": "file",
                "executable": false,
                "downloads": {"raw": {
                    "sha1": sha1_hex(&library),
                    "size": library.len(),
                    "url": format!("{base}/blob/library")
                }}
            },
            "legal/java.base/LICENCE": {
                "type": "file",
                "executable": false,
                "downloads": {"raw": {
                    "sha1": sha1_hex(&licence),
                    "size": licence.len(),
                    "url": format!("{base}/blob/licence")
                }}
            },
            "legal/java.compiler/LICENCE": {
                "type": "link",
                "target": "../java.base/LICENCE"
            }
        }
    });

    let listing_body = serde_json::to_vec(&listing).unwrap();
    let listing_sha1 = sha1_hex(&listing_body);
    let hits = Arc::new(AtomicUsize::new(0));

    Mock::given(method("GET"))
        .and(path("/listing.json"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(listing_body))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path_regex(r"^/blob/"))
        .respond_with(Blobs {
            bodies,
            hits: hits.clone(),
        })
        .mount(&server)
        .await;

    let fetcher = Fetcher::builder()
        .allow_any_host()
        .require_https(false)
        .build()
        .unwrap();

    let paths = Paths::with_root(dir.path().join("acelus"));

    Harness {
        provisioner: JavaProvisioner::new(fetcher, paths.clone()),
        paths,
        java: PlannedJava {
            component: Some("java-runtime-test".into()),
            major_version: 25,
            source: JavaSource::Mojang,
            manifest_url: Some(format!("{base}/listing.json")),
            manifest_sha1: Some(listing_sha1),
        },
        blob_hits: hits,
        _server: server,
        _dir: dir,
    }
}

#[tokio::test]
async fn a_runtime_is_laid_out_with_files_directories_and_links() {
    let harness = harness().await;
    let runtime = harness
        .provisioner
        .provision(&harness.java, Os::Linux)
        .await
        .unwrap();

    let root = harness.paths.runtime("java-runtime-test");
    assert_eq!(runtime.root, root);
    assert_eq!(runtime.java_executable, root.join("bin/java"));
    assert_eq!(runtime.major_version, 25);

    assert!(root.join("lib").is_dir());
    assert_eq!(
        std::fs::read(root.join("lib/libjvm.so")).unwrap(),
        b"shared object payload"
    );
    assert_eq!(
        std::fs::read(root.join("legal/java.compiler/LICENCE")).unwrap(),
        b"licence text",
        "the link should resolve to the file it points at"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn the_java_binary_is_executable_and_the_libraries_are_not() {
    use std::os::unix::fs::PermissionsExt;

    let harness = harness().await;
    harness
        .provisioner
        .provision(&harness.java, Os::Linux)
        .await
        .unwrap();

    let root = harness.paths.runtime("java-runtime-test");

    let java = std::fs::metadata(root.join("bin/java")).unwrap();
    assert!(
        java.permissions().mode() & 0o111 != 0,
        "a runtime whose java binary is not executable cannot launch anything"
    );

    let library = std::fs::metadata(root.join("lib/libjvm.so")).unwrap();
    assert_eq!(
        library.permissions().mode() & 0o111,
        0,
        "only entries the listing marks executable should gain the bit"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn links_are_created_as_symlinks_rather_than_copies() {
    let harness = harness().await;
    harness
        .provisioner
        .provision(&harness.java, Os::Linux)
        .await
        .unwrap();

    let link = harness
        .paths
        .runtime("java-runtime-test")
        .join("legal/java.compiler/LICENCE");

    let metadata = std::fs::symlink_metadata(&link).unwrap();
    assert!(
        metadata.file_type().is_symlink(),
        "205 of the entries in a real runtime are links; copying them all wastes space"
    );
}

#[tokio::test]
async fn a_provisioned_runtime_is_not_downloaded_twice() {
    let harness = harness().await;

    harness
        .provisioner
        .provision(&harness.java, Os::Linux)
        .await
        .unwrap();
    let after_first = harness.blob_hits.load(Ordering::SeqCst);
    assert!(after_first > 0);

    harness
        .provisioner
        .provision(&harness.java, Os::Linux)
        .await
        .unwrap();

    assert_eq!(
        harness.blob_hits.load(Ordering::SeqCst),
        after_first,
        "a runtime already marked complete should not be fetched again"
    );
}

#[tokio::test]
async fn a_runtime_installed_from_a_different_listing_is_reprovisioned() {
    let harness = harness().await;
    harness
        .provisioner
        .provision(&harness.java, Os::Linux)
        .await
        .unwrap();

    let marker = harness.paths.runtime("java-runtime-test").join(MARKER_FILE);
    let genuine = std::fs::read_to_string(&marker).unwrap();
    std::fs::write(&marker, "0".repeat(40)).unwrap();

    harness
        .provisioner
        .provision(&harness.java, Os::Linux)
        .await
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(&marker).unwrap(),
        genuine,
        "a stale marker must trigger reprovisioning, which restores the real listing digest"
    );
}

#[tokio::test]
async fn reprovisioning_does_not_refetch_files_already_correct_on_disk() {
    let harness = harness().await;
    harness
        .provisioner
        .provision(&harness.java, Os::Linux)
        .await
        .unwrap();

    let marker = harness.paths.runtime("java-runtime-test").join(MARKER_FILE);
    std::fs::write(&marker, "0".repeat(40)).unwrap();

    let before = harness.blob_hits.load(Ordering::SeqCst);
    harness
        .provisioner
        .provision(&harness.java, Os::Linux)
        .await
        .unwrap();

    assert_eq!(
        harness.blob_hits.load(Ordering::SeqCst),
        before,
        "files already present at the right size should not be downloaded again"
    );
}

#[tokio::test]
async fn a_platform_without_a_mojang_runtime_says_so_plainly() {
    let harness = harness().await;

    let unavailable = PlannedJava {
        component: Some("jre-legacy".into()),
        major_version: 8,
        source: JavaSource::Adoptium,
        manifest_url: None,
        manifest_sha1: None,
    };

    let error = harness
        .provisioner
        .provision(&unavailable, Os::Linux)
        .await
        .unwrap_err();

    match error {
        Error::NoMojangRuntime { major_version } => assert_eq!(major_version, 8),
        other => panic!("expected a clear explanation, got {other:?}"),
    }
}

#[tokio::test]
async fn a_corrupt_listing_is_refused_by_its_digest() {
    let server = MockServer::start().await;
    let base = server.uri();
    let dir = tempfile::tempdir().unwrap();

    Mock::given(method("GET"))
        .and(path("/listing.json"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"{\"files\":{}}".to_vec()))
        .mount(&server)
        .await;

    let provisioner = JavaProvisioner::new(
        Fetcher::builder()
            .allow_any_host()
            .require_https(false)
            .attempts(1)
            .build()
            .unwrap(),
        Paths::with_root(dir.path()),
    );

    let java = PlannedJava {
        component: Some("java-runtime-test".into()),
        major_version: 25,
        source: JavaSource::Mojang,
        manifest_url: Some(format!("{base}/listing.json")),
        manifest_sha1: Some("0".repeat(40)),
    };

    let error = provisioner.provision(&java, Os::Linux).await.unwrap_err();
    assert!(
        matches!(error, Error::Fetch { .. }),
        "a listing whose digest disagrees with the manifest must be refused, got {error:?}"
    );
}
