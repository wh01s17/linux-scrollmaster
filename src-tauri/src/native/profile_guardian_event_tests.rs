// SPDX-License-Identifier: GPL-3.0-or-later
//! Persistent child scheduling and error sequencing, with a synthetic backend.
use super::*;
#[test]
fn independent_child_polls_between_requests_on_the_existing_backend() {
    let dir = Sandbox::new();
    let mut owner = dir.owner("idle-event");
    owner.snapshot(budget()).unwrap();
    let pid = owner.status()["guardian_pid"].clone();
    fs::write(dir.0.join("emit-idle"), b"yes").unwrap();
    wait_file(&dir.0.join("idle-consumed"));
    assert_eq!(
        owner.snapshot(budget()).unwrap()["synthetic_idle_reports"],
        1
    );
    assert_eq!(owner.status()["guardian_pid"], pid);
    owner.close(budget()).unwrap();
}
#[test]
fn an_idle_failure_is_reported_once_before_a_new_read_or_reconnection() {
    let dir = Sandbox::new();
    let mut owner = dir.owner("idle-failure");
    owner.snapshot(budget()).unwrap();
    let pid = owner.status()["guardian_pid"].clone();
    fs::write(dir.0.join("fail-idle"), b"yes").unwrap();
    wait_file(&dir.0.join("idle-failed"));
    assert!(HardwareLease::at(&dir.0).is_ok());
    let reads = dir.log().lines().filter(|s| *s == "snapshot").count();
    assert!(owner
        .snapshot(budget())
        .unwrap_err()
        .contains("synthetic invalid idle notification"));
    assert_eq!(
        dir.log().lines().filter(|s| *s == "snapshot").count(),
        reads
    );
    assert_eq!(owner.snapshot(budget()).unwrap()["id"], ID);
    assert_eq!(owner.status()["guardian_pid"], pid);
    owner.close(budget()).unwrap();
}
#[test]
fn idle_failure_invalidates_a_prepared_remote_write_without_replaying_it() {
    let dir = Sandbox::new();
    let mut owner = dir.owner("idle-failure");
    let deadline = budget();
    owner.prepare(&changes(1850), deadline).unwrap();
    fs::write(dir.0.join("fail-idle"), b"yes").unwrap();
    wait_file(&dir.0.join("idle-failed"));
    assert!(owner
        .apply(&changes(1850), deadline)
        .unwrap_err()
        .message
        .contains("Resident event stream failed"));
    wait_file(&dir.0.join("closed"));
    assert!(!dir.log().lines().any(|s| s == "apply"));
}
#[test]
fn a_busy_event_stream_never_starves_snapshot_or_shutdown() {
    let dir = Sandbox::new();
    let mut owner = dir.owner("idle-busy");
    owner.snapshot(budget()).unwrap();
    wait_file(&dir.0.join("idle-consumed"));
    assert!(
        owner.snapshot(budget()).unwrap()["synthetic_idle_reports"]
            .as_u64()
            .unwrap()
            > 0
    );
    owner.close(budget()).unwrap();
    assert!(HardwareLease::at(&dir.0).is_ok());
}
