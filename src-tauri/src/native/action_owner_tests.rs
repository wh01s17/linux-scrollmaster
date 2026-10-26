// SPDX-License-Identifier: GPL-3.0-or-later
//! Actual session/decoder/action engine with independent synthetic wire frames.
use super::*;
use crate::native::{
    actions::{Output, Runtime},
    events::SinkHandle,
    ActionBindings, HeldModifiers, KeyboardAction, PassiveEventOwner,
};
use std::{cell::Cell, collections::VecDeque};

#[derive(Default)]
struct Log {
    events: Vec<String>,
    tap_error: bool,
    tap_panic: bool,
    close_error: bool,
}
struct Keyboard(Rc<RefCell<Log>>);
impl Output for Keyboard {
    fn held(&mut self, value: HeldModifiers) -> Result<(), String> {
        self.0.borrow_mut().events.push(format!("held:{value:?}"));
        Ok(())
    }
    fn tap(&mut self, value: &KeyboardAction) -> Result<(), String> {
        let mut log = self.0.borrow_mut();
        log.events.push(format!("tap:{:?}", value.code()));
        assert!(!log.tap_panic, "Synthetic keyboard panic");
        if log.tap_error {
            Err("Synthetic tap failure".into())
        } else {
            Ok(())
        }
    }
    fn close(&mut self) -> Result<(), String> {
        let mut log = self.0.borrow_mut();
        log.events.push("close".into());
        if log.close_error {
            Err("Synthetic release unconfirmed".into())
        } else {
            Ok(())
        }
    }
}
struct Feed {
    io: Interleaved,
    idle: Rc<RefCell<VecDeque<io::Result<Vec<u8>>>>>,
    dpi: Rc<Cell<u16>>,
    log: Rc<RefCell<Log>>,
}
impl Drop for Feed {
    fn drop(&mut self) {
        self.log.borrow_mut().events.push("hid-drop".into());
    }
}
impl Transport for Feed {
    fn write_report(&mut self, p: &[u8], deadline: Instant) -> io::Result<()> {
        self.io.write_report(p, deadline)
    }
    fn read_report(&mut self, deadline: Instant) -> io::Result<Vec<u8>> {
        if self.io.incoming.is_empty() && !self.io.replying {
            if let Some(next) = self.idle.borrow_mut().pop_front() {
                return next;
            }
        }
        let mut p = self.io.read_report(deadline)?;
        if p[2] == 14 && p[3] >> 4 == 2 {
            p[5..7].copy_from_slice(&self.dpi.get().to_be_bytes());
        }
        Ok(p)
    }
}
struct Fixture {
    owner: PassiveEventOwner<Feed>,
    log: Rc<RefCell<Log>>,
    state: Rc<RefCell<State>>,
    idle: Rc<RefCell<VecDeque<io::Result<Vec<u8>>>>>,
    dpi: Rc<Cell<u16>>,
    cancelled: Rc<Cell<bool>>,
    sink: SinkHandle,
    gesture_held: bool,
}
impl Fixture {
    fn new(
        bindings: Value,
        schedule: BTreeMap<(u8, u8, usize), Vec<Vec<u8>>>,
        raw_side: bool,
    ) -> Self {
        let (device, state) = Device::new();
        if raw_side {
            state.borrow_mut().table[3][8] = 1;
        }
        let log = Rc::new(RefCell::new(Log::default()));
        let sink = SinkHandle::new(
            Runtime::new(
                Keyboard(log.clone()),
                ActionBindings::parse(&bindings).unwrap(),
                Some(1850),
            )
            .unwrap(),
        );
        let idle = Rc::new(RefCell::new(VecDeque::new()));
        let dpi = Rc::new(Cell::new(1850));
        let cancelled = Rc::new(Cell::new(false));
        let flag = cancelled.clone();
        let feed = Feed {
            io: Interleaved::new(device, schedule),
            idle: idle.clone(),
            dpi: dpi.clone(),
            log: log.clone(),
        };
        let owner = PassiveEventOwner::open_with_sink(
            feed,
            1,
            "11223344",
            Duration::from_secs(3),
            move || flag.get(),
            Some(sink.clone()),
        )
        .unwrap();
        Self {
            owner,
            log,
            state,
            idle,
            dpi,
            cancelled,
            sink,
            gesture_held: false,
        }
    }
    fn send(&mut self, p: Vec<u8>) {
        self.idle.borrow_mut().push_back(Ok(p));
        self.owner.poll(Duration::from_millis(50)).unwrap();
    }
    fn buttons(&mut self, cids: &[u16]) {
        self.send(notification(cids));
        // Mirror hardware: each gesture press is followed by the 1B04 v5 report.
        let pressed = cids.contains(&195) && !self.gesture_held;
        self.gesture_held = cids.contains(&195);
        if pressed {
            self.send(initial_raw());
        }
    }
    fn motion(&mut self, dx: i16, dy: i16) {
        self.send(frame(
            &[0x11, 1, 18, 0x10],
            &[dx.to_be_bytes(), dy.to_be_bytes()].concat(),
        ));
    }
    fn refresh(&mut self) {
        self.owner.refresh(Duration::from_secs(1)).unwrap();
    }
    fn events(&self) -> Vec<String> {
        self.log.borrow().events.clone()
    }
}
fn gesture(physical: bool) -> Value {
    json!({"type":"gestures","units":if physical {"tenths-mm"} else {"counts"},
        "threshold":if physical {10} else {20},"actions":{
        "up":null,"down":null,"left":"shortcut:copy","right":"shortcut:paste","click":"media:mute"}})
}

