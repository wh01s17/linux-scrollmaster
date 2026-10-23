// SPDX-License-Identifier: GPL-3.0-or-later
//! Actual resident backend and hand-authored HID++ frames; no hardware.
use super::*;
use crate::native::profile_events::Cancellation;
use std::{cell::Cell, collections::VecDeque};
struct Live {
    queued: VecDeque<Vec<u8>>,
    dropped: bool,
}
struct EventDevice {
    device: Device,
    live: Rc<RefCell<Live>>,
}
impl Drop for EventDevice {
    fn drop(&mut self) {
        self.live.borrow_mut().dropped = true;
    }
}
impl Transport for EventDevice {
    fn write_report(&mut self, p: &[u8], deadline: Instant) -> io::Result<()> {
        self.device.write_report(p, deadline)
    }
    fn read_report(&mut self, deadline: Instant) -> io::Result<Vec<u8>> {
        if let Some(report) = self.live.borrow_mut().queued.pop_front() {
            return Ok(report);
        }
        self.device.read_report(deadline)
    }
}
fn fixture() -> (
    NativeProfileBackend<EventDevice>,
    Rc<RefCell<State>>,
    Rc<RefCell<Live>>,
) {
    let (mut device, state) = Device::new();
    device.base.accept_writes = true;
    let live = Rc::new(RefCell::new(Live {
        queued: VecDeque::new(),
        dropped: false,
    }));
    let backend = NativeProfileBackend::new(
        Session::new(EventDevice {
            device,
            live: live.clone(),
        }),
        1,
        "11223344",
    )
    .unwrap();
    (backend, state, live)
}
#[test]
fn creation_and_poll_before_first_snapshot_do_not_read_or_open_the_session() {
    let (mut backend, state, _) = fixture();
    assert_eq!(backend.event_status()["initialized"], false);
    assert!(!backend.poll(Duration::from_millis(5)).unwrap());
    assert!(state.borrow().wire.is_empty());
}
#[test]
fn one_consumer_records_idle_and_query_frames_without_reporting_setters() {
    let (mut backend, state, live) = fixture();
    backend.snapshot(deadline()).unwrap();
    live.borrow_mut()
        .queued
        .extend([notification(&[]), notification(&[83]), notification(&[])]);
    for _ in 0..3 {
        assert!(backend.poll(Duration::from_millis(5)).unwrap());
    }
    let data = backend.snapshot(deadline()).unwrap();
    let events = &data["native_events"];
    assert_eq!(events["events"], 3);
    assert_eq!(events["recent"][0]["phase"], "idle");
    assert_eq!(events["recent"][1]["pressed"], json!([83]));
    assert_eq!(events["recent"][2]["released"], json!([83]));
    assert_eq!(events["actions_enabled"], false);
    assert!(setters(&state.borrow()).is_empty());
    assert!(scalar_setters(&state.borrow()).is_empty());
}
#[test]
fn validated_consumer_accepts_more_than_fifo_capacity_during_a_query() {
    let (mut backend, _, live) = fixture();
    backend.snapshot(deadline()).unwrap();
    live.borrow_mut()
        .queued
        .extend(vec![notification(&[]); 300]);
    let data = backend.snapshot(deadline()).unwrap();
    let events = &data["native_events"];
    assert_eq!(events["reports"], 300);
    assert_eq!(events["history_evicted"], 236);
    assert_eq!(events["recent"].as_array().unwrap().len(), 64);
    assert_eq!(events["recent"][0]["phase"], "snapshot");
    assert_eq!(events["active"], true);
}
#[test]
fn report_flood_closes_the_unique_transport_before_returning_error() {
    let (mut backend, _, live) = fixture();
    backend.snapshot(deadline()).unwrap();
    live.borrow_mut()
        .queued
        .extend(vec![notification(&[]); 4097]);
    assert!(backend.snapshot(deadline()).unwrap_err().contains("budget"));
    assert!(live.borrow().dropped);
    assert_eq!(backend.event_status()["active"], false);
}
#[test]
fn layout_drift_invalidates_the_session_and_never_reopens_it() {
    let (mut backend, state, live) = fixture();
    backend.snapshot(deadline()).unwrap();
    state.borrow_mut().version = 4;
    assert!(backend
        .snapshot(deadline())
        .unwrap_err()
        .contains("layout changed"));
    assert!(live.borrow().dropped);
    let writes = state.borrow().wire.len();
    assert!(backend.snapshot(deadline()).is_err());
    assert_eq!(state.borrow().wire.len(), writes);
}
#[test]
fn invalid_idle_event_releases_the_reader_and_does_not_forge_button_state() {
    let (mut backend, state, live) = fixture();
    backend.snapshot(deadline()).unwrap();
    let writes = state.borrow().wire.len();
    live.borrow_mut().queued.push_back(notification(&[80]));
    assert!(backend.poll(Duration::from_millis(5)).is_err());
    assert!(live.borrow().dropped);
    assert_eq!(backend.event_status()["events"], 0);
    assert_eq!(state.borrow().wire.len(), writes);
}
#[test]
fn cancellation_prevents_new_getters_and_setters_in_an_initialized_session() {
    let (device, state) = Device::new();
    let flag = Rc::new(Cell::new(false));
    let callback = flag.clone();
    let mut backend = NativeProfileBackend::with_cancellation(
        Session::new(device),
        1,
        "11223344",
        Cancellation::new(move || callback.get()),
    )
    .unwrap();
    backend.snapshot(deadline()).unwrap();
    let writes = state.borrow().wire.len();
    flag.set(true);
    let changes = Changes::new(Some(json!({"dpi":1800})), None).unwrap();
    assert!(backend
        .apply(&changes, deadline())
        .unwrap_err()
        .message
        .contains("disconnected"));
    assert_eq!(state.borrow().wire.len(), writes);
    assert_eq!(backend.event_status()["active"], false);
}
#[test]
fn initialization_fifo_is_delivered_after_identity_validation_and_tagged_as_old() {
    let (device, _) = Device::new();
    let wire = Interleaved::new(
        device,
        [((0, 1, 1), vec![notification(&[83]), notification(&[])])].into(),
    );
    let mut backend = NativeProfileBackend::new(Session::new(wire), 1, "11223344").unwrap();
    let data = backend.snapshot(deadline()).unwrap();
    let events = &data["native_events"];
    assert_eq!(events["events"], 2);
    assert_eq!(events["recent"][0]["phase"], "initialization");
    assert_eq!(events["recent"][1]["queued_before_attachment"], true);
    assert_eq!(events["recent"][0]["pressed"], json!([]));
}
#[test]
fn events_between_remap_ack_and_readback_keep_native_confirmation_intact() {
    let (mut device, state) = Device::new();
    device.base.accept_writes = true;
    let wire = Interleaved::new(
        device,
        [((18, 2, 7), vec![notification(&[]), notification(&[83])])].into(),
    );
    let mut backend = NativeProfileBackend::new(Session::new(wire), 1, "11223344").unwrap();
    let changes = Changes::new(None, Some(json!({"83":86}))).unwrap();
    let receipt = backend.apply(&changes, deadline()).unwrap();
    assert_eq!(receipt.confirmed, vec!["button:83"]);
    assert_eq!(receipt.snapshot["native_events"]["events"], 2);
    assert_eq!(
        receipt.snapshot["native_events"]["recent"][0]["phase"],
        "apply"
    );
    assert_eq!(setters(&state.borrow()).len(), 1);
    assert_eq!(setters(&state.borrow())[0][6], 0);
}

