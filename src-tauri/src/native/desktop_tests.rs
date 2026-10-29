// SPDX-License-Identifier: GPL-3.0-or-later
//! Synthetic Unix peers only. Never resolve the user's compositor environment.
use super::*;
use std::{
    os::unix::net::UnixListener,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
};
const VERSION: &[u8] = br#"{"version":"0.56.2"}"#;
type Requests = Arc<Mutex<Vec<Vec<u8>>>>;
struct Peer {
    path: PathBuf,
    requests: Requests,
    worker: Option<JoinHandle<()>>,
}
impl Peer {
    fn custom(handler: impl FnOnce(UnixListener, Requests) + Send + 'static) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "scrollmaster-desktop-{}-{}.sock",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = requests.clone();
        Self {
            path,
            requests,
            worker: Some(thread::spawn(move || handler(listener, log))),
        }
    }
    fn script(steps: Vec<(&'static [u8], Vec<u8>)>) -> Self {
        Self::custom(move |listener, log| {
            for (expected, reply) in steps {
                let mut stream = accept(&listener);
                read_request(&mut stream, expected, &log);
                stream.write_all(&reply).unwrap();
            }
        })
    }
    fn client(&self) -> Hyprland<impl FnMut() -> Result<PathBuf, String>> {
        let path = self.path.clone();
        Hyprland {
            resolve: move || Ok(path.clone()),
            timeout: TIMEOUT,
        }
    }
    fn finish(mut self) -> Vec<Vec<u8>> {
        self.worker.take().unwrap().join().unwrap();
        self.requests.lock().unwrap().clone()
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
        std::fs::remove_file(&self.path).unwrap();
    }
}
fn accept(listener: &UnixListener) -> UnixStream {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_read_timeout(Some(TIMEOUT)).unwrap();
                stream.set_write_timeout(Some(TIMEOUT)).unwrap();
                return stream;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(1))
            }
            other => panic!("Expected synthetic connection: {other:?}"),
        }
    }
}
fn read_request(stream: &mut UnixStream, expected: &[u8], log: &Requests) {
    let mut bytes = vec![0; expected.len()];
    stream.read_exact(&mut bytes).unwrap();
    assert_eq!(bytes, expected);
    log.lock().unwrap().push(bytes);
}
fn preflight() -> Vec<(&'static [u8], Vec<u8>)> {
    vec![(b"j/version", VERSION.to_vec()), (PROBE, b"ok".to_vec())]
}