#[test]
fn initialization_fifo_is_evidence_only_and_live_edges_are_delivered_during_getters() {
    let mut f = Fixture::new(
        json!({"83":"shortcut:copy"}),
        [
            (
                (0, 1, 1),
                vec![notification(&[]), notification(&[83]), notification(&[])],
            ),
            (
                (0, 1, 2),
                vec![notification(&[83]), notification(&[]), notification(&[83])],
            ),
        ]
        .into(),
        false,
    );
    assert!(f.events().is_empty());
    f.refresh();
    assert_eq!(f.events(), ["tap:(46, 4)"]);
    let status = f.owner.status();
    let events = status["observation"]["events"].as_array().unwrap();
    assert!(events[..3]
        .iter()
        .all(|e| e["delivered_to_sink"] == false && e["queued_before_observation"] == true));
    assert!(events[3..]
        .iter()
        .all(|e| e["delivered_to_sink"] == true && e["during_query"] == true));
    assert_eq!(status["observation"]["sink_deliveries"], 3);
    f.owner.status();
    f.owner.status(); // no replay through telemetry
    assert_eq!(f.events(), ["tap:(46, 4)"]);
    assert!(setters(&f.state.borrow()).is_empty());
}
#[test]
fn idle_and_query_delivery_share_synchronization_modifiers_and_gesture_state() {
    let mut f = Fixture::new(
        json!({"83":"modifier:ctrl","86":"shortcut:copy","195":gesture(false)}),
        [
            (
                (0, 1, 2),
                vec![
                    notification(&[83, 195]),
                    initial_raw(),
                    frame(&[0x11, 1, 18, 0x10], &[0xff, 0xec, 0, 0]),
                ],
            ),
            ((18, 2, 6), vec![notification(&[83, 86])]),
        ]
        .into(),
        false,
    );
    f.buttons(&[]);
    f.buttons(&[83]);
    f.refresh();
    assert_eq!(f.events(), ["held:Ctrl", "tap:(46, 4)", "tap:(46, 4)"]);
    f.buttons(&[83, 86]);
    assert_eq!(f.events().len(), 3);
    f.buttons(&[]);
    assert_eq!(f.events().last().unwrap(), "held:None");
}
#[test]
fn unchanged_dpi_refresh_preserves_an_inflight_physical_gesture() {
    let mut f = Fixture::new(json!({"195":gesture(true)}), Default::default(), false);
    f.buttons(&[]);
    f.buttons(&[195]);
    f.motion(2, 0);
    // Several quiet periodic reads must not require releasing and pressing again.
    for _ in 0..4 {
        f.refresh();
    }
    f.motion(100, 0);
    f.buttons(&[]);
    assert_eq!(f.events(), ["tap:(47, 4)"]);
}
#[test]
fn physical_gestures_during_an_unchanged_dpi_refresh_are_not_lost() {
    let mut f = Fixture::new(
        json!({"83":"shortcut:copy","195":gesture(true)}),
        [(
            (0, 1, 2),
            vec![
                notification(&[195, 83]),
                initial_raw(),
                frame(&[0x11, 1, 18, 0x10], &[0, 100, 0, 0]),
                notification(&[]),
                notification(&[195]),
                initial_raw(),
                frame(&[0x11, 1, 18, 0x10], &[0, 100, 0, 0]),
                notification(&[]),
            ],
        )]
        .into(),
        false,
    );
    f.buttons(&[]);
    f.refresh();
    assert_eq!(f.events(), ["tap:(46, 4)", "tap:(47, 4)", "tap:(47, 4)"]);
}
#[test]
fn physical_gestures_are_cancelled_across_dpi_queries_and_resume_only_on_a_fresh_press() {
    let mut f = Fixture::new(
        json!({"83":"shortcut:copy","195":gesture(true)}),
        [(
            (0, 1, 2),
            vec![
                notification(&[]),
                notification(&[195, 83]),
                initial_raw(),
                notification(&[]),
            ],
        )]
        .into(),
        false,
    );
    f.buttons(&[]);
    f.buttons(&[195]);
    // In flight but below the start distance when the DPI query begins.
    f.motion(2, 0);
    f.dpi.set(800);
    f.refresh();
    // In-flight physical gestures and a new one during the getter are suppressed;
    // side shortcuts still execute synchronously while the DPI reading is unknown.
    assert_eq!(f.events(), ["tap:(46, 4)"]);
    // The scripted getter replies ended with every control released.
    f.gesture_held = false;
    f.buttons(&[195]);
    f.motion(31, 0);
    f.motion(1, 0);
    f.buttons(&[]);
    assert_eq!(f.events(), ["tap:(46, 4)", "tap:(47, 4)"]);
}
#[test]
fn raw_xy_with_more_than_one_possible_source_cancels_instead_of_inventing_a_click() {
    let mut f = Fixture::new(json!({"195":gesture(false)}), Default::default(), true);
    f.buttons(&[]);
    f.buttons(&[195, 83]);
    f.motion(100, 0);
    f.buttons(&[]);
    assert!(f.events().is_empty());
    f.buttons(&[195]);
    f.motion(20, 0);
    f.buttons(&[]);
    assert_eq!(f.events(), ["tap:(47, 4)"]);
}
#[test]
fn output_error_or_panic_stops_refresh_without_retry_and_keeps_partial_evidence() {
    for panic in [false, true] {
        let mut f = Fixture::new(
            json!({"83":"shortcut:copy"}),
            [(
                (0, 1, 2),
                vec![
                    notification(&[]),
                    notification(&[83]),
                    notification(&[]),
                    notification(&[83]),
                ],
            )]
            .into(),
            false,
        );
        f.log.borrow_mut().tap_error = !panic;
        f.log.borrow_mut().tap_panic = panic;
        assert!(f.owner.refresh(Duration::from_secs(1)).is_err());
        assert!(!f.owner.is_running());
        let status = f.owner.status();
        assert_eq!(status["observation"]["stop_reason"], "output-error");
        assert_eq!(status["observation"]["sink_deliveries"], 2);
        assert_eq!(
            f.events().iter().filter(|e| e.starts_with("tap:")).count(),
            1
        );
        assert_eq!(
            f.events().iter().filter(|e| e.as_str() == "close").count(),
            1
        );
        f.owner.stop();
        f.sink.close().unwrap();
        assert_eq!(
            f.events().iter().filter(|e| e.as_str() == "close").count(),
            1
        );
    }
}
#[test]
fn cancellation_disconnect_and_drop_release_after_the_reader_closes() {
    for kind in 0..3 {
        let mut f = Fixture::new(json!({"83":"modifier:alt"}), Default::default(), false);
        f.buttons(&[]);
        f.buttons(&[83]);
        match kind {
            0 => {
                f.cancelled.set(true);
                assert!(f.owner.poll(Duration::from_millis(50)).is_err());
            }
            1 => {
                f.idle
                    .borrow_mut()
                    .push_back(Err(io::ErrorKind::BrokenPipe.into()));
                assert!(f.owner.poll(Duration::from_millis(50)).is_err());
            }
            _ => {
                drop(f.owner);
                assert_eq!(f.log.borrow().events, ["held:Alt", "hid-drop", "close"]);
                continue;
            }
        }
        assert_eq!(f.events(), ["held:Alt", "hid-drop", "close"]);
        assert_eq!(f.owner.status()["observation"]["cleared_held"], json!([83]));
    }
}
#[test]
fn unconfirmed_release_is_cached_and_is_not_reported_as_success() {
    let mut f = Fixture::new(json!({}), Default::default(), false);
    f.log.borrow_mut().close_error = true;
    let status = f.owner.stop();
    assert_eq!(
        status["observation"]["sink_release_error"],
        "Synthetic release unconfirmed"
    );
    assert_eq!(f.sink.close().unwrap_err(), "Synthetic release unconfirmed");
    assert_eq!(f.sink.close().unwrap_err(), "Synthetic release unconfirmed");
    assert_eq!(f.events(), ["hid-drop", "close"]);
}
#[test]
fn changed_device_binding_closes_output_before_committing_a_new_snapshot() {
    let mut f = Fixture::new(json!({"83":"modifier:ctrl"}), Default::default(), false);
    f.buttons(&[]);
    f.buttons(&[83]);
    let old = f.owner.last_snapshot().clone();
    f.state.borrow_mut().targets.insert(83, 86);
    assert!(f.owner.refresh(Duration::from_secs(1)).is_err());
    assert_eq!(f.owner.last_snapshot(), &old);
    assert_eq!(f.events(), ["held:Ctrl", "hid-drop", "close"]);
}
#[test]
fn setup_capabilities_and_arguments_are_validated_before_live_output() {
    for kind in 0..4 {
        let (device, state) = Device::new();
        if kind == 0 {
            state.borrow_mut().table[5][8] = 0;
        }
        if kind == 1 {
            state.borrow_mut().table[3][4] = 1;
        }
        let log = Rc::new(RefCell::new(Log::default()));
        let sink = SinkHandle::new(
            Runtime::new(
                Keyboard(log.clone()),
                ActionBindings::parse(&json!({"83":"shortcut:copy","195":gesture(false)})).unwrap(),
                None,
            )
            .unwrap(),
        );
        let id = if kind == 2 { "55667788" } else { "11223344" };
        let duration = if kind == 3 {
            Duration::ZERO
        } else {
            Duration::from_secs(1)
        };
        assert!(PassiveEventOwner::open_with_sink(
            device,
            1,
            id,
            duration,
            || false,
            Some(sink.clone())
        )
        .is_err());
        sink.close().unwrap();
        assert_eq!(log.borrow().events, ["close"]);
        assert!(setters(&state.borrow()).is_empty());
    }
}
#[test]
fn lost_dpi_during_refresh_closes_physical_actions_without_an_assumed_value() {
    let mut f = Fixture::new(json!({"195":gesture(true)}), Default::default(), false);
    f.buttons(&[]);
    f.buttons(&[195]);
    // Zero legitimately selects the device-advertised default. An unannounced
    // out-of-range value makes this getter unavailable instead.
    f.dpi.set(65535);
    assert!(f.owner.refresh(Duration::from_secs(1)).is_err());
    assert!(!f.owner.is_running());
    // The engine fails closed immediately inside snapshot validation; the
    // enclosing capture then closes its transport before returning the error.
    assert_eq!(f.events(), ["close", "hid-drop"]);
}

