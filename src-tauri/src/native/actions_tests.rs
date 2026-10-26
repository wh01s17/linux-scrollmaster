// SPDX-License-Identifier: GPL-3.0-or-later
#[path = "actions_refresh_tests.rs"]
mod refresh_tests;
use super::*;
use serde_json::json;
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Emission {
    Held(HeldModifiers),
    Tap(u16, u8),
    Close,
}
#[derive(Default)]
struct Log {
    events: Vec<Emission>,
    fail_held: bool,
    fail_tap: bool,
    fail_close: bool,
}
struct Mock(Rc<RefCell<Log>>);
impl Output for Mock {
    fn held(&mut self, held: HeldModifiers) -> Result<(), String> {
        let mut log = self.0.borrow_mut();
        log.events.push(Emission::Held(held));
        if log.fail_held {
            Err("held failed".into())
        } else {
            Ok(())
        }
    }
    fn tap(&mut self, action: &KeyboardAction) -> Result<(), String> {
        let mut log = self.0.borrow_mut();
        let (key, mask) = action.code();
        log.events.push(Emission::Tap(key, mask));
        if log.fail_tap {
            Err("tap failed".into())
        } else {
            Ok(())
        }
    }
    fn close(&mut self) -> Result<(), String> {
        let mut log = self.0.borrow_mut();
        log.events.push(Emission::Close);
        if log.fail_close {
            Err("close failed".into())
        } else {
            Ok(())
        }
    }
}
fn bindings(value: Value) -> ActionBindings {
    ActionBindings::parse(&value).unwrap()
}
fn gesture(units: &str, threshold: u16) -> Value {
    json!({"type":"gestures","units":units,"threshold":threshold,"actions":{
        "up":"media:volume-up","down":"media:volume-down","left":"shortcut:copy",
        "right":"shortcut:paste","click":"media:mute"}})
}
fn fixture(value: Value, dpi: Option<u16>) -> (Runtime<Mock>, Rc<RefCell<Log>>) {
    let log = Rc::new(RefCell::new(Log::default()));
    (
        Runtime::new(Mock(log.clone()), bindings(value), dpi).unwrap(),
        log,
    )
}
fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}
fn press(runtime: &mut Runtime<Mock>) {
    runtime.buttons(&[], ms(0)).unwrap();
    runtime.buttons(&[195], ms(1)).unwrap();
}