#[test]
fn desktop_catalogue_is_closed_and_uses_fixed_lua_payloads() {
    for (id, expected) in [
        (
            "desktop:workspace-previous",
            "/dispatch hl.dsp.focus({ workspace = \"r-1\" })",
        ),
        (
            "desktop:workspace-next",
            "/dispatch hl.dsp.focus({ workspace = \"r+1\" })",
        ),
        (
            "desktop:fullscreen",
            "/dispatch hl.dsp.window.fullscreen({ mode = \"fullscreen\", action = \"toggle\" })",
        ),
        (
            "desktop:maximize",
            "/dispatch hl.dsp.window.fullscreen({ mode = \"maximized\", action = \"toggle\" })",
        ),
    ] {
        assert_eq!(
            DesktopAction::parse(id).unwrap().payload(),
            expected.as_bytes()
        );
    }
    for id in [
        "desktop:exec",
        "desktop:fullscreen ",
        "desktop:fullscreen\n",
        "desktop:workspace-next;exec x",
        "/dispatch exec x",
        "shell:sh",
        "",
        "desktop:hl.dsp.focus({})",
    ] {
        assert!(DesktopAction::parse(id).is_err(), "{id:?}");
    }
}
#[test]
fn supported_versions_match_the_existing_lua_contract() {
    for version in [
        "0.55.0",
        "v0.55.1",
        "0.56.2",
        "v0.56.2-3-gdeadbeef",
        "0.56.2+dev.1",
        "0.57.0",
        "v0.57.1",
    ] {
        assert!(supported_version(version), "{version}");
    }
    for version in [
        "0.54.9",
        "0.58.0",
        "1.56.0",
        "0.56",
        "0.56.x",
        "0.56.2-",
        "0.56.2+",
        "0.56.2 dev",
        "0.56.2\n",
        "0.56.2+dev+x",
        "v0.56.٢",
        "0.56.2/foo",
    ] {
        assert!(!supported_version(version), "{version}");
    }
}
#[test]
fn session_paths_reject_missing_relative_and_traversing_environments() {
    assert_eq!(
        session_path(Path::new("/run/user/1000"), "abc_123").unwrap(),
        Path::new("/run/user/1000/hypr/abc_123/.socket.sock")
    );
    for runtime in ["", "relative"] {
        assert!(session_path(Path::new(runtime), "abc").is_err());
    }
    for signature in [
        "", ".", "..", "/abc", "../abc", "abc/", "a/b", "a\0b", "a\nb",
    ] {
        assert!(
            session_path(Path::new("/run/user/1000"), signature).is_err(),
            "{signature:?}"
        );
    }
}
#[test]
fn capability_probe_is_read_only() {
    let peer = Peer::script(preflight());
    peer.client().require_actions().unwrap();
    assert_eq!(peer.finish(), [b"j/version".to_vec(), PROBE.to_vec()]);
}
#[test]
fn every_dispatch_rechecks_version_and_capabilities_without_caching() {
    let actions = [
        DesktopAction::WorkspacePrevious,
        DesktopAction::WorkspaceNext,
        DesktopAction::Fullscreen,
        DesktopAction::Maximize,
    ];
    let mut steps = Vec::new();
    for action in actions {
        steps.extend(preflight());
        steps.push((action.payload(), b" \nok\t".to_vec()));
    }
    let peer = Peer::script(steps);
    let mut client = peer.client();
    for action in actions {
        client.dispatch(action).unwrap();
    }
    assert_eq!(peer.finish().len(), 12);
}
#[test]
fn bad_versions_never_probe_or_dispatch() {
    for reply in [
        b"no JSON".as_slice(),
        b"[]",
        b"null",
        b"{}",
        br#"{"version":56}"#,
        br#"{"version":"0.54.3"}"#,
        br#"{"version":"0.57.0"}"#,
    ] {
        let peer = Peer::script(vec![(b"j/version", reply.to_vec())]);
        assert!(peer.client().dispatch(DesktopAction::Fullscreen).is_err());
        assert_eq!(peer.finish().len(), 1);
    }
}
#[test]
fn legacy_configuration_and_bad_capability_reply_never_dispatch() {
    for reply in [
        b"eval is only supported with the lua config manager".as_slice(),
        b"error",
        b"ok but error",
        b"",
        b"OK",
        b"ok\0",
        &[0xff],
    ] {
        let peer = Peer::script(vec![
            (b"j/version", VERSION.to_vec()),
            (PROBE, reply.to_vec()),
        ]);
        assert!(peer.client().dispatch(DesktopAction::Maximize).is_err());
        assert_eq!(peer.finish().len(), 2);
    }
}
#[test]
fn uncertain_dispatch_ack_is_not_retried() {
    for reply in [
        b"".as_slice(),
        b"error",
        b"ok\nerror",
        b"OK",
        b"ok\0",
        &[0xff],
    ] {
        let mut steps = preflight();
        steps.push((DesktopAction::Fullscreen.payload(), reply.to_vec()));
        let peer = Peer::script(steps);
        assert!(peer.client().dispatch(DesktopAction::Fullscreen).is_err());
        assert_eq!(peer.finish().len(), 3);
    }
}
#[test]
fn current_session_is_resolved_once_per_action_and_again_for_the_next() {
    let mut steps = preflight();
    steps.push((DesktopAction::WorkspaceNext.payload(), b"ok".to_vec()));
    let a = Peer::script(steps.clone());
    let b = Peer::script(steps);
    let paths = [a.path.clone(), b.path.clone()];
    let mut calls = 0;
    let mut client = Hyprland {
        resolve: || {
            let path = paths[calls].clone();
            calls += 1;
            Ok(path)
        },
        timeout: TIMEOUT,
    };
    client.dispatch(DesktopAction::WorkspaceNext).unwrap();
    client.dispatch(DesktopAction::WorkspaceNext).unwrap();
    assert_eq!(calls, 2);
    assert_eq!(a.finish().len(), 3);
    assert_eq!(b.finish().len(), 3);
}
#[test]
fn failed_session_resolution_does_not_fall_back_or_retry() {
    let mut calls = 0;
    let mut client = Hyprland {
        resolve: || {
            calls += 1;
            Err("no session".into())
        },
        timeout: TIMEOUT,
    };
    assert_eq!(
        client.dispatch(DesktopAction::Fullscreen).unwrap_err(),
        "no session"
    );
    assert_eq!(calls, 1);
}
#[test]
fn oversized_probe_and_dispatch_replies_fail() {
    for probe in [true, false] {
        let mut steps = vec![(b"j/version".as_slice(), VERSION.to_vec())];
        steps.push((
            PROBE,
            if probe {
                vec![b'x'; 4097]
            } else {
                b"ok".to_vec()
            },
        ));
        if !probe {
            steps.push((DesktopAction::Fullscreen.payload(), vec![b'x'; 4097]));
        }
        let peer = Peer::script(steps);
        assert!(peer
            .client()
            .dispatch(DesktopAction::Fullscreen)
            .unwrap_err()
            .contains("exceeds"));
        assert_eq!(peer.finish().len(), if probe { 2 } else { 3 });
    }
}
#[test]
fn exchange_enforces_an_exact_reply_byte_limit() {
    for size in [16, 17] {
        let peer = Peer::script(vec![(b"test", vec![b'x'; size])]);
        let reply = exchange(&peer.path, b"test", 16, TIMEOUT);
        if size == 16 {
            assert_eq!(reply.unwrap(), vec![b'x'; 16]);
        } else {
            assert!(reply.unwrap_err().contains("exceeds"));
        }
        assert_eq!(peer.finish().len(), 1);
    }
}
#[test]
fn fragmented_replies_are_read_until_eof() {
    let peer = Peer::custom(|listener, log| {
        let mut stream = accept(&listener);
        read_request(&mut stream, b"test", &log);
        for byte in b"ok" {
            stream.write_all(&[*byte]).unwrap();
            thread::sleep(Duration::from_millis(2));
        }
    });
    assert_eq!(exchange(&peer.path, b"test", 16, TIMEOUT).unwrap(), b"ok");
    peer.finish();
}
#[test]
fn a_trickling_peer_cannot_extend_the_absolute_deadline() {
    let peer = Peer::custom(|listener, log| {
        let mut stream = accept(&listener);
        read_request(&mut stream, b"test", &log);
        for _ in 0..30 {
            if stream.write_all(b"x").is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
    });
    let start = Instant::now();
    assert!(
        exchange(&peer.path, b"test", 4096, Duration::from_millis(50))
            .unwrap_err()
            .contains("timed out")
    );
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(peer.finish().len(), 1);
}
#[test]
fn an_ok_without_eof_is_uncertain_and_times_out() {
    let peer = Peer::custom(|listener, log| {
        let mut stream = accept(&listener);
        read_request(&mut stream, b"test", &log);
        stream.write_all(b"ok").unwrap();
        thread::sleep(Duration::from_millis(100));
    });
    assert!(
        exchange(&peer.path, b"test", 4096, Duration::from_millis(30))
            .unwrap_err()
            .contains("timed out")
    );
    assert_eq!(peer.finish().len(), 1);
}
#[test]
fn malformed_and_unavailable_socket_paths_fail_without_discovery() {
    for path in [
        PathBuf::from(""),
        PathBuf::from("/tmp/a\0b"),
        PathBuf::from(format!("/tmp/{}", "x".repeat(108))),
        PathBuf::from("/tmp/scrollmaster-absent-session-dir/.socket.sock"),
    ] {
        assert!(exchange(&path, b"test", 16, TIMEOUT).is_err());
    }
}
