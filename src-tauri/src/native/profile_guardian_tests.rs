// SPDX-License-Identifier: GPL-3.0-or-later
//! Real private processes, temporary leases/profiles and synthetic hardware only.
use super::super::{
    guardian::Channel, profile_guardian_server::serve, profile_guardian_wire::*, HardwareLease,
};
use super::*;
use crate::{
    profiles::{Document, ProfileStore},
    service::ProfileService,
};
use std::{
    fs,
    io::Write,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::Stdio,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
const ID: &str = "11223344";
const CHILD: &str = "native::profile_guardian::tests::fixture_child";
const CLIENT: &str = "native::profile_guardian::tests::client_child";
struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "scrollmaster-profile-guardian-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
    fn owner(&self, mode: &str) -> ProfileGuardianOwner {
        let mut owner = ProfileGuardianOwner::system(1, ID).unwrap();
        let (path, mode) = (self.0.clone(), mode.to_owned());
        owner.launcher = Box::new(move |_, _| Ok(command(&path, &mode, CHILD)));
        owner
    }
    fn log(&self) -> String {
        fs::read_to_string(self.0.join("log")).unwrap_or_default()
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn command(path: &Path, mode: &str, test: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", test, "--nocapture"])
        .env("SCROLLMASTER_PROFILE_GUARD_FIXTURE", mode)
        .env("SCROLLMASTER_PROFILE_GUARD_DIR", path);
    command
}
fn log(path: &Path, event: &str) {
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path.join("log"))
        .unwrap();
    writeln!(f, "{event}").unwrap();
}
fn wait_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        assert!(Instant::now() < deadline, "Missing {}", path.display());
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn budget() -> Instant {
    Instant::now() + Duration::from_secs(3)
}
fn changes(dpi: u16) -> Changes {
    Changes::new(Some(json!({"dpi":dpi})), None).unwrap()
}
struct Backend {
    path: PathBuf,
    mode: String,
    dpi: u16,
    lease: Option<HardwareLease>,
    prepared: Option<(Changes, Instant)>,
    reads: usize,
    idle_reports: usize,
    idle_failed: bool,
}
impl Backend {
    fn acquire(&mut self) {
        if self.lease.is_none() {
            self.lease = Some(HardwareLease::at(&self.path).unwrap());
            log(&self.path, "lease");
        }
    }
    fn description(&self) -> Value {
        json!({"id":ID,"settings":{"dpi":{"available":true,"value":self.dpi}},"buttons":{"controls":[],"error":null},"synthetic_idle_reports":self.idle_reports})
    }
}
impl Drop for Backend {
    fn drop(&mut self) {
        log(&self.path, "backend-drop");
        if self.mode == "hold-drop" {
            fs::write(self.path.join("closing"), b"yes").unwrap();
            wait_file(&self.path.join("release"));
        }
        self.lease.take();
        fs::write(self.path.join("closed"), b"yes").unwrap();
    }
}
impl ProfileBackend for Backend {
    fn close(&mut self, _: Instant) -> Result<(), String> {
        if self.mode == "cleanup-error" {
            Err("Synthetic output release unconfirmed".into())
        } else {
            Ok(())
        }
    }
    fn poll(&mut self, _: Duration) -> Result<bool, String> {
        if self.lease.is_none() || !self.mode.starts_with("idle-") {
            return Ok(false);
        }
        if self.mode == "idle-failure" && !self.idle_failed && self.path.join("fail-idle").exists()
        {
            self.idle_failed = true;
            self.prepared = None;
            self.lease.take();
            fs::write(self.path.join("idle-failed"), b"yes").unwrap();
            return Err("synthetic invalid idle notification".into());
        }
        if self.mode == "idle-busy"
            || (self.mode == "idle-event"
                && self.idle_reports == 0
                && self.path.join("emit-idle").exists())
        {
            self.idle_reports += 1;
            if self.idle_reports == 1 {
                fs::write(self.path.join("idle-consumed"), b"yes").unwrap();
            }
            return Ok(true);
        }
        Ok(false)
    }
    fn identity(&self) -> &str {
        ID
    }
    fn snapshot(&mut self, _: Instant) -> Result<Value, String> {
        self.acquire();
        self.prepared = None;
        self.reads += 1;
        log(&self.path, "snapshot");
        if self.mode == "snapshot-error-first" && self.reads == 1 {
            return Err("synthetic disconnect".into());
        }
        Ok(self.description())
    }
    fn prepare(&mut self, changes: &Changes, deadline: Instant) -> Result<(), String> {
        self.acquire();
        log(&self.path, "prepare");
        self.prepared = Some((changes.clone(), deadline));
        Ok(())
    }
    fn apply(&mut self, changes: &Changes, deadline: Instant) -> Result<Applied, ApplyError> {
        let (prepared, time) = self.prepared.take().unwrap();
        assert_eq!(time, deadline);
        assert_eq!(prepared.settings(), changes.settings());
        log(&self.path, "apply");
        if self.mode == "require-disarm" {
            let document: Value = serde_json::from_slice(
                &fs::read(self.path.join("linux-scrollmaster/profiles.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(document["active"], json!({}));
            log(&self.path, "apply-disarmed");
        }
        if self.mode == "late-apply" {
            std::thread::sleep(Duration::from_millis(250));
        }
        self.dpi = changes.settings().unwrap()["dpi"].as_u64().unwrap() as u16;
        if self.mode == "partial" {
            return Err(ApplyError {
                message: "synthetic partial write".into(),
                confirmed: vec!["dpi".into()],
                attempted: Some("button:83".into()),
            });
        }
        let mut snapshot = self.description();
        if self.mode == "wrong-receipt" {
            snapshot["settings"]["dpi"]["value"] = json!(999);
        }
        Ok(Applied {
            snapshot,
            confirmed: vec!["dpi".into()],
        })
    }
}
#[test]
fn fixture_child() {
    let Ok(mode) = std::env::var("SCROLLMASTER_PROFILE_GUARD_FIXTURE") else {
        return;
    };
    let path = PathBuf::from(std::env::var_os("SCROLLMASTER_PROFILE_GUARD_DIR").unwrap());
    fs::write(path.join("pid"), std::process::id().to_string()).unwrap();
    let backend = Backend {
        path: path.clone(),
        mode,
        dpi: 1800,
        lease: None,
        prepared: None,
        reads: 0,
        idle_reports: 0,
        idle_failed: false,
    };
    let result = serve(backend, Channel::inherited().unwrap());
    fs::write(path.join("result"), format!("{result:?}")).unwrap();
}

#[path = "profile_guardian_event_tests.rs"]
mod resident_tests;
#[test]
fn client_child() {
    if std::env::var("SCROLLMASTER_PROFILE_GUARD_FIXTURE").is_err() {
        return;
    }
    let dir = Sandbox(PathBuf::from(
        std::env::var_os("SCROLLMASTER_PROFILE_GUARD_DIR").unwrap(),
    ));
    let mut owner = dir.owner("normal");
    owner.snapshot(budget()).unwrap();
    fs::write(dir.0.join("client-ready"), b"yes").unwrap();
    // The test parent kills this fixture; the guardian is in its own session.
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}
#[test]
fn construction_and_unsupported_software_never_start_a_guardian_or_take_a_lease() {
    let dir = Sandbox::new();
    let mut owner = dir.owner("normal");
    assert_eq!(owner.status()["guardian_pid"], Value::Null);
    let software = Changes::new(None, Some(json!({"83":"modifier:ctrl"}))).unwrap();
    assert!(owner.prepare(&software, budget()).is_err());
    assert!(!dir.0.join("pid").exists());
    assert!(!dir.0.join("linux-scrollmaster-hardware.lock").exists());
}
#[test]
fn repeated_requests_use_one_process_and_one_use_preparation() {
    let dir = Sandbox::new();
    let mut owner = dir.owner("normal");
    assert_eq!(
        owner.snapshot(budget()).unwrap()["settings"]["dpi"]["value"],
        1800
    );
    let pid = owner.status()["guardian_pid"].clone();
    let deadline = budget();
    owner.prepare(&changes(1850), deadline).unwrap();
    assert_eq!(
        owner.apply(&changes(1850), deadline).unwrap().confirmed,
        vec!["dpi"]
    );
    assert_eq!(
        owner.snapshot(budget()).unwrap()["settings"]["dpi"]["value"],
        1850
    );
    assert_eq!(owner.status()["guardian_pid"], pid);
    assert!(owner.apply(&changes(1850), deadline).is_err());
    wait_file(&dir.0.join("closed"));
    assert_eq!(dir.log().lines().filter(|s| *s == "apply").count(), 1);
    assert!(HardwareLease::at(&dir.0).is_ok());
}
#[test]
fn changed_parameters_deadline_or_intervening_snapshot_invalidate_preparation() {
    for mode in [0, 1, 2] {
        let dir = Sandbox::new();
        let mut owner = dir.owner("normal");
        let deadline = budget();
        owner.prepare(&changes(1850), deadline).unwrap();
        if mode == 2 {
            owner.snapshot(budget()).unwrap();
        }
        let target = if mode == 0 {
            changes(1900)
        } else {
            changes(1850)
        };
        let apply_time = if mode == 1 {
            deadline - Duration::from_millis(1)
        } else {
            deadline
        };
        assert!(owner.apply(&target, apply_time).is_err());
        wait_file(&dir.0.join("closed"));
        assert!(!dir.log().lines().any(|s| s == "apply"));
    }
}
#[test]
fn business_read_error_keeps_process_and_allows_another_request() {
    let dir = Sandbox::new();
    let mut owner = dir.owner("snapshot-error-first");
    assert!(owner.snapshot(budget()).is_err());
    let pid = owner.status()["guardian_pid"].clone();
    assert_eq!(owner.snapshot(budget()).unwrap()["id"], ID);
    assert_eq!(owner.status()["guardian_pid"], pid);
    drop(owner);
    wait_file(&dir.0.join("closed"));
}
#[test]
fn partial_writes_preserve_evidence_without_retrying() {
    let dir = Sandbox::new();
    let mut owner = dir.owner("partial");
    let deadline = budget();
    owner.prepare(&changes(1850), deadline).unwrap();
    let error = owner.apply(&changes(1850), deadline).unwrap_err();
    assert_eq!(error.confirmed, vec!["dpi"]);
    assert_eq!(error.attempted.as_deref(), Some("button:83"));
    wait_file(&dir.0.join("closed"));
    assert_eq!(dir.log().lines().filter(|s| *s == "apply").count(), 1);
}
#[test]
fn capture_proxy_keeps_the_authoritative_child_after_a_structured_apply_error() {
    let dir = Sandbox::new();
    let mut owner = dir.owner("partial");
    owner.capture = true;
    let deadline = budget();
    owner.prepare(&changes(1850), deadline).unwrap();
    let pid = owner.status()["guardian_pid"].clone();
    let error = owner.apply(&changes(1850), deadline).unwrap_err();
    assert_eq!(error.confirmed, vec!["dpi"]);
    // The ticket was consumed: no repeat, but the same child answers next.
    assert!(owner.apply(&changes(1850), deadline).is_err());
    assert_eq!(owner.snapshot(budget()).unwrap()["id"], ID);
    assert_eq!(owner.status()["guardian_pid"], pid);
    assert_eq!(dir.log().lines().filter(|s| *s == "apply").count(), 1);
    owner.close(budget()).unwrap();
    wait_file(&dir.0.join("closed"));
}
#[test]
fn capture_proxy_treats_an_unknown_apply_outcome_as_terminal() {
    let dir = Sandbox::new();
    let mut owner = dir.owner("late-apply");
    owner.capture = true;
    let deadline = Instant::now() + Duration::from_millis(100);
    owner.prepare(&changes(1850), deadline).unwrap();
    assert!(owner
        .apply(&changes(1850), deadline)
        .unwrap_err()
        .message
        .contains("outcome uncertain"));
    wait_file(&dir.0.join("closed"));
    assert!(owner
        .snapshot(budget())
        .unwrap_err()
        .contains("unconfirmed"));
    assert_eq!(dir.log().lines().filter(|s| *s == "apply").count(), 1);
}
#[test]
fn incorrect_receipts_cannot_become_success() {
    let dir = Sandbox::new();
    let mut owner = dir.owner("wrong-receipt");
    let deadline = budget();
    owner.prepare(&changes(1850), deadline).unwrap();
    let error = owner.apply(&changes(1850), deadline).unwrap_err();
    assert!(error.message.contains("unconfirmed"));
    assert_eq!(error.confirmed, vec!["dpi"]);
    wait_file(&dir.0.join("closed"));
}
#[test]
fn timeout_is_uncertain_never_retried_and_does_not_kill_the_child() {
    let dir = Sandbox::new();
    let mut owner = dir.owner("late-apply");
    owner.snapshot(budget()).unwrap();
    let deadline = Instant::now() + Duration::from_millis(100);
    owner.prepare(&changes(1850), deadline).unwrap();
    let error = owner.apply(&changes(1850), deadline).unwrap_err();
    assert!(error.message.contains("outcome uncertain"));
    assert!(owner
        .snapshot(budget())
        .unwrap_err()
        .contains("still closing"));
    wait_file(&dir.0.join("closed"));
    assert_eq!(dir.log().lines().filter(|s| *s == "apply").count(), 1);
}
#[test]
fn disconnect_keeps_the_lease_until_child_cleanup_and_prevents_a_new_owner() {
    let dir = Sandbox::new();
    let mut owner = dir.owner("hold-drop");
    owner.snapshot(budget()).unwrap();
    owner.disconnect();
    wait_file(&dir.0.join("closing"));
    assert!(HardwareLease::at(&dir.0).is_err());
    assert!(owner
        .snapshot(budget())
        .unwrap_err()
        .contains("still closing"));
    fs::write(dir.0.join("release"), b"yes").unwrap();
    wait_file(&dir.0.join("closed"));
    assert!(HardwareLease::at(&dir.0).is_ok());
}
#[test]
fn killed_client_releases_only_ipc_and_independent_owner_closes_its_lease() {
    let dir = Sandbox::new();
    let mut client = command(&dir.0, "normal", CLIENT)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_file(&dir.0.join("client-ready"));
    assert!(HardwareLease::at(&dir.0).is_err());
    let guardian: u32 = fs::read_to_string(dir.0.join("pid"))
        .unwrap()
        .parse()
        .unwrap();
    assert_ne!(unsafe { libc::getsid(guardian as i32) }, unsafe {
        libc::getsid(client.id() as i32)
    });
    client.kill().unwrap();
    client.wait().unwrap();
    wait_file(&dir.0.join("closed"));
    assert!(HardwareLease::at(&dir.0).is_ok());
}
#[test]
fn framing_duplicate_fields_and_unknown_operations_close_before_backend_access() {
    for frame in [
        b"{\"op\":\"snapshot\",\"id\":1,\"id\":2,\"deadline_ns\":1}\n".to_vec(),
        b"{\"op\":\"exec\",\"id\":1}\n".to_vec(),
        b"{\"op\":\"snapshot\",\"id\":1,\"deadline_ns\":1,\"path\":\"x\"}\n".to_vec(),
        vec![b'x'; REQUEST_LIMIT + 1],
        b"{\"op\":\"snapshot\"".to_vec(),
    ] {
        let dir = Sandbox::new();
        let (mut child, mut socket) = spawn_channel(command(&dir.0, "normal", CHILD)).unwrap();
        socket.write_all(&frame).unwrap();
        socket.shutdown(std::net::Shutdown::Write).unwrap();
        assert!(child.wait().unwrap().success());
        wait_file(&dir.0.join("closed"));
        assert!(!dir.log().contains("lease"));
        assert!(!dir.log().contains("snapshot"));
        assert!(fs::read_to_string(dir.0.join("result"))
            .unwrap()
            .contains("Err"));
    }
}
#[test]
fn absolute_deadline_conversion_does_not_grant_a_new_budget() {
    let deadline = Instant::now() + Duration::from_millis(100);
    let wire = encode_deadline(deadline).unwrap();
    std::thread::sleep(Duration::from_millis(5));
    let decoded = decode_deadline(wire).unwrap();
    assert!(decoded <= deadline);
    assert!(decode_deadline(0).is_err());
    assert!(encode_deadline(Instant::now()).is_err());
    assert!(encode_deadline(Instant::now() + Duration::from_secs(31)).is_err());
}
#[test]
fn service_disarms_on_disk_before_remote_apply_and_keeps_one_guardian() {
    let dir = Sandbox::new();
    let store = ProfileStore::at(dir.0.join("linux-scrollmaster")).unwrap();
    let doc=Document::from_value(json!({"version":1,"profiles":[{"device":ID,"name":"Work","settings":{"dpi":1850}}],"active":{ID:"Work"}})).unwrap();
    store.commit(&store.load().unwrap(), &doc).unwrap();
    let mut service =
        ProfileService::new(store, dir.owner("require-disarm"), Duration::from_secs(3)).unwrap();
    service
        .handle(&json!({"op":"restore","device":ID,"name":"Work"}), None)
        .unwrap();
    assert!(dir.log().contains("apply-disarmed"));
    service
        .handle(&json!({"op":"snapshot","device":ID}), None)
        .unwrap();
    assert_eq!(dir.log().lines().filter(|s| *s == "apply").count(), 1);
    drop(service);
    wait_file(&dir.0.join("closed"));
}

fn rpc(channel: &mut Channel, request: Request) -> Reply {
    channel
        .send_bounded(
            &serde_json::to_value(request).unwrap(),
            REQUEST_LIMIT,
            budget(),
        )
        .unwrap();
    serde_json::from_slice(
        &channel
            .receive_bytes_bounded(REPLY_LIMIT, budget())
            .unwrap(),
    )
    .unwrap()
}
fn raw_guardian(dir: &Sandbox) -> (Child, Channel) {
    let (child, socket) = spawn_channel(command(&dir.0, "normal", CHILD)).unwrap();
    let mut channel = Channel::new(socket).unwrap();
    assert!(matches!(
        serde_json::from_slice::<Reply>(
            &channel
                .receive_bytes_bounded(REPLY_LIMIT, budget())
                .unwrap()
        )
        .unwrap(),
        Reply::Ready { .. }
    ));
    (child, channel)
}
#[test]
fn private_rpc_consumes_tickets_once_and_drops_backend_before_shutdown_ack() {
    let dir = Sandbox::new();
    let (mut child, mut channel) = raw_guardian(&dir);
    let deadline_ns = encode_deadline(budget()).unwrap();
    assert!(matches!(
        rpc(
            &mut channel,
            Request::Prepare {
                id: 1,
                deadline_ns,
                settings: Some(json!({"dpi":1850})),
                buttons: None
            }
        ),
        Reply::Prepared { ticket: 1, .. }
    ));
    assert!(matches!(
        rpc(&mut channel, Request::Apply { id: 2, ticket: 1 }),
        Reply::Applied { .. }
    ));
    assert!(matches!(
        rpc(&mut channel, Request::Apply { id: 3, ticket: 1 }),
        Reply::Error { .. }
    ));
    assert!(matches!(
        rpc(&mut channel, Request::Shutdown { id: 4 }),
        Reply::Stopped { .. }
    ));
    assert!(dir.0.join("closed").exists());
    assert!(HardwareLease::at(&dir.0).is_ok());
    child.wait().unwrap();
    assert_eq!(dir.log().lines().filter(|s| *s == "apply").count(), 1);
}
#[test]
fn private_rpc_wrong_ticket_or_snapshot_cannot_reuse_a_preparation() {
    for snapshot in [false, true] {
        let dir = Sandbox::new();
        let (mut child, mut channel) = raw_guardian(&dir);
        let deadline_ns = encode_deadline(budget()).unwrap();
        rpc(
            &mut channel,
            Request::Prepare {
                id: 1,
                deadline_ns,
                settings: Some(json!({"dpi":1850})),
                buttons: None,
            },
        );
        if snapshot {
            rpc(&mut channel, Request::Snapshot { id: 2, deadline_ns });
        } else {
            assert!(matches!(
                rpc(&mut channel, Request::Apply { id: 2, ticket: 999 }),
                Reply::Error { .. }
            ));
        }
        assert!(matches!(
            rpc(&mut channel, Request::Apply { id: 3, ticket: 1 }),
            Reply::Error { .. }
        ));
        rpc(&mut channel, Request::Shutdown { id: 4 });
        child.wait().unwrap();
        assert!(!dir.log().lines().any(|s| s == "apply"));
    }
}
#[test]
fn private_rpc_rejects_expired_preparation_before_any_apply() {
    let dir = Sandbox::new();
    let (mut child, mut channel) = raw_guardian(&dir);
    let deadline_ns = encode_deadline(Instant::now() + Duration::from_millis(300)).unwrap();
    rpc(
        &mut channel,
        Request::Prepare {
            id: 1,
            deadline_ns,
            settings: Some(json!({"dpi":1850})),
            buttons: None,
        },
    );
    std::thread::sleep(Duration::from_millis(350));
    assert!(matches!(
        rpc(&mut channel, Request::Apply { id: 2, ticket: 1 }),
        Reply::Error { .. }
    ));
    rpc(&mut channel, Request::Shutdown { id: 3 });
    child.wait().unwrap();
    assert!(!dir.log().lines().any(|s| s == "apply"));
}
#[test]
fn private_rpc_rechecks_software_policy_before_acquisition() {
    let dir = Sandbox::new();
    let (mut child, mut channel) = raw_guardian(&dir);
    assert!(matches!(
        rpc(
            &mut channel,
            Request::Prepare {
                id: 1,
                deadline_ns: encode_deadline(budget()).unwrap(),
                settings: Some(json!({"dpi":1850})),
                buttons: Some(json!({"83":"modifier:ctrl"}))
            }
        ),
        Reply::Error { .. }
    ));
    rpc(&mut channel, Request::Shutdown { id: 2 });
    child.wait().unwrap();
    assert!(!dir.log().contains("lease"));
}
#[test]
fn graceful_close_confirms_lease_release_and_never_starts_an_idle_child() {
    let dir = Sandbox::new();
    let mut owner = dir.owner("normal");
    owner.close(budget()).unwrap();
    assert!(!dir.0.join("pid").exists());
    owner.snapshot(budget()).unwrap();
    owner.close(budget()).unwrap();
    assert!(HardwareLease::at(&dir.0).is_ok());
    assert_eq!(owner.status()["guardian_pid"], Value::Null);
    assert_eq!(owner.status()["closing_pid"], Value::Null);
}
#[test]
fn shutdown_cleanup_failure_is_sticky_and_never_becomes_stopped_or_reconnects() {
    let dir = Sandbox::new();
    let mut owner = dir.owner("cleanup-error");
    owner.snapshot(budget()).unwrap();
    assert!(owner
        .close(budget())
        .unwrap_err()
        .contains("release unconfirmed"));
    wait_file(&dir.0.join("closed"));
    assert!(owner.close(budget()).is_err());
    assert!(owner.snapshot(budget()).is_err());
    assert_eq!(dir.log().matches("lease").count(), 1);
}
#[test]
fn shutdown_timeout_is_reported_without_killing_cleanup() {
    let dir = Sandbox::new();
    let mut owner = dir.owner("hold-drop");
    owner.snapshot(budget()).unwrap();
    let error = owner
        .close(Instant::now() + Duration::from_millis(30))
        .unwrap_err();
    assert!(error.contains("cleanup may still be running"));
    wait_file(&dir.0.join("closing"));
    assert!(HardwareLease::at(&dir.0).is_err());
    fs::write(dir.0.join("release"), b"yes").unwrap();
    owner.close(budget()).unwrap();
    assert!(HardwareLease::at(&dir.0).is_ok());
}

#[test]
fn worker_never_acknowledges_stopped_while_guardian_cleanup_is_unconfirmed() {
    let dir = Sandbox::new();
    let mut owner = dir.owner("hold-drop");
    owner.snapshot(budget()).unwrap();
    let store = ProfileStore::at(dir.0.join("linux-scrollmaster")).unwrap();
    let service = ProfileService::new(store, owner, Duration::from_millis(30)).unwrap();
    let mut output = Vec::new();
    crate::worker::serve(
        service,
        std::io::Cursor::new(b"{\"op\":\"shutdown\"}\n"),
        &mut output,
    )
    .unwrap();
    let reply: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["stage"], "shutdown");
    assert_ne!(reply["data"]["stopped"], true);
    wait_file(&dir.0.join("closing"));
    assert!(HardwareLease::at(&dir.0).is_err());
    fs::write(dir.0.join("release"), b"yes").unwrap();
    wait_file(&dir.0.join("closed"));
}