#[test]
fn private_client_eof_cancels_before_the_next_hid_request_or_setter() {
    use crate::native::guardian::{peer_gone, Channel};
    use std::os::unix::net::UnixStream;
    let (peer, socket) = UnixStream::pair().unwrap();
    let channel = Channel::new(socket).unwrap();
    let fd = channel.fd();
    let cancelled = Cancellation::new(move || peer_gone(fd));
    let init = cancelled.clone();
    let (device, state) = Device::new();
    let session = Session::with_initialization_cancellation(device, move || init.check());
    let mut backend =
        NativeProfileBackend::with_cancellation(session, 1, "11223344", cancelled).unwrap();
    backend.snapshot(deadline()).unwrap();
    let writes = state.borrow().wire.len();
    drop(peer);
    let changes = Changes::new(Some(json!({"dpi":1800})), None).unwrap();
    assert!(backend
        .apply(&changes, deadline())
        .unwrap_err()
        .message
        .contains("disconnected"));
    assert_eq!(state.borrow().wire.len(), writes);
    assert!(scalar_setters(&state.borrow()).is_empty());
}
#[test]
fn cancellation_before_initialization_never_sends_a_hid_request() {
    let (device, state) = Device::new();
    let cancelled = Cancellation::new(|| true);
    let init = cancelled.clone();
    let session = Session::with_initialization_cancellation(device, move || init.check());
    let mut backend =
        NativeProfileBackend::with_cancellation(session, 1, "11223344", cancelled).unwrap();
    assert!(backend
        .snapshot(deadline())
        .unwrap_err()
        .contains("cancelled"));
    assert!(state.borrow().wire.is_empty());
}

#[test]
fn native_only_capabilities_do_not_require_diversion_support_to_read_or_remap() {
    let (device, state) = Device::new();
    for row in &mut state.borrow_mut().table {
        row[4] &= !0x20;
    }
    let mut backend = backend(device);
    assert_eq!(backend.snapshot(deadline()).unwrap()["id"], "11223344");
    let changes = Changes::new(None, Some(json!({"83":86}))).unwrap();
    assert_eq!(
        backend.apply(&changes, deadline()).unwrap().confirmed,
        vec!["button:83"]
    );
    assert_eq!(setters(&state.borrow())[0][6], 0);
}

#[test]
fn consumer_failure_after_a_remap_preserves_uncertainty_without_retry_or_reopen() {
    let (device, state) = Device::new();
    let wire = Interleaved::new(device, [((18, 3, 1), vec![notification(&[80])])].into());
    let mut backend = NativeProfileBackend::new(Session::new(wire), 1, "11223344").unwrap();
    let changes = Changes::new(None, Some(json!({"83":86}))).unwrap();
    let error = backend.apply(&changes, deadline()).unwrap_err();
    assert!(error.confirmed.is_empty());
    assert_eq!(error.attempted.as_deref(), Some("button:83"));
    assert_eq!(setters(&state.borrow()).len(), 1);
    assert_eq!(state.borrow().targets[&83], 86);
    assert_eq!(backend.event_status()["active"], false);
    let writes = state.borrow().wire.len();
    assert!(backend.snapshot(deadline()).is_err());
    assert_eq!(state.borrow().wire.len(), writes);
}
