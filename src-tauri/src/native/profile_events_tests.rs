// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use std::cell::Cell;
fn snapshot() -> Value {
    json!({"feature":{"index":18,"version":5},"catalogue_count":7,"controls":[
        {"id":"83","capabilities":{"flags":49},"choices":[{"value":83}]},
        {"id":"86","capabilities":{"flags":49},"choices":[{"value":86}]},
        {"id":"195","capabilities":{"flags":305},"choices":[{"value":195}]}
    ]})
}
fn report(slot: u8, index: u8, function: u8, payload: &[u8]) -> Report {
    let mut bytes = vec![0; 20];
    bytes[..4].copy_from_slice(&[0x11, slot, index, function]);
    bytes[4..4 + payload.len()].copy_from_slice(payload);
    Report::decode(&bytes).unwrap()
}
fn buttons(cids: &[u16]) -> Report {
    let bytes: Vec<_> = cids.iter().flat_map(|cid| cid.to_be_bytes()).collect();
    report(1, 18, 0, &bytes)
}
fn events(queued: usize) -> ProfileEvents {
    ProfileEvents::new(&snapshot(), 1, queued, Cancellation::default()).unwrap()
}
#[test]
fn initialization_history_is_distinct_and_edges_start_from_a_synchronization() {
    let mut events = events(1);
    events.consume(&buttons(&[83])).unwrap();
    events.begin("snapshot");
    events.consume(&buttons(&[])).unwrap();
    let data = events.describe(true);
    assert_eq!(data["recent"][0]["phase"], "initialization");
    assert_eq!(data["recent"][0]["queued_before_attachment"], true);
    assert_eq!(data["recent"][0]["initial"], true);
    assert_eq!(data["recent"][0]["pressed"], json!([]));
    assert_eq!(data["recent"][1]["phase"], "snapshot");
    assert_eq!(data["recent"][1]["released"], json!([83]));
}
#[test]
fn history_is_bounded_while_a_resident_session_can_run_for_many_turns() {
    let mut events = events(0);
    for _ in 0..5000 {
        events.begin("idle");
        events.checkpoint().unwrap();
        events.consume(&buttons(&[])).unwrap();
    }
    let data = events.describe(true);
    assert_eq!(data["reports"], 5000);
    assert_eq!(data["events"], 5000);
    assert_eq!(data["recent"].as_array().unwrap().len(), 64);
    assert_eq!(data["history_evicted"], 4936);
    assert_eq!(data["recent"][0]["sequence"], 4937);
    assert_eq!(data["recent"][63]["sequence"], 5000);
    assert_eq!(data["active"], true);
    assert_eq!(data["actions_enabled"], false);
    assert_eq!(data["diversion_enabled_by_backend"], false);
}
#[test]
fn a_single_turn_has_a_finite_work_budget_and_an_error_stays_terminal() {
    let mut events = events(0);
    events.begin("apply");
    for _ in 0..REPORTS_PER_TURN {
        events.checkpoint().unwrap();
        events.consume(&buttons(&[])).unwrap();
    }
    assert!(events.checkpoint().unwrap_err().contains("budget"));
    events.begin("idle");
    assert!(events.checkpoint().is_err());
    assert_eq!(events.describe(true)["active"], false);
}
#[test]
fn cancellation_is_shared_and_releases_future_checkpoints() {
    let flag = Rc::new(Cell::new(false));
    let callback = flag.clone();
    let cancellation = Cancellation::new(move || callback.get());
    let mut events = ProfileEvents::new(&snapshot(), 1, 0, cancellation.clone()).unwrap();
    events.checkpoint().unwrap();
    flag.set(true);
    assert!(cancellation.check());
    assert!(events.checkpoint().unwrap_err().contains("disconnected"));
    flag.set(false);
    assert!(events.checkpoint().is_err());
}
#[test]
fn unrelated_reports_are_counted_without_entering_mouse_history() {
    let mut events = events(0);
    for notification in [
        report(2, 18, 0, &[]),
        report(1, 17, 0, &[]),
        report(1, 18, 8, &[]),
        report(1, 18, 0x20, &[]),
    ] {
        events.consume(&notification).unwrap();
    }
    let data = events.describe(true);
    assert_eq!(data["reports"], 4);
    assert_eq!(data["ignored"], 4);
    assert_eq!(data["events"], 0);
    assert_eq!(data["recent"], json!([]));
}
#[test]
fn invalid_cids_fail_without_becoming_valid_history() {
    let mut events = events(0);
    assert!(events.consume(&buttons(&[80])).is_err());
    let data = events.describe(true);
    assert_eq!(data["active"], false);
    assert_eq!(data["events"], 0);
    assert!(events.checkpoint().is_err());
}
#[test]
fn raw_xy_uses_known_gesture_source_and_preserves_signed_motion() {
    let mut events = events(0);
    let xy = report(1, 18, 0x10, &[0xff, 0xfe, 0, 3]);
    events.consume(&xy).unwrap();
    assert_eq!(events.describe(true)["events"], 0);
    events.consume(&buttons(&[195])).unwrap();
    events.consume(&xy).unwrap();
    let data = events.describe(true);
    assert_eq!(data["recent"][1]["dx"], -2);
    assert_eq!(data["recent"][1]["dy"], 3);
}
#[test]
fn native_mapping_changes_preserve_decoder_but_layout_changes_are_terminal() {
    let stream = events(0);
    let mut remapped = snapshot();
    remapped["controls"][0]["reported_target"] = json!(86);
    stream.validate_snapshot(&remapped).unwrap();
    for field in ["feature", "controls", "catalogue_count"] {
        let events = events(0);
        let mut changed = snapshot();
        changed[field] = Value::Null;
        assert!(events.validate_snapshot(&changed).is_err());
        assert_eq!(events.describe(true)["active"], false);
    }
}
