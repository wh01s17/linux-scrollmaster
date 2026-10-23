// SPDX-License-Identifier: GPL-3.0-or-later
//! Synthetic Unix peers. Never consult the user's session or send dispatchers.
use super::*;
use std::{
    collections::VecDeque,
    io::{Read, Write},
    os::unix::net::UnixListener,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
};

struct Step {
    expected: &'static [u8],
    reply: Vec<u8>,
    delay: Duration,
    fragmented: bool,
}
fn step(expected: &'static [u8], reply: Value) -> Step {
    Step {
        expected,
        reply: serde_json::to_vec(&reply).unwrap(),
        delay: Duration::ZERO,
        fragmented: false,
    }
}
struct Peer {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<Vec<u8>>>>,
    thread: Option<JoinHandle<()>>,
}
impl Peer {
    fn new(steps: Vec<Step>) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "scrollmaster-focus-{}-{}.sock",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = stop.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = requests.clone();
        let thread = thread::spawn(move || {
            for step in steps {
                let end = Instant::now() + Duration::from_secs(3);
                let mut stream = loop {
                    if cancelled.load(Ordering::Relaxed) || Instant::now() >= end {
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
                let mut bytes = vec![0; step.expected.len()];
                if stream.read_exact(&mut bytes).is_err() {
                    return;
                }
                log.lock().unwrap().push(bytes);
                thread::sleep(step.delay);
                if step.fragmented {
                    for chunk in step.reply.chunks(3) {
                        if stream.write_all(chunk).is_err() {
                            break;
                        }
                        thread::sleep(Duration::from_millis(1));
                    }
                } else {
                    let _ = stream.write_all(&step.reply);
                }
            }
        });
        Self {
            path,
            stop,
            requests,
            thread: Some(thread),
        }
    }
    fn client(&self) -> HyprlandFocus<impl FnMut() -> Result<PathBuf, String>> {
        let path = self.path.clone();
        HyprlandFocus {
            resolve: move || Ok(path.clone()),
            budget: FOCUS_BUDGET,
        }
    }
    fn log(&self) -> Vec<Vec<u8>> {
        self.requests.lock().unwrap().clone()
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
        let _ = std::fs::remove_file(&self.path);
    }
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(1)
}

#[test]
fn only_exact_class_ids_leave_the_adapter_and_catalogue_is_sorted_and_deduplicated() {
    let snapshot = DesktopSnapshot::from_values(&json!({"class":"Editor","title":"secret.txt","pid":123,"address":"private"}), Some(&json!([
        {"class":"zeta","title":"secret-title"},{"class":"Alpha"},{"class":"zeta"},{"class":"alpha"},{"class":"Árbol"},{"class":null},{"title":"missing"}
    ]))).unwrap();
    assert_eq!(snapshot.active_application(), Some("Editor"));
    assert_eq!(
        snapshot.describe(),
        json!({"available":true,"name":"Hyprland","active_application":"Editor","applications":["Alpha","alpha","zeta","Árbol"]})
    );
    let output = snapshot.describe().to_string();
    for private in ["title", "pid", "address", "secret"] {
        assert!(!output.contains(private));
    }
}
#[test]
fn class_validation_matches_profile_character_limits_without_trimming_or_case_changes() {
    for class in ["", "line\nbreak", "tab\t", "\0", &"é".repeat(257)] {
        let result =
            DesktopSnapshot::from_values(&json!({"class":class}), Some(&json!([{"class":class}])))
                .unwrap();
        assert_eq!(result.active_application(), None);
        assert_eq!(result.describe()["applications"], json!([]));
    }
    for class in [" App ", "CaseSensitive", "控制台", &"é".repeat(256)] {
        assert_eq!(
            DesktopSnapshot::from_values(&json!({"class":class}), None)
                .unwrap()
                .active_application(),
            Some(class)
        );
    }
}
#[test]
fn absent_focus_is_available_but_malformed_response_shapes_are_errors() {
    for active in [json!({}), json!({"class":null}), json!({"class":42})] {
        let result = DesktopSnapshot::from_values(&active, None).unwrap();
        assert_eq!(result.describe()["available"], true);
        assert_eq!(result.active_application(), None);
    }
    for active in [json!([]), json!(null), json!("App")] {
        assert!(DesktopSnapshot::from_values(&active, None).is_err());
    }
    for clients in [json!({}), json!(null), json!([{}, 42])] {
        assert!(DesktopSnapshot::from_values(&json!({"class":"App"}), Some(&clients)).is_err());
    }
}
#[test]
fn focus_queries_only_activewindow_and_never_lua_version_or_clients() {
    let peer = Peer::new(vec![step(b"j/activewindow", json!({"class":"Editor"}))]);
    let result = peer.client().describe(false, deadline());
    assert_eq!(result.active_application(), Some("Editor"));
    assert!(result.describe().get("applications").is_none());
    assert_eq!(peer.log(), vec![b"j/activewindow".to_vec()]);
}
#[test]
fn catalogue_uses_two_fixed_queries_with_one_session_resolution() {
    let peer = Peer::new(vec![
        step(b"j/activewindow", json!({"class":"App"})),
        step(b"j/clients", json!([{"class":"App"}])),
    ]);
    let mut resolutions = 0;
    let mut client = HyprlandFocus {
        resolve: || {
            resolutions += 1;
            Ok(peer.path.clone())
        },
        budget: FOCUS_BUDGET,
    };
    assert_eq!(
        client.describe(true, deadline()).describe()["applications"],
        json!(["App"])
    );
    assert_eq!(resolutions, 1);
    assert_eq!(
        peer.log(),
        vec![b"j/activewindow".to_vec(), b"j/clients".to_vec()]
    );
}
#[test]
fn next_request_resolves_the_new_session_without_reusing_focus() {
    let first = Peer::new(vec![step(b"j/activewindow", json!({"class":"First"}))]);
    let second = Peer::new(vec![step(b"j/activewindow", json!({"class":"Second"}))]);
    let mut paths = VecDeque::from([first.path.clone(), second.path.clone()]);
    let mut client = HyprlandFocus {
        resolve: move || Ok(paths.pop_front().unwrap()),
        budget: FOCUS_BUDGET,
    };
    assert_eq!(
        client.describe(false, deadline()).active_application(),
        Some("First")
    );
    assert_eq!(
        client.describe(false, deadline()).active_application(),
        Some("Second")
    );
}
#[test]
fn malformed_active_json_or_shape_stops_before_catalogue_query() {
    for bytes in [b"{broken".as_slice(), b"[]", b"\xff"] {
        let peer = Peer::new(vec![Step {
            expected: b"j/activewindow",
            reply: bytes.to_vec(),
            delay: Duration::ZERO,
            fragmented: false,
        }]);
        let result = peer.client().describe(true, deadline());
        assert_eq!(result.describe()["available"], false);
        assert_eq!(result.active_application(), None);
        assert_eq!(peer.log(), vec![b"j/activewindow".to_vec()]);
    }
}
#[test]
fn failed_catalogue_discards_the_earlier_focus_and_later_query_recovers() {
    let peer = Peer::new(vec![
        step(b"j/activewindow", json!({"class":"Stale"})),
        step(b"j/clients", json!({"bad":"shape"})),
        step(b"j/activewindow", json!({"class":"Fresh"})),
    ]);
    let mut client = peer.client();
    let failed = client.describe(true, deadline());
    assert_eq!(failed.describe()["available"], false);
    assert_eq!(failed.active_application(), None);
    assert!(failed.describe().get("applications").is_none());
    assert_eq!(
        client.describe(false, deadline()).active_application(),
        Some("Fresh")
    );
}
#[test]
fn peer_fragmentation_does_not_publish_a_partial_json_response() {
    let mut reply = step(b"j/activewindow", json!({"class":"Editor"}));
    reply.fragmented = true;
    let peer = Peer::new(vec![reply]);
    assert_eq!(
        peer.client()
            .describe(false, deadline())
            .active_application(),
        Some("Editor")
    );
}
#[test]
fn focus_response_byte_limit_includes_padding_and_rejects_overflow() {
    for extra in [0, 1] {
        let mut reply = step(b"j/activewindow", json!({"class":"App"}));
        reply.reply.resize(MAX_REPLY + extra, b' ');
        let peer = Peer::new(vec![reply]);
        let result = peer.client().describe(false, deadline());
        assert_eq!(result.describe()["available"], extra == 0);
        if extra == 1 {
            assert_eq!(result.active_application(), None);
        }
    }
}
#[test]
fn one_absolute_budget_covers_both_queries_without_extension() {
    let mut active = step(b"j/activewindow", json!({"class":"App"}));
    active.delay = Duration::from_millis(65);
    let mut clients = step(b"j/clients", json!([]));
    clients.delay = Duration::from_millis(65);
    let peer = Peer::new(vec![active, clients]);
    let mut client = peer.client();
    client.budget = Duration::from_millis(100);
    let started = Instant::now();
    let result = client.describe(true, deadline());
    assert_eq!(result.describe()["available"], false);
    assert_eq!(result.active_application(), None);
    assert!(started.elapsed() < Duration::from_secs(2));
}
#[test]
fn expired_budget_and_missing_session_never_invent_focus_or_probe_other_paths() {
    let mut resolutions = 0;
    let mut client = HyprlandFocus {
        resolve: || {
            resolutions += 1;
            Err("No synthetic session".into())
        },
        budget: FOCUS_BUDGET,
    };
    let result = client.describe(false, Instant::now());
    assert_eq!(result.active_application(), None);
    assert_eq!(resolutions, 0);
    let mut absent = HyprlandFocus {
        resolve: || Err("No synthetic session".into()),
        budget: FOCUS_BUDGET,
    };
    assert_eq!(
        absent.describe(true, deadline()).describe()["error"],
        "No synthetic session"
    );
    assert_eq!(
        NoDesktop.describe(false, deadline()).describe()["available"],
        false
    );
}