#[test]
fn bindings_accept_existing_keyboard_ids_and_legacy_gesture_units() {
    for action in [
        "modifier:ctrl",
        "modifier:alt",
        "shortcut:copy",
        "media:next",
        "custom:ctrl+shift:KeyS",
    ] {
        assert!(ActionBindings::parse(&json!({"83":action,"86":action})).is_ok());
    }
    for (units, lower, upper) in [("counts", 20, 2000), ("tenths-mm", 10, 200)] {
        for threshold in [lower, upper] {
            assert!(ActionBindings::parse(&json!({"195":gesture(units, threshold)})).is_ok());
        }
    }
    let mut legacy = gesture("counts", 20);
    legacy.as_object_mut().unwrap().remove("units");
    assert_eq!(
        bindings(json!({"195":legacy})),
        bindings(json!({"195":gesture("counts",20)}))
    );
    let mut silent = gesture("counts", 20);
    for value in silent["actions"].as_object_mut().unwrap().values_mut() {
        *value = Value::Null;
    }
    assert!(ActionBindings::parse(&json!({"195":silent})).is_ok());
}
#[test]
fn malformed_bindings_cannot_smuggle_commands_or_unknown_fields() {
    for value in [
        Value::Null,
        json!([]),
        json!({"80":"modifier:ctrl"}),
        json!({"083":"modifier:ctrl"}),
        json!({"83":null}),
        json!({"83":gesture("counts",20)}),
        json!({"195":"shortcut:copy"}),
        json!({"86":"desktop:exec"}),
        json!({"83":"shell:echo hi"}),
    ] {
        assert!(ActionBindings::parse(&value).is_err(), "{value}");
    }
    for (key, value) in [
        ("type", json!("gesture")),
        ("threshold", json!(true)),
        ("threshold", json!(20.0)),
        ("threshold", json!(-1)),
        ("threshold", json!(19)),
        ("threshold", json!(2001)),
        ("units", json!(null)),
        ("units", json!("mm")),
        ("extra", json!(true)),
        ("actions", json!({})),
    ] {
        let mut config = gesture("counts", 20);
        config[key] = value;
        assert!(ActionBindings::parse(&json!({"195":config})).is_err());
    }
    for threshold in [9, 201] {
        assert!(ActionBindings::parse(&json!({"195":gesture("tenths-mm",threshold)})).is_err());
    }
    let mut config = gesture("counts", 20);
    config["actions"]["click"] = json!("modifier:ctrl");
    assert!(ActionBindings::parse(&json!({"195":config})).is_err());
    config["actions"]["click"] = Value::Null;
    config["actions"]["extra"] = Value::Null;
    assert!(ActionBindings::parse(&json!({"195":config})).is_err());
}
#[test]
fn initially_held_buttons_and_gesture_are_suppressed_until_their_release() {
    let (mut r, log) = fixture(
        json!({"83":"modifier:alt","86":"shortcut:copy","195":gesture("counts",20)}),
        None,
    );
    r.buttons(&[83, 86, 195], ms(0)).unwrap();
    r.motion(100, 0, ms(1)).unwrap();
    r.buttons(&[195, 86, 83], ms(2)).unwrap();
    r.buttons(&[], ms(3)).unwrap();
    assert!(log.borrow().events.is_empty());
    assert!(r.feedback.is_none());
    r.buttons(&[83, 86, 195], ms(4)).unwrap();
    r.buttons(&[], ms(5)).unwrap();
    assert_eq!(
        log.borrow().events,
        [
            Emission::Held(HeldModifiers::Alt),
            Emission::Tap(46, 4),
            Emission::Held(HeldModifiers::None),
            Emission::Tap(113, 0)
        ]
    );
}
#[test]
fn side_taps_are_edges_with_deterministic_order_and_no_repeats() {
    let (mut r, log) = fixture(json!({"83":"shortcut:copy","86":"shortcut:paste"}), None);
    r.buttons(&[], ms(0)).unwrap();
    for list in [&[86, 83][..], &[83, 86], &[86], &[86], &[], &[83]] {
        r.buttons(list, ms(1)).unwrap();
    }
    assert_eq!(
        log.borrow().events,
        [
            Emission::Tap(46, 4),
            Emission::Tap(47, 4),
            Emission::Tap(46, 4)
        ]
    );
}
#[test]
fn shared_modifiers_remain_held_until_last_bound_button_is_released() {
    let (mut r, log) = fixture(json!({"83":"modifier:ctrl","86":"modifier:ctrl"}), None);
    r.buttons(&[], ms(0)).unwrap();
    for list in [&[83][..], &[86, 83], &[86], &[]] {
        r.buttons(list, ms(1)).unwrap();
    }
    assert_eq!(
        log.borrow().events,
        [
            Emission::Held(HeldModifiers::Ctrl),
            Emission::Held(HeldModifiers::None)
        ]
    );
    r.replace_bindings(bindings(json!({"83":"modifier:ctrl","86":"modifier:alt"})))
        .unwrap();
    for list in [&[83, 86][..], &[83], &[]] {
        r.buttons(list, ms(2)).unwrap();
    }
    assert_eq!(
        &log.borrow().events[2..],
        [
            Emission::Held(HeldModifiers::CtrlAlt),
            Emission::Held(HeldModifiers::Ctrl),
            Emission::Held(HeldModifiers::None)
        ]
    );
}
#[test]
fn modifiers_are_updated_before_taps_without_losing_held_state() {
    let (mut r, log) = fixture(json!({"83":"shortcut:copy","86":"modifier:alt"}), None);
    r.buttons(&[], ms(0)).unwrap();
    r.buttons(&[83, 86], ms(1)).unwrap();
    r.buttons(&[86], ms(2)).unwrap();
    assert_eq!(r.held, HeldModifiers::Alt);
    assert_eq!(
        log.borrow().events,
        [Emission::Held(HeldModifiers::Alt), Emission::Tap(46, 4)]
    );
}
#[test]
fn cancel_releases_modifiers_cancels_gesture_and_resynchronizes_after_link_loss() {
    let (mut r, log) = fixture(
        json!({"83":"modifier:ctrl","86":"shortcut:copy","195":gesture("counts",20)}),
        None,
    );
    r.buttons(&[], ms(0)).unwrap();
    r.buttons(&[83, 195], ms(1)).unwrap();
    // Below the start distance (5^1.4 < 20): nothing ran before the cancel.
    r.motion(5, 0, ms(2)).unwrap();
    r.cancel().unwrap();
    r.cancel().unwrap();
    r.buttons(&[86, 195], ms(3)).unwrap(); // newly seen while disconnected: synchronization only
    r.buttons(&[], ms(4)).unwrap();
    assert!(r.feedback.is_none());
    assert_eq!(
        log.borrow().events,
        [
            Emission::Held(HeldModifiers::Ctrl),
            Emission::Held(HeldModifiers::None)
        ]
    );
    r.buttons(&[86], ms(5)).unwrap();
    assert_eq!(log.borrow().events.last(), Some(&Emission::Tap(46, 4)));
}
#[test]
fn changed_held_binding_is_released_and_suppressed_without_affecting_other_bindings() {
    let (mut r, log) = fixture(json!({"83":"modifier:ctrl","86":"modifier:alt"}), None);
    r.buttons(&[], ms(0)).unwrap();
    r.buttons(&[83, 86], ms(1)).unwrap();
    r.replace_bindings(bindings(json!({"83":"shortcut:copy","86":"modifier:alt"})))
        .unwrap();
    r.buttons(&[83, 86], ms(2)).unwrap();
    assert_eq!(
        log.borrow().events,
        [
            Emission::Held(HeldModifiers::CtrlAlt),
            Emission::Held(HeldModifiers::Alt)
        ]
    );
    r.buttons(&[86], ms(3)).unwrap();
    r.buttons(&[83, 86], ms(4)).unwrap();
    assert_eq!(log.borrow().events.last(), Some(&Emission::Tap(46, 4)));
    r.replace_bindings(ActionBindings::default()).unwrap();
    assert_eq!(
        log.borrow().events.last(),
        Some(&Emission::Held(HeldModifiers::None))
    );
}
#[test]
fn changing_gesture_while_held_never_turns_release_into_a_click() {
    let config = json!({"195":gesture("counts",20)});
    let (mut r, log) = fixture(config.clone(), None);
    press(&mut r);
    r.replace_bindings(ActionBindings::default()).unwrap();
    r.replace_bindings(bindings(config)).unwrap();
    r.motion(40, 0, ms(2)).unwrap();
    r.buttons(&[], ms(3)).unwrap();
    assert!(log.borrow().events.is_empty());
    assert!(r.feedback.is_none());
    r.buttons(&[195], ms(4)).unwrap();
    r.buttons(&[], ms(5)).unwrap();
    assert_eq!(log.borrow().events, [Emission::Tap(113, 0)]);
}
#[test]
fn five_directions_run_once_when_the_gesture_starts_with_correct_signs() {
    for (dx, dy, direction, key, mask) in [
        (0, -20, GestureDirection::Up, 115, 0),
        (0, 20, GestureDirection::Down, 114, 0),
        (-20, 0, GestureDirection::Left, 46, 4),
        (20, 0, GestureDirection::Right, 47, 4),
        (2, 2, GestureDirection::Click, 113, 0),
    ] {
        let (mut r, log) = fixture(json!({"195":gesture("counts",20)}), None);
        press(&mut r);
        r.motion(dx, dy, ms(2)).unwrap();
        // A gesture runs as soon as it starts; a click on release.
        let started = direction != GestureDirection::Click;
        assert_eq!(log.borrow().events.len(), usize::from(started));
        r.buttons(&[], ms(3)).unwrap();
        r.buttons(&[], ms(4)).unwrap();
        assert_eq!(log.borrow().events, [Emission::Tap(key, mask)]);
        assert_eq!(
            r.feedback,
            Some(GestureFeedback {
                sequence: 1,
                direction: Some(direction)
            })
        );
    }
}
#[test]
fn axis_changes_mid_gesture_but_reversal_and_return_never_repeat_or_click() {
    let (mut r, log) = fixture(json!({"195":gesture("counts",20)}), None);
    press(&mut r);
    r.motion(20, 0, ms(2)).unwrap();
    // Reversing on the same axis neither repeats nor runs the opposite action.
    r.motion(-80, 0, ms(3)).unwrap();
    assert_eq!(log.borrow().events, [Emission::Tap(47, 4)]);
    // The other axis dominating the movement since the last action takes over.
    r.motion(0, -80, ms(3)).unwrap();
    r.buttons(&[], ms(4)).unwrap();
    assert_eq!(
        log.borrow().events,
        [Emission::Tap(47, 4), Emission::Tap(115, 0)]
    );
    assert_eq!(r.feedback.unwrap().direction, Some(GestureDirection::Up));
}
#[test]
fn diagonal_dominance_boundary_and_ambiguous_feedback_are_stable() {
    // Speed weighting raises the ratio to 1.4: 24/20 dominates, 22/20 does not.
    for (dx, dy, expected) in [
        (24, 20, Some(GestureDirection::Right)),
        (22, 20, None),
        (20, 24, Some(GestureDirection::Down)),
    ] {
        let (mut r, log) = fixture(json!({"195":gesture("counts",20)}), None);
        press(&mut r);
        r.motion(dx, dy, ms(2)).unwrap();
        r.motion(-dx, -dy, ms(3)).unwrap();
        r.buttons(&[], ms(4)).unwrap();
        // Volume returns with the movement (one step down, one back up); the
        // reversal changes the reported direction. Other actions run once.
        let (events, last) = match expected {
            Some(GestureDirection::Down) => (2, Some(GestureDirection::Up)),
            Some(direction) => (1, Some(direction)),
            None => (0, None),
        };
        assert_eq!(r.feedback.unwrap().direction, last);
        assert_eq!(log.borrow().events.len(), events);
        r.buttons(&[195], ms(5)).unwrap();
        r.buttons(&[], ms(6)).unwrap();
        assert_eq!(r.feedback.unwrap().sequence, 2);
    }
}
#[test]
fn physical_distance_uses_integer_ceiling_and_speed_weighting_is_dpi_stable() {
    for dpi in [200, 800, 1600, 8000, 65535] {
        let config = GestureConfig::parse(&gesture("tenths-mm", 10)).unwrap();
        assert_eq!(
            config.counts(Some(dpi)).unwrap(),
            (10 * i32::from(dpi) + 253) / 254
        );
    }
    // At the 1000 DPI reference a 1-count report weighs exactly 1.
    let (mut r, log) = fixture(json!({"195":gesture("tenths-mm",10)}), Some(1000));
    press(&mut r);
    for _ in 0..39 {
        r.motion(1, 0, ms(2)).unwrap();
    }
    assert!(r.stroke.as_ref().unwrap().direction.is_none());
    r.motion(1, 0, ms(2)).unwrap();
    assert_eq!(log.borrow().events, [Emission::Tap(47, 4)]);
    // The same hand speed needs the same number of reports at any DPI.
    let reports = |dpi: u16| {
        let (mut r, _) = fixture(json!({"195":gesture("tenths-mm",30)}), Some(dpi));
        press(&mut r);
        (1..)
            .find(|_| {
                r.motion((dpi / 400) as i16, 0, ms(2)).unwrap();
                r.stroke.as_ref().unwrap().direction.is_some()
            })
            .unwrap()
    };
    assert_eq!(reports(800), reports(1600));
    assert_eq!(reports(800), reports(3200));
    // Fast reports need less travel than slow ones (speed weighting).
    let (mut r, _) = fixture(json!({"195":gesture("counts",40)}), None);
    press(&mut r);
    r.motion(14, 0, ms(2)).unwrap();
    assert!(r.stroke.as_ref().unwrap().direction.is_some());
}
#[test]
fn physical_dpi_changes_or_loss_cancel_until_release_but_same_dpi_is_safe() {
    for next in [Some(1600), None] {
        let (mut r, log) = fixture(json!({"195":gesture("tenths-mm",10)}), Some(800));
        press(&mut r);
        // In flight but not started yet (4 counts weigh about 7.6 < 32).
        r.motion(4, 0, ms(2)).unwrap();
        r.set_dpi(next).unwrap();
        r.set_dpi(Some(800)).unwrap();
        r.buttons(&[195], ms(3)).unwrap();
        r.motion(32, 0, ms(4)).unwrap();
        r.buttons(&[], ms(5)).unwrap();
        assert!(log.borrow().events.is_empty());
        r.buttons(&[195], ms(6)).unwrap();
        r.motion(32, 0, ms(7)).unwrap();
        r.set_dpi(Some(800)).unwrap();
        r.buttons(&[], ms(8)).unwrap();
        assert_eq!(log.borrow().events, [Emission::Tap(47, 4)]);
    }
}
#[test]
fn count_gesture_survives_dpi_changes_and_identical_binding_refresh() {
    let config = json!({"195":gesture("counts",20)});
    let (mut r, log) = fixture(config.clone(), None);
    press(&mut r);
    r.motion(20, 0, ms(2)).unwrap();
    r.set_dpi(Some(1600)).unwrap();
    r.set_dpi(None).unwrap();
    r.replace_bindings(bindings(config)).unwrap();
    r.buttons(&[], ms(3)).unwrap();
    assert_eq!(log.borrow().events, [Emission::Tap(47, 4)]);
}
#[test]
fn timeout_never_degrades_into_click_and_exact_ten_seconds_is_allowed() {
    for motion in [false, true] {
        let (mut r, log) = fixture(json!({"195":gesture("counts",20)}), None);
        press(&mut r);
        if motion {
            r.motion(40, 0, ms(10002)).unwrap();
        }
        r.buttons(&[], ms(10002)).unwrap();
        assert!(log.borrow().events.is_empty());
        assert!(r.feedback.is_none());
    }
    let (mut r, log) = fixture(json!({"195":gesture("counts",20)}), None);
    press(&mut r);
    r.buttons(&[], ms(10001)).unwrap();
    assert_eq!(log.borrow().events, [Emission::Tap(113, 0)]);
}
#[test]
fn motion_ignores_unpressed_and_initial_held_sources_and_saturates_safely() {
    let (mut r, log) = fixture(json!({"195":gesture("counts",20)}), None);
    r.motion(i16::MIN, i16::MAX, ms(0)).unwrap();
    r.buttons(&[195], ms(0)).unwrap();
    r.motion(100, 0, ms(1)).unwrap();
    r.buttons(&[], ms(2)).unwrap();
    assert!(log.borrow().events.is_empty());
    r.buttons(&[195], ms(3)).unwrap();
    // Each extreme report weighs about -2.1e6; the total saturates at -1e9.
    for _ in 0..1000 {
        r.motion(i16::MIN, 0, ms(4)).unwrap();
    }
    assert_eq!(r.stroke.as_ref().unwrap().x, -1e9);
    r.buttons(&[], ms(5)).unwrap();
    assert_eq!(log.borrow().events, [Emission::Tap(46, 4)]);
}
#[test]
fn modifier_failure_closes_before_any_simultaneous_tap_and_preserves_cleanup_error() {
    let (mut r, log) = fixture(json!({"83":"modifier:alt","86":"shortcut:copy"}), None);
    r.buttons(&[], ms(0)).unwrap();
    log.borrow_mut().fail_held = true;
    log.borrow_mut().fail_close = true;
    assert_eq!(r.buttons(&[83, 86], ms(1)).unwrap_err(), "held failed");
    assert_eq!(r.close().unwrap_err(), "close failed");
    assert!(r.buttons(&[], ms(2)).is_err());
    assert_eq!(r.error.as_deref(), Some("held failed"));
    drop(r);
    assert_eq!(
        log.borrow().events,
        [Emission::Held(HeldModifiers::Alt), Emission::Close]
    );
}
#[test]
fn failed_gesture_tap_is_consumed_before_close_and_never_retried() {
    let (mut r, log) = fixture(json!({"195":gesture("counts",20)}), None);
    press(&mut r);
    log.borrow_mut().fail_tap = true;
    assert_eq!(r.buttons(&[], ms(2)).unwrap_err(), "tap failed");
    assert!(r.stroke.is_none());
    assert!(r.buttons(&[], ms(3)).is_err());
    r.close().unwrap();
    drop(r);
    assert_eq!(
        log.borrow().events,
        [Emission::Tap(113, 0), Emission::Close]
    );
}
#[test]
fn invalid_control_lists_and_backwards_time_fail_closed_and_release_held_state() {
    for bad in [vec![83, 83], vec![80], vec![83, 86, 195, 83]] {
        let (mut r, log) = fixture(json!({"83":"modifier:ctrl"}), None);
        r.buttons(&[], ms(0)).unwrap();
        r.buttons(&[83], ms(1)).unwrap();
        assert!(r.buttons(&bad, ms(2)).is_err());
        assert_eq!(
            log.borrow().events,
            [Emission::Held(HeldModifiers::Ctrl), Emission::Close]
        );
    }
    let (mut r, log) = fixture(json!({"195":gesture("counts",20)}), None);
    press(&mut r);
    assert!(r.buttons(&[], ms(0)).unwrap_err().contains("backwards"));
    assert_eq!(log.borrow().events, [Emission::Close]);
}
#[test]
fn missing_or_invalid_dpi_is_never_replaced_with_a_default() {
    for dpi in [None, Some(0)] {
        let log = Rc::new(RefCell::new(Log::default()));
        assert!(Runtime::new(
            Mock(log.clone()),
            bindings(json!({"195":gesture("tenths-mm",10)})),
            dpi
        )
        .is_err());
        assert_eq!(log.borrow().events, [Emission::Close]);
    }
    let (mut r, log) = fixture(json!({"195":gesture("tenths-mm",10)}), Some(800));
    r.buttons(&[], ms(0)).unwrap();
    r.set_dpi(None).unwrap();
    assert!(r.buttons(&[195], ms(1)).is_err());
    assert_eq!(log.borrow().events, [Emission::Close]);
    let (mut r, log) = fixture(json!({}), None);
    assert!(r.set_dpi(Some(0)).is_err());
    assert_eq!(log.borrow().events, [Emission::Close]);
}
#[test]
fn silent_gesture_reports_feedback_and_drop_closes_exactly_once() {
    let mut config = gesture("counts", 20);
    config["actions"]["click"] = Value::Null;
    let (mut r, log) = fixture(json!({"83":"modifier:ctrl","195":config}), None);
    press(&mut r);
    r.buttons(&[83], ms(2)).unwrap();
    assert_eq!(r.feedback.unwrap().direction, Some(GestureDirection::Click));
    drop(r);
    assert_eq!(
        log.borrow().events,
        [Emission::Held(HeldModifiers::Ctrl), Emission::Close]
    );
}
#[test]
fn hand_authored_hid_frames_decode_into_single_taps_and_a_signed_gesture() {
    use crate::{hidpp::Report, native::events::Decoder};
    let snapshot = json!({"feature":{"index":18},"controls":[
        {"id":"83","capabilities":{"flags":0x31}},
        {"id":"195","capabilities":{"flags":0x131}}]});
    let mut decoder = Decoder::from_snapshot(&snapshot, 2).unwrap();
    let (mut r, log) = fixture(
        json!({"83":"shortcut:copy","195":gesture("counts",20)}),
        None,
    );
    // Actual 1B04 frames; stale initial hold and a response with software ID 8
    // cannot synthesize action edges. No transport or hardware is opened.
    for (address, payload) in [
        (0, vec![0, 83]),
        (8, vec![]),
        (0, vec![]),
        (0, vec![0, 83]),
        (0, vec![0, 83]),
        (0, vec![]),
        (0, vec![0, 195]),
        (0x10, vec![0xff, 0xec, 0, 0]),
        (0, vec![]),
    ] {
        let mut wire = [0u8; 20];
        wire[..4].copy_from_slice(&[0x11, 2, 18, address]);
        wire[4..4 + payload.len()].copy_from_slice(&payload);
        if let Some(event) = decoder.decode(&Report::decode(&wire).unwrap()).unwrap() {
            if event["kind"] == "buttons" {
                let held: Vec<_> = event["held"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as u16)
                    .collect();
                r.buttons(&held, ms(0)).unwrap();
            } else {
                r.motion(
                    event["dx"].as_i64().unwrap() as i16,
                    event["dy"].as_i64().unwrap() as i16,
                    ms(0),
                )
                .unwrap();
            }
        }
    }
    assert_eq!(
        log.borrow().events,
        [Emission::Tap(46, 4), Emission::Tap(46, 4)]
    );
    assert_eq!(r.feedback.unwrap().direction, Some(GestureDirection::Left));
}

