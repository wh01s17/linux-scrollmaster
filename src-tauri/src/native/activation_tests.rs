// SPDX-License-Identifier: GPL-3.0-or-later
//! Independent fixed activation/clear vectors around the N3 synthetic catalogue.
use super::*;
use crate::{
    hidpp::Session,
    native::{
        actions::{Output, Runtime},
        events::SinkHandle,
        ActionBindings, HeldModifiers, KeyboardAction, PassiveEventOwner, ReportingRecovery,
    },
};
use std::{cell::Cell, collections::VecDeque};

#[path = "resident_capture_tests.rs"]
mod resident_capture_tests;

#[derive(Default)]
struct Faults {
    bad_ack: Option<u16>,
    no_effect: Option<u16>,
    timeout: Option<u16>,
    late_ack: bool,
    remap: bool,
    identity_round: Option<usize>,
    layout_round: Option<usize>,
    rearm_final: bool,
    reporting_race: bool,
    before_ack: Vec<Vec<u8>>,
    after_ack: Vec<Vec<u8>>,
    cancel_after_write: bool,
    dpi_lost: bool,
    owner_fifo: Vec<Vec<u8>>,
}
struct Machine {
    device: Device,
    pending: Option<Vec<u8>>,
    events: VecDeque<Vec<u8>>,
    idle: Rc<RefCell<VecDeque<Vec<u8>>>>,
    faults: Faults,
    round: usize,
    report_reads: BTreeMap<u16, usize>,
    cancelled: Rc<Cell<bool>>,
    enabled: bool,
}
impl Transport for Machine {
    fn write_report(&mut self, p: &[u8], deadline: Instant) -> io::Result<()> {
        if p[2] == 0 && p[3] >> 4 == 1 {
            self.round += 1;
            if self.round == 2 {
                self.events
                    .extend(std::mem::take(&mut self.faults.owner_fifo));
            }
            if self.faults.layout_round == Some(self.round) {
                self.device.state.borrow_mut().table[3][4] ^= 0x40;
            }
            if self.faults.rearm_final && self.round == 4 {
                self.device.state.borrow_mut().flags.insert(83, 0);
            }
        }
        if p[2] == 18 && p[3] >> 4 == 2 {
            let cid = u16::from_be_bytes([p[4], p[5]]);
            let reads = self.report_reads.entry(cid).or_default();
            *reads += 1;
            if self.faults.reporting_race && cid == 83 && *reads == 4 {
                self.device.state.borrow_mut().flags.insert(cid, 4);
            }
            if self.enabled {
                self.events
                    .extend(std::mem::take(&mut self.faults.after_ack));
            }
        }
        if p[2] != 18 || p[3] >> 4 != 3 {
            return self.device.write_report(p, deadline);
        }
        let cid = u16::from_be_bytes([p[4], p[5]]);
        let (enable, owned) = match (cid, p[6]) {
            (83 | 86, 3) => (true, 1u16),
            (195, 0x33) => (true, 0x11),
            (83 | 86, 2) => (false, 1),
            (195, 0x22) => (false, 0x11),
            _ => panic!("Unexpected activation/restore vector"),
        };
        assert_eq!(p[0], 0x11);
        assert!(
            p[7..].iter().all(|v| *v == 0),
            "Mapping and unrelated flags must be untouched"
        );
        {
            let mut state = self.device.state.borrow_mut();
            state.wire.push(p.to_vec());
            let flags = state.flags.get(&cid).copied().unwrap_or(0);
            if !enable || self.faults.no_effect != Some(cid) {
                state.flags.insert(
                    cid,
                    if enable {
                        flags | owned
                    } else {
                        flags & !owned
                    },
                );
            }
            if enable && self.faults.remap {
                state.targets.insert(cid, 83);
            }
        }
        if enable {
            self.enabled = true;
            self.events
                .extend(std::mem::take(&mut self.faults.before_ack));
            if self.faults.cancel_after_write {
                self.cancelled.set(true);
            }
            if self.faults.timeout == Some(cid) {
                return Err(io::ErrorKind::TimedOut.into());
            }
        }
        let mut ack = frame(p, &p[4..10]);
        if enable && self.faults.bad_ack == Some(cid) {
            ack[6] ^= 1;
        }
        self.pending = Some(ack);
        Ok(())
    }
    fn read_report(&mut self, deadline: Instant) -> io::Result<Vec<u8>> {
        if let Some(p) = self.events.pop_front() {
            return Ok(p);
        }
        if let Some(p) = self.pending.take() {
            if self.faults.late_ack && p[6] & 1 != 0 {
                // Past the 80 ms activation budget, not only one wait slice.
                std::thread::sleep(
                    deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(100),
                );
            }
            return Ok(p);
        }
        if self.device.pending.is_none() {
            if let Some(p) = self.idle.borrow_mut().pop_front() {
                return Ok(p);
            }
        }
        let mut p = self.device.read_report(deadline)?;
        if self.faults.dpi_lost && self.round >= 3 && p[2] == 14 && p[3] >> 4 == 2 {
            p[5..7].copy_from_slice(&65535u16.to_be_bytes());
        }
        if self.faults.identity_round == Some(self.round) && p[2] == 2 && p[3] >> 4 == 0 {
            p[5..9].copy_from_slice(&[0x55, 0x66, 0x77, 0x88]);
        }
        Ok(p)
    }
}
struct Borrowed<'a>(&'a mut Machine);
impl Transport for Borrowed<'_> {
    fn write_report(&mut self, p: &[u8], deadline: Instant) -> io::Result<()> {
        self.0.write_report(p, deadline)
    }
    fn read_report(&mut self, deadline: Instant) -> io::Result<Vec<u8>> {
        self.0.read_report(deadline)
    }
}
struct Keyboard(Rc<RefCell<Vec<String>>>);
impl Output for Keyboard {
    fn held(&mut self, v: HeldModifiers) -> Result<(), String> {
        self.0.borrow_mut().push(format!("held:{v:?}"));
        Ok(())
    }
    fn tap(&mut self, v: &KeyboardAction) -> Result<(), String> {
        self.0.borrow_mut().push(format!("tap:{:?}", v.code()));
        Ok(())
    }
    fn close(&mut self) -> Result<(), String> {
        self.0.borrow_mut().push("close".into());
        Ok(())
    }
}
struct Context {
    actions: ActionBindings,
    state: Rc<RefCell<State>>,
    idle: Rc<RefCell<VecDeque<Vec<u8>>>>,
    log: Rc<RefCell<Vec<String>>>,
    cancelled: Rc<Cell<bool>>,
}
fn run(
    value: Value,
    faults: Faults,
    duration: Duration,
    check: impl FnOnce(&mut PassiveEventOwner<Borrowed<'_>>, &ReportingRecovery, &Context),
) -> (Value, Value, Vec<String>) {
    let (device, state) = Device::new();
    // A non-default native mapping must survive enabling AND restoration.
    state.borrow_mut().targets.insert(83, 86);
    let actions = ActionBindings::parse(&value).unwrap();
    let expected_mapping = if faults.remap { 83 } else { 86 };
    let expect_recovery = faults.layout_round.is_none() && faults.identity_round.is_none();
    let idle = Rc::new(RefCell::new(VecDeque::new()));
    let cancelled = Rc::new(Cell::new(false));
    let mut machine = Machine {
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
    let recovery = ReportingRecovery::prepare(
        &mut Session::new(Borrowed(&mut machine)),
        1,
        "11223344",
        &actions.controls(),
        Duration::from_secs(1),
    )
    .unwrap();
    let log = Rc::new(RefCell::new(vec![]));
    let sink = SinkHandle::new(
        Runtime::new(
            Keyboard(log.clone()),
            actions.clone(),
            recovery.initial_dpi(),
        )
        .unwrap(),
    );
    let flag = cancelled.clone();
    let mut owner = PassiveEventOwner::open_with_sink(
        Borrowed(&mut machine),
        1,
        "11223344",
        duration,
        move || flag.get(),
        Some(sink.clone()),
    )
    .unwrap();
    let context = Context {
        actions,
        state: state.clone(),
        idle,
        log: log.clone(),
        cancelled,
    };
    check(&mut owner, &recovery, &context);
    let status = owner.stop();
    drop(owner);
    drop(sink);
    let restored = recovery.restore(machine, Duration::from_secs(1));
    assert_eq!(restored["completed"], expect_recovery, "{restored}");
    assert_eq!(
        state.borrow().targets[&83],
        expected_mapping,
        "Preserve the current mapping, including an external change"
    );
    let output = log.borrow().clone();
    (status, restored, output)
}
fn single() -> Value {
    json!({"83":"shortcut:copy"})
}
fn all() -> Value {
    json!({"83":"modifier:ctrl","86":"shortcut:copy","195":{"type":"gestures","threshold":20,
    "actions":{"up":null,"down":null,"left":null,"right":"shortcut:paste","click":"media:mute"}}})
}
fn activate(owner: &mut PassiveEventOwner<Borrowed<'_>>, plan: &ReportingRecovery, c: &Context) {
    owner.activate_registered(&c.actions, plan).unwrap();
}
fn poll(owner: &mut PassiveEventOwner<Borrowed<'_>>, c: &Context, p: Vec<u8>) {
    c.idle.borrow_mut().push_back(p);
    owner.poll(Duration::from_millis(50)).unwrap();
}
fn failed(faults: Faults) -> Value {
    let (status, _, log) = run(
        single(),
        faults,
        Duration::from_secs(1),
        |owner, plan, c| {
            assert!(owner.activate_registered(&c.actions, plan).is_err());
            assert!(!owner.is_running());
        },
    );
    assert_eq!(status["activation"]["completed"], false);
    assert_eq!(log, ["close"]);
    status
}

#[test]
fn three_fixed_setters_preserve_mapping_confirm_uncached_and_refresh_the_new_baseline() {
    let (status, restored, log) = run(
        all(),
        Faults::default(),
        Duration::from_secs(2),
        |owner, plan, c| {
            activate(owner, plan, c);
            assert_eq!(c.state.borrow().flags, json_flags());
            assert_eq!(
                owner.status()["activation"]["confirmed"],
                json!([83, 86, 195])
            );
            owner.refresh(Duration::from_secs(1)).unwrap();
            let wire = setters(&c.state.borrow());
            assert_eq!(wire.iter().map(|p| p[6]).collect::<Vec<_>>(), [3, 3, 0x33]);
            assert!(c.log.borrow().is_empty());
        },
    );
    assert_eq!(status["mode"], "capture-owner");
    assert_eq!(status["read_only"], false);
    assert_eq!(restored["written"], json!([83, 86, 195]));
    assert_eq!(log, ["close"]);
}
fn json_flags() -> BTreeMap<u16, u16> {
    [(83, 1), (86, 1), (195, 0x11)].into()
}
#[test]
fn interleaved_ack_and_readback_events_are_gated_and_held_buttons_are_synchronized() {
    let faults = Faults {
        before_ack: vec![notification(&[]), notification(&[83]), notification(&[])],
        after_ack: vec![notification(&[83])],
        ..Default::default()
    };
    let (status, _, log) = run(
        single(),
        faults,
        Duration::from_secs(1),
        |owner, plan, c| {
            activate(owner, plan, c);
            assert!(c.log.borrow().is_empty());
            assert_eq!(owner.status()["observation"]["sink_deliveries"], 0);
            poll(owner, c, notification(&[83]));
            poll(owner, c, notification(&[]));
            assert!(c.log.borrow().is_empty());
            poll(owner, c, notification(&[83]));
        },
    );
    let events = status["observation"]["events"].as_array().unwrap();
    assert!(events[..4]
        .iter()
        .all(|e| e["during_activation"] == true && e["delivered_to_sink"] == false));
    assert_eq!(log, ["tap:(46, 4)", "close"]);
}
#[test]
fn absent_button_state_never_assumes_an_empty_mouse_at_activation() {
    let (_, _, log) = run(
        single(),
        Faults::default(),
        Duration::from_secs(1),
        |owner, plan, c| {
            activate(owner, plan, c);
            poll(owner, c, notification(&[83]));
            poll(owner, c, notification(&[]));
            assert!(c.log.borrow().is_empty());
            poll(owner, c, notification(&[83]));
        },
    );
    assert_eq!(log, ["tap:(46, 4)", "close"]);
}
#[test]
fn malformed_ack_keeps_the_attempt_uncertain_without_retry() {
    let status = failed(Faults {
        bad_ack: Some(83),
        ..Default::default()
    });
    assert_eq!(status["activation"]["attempted"], 83);
    assert_eq!(status["activation"]["setters_attempted"], 1);
}
#[test]
fn unchanged_firmware_is_not_confused_with_a_successful_ack() {
    let status = failed(Faults {
        no_effect: Some(83),
        ..Default::default()
    });
    assert_eq!(status["activation"]["attempted"], 83);
}
#[test]
fn timeout_after_write_is_restored_without_replaying_the_setter() {
    let status = failed(Faults {
        timeout: Some(83),
        ..Default::default()
    });
    assert_eq!(status["activation"]["setters_attempted"], 1);
}
#[test]
fn late_ack_is_not_accepted_after_the_global_activation_deadline() {
    let (status, _, log) = run(
        single(),
        Faults {
            late_ack: true,
            ..Default::default()
        },
        Duration::from_millis(80),
        |owner, plan, c| {
            assert!(owner.activate_registered(&c.actions, plan).is_err());
        },
    );
    assert_eq!(status["activation"]["attempted"], 83);
    assert_eq!(status["observation"]["completed"], false);
    assert_eq!(log, ["close"]);
}
#[test]
fn mapping_change_during_setter_is_not_overwritten_by_recovery() {
    // The synthetic mapper changes 83->86 to 83->83 despite our zero remap.
    let (status, _, _) = run(
        single(),
        Faults {
            remap: true,
            ..Default::default()
        },
        Duration::from_secs(1),
        |owner, plan, c| {
            assert!(owner.activate_registered(&c.actions, plan).is_err());
        },
    );
    assert_eq!(status["activation"]["completed"], false);
    assert_eq!(status["activation"]["attempted"], 83);
}
#[test]
fn changed_identity_or_layout_before_write_prevents_any_activation_setter() {
    for faults in [
        Faults {
            identity_round: Some(3),
            ..Default::default()
        },
        Faults {
            layout_round: Some(3),
            ..Default::default()
        },
    ] {
        // Nothing was written; no recovery setter is needed after rejection.
        let (device, state) = Device::new();
        let actions = ActionBindings::parse(&single()).unwrap();
        let mut machine = Machine {
            device,
            pending: None,
            events: Default::default(),
            idle: Default::default(),
            faults,
            round: 0,
            report_reads: Default::default(),
            cancelled: Rc::new(Cell::new(false)),
            enabled: false,
        };
        let plan = ReportingRecovery::prepare(
            &mut Session::new(Borrowed(&mut machine)),
            1,
            "11223344",
            &[83],
            Duration::from_secs(1),
        )
        .unwrap();
        let log = Rc::new(RefCell::new(vec![]));
        let sink = SinkHandle::new(Runtime::new(Keyboard(log), actions.clone(), None).unwrap());
        let mut owner = PassiveEventOwner::open_with_sink(
            Borrowed(&mut machine),
            1,
            "11223344",
            Duration::from_secs(1),
            || false,
            Some(sink),
        )
        .unwrap();
        assert!(owner.activate_registered(&actions, &plan).is_err());
        assert!(setters(&state.borrow()).is_empty());
        drop(owner);
    }
}
#[test]
fn partial_batch_keeps_earlier_confirmations_and_restores_every_registered_control() {
    let (status, restored, log) = run(
        all(),
        Faults {
            bad_ack: Some(86),
            ..Default::default()
        },
        Duration::from_secs(1),
        |owner, plan, c| {
            assert!(owner.activate_registered(&c.actions, plan).is_err());
        },
    );
    assert_eq!(status["activation"]["confirmed"], json!([83]));
    assert_eq!(status["activation"]["attempted"], 86);
    assert_eq!(status["activation"]["setters_attempted"], 2);
    assert_eq!(restored["written"], json!([83, 86]));
    assert_eq!(log, ["close"]);
}
#[test]
fn final_full_readback_detects_a_changed_previously_confirmed_control() {
    let status = failed(Faults {
        rearm_final: true,
        ..Default::default()
    });
    assert_eq!(status["activation"]["confirmed"], json!([83]));
    assert_eq!(status["activation"]["hardware_confirmed"], false);
}
#[test]
fn reporting_race_between_preflight_and_write_sends_no_setter_and_preserves_unrelated_flags() {
    let (status, restored, _) = run(
        single(),
        Faults {
            reporting_race: true,
            ..Default::default()
        },
        Duration::from_secs(1),
        |owner, plan, c| {
            assert!(owner.activate_registered(&c.actions, plan).is_err());
            assert!(setters(&c.state.borrow()).is_empty());
        },
    );
    assert_eq!(status["activation"]["setters_attempted"], 0);
    assert_eq!(restored["written"], json!([]));
}
#[test]
fn cancellation_after_a_write_is_uncertain_and_does_not_cancel_recovery() {
    let (status, restored, log) = run(
        single(),
        Faults {
            cancel_after_write: true,
            ..Default::default()
        },
        Duration::from_secs(1),
        |owner, plan, c| {
            assert!(owner.activate_registered(&c.actions, plan).is_err());
            assert!(c.cancelled.get());
        },
    );
    assert_eq!(status["activation"]["attempted"], 83);
    assert_eq!(restored["written"], json!([83]));
    assert_eq!(log, ["close"]);
}
#[test]
fn saturation_before_activation_ack_never_dispatches_actions_or_retries() {
    let status = failed(Faults {
        before_ack: vec![notification(&[83]); 129],
        ..Default::default()
    });
    assert_eq!(status["activation"]["attempted"], 83);
    assert_eq!(status["observation"]["sink_deliveries"], 0);
}
#[test]
fn activation_cannot_be_repeated_on_an_already_armed_owner() {
    let _ = run(
        single(),
        Faults::default(),
        Duration::from_secs(1),
        |owner, plan, c| {
            activate(owner, plan, c);
            let writes = setters(&c.state.borrow()).len();
            assert!(owner.activate_registered(&c.actions, plan).is_err());
            assert_eq!(setters(&c.state.borrow()).len(), writes);
            assert!(owner.is_running());
        },
    );
}

#[test]
fn a_different_recovery_control_set_cannot_authorize_activation() {
    let (status, _, log) = run(
        all(),
        Faults::default(),
        Duration::from_secs(1),
        |owner, plan, c| {
            assert!(owner
                .activate_registered(&ActionBindings::parse(&single()).unwrap(), plan)
                .is_err());
            assert!(setters(&c.state.borrow()).is_empty());
        },
    );
    assert_eq!(status["activation"]["setters_attempted"], 0);
    assert_eq!(log, ["close"]);
}
#[test]
fn each_setter_requires_fresh_capabilities_and_recovery_refuses_a_changed_layout() {
    let (status, restored, log) = run(
        all(),
        Faults {
            layout_round: Some(4),
            ..Default::default()
        },
        Duration::from_secs(1),
        |owner, plan, c| {
            assert!(owner.activate_registered(&c.actions, plan).is_err());
            assert_eq!(setters(&c.state.borrow()).len(), 1);
        },
    );
    assert_eq!(status["activation"]["confirmed"], json!([83]));
    assert_eq!(restored["completed"], false);
    assert_eq!(restored["pending"], json!([83, 86, 195]));
    assert_eq!(log, ["close"]);
}
#[test]
fn lost_dpi_prevents_physical_gesture_activation_before_any_setter() {
    let mut config = all()["195"].clone();
    config["units"] = json!("tenths-mm");
    config["threshold"] = json!(10);
    let (status, _, log) = run(
        json!({"195":config}),
        Faults {
            dpi_lost: true,
            ..Default::default()
        },
        Duration::from_secs(1),
        |owner, plan, c| {
            assert!(owner.activate_registered(&c.actions, plan).is_err());
            assert!(setters(&c.state.borrow()).is_empty());
        },
    );
    assert_eq!(status["activation"]["setters_attempted"], 0);
    assert_eq!(log, ["close"]);
}

#[test]
fn initialization_history_cannot_seed_a_post_activation_press_edge() {
    let (_, _, log) = run(
        single(),
        Faults {
            owner_fifo: vec![notification(&[])],
            ..Default::default()
        },
        Duration::from_secs(1),
        |owner, plan, c| {
            activate(owner, plan, c);
            poll(owner, c, notification(&[83]));
            poll(owner, c, notification(&[]));
            assert!(c.log.borrow().is_empty());
            poll(owner, c, notification(&[83]));
        },
    );
    assert_eq!(log, ["tap:(46, 4)", "close"]);
}
#[test]
fn an_output_that_already_processed_events_cannot_authorize_capture() {
    let (status, _, log) = run(
        single(),
        Faults::default(),
        Duration::from_secs(1),
        |owner, plan, c| {
            poll(owner, c, notification(&[]));
            assert!(owner.activate_registered(&c.actions, plan).is_err());
            assert!(setters(&c.state.borrow()).is_empty());
            assert!(!owner.is_running());
        },
    );
    assert_eq!(status["activation"]["setters_attempted"], 0);
    assert_eq!(log, ["close"]);
}
#[test]
fn a_passive_owner_without_output_cannot_send_capture_setters() {
    let (device, state) = Device::new();
    let plan = ReportingRecovery::prepare(
        &mut Session::new(device),
        1,
        "11223344",
        &[83],
        Duration::from_secs(1),
    )
    .unwrap();
    let (mut device, _) = Device::new();
    device.state = state.clone();
    let mut owner =
        PassiveEventOwner::open(device, 1, "11223344", Duration::from_secs(1), || false).unwrap();
    assert!(owner
        .activate_registered(&ActionBindings::parse(&single()).unwrap(), &plan)
        .is_err());
    assert!(setters(&state.borrow()).is_empty());
    assert!(!owner.is_running());
}
