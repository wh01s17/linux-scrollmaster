// SPDX-License-Identifier: GPL-3.0-or-later
//! No /proc, runtime lease, discovery, HID or compositor calls in these tests.
use super::*;
use crate::{
    profiles::{Document, ProfileStore},
    service::ProfileService,
};
use std::{
    cell::RefCell,
    fs,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

const ID: &str = "11223344";
struct State {
    events: Vec<&'static str>,
    online: bool,
    lease: bool,
    opened: bool,
    blocked: bool,
    deny_lease: bool,
    block_after_acquire: bool,
    block_after_connect: bool,
    declared: String,
    actual: String,
    fail_prepare: bool,
    fail_apply: bool,
    fail_close: bool,
    pending_recovery: bool,
    bad_receipt: bool,
    late_snapshot: bool,
    late_prepare: bool,
    late_apply: bool,
    dpi: u16,
    deadlines: Vec<Instant>,
    selections: Vec<(u8, String)>,
}
type Shared = Rc<RefCell<State>>;
struct Connector(Shared);
struct Lease(Shared);
struct Backend(Shared, String);
fn fixture() -> (NativeProfileOwner<Connector>, Shared) {
    let state = Rc::new(RefCell::new(State {
        events: vec![],
        online: true,
        lease: false,
        opened: false,
        blocked: false,
        deny_lease: false,
        block_after_acquire: false,
        block_after_connect: false,
        declared: ID.into(),
        actual: ID.into(),
        fail_prepare: false,
        fail_apply: false,
        fail_close: false,
        pending_recovery: false,
        bad_receipt: false,
        late_snapshot: false,
        late_prepare: false,
        late_apply: false,
        dpi: 1850,
        deadlines: vec![],
        selections: vec![],
    }));
    (
        NativeProfileOwner::new(Connector(state.clone()), 2, ID).unwrap(),
        state,
    )
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(1)
}
fn changes(dpi: u16) -> Changes {
    Changes::new(Some(json!({"dpi":dpi})), None).unwrap()
}
fn snapshot(s: &State) -> Value {
    json!({"id":s.actual,"settings":{"dpi":{"available":true,"value":s.dpi}}})
}
fn late(enabled: bool, deadline: Instant) {
    if enabled {
        std::thread::sleep(
            deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(1),
        );
    }
}
impl ProfileConnector for Connector {
    type Backend = Backend;
    type Lease = Lease;
    fn check_controllers(&mut self) -> Result<(), String> {
        let mut s = self.0.borrow_mut();
        s.events.push("guard");
        if s.blocked {
            Err("Synthetic other controller".into())
        } else {
            Ok(())
        }
    }
    fn acquire(&mut self) -> Result<Lease, String> {
        let mut s = self.0.borrow_mut();
        s.events.push("lease");
        assert!(!s.lease && !s.opened, "no concurrent lease/session");
        if s.deny_lease {
            return Err("Synthetic lease busy".into());
        }
        s.lease = true;
        s.blocked = s.block_after_acquire;
        Ok(Lease(self.0.clone()))
    }
    fn connect(&mut self, slot: u8, identity: &str, deadline: Instant) -> Result<Backend, String> {
        let mut s = self.0.borrow_mut();
        assert!(s.lease && !s.opened);
        s.events.push("connect");
        s.deadlines.push(deadline);
        s.selections.push((slot, identity.into()));
        if !s.online {
            return Err("Synthetic no device".into());
        }
        s.opened = true;
        s.blocked = s.block_after_connect;
        Ok(Backend(self.0.clone(), s.declared.clone()))
    }
    fn finish(&mut self, _: Instant) -> Result<(), String> {
        let mut s = self.0.borrow_mut();
        assert!(!s.lease && !s.opened, "finish runs after backend and lease");
        s.events.push("finish");
        if s.pending_recovery {
            Err("Synthetic recovery pending".into())
        } else {
            Ok(())
        }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let mut s = self.0.borrow_mut();
        assert!(
            !s.opened && s.lease,
            "descriptor must close BEFORE releasing lease"
        );
        s.events.push("release");
        s.lease = false;
    }
}
impl Drop for Backend {
    fn drop(&mut self) {
        let mut s = self.0.borrow_mut();
        assert!(s.lease && s.opened);
        s.events.push("close");
        s.opened = false;
    }
}
impl ProfileBackend for Backend {
    fn close(&mut self, _: Instant) -> Result<(), String> {
        let state = self.0.borrow();
        assert!(state.lease && state.opened);
        if state.fail_close {
            Err("Synthetic recovery unconfirmed".into())
        } else {
            Ok(())
        }
    }
    fn poll(&mut self, _: Duration) -> Result<bool, String> {
        let mut s = self.0.borrow_mut();
        s.events.push("poll");
        if s.online {
            Ok(false)
        } else {
            Err("Synthetic idle disconnect".into())
        }
    }
    fn identity(&self) -> &str {
        &self.1
    }
    fn snapshot(&mut self, deadline: Instant) -> Result<Value, String> {
        let mut s = self.0.borrow_mut();
        s.events.push("snapshot");
        s.deadlines.push(deadline);
        late(s.late_snapshot, deadline);
        if !s.online {
            return Err("Synthetic disconnected reader".into());
        }
        Ok(snapshot(&s))
    }
    fn prepare(&mut self, _: &Changes, deadline: Instant) -> Result<(), String> {
        let mut s = self.0.borrow_mut();
        s.events.push("prepare");
        s.deadlines.push(deadline);
        late(s.late_prepare, deadline);
        if s.fail_prepare {
            Err("Synthetic unsupported choice".into())
        } else {
            Ok(())
        }
    }
    fn apply(&mut self, changes: &Changes, deadline: Instant) -> Result<Applied, ApplyError> {
        let mut s = self.0.borrow_mut();
        s.events.push("apply");
        s.deadlines.push(deadline);
        s.dpi = changes.settings().unwrap()["dpi"].as_u64().unwrap() as u16;
        late(s.late_apply, deadline);
        if s.fail_apply {
            return Err(ApplyError {
                message: "Synthetic uncertain write".into(),
                confirmed: vec!["dpi".into()],
                attempted: Some("button:83".into()),
            });
        }
        let mut receipt = snapshot(&s);
        if s.bad_receipt {
            receipt["settings"]["dpi"]["value"] = json!(800);
        }
        Ok(Applied {
            confirmed: vec!["dpi".into()],
            snapshot: receipt,
        })
    }
}

#[test]
fn idle_poll_never_acquires_offline_and_preserves_a_valid_preparation() {
    let (mut owner, state) = fixture();
    assert!(!owner.poll(Duration::from_millis(5)).unwrap());
    assert!(state.borrow().events.is_empty());
    let end = deadline();
    owner.prepare(&changes(1800), end).unwrap();
    assert!(!owner.poll(Duration::from_millis(5)).unwrap());
    owner.apply(&changes(1800), end).unwrap();
    assert_eq!(state.borrow().dpi, 1800);
    assert_eq!(state.borrow().selections.len(), 1);
}
#[test]
fn unconfirmed_recovery_remains_terminal_after_backend_and_lease_are_dropped() {
    let (mut owner, state) = fixture();
    owner.snapshot(deadline()).unwrap();
    state.borrow_mut().fail_close = true;
    assert!(owner
        .close(deadline())
        .unwrap_err()
        .contains("recovery unconfirmed"));
    assert!(!state.borrow().lease && !state.borrow().opened);
    let connections = state.borrow().selections.len();
    assert!(owner
        .snapshot(deadline())
        .unwrap_err()
        .contains("recovery unconfirmed"));
    assert_eq!(state.borrow().selections.len(), connections);
    assert!(owner.close(deadline()).is_err());
}
#[test]
fn confirmed_cleanup_allows_only_a_later_request_to_reconnect() {
    let (mut owner, state) = fixture();
    owner.snapshot(deadline()).unwrap();
    state.borrow_mut().online = false;
    assert!(owner.poll(Duration::from_millis(5)).is_err());
    assert!(!state.borrow().lease && !state.borrow().opened);
    // The failed turn is not retried; idle polling never reopens either.
    assert!(!owner.poll(Duration::from_millis(5)).unwrap());
    assert_eq!(state.borrow().selections.len(), 1);
    state.borrow_mut().online = true;
    owner.snapshot(deadline()).unwrap();
    assert_eq!(state.borrow().selections.len(), 2);
    owner.close(deadline()).unwrap();
}
#[test]
fn idle_failure_closes_before_lease_release_and_invalidates_preparation() {
    let (mut owner, state) = fixture();
    let end = deadline();
    owner.prepare(&changes(1800), end).unwrap();
    state.borrow_mut().online = false;
    assert!(owner.poll(Duration::from_millis(5)).is_err());
    assert!(state.borrow().events.ends_with(&["close", "release"]));
    assert!(owner.apply(&changes(1800), end).is_err());
    assert!(!state.borrow().events.contains(&"apply"));
    state.borrow_mut().online = true;
    owner.snapshot(deadline()).unwrap();
    assert_eq!(state.borrow().selections.len(), 2);
}
fn count(state: &Shared, event: &str) -> usize {
    state
        .borrow()
        .events
        .iter()
        .filter(|e| **e == event)
        .count()
}

#[test]
fn construction_and_status_are_offline_and_validate_explicit_selection() {
    let (owner, state) = fixture();
    assert_eq!(owner.status()["session_owned"], false);
    assert!(state.borrow().events.is_empty());
    assert!(NativeProfileOwner::system(2, ID).is_ok());
    for (slot, id) in [(0, ID), (7, ID), (2, "aabbccdd"), (2, "hidraw7"), (2, "")] {
        assert!(NativeProfileOwner::system(slot, id).is_err());
    }
}
#[test]
fn failed_connection_releases_lease_and_next_request_can_connect() {
    let (mut owner, state) = fixture();
    state.borrow_mut().online = false;
    assert!(owner.snapshot(deadline()).is_err());
    assert!(!state.borrow().lease);
    assert_eq!(count(&state, "connect"), 1);
    state.borrow_mut().online = true;
    assert_eq!(owner.snapshot(deadline()).unwrap()["id"], ID);
    assert_eq!(owner.status()["connection_attempts"], 2);
    assert_eq!(owner.status()["validated_connections"], 1);
    assert_eq!(owner.status()["last_error"], Value::Null);
    owner.snapshot(deadline()).unwrap();
    assert_eq!(count(&state, "connect"), 2);
    assert_eq!(
        count(&state, "snapshot"),
        2,
        "reuse handshake on first read, then read freshly"
    );
    assert_eq!(
        state.borrow().selections,
        vec![(2, ID.into()), (2, ID.into())]
    );
}
#[test]
fn disconnect_closes_before_releasing_and_drop_is_idempotent() {
    let (mut owner, state) = fixture();
    owner.snapshot(deadline()).unwrap();
    owner.disconnect();
    owner.disconnect();
    assert!(state.borrow().events.ends_with(&["close", "release"]));
    drop(owner);
    assert_eq!(count(&state, "close"), 1);
    assert!(!state.borrow().lease);
    let (mut owner, state) = fixture();
    owner.snapshot(deadline()).unwrap();
    drop(owner);
    assert!(state.borrow().events.ends_with(&["close", "release"]));
}
#[test]
fn disconnection_never_reopens_inside_the_failed_request() {
    let (mut owner, state) = fixture();
    owner.snapshot(deadline()).unwrap();
    state.borrow_mut().online = false;
    assert!(owner.snapshot(deadline()).is_err());
    assert_eq!(count(&state, "connect"), 1);
    assert!(state.borrow().events.ends_with(&["close", "release"]));
    state.borrow_mut().online = true;
    owner.snapshot(deadline()).unwrap();
    assert_eq!(count(&state, "connect"), 2);
}
#[test]
fn recycled_interface_with_wrong_unit_id_is_not_adopted_or_written() {
    let (mut owner, state) = fixture();
    state.borrow_mut().actual = "55667788".into();
    assert!(owner.prepare(&changes(1800), deadline()).is_err());
    assert_eq!(count(&state, "prepare"), 0);
    assert_eq!(count(&state, "apply"), 0);
    assert!(!state.borrow().lease);
    state.borrow_mut().actual = ID.into();
    owner.snapshot(deadline()).unwrap();
    state.borrow_mut().actual = "55667788".into();
    assert!(owner.snapshot(deadline()).is_err());
    assert!(!state.borrow().opened);
}
#[test]
fn controller_and_lease_conflicts_prevent_opening() {
    for busy_lease in [false, true] {
        let (mut owner, state) = fixture();
        state.borrow_mut().blocked = !busy_lease;
        state.borrow_mut().deny_lease = busy_lease;
        assert!(owner.snapshot(deadline()).is_err());
        assert_eq!(count(&state, "connect"), 0);
        assert!(!state.borrow().lease);
    }
}
#[test]
fn connector_cannot_substitute_the_selected_backend_identity() {
    let (mut owner, state) = fixture();
    state.borrow_mut().declared = "55667788".into();
    assert!(owner
        .snapshot(deadline())
        .unwrap_err()
        .contains("another identity"));
    assert_eq!(count(&state, "snapshot"), 0);
    assert!(state.borrow().events.ends_with(&["close", "release"]));
}
#[test]
fn controllers_appearing_during_acquisition_or_connect_abort_before_getters() {
    for during_connect in [false, true] {
        let (mut owner, state) = fixture();
        state.borrow_mut().block_after_acquire = !during_connect;
        state.borrow_mut().block_after_connect = during_connect;
        assert!(owner.snapshot(deadline()).is_err());
        assert_eq!(count(&state, "snapshot"), 0);
        assert_eq!(count(&state, "connect"), usize::from(during_connect));
        assert!(!state.borrow().lease);
    }
}
#[test]
fn controller_appearing_before_apply_closes_without_a_write_or_reconnect() {
    let (mut owner, state) = fixture();
    let end = deadline();
    owner.prepare(&changes(1800), end).unwrap();
    state.borrow_mut().blocked = true;
    assert!(owner.apply(&changes(1800), end).is_err());
    assert_eq!(count(&state, "apply"), 0);
    assert_eq!(count(&state, "connect"), 1);
    assert!(!state.borrow().lease);
}
#[test]
fn expired_or_late_handshake_is_not_a_connection_and_never_retried() {
    let (mut owner, state) = fixture();
    assert!(owner.snapshot(Instant::now()).is_err());
    assert!(state.borrow().events.is_empty());
    state.borrow_mut().late_snapshot = true;
    assert!(owner
        .snapshot(Instant::now() + Duration::from_millis(5))
        .is_err());
    assert_eq!(owner.status()["validated_connections"], 0);
    assert_eq!(count(&state, "connect"), 1);
    assert!(!state.borrow().lease);
}
#[test]
fn all_connection_and_hardware_phases_share_one_absolute_deadline() {
    let (mut owner, state) = fixture();
    let end = deadline();
    owner.prepare(&changes(1800), end).unwrap();
    assert_eq!(
        owner.apply(&changes(1800), end).unwrap().snapshot["settings"]["dpi"]["value"],
        1800
    );
    assert!(state.borrow().deadlines.iter().all(|d| *d == end));
    assert_eq!(count(&state, "connect"), 1);
    assert!(
        owner.apply(&changes(1800), end).is_err(),
        "preflight consumed once"
    );
    assert_eq!(count(&state, "apply"), 1);
}
#[test]
fn apply_without_matching_preflight_never_opens_or_writes() {
    let (mut owner, state) = fixture();
    assert!(owner.apply(&changes(1800), deadline()).is_err());
    assert!(state.borrow().events.is_empty());
    for wrong_deadline in [false, true] {
        let (mut owner, state) = fixture();
        let end = deadline();
        owner.prepare(&changes(1800), end).unwrap();
        let result = if wrong_deadline {
            owner.apply(&changes(1800), end + Duration::from_millis(1))
        } else {
            owner.apply(&changes(2400), end)
        };
        assert!(result.is_err());
        assert_eq!(count(&state, "apply"), 0);
        assert!(!state.borrow().lease);
    }
}
#[test]
fn intervening_read_or_disconnect_invalidates_preflight() {
    for disconnect in [false, true] {
        let (mut owner, state) = fixture();
        let end = deadline();
        owner.prepare(&changes(1800), end).unwrap();
        if disconnect {
            owner.disconnect();
        } else {
            owner.snapshot(end).unwrap();
        }
        assert!(owner.apply(&changes(1800), end).is_err());
        assert_eq!(count(&state, "apply"), 0);
        assert_eq!(count(&state, "connect"), 1);
    }
}
#[test]
fn software_assignments_are_rejected_without_lease_or_getters() {
    let (mut owner, state) = fixture();
    let changes = Changes::new(
        Some(json!({"dpi":1800})),
        Some(json!({"83":"modifier:ctrl"})),
    )
    .unwrap();
    assert!(owner
        .prepare(&changes, deadline())
        .unwrap_err()
        .contains("guardian"));
    assert!(state.borrow().events.is_empty());
}
#[test]
fn failed_or_late_preflight_closes_and_cannot_authorize_an_apply() {
    for late_prepare in [false, true] {
        let (mut owner, state) = fixture();
        state.borrow_mut().fail_prepare = !late_prepare;
        state.borrow_mut().late_prepare = late_prepare;
        let end = Instant::now() + Duration::from_millis(20);
        assert!(owner.prepare(&changes(1800), end).is_err());
        assert!(owner.apply(&changes(1800), end).is_err());
        assert_eq!(count(&state, "apply"), 0);
        assert!(!state.borrow().lease);
    }
}
#[test]
fn uncertain_mutation_preserves_history_closes_and_has_no_internal_retry() {
    let (mut owner, state) = fixture();
    let end = deadline();
    owner.prepare(&changes(1800), end).unwrap();
    state.borrow_mut().fail_apply = true;
    let error = owner.apply(&changes(1800), end).unwrap_err();
    assert_eq!(error.confirmed, vec!["dpi"]);
    assert_eq!(error.attempted.as_deref(), Some("button:83"));
    assert_eq!(count(&state, "apply"), 1);
    assert_eq!(count(&state, "connect"), 1);
    assert!(!state.borrow().lease);
    assert!(owner.apply(&changes(1800), end).is_err());
    assert_eq!(count(&state, "connect"), 1);
    state.borrow_mut().fail_apply = false;
    assert_eq!(
        owner.snapshot(deadline()).unwrap()["settings"]["dpi"]["value"],
        1800
    );
    assert_eq!(
        count(&state, "apply"),
        1,
        "next request reads without replaying the mutation"
    );
}
#[test]
fn late_or_false_success_receipts_are_rejected_with_historical_confirmation() {
    for late_apply in [false, true] {
        let (mut owner, state) = fixture();
        state.borrow_mut().bad_receipt = !late_apply;
        state.borrow_mut().late_apply = late_apply;
        let end = Instant::now() + Duration::from_millis(20);
        owner.prepare(&changes(1800), end).unwrap();
        let error = owner.apply(&changes(1800), end).unwrap_err();
        assert_eq!(error.confirmed, vec!["dpi"]);
        assert_eq!(count(&state, "apply"), 1);
        assert!(!state.borrow().lease);
    }
}

struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "scrollmaster-profile-owner-{}-{}",
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
#[test]
fn offline_profile_service_keeps_local_management_available_and_reconciles_after_reconnect() {
    let dir = Sandbox::new();
    let store = dir.store();
    let document = Document::from_value(
        json!({"version":1,"profiles":[{"device":ID,"name":"Work","settings":{"dpi":1800}}],"active":{}}),
    )
    .unwrap();
    store.commit(&store.load().unwrap(), &document).unwrap();
    let (owner, state) = fixture();
    state.borrow_mut().online = false;
    let mut service = ProfileService::new(store, owner, Duration::from_secs(1)).unwrap();
    service
        .handle(&json!({"op":"auto","device":ID,"name":"Work"}), None)
        .unwrap();
    let exported = service
        .handle(&json!({"op":"export","device":ID,"name":"Work"}), None)
        .unwrap();
    assert!(!exported["profiles"].as_array().unwrap().is_empty());
    assert!(
        state.borrow().events.is_empty(),
        "local operations must not acquire/open/probe"
    );
    let offline = service.handle(&json!({"op":"snapshot"}), None).unwrap();
    assert!(offline["devices"].as_array().unwrap().is_empty());
    assert_eq!(offline["active"][ID], "Work");
    assert!(!offline["warnings"].as_array().unwrap().is_empty());
    state.borrow_mut().online = true;
    let online = service.handle(&json!({"op":"snapshot"}), None).unwrap();
    assert_eq!(online["devices"][0]["settings"]["dpi"]["value"], 1800);
    assert_eq!(online["reconciliation"][0]["profile"], "Work");
    service
        .handle(
            &json!({"op":"set","device":ID,"changes":{"dpi":2400}}),
            None,
        )
        .unwrap();
    assert!(dir.store().load().unwrap().document().as_value()["active"]
        .get(ID)
        .is_none());
    state.borrow_mut().online = false;
    assert!(
        service.handle(&json!({"op":"snapshot"}), None).unwrap()["devices"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    state.borrow_mut().online = true;
    assert_eq!(
        service.handle(&json!({"op":"snapshot"}), None).unwrap()["devices"][0]["settings"]["dpi"]
            ["value"],
        2400
    );
    assert_eq!(count(&state, "connect"), 3);
    drop(service);
    assert!(!state.borrow().lease);
}

#[test]
fn close_reports_a_recovery_still_pending_in_the_connector() {
    let (mut owner, state) = fixture();
    owner.snapshot(deadline()).unwrap();
    owner.close(deadline()).unwrap();
    assert_eq!(state.borrow().events.last(), Some(&"finish"));
    owner.snapshot(deadline()).unwrap();
    state.borrow_mut().pending_recovery = true;
    // Shutdown must not be acknowledged while the device may stay diverted.
    assert_eq!(
        owner.close(deadline()).unwrap_err(),
        "Synthetic recovery pending"
    );
}

#[test]
fn resident_poll_does_not_scan_controllers_inside_its_short_slice() {
    let (mut owner, state) = fixture();
    owner.snapshot(deadline()).unwrap();
    state.borrow_mut().events.clear();
    for _ in 0..3 {
        assert!(!owner.poll(Duration::from_millis(5)).unwrap());
    }
    assert!(!state.borrow().events.contains(&"guard"));
    // Requests still check for competing controllers before touching HID.
    state.borrow_mut().blocked = true;
    assert!(owner.snapshot(deadline()).is_err());
}
