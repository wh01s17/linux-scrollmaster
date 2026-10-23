// SPDX-License-Identifier: GPL-3.0-or-later
//! Reconnecting owner + real native backend + hand-authored HID++ protocol.
use super::*;
use crate::native::{NativeProfileOwner, ProfileConnector};
use std::collections::VecDeque;

#[derive(Default)]
struct Lifecycle {
    lease: bool,
    readers: usize,
    opens: usize,
    disconnected: bool,
    wrong_identity: bool,
    events: Vec<&'static str>,
}
type Shared = Rc<RefCell<Lifecycle>>;
struct Lease(Shared);
impl Drop for Lease {
    fn drop(&mut self) {
        let mut s = self.0.borrow_mut();
        assert_eq!(
            s.readers, 0,
            "release only after Session dropped the transport"
        );
        assert!(s.lease);
        s.lease = false;
        s.events.push("release");
    }
}
struct Tracked {
    device: Device,
    lifecycle: Shared,
    identity_query: bool,
}
impl Drop for Tracked {
    fn drop(&mut self) {
        let mut s = self.lifecycle.borrow_mut();
        assert!(s.lease);
        assert_eq!(s.readers, 1);
        s.readers = 0;
        s.events.push("close");
    }
}
impl Transport for Tracked {
    fn write_report(&mut self, p: &[u8], end: Instant) -> io::Result<()> {
        if self.lifecycle.borrow().disconnected {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        self.identity_query = p[2] == 2 && p[3] >> 4 == 0;
        self.device.write_report(p, end)
    }
    fn read_report(&mut self, end: Instant) -> io::Result<Vec<u8>> {
        let mut bytes = self.device.read_report(end)?;
        if self.identity_query && self.lifecycle.borrow().wrong_identity && bytes.len() >= 9 {
            bytes[5..9].copy_from_slice(&[0x55, 0x66, 0x77, 0x88]);
        }
        Ok(bytes)
    }
}
struct Connector {
    devices: VecDeque<Device>,
    lifecycle: Shared,
}
impl ProfileConnector for Connector {
    type Backend = NativeProfileBackend<Tracked>;
    type Lease = Lease;
    fn check_controllers(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn acquire(&mut self) -> Result<Lease, String> {
        let mut s = self.lifecycle.borrow_mut();
        assert!(!s.lease && s.readers == 0);
        s.lease = true;
        Ok(Lease(self.lifecycle.clone()))
    }
    fn connect(&mut self, slot: u8, identity: &str, _: Instant) -> Result<Self::Backend, String> {
        let mut device = self.devices.pop_front().ok_or("No synthetic receiver")?;
        device.base.accept_writes = true;
        let mut s = self.lifecycle.borrow_mut();
        assert!(s.lease && s.readers == 0);
        s.readers = 1;
        s.opens += 1;
        NativeProfileBackend::new(
            Session::new(Tracked {
                device,
                lifecycle: self.lifecycle.clone(),
                identity_query: false,
            }),
            slot,
            identity,
        )
    }
}
fn fixture(devices: Vec<Device>) -> (NativeProfileOwner<Connector>, Shared) {
    let lifecycle = Rc::new(RefCell::new(Lifecycle::default()));
    let owner = NativeProfileOwner::new(
        Connector {
            devices: devices.into(),
            lifecycle: lifecycle.clone(),
        },
        1,
        "11223344",
    )
    .unwrap();
    (owner, lifecycle)
}

#[test]
fn native_session_reconnects_after_read_failure_without_two_readers_or_cached_state() {
    let (device1, state1) = Device::new();
    let (device2, state2) = Device::new();
    state2.borrow_mut().targets.insert(83, 80);
    let (mut owner, lifecycle) = fixture(vec![device1, device2]);
    assert_eq!(
        owner.snapshot(deadline()).unwrap()["buttons"]["controls"][0]["value"],
        83
    );
    lifecycle.borrow_mut().disconnected = true;
    assert!(owner.snapshot(deadline()).is_err());
    assert_eq!(lifecycle.borrow().opens, 1);
    assert!(lifecycle.borrow().events.ends_with(&["close", "release"]));
    lifecycle.borrow_mut().disconnected = false;
    assert_eq!(
        owner.snapshot(deadline()).unwrap()["buttons"]["controls"][0]["value"],
        80
    );
    assert_eq!(lifecycle.borrow().opens, 2);
    assert!(setters(&state1.borrow()).is_empty());
    assert!(setters(&state2.borrow()).is_empty());
    drop(owner);
    assert!(!lifecycle.borrow().lease);
}
#[test]
fn native_handshake_rejects_changed_unit_id_before_catalogue_or_setters() {
    let (device, state) = Device::new();
    let (mut owner, lifecycle) = fixture(vec![device]);
    lifecycle.borrow_mut().wrong_identity = true;
    let changes = Changes::new(Some(json!({"dpi":1800})), Some(json!({"83":80}))).unwrap();
    assert!(owner.prepare(&changes, deadline()).is_err());
    assert!(
        state.borrow().wire.iter().all(|p| p[2] != 18),
        "identity must match before button queries"
    );
    assert!(scalar_setters(&state.borrow()).is_empty());
    assert!(setters(&state.borrow()).is_empty());
    assert!(!lifecycle.borrow().lease);
}
#[test]
fn native_partial_write_closes_and_a_subsequent_snapshot_does_not_replay_it() {
    let (device1, state1) = Device::new();
    let (device2, state2) = Device::new();
    state1.borrow_mut().bad_ack = true;
    let (mut owner, lifecycle) = fixture(vec![device1, device2]);
    let changes = Changes::new(Some(json!({"dpi":1800})), Some(json!({"83":80}))).unwrap();
    let end = deadline();
    owner.prepare(&changes, end).unwrap();
    let error = owner.apply(&changes, end).unwrap_err();
    assert_eq!(error.confirmed, vec!["dpi"]);
    assert_eq!(error.attempted.as_deref(), Some("button:83"));
    assert_eq!(scalar_setters(&state1.borrow()).len(), 1);
    assert_eq!(setters(&state1.borrow()).len(), 1);
    assert_eq!(lifecycle.borrow().opens, 1);
    assert!(!lifecycle.borrow().lease);
    owner.snapshot(deadline()).unwrap();
    assert!(scalar_setters(&state2.borrow()).is_empty());
    assert!(setters(&state2.borrow()).is_empty());
    assert_eq!(lifecycle.borrow().opens, 2);
}