#[derive(Default)]
struct DesktopLog {
    checks: usize,
    actions: Vec<DesktopAction>,
    fail_check: bool,
    fail_dispatch: bool,
}
struct DesktopMock(Rc<RefCell<DesktopLog>>);
impl Dispatcher for DesktopMock {
    fn require_actions(&mut self) -> Result<(), String> {
        let mut log = self.0.borrow_mut();
        log.checks += 1;
        if log.fail_check {
            Err("desktop unavailable".into())
        } else {
            Ok(())
        }
    }
    fn dispatch(&mut self, action: DesktopAction) -> Result<(), String> {
        let mut log = self.0.borrow_mut();
        log.actions.push(action);
        if log.fail_dispatch {
            Err("desktop reply uncertain".into())
        } else {
            Ok(())
        }
    }
}
type Routed = Runtime<NativeOutput<Mock, DesktopMock>>;
fn routed(value: Value) -> (Routed, Rc<RefCell<Log>>, Rc<RefCell<DesktopLog>>) {
    let keyboard = Rc::new(RefCell::new(Log::default()));
    let desktop = Rc::new(RefCell::new(DesktopLog::default()));
    let runtime = Runtime::new(
        NativeOutput {
            keyboard: Mock(keyboard.clone()),
            desktop: DesktopMock(desktop.clone()),
        },
        bindings(value),
        None,
    )
    .unwrap();
    (runtime, keyboard, desktop)
}
fn desktop_gestures() -> Value {
    json!({"type":"gestures","threshold":20,"units":"counts","actions":{
        "up":"desktop:fullscreen","down":"desktop:maximize","left":"desktop:workspace-previous",
        "right":"desktop:workspace-next","click":"desktop:maximize"}})
}
#[test]
fn desktop_bindings_detect_sides_and_gestures_without_touching_io() {
    for action in [
        "desktop:fullscreen",
        "desktop:maximize",
        "desktop:workspace-previous",
        "desktop:workspace-next",
    ] {
        for cid in ["83", "86"] {
            assert!(bindings(json!({cid:action})).uses_desktop());
        }
        for direction in ["up", "down", "left", "right", "click"] {
            let mut config = gesture("counts", 20);
            config["actions"][direction] = json!(action);
            assert!(bindings(json!({"195":config})).uses_desktop());
        }
        assert!(KeyboardAction::parse(action).is_err());
    }
    assert!(!bindings(json!({"83":"modifier:ctrl","195":gesture("counts",20)})).uses_desktop());
    let mut config = desktop_gestures();
    config["actions"]["up"] = json!("desktop:exec");
    assert!(ActionBindings::parse(&json!({"195":config})).is_err());
}
#[test]
fn keyboard_only_output_never_probes_a_compositor() {
    let (mut r, keyboard, desktop) = routed(json!({"83":"shortcut:copy"}));
    desktop.borrow_mut().fail_check = true;
    r.buttons(&[], ms(0)).unwrap();
    r.buttons(&[83], ms(1)).unwrap();
    r.replace_bindings(bindings(json!({"83":"modifier:ctrl"})))
        .unwrap();
    assert_eq!(keyboard.borrow().events, [Emission::Tap(46, 4)]);
    assert_eq!(desktop.borrow().checks, 0);
}
#[test]
fn desktop_preflight_failure_closes_keyboard_before_a_runtime_can_be_armed() {
    let keyboard = Rc::new(RefCell::new(Log {
        fail_close: true,
        ..Log::default()
    }));
    let desktop = Rc::new(RefCell::new(DesktopLog {
        fail_check: true,
        ..DesktopLog::default()
    }));
    let result = Runtime::new(
        NativeOutput {
            keyboard: Mock(keyboard.clone()),
            desktop: DesktopMock(desktop.clone()),
        },
        bindings(json!({"83":"desktop:fullscreen"})),
        None,
    );
    let error = result.err().unwrap();
    assert!(error.contains("desktop unavailable") && error.contains("close failed"));
    assert_eq!(keyboard.borrow().events, [Emission::Close]);
    assert_eq!(desktop.borrow().checks, 1);
    assert!(desktop.borrow().actions.is_empty());
}
#[test]
fn a_keyboard_only_sink_cannot_silently_accept_desktop_assignments() {
    let keyboard = Rc::new(RefCell::new(Log::default()));
    assert!(Runtime::new(
        Mock(keyboard.clone()),
        bindings(json!({"83":"desktop:maximize"})),
        None
    )
    .is_err());
    assert_eq!(keyboard.borrow().events, [Emission::Close]);
}
#[test]
fn desktop_side_edges_preserve_held_modifiers_and_suppress_initial_presses() {
    let (mut r, keyboard, desktop) =
        routed(json!({"83":"modifier:ctrl","86":"desktop:fullscreen"}));
    r.buttons(&[86], ms(0)).unwrap();
    r.buttons(&[], ms(1)).unwrap();
    r.buttons(&[83], ms(2)).unwrap();
    for _ in 0..3 {
        r.buttons(&[83, 86], ms(3)).unwrap();
    }
    r.buttons(&[83], ms(4)).unwrap();
    r.buttons(&[83, 86], ms(5)).unwrap();
    assert_eq!(
        keyboard.borrow().events,
        [Emission::Held(HeldModifiers::Ctrl)]
    );
    assert_eq!(
        desktop.borrow().actions,
        [DesktopAction::Fullscreen, DesktopAction::Fullscreen]
    );
    r.buttons(&[], ms(6)).unwrap();
    assert_eq!(
        keyboard.borrow().events,
        [
            Emission::Held(HeldModifiers::Ctrl),
            Emission::Held(HeldModifiers::None)
        ]
    );
}
#[test]
fn desktop_gesture_dispatches_once_when_it_starts_with_the_selected_direction() {
    let (mut r, keyboard, desktop) = routed(json!({"195":desktop_gestures()}));
    r.buttons(&[], ms(0)).unwrap();
    for (index, (dx, dy, expected)) in [
        (-20, 0, DesktopAction::WorkspacePrevious),
        (20, 0, DesktopAction::WorkspaceNext),
        (0, -20, DesktopAction::Fullscreen),
        (0, 20, DesktopAction::Maximize),
        (0, 0, DesktopAction::Maximize),
    ]
    .into_iter()
    .enumerate()
    {
        let base = (index as u64) * 4;
        let before = desktop.borrow().actions.len();
        r.buttons(&[195], ms(base + 1)).unwrap();
        r.motion(dx, dy, ms(base + 2)).unwrap();
        let started = usize::from(dx != 0 || dy != 0);
        assert_eq!(desktop.borrow().actions.len(), before + started);
        r.buttons(&[], ms(base + 3)).unwrap();
        r.buttons(&[], ms(base + 3)).unwrap();
        assert_eq!(desktop.borrow().actions.last(), Some(&expected));
    }
    assert_eq!(desktop.borrow().actions.len(), 5);
    assert!(keyboard.borrow().events.is_empty());
}
#[test]
fn canceled_ambiguous_and_expired_gestures_never_dispatch_desktop_actions() {
    let (mut r, keyboard, desktop) = routed(json!({"195":desktop_gestures()}));
    r.buttons(&[], ms(0)).unwrap();
    r.buttons(&[195], ms(1)).unwrap();
    r.motion(20, 20, ms(2)).unwrap();
    r.buttons(&[], ms(3)).unwrap();
    assert_eq!(r.feedback.unwrap().direction, None);
    r.buttons(&[195], ms(4)).unwrap();
    // Cancelled before the gesture started (4^1.4 is below the distance).
    r.motion(-4, 0, ms(5)).unwrap();
    r.cancel().unwrap();
    r.buttons(&[195], ms(6)).unwrap();
    r.buttons(&[], ms(7)).unwrap();
    r.buttons(&[195], ms(8)).unwrap();
    r.buttons(&[], ms(10_009)).unwrap();
    assert!(desktop.borrow().actions.is_empty());
    assert!(keyboard.borrow().events.is_empty());
}
#[test]
fn failed_desktop_dispatch_is_consumed_then_closes_and_releases_once() {
    for gesture in [false, true] {
        let value = if gesture {
            json!({"83":"modifier:ctrl","195":desktop_gestures()})
        } else {
            json!({"83":"modifier:ctrl","86":"desktop:fullscreen"})
        };
        let (mut r, keyboard, desktop) = routed(value);
        r.buttons(&[], ms(0)).unwrap();
        r.buttons(&[83], ms(1)).unwrap();
        desktop.borrow_mut().fail_dispatch = true;
        if gesture {
            r.buttons(&[83, 195], ms(2)).unwrap();
        }
        let controls = if gesture { vec![83] } else { vec![83, 86] };
        assert_eq!(
            r.buttons(&controls, ms(3)).unwrap_err(),
            "desktop reply uncertain"
        );
        assert!(r.buttons(&controls, ms(4)).is_err());
        assert!(r.stroke.is_none());
        r.close().unwrap();
        drop(r);
        assert_eq!(
            keyboard.borrow().events,
            [Emission::Held(HeldModifiers::Ctrl), Emission::Close]
        );
        assert_eq!(desktop.borrow().actions.len(), 1);
    }
}
#[test]
fn replacing_with_desktop_bindings_probes_before_acceptance_and_suppresses_held_buttons() {
    let (mut r, keyboard, desktop) = routed(json!({"83":"modifier:ctrl"}));
    r.buttons(&[], ms(0)).unwrap();
    r.buttons(&[83], ms(1)).unwrap();
    r.replace_bindings(bindings(json!({"83":"desktop:maximize"})))
        .unwrap();
    r.buttons(&[83], ms(2)).unwrap();
    assert!(desktop.borrow().actions.is_empty());
    assert_eq!(desktop.borrow().checks, 1);
    assert_eq!(
        keyboard.borrow().events,
        [
            Emission::Held(HeldModifiers::Ctrl),
            Emission::Held(HeldModifiers::None)
        ]
    );
    r.buttons(&[], ms(3)).unwrap();
    r.buttons(&[83], ms(4)).unwrap();
    assert_eq!(desktop.borrow().actions, [DesktopAction::Maximize]);
    desktop.borrow_mut().fail_check = true;
    assert!(r
        .replace_bindings(bindings(json!({"83":"desktop:fullscreen"})))
        .is_err());
    assert_eq!(keyboard.borrow().events.last(), Some(&Emission::Close));
}
#[test]
fn accepted_snapshots_revalidate_desktop_capabilities_and_close_on_failure() {
    let (mut r, keyboard, desktop) =
        routed(json!({"83":"modifier:ctrl","86":"desktop:fullscreen"}));
    let snapshot = json!({"controls":[{"id":"83","capabilities":{"flags":0x31}},{"id":"86","capabilities":{"flags":0x31}}]});
    r.snapshot(&snapshot).unwrap();
    assert_eq!(desktop.borrow().checks, 2);
    r.buttons(&[], ms(0)).unwrap();
    r.buttons(&[83], ms(1)).unwrap();
    desktop.borrow_mut().fail_check = true;
    assert_eq!(r.snapshot(&snapshot).unwrap_err(), "desktop unavailable");
    assert_eq!(
        keyboard.borrow().events,
        [Emission::Held(HeldModifiers::Ctrl), Emission::Close]
    );
    assert!(desktop.borrow().actions.is_empty());
}
fn raw(dx: i16, dy: i16) -> crate::native::events::Event {
    crate::native::events::Event::RawXY {
        dx,
        dy,
        held: vec![195],
        sources: vec![195],
    }
}
fn press_event(held: &[u16]) -> crate::native::events::Event {
    crate::native::events::Event::Buttons {
        initial: false,
        held: held.to_vec(),
        pressed: vec![],
        released: vec![],
    }
}
#[test]
fn first_raw_xy_of_each_press_is_skipped_only_on_1b04_version_5() {
    // Measured on the reference mouse: (-1537,-102) arrived before an upward stroke.
    for (version, expected) in [(5, Emission::Tap(115, 0)), (4, Emission::Tap(46, 4))] {
        let (mut r, log) = fixture(json!({"195":gesture("counts",20)}), None);
        let snapshot = json!({"feature":{"version":version},
            "controls":[{"id":"195","capabilities":{"flags":0x131}}]});
        r.snapshot(&snapshot).unwrap();
        for _ in 0..2 {
            log.borrow_mut().events.clear();
            r.event(&press_event(&[])).unwrap();
            r.event(&press_event(&[195])).unwrap();
            r.event(&raw(-1537, -102)).unwrap();
            r.event(&raw(0, -30)).unwrap();
            r.event(&press_event(&[])).unwrap();
            // The rule applies to every press, not only the first one.
            assert_eq!(log.borrow().events.first(), Some(&expected), "v{version}");
        }
    }
}
#[test]
fn gestures_require_a_verified_1b04_version() {
    let (mut r, log) = fixture(json!({"195":gesture("counts",20)}), None);
    let snapshot = json!({"controls":[{"id":"195","capabilities":{"flags":0x131}}]});
    assert!(r.snapshot(&snapshot).unwrap_err().contains("1B04 version"));
    assert_eq!(log.borrow().events, [Emission::Close]);
    // Side buttons alone do not depend on raw XY and stay valid.
    let (mut r, _) = fixture(json!({"83":"shortcut:copy"}), None);
    let snapshot = json!({"controls":[{"id":"83","capabilities":{"flags":0x31}}]});
    assert!(r.snapshot(&snapshot).is_ok());
}

