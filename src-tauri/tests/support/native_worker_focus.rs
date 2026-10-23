// SPDX-License-Identifier: GPL-3.0-or-later
//! Real production worker and synthetic compositor sockets. No HID operations.
use super::*;
use std::{
    os::unix::net::UnixListener,
    sync::{atomic::AtomicBool, Arc, Mutex},
    thread::{self, JoinHandle},
};

struct Peer {
    runtime: Sandbox,
    requests: Arc<Mutex<Vec<Vec<u8>>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl Peer {
    fn new(steps: Vec<(&'static [u8], Value)>) -> Self {
        let runtime = Sandbox::new();
        let socket_dir = runtime.0.join("hypr/focus-test");
        fs::create_dir_all(&socket_dir).unwrap();
        let listener = UnixListener::bind(socket_dir.join(".socket.sock")).unwrap();
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = stop.clone();
        let thread = thread::spawn(move || {
            for (expected, reply) in steps {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    if cancelled.load(Ordering::Relaxed) || Instant::now() >= deadline {
                        return;
                    }
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(1))
                        }
                        Err(e) => panic!("Synthetic accept failed: {e}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut bytes = vec![0; expected.len()];
                if stream.read_exact(&mut bytes).is_err() {
                    return;
                }
                assert_eq!(bytes, expected);
                log.lock().unwrap().push(bytes);
                let _ = stream.write_all(&serde_json::to_vec(&reply).unwrap());
            }
        });
        Self {
            runtime,
            requests,
            stop,
            thread: Some(thread),
        }
    }
    fn command(&self, dir: &Sandbox) -> Command {
        let mut command = dir.configured();
        command
            .env("XDG_RUNTIME_DIR", &self.runtime.0)
            .env("HYPRLAND_INSTANCE_SIGNATURE", "focus-test");
        command
    }
    fn log(&self) -> Vec<Vec<u8>> {
        self.requests.lock().unwrap().clone()
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}
fn application_steps(class: &str) -> Vec<(&'static [u8], Value)> {
    vec![
        (
            b"j/activewindow",
            json!({"class":class,"title":"secret-document","pid":123}),
        ),
        (
            b"j/clients",
            json!([{"class":class,"title":"private"},{"class":"Other"}]),
        ),
    ]
}

#[test]
fn native_worker_lists_applications_over_ipc_without_hid_or_profile_mutations() {
    let dir = Sandbox::new();
    dir.seed_armed(true);
    let original = dir.bytes();
    let peer = Peer::new(application_steps("Editor"));
    let mut worker = Worker::start(peer.command(&dir)).unwrap();
    let reply = transact(&mut worker, json!({"op":"applications","device":ID}));
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["data"]["desktop"]["active_application"], "Editor");
    assert_eq!(
        reply["data"]["desktop"]["applications"],
        json!(["Editor", "Other"])
    );
    assert_eq!(reply["data"]["devices"], json!([]));
    assert_eq!(reply["data"]["reconciliation"], json!([]));
    assert_eq!(reply["data"]["active"][ID], "Work");
    let serialized = reply.to_string();
    for private in ["secret-document", "private", "pid", "title"] {
        assert!(!serialized.contains(private));
    }
    assert_eq!(
        peer.log(),
        vec![b"j/activewindow".to_vec(), b"j/clients".to_vec()]
    );
    assert_eq!(transact(&mut worker, json!({"op":"shutdown"}))["ok"], true);
    drop(worker);
    assert_eq!(dir.bytes(), original);
    assert!(!peer
        .runtime
        .0
        .join("linux-scrollmaster-hardware.lock")
        .exists());
}
#[test]
fn native_worker_drops_partial_focus_after_catalogue_failure_and_reads_again() {
    let dir = Sandbox::new();
    dir.seed();
    let mut steps = application_steps("First");
    steps.extend(vec![
        (b"j/activewindow".as_slice(), json!({"class":"Stale"})),
        (b"j/clients".as_slice(), json!({"bad":"shape"})),
    ]);
    steps.extend(application_steps("Fresh"));
    let peer = Peer::new(steps);
    let mut worker = Worker::start(peer.command(&dir)).unwrap();
    assert_eq!(
        transact(&mut worker, json!({"op":"applications"}))["data"]["desktop"]
            ["active_application"],
        "First"
    );
    let failed = transact(&mut worker, json!({"op":"applications"}));
    assert_eq!(failed["ok"], true);
    assert_eq!(failed["data"]["desktop"]["available"], false);
    assert_eq!(failed["data"]["desktop"]["active_application"], Value::Null);
    assert_eq!(
        transact(&mut worker, json!({"op":"applications"}))["data"]["desktop"]
            ["active_application"],
        "Fresh"
    );
    assert_eq!(peer.log().len(), 6);
    transact(&mut worker, json!({"op":"shutdown"}));
    assert!(!peer
        .runtime
        .0
        .join("linux-scrollmaster-hardware.lock")
        .exists());
}
#[test]
fn absent_synthetic_session_keeps_profiles_available_and_rejects_focus_injection() {
    let dir = Sandbox::new();
    dir.seed();
    let original = dir.bytes();
    let mut command = dir.configured();
    command.env("HYPRLAND_INSTANCE_SIGNATURE", "absent-synthetic-session");
    let mut worker = Worker::start(command).unwrap();
    let invalid = transact(
        &mut worker,
        json!({"op":"applications","application":"injected-focus"}),
    );
    assert_eq!(invalid["stage"], "validation");
    let reply = transact(&mut worker, json!({"op":"applications"}));
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["data"]["desktop"]["available"], false);
    assert_eq!(reply["data"]["profiles"][0]["name"], "Work");
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
