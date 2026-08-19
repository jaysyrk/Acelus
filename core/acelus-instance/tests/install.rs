use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;

use acelus_cas::Store;
use acelus_instance::install::Installer;
use acelus_instance::lock::Role;
use acelus_instance::paths::{InstanceLayout, Paths};
use acelus_instance::resolve::Resolver;
use acelus_instance::Progress;
use acelus_meta::{Arch, Environment, Os};
use acelus_net::Fetcher;
use serde_json::{json, Value};
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const LEGACY: &[u8] = include_bytes!("../../acelus-meta/tests/fixtures/1.12.2.json");
const JAVA_ALL: &[u8] = include_bytes!("../../acelus-meta/tests/fixtures/java_runtime_all.json");

fn sha1_hex(bytes: &[u8]) -> String {
    use sha1::Digest;
    hex::encode(sha1::Sha1::digest(bytes))
}

fn native_jar() -> Vec<u8> {
    let mut buffer = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buffer));
        let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();

        zip.start_file("liblwjgl.so", options).unwrap();
        zip.write_all(b"native library payload").unwrap();

        zip.start_file("META-INF/MANIFEST.MF", options).unwrap();
        zip.write_all(b"Manifest-Version: 1.0").unwrap();

        zip.finish().unwrap();
    }
    buffer
}

fn hostile_jar() -> Vec<u8> {
    let mut buffer = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buffer));
        let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        zip.start_file("../../escaped.so", options).unwrap();
        zip.write_all(b"you should never see this").unwrap();
        zip.finish().unwrap();
    }
    buffer
}

struct Blobs(HashMap<String, Vec<u8>>);

impl Respond for Blobs {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        match self.0.get(request.url.path()) {
            Some(body) => ResponseTemplate::new(200).set_body_bytes(body.clone()),
            None => ResponseTemplate::new(404),
        }
    }
}

struct Rewriter {
    base: String,
    blobs: HashMap<String, Vec<u8>>,
    next: usize,
}

impl Rewriter {
    fn new(base: &str) -> Self {
        Self {
            base: base.to_string(),
            blobs: HashMap::new(),
            next: 0,
        }
    }

    fn publish(&mut self, body: Vec<u8>) -> (String, String, usize) {
        let route = format!("/blob/{}", self.next);
        self.next += 1;
        let digest = sha1_hex(&body);
        let size = body.len();
        self.blobs.insert(route.clone(), body);
        (format!("{}{route}", self.base), digest, size)
    }

    fn rewrite_download(&mut self, node: &mut Value, body: Vec<u8>) {
        let (url, sha1, size) = self.publish(body);
        node["url"] = json!(url);
        node["sha1"] = json!(sha1);
        node["size"] = json!(size);
    }

    fn rewrite_version(&mut self, raw: &[u8], asset_index: Vec<u8>) -> Vec<u8> {
        let mut version: Value = serde_json::from_slice(raw).unwrap();

        let client = b"client jar payload".to_vec();
        self.rewrite_download(&mut version["downloads"]["client"], client);
        if version["downloads"].get("server").is_some() {
            version["downloads"]
                .as_object_mut()
                .unwrap()
                .remove("server");
        }

        let libraries = version["libraries"].as_array_mut().unwrap();
        for (index, library) in libraries.iter_mut().enumerate() {
            if let Some(artifact) = library
                .get_mut("downloads")
                .and_then(|d| d.get_mut("artifact"))
            {
                let body = format!("library payload {index}").into_bytes();
                self.rewrite_download(artifact, body);
            }

            if let Some(classifiers) = library
                .get_mut("downloads")
                .and_then(|d| d.get_mut("classifiers"))
                .and_then(|c| c.as_object_mut())
            {
                let keys: Vec<String> = classifiers.keys().cloned().collect();
                for key in keys {
                    let jar = native_jar();
                    self.rewrite_download(classifiers.get_mut(&key).unwrap(), jar);
                }
            }
        }

        if let Some(file) = version
            .get_mut("logging")
            .and_then(|l| l.get_mut("client"))
            .and_then(|c| c.get_mut("file"))
        {
            self.rewrite_download(file, b"<Configuration/>".to_vec());
        }

        self.rewrite_download(&mut version["assetIndex"], asset_index);

        serde_json::to_vec(&version).unwrap()
    }
}

