// SPDX-License-Identifier: GPL-3.0-or-later
//! Worker framing/lifecycle with a temporary store and synthetic backend only.
#[path = "worker_focus_tests.rs"]
mod focus;
use super::*;
use crate::{
    native::ApplyError,
    profiles::Document,
    service::{Applied, Changes},
};
use std::io;
use std::{
    cell::RefCell,
    fs,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

const ID: &str = "11223344";
#[derive(Default)]
struct State {
    calls: Vec<&'static str>,
    dropped: bool,
    dpi: u16,
    fail_apply: bool,
}
struct Backend(Rc<RefCell<State>>);
impl Drop for Backend {
    fn drop(&mut self) {
        self.0.borrow_mut().dropped = true;
    }
}
impl ProfileBackend for Backend {
    fn identity(&self) -> &str {
        ID
    }
    fn snapshot(&mut self, _: Instant) -> Result<Value, String> {
        let mut s = self.0.borrow_mut();
        s.calls.push("snapshot");
        Ok(json!({"id":ID,"settings":{"dpi":{"available":true,"value":s.dpi}}}))
    }
    fn prepare(&mut self, _: &Changes, _: Instant) -> Result<(), String> {
        self.0.borrow_mut().calls.push("prepare");
        Ok(())
    }
    fn apply(&mut self, changes: &Changes, _: Instant) -> Result<Applied, ApplyError> {
        let mut s = self.0.borrow_mut();
        s.calls.push("apply");
        s.dpi = changes.settings().unwrap()["dpi"].as_u64().unwrap() as u16;
        if s.fail_apply {
            return Err(ApplyError {
                message: "Synthetic uncertain write".into(),
                confirmed: vec!["dpi".into()],
                attempted: Some("button:83".into()),
            });
        }
        Ok(Applied {
            confirmed: vec!["dpi".into()],
            snapshot: json!({"id":ID,"settings":{"dpi":{"available":true,"value":s.dpi}}}),
        })
    }
}
struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "scrollmaster-worker-unit-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
    fn store(&self) -> ProfileStore {
        ProfileStore::at(self.0.join("linux-scrollmaster")).unwrap()
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn service() -> (Sandbox, ProfileService<Backend>, Rc<RefCell<State>>) {
    let dir = Sandbox::new();
    let store = dir.store();
    let document = Document::from_value(json!({"version":1,"profiles":[{"device":ID,"name":"Work","settings":{"dpi":1800}}],"active":{}})).unwrap();
    store.commit(&store.load().unwrap(), &document).unwrap();
    let state = Rc::new(RefCell::new(State {
        dpi: 1850,
        ..State::default()
    }));
    let service =
        ProfileService::new(store, Backend(state.clone()), Duration::from_secs(1)).unwrap();
    (dir, service, state)
}
fn replies(bytes: &[u8]) -> Vec<Value> {
    bytes
        .split(|b| *b == b'\n')
        .filter(|b| !b.is_empty())
        .map(|b| serde_json::from_slice(b).unwrap())
        .collect()
}
fn frame(request: Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(&request).unwrap();
    bytes.push(b'\n');
    bytes
}

#[test]
fn startup_requires_explicit_mode_slot_and_uppercase_unit_identity() {
    for args in [
        vec!["--serve", "--slot", "2", "--device", ID],
        vec!["--serve", "--device", ID, "--slot", "2"],
    ] {
        let config =
            WorkerConfig::parse(&args.into_iter().map(String::from).collect::<Vec<_>>()).unwrap();
        assert_eq!((config.slot(), config.identity()), (2, ID));
    }
    for args in [
        vec![],
        vec!["--serve"],
        vec!["--serve", "--slot", "2", "--slot", "2"],
        vec!["--serve", "--slot", "0", "--device", ID],
        vec!["--serve", "--slot", "02", "--device", ID],
        vec!["--serve", "--slot", "2", "--device", "aabbccdd"],
        vec!["--serve", "--path", "/tmp/x", "--device", ID],
        vec!["--serve", "--slot", "2", "--device", ID, "--fallback"],
    ] {
        assert!(
            WorkerConfig::parse(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_err()
        );
    }
}
#[test]
fn capture_is_a_separate_explicit_startup_mode() {
    let args = ["--serve-guarded-capture", "--slot", "2", "--device", ID].map(String::from);
    let capture = WorkerConfig::parse(&args).unwrap();
    assert_eq!(capture.serve_flag(), "--serve-guarded-capture");
    assert_eq!(WorkerConfig::new(2, ID).unwrap().serve_flag(), "--serve");
    assert_eq!(
        WorkerConfig::new(2, ID)
            .unwrap()
            .with_guardian()
            .serve_flag(),
        "--serve-guarded"
    );
    assert_eq!(
        WorkerConfig::new(2, ID)
            .unwrap()
            .with_capture()
            .serve_flag(),
        "--serve-guarded-capture"
    );
}
#[test]
fn malformed_and_unknown_requests_do_not_probe_or_mutate_and_stream_recovers() {
    let (_dir, service, state) = service();
    let mut input = Vec::new();
    for bytes in [
        b"[]".as_slice(),
        b"null",
        b"{}",
        b"{broken",
        b"\xff",
        br#"{"op":"snapshot","op":"set"}"#,
        br#"{"op":"set","device":"11223344","device":"55667788","changes":{"dpi":800}}"#,
        br#"{"op":"snapshot","application":"injected-focus"}"#,
        br#"{"op":"snapshot","path":"/tmp/x"}"#,
        br#"{"op":"applications","path":"/tmp/x"}"#,
        br#"{"op":"exec","command":"anything"}"#,
        br#"{"op":"set","device":"11223344"}"#,
        br#"{"op":"shutdown","device":"11223344"}"#,
        br#"{"op":"snapshot"} {"op":"shutdown"}"#,
    ] {
        input.extend_from_slice(bytes);
        input.push(b'\n');
    }
    input.extend(frame(json!({"op":"shutdown"})));
    let mut output = Vec::new();
    serve(service, input.as_slice(), &mut output).unwrap();
    let results = replies(&output);
    assert_eq!(results.len(), 15);
    assert!(results[..14]
        .iter()
        .all(|r| r["ok"] == false && r["stage"] == "validation"));
    assert_eq!(results[14]["data"]["stopped"], true);
    assert!(state.borrow().calls.is_empty());
    assert!(state.borrow().dropped);
}
#[test]
fn serial_operations_keep_profile_changes_and_do_not_process_past_shutdown() {
    let (dir, service, state) = service();
    let mut input = Vec::new();
    for request in [
        json!({"op":"rename","device":ID,"name":"Work","new_name":"Office"}),
        json!({"op":"export","device":ID,"name":"Office"}),
        json!({"op":"set","device":ID,"changes":{"dpi":2400}}),
        json!({"op":"snapshot"}),
        json!({"op":"shutdown"}),
        json!({"op":"set","device":ID,"changes":{"dpi":800}}),
    ] {
        input.extend(frame(request));
    }
    let mut output = Vec::new();
    serve(service, input.as_slice(), &mut output).unwrap();
    let results = replies(&output);
    assert_eq!(results.len(), 5);
    assert!(results.iter().all(|r| r["ok"] == true));
    assert_eq!(results[1]["data"]["profiles"][0]["name"], "Office");
    assert_eq!(
        results[3]["data"]["devices"][0]["settings"]["dpi"]["value"],
        2400
    );
    assert_eq!(state.borrow().calls, vec!["prepare", "apply", "snapshot"]);
    assert!(state.borrow().dropped);
    assert_eq!(
        dir.store().load().unwrap().document().profiles()[0]["name"],
        "Office"
    );
}
#[test]
fn partial_apply_error_preserves_history_disarming_and_next_read() {
    let (dir, service, state) = service();
    state.borrow_mut().fail_apply = true;
    dir.store()
        .manage(&json!({"op":"auto","device":ID,"name":"Work"}))
        .unwrap();
    let mut input = frame(json!({"op":"set","device":ID,"changes":{"dpi":2400}}));
    input.extend(frame(json!({"op":"snapshot"})));
    let mut output = Vec::new();
    serve(service, input.as_slice(), &mut output).unwrap();
    let results = replies(&output);
    assert_eq!(results[0]["ok"], false);
    assert_eq!(results[0]["stage"], "apply");
    assert_eq!(results[0]["confirmed"], json!(["dpi"]));
    assert_eq!(results[0]["attempted"], "button:83");
    assert_eq!(
        results[1]["data"]["devices"][0]["settings"]["dpi"]["value"],
        2400
    );
    assert!(dir.store().load().unwrap().document().as_value()["active"]
        .get(ID)
        .is_none());
    assert_eq!(state.borrow().calls, vec!["prepare", "apply", "snapshot"]);
}
#[test]
fn corrupt_store_stops_service_before_hardware_without_repair() {
    let (dir, service, state) = service();
    let path = dir.0.join("linux-scrollmaster/profiles.json");
    fs::write(&path, b"broken").unwrap();
    let mut output = Vec::new();
    serve(
        service,
        frame(json!({"op":"set","device":ID,"changes":{"dpi":800}})).as_slice(),
        &mut output,
    )
    .unwrap();
    assert_eq!(replies(&output)[0]["stage"], "persistence");
    assert!(state.borrow().calls.is_empty());
    assert_eq!(fs::read(path).unwrap(), b"broken");
}
#[test]
fn eof_and_unterminated_frames_close_without_applying() {
    for bytes in [
        b"".as_slice(),
        br#"{"op":"set","device":"11223344","changes":{"dpi":800}}"#,
    ] {
        let (_dir, service, state) = service();
        let mut output = Vec::new();
        let result = serve(service, bytes, &mut output);
        assert_eq!(result.is_ok(), bytes.is_empty());
        assert!(state.borrow().calls.is_empty());
        assert!(state.borrow().dropped);
        if !bytes.is_empty() {
            assert_eq!(replies(&output)[0]["stage"], "framing");
        }
    }
}
#[test]
fn exact_frame_limit_is_accepted_but_overflow_is_terminal() {
    for extra in [0, 1] {
        let (_dir, service, state) = service();
        let mut bytes = serde_json::to_vec(&json!({"op":"shutdown"})).unwrap();
        bytes.resize(MAX_REQUEST_BYTES + extra, b' ');
        bytes.push(b'\n');
        bytes.extend(frame(json!({"op":"set","device":ID,"changes":{"dpi":800}})));
        let mut output = Vec::new();
        let result = serve(service, bytes.as_slice(), &mut output);
        assert_eq!(result.is_ok(), extra == 0);
        assert!(state.borrow().calls.is_empty());
        assert!(state.borrow().dropped);
        assert_eq!(replies(&output).len(), 1);
    }
}
#[test]
fn read_error_is_terminal_and_drops_before_framing_reply() {
    struct Failed;
    impl Read for Failed {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("synthetic read failure"))
        }
    }
    impl BufRead for Failed {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            Err(io::Error::other("synthetic read failure"))
        }
        fn consume(&mut self, _: usize) {}
    }
    let (_dir, service, state) = service();
    let mut output = ClosingWriter {
        state: state.clone(),
        bytes: vec![],
    };
    assert!(serve(service, Failed, &mut output)
        .unwrap_err()
        .contains("synthetic read"));
    assert_eq!(replies(&output.bytes)[0]["stage"], "framing");
}
struct ClosingWriter {
    state: Rc<RefCell<State>>,
    bytes: Vec<u8>,
}
impl Write for ClosingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        assert!(self.state.borrow().dropped);
        self.bytes.extend(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[test]
fn shutdown_ack_is_sent_only_after_backend_destruction() {
    let (_dir, service, state) = service();
    let mut output = ClosingWriter {
        state,
        bytes: vec![],
    };
    serve(
        service,
        frame(json!({"op":"shutdown"})).as_slice(),
        &mut output,
    )
    .unwrap();
    assert_eq!(replies(&output.bytes)[0]["data"]["stopped"], true);
}
#[test]
fn output_write_or_flush_failure_after_apply_closes_without_retry() {
    struct Failed(bool);
    impl Write for Failed {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            if self.0 {
                Ok(b.len())
            } else {
                Err(io::ErrorKind::BrokenPipe.into())
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
    }
    for flush in [false, true] {
        let (_dir, service, state) = service();
        let input = frame(json!({"op":"set","device":ID,"changes":{"dpi":2400}}));
        assert!(serve(service, input.as_slice(), Failed(flush)).is_err());
        assert_eq!(state.borrow().calls, vec!["prepare", "apply"]);
        assert!(state.borrow().dropped);
        assert_eq!(state.borrow().dpi, 2400);
    }
}
#[test]
fn oversized_output_reports_uncertainty_instead_of_false_success() {
    let mut output = Vec::new();
    emit(
        &mut output,
        &json!({"ok":true,"data":"x".repeat(MAX_RESPONSE_BYTES)}),
    )
    .unwrap();
    assert!(output.len() <= MAX_RESPONSE_BYTES);
    let result = &replies(&output)[0];
    assert_eq!(result["ok"], false);
    assert_eq!(result["outcome_uncertain"], true);
}
#[test]
fn response_is_flushed_before_reading_the_next_request() {
    struct Gated {
        input: io::Cursor<Vec<u8>>,
        first: usize,
        flushes: Rc<RefCell<usize>>,
    }
    impl Read for Gated {
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            self.input.read(b)
        }
    }
    impl BufRead for Gated {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            if self.input.position() as usize >= self.first {
                assert!(*self.flushes.borrow() >= 1);
            }
            self.input.fill_buf()
        }
        fn consume(&mut self, n: usize) {
            self.input.consume(n);
        }
    }
    struct Flushed(Rc<RefCell<usize>>);
    impl Write for Flushed {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            *self.0.borrow_mut() += 1;
            Ok(())
        }
    }
    let (_dir, service, _state) = service();
    let mut bytes = frame(json!({"op":"export","device":ID,"name":"Work"}));
    let first = bytes.len();
    bytes.extend(frame(json!({"op":"shutdown"})));
    let flushes = Rc::new(RefCell::new(0));
    serve(
        service,
        Gated {
            input: io::Cursor::new(bytes),
            first,
            flushes: flushes.clone(),
        },
        Flushed(flushes.clone()),
    )
    .unwrap();
    assert_eq!(*flushes.borrow(), 2);
}
