#![cfg(unix)]

use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use acelus_native_sys::ProcessGuard;

fn spawn_guarded_sleeper() -> Child {
    let mut command = Command::new("sleep");
    command.arg("60");

    unsafe {
        command.pre_exec(|| {
            ProcessGuard::prepare_child().map_err(io::Error::other)?;
            Ok(())
        });
    }

    command.spawn().expect("sleep is available")
}

fn wait_for_exit(child: &mut Child, within: Duration) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        match child.try_wait().expect("the child can be polled") {
            Some(status) => return Some(status),
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    None
}

#[test]
fn a_guarded_child_dies_when_the_guard_is_dropped() {
    let mut child = spawn_guarded_sleeper();

    let mut guard = ProcessGuard::new().expect("a guard is created");
    guard.adopt(child.id()).expect("the child is adopted");
    assert!(guard.is_guarding());

    drop(guard);

    let status = wait_for_exit(&mut child, Duration::from_secs(5))
        .expect("the child must not outlive the guard that adopted it");
    assert!(
        !status.success(),
        "the child should have been signalled, not exited cleanly"
    );
}

#[test]
fn a_guarded_child_keeps_running_while_the_guard_is_alive() {
    let mut child = spawn_guarded_sleeper();

    let mut guard = ProcessGuard::new().unwrap();
    guard.adopt(child.id()).unwrap();

    assert!(
        wait_for_exit(&mut child, Duration::from_millis(300)).is_none(),
        "the child was killed while its guard was still alive"
    );

    guard.terminate().expect("terminate succeeds");
    wait_for_exit(&mut child, Duration::from_secs(5)).expect("terminate stops the child");
}

#[test]
fn terminating_twice_is_not_an_error() {
    let mut child = spawn_guarded_sleeper();

    let mut guard = ProcessGuard::new().unwrap();
    guard.adopt(child.id()).unwrap();

    guard.terminate().expect("the first terminate succeeds");
    wait_for_exit(&mut child, Duration::from_secs(5)).expect("the child stops");

    guard
        .terminate()
        .expect("terminating an already dead group is not an error");
}

#[cfg(target_os = "linux")]
#[test]
fn a_child_is_placed_in_its_own_process_group() {
    let mut child = spawn_guarded_sleeper();
    let pid = child.id();
    let group = process_group_of(pid);

    let mut guard = ProcessGuard::new().unwrap();
    guard.adopt(pid).unwrap();
    guard.terminate().ok();
    wait_for_exit(&mut child, Duration::from_secs(5));

    assert_eq!(
        group,
        Some(pid),
        "prepare_child should have made the child a process group leader"
    );
}

#[cfg(target_os = "linux")]
fn process_group_of(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after_comm = stat.rsplit_once(") ")?.1;
    after_comm.split_whitespace().nth(2)?.parse().ok()
}