struct Harness {
    _server: MockServer,
    installer: Installer,
    resolver: Resolver,
    paths: Paths,
    _dir: tempfile::TempDir,
}

async fn harness() -> Harness {
    let server = MockServer::start().await;
    let base = server.uri();
    let dir = tempfile::tempdir().unwrap();

    let asset_body = b"asset object payload".to_vec();
    let asset_index = serde_json::to_vec(&json!({
        "objects": {
            "icons/icon.png": {"hash": sha1_hex(&asset_body), "size": asset_body.len()}
        }
    }))
    .unwrap();

    let mut rewriter = Rewriter::new(&base);
    let version_body = rewriter.rewrite_version(LEGACY, asset_index);
    let version_sha1 = sha1_hex(&version_body);

    let asset_route = format!("/{}/{}", &sha1_hex(&asset_body)[..2], sha1_hex(&asset_body));
    rewriter.blobs.insert(asset_route, asset_body);

    let manifest = json!({
        "latest": {"release": "1.12.2", "snapshot": "1.12.2"},
        "versions": [{
            "id": "1.12.2",
            "type": "release",
            "url": format!("{base}/version.json"),
            "time": "2017-09-18T08:39:46+00:00",
            "releaseTime": "2017-09-18T08:39:46+00:00",
            "sha1": version_sha1,
            "complianceLevel": 0
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
        .and(path("/java-all.json"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(JAVA_ALL))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path_regex(r"^/(blob/\d+|[0-9a-f]{2}/[0-9a-f]{40})$"))
        .respond_with(Blobs(rewriter.blobs))
        .mount(&server)
        .await;

    let fetcher = Fetcher::builder()
        .allow_any_host()
        .require_https(false)
        .build()
        .unwrap();

    let paths = Paths::with_root(dir.path().join("acelus"));
    let store = Store::open(paths.store()).unwrap();

    let resolver = Resolver::with_endpoints(
        fetcher.clone(),
        format!("{base}/version_manifest.json"),
        format!("{base}/java-all.json"),
    );

    Harness {
        installer: Installer::new(fetcher, store, paths.clone()).with_asset_base_url(&base),
        resolver,
        paths,
        _server: server,
        _dir: dir,
    }
}

fn linux() -> Environment {
    Environment::new(Os::Linux, Arch::X86_64)
}

fn silent() -> impl Fn(&Progress) + Sync {
    |_: &Progress| {}
}

async fn install_into(harness: &Harness, name: &str) -> acelus_instance::Lockfile {
    let plan = harness.resolver.resolve("1.12.2", &linux()).await.unwrap();
    let layout = InstanceLayout::new(harness.paths.instance(name));
    harness
        .installer
        .install(&plan, &layout, &silent())
        .await
        .unwrap()
}

fn physical_bytes(root: &std::path::Path) -> u64 {
    fn walk(path: &std::path::Path, seen: &mut std::collections::HashSet<u64>, total: &mut u64) {
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                walk(&entry.path(), seen, total);
            } else {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if seen.insert(meta.ino()) {
                        *total += meta.len();
                    }
                }
                #[cfg(not(unix))]
                {
                    let _ = &seen;
                    *total += meta.len();
                }
            }
        }
    }

    let mut seen = std::collections::HashSet::new();
    let mut total = 0;
    walk(root, &mut seen, &mut total);
    total
}

#[tokio::test]
async fn every_locked_artifact_is_on_disk_and_verifies() {
    let harness = harness().await;
    let lockfile = install_into(&harness, "survival").await;
    let layout = InstanceLayout::new(harness.paths.instance("survival"));

    assert!(!lockfile.artifacts.is_empty());

    for artifact in &lockfile.artifacts {
        let destination = match artifact.role {
            Role::Asset => harness.paths.root().join(&artifact.path),
            _ => layout.resolve(&artifact.path),
        };

        let metadata = std::fs::metadata(&destination)
            .unwrap_or_else(|_| panic!("{} was locked but is not on disk", artifact.path));
        assert_eq!(
            metadata.len(),
            artifact.size,
            "{} is the wrong size on disk",
            artifact.path
        );

        let hash: acelus_cas::Hash = artifact.blake3.parse().unwrap();
        harness
            .installer
            .store()
            .verify(&hash)
            .unwrap_or_else(|e| panic!("{} failed verification: {e}", artifact.path));
    }
}

#[tokio::test]
async fn the_lockfile_records_what_the_plan_promised() {
    let harness = harness().await;
    let lockfile = install_into(&harness, "survival").await;

    assert_eq!(lockfile.minecraft.id, "1.12.2");
    assert_eq!(lockfile.java.major_version, 8);
    assert_eq!(lockfile.artifacts_with_role(Role::Client).count(), 1);
    assert_eq!(lockfile.artifacts_with_role(Role::Native).count(), 3);
    assert_eq!(lockfile.artifacts_with_role(Role::Asset).count(), 1);

    for artifact in &lockfile.artifacts {
        assert_eq!(artifact.blake3.len(), 64);
        assert!(artifact.sha1.is_some());
        assert!(!artifact.origin.is_empty());
    }
}

#[tokio::test]
async fn natives_are_extracted_with_excluded_entries_left_out() {
    let harness = harness().await;
    install_into(&harness, "survival").await;
    let layout = InstanceLayout::new(harness.paths.instance("survival"));

    let extracted = layout.natives().join("liblwjgl.so");
    assert!(
        extracted.is_file(),
        "the native library should have been unpacked"
    );
    assert_eq!(
        std::fs::read(&extracted).unwrap(),
        b"native library payload"
    );

    assert!(
        !layout.natives().join("META-INF").exists(),
        "META-INF is excluded by the manifest and must not be unpacked"
    );
}

#[tokio::test]
async fn a_second_instance_of_the_same_version_costs_no_extra_disk() {
    let harness = harness().await;

    install_into(&harness, "first").await;
    let after_first = physical_bytes(harness.paths.root());

    install_into(&harness, "second").await;
    let after_second = physical_bytes(harness.paths.root());

    let natives_growth: u64 = 3 * (b"native library payload".len() as u64);
    let growth = after_second - after_first;

    assert!(
        growth <= natives_growth,
        "a second instance grew the install by {growth} bytes; only unpacked natives should cost \
         anything, which is at most {natives_growth}"
    );
}

#[tokio::test]
async fn assets_are_stored_once_outside_any_instance() {
    let harness = harness().await;
    let lockfile = install_into(&harness, "survival").await;

    let index = lockfile
        .artifacts_with_role(Role::Asset)
        .next()
        .expect("the asset index is locked");
    assert!(index.path.starts_with("assets/indexes/"));

    let objects = harness.paths.asset_objects();
    let stored: Vec<_> = walkdir(&objects);
    assert_eq!(stored.len(), 1, "the single asset object should be stored");
    assert_eq!(std::fs::read(&stored[0]).unwrap(), b"asset object payload");

    assert!(
        !objects.starts_with(harness.paths.instances()),
        "assets must be shared, not held inside an instance"
    );
}

#[tokio::test]
async fn reinstalling_reuses_the_store_instead_of_the_network() {
    let harness = harness().await;
    install_into(&harness, "first").await;

    let reused = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let sink = reused.clone();

    let plan = harness.resolver.resolve("1.12.2", &linux()).await.unwrap();
    let layout = InstanceLayout::new(harness.paths.instance("second"));
    harness
        .installer
        .install(&plan, &layout, &move |progress: &Progress| {
            sink.store(progress.reused_bytes, std::sync::atomic::Ordering::Relaxed);
        })
        .await
        .unwrap();

    assert!(
        reused.load(std::sync::atomic::Ordering::Relaxed) > 0,
        "the second install should have satisfied artifacts from the store"
    );
}

#[tokio::test]
async fn a_hostile_archive_cannot_write_outside_the_natives_directory() {
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("instance/natives");

    let error =
        acelus_instance::extract::extract_natives(&hostile_jar(), &destination, &[]).unwrap_err();

    assert!(
        matches!(error, acelus_instance::extract::Error::PathEscape { .. }),
        "a traversal entry must be refused, got {error:?}"
    );
    assert!(
        !dir.path().join("escaped.so").exists(),
        "nothing should have been written outside the destination"
    );
}

fn walkdir(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                found.push(path);
            }
        }
    }

    found
}
