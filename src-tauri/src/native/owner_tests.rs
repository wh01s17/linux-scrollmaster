// SPDX-License-Identifier: GPL-3.0-or-later
//! Lifecycle tests using the parent's independent synthetic mouse and wire log.
use super::*;
use crate::native::PassiveEventOwner;
use std::cell::Cell;

#[derive(Clone, Default)]
struct Control {
    dropped: Rc<Cell<usize>>,
    fail: Rc<Cell<bool>>,
    late: Rc<Cell<bool>>,
    identity: Rc<Cell<bool>>,
    dpi: Rc<Cell<bool>>,
}
struct Tracked {
    io: Interleaved,
    control: Control,
}
impl Drop for Tracked {
    fn drop(&mut self) {
        self.control.dropped.set(self.control.dropped.get() + 1);
    }
}
impl Transport for Tracked {
    fn write_report(&mut self, p: &[u8], deadline: Instant) -> io::Result<()> {
        if self.control.fail.get() {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        self.io.write_report(p, deadline)
    }
    fn read_report(&mut self, deadline: Instant) -> io::Result<Vec<u8>> {
        if self.control.late.replace(false) {
            std::thread::sleep(
                deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(1),
            );
        }
        let mut p = self.io.read_report(deadline)?;
        if self.control.identity.get() && p[2] == 2 && p[3] >> 4 == 0 {
            p[5..9].copy_from_slice(&[0x55, 0x66, 0x77, 0x88]);
        }
        if self.control.dpi.get() && p[2] == 14 && p[3] >> 4 == 2 {
            p[5..7].copy_from_slice(&1800u16.to_be_bytes());
        }
        Ok(p)
    }
}
fn open(
    schedule: BTreeMap<(u8, u8, usize), Vec<Vec<u8>>>,
    duration: Duration,
    cancelled: impl FnMut() -> bool + 'static,
) -> (PassiveEventOwner<Tracked>, Control, Rc<RefCell<State>>) {
    let (device, state) = Device::new();
    let control = Control::default();
    let io = Tracked {
        io: Interleaved::new(device, schedule),
        control: control.clone(),
    };
    let owner = PassiveEventOwner::open(io, 1, "11223344", duration, cancelled).unwrap();
    (owner, control, state)
}

#[test]
fn owner_refreshes_and_collects_interleaved_events_on_one_transport() {
    let (mut owner, control, state) = open(
        [
            ((0, 1, 2), vec![notification(&[]), notification(&[195])]),
            ((18, 2, 6), vec![notification(&[])]),
        ]
        .into(),
        Duration::from_secs(1),
        || false,
    );
    owner.refresh(Duration::from_secs(1)).unwrap();
    owner.refresh(Duration::from_secs(1)).unwrap();
    let status = owner.status();
    assert_eq!(status["refreshes"], 2);
    assert_eq!(status["refresh_attempts"], 2);
    assert_eq!(status["observation"]["reports"], 3);
    let events = status["observation"]["events"].as_array().unwrap();
    assert_eq!(events[1]["pressed"], json!([195]));
    assert_eq!(events[2]["released"], json!([195]));
    assert!(events
        .iter()
        .all(|e| e["during_query"] == true && e["queued_before_observation"] == false));
    assert!(owner.is_running());
    assert_eq!(control.dropped.get(), 0);
    owner.stop();
    assert_eq!(control.dropped.get(), 1);
    assert!(setters(&state.borrow()).is_empty());
}

#[test]
fn changed_feature_capabilities_mapping_or_reporting_close_without_committing_snapshot() {
    for kind in 0..5 {
        let (mut owner, control, state) =
            open(Default::default(), Duration::from_secs(1), || false);
        let before = owner.last_snapshot().clone();
        match kind {
            0 => state.borrow_mut().version = 4,
            1 => state.borrow_mut().table[3][4] ^= 0x40,
            2 => {
                state.borrow_mut().targets.insert(83, 86);
            }
            3 => {
                state.borrow_mut().flags.insert(83, 1);
            }
            _ => {
                state.borrow_mut().table[6][1] = 197;
            }
        }
        assert!(owner
            .refresh(Duration::from_secs(1))
            .unwrap_err()
            .contains("changed"));
        assert_eq!(owner.last_snapshot(), &before);
        assert!(!owner.is_running());
        assert_eq!(control.dropped.get(), 1);
        assert_eq!(
            owner.status()["observation"]["stop_reason"],
            "configuration-changed"
        );
        assert_eq!(owner.status()["refreshes"], 0);
        assert!(setters(&state.borrow()).is_empty());
    }
}

#[test]
fn changed_identity_and_disconnect_are_terminal_with_the_last_valid_snapshot() {
    for disconnect in [false, true] {
        let (mut owner, control, state) =
            open(Default::default(), Duration::from_secs(1), || false);
        let before = owner.last_snapshot().clone();
        if disconnect {
            control.fail.set(true);
        } else {
            control.identity.set(true);
        }
        assert!(owner.refresh(Duration::from_secs(1)).is_err());
        assert!(!owner.is_running());
        assert_eq!(control.dropped.get(), 1);
        assert_eq!(owner.last_snapshot(), &before);
        assert_eq!(owner.status()["observation"]["stop_reason"], "query-error");
        let writes = state.borrow().wire.len();
        assert!(owner.refresh(Duration::from_secs(1)).is_err());
        assert_eq!(state.borrow().wire.len(), writes);
    }
}

#[test]
fn stop_is_immediate_idempotent_and_preserves_cleared_logical_state() {
    let (mut owner, control, state) = open(
        [((0, 1, 1), vec![notification(&[195])])].into(),
        Duration::from_secs(1),
        || false,
    );
    let writes = state.borrow().wire.len();
    let first = owner.stop();
    assert_eq!(first["observation"]["cleared_held"], json!([195]));
    assert_eq!(first["observation"]["stop_reason"], "stopped");
    assert_eq!(control.dropped.get(), 1);
    std::thread::sleep(Duration::from_millis(2));
    assert_eq!(
        owner.status(),
        first,
        "Terminal elapsed time must remain frozen"
    );
    assert_eq!(owner.stop()["observation"]["cleared_held"], json!([195]));
    assert!(owner.poll(Duration::from_secs(1)).is_err());
    assert!(owner.refresh(Duration::from_secs(1)).is_err());
    assert_eq!(state.borrow().wire.len(), writes);
    drop(owner);
    assert_eq!(control.dropped.get(), 1);
}

#[test]
fn dropping_a_running_owner_closes_without_queries_or_setters() {
    let (owner, control, state) = open(Default::default(), Duration::from_secs(1), || false);
    let writes = state.borrow().wire.len();
    drop(owner);
    assert_eq!(control.dropped.get(), 1);
    assert_eq!(state.borrow().wire.len(), writes);
    assert!(setters(&state.borrow()).is_empty());
}

#[test]
fn cancellation_during_refresh_stops_before_new_io_and_clears_held_state() {
    let cancelled = Rc::new(Cell::new(false));
    let flag = cancelled.clone();
    let (mut owner, control, state) = open(
        [((0, 1, 1), vec![notification(&[83])])].into(),
        Duration::from_secs(1),
        move || flag.get(),
    );
    let writes = state.borrow().wire.len();
    cancelled.set(true);
    assert!(owner.refresh(Duration::from_secs(1)).is_err());
    let status = owner.status();
    assert_eq!(status["observation"]["stop_reason"], "cancelled");
    assert_eq!(status["observation"]["cleared_held"], json!([83]));
    assert_eq!(status["observation"]["completed"], false);
    assert_eq!(state.borrow().wire.len(), writes);
    assert_eq!(control.dropped.get(), 1);
}

#[test]
fn invalid_interleaved_event_keeps_partial_evidence_and_closes_the_refresh() {
    let (mut owner, control, _) = open(
        [(
            (0, 1, 2),
            vec![notification(&[]), notification(&[195]), notification(&[80])],
        )]
        .into(),
        Duration::from_secs(1),
        || false,
    );
    let before = owner.last_snapshot().clone();
    assert!(owner.refresh(Duration::from_secs(1)).is_err());
    let status = owner.status();
    assert_eq!(status["observation"]["stop_reason"], "invalid-event");
    assert_eq!(status["observation"]["events"].as_array().unwrap().len(), 2);
    assert_eq!(status["observation"]["cleared_held"], json!([195]));
    assert_eq!(owner.last_snapshot(), &before);
    assert_eq!(control.dropped.get(), 1);
}

#[test]
fn unfinished_refresh_at_global_deadline_is_not_reported_as_completed_observation() {
    let (mut owner, control, _) = open(Default::default(), Duration::from_millis(5), || false);
    control.late.set(true);
    assert!(owner.refresh(Duration::from_secs(1)).is_err());
    let status = owner.status();
    assert_eq!(status["observation"]["stop_reason"], "query-timeout");
    assert_eq!(status["observation"]["completed"], false);
    assert_eq!(status["refreshes"], 0);
    assert_eq!(control.dropped.get(), 1);
}

#[test]
fn invalid_poll_or_refresh_arguments_preserve_a_running_owner_without_io() {
    let (mut owner, control, state) = open(Default::default(), Duration::from_secs(1), || false);
    let writes = state.borrow().wire.len();
    for timeout in [Duration::ZERO, Duration::from_secs(21), Duration::MAX] {
        assert!(owner.refresh(timeout).is_err());
    }
    assert!(owner.poll(Duration::ZERO).is_err());
    assert!(owner.is_running());
    assert_eq!(control.dropped.get(), 0);
    assert_eq!(owner.status()["refresh_attempts"], 0);
    assert_eq!(state.borrow().wire.len(), writes);
}

#[test]
fn scalar_changes_can_refresh_without_changing_button_binding_or_decoder() {
    let (mut owner, control, state) = open(Default::default(), Duration::from_secs(1), || false);
    assert_eq!(
        owner.last_snapshot()["snapshot"]["device"]["settings"]["dpi"]["value"],
        1850
    );
    control.dpi.set(true);
    owner.refresh(Duration::from_secs(1)).unwrap();
    assert_eq!(
        owner.last_snapshot()["snapshot"]["device"]["settings"]["dpi"]["value"],
        1800
    );
    assert_eq!(owner.status()["refreshes"], 1);
    assert!(owner.is_running());
    assert!(setters(&state.borrow()).is_empty());
}

#[test]
fn initialization_cancellation_and_callback_panic_close_before_any_io() {
    for panic in [false, true] {
        let (device, state) = Device::new();
        let control = Control::default();
        let io = Tracked {
            io: Interleaved::new(device, Default::default()),
            control: control.clone(),
        };
        let result =
            PassiveEventOwner::open(io, 1, "11223344", Duration::from_secs(1), move || {
                assert!(!panic, "synthetic cancellation callback failure");
                true
            });
        let error = result.err().expect("Initialization must fail");
        assert!(error.contains(if panic { "panicked" } else { "cancelled" }));
        assert!(state.borrow().wire.is_empty());
        assert_eq!(control.dropped.get(), 1);
    }
}

#[test]
fn initialization_cancellation_interrupts_identity_catalogue_and_reporting_queries() {
    for (feature, function) in [(0, 1), (2, 0), (18, 1), (18, 2)] {
        let (device, state) = Device::new();
        let control = Control::default();
        let io = Tracked {
            io: Interleaved::new(device, Default::default()),
            control: control.clone(),
        };
        let log = state.clone();
        let result =
            PassiveEventOwner::open(io, 1, "11223344", Duration::from_secs(1), move || {
                log.borrow()
                    .wire
                    .last()
                    .is_some_and(|p| p[2] == feature && p[3] >> 4 == function)
            });
        assert!(result.err().unwrap().contains("cancelled"));
        let wire = &state.borrow().wire;
        assert_eq!(wire.last().unwrap()[2], feature);
        assert_eq!(wire.last().unwrap()[3] >> 4, function);
        assert_eq!(
            wire.iter()
                .filter(|p| p[2] == feature && p[3] >> 4 == function)
                .count(),
            1
        );
        assert_eq!(control.dropped.get(), 1);
        assert!(setters(&state.borrow()).is_empty());
    }
}

struct QuietFirstRead {
    io: Tracked,
    first: bool,
    cancel: Rc<Cell<bool>>,
    cancel_on_timeout: bool,
}
impl Transport for QuietFirstRead {
    fn write_report(&mut self, p: &[u8], deadline: Instant) -> io::Result<()> {
        self.io.write_report(p, deadline)
    }
    fn read_report(&mut self, deadline: Instant) -> io::Result<Vec<u8>> {
        if self.first {
            self.first = false;
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(remaining <= Duration::from_millis(50));
            std::thread::sleep(remaining);
            self.cancel.set(self.cancel_on_timeout);
            return Err(io::ErrorKind::TimedOut.into());
        }
        self.io.read_report(deadline)
    }
}

#[test]
fn quiet_initialization_checks_cancellation_without_waiting_for_request_timeout() {
    let (device, state) = Device::new();
    let control = Control::default();
    let cancelled = Rc::new(Cell::new(false));
    let flag = cancelled.clone();
    let io = QuietFirstRead {
        io: Tracked {
            io: Interleaved::new(device, Default::default()),
            control: control.clone(),
        },
        first: true,
        cancel: cancelled,
        cancel_on_timeout: true,
    };
    let result = PassiveEventOwner::open(io, 1, "11223344", Duration::from_secs(1), move || {
        flag.get()
    });
    assert!(result.err().unwrap().contains("cancelled"));
    assert_eq!(state.borrow().wire.len(), 1);
    assert_eq!(control.dropped.get(), 1);
}

#[test]
fn initialization_quiet_slice_does_not_retry_the_query_or_discard_queued_events() {
    let (device, state) = Device::new();
    let control = Control::default();
    let io = QuietFirstRead {
        io: Tracked {
            io: Interleaved::new(device, [((0, 1, 1), vec![notification(&[83])])].into()),
            control: control.clone(),
        },
        first: true,
        cancel: Rc::new(Cell::new(false)),
        cancel_on_timeout: false,
    };
    let mut owner =
        PassiveEventOwner::open(io, 1, "11223344", Duration::from_secs(1), || false).unwrap();
    assert!(owner.is_running());
    assert_eq!(owner.status()["observation"]["queued_at_start"], 1);
    assert_eq!(
        owner.status()["observation"]["events"][0]["held"],
        json!([83])
    );
    assert_eq!(
        state
            .borrow()
            .wire
            .iter()
            .filter(|p| p[2] == 0 && p[3] >> 4 == 1)
            .count(),
        1
    );
    owner.stop();
    assert_eq!(control.dropped.get(), 1);
    assert!(setters(&state.borrow()).is_empty());
}
