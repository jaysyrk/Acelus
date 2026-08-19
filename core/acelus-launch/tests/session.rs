#![cfg(unix)]

use std::path::PathBuf;
use std::time::Duration;

use acelus_launch::invocation::Invocation;
use acelus_launch::session::{Session, Stream, DEFAULT_LOG_CAPACITY};

fn shell(script: &str, working_directory: &std::path::Path) -> Invocation {
    Invocation {
        program: PathBuf::from("/bin/sh"),
        arguments: vec!["-c".into(), script.into()],
        working_directory: working_directory.to_path_buf(),
    }
}

#[tokio::test]
async fn a_session_captures_both_output_streams() {
    let dir = tempfile::tempdir().unwrap();
    let invocation = shell("echo to stdout; echo to stderr >&2", dir.path());

    let mut session = Session::spawn(&invocation, DEFAULT_LOG_CAPACITY)
        .await
        .unwrap();
    let report = session.wait().await.unwrap();

    assert_eq!(report.code, Some(0));
    assert!(!report.crashed);

    tokio::time::sleep(Duration::from_millis(100)).await;
    let lines = session.log().snapshot();

    assert!(lines
        .iter()
        .any(|line| line.stream == Stream::Stdout && line.text == "to stdout"));
    assert!(lines
        .iter()
        .any(|line| line.stream == Stream::Stderr && line.text == "to stderr"));
}

#[tokio::test]
async fn a_non_zero_exit_is_reported_as_a_crash() {
    let dir = tempfile::tempdir().unwrap();
    let invocation = shell("exit 3", dir.path());

    let mut session = Session::spawn(&invocation, DEFAULT_LOG_CAPACITY)
        .await
        .unwrap();
    let report = session.wait().await.unwrap();

    assert_eq!(report.code, Some(3));
    assert!(report.crashed);
    assert_eq!(report.signal, None);
}

#[tokio::test]
async fn a_running_session_can_be_terminated() {
    let dir = tempfile::tempdir().unwrap();
    let invocation = shell("sleep 60", dir.path());

    let mut session = Session::spawn(&invocation, DEFAULT_LOG_CAPACITY)
        .await
        .unwrap();
    assert!(session.is_guarded());
    assert!(session.pid() > 0);

    session.terminate().unwrap();

    let report = tokio::time::timeout(Duration::from_secs(5), session.wait())
        .await
        .expect("a terminated session must not hang")
        .unwrap();

    assert!(report.crashed, "a terminated game did not exit cleanly");
}

#[tokio::test]
async fn the_working_directory_is_created_and_used() {
    let dir = tempfile::tempdir().unwrap();
    let game_dir = dir.path().join("instance/minecraft");
    let invocation = shell("pwd", &game_dir);

    let mut session = Session::spawn(&invocation, DEFAULT_LOG_CAPACITY)
        .await
        .unwrap();
    session.wait().await.unwrap();

    assert!(game_dir.is_dir(), "the game directory should be created");

    tokio::time::sleep(Duration::from_millis(100)).await;
    let lines = session.log().snapshot();
    let reported = lines
        .iter()
        .find(|line| line.stream == Stream::Stdout)
        .expect("the shell reported its working directory");

    assert!(
        reported.text.ends_with("instance/minecraft"),
        "the game ran in {} rather than its own directory",
        reported.text
    );
}

#[tokio::test]
async fn output_beyond_the_buffer_capacity_keeps_the_most_recent_lines() {
    let dir = tempfile::tempdir().unwrap();
    let invocation = shell(
        "for n in 1 2 3 4 5 6 7 8; do echo line $n; done",
        dir.path(),
    );

    let mut session = Session::spawn(&invocation, 3).await.unwrap();
    session.wait().await.unwrap();

    tokio::time::sleep(Duration::from_millis(150)).await;
    let lines = session.log().snapshot();

    assert!(lines.len() <= 3, "the ring buffer should bound its size");
    assert_eq!(
        lines.last().map(|line| line.text.as_str()),
        Some("line 8"),
        "the newest line should survive"
    );
}

#[tokio::test]
async fn spawning_a_missing_program_reports_the_program_that_failed() {
    let dir = tempfile::tempdir().unwrap();
    let invocation = Invocation {
        program: PathBuf::from("/nonexistent/java"),
        arguments: Vec::new(),
        working_directory: dir.path().to_path_buf(),
    };

    let error = Session::spawn(&invocation, DEFAULT_LOG_CAPACITY)
        .await
        .unwrap_err();

    assert!(
        format!("{error}").contains("/nonexistent/java"),
        "the error should name the program, got {error}"
    );
}