#[test]
fn close_panic_is_cached_across_owner_stop_guardian_close_and_drop() {
    struct Panics(Rc<Cell<usize>>);
    impl crate::native::events::EventSink for Panics {
        fn close(&mut self) -> Result<(), String> {
            self.0.set(self.0.get() + 1);
            panic!("Synthetic close panic");
        }
    }
    let count = Rc::new(Cell::new(0));
    let sink = SinkHandle::new(Panics(count.clone()));
    let (device, _) = Device::new();
    let mut owner = PassiveEventOwner::open_with_sink(
        device,
        1,
        "11223344",
        Duration::from_secs(1),
        || false,
        Some(sink.clone()),
    )
    .unwrap();
    assert_eq!(
        owner.stop()["observation"]["sink_release_error"],
        "Event sink close panicked"
    );
    assert_eq!(sink.close().unwrap_err(), "Event sink close panicked");
    drop(owner);
    drop(sink);
    assert_eq!(count.get(), 1);
}
#[test]
fn query_hook_failure_or_panic_closes_before_another_hardware_request() {
    struct Fails {
        panic: bool,
        count: Rc<Cell<usize>>,
    }
    impl crate::native::events::EventSink for Fails {
        fn before_query(&mut self) -> Result<(), String> {
            assert!(!self.panic, "Synthetic query preparation panic");
            Err("Query output unavailable".into())
        }
        fn close(&mut self) -> Result<(), String> {
            self.count.set(self.count.get() + 1);
            Ok(())
        }
    }
    for panic in [false, true] {
        let count = Rc::new(Cell::new(0));
        let sink = SinkHandle::new(Fails {
            panic,
            count: count.clone(),
        });
        let (device, state) = Device::new();
        let mut owner = PassiveEventOwner::open_with_sink(
            device,
            1,
            "11223344",
            Duration::from_secs(1),
            || false,
            Some(sink),
        )
        .unwrap();
        let writes = state.borrow().wire.len();
        assert!(owner.refresh(Duration::from_secs(1)).is_err());
        assert_eq!(state.borrow().wire.len(), writes);
        assert_eq!(count.get(), 1);
        assert_eq!(owner.status()["observation"]["stop_reason"], "output-error");
    }
}
#[test]
fn event_limit_closes_output_without_dispatching_overflow_or_replaying_history() {
    let mut f = Fixture::new(json!({"83":"shortcut:copy"}), Default::default(), false);
    for n in 0..128 {
        f.buttons(if n % 2 == 0 { &[] } else { &[83] });
    }
    assert_eq!(
        f.events().iter().filter(|e| e.starts_with("tap:")).count(),
        64
    );
    f.idle.borrow_mut().push_back(Ok(notification(&[83])));
    assert!(f.owner.poll(Duration::from_millis(50)).is_err());
    assert_eq!(
        f.owner.status()["observation"]["stop_reason"],
        "event-limit"
    );
    assert_eq!(f.owner.status()["observation"]["sink_deliveries"], 128);
    assert_eq!(
        f.events().iter().filter(|e| e.starts_with("tap:")).count(),
        64
    );
    assert_eq!(
        f.events().iter().filter(|e| e.as_str() == "close").count(),
        1
    );
}
