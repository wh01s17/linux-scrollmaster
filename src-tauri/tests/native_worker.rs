// SPDX-License-Identifier: GPL-3.0-or-later
#![cfg(target_os = "linux")]
//! Real worker processes, exclusively local profile operations. Never snapshot
//! or numeric setters: the production connector must not open hardware in tests.
#[path = "support/native_worker_focus.rs"]
mod focus;
#[path = "support/native_worker_guardian.rs"]
mod guardian;
use linux_scrollmaster::{
    bridge::Worker,
    profiles::{Document, ProfileStore},
    worker::MAX_REQUEST_BYTES,
};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
const ID: &str = "11223344";
const EXE: &str = env!("CARGO_BIN_EXE_scrollmaster-native-worker");
struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "scrollmaster-worker-process-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
    fn store(&self) -> ProfileStore {
        ProfileStore::at(self.0.join("linux-scrollmaster")).unwrap()
    }
    fn seed(&self) {
        self.seed_armed(false);
    }
    fn seed_armed(&self, armed: bool) {
        let store = self.store();
        let document=Document::from_value(json!({"version":1,"profiles":[{"device":ID,"name":"Work","settings":{"dpi":1800}}],"active":if armed {json!({ID:"Work"})} else {json!({})},"keep":"metadata"})).unwrap();
        store.commit(&store.load().unwrap(), &document).unwrap();
    }
    fn command(&self) -> Command {
        let mut command = Command::new(EXE);
        command
            .env("XDG_CONFIG_HOME", &self.0)
            .env("XDG_RUNTIME_DIR", self.0.join("missing-runtime"));
        command
    }
    fn configured(&self) -> Command {
        let mut command = self.command();
        command.args(["--serve", "--slot", "2", "--device", ID]);
        command
    }
    fn bytes(&self) -> Vec<u8> {
        fs::read(self.0.join("linux-scrollmaster/profiles.json")).unwrap()
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn transact(w: &mut Worker, request: Value) -> Value {
    w.transact(&request, Duration::from_secs(5)).unwrap()
}
struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn raw(mut command: Command, input: &[u8]) -> (bool, Vec<Value>) {
    let mut child = ChildGuard(
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut stdout = child.0.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut output = Vec::new();
        stdout
            .by_ref()
            .take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut output)
            .unwrap();
        tx.send(output).unwrap();
    });
    // Test frames fit below 257 KiB and the child reads them immediately.
    // A separate supervisor test covers a child that does not read its pipe.
    let _ = child.0.stdin.take().unwrap().write_all(input);
    let end = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < end, "worker did not exit");
        std::thread::sleep(Duration::from_millis(5));
    };
    let output = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    reader.join().unwrap();
    let replies = output
        .split(|b| *b == b'\n')
        .filter(|b| !b.is_empty())
        .map(|b| serde_json::from_slice(b).unwrap())
        .collect();
    (status.success(), replies)
}
#[test]
fn supervisor_keeps_one_native_process_for_offline_profile_operations() {
    let dir = Sandbox::new();
    dir.seed();
    let mut worker = Worker::start(dir.configured()).unwrap();
    assert_eq!(
        transact(
            &mut worker,
            json!({"op":"rename","device":ID,"name":"Work","new_name":"Office"})
        )["ok"],
        true
    );
    assert_eq!(
        transact(
            &mut worker,
            json!({"op":"auto","device":ID,"name":"Office"})
        )["data"]["active"][ID],
        "Office"
    );
    assert_eq!(
        transact(
            &mut worker,
            json!({"op":"export","device":ID,"name":"Office"})
        )["data"]["profiles"][0]["name"],
        "Office"
    );
    assert_eq!(
        transact(&mut worker, json!({"op":"shutdown"}))["data"]["stopped"],
        true
    );
    drop(worker);
    assert!(!dir.0.join("missing-runtime").exists());
    assert_eq!(
        dir.store().load().unwrap().document().as_value()["keep"],
        "metadata"
    );
}
#[test]
fn production_owner_rejects_software_before_lease_and_keeps_profiles_armed() {
    let dir = Sandbox::new();
    // Publish setup once. A concurrent process spawn may briefly inherit another
    // thread's flock until exec closes CLOEXEC fds; Busy remains a valid result.
    dir.seed_armed(true);
    let original = dir.bytes();
    let mut worker = Worker::start(dir.configured()).unwrap();
    let response = transact(
        &mut worker,
        json!({"op":"remap","device":ID,"changes":{"83":"modifier:ctrl"}}),
    );
    assert_eq!(response["stage"], "preflight");
    assert!(response["error"].as_str().unwrap().contains("guardian"));
    assert_eq!(
        transact(
            &mut worker,
            json!({"op":"export","device":ID,"name":"Work"})
        )["ok"],
        true
    );
    transact(&mut worker, json!({"op":"shutdown"}));
    assert_eq!(dir.bytes(), original);
    assert!(!dir.0.join("missing-runtime").exists());
}
#[test]
fn unknown_fields_and_foreign_device_fail_without_effect_and_allow_next_request() {
    let dir = Sandbox::new();
    dir.seed();
    let original = dir.bytes();
    let mut worker = Worker::start(dir.configured()).unwrap();
    let reply = transact(
        &mut worker,
        json!({"op":"rename","device":ID,"name":"Work","new_name":"Changed","path":"/tmp/forbidden"}),
    );
    assert_eq!(reply["ok"], false);
    let reply = transact(
        &mut worker,
        json!({"op":"set","device":"55667788","changes":{"dpi":800}}),
    );
    assert_eq!(reply["stage"], "validation");
    assert_eq!(transact(&mut worker, json!({"op":"shutdown"}))["ok"], true);
    assert_eq!(dir.bytes(), original);
}
#[test]
fn eof_after_complete_local_request_exits_successfully() {
    let dir = Sandbox::new();
    dir.seed();
    let (success, replies) = raw(
        dir.configured(),
        b"{\"op\":\"export\",\"device\":\"11223344\",\"name\":\"Work\"}\n",
    );
    assert!(success);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["ok"], true);
}
#[test]
fn truncated_or_oversized_stream_exits_without_processing_following_requests() {
    for overflow in [false, true] {
        let dir = Sandbox::new();
        dir.seed();
        let original = dir.bytes();
        let input = if overflow {
            let mut bytes = vec![b' '; MAX_REQUEST_BYTES + 1];
            bytes.extend_from_slice(
                b"\n{\"op\":\"delete\",\"device\":\"11223344\",\"name\":\"Work\"}\n",
            );
            bytes
        } else {
            b"{\"op\":\"delete\",\"device\":\"11223344\",\"name\":\"Work\"}".to_vec()
        };
        let (success, replies) = raw(dir.configured(), &input);
        assert!(!success);
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0]["stage"], "framing");
        assert_eq!(dir.bytes(), original);
    }
}
#[test]
fn invalid_startup_has_no_stdout_and_creates_no_profile_files() {
    let dir = Sandbox::new();
    let mut command = dir.command();
    command.args(["--serve", "--slot", "2", "--device", "aabbccdd"]);
    let (success, replies) = raw(command, b"");
    assert!(!success);
    assert!(replies.is_empty());
    assert!(!dir.0.join("linux-scrollmaster").exists());
}
