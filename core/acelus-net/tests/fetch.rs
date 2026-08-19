use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use acelus_net::{Error, Fetcher, Integrity};
use url::Url;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const BODY: &[u8] = b"The quick brown fox jumps over the lazy dog";
const BODY_SHA1: &str = "2fd4e1c67a2d28fced849ee1bb76e7391b93eb12";

fn fetcher() -> Fetcher {
    Fetcher::builder()
        .allow_any_host()
        .require_https(false)
        .backoff(Duration::from_millis(1))
        .build()
        .unwrap()
}

fn url(server: &MockServer, suffix: &str) -> Url {
    format!("{}{}", server.uri(), suffix).parse().unwrap()
}

#[tokio::test]
async fn downloads_and_verifies_a_file() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/client.jar"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(BODY))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("client.jar");
    let integrity = Integrity::sha1(BODY_SHA1).with_size(BODY.len() as u64);

    let written = fetcher()
        .to_file(&url(&server, "/client.jar"), &integrity, &dest)
        .await
        .unwrap();

    assert_eq!(written, BODY.len() as u64);
    assert_eq!(std::fs::read(&dest).unwrap(), BODY);
}

#[tokio::test]
async fn rejects_a_body_whose_digest_does_not_match() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"tampered payload".as_slice()))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("client.jar");
    let integrity = Integrity::sha1(BODY_SHA1);

    let error = fetcher()
        .to_file(&url(&server, "/client.jar"), &integrity, &dest)
        .await
        .unwrap_err();

    assert!(
        matches!(error, Error::Integrity { .. }),
        "expected an integrity failure, got {error:?}"
    );
}

#[tokio::test]
async fn rejects_a_body_of_the_wrong_length() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(BODY))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let integrity = Integrity::sha1(BODY_SHA1).with_size(BODY.len() as u64 + 1);

    let error = fetcher()
        .to_file(
            &url(&server, "/client.jar"),
            &integrity,
            &dir.path().join("client.jar"),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, Error::Integrity { .. }));
}

#[tokio::test]
async fn refuses_hosts_outside_the_allowlist() {
    let server = MockServer::start().await;
    let fetcher = Fetcher::builder()
        .allow_host("piston-data.mojang.com")
        .require_https(false)
        .build()
        .unwrap();

    let error = fetcher.bytes(&url(&server, "/anything")).await.unwrap_err();

    assert!(
        matches!(error, Error::HostNotAllowed { .. }),
        "expected the allowlist to reject this host, got {error:?}"
    );
}

struct FailThenSucceed {
    failures: usize,
    seen: AtomicUsize,
}

impl Respond for FailThenSucceed {
    fn respond(&self, _: &Request) -> ResponseTemplate {
        if self.seen.fetch_add(1, Ordering::SeqCst) < self.failures {
            ResponseTemplate::new(503)
        } else {
            ResponseTemplate::new(200).set_body_bytes(BODY)
        }
    }
}

#[tokio::test]
async fn retries_transient_server_errors() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(FailThenSucceed {
            failures: 2,
            seen: AtomicUsize::new(0),
        })
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let written = fetcher()
        .to_file(
            &url(&server, "/flaky"),
            &Integrity::sha1(BODY_SHA1),
            &dir.path().join("flaky.bin"),
        )
        .await
        .unwrap();

    assert_eq!(written, BODY.len() as u64);
}

#[tokio::test]
async fn gives_up_after_the_configured_attempts() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let fetcher = Fetcher::builder()
        .allow_any_host()
        .require_https(false)
        .attempts(3)
        .backoff(Duration::from_millis(1))
        .build()
        .unwrap();

    let dir = tempfile::tempdir().unwrap();
    let error = fetcher
        .to_file(
            &url(&server, "/broken"),
            &Integrity::none(),
            &dir.path().join("broken.bin"),
        )
        .await
        .unwrap_err();

    match error {
        Error::Exhausted { attempts, .. } => assert_eq!(attempts, 3),
        other => panic!("expected exhaustion, got {other:?}"),
    }
}

#[tokio::test]
async fn does_not_retry_a_client_error() {
    let server = MockServer::start().await;
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();

    Mock::given(method("GET"))
        .respond_with(move |_: &Request| {
            counter.fetch_add(1, Ordering::SeqCst);
            ResponseTemplate::new(404)
        })
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let error = fetcher()
        .to_file(
            &url(&server, "/missing"),
            &Integrity::none(),
            &dir.path().join("missing.bin"),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, Error::Status { status: 404, .. }));
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn resumes_from_a_partial_file_using_a_range_request() {
    let server = MockServer::start().await;
    let split = 10;

    Mock::given(method("GET"))
        .and(header("range", format!("bytes={split}-")))
        .respond_with(
            ResponseTemplate::new(206)
                .insert_header(
                    "content-range",
                    format!("bytes {split}-{}/{}", BODY.len() - 1, BODY.len()).as_str(),
                )
                .set_body_bytes(&BODY[split..]),
        )
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("partial.bin");
    std::fs::write(&dest, &BODY[..split]).unwrap();

    let integrity = Integrity::sha1(BODY_SHA1).with_size(BODY.len() as u64);
    let written = fetcher()
        .to_file(&url(&server, "/resume"), &integrity, &dest)
        .await
        .unwrap();

    assert_eq!(written, BODY.len() as u64);
    assert_eq!(std::fs::read(&dest).unwrap(), BODY);
}

#[tokio::test]
async fn restarts_cleanly_when_the_server_ignores_the_range() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(BODY))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("partial.bin");
    std::fs::write(&dest, &BODY[..10]).unwrap();

    let integrity = Integrity::sha1(BODY_SHA1).with_size(BODY.len() as u64);
    let written = fetcher()
        .to_file(&url(&server, "/ignores-range"), &integrity, &dest)
        .await
        .unwrap();

    assert_eq!(written, BODY.len() as u64);
    assert_eq!(std::fs::read(&dest).unwrap(), BODY);
}

#[tokio::test]
async fn a_complete_file_is_not_resumed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(BODY))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("complete.bin");
    std::fs::write(&dest, BODY).unwrap();

    let integrity = Integrity::sha1(BODY_SHA1).with_size(BODY.len() as u64);
    fetcher()
        .to_file(&url(&server, "/complete"), &integrity, &dest)
        .await
        .unwrap();

    assert_eq!(std::fs::read(&dest).unwrap(), BODY);
}

#[tokio::test]
async fn streams_to_an_arbitrary_sink() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(BODY))
        .mount(&server)
        .await;

    let mut sink = Vec::new();
    let written = fetcher()
        .to_writer(
            &url(&server, "/stream"),
            &Integrity::sha1(BODY_SHA1),
            &mut sink,
        )
        .await
        .unwrap();

    assert_eq!(written, BODY.len() as u64);
    assert_eq!(sink, BODY);
}

#[tokio::test]
async fn fetches_metadata_into_memory() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/version_manifest_v2.json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"latest":{}}"#))
        .mount(&server)
        .await;

    let body = fetcher()
        .bytes(&url(&server, "/version_manifest_v2.json"))
        .await
        .unwrap();

    assert_eq!(body, br#"{"latest":{}}"#);
}