#[test]
fn volume_follows_the_movement_while_held_and_restarts_after_each_step() {
    let (mut r, log) = fixture(json!({"195":gesture("counts",40)}), None);
    press(&mut r);
    // Slow 1-count reports weigh 1: starts at 40 and steps every 20 (half).
    for _ in 0..80 {
        r.motion(0, -1, ms(2)).unwrap();
    }
    assert_eq!(log.borrow().events, vec![Emission::Tap(115, 0); 3]);
    // Jitter after a step restarts from zero and cannot alternate.
    for dy in [1; 9].into_iter().chain([-1; 9]) {
        r.motion(0, dy, ms(3)).unwrap();
    }
    assert_eq!(log.borrow().events.len(), 3);
    // Moving back a full step lowers it again.
    for _ in 0..20 {
        r.motion(0, 1, ms(4)).unwrap();
    }
    assert_eq!(log.borrow().events.last(), Some(&Emission::Tap(114, 0)));
    r.buttons(&[], ms(5)).unwrap();
    assert_eq!(log.borrow().events.len(), 4);
    assert_eq!(r.feedback.unwrap().direction, Some(GestureDirection::Down));
}
#[test]
fn non_volume_actions_run_once_when_the_gesture_starts() {
    let (mut r, log) = fixture(json!({"195":gesture("counts",20)}), None);
    press(&mut r);
    r.motion(-80, 0, ms(2)).unwrap();
    assert_eq!(log.borrow().events, [Emission::Tap(46, 4)]);
    r.motion(-80, 0, ms(2)).unwrap();
    r.buttons(&[], ms(3)).unwrap();
    assert_eq!(log.borrow().events, [Emission::Tap(46, 4)]);
}
