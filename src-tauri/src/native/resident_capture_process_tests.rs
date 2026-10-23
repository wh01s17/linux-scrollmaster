// SPDX-License-Identifier: GPL-3.0-or-later
//! Independent real processes with synthetic transport/output and temporary lease.
use super::*;
use crate::native::{
    guardian::{spawn_channel, Channel},
    profile_guardian_server::serve,
    profile_guardian_wire::*,
    HardwareLease,
};
use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};
const CHILD: &str =
    "native::buttons_tests::activation_tests::resident_capture_tests::process_tests::capture_child";
const CLIENT:&str="native::buttons_tests::activation_tests::resident_capture_tests::process_tests::capture_client";
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "resident-capture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
        Self(p)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn command(dir: &Path, test: &str, mode: &str) -> Command {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", test, "--nocapture"])
        .env("SCROLLMASTER_RESIDENT_CAPTURE_DIR", dir)
        .env("SCROLLMASTER_RESIDENT_CAPTURE_MODE", mode)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd
}
fn limit() -> Instant {
    Instant::now() + Duration::from_secs(4)
}
fn wait_file(p: &Path) {
    let end = limit();
    while !p.exists() {
        assert!(Instant::now() < end, "Missing {}", p.display());
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn read(channel: &mut Channel) -> Value {
    serde_json::from_slice(&channel.receive_bytes_bounded(REPLY_LIMIT, limit()).unwrap()).unwrap()
}
fn rpc(channel: &mut Channel, req: Value) -> Value {
    channel.send_bounded(&req, REQUEST_LIMIT, limit()).unwrap();
    read(channel)
}
fn start(dir: &Path, mode: &str) -> (Child, Channel) {
    let (child, socket) = spawn_channel(command(dir, CHILD, mode)).unwrap();
    let mut channel = Channel::new(socket).unwrap();
    assert_eq!(read(&mut channel)["result"], "ready");
    (child, channel)
}
fn activate(channel: &mut Channel) {
    let prepared = rpc(
        channel,
        json!({"op":"prepare","id":1,"deadline_ns":encode_deadline(limit()).unwrap(),"buttons":{"83":"modifier:ctrl"}}),
    );
    assert_eq!(prepared["result"], "prepared", "{prepared}");
    let applied = rpc(channel, json!({"op":"apply","id":2,"ticket":1}));
    assert_eq!(applied["result"], "applied", "{applied}");
    assert_eq!(
        applied["snapshot"]["native_events"]["actions_enabled"],
        true
    );
}
struct Backend {
    fixture: Fixture,
    _lease: HardwareLease,
    path: PathBuf,
    emitted: bool,
}
impl Backend {
    fn record(&self, result: Result<(), String>) {
        let log = self.fixture.log.borrow().join("\n");
        fs::write(self.path.join("log"), log).unwrap();
        fs::write(self.path.join("result"),serde_json::to_vec(&json!({"ok":result.is_ok(),"error":result.err(),"flags":self.fixture.state.borrow().flags})).unwrap()).unwrap();
    }
}
impl Drop for Backend {
    fn drop(&mut self) {
        let r = self.fixture.backend.close(limit());
        self.record(r);
    }
}
impl ProfileBackend for Backend {
    fn validate_changes(&self, _: &Changes) -> Result<(), String> {
        Ok(())
    }
    fn identity(&self) -> &str {
        self.fixture.backend.identity()
    }
    fn snapshot(&mut self, d: Instant) -> Result<Value, String> {
        self.fixture.backend.snapshot(d)
    }
    fn prepare(&mut self, c: &Changes, d: Instant) -> Result<(), String> {
        self.fixture.backend.prepare(c, d)
    }
    fn apply(
        &mut self,
        c: &Changes,
        d: Instant,
    ) -> Result<crate::service::Applied, crate::native::ApplyError> {
        self.fixture.backend.apply(c, d)
    }
    fn poll(&mut self, t: Duration) -> Result<bool, String> {
        if !self.emitted && self.path.join("emit").exists() {
            self.emitted = true;
            self.fixture
                .idle
                .borrow_mut()
                .extend([notification(&[]), notification(&[83])]);
        }
        let r = self.fixture.backend.poll(t);
        if self.fixture.log.borrow().iter().any(|s| s == "held:Ctrl") {
            fs::write(self.path.join("held"), b"yes").unwrap();
        }
        r
    }
    fn close(&mut self, d: Instant) -> Result<(), String> {
        assert!(
            HardwareLease::at(&self.path).is_err(),
            "lease must cover output/recovery"
        );
        let r = self.fixture.backend.close(d);
        self.record(r.clone());
        assert!(HardwareLease::at(&self.path).is_err());
        r
    }
}
#[test]
fn capture_child() {
    let Ok(dir) = std::env::var("SCROLLMASTER_RESIDENT_CAPTURE_DIR") else {
        return;
    };
    let path = PathBuf::from(dir);
    let channel = Channel::inherited().unwrap();
    let f = Fixture::new(Faults::default());
    if std::env::var("SCROLLMASTER_RESIDENT_CAPTURE_MODE").unwrap() == "release-error" {
        f.fail_close.set(true);
    }
    let lease = HardwareLease::at(&path).unwrap();
    let backend = Backend {
        fixture: f,
        _lease: lease,
        path,
        emitted: false,
    };
    let _ = serve(backend, channel);
}
#[test]
fn capture_client() {
    let Ok(dir) = std::env::var("SCROLLMASTER_RESIDENT_CAPTURE_DIR") else {
        return;
    };
    let path = PathBuf::from(dir);
    let (_child, mut channel) = start(&path, "normal");
    activate(&mut channel);
    fs::write(path.join("emit"), b"yes").unwrap();
    wait_file(&path.join("held"));
    fs::write(path.join("client-ready"), b"yes").unwrap();
    let end = Instant::now() + Duration::from_secs(15);
    while Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
}
fn assert_restored(dir: &Directory) {
    wait_file(&dir.0.join("result"));
    let result: Value = serde_json::from_slice(&fs::read(dir.0.join("result")).unwrap()).unwrap();
    assert_eq!(result["flags"]["83"], 0, "{result}");
    let log = fs::read_to_string(dir.0.join("log")).unwrap();
    assert!(log.find("output-close").unwrap() < log.find("wire:83:2").unwrap());
    assert!(log.find("wire:83:2").unwrap() < log.find("descriptor-close").unwrap());
    let end = limit();
    loop {
        if HardwareLease::at(&dir.0).is_ok() {
            break;
        }
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn resident_actions_and_shutdown_complete_before_the_lease_is_released() {
    let dir = Directory::new();
    let (mut child, mut channel) = start(&dir.0, "normal");
    activate(&mut channel);
    fs::write(dir.0.join("emit"), b"yes").unwrap();
    wait_file(&dir.0.join("held"));
    assert_eq!(
        rpc(&mut channel, json!({"op":"shutdown","id":3}))["result"],
        "stopped"
    );
    assert!(child.wait().unwrap().success());
    assert_restored(&dir);
}
#[test]
fn eof_recovers_a_live_capture_without_an_explicit_shutdown() {
    let dir = Directory::new();
    let (mut child, mut channel) = start(&dir.0, "normal");
    activate(&mut channel);
    drop(channel);
    assert!(child.wait().unwrap().success());
    assert_restored(&dir);
}
#[test]
fn release_failure_returns_error_instead_of_stopped_but_still_restores_mouse() {
    let dir = Directory::new();
    let (mut child, mut channel) = start(&dir.0, "release-error");
    activate(&mut channel);
    let reply = rpc(&mut channel, json!({"op":"shutdown","id":3}));
    assert_eq!(reply["result"], "error");
    assert!(reply["error"]
        .as_str()
        .unwrap()
        .contains("release unconfirmed"));
    assert!(child.wait().unwrap().success());
    assert_restored(&dir);
}
#[test]
fn sigkill_of_the_client_leaves_an_independent_owner_that_restores_held_capture() {
    let dir = Directory::new();
    let mut client = command(&dir.0, CLIENT, "normal").spawn().unwrap();
    wait_file(&dir.0.join("client-ready"));
    assert!(HardwareLease::at(&dir.0).is_err());
    client.kill().unwrap();
    client.wait().unwrap();
    assert_restored(&dir);
}
