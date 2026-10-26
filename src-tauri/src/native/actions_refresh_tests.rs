// SPDX-License-Identifier: GPL-3.0-or-later
//! Physical gesture output while the single reader is refreshing its snapshot.
use super::*;

fn snapshot(dpi: u16) -> Value {
    json!({"feature":{"version":5},
        "snapshot":{"device":{"settings":{"dpi":{"value":dpi}}}},
        "controls":[{"id":"195","capabilities":{"flags":0x131}}]})
}
fn pending_gesture() -> (Runtime<Mock>, Rc<RefCell<Log>>) {
    let (mut runtime, log) = fixture(json!({"195":gesture("tenths-mm",10)}), Some(1000));
    press(&mut runtime);
    runtime.before_query().unwrap();
    runtime.motion(100, 0, ms(2)).unwrap();
    runtime.buttons(&[], ms(3)).unwrap();
    assert!(log.borrow().events.is_empty());
    (runtime, log)
}

#[test]
fn refresh_confirms_gesture_output_once_and_only_with_the_same_dpi() {
    let (mut runtime, log) = pending_gesture();
    runtime.snapshot(&snapshot(1000)).unwrap();
    assert_eq!(log.borrow().events, [Emission::Tap(47, 4)]);
    runtime.before_query().unwrap();
    runtime.snapshot(&snapshot(1000)).unwrap();
    assert_eq!(log.borrow().events, [Emission::Tap(47, 4)]);
}

#[test]
fn changed_dpi_discards_provisional_output_and_requires_a_new_press() {
    let (mut runtime, log) = pending_gesture();
    runtime.buttons(&[195], ms(4)).unwrap();
    runtime.motion(100, 0, ms(5)).unwrap();
    runtime.snapshot(&snapshot(1600)).unwrap();
    runtime.motion(100, 0, ms(6)).unwrap();
    runtime.buttons(&[], ms(7)).unwrap();
    assert!(log.borrow().events.is_empty());
    runtime.buttons(&[195], ms(8)).unwrap();
    runtime.motion(100, 0, ms(9)).unwrap();
    assert_eq!(log.borrow().events, [Emission::Tap(47, 4)]);
}

#[test]
fn invalid_snapshot_or_lost_dpi_closes_without_emitting_provisional_actions() {
    for invalid in [Value::Null, snapshot(0), {
        let mut snapshot = snapshot(1000);
        snapshot["feature"]["version"] = json!(99);
        snapshot
    }] {
        let (mut runtime, log) = pending_gesture();
        assert!(runtime.snapshot(&invalid).is_err());
        assert_eq!(log.borrow().events, [Emission::Close]);
        assert!(runtime.snapshot(&snapshot(1000)).is_err());
        assert_eq!(log.borrow().events, [Emission::Close]);
    }
}

#[test]
fn cancellation_close_and_reassignment_discard_provisional_output() {
    for operation in 0..3 {
        let (mut runtime, log) = pending_gesture();
        match operation {
            0 => runtime.cancel().unwrap(),
            1 => runtime.close().unwrap(),
            _ => runtime.replace_bindings(bindings(json!({}))).unwrap(),
        }
        if operation == 1 {
            assert!(runtime.before_query().is_err());
        }
        let _ = runtime.snapshot(&snapshot(1000));
        assert!(!log
            .borrow()
            .events
            .iter()
            .any(|e| matches!(e, Emission::Tap(..))));
    }
}

#[test]
fn failed_provisional_output_is_consumed_without_replaying_the_rest() {
    let (mut runtime, log) = pending_gesture();
    runtime.buttons(&[195], ms(4)).unwrap();
    runtime.motion(100, 0, ms(5)).unwrap();
    runtime.buttons(&[], ms(6)).unwrap();
    log.borrow_mut().fail_tap = true;
    assert!(runtime.snapshot(&snapshot(1000)).is_err());
    assert_eq!(log.borrow().events, [Emission::Tap(47, 4), Emission::Close]);
    log.borrow_mut().fail_tap = false;
    assert!(runtime.snapshot(&snapshot(1000)).is_err());
    assert_eq!(log.borrow().events, [Emission::Tap(47, 4), Emission::Close]);
}

#[test]
fn provisional_volume_output_has_a_finite_budget() {
    let (mut runtime, log) = fixture(json!({"195":gesture("tenths-mm",10)}), Some(1000));
    press(&mut runtime);
    runtime.before_query().unwrap();
    for _ in 0..PENDING_GESTURE_ACTION_LIMIT {
        runtime.motion(0, -100, ms(2)).unwrap();
    }
    assert!(log.borrow().events.is_empty());
    assert!(runtime
        .motion(0, -100, ms(2))
        .unwrap_err()
        .contains("budget"));
    assert_eq!(log.borrow().events, [Emission::Close]);
}
