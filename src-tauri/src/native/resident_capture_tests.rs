// SPDX-License-Identifier: GPL-3.0-or-later
//! Real resident capture, recovery and action engine over synthetic HID++ only.
use super::*;
#[path = "resident_capture_process_tests.rs"]
mod process_tests;
#[path = "resident_capture_service_tests.rs"]
mod service_tests;
use crate::{
    native::{profile_capture::GuardedProfileBackend, profile_events::Cancellation},
    service::{Changes, ProfileBackend},
};

struct Observed {
    machine: Machine,
    log: Rc<RefCell<Vec<String>>>,
    // One clear (release/restore) write is lost on the wire and never ACKed.
    drop_clear: Rc<Cell<bool>>,
}
impl Transport for Observed {
    fn write_report(&mut self, p: &[u8], deadline: Instant) -> io::Result<()> {
        if p[2] == 18 && p[3] >> 4 == 3 {
            self.log.borrow_mut().push(format!(
                "wire:{}:{}",
                u16::from_be_bytes([p[4], p[5]]),
                p[6]
            ));
            if p[6] & 1 == 0 && p[6] != 0 && self.drop_clear.replace(false) {
                return Err(io::ErrorKind::TimedOut.into());
            }
            // Native remaps remain handled by the original independent fixture.
            if p[6] == 0 {
                return self.machine.device.write_report(p, deadline);
            }
        }
        self.machine.write_report(p, deadline)
    }
    fn read_report(&mut self, deadline: Instant) -> io::Result<Vec<u8>> {
        match self.machine.read_report(deadline) {
            // The original finite fixture uses EOF for an empty queue. Resident
            // hidraw silence is a timeout, allowing the next private IPC turn.
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
                Err(io::ErrorKind::TimedOut.into())
            }
            result => result,
        }
    }
}
impl Drop for Observed {
    fn drop(&mut self) {
        self.log.borrow_mut().push("descriptor-close".into());
    }
}
struct OutputState {
    log: Rc<RefCell<Vec<String>>>,
    fail_event: Rc<Cell<bool>>,
    fail_close: Rc<Cell<bool>>,
}
impl Output for OutputState {
    fn held(&mut self, value: HeldModifiers) -> Result<(), String> {
        self.log.borrow_mut().push(format!("held:{value:?}"));
        if self.fail_event.get() {
            Err("synthetic output failure".into())
        } else {
            Ok(())
        }
    }
    fn tap(&mut self, action: &KeyboardAction) -> Result<(), String> {
        self.log
            .borrow_mut()
            .push(format!("tap:{:?}", action.code()));
        if self.fail_event.get() {
            Err("synthetic output failure".into())
        } else {
            Ok(())
        }
    }
    fn close(&mut self) -> Result<(), String> {
        self.log.borrow_mut().push("output-close".into());
        if self.fail_close.get() {
            Err("synthetic release unconfirmed".into())
        } else {
            Ok(())
        }
    }
}
struct Fixture {
    backend: GuardedProfileBackend<Observed>,
    state: Rc<RefCell<State>>,
    idle: Rc<RefCell<VecDeque<Vec<u8>>>>,
    log: Rc<RefCell<Vec<String>>>,
    cancelled: Rc<Cell<bool>>,
    fail_event: Rc<Cell<bool>>,
    fail_close: Rc<Cell<bool>>,
    drop_clear: Rc<Cell<bool>>,
}
impl Fixture {
    fn new(faults: Faults) -> Self {
        Self::build(faults, None)
    }
    fn build(
        faults: Faults,
        deferred: Option<crate::native::profile_capture::DeferredRecovery>,
    ) -> Self {
        let (mut device, state) = Device::new();
        device.base.accept_writes = true;
        let idle = Rc::new(RefCell::new(VecDeque::new()));
        let cancelled = Rc::new(Cell::new(false));
        let log = Rc::new(RefCell::new(vec![]));
        let fail_event = Rc::new(Cell::new(false));
        let fail_close = Rc::new(Cell::new(false));
        let drop_clear = Rc::new(Cell::new(false));
        let machine = Machine {
            device,
            pending: None,
            events: Default::default(),
            idle: idle.clone(),
            faults,
            round: 0,
            report_reads: Default::default(),
            cancelled: cancelled.clone(),
            enabled: false,
        };
        let callback = cancelled.clone();
        let (output_log, output_fail, close_fail) =
            (log.clone(), fail_event.clone(), fail_close.clone());
        let backend = GuardedProfileBackend::with_output(
            Observed {
                machine,
                log: log.clone(),
                drop_clear: drop_clear.clone(),
            },
            1,
            "11223344",
            Cancellation::new(move || callback.get()),
            move |bindings, dpi| {
                output_log.borrow_mut().push("output-open".into());
                Runtime::new(
                    OutputState {
                        log: output_log.clone(),
                        fail_event: output_fail.clone(),
                        fail_close: close_fail.clone(),
                    },
                    bindings,
                    dpi,
                )
                .map(SinkHandle::new)
            },
        )
        .unwrap();
        let backend = match deferred {
            Some(slot) => backend.with_deferred(slot),
            None => backend,
        };
        Self {
            backend,
            state,
            idle,
            log,
            cancelled,
            fail_event,
            fail_close,
            drop_clear,
        }
    }
    fn count(&self, entry: &str) -> usize {
        self.log.borrow().iter().filter(|s| *s == entry).count()
    }
    fn activate(&mut self, buttons: Value) -> crate::service::Applied {
        let changes = Changes::new(None, Some(buttons)).unwrap();
        let limit = deadline();
        self.backend.prepare(&changes, limit).unwrap();
        self.backend.apply(&changes, limit).unwrap()
    }
    fn emit(&mut self, cids: &[u16]) -> Result<bool, String> {
        self.idle.borrow_mut().push_back(notification(cids));
        self.backend.poll(Duration::from_millis(5))
    }
    fn close(&mut self) {
        self.backend.close(deadline()).unwrap();
    }
    fn assert_released(&self) {
        let log = self.log.borrow();
        let release = log.iter().position(|s| s == "output-close").unwrap();
        let clear = log.iter().position(|s| s == "wire:83:2").unwrap();
        let descriptor = log.iter().position(|s| s == "descriptor-close").unwrap();
        assert!(release < clear && clear < descriptor, "{log:?}");
        assert_eq!(log.iter().filter(|s| *s == "output-close").count(), 1);
        assert_eq!(self.state.borrow().flags.get(&83), Some(&0));
    }
}
#[test]
fn preparation_is_read_only_and_live_capture_restores_on_confirmed_close() {
    let mut f = Fixture::new(Faults::default());
    assert!(!f.backend.poll(Duration::from_millis(5)).unwrap());
    assert!(f.state.borrow().wire.is_empty());
    let changes = Changes::new(
        Some(json!({"dpi":1800})),
        Some(json!({"83":"modifier:ctrl","86":81})),
    )
    .unwrap();
    let limit = deadline();
    f.backend.prepare(&changes, limit).unwrap();
    assert!(setters(&f.state.borrow()).is_empty());
    let applied = f.backend.apply(&changes, limit).unwrap();
    assert_eq!(applied.confirmed, vec!["dpi", "button:86", "button:83"]);
    assert_eq!(
        applied.snapshot["buttons"]["controls"][0]["value"],
        "modifier:ctrl"
    );
    assert_eq!(applied.snapshot["native_events"]["actions_enabled"], true);
    f.emit(&[]).unwrap();
    f.emit(&[83]).unwrap();
    assert!(f.log.borrow().iter().any(|s| s == "held:Ctrl"));
    f.close();
    f.assert_released();
    assert_eq!(f.state.borrow().targets.get(&86), Some(&81));
    let wire = f.state.borrow().wire.len();
    f.close();
    assert_eq!(wire, f.state.borrow().wire.len());
}
#[test]
fn capture_is_resident_and_does_not_stop_at_the_finite_history_limit() {
    let mut f = Fixture::new(Faults::default());
    f.activate(json!({"83":"shortcut:copy"}));
    for i in 0..5000 {
        f.emit(if i % 2 == 0 { &[] } else { &[83] }).unwrap();
    }
    let snapshot = f.backend.snapshot(deadline()).unwrap();
    assert_eq!(snapshot["native_events"]["events"], 5000);
    assert_eq!(
        snapshot["native_events"]["recent"]
            .as_array()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(
        f.log
            .borrow()
            .iter()
            .filter(|s| s.starts_with("tap:"))
            .count(),
        2500
    );
    f.close();
    f.assert_released();
}
#[test]
fn old_notifications_cannot_seed_activation_or_invent_a_press() {
    let faults = Faults {
        owner_fifo: vec![notification(&[]), notification(&[83])],
        ..Default::default()
    };
    let mut f = Fixture::new(faults);
    f.activate(json!({"83":"shortcut:copy"}));
    f.emit(&[83]).unwrap();
    f.emit(&[]).unwrap();
    assert!(!f.log.borrow().iter().any(|s| s.starts_with("tap:")));
    f.emit(&[83]).unwrap();
    assert_eq!(
        f.log
            .borrow()
            .iter()
            .filter(|s| s.starts_with("tap:"))
            .count(),
        1
    );
    f.close();
}
#[test]
fn held_state_after_the_final_setter_is_suppressed_until_release() {
    let mut f = Fixture::new(Faults {
        after_ack: vec![notification(&[83])],
        ..Default::default()
    });
    f.activate(json!({"83":"modifier:ctrl"}));
    f.emit(&[83]).unwrap();
    assert!(!f.log.borrow().iter().any(|s| s == "held:Ctrl"));
    f.emit(&[]).unwrap();
    f.emit(&[83]).unwrap();
    assert_eq!(
        f.log.borrow().iter().filter(|s| *s == "held:Ctrl").count(),
        1
    );
    f.close();
    f.assert_released();
}
#[test]
fn uncertain_activation_is_not_retried_and_restores_using_the_retained_descriptor() {
    let mut f = Fixture::new(Faults {
        bad_ack: Some(83),
        ..Default::default()
    });
    let c = Changes::new(
        Some(json!({"dpi":1800})),
        Some(json!({"83":"modifier:ctrl"})),
    )
    .unwrap();
    let limit = deadline();
    f.backend.prepare(&c, limit).unwrap();
    let e = f.backend.apply(&c, limit).unwrap_err();
    assert_eq!(e.confirmed, vec!["dpi"]);
    assert_eq!(e.attempted.as_deref(), Some("button:83"));
    assert!(e.message.contains("ACK"));
    f.assert_released();
    assert_eq!(
        f.log.borrow().iter().filter(|s| *s == "wire:83:3").count(),
        1
    );
    assert!(f.backend.snapshot(deadline()).is_err());
}
#[test]
fn idle_output_failure_releases_before_mouse_recovery_without_replaying_the_action() {
    let mut f = Fixture::new(Faults::default());
    f.activate(json!({"83":"shortcut:copy"}));
    f.emit(&[]).unwrap();
    f.fail_event.set(true);
    assert!(f.emit(&[83]).unwrap_err().contains("output failure"));
    f.assert_released();
    assert_eq!(
        f.log
            .borrow()
            .iter()
            .filter(|s| s.starts_with("tap:"))
            .count(),
        1
    );
}
#[test]
fn peer_loss_does_not_cancel_terminal_recovery() {
    let mut f = Fixture::new(Faults::default());
    f.activate(json!({"83":"modifier:ctrl"}));
    f.emit(&[]).unwrap();
    f.emit(&[83]).unwrap();
    f.cancelled.set(true);
    assert!(f
        .backend
        .poll(Duration::from_millis(5))
        .unwrap_err()
        .contains("disconnected"));
    f.assert_released();
}
#[test]
fn unconfirmed_release_still_restores_mouse_and_never_becomes_a_successful_close() {
    let mut f = Fixture::new(Faults::default());
    f.activate(json!({"83":"modifier:ctrl"}));
    f.fail_close.set(true);
    assert!(f
        .backend
        .close(deadline())
        .unwrap_err()
        .contains("release unconfirmed"));
    f.assert_released();
    let wire = f.state.borrow().wire.len();
    assert!(f.backend.close(deadline()).is_err());
    assert_eq!(wire, f.state.borrow().wire.len());
}
#[test]
fn changed_layout_cannot_be_recovered_as_if_the_original_device_were_present() {
    let mut f = Fixture::new(Faults::default());
    f.activate(json!({"83":"modifier:ctrl"}));
    f.state.borrow_mut().version = 4;
    let e = f.backend.snapshot(deadline()).unwrap_err();
    assert!(e.contains("cleanup unconfirmed"), "{e}");
    assert!(!f.log.borrow().iter().any(|s| s == "wire:83:2"));
    assert!(f.backend.close(deadline()).is_err());
}
#[test]
fn a_new_plan_releases_and_replaces_the_capture_under_the_same_owner() {
    let mut f = Fixture::new(Faults::default());
    f.activate(json!({"83":"modifier:ctrl"}));
    f.emit(&[]).unwrap();
    f.emit(&[83]).unwrap();
    let c = Changes::new(
        Some(json!({"dpi":1800})),
        Some(json!({"83":"modifier:alt"})),
    )
    .unwrap();
    let limit = deadline();
    f.backend.prepare(&c, limit).unwrap();
    // Preflight opens the replacement output but writes nothing.
    assert_eq!(f.count("output-open"), 2);
    assert_eq!(f.count("output-close"), 0);
    assert_eq!(f.count("wire:83:2"), 0);
    let applied = f.backend.apply(&c, limit).unwrap();
    assert_eq!(applied.confirmed, vec!["dpi", "button:83"]);
    assert_eq!(
        applied.snapshot["buttons"]["controls"][0]["value"],
        "modifier:alt"
    );
    let capture = &applied.snapshot["native_capture"];
    assert_eq!(capture["button_transitions_supported"], true);
    assert_eq!(capture["last_release"]["completed"], true);
    assert_eq!(capture["last_release"]["mode"], "live-reporting-release");
    {
        // Held Ctrl is released, then the old bit is cleared, then re-armed,
        // all through the one descriptor that is still open.
        let log = f.log.borrow();
        let first_close = log.iter().position(|s| s == "output-close").unwrap();
        let clear = log.iter().position(|s| s == "wire:83:2").unwrap();
        let rearm = log.iter().rposition(|s| s == "wire:83:3").unwrap();
        assert!(first_close < clear && clear < rearm, "{log:?}");
        assert!(!log.iter().any(|s| s == "descriptor-close"));
    }
    assert_eq!(f.count("wire:83:3"), 2);
    f.emit(&[]).unwrap();
    f.emit(&[83]).unwrap();
    assert!(
        f.log.borrow().iter().any(|s| s == "held:Alt"),
        "{:?}",
        f.log.borrow()
    );
    f.close();
    assert_eq!(f.count("wire:83:2"), 2);
    assert_eq!(f.count("output-close"), 2);
    assert_eq!(f.state.borrow().flags.get(&83), Some(&0));
}
#[test]
fn switching_to_a_native_only_plan_releases_capture_before_remapping() {
    let mut f = Fixture::new(Faults::default());
    f.activate(json!({"83":"modifier:ctrl"}));
    let receipt = f.activate(json!({"83":83,"86":81}));
    assert_eq!(receipt.confirmed, vec!["button:86"]);
    assert_eq!(receipt.snapshot["native_capture"]["active"], false);
    assert_eq!(
        receipt.snapshot["native_capture"]["recovery_registered"],
        false
    );
    assert_eq!(f.state.borrow().flags.get(&83), Some(&0));
    assert_eq!(f.state.borrow().targets.get(&86), Some(&81));
    f.emit(&[]).unwrap();
    f.emit(&[83]).unwrap();
    assert!(!f.log.borrow().iter().any(|s| s == "held:Ctrl"));
    // Nothing is left to restore: close does not write the cleared bit again.
    f.close();
    assert_eq!(f.count("wire:83:2"), 1);
    assert_eq!(f.count("output-close"), 1);
}
#[test]
fn unconfirmed_live_release_is_not_replayed_and_terminal_cleanup_restores() {
    let mut f = Fixture::new(Faults::default());
    f.activate(json!({"83":"modifier:ctrl"}));
    let c = Changes::new(
        Some(json!({"dpi":1800})),
        Some(json!({"86":"modifier:alt"})),
    )
    .unwrap();
    let limit = deadline();
    f.backend.prepare(&c, limit).unwrap();
    f.drop_clear.set(true);
    let e = f.backend.apply(&c, limit).unwrap_err();
    assert!(e.message.contains("release unconfirmed"), "{}", e.message);
    assert!(e.confirmed.is_empty());
    // No scalar, remap or replacement setter followed the uncertain release.
    assert!(!f
        .state
        .borrow()
        .wire
        .iter()
        .any(|p| p[2] == 14 && p[3] >> 4 == 3));
    assert_eq!(f.count("wire:86:3"), 0);
    // Terminal recovery re-read the bit through the retained descriptor once.
    assert_eq!(f.count("wire:83:2"), 2);
    assert_eq!(f.state.borrow().flags.get(&83), Some(&0));
    assert_eq!(f.count("output-close"), 2);
    assert!(f.backend.snapshot(deadline()).is_err());
}
#[test]
fn discarded_transition_preparation_closes_only_the_replacement_output() {
    let mut f = Fixture::new(Faults::default());
    f.activate(json!({"83":"modifier:ctrl"}));
    let next = Changes::new(None, Some(json!({"83":"modifier:alt"}))).unwrap();
    f.backend.prepare(&next, deadline()).unwrap();
    assert!(f.backend.apply(&next, deadline()).is_err());
    assert_eq!(f.count("wire:83:2"), 1);
    assert_eq!(f.count("wire:83:3"), 1);
    assert_eq!(f.count("output-close"), 2);
    assert_eq!(f.state.borrow().flags.get(&83), Some(&0));
}
#[test]
fn a_partial_remap_keeps_unlisted_software_bindings_captured() {
    let mut f = Fixture::new(Faults::default());
    f.activate(json!({"83":"modifier:ctrl"}));
    let receipt = f.activate(json!({"86":81}));
    assert_eq!(receipt.confirmed, vec!["button:86", "button:83"]);
    assert_eq!(receipt.snapshot["native_capture"]["active"], true);
    assert_eq!(
        receipt.snapshot["buttons"]["controls"][0]["value"],
        "modifier:ctrl"
    );
    assert_eq!(f.state.borrow().targets.get(&86), Some(&81));
    assert_eq!(f.state.borrow().flags.get(&83), Some(&1));
    f.emit(&[]).unwrap();
    f.emit(&[83]).unwrap();
    assert!(f.log.borrow().iter().any(|s| s == "held:Ctrl"));
    f.close();
    assert_eq!(f.state.borrow().flags.get(&83), Some(&0));
}
#[test]
fn identical_plan_and_scalar_updates_do_not_repeat_capture_setters() {
    let mut f = Fixture::new(Faults::default());
    f.activate(json!({"83":"modifier:ctrl"}));
    let c = Changes::new(
        Some(json!({"dpi":1800})),
        Some(json!({"83":"modifier:ctrl"})),
    )
    .unwrap();
    let limit = deadline();
    f.backend.prepare(&c, limit).unwrap();
    let receipt = f.backend.apply(&c, limit).unwrap();
    assert_eq!(receipt.confirmed, vec!["dpi"]);
    assert_eq!(
        f.log.borrow().iter().filter(|s| *s == "wire:83:3").count(),
        1
    );
    f.close();
    f.assert_released();
}
#[test]
fn whole_batch_validation_blocks_output_and_all_setters() {
    for buttons in [
        json!({"83":"modifier:ctrl","86":65535}),
        json!({"83":"modifier:ctrl"}),
    ] {
        let mut f = Fixture::new(Faults::default());
        let dpi = if buttons.get("86").is_some() {
            1800
        } else {
            1801
        };
        let c = Changes::new(Some(json!({"dpi":dpi})), Some(buttons)).unwrap();
        assert!(f.backend.prepare(&c, deadline()).is_err());
        assert!(setters(&f.state.borrow()).is_empty());
        assert!(!f.log.borrow().iter().any(|s| s == "output-open"));
    }
}
#[test]
fn replayed_apply_cannot_repeat_a_setter_and_closes_the_capture() {
    let mut f = Fixture::new(Faults::default());
    let c = Changes::new(None, Some(json!({"83":"modifier:ctrl"}))).unwrap();
    let limit = deadline();
    f.backend.prepare(&c, limit).unwrap();
    f.backend.apply(&c, limit).unwrap();
    assert!(f
        .backend
        .apply(&c, limit)
        .unwrap_err()
        .message
        .contains("preparation"));
    f.assert_released();
}
#[test]
fn replaced_read_only_preparation_closes_its_output_without_diverting() {
    let mut f = Fixture::new(Faults::default());
    let a = Changes::new(None, Some(json!({"83":"modifier:ctrl"}))).unwrap();
    let b = Changes::new(None, Some(json!({"83":"modifier:alt"}))).unwrap();
    f.backend.prepare(&a, deadline()).unwrap();
    f.backend.prepare(&b, deadline()).unwrap();
    assert_eq!(
        f.log
            .borrow()
            .iter()
            .filter(|s| *s == "output-open")
            .count(),
        2
    );
    assert!(setters(&f.state.borrow()).is_empty());
    f.close();
    assert_eq!(
        f.log
            .borrow()
            .iter()
            .filter(|s| *s == "output-close")
            .count(),
        2
    );
}
#[test]
fn drop_is_a_fail_safe_after_a_held_modifier() {
    let mut f = Fixture::new(Faults::default());
    f.activate(json!({"83":"modifier:ctrl"}));
    f.emit(&[]).unwrap();
    f.emit(&[83]).unwrap();
    let (log, state) = (f.log.clone(), f.state.clone());
    drop(f);
    assert_eq!(state.borrow().flags.get(&83), Some(&0));
    let log = log.borrow();
    assert!(
        log.iter().position(|s| s == "output-close").unwrap()
            < log.iter().position(|s| s == "wire:83:2").unwrap()
    );
}
#[test]
fn resident_raw_xy_dispatches_a_gesture_and_restores_both_owned_flags() {
    let mut f = Fixture::new(Faults::default());
    f.activate(json!({"195":{"type":"gestures","threshold":20,"actions":{"up":null,"down":null,"left":null,"right":"shortcut:paste","click":null}}}));
    f.emit(&[]).unwrap();
    f.emit(&[195]).unwrap();
    f.idle.borrow_mut().push_back(initial_raw());
    f.idle
        .borrow_mut()
        .push_back(frame(&[0x11, 1, 18, 0x10], &[0, 30, 0, 0]));
    // One report per poll: the skipped initial report, then the stroke.
    f.backend.poll(Duration::from_millis(5)).unwrap();
    f.backend.poll(Duration::from_millis(5)).unwrap();
    f.emit(&[]).unwrap();
    assert_eq!(
        f.log
            .borrow()
            .iter()
            .filter(|s| s.starts_with("tap:"))
            .count(),
        1
    );
    f.close();
    assert_eq!(f.state.borrow().flags.get(&195), Some(&0));
    assert!(f.log.borrow().iter().any(|s| s == "wire:195:51"));
    assert!(f.log.borrow().iter().any(|s| s == "wire:195:34"));
}
#[test]
fn unavailable_desktop_output_prevents_scalar_and_capture_writes() {
    let mut f = Fixture::new(Faults::default());
    let c = Changes::new(
        Some(json!({"dpi":1800})),
        Some(json!({"83":"desktop:fullscreen"})),
    )
    .unwrap();
    assert!(f
        .backend
        .prepare(&c, deadline())
        .unwrap_err()
        .contains("Desktop output"));
    assert!(setters(&f.state.borrow()).is_empty());
    assert!(!f
        .state
        .borrow()
        .wire
        .iter()
        .any(|p| p[2] == 14 && p[3] >> 4 == 3));
    assert_eq!(
        f.log
            .borrow()
            .iter()
            .filter(|s| *s == "output-close")
            .count(),
        1
    );
}
#[test]
fn changing_the_apply_deadline_cannot_reuse_a_preparation() {
    let mut f = Fixture::new(Faults::default());
    let c = Changes::new(None, Some(json!({"83":"modifier:ctrl"}))).unwrap();
    f.backend.prepare(&c, deadline()).unwrap();
    assert!(f.backend.apply(&c, deadline()).is_err());
    assert!(setters(&f.state.borrow()).is_empty());
    assert_eq!(
        f.log
            .borrow()
            .iter()
            .filter(|s| *s == "output-close")
            .count(),
        1
    );
}

#[test]
fn recovery_that_sent_no_setter_is_handed_to_the_connector_not_made_terminal() {
    for shared in [false, true] {
        let slot = crate::native::profile_capture::DeferredRecovery::default();
        let mut f = Fixture::build(Faults::default(), shared.then(|| slot.clone()));
        f.activate(json!({"83":"modifier:ctrl"}));
        assert_eq!(f.state.borrow().flags.get(&83), Some(&1));
        // Recovery rereads the layout first and sends nothing when it changed.
        f.state.borrow_mut().version = 4;
        let closed = f.backend.close(deadline());
        assert_eq!(f.state.borrow().flags.get(&83), Some(&1));
        if !shared {
            // Without a connector to hand it to, an unconfirmed close stays terminal.
            assert!(closed.unwrap_err().contains("Resident cleanup unconfirmed"));
            assert!(slot.borrow().is_none());
            continue;
        }
        closed.unwrap();
        // The connector later completes it on a fresh reader. As observed after
        // resume, the reconnected mouse already dropped its temporary reporting.
        f.state.borrow_mut().version = 5;
        f.state.borrow_mut().flags.insert(83, 0);
        let (mut device, _) = Device::new();
        device.state = f.state.clone();
        let plan = slot.borrow_mut().take().unwrap();
        let (report, retry) = plan.restore_or_defer(device, Duration::from_secs(1));
        assert_eq!(report["completed"], true, "{report}");
        assert_eq!(report["written"], json!([]));
        assert!(retry.is_none());
    }
}
