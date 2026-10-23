// SPDX-License-Identifier: GPL-3.0-or-later
//! The actual Rust backend over hand-authored protocol fixtures, never hardware.
#[path = "profile_event_protocol_tests.rs"]
mod profile_event_protocol_tests;
#[path = "profile_owner_protocol_tests.rs"]
mod profile_owner_protocol_tests;
use super::*;
use crate::{
    hidpp::Session,
    native::NativeProfileBackend,
    profiles::{Document, ProfileStore},
    service::{Changes, ProfileBackend, ProfileService},
};

fn backend(mut device: Device) -> NativeProfileBackend<Device> {
    device.base.accept_writes = true;
    NativeProfileBackend::new(Session::new(device), 1, "11223344").unwrap()
}
fn scalar_setters(state: &State) -> Vec<Vec<u8>> {
    state
        .wire
        .iter()
        .filter(|p| matches!((p[2], p[3] >> 4), (14, 3) | (8, 1) | (10, 2) | (12, 2)))
        .cloned()
        .collect()
}
#[test]
fn a_combined_profile_is_confirmed_on_one_session_without_reporting_writes() {
    let (device, state) = Device::new();
    let mut backend = backend(device);
    let changes = Changes::new(Some(json!({"dpi":1800})), Some(json!({"83":80,"86":81}))).unwrap();
    backend.prepare(&changes, deadline()).unwrap();
    assert!(scalar_setters(&state.borrow()).is_empty());
    assert!(setters(&state.borrow()).is_empty());
    let receipt = backend.apply(&changes, deadline()).unwrap();
    assert_eq!(receipt.confirmed, vec!["dpi", "button:83", "button:86"]);
    assert_eq!(receipt.snapshot["settings"]["dpi"]["value"], 1800);
    assert_eq!(receipt.snapshot["buttons"]["controls"][0]["value"], 80);
    assert_eq!(scalar_setters(&state.borrow()).len(), 1);
    assert_eq!(setters(&state.borrow()).len(), 2);
    for request in setters(&state.borrow()) {
        assert_eq!(request[6], 0);
        assert!(request[9..].iter().all(|b| *b == 0));
    }
}
#[test]
fn unsupported_button_target_blocks_scalar_writes_before_any_setter() {
    let (device, state) = Device::new();
    let mut backend = backend(device);
    let changes = Changes::new(Some(json!({"dpi":1800})), Some(json!({"83":65535}))).unwrap();
    assert!(backend
        .apply(&changes, deadline())
        .unwrap_err()
        .confirmed
        .is_empty());
    assert!(scalar_setters(&state.borrow()).is_empty());
    assert!(setters(&state.borrow()).is_empty());
}
#[test]
fn unsupported_scalar_choices_block_an_otherwise_valid_remap() {
    let (device, state) = Device::new();
    let mut backend = backend(device);
    let changes = Changes::new(Some(json!({"dpi":1801})), Some(json!({"83":80}))).unwrap();
    assert!(backend.apply(&changes, deadline()).is_err());
    assert!(scalar_setters(&state.borrow()).is_empty());
    assert!(setters(&state.borrow()).is_empty());
}
#[test]
fn software_profiles_fail_before_getters_or_partial_scalar_configuration() {
    for action in [
        "modifier:ctrl",
        "shortcut:copy",
        "custom::KeyS",
        "desktop:fullscreen",
    ] {
        let (device, state) = Device::new();
        let mut backend = backend(device);
        let changes = Changes::new(Some(json!({"dpi":1800})), Some(json!({"83":action}))).unwrap();
        assert!(backend
            .prepare(&changes, deadline())
            .unwrap_err()
            .contains("guardian"));
        assert!(backend.apply(&changes, deadline()).is_err());
        assert!(state.borrow().wire.is_empty());
    }
}
#[test]
fn unchanged_values_are_read_back_without_repeating_setters() {
    let (device, state) = Device::new();
    let mut backend = backend(device);
    let changes = Changes::new(Some(json!({"dpi":1850})), Some(json!({"83":83}))).unwrap();
    let receipt = backend.apply(&changes, deadline()).unwrap();
    assert!(receipt.confirmed.is_empty());
    assert!(scalar_setters(&state.borrow()).is_empty());
    assert!(setters(&state.borrow()).is_empty());
}
#[test]
fn button_failure_retains_prior_scalar_confirmation_and_uncertain_target() {
    let (device, state) = Device::new();
    state.borrow_mut().bad_ack = true;
    let mut backend = backend(device);
    let changes = Changes::new(Some(json!({"dpi":1800})), Some(json!({"83":80}))).unwrap();
    let error = backend.apply(&changes, deadline()).unwrap_err();
    assert_eq!(error.confirmed, vec!["dpi"]);
    assert_eq!(error.attempted.as_deref(), Some("button:83"));
    assert_eq!(setters(&state.borrow()).len(), 1);
    assert_eq!(state.borrow().targets.get(&83), Some(&80));
}
#[test]
fn missing_effect_after_an_ack_is_never_reported_as_an_applied_profile() {
    let (device, state) = Device::new();
    state.borrow_mut().ignore_set = true;
    let mut backend = backend(device);
    let changes = Changes::new(Some(json!({"dpi":1800})), Some(json!({"83":80}))).unwrap();
    let error = backend.apply(&changes, deadline()).unwrap_err();
    assert_eq!(error.confirmed, vec!["dpi"]);
    assert!(error.message.contains("not confirmed"));
    assert_eq!(setters(&state.borrow()).len(), 1);
}
#[test]
fn selected_identity_and_request_deadlines_gate_the_entire_batch() {
    let changes = Changes::new(Some(json!({"dpi":1800})), None).unwrap();
    let (device, state) = Device::new();
    let mut wrong = NativeProfileBackend::new(Session::new(device), 1, "55667788").unwrap();
    assert!(wrong.apply(&changes, deadline()).is_err());
    assert!(scalar_setters(&state.borrow()).is_empty());
    let (device, state) = Device::new();
    let mut backend = backend(device);
    assert!(backend.apply(&changes, Instant::now()).is_err());
    assert!(state.borrow().wire.is_empty());
}

