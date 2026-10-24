// SPDX-License-Identifier: GPL-3.0-or-later
//! Synthetic Unix peers only. Never resolve the user's compositor environment.
use super::*;
use serde_json::json;
use std::{
    io::{Read, Write},
    os::unix::net::UnixListener,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    thread::{self, JoinHandle},
};

/// Answers each scripted request in order and returns what it received.
struct Peer {
    path: PathBuf,
    worker: Option<JoinHandle<Vec<String>>>,
}
impl Peer {
    fn script(replies: Vec<Vec<u8>>) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "scrollmaster-scroll-{}-{}.sock",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let listener = UnixListener::bind(&path).unwrap();
        let worker = thread::spawn(move || {
            let mut seen = Vec::new();
            for reply in replies {
                let (mut stream, _) = listener.accept().unwrap();
                stream.set_read_timeout(Some(TIMEOUT)).unwrap();
                let mut buffer = [0; 4096];
                let n = stream.read(&mut buffer).unwrap();
                seen.push(String::from_utf8(buffer[..n].to_vec()).unwrap());
                stream.write_all(&reply).unwrap();
            }
            seen
        });
        Self {
            path,
            worker: Some(worker),
        }
    }
    fn finish(mut self) -> Vec<String> {
        self.worker.take().unwrap().join().unwrap()
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
fn version() -> Vec<u8> {
    br#"{"version":"0.56.2"}"#.to_vec()
}
fn devices(factor: f64) -> Vec<u8> {
    json!({"mice": [
        {"name": "logitech-usb-receiver-mouse", "scrollFactor": factor},
        {"name": "other-mouse", "scrollFactor": -1.0},
    ]})
    .to_string()
    .into_bytes()
}
fn read_script(factor: f64) -> Vec<Vec<u8>> {
    vec![version(), b"ok".to_vec(), devices(factor)]
}

#[test]
fn range_is_bounded_and_rounded_never_zero_or_false() {
    assert_eq!(normalize(1.234).unwrap(), 1.23);
    assert_eq!(normalize(MIN).unwrap(), MIN);
    assert_eq!(normalize(MAX).unwrap(), MAX);
    for bad in [0.0, 0.05, 5.01, -1.0, f64::NAN, f64::INFINITY] {
        assert!(normalize(bad).is_err(), "{bad}");
    }
}

#[test]
fn only_mx_master_entries_with_safe_names_are_targets() {
    let reply = json!({"mice": [
        {"name": "logitech-usb-receiver-mouse", "scrollFactor": -1.0},
        {"name": "logitech-mx-master-3s", "scrollFactor": 1.5},
        {"name": "logitech-usb-receiver-consumer-control-1", "scrollFactor": -1.0},
        {"name": "mx-master-3s\", scroll_factor = 0 })", "scrollFactor": 1.0},
        {"name": "touchpad", "scrollFactor": 1.0},
    ]});
    let names: Vec<_> = targets(&reply)
        .unwrap()
        .into_iter()
        .map(|d| d.name)
        .collect();
    assert_eq!(
        names,
        ["logitech-usb-receiver-mouse", "logitech-mx-master-3s"]
    );
    assert!(targets(&json!({})).is_err());
}

#[test]
fn payload_targets_one_device_with_two_decimals() {
    assert_eq!(
        payload("logitech-usb-receiver-mouse", 1.5),
        b"/eval hl.device({ name = \"logitech-usb-receiver-mouse\", scroll_factor = 1.50 })"
    );
}

#[test]
fn apply_writes_only_the_mouse_and_confirms_by_reading_back() {
    let mut script = read_script(-1.0);
    script.push(b"ok".to_vec());
    script.extend(read_script(1.5));
    let peer = Peer::script(script);
    let confirmed = apply_at(&peer.path, 1.5).unwrap();
    assert_eq!(confirmed[0].factor, 1.5);
    let seen = peer.finish();
    assert_eq!(
        seen.iter()
            .filter(|r| r.starts_with("/eval hl.device"))
            .count(),
        1
    );
    assert!(seen[3].contains("logitech-usb-receiver-mouse"));
    assert!(!seen.iter().any(|r| r.contains("other-mouse")));
}

#[test]
fn unconfirmed_readback_is_an_error() {
    let mut script = read_script(-1.0);
    script.push(b"ok".to_vec());
    script.extend(read_script(-1.0));
    let peer = Peer::script(script);
    assert!(apply_at(&peer.path, 1.5).is_err());
    peer.finish();
}

#[test]
fn unsupported_version_or_missing_mouse_writes_nothing() {
    let peer = Peer::script(vec![br#"{"version":"0.54.0"}"#.to_vec()]);
    assert!(apply_at(&peer.path, 1.5).is_err());
    assert_eq!(peer.finish().len(), 1);
    let peer = Peer::script(vec![
        version(),
        b"ok".to_vec(),
        json!({"mice": []}).to_string().into_bytes(),
    ]);
    assert!(apply_at(&peer.path, 1.5).is_err());
    assert!(!peer
        .finish()
        .iter()
        .any(|r| r.starts_with("/eval hl.device")));
}

#[test]
fn global_factor_is_read_for_reset() {
    let peer = Peer::script(vec![
        br#"{"option":"input:scroll_factor","float":0.8,"set":true}"#.to_vec(),
    ]);
    assert_eq!(global_at(&peer.path).unwrap(), 0.8);
    assert_eq!(peer.finish(), ["j/getoption input:scroll_factor"]);
}