struct DriftAfterRemap {
    inner: Device,
    remapped: bool,
    dpi_reply: bool,
}
impl Transport for DriftAfterRemap {
    fn write_report(&mut self, p: &[u8], limit: Instant) -> io::Result<()> {
        self.inner.write_report(p, limit)?;
        if p[2] == 18 && p[3] >> 4 == 3 {
            self.remapped = true;
        }
        self.dpi_reply = p[2] == 14 && p[3] >> 4 == 2;
        Ok(())
    }
    fn read_report(&mut self, limit: Instant) -> io::Result<Vec<u8>> {
        let mut reply = self.inner.read_report(limit)?;
        if self.remapped && self.dpi_reply {
            reply[4..9].copy_from_slice(&[0, 7, 58, 3, 32]);
        }
        self.dpi_reply = false;
        Ok(reply)
    }
}
#[test]
fn final_combined_readback_detects_scalar_drift_during_a_remap() {
    let (mut device, state) = Device::new();
    device.base.accept_writes = true;
    let mut backend = NativeProfileBackend::new(
        Session::new(DriftAfterRemap {
            inner: device,
            remapped: false,
            dpi_reply: false,
        }),
        1,
        "11223344",
    )
    .unwrap();
    let changes = Changes::new(Some(json!({"dpi":1800})), Some(json!({"83":80}))).unwrap();
    let error = backend.apply(&changes, deadline()).unwrap_err();
    assert_eq!(error.confirmed, vec!["dpi", "button:83"]);
    assert!(error.message.contains("Final profile setting unconfirmed"));
    assert_eq!(setters(&state.borrow()).len(), 1);
}

#[test]
fn persisted_profile_reconciles_through_the_real_rust_backend_with_synthetic_wire() {
    let root = ProfileDirectory::new();
    let store = ProfileStore::at(root.0.join("linux-scrollmaster")).unwrap();
    let document=Document::from_value(json!({"version":2,"profiles":[{"device":"11223344","name":"Work","settings":{"dpi":1800},"buttons":{"83":80}}],"active":{"11223344":"Work"}})).unwrap();
    store.commit(&store.load().unwrap(), &document).unwrap();
    let (device, state) = Device::new();
    let mut service = ProfileService::new(store, backend(device), Duration::from_secs(2)).unwrap();
    let first = service.handle(&json!({"op":"snapshot"}), None).unwrap();
    assert_eq!(
        first["reconciliation"][0]["applied"],
        json!(["dpi", "button:83"])
    );
    let second = service.handle(&json!({"op":"snapshot"}), None).unwrap();
    assert_eq!(second["reconciliation"][0]["applied"], json!([]));
    assert_eq!(setters(&state.borrow()).len(), 1);
    assert_eq!(scalar_setters(&state.borrow()).len(), 1);
}

struct ProfileDirectory(std::path::PathBuf);
impl ProfileDirectory {
    fn new() -> Self {
        use std::os::unix::fs::DirBuilderExt;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "scrollmaster-profile-wire-{}-{stamp}",
            std::process::id()
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        Self(path)
    }
}
impl Drop for ProfileDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
