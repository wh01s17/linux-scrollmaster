// SPDX-License-Identifier: GPL-3.0-or-later
//! Independent synthetic recovery wire responses, never physical hardware.
use super::*;
use crate::{hidpp::Session, native::ReportingRecovery};
use std::{cell::Cell, collections::VecDeque};

#[derive(Default)]
struct Faults {
    bad_ack: bool,
    no_effect: bool,
    timeout_after_write: Option<u16>,
    remap_after_write: bool,
    rearm_on_final: bool,
    identity_changed: bool,
    late_setter_reply: bool,
    interleave_on_set: usize,
    /// Receiver answers like a Bolt with the mouse link down after resume.
    offline: bool,
}
struct RecoveryDevice {
    device: Device,
    pending: Option<Vec<u8>>,
    events: VecDeque<Vec<u8>>,
    faults: Faults,
    reporting_reads: BTreeMap<u16, usize>,
    dropped: Rc<Cell<usize>>,
}
impl Drop for RecoveryDevice {
    fn drop(&mut self) {
        self.dropped.set(self.dropped.get() + 1);
    }
}
impl Transport for RecoveryDevice {
    fn write_report(&mut self, p: &[u8], deadline: Instant) -> io::Result<()> {
        if self.faults.offline {
            // HID++ 1.0 error 4 (connection request failed), observed on hardware.
            self.pending = Some(vec![0x10, p[1], 0x8f, p[2], p[3], 4, 0]);
            return Ok(());
        }
        if p[2] == 18 && p[3] >> 4 == 2 {
            let cid = u16::from_be_bytes([p[4], p[5]]);
            let count = self.reporting_reads.entry(cid).or_default();
            *count += 1;
            if self.faults.rearm_on_final && cid == 83 && *count == 4 {
                self.device.state.borrow_mut().flags.insert(cid, 1);
            }
        }
        if p[2] != 18 || p[3] >> 4 != 3 {
            return self.device.write_report(p, deadline);
        }
        // Fixed independent vectors: only change-valid bits, never enable values,
        // primary controls, remaps, persistent flags or high reporting flags.
        let cid = u16::from_be_bytes([p[4], p[5]]);
        let valid: u16 = match cid {
            83 | 86 => 0x02,
            195 => 0x22,
            _ => panic!("Primary CID written"),
        };
        assert_eq!(p[0], 0x11);
        assert_eq!(p[6], valid as u8);
        assert!(p[7..].iter().all(|b| *b == 0));
        let mut state = self.device.state.borrow_mut();
        state.wire.push(p.to_vec());
        if !self.faults.no_effect {
            let flags = state.flags.get(&cid).copied().unwrap_or(0);
            state.flags.insert(cid, flags & !(valid >> 1));
        }
        if self.faults.remap_after_write {
            state.targets.insert(cid, 86);
        }
        if self.faults.timeout_after_write == Some(cid) {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let mut ack = frame(p, &p[4..10]);
        if self.faults.bad_ack {
            ack[6] ^= 1;
        }
        self.pending = Some(ack);
        self.events
            .extend((0..self.faults.interleave_on_set).map(|_| notification(&[195])));
        Ok(())
    }
    fn read_report(&mut self, deadline: Instant) -> io::Result<Vec<u8>> {
        if let Some(event) = self.events.pop_front() {
            return Ok(event);
        }
        if let Some(ack) = self.pending.take() {
            if self.faults.late_setter_reply {
                // Past the whole 1 s restore budget, not only one wait slice:
                // a reply inside the budget after a slice is valid.
                std::thread::sleep(
                    deadline.saturating_duration_since(Instant::now())
                        + Duration::from_millis(1100),
                );
            }
            return Ok(ack);
        }
        let mut p = self.device.read_report(deadline)?;
        if self.faults.identity_changed && p[2] == 2 && p[3] >> 4 == 0 {
            p[5..9].copy_from_slice(&[0x55, 0x66, 0x77, 0x88]);
        }
        Ok(p)
    }
}
fn prepare(controls: &[u16]) -> (ReportingRecovery, Rc<RefCell<State>>) {
    let (device, state) = Device::new();
    let plan = ReportingRecovery::prepare(
        &mut Session::new(device),
        1,
        "11223344",
        controls,
        Duration::from_secs(1),
    )
    .unwrap();
    assert!(setters(&state.borrow()).is_empty());
    state.borrow_mut().wire.clear();
    (plan, state)
}
fn transport(
    state: &Rc<RefCell<State>>,
    faults: Faults,
    events: usize,
) -> (RecoveryDevice, Rc<Cell<usize>>) {
    let (mut device, _) = Device::new();
    device.state = state.clone();
    let dropped = Rc::new(Cell::new(0));
    (
        RecoveryDevice {
            device,
            pending: None,
            events: (0..events).map(|_| notification(&[195])).collect(),
            faults,
            reporting_reads: BTreeMap::new(),
            dropped: dropped.clone(),
        },
        dropped,
    )
}
fn assert_failed(report: &Value, pending: &[u16]) {
    assert_eq!(report["completed"], false, "{report}");
    assert_eq!(report["pending"], json!(pending));
    assert!(report["error"].is_string());
}

#[test]
fn recovery_registration_rejects_invalid_lists_and_timeouts_before_io() {
    for controls in [
        vec![],
        vec![80],
        vec![81],
        vec![82],
        vec![196],
        vec![83, 83],
        vec![83, 86, 195, 196],
    ] {
        let (device, state) = Device::new();
        assert!(ReportingRecovery::prepare(
            &mut Session::new(device),
            1,
            "11223344",
            &controls,
            Duration::from_secs(1)
        )
        .is_err());
        assert!(state.borrow().wire.is_empty());
    }
    for timeout in [Duration::ZERO, Duration::from_secs(21), Duration::MAX] {
        let (device, state) = Device::new();
        assert!(ReportingRecovery::prepare(
            &mut Session::new(device),
            1,
            "11223344",
            &[83],
            timeout
        )
        .is_err());
        assert!(state.borrow().wire.is_empty());
    }
}
#[test]
fn registration_requires_real_divertable_physical_capabilities_and_zero_reporting() {
    for case in 0..6 {
        let (device, state) = Device::new();
        let controls = if case == 5 { vec![195] } else { vec![83] };
        match case {
            0 => {
                state.borrow_mut().flags.insert(83, 1);
            }
            1 => state.borrow_mut().table[3][4] &= !0x20,
            2 => state.borrow_mut().table[3][4] |= 0x80,
            3 => state.borrow_mut().table[3][4] &= !0x10,
            4 => {
                state.borrow_mut().targets.insert(83, 65000);
            }
            _ => state.borrow_mut().table[5][8] = 0,
        }
        assert!(ReportingRecovery::prepare(
            &mut Session::new(device),
            1,
            "11223344",
            &controls,
            Duration::from_secs(1)
        )
        .is_err());
        assert!(setters(&state.borrow()).is_empty());
    }
}
#[test]
fn recovery_clears_registered_temporary_flags_with_uncached_confirmation() {
    let (plan, state) = prepare(&[195, 83, 86]);
    state
        .borrow_mut()
        .flags
        .extend([(83, 1), (86, 1), (195, 0x11)]);
    let (io, dropped) = transport(&state, Faults::default(), 0);
    let report = plan.restore(io, Duration::from_secs(1));
    assert_eq!(report["completed"], true, "{report}");
    assert_eq!(report["diversion_enabled_by_recovery"], false);
    assert_eq!(report["confirmed"], json!([83, 86, 195]));
    assert_eq!(report["written"], json!([83, 86, 195]));
    assert_eq!(report["pending"], json!([]));
    assert_eq!(report["attempted"], Value::Null);
    assert_eq!(setters(&state.borrow()).len(), 3);
    assert!(state.borrow().flags.values().all(|v| *v == 0));
    assert_eq!(dropped.get(), 1);
}
#[test]
fn recovery_preserves_current_mapping_unrelated_flags_and_unregistered_controls() {
    let (plan, state) = prepare(&[83, 195]);
    state
        .borrow_mut()
        .flags
        .extend([(83, 0x0575), (86, 1), (195, 0x0575)]);
    state.borrow_mut().targets.extend([(83, 86), (195, 83)]);
    let (io, dropped) = transport(&state, Faults::default(), 0);
    let report = plan.restore(io, Duration::from_secs(1));
    assert_eq!(report["completed"], true, "{report}");
    assert_eq!(state.borrow().targets[&83], 86);
    assert_eq!(state.borrow().targets[&195], 83);
    assert_eq!(state.borrow().flags[&83], 0x0574);
    assert_eq!(state.borrow().flags[&195], 0x0564);
    assert_eq!(state.borrow().flags[&86], 1);
    assert_eq!(setters(&state.borrow()).len(), 2);
    assert_eq!(dropped.get(), 1);
}
#[test]
fn already_restored_controls_are_verified_without_setters() {
    let (plan, state) = prepare(&[83, 86, 195]);
    let (io, dropped) = transport(&state, Faults::default(), 0);
    let report = plan.restore(io, Duration::from_secs(1));
    assert_eq!(report["completed"], true);
    assert_eq!(report["written"], json!([]));
    assert_eq!(report["confirmed"], json!([83, 86, 195]));
    assert!(setters(&state.borrow()).is_empty());
    assert_eq!(dropped.get(), 1);
}
#[test]
fn recovery_revalidates_identity_feature_and_control_layout_before_writing() {
    for case in 0..4 {
        let (plan, state) = prepare(&[83]);
        state.borrow_mut().flags.insert(83, 1);
        let faults = Faults {
            identity_changed: case == 0,
            ..Default::default()
        };
        match case {
            1 => state.borrow_mut().version = 4,
            2 => state.borrow_mut().table[3][4] ^= 0x40,
            3 => {
                state.borrow_mut().table.pop();
            }
            _ => (),
        }
        let (io, dropped) = transport(&state, faults, 0);
        let report = plan.restore(io, Duration::from_secs(1));
        assert_failed(&report, &[83]);
        assert_eq!(report["attempted"], Value::Null);
        assert!(setters(&state.borrow()).is_empty());
        assert_eq!(dropped.get(), 1);
    }
}
#[test]
fn unconfirmed_setter_never_reports_success_or_retries() {
    for faults in [
        Faults {
            bad_ack: true,
            ..Default::default()
        },
        Faults {
            no_effect: true,
            ..Default::default()
        },
        Faults {
            remap_after_write: true,
            ..Default::default()
        },
        Faults {
            timeout_after_write: Some(83),
            ..Default::default()
        },
    ] {
        let (plan, state) = prepare(&[83]);
        state.borrow_mut().flags.insert(83, 1);
        let (io, dropped) = transport(&state, faults, 0);
        let report = plan.restore(io, Duration::from_secs(1));
        assert_failed(&report, &[83]);
        assert_eq!(report["confirmed"], json!([]));
        assert_eq!(report["attempted"], 83);
        assert_eq!(setters(&state.borrow()).len(), 1);
        assert_eq!(dropped.get(), 1);
    }
}
#[test]
fn partial_recovery_keeps_earlier_confirmations_and_uncertain_attempt() {
    let (plan, state) = prepare(&[83, 86, 195]);
    state
        .borrow_mut()
        .flags
        .extend([(83, 1), (86, 1), (195, 0x11)]);
    let (io, dropped) = transport(
        &state,
        Faults {
            timeout_after_write: Some(86),
            ..Default::default()
        },
        0,
    );
    let report = plan.restore(io, Duration::from_secs(1));
    assert_failed(&report, &[86, 195]);
    assert_eq!(report["confirmed"], json!([83]));
    assert_eq!(report["written"], json!([83]));
    assert_eq!(report["attempted"], 86);
    assert_eq!(state.borrow().flags[&195], 0x11);
    assert_eq!(setters(&state.borrow()).len(), 2);
    assert_eq!(dropped.get(), 1);
}
#[test]
fn final_readback_detects_rearmed_controls_and_keeps_remaining_set_unconfirmed() {
    let (plan, state) = prepare(&[83, 86]);
    state.borrow_mut().flags.extend([(83, 1), (86, 1)]);
    let (io, dropped) = transport(
        &state,
        Faults {
            rearm_on_final: true,
            ..Default::default()
        },
        0,
    );
    let report = plan.restore(io, Duration::from_secs(1));
    assert_failed(&report, &[83, 86]);
    assert!(report["error"]
        .as_str()
        .unwrap()
        .contains("final confirmation"));
    assert_eq!(report["confirmed"], json!([]));
    assert_eq!(setters(&state.borrow()).len(), 2);
    assert_eq!(dropped.get(), 1);
}
#[test]
fn fixed_recovery_sink_handles_interleaved_reports_beyond_normal_fifo_capacity() {
    let (plan, state) = prepare(&[195]);
    state.borrow_mut().flags.insert(195, 0x11);
    let (io, dropped) = transport(&state, Faults::default(), 300);
    let report = plan.restore(io, Duration::from_secs(1));
    assert_eq!(report["completed"], true);
    assert_eq!(report["discarded_reports"], 300);
    assert_eq!(setters(&state.borrow()).len(), 1);
    assert_eq!(dropped.get(), 1);
}
#[test]
fn unmatched_report_limit_closes_without_claiming_or_attempting_recovery() {
    let (plan, state) = prepare(&[195]);
    state.borrow_mut().flags.insert(195, 0x11);
    let (io, dropped) = transport(&state, Faults::default(), 4097);
    let report = plan.restore(io, Duration::from_secs(1));
    assert_failed(&report, &[195]);
    assert_eq!(report["discarded_reports"], 4096);
    assert!(report["error"].as_str().unwrap().contains("report limit"));
    assert!(setters(&state.borrow()).is_empty());
    assert_eq!(dropped.get(), 1);
}
#[test]
fn invalid_restore_timeouts_close_without_io() {
    for timeout in [Duration::ZERO, Duration::from_secs(21), Duration::MAX] {
        let (plan, state) = prepare(&[83]);
        let (io, dropped) = transport(&state, Faults::default(), 0);
        let report = plan.restore(io, timeout);
        assert_failed(&report, &[83]);
        assert!(state.borrow().wire.is_empty());
        assert_eq!(dropped.get(), 1);
    }
}
#[test]
fn late_recovery_ack_is_uncertain_and_never_retried() {
    let (plan, state) = prepare(&[83]);
    state.borrow_mut().flags.insert(83, 1);
    let (io, dropped) = transport(
        &state,
        Faults {
            late_setter_reply: true,
            ..Default::default()
        },
        0,
    );
    let report = plan.restore(io, Duration::from_secs(1));
    assert_failed(&report, &[83]);
    assert_eq!(report["attempted"], 83);
    assert_eq!(report["confirmed"], json!([]));
    assert_eq!(setters(&state.borrow()).len(), 1);
    assert_eq!(dropped.get(), 1);
}

#[test]
fn recovery_sink_also_drains_reports_between_setter_and_ack() {
    let (plan, state) = prepare(&[195]);
    state.borrow_mut().flags.insert(195, 0x11);
    let (io, dropped) = transport(
        &state,
        Faults {
            interleave_on_set: 300,
            ..Default::default()
        },
        0,
    );
    let report = plan.restore(io, Duration::from_secs(1));
    assert_eq!(report["completed"], true);
    assert_eq!(report["discarded_reports"], 300);
    assert_eq!(setters(&state.borrow()).len(), 1);
    assert_eq!(dropped.get(), 1);
}

#[test]
fn exceeding_work_limit_after_setter_keeps_the_attempt_uncertain() {
    let (plan, state) = prepare(&[195]);
    state.borrow_mut().flags.insert(195, 0x11);
    let (io, dropped) = transport(
        &state,
        Faults {
            interleave_on_set: 4097,
            ..Default::default()
        },
        0,
    );
    let report = plan.restore(io, Duration::from_secs(1));
    assert_failed(&report, &[195]);
    assert_eq!(report["attempted"], 195);
    assert_eq!(report["discarded_reports"], 4096);
    assert_eq!(report["confirmed"], json!([]));
    assert_eq!(setters(&state.borrow()).len(), 1);
    assert_eq!(dropped.get(), 1);
}
#[test]
fn live_release_requires_the_resident_consumer_and_keeps_the_record_reusable() {
    let (plan, state) = prepare(&[83]);
    state.borrow_mut().flags.insert(83, 1);
    let (io, dropped) = transport(&state, Faults::default(), 0);
    let mut session = Session::new(io);
    let report = plan.release_live(&mut session, Duration::from_secs(1));
    assert_failed(&report, &[83]);
    assert_eq!(report["mode"], "live-reporting-release");
    assert!(state.borrow().wire.is_empty());
    struct Resident;
    impl crate::hidpp::ReportConsumer for Resident {
        fn consume(&mut self, _: &crate::hidpp::Report) -> Result<(), String> {
            Ok(())
        }
    }
    session.install_consumer(Resident).unwrap();
    let report = plan.release_live(&mut session, Duration::from_secs(1));
    assert_eq!(report["completed"], true, "{report}");
    assert_eq!(report["written"], json!([83]));
    assert_eq!(report["discarded_reports"], Value::Null);
    // The descriptor stays open for the owner; the borrowed record still works.
    assert_eq!(dropped.get(), 0);
    let report = plan.release_live(&mut session, Duration::from_secs(1));
    assert_eq!(report["written"], json!([]));
    assert_eq!(state.borrow().flags.get(&83), Some(&0));
}

#[test]
fn recovery_without_a_setter_is_handed_back_and_completes_later() {
    let (plan, state) = prepare(&[83, 86]);
    state.borrow_mut().flags.extend([(83, 1), (86, 1)]);
    let offline = Faults {
        offline: true,
        ..Default::default()
    };
    let (io, _) = transport(&state, offline, 0);
    let (report, retry) = plan.restore_or_defer(io, Duration::from_secs(1));
    assert_failed(&report, &[83, 86]);
    assert_eq!(report["attempted"], Value::Null);
    assert!(setters(&state.borrow()).is_empty());
    // Link back: the retained record rereads and clears only our bits.
    let (io, _) = transport(&state, Faults::default(), 0);
    let (report, retry) = retry.unwrap().restore_or_defer(io, Duration::from_secs(1));
    assert_eq!(report["completed"], true, "{report}");
    assert!(retry.is_none());
    assert_eq!(state.borrow().flags.get(&83), Some(&0));
    assert_eq!(state.borrow().flags.get(&86), Some(&0));
}
#[test]
fn reconnect_that_already_reset_the_device_completes_without_writing() {
    let (plan, state) = prepare(&[83]);
    state.borrow_mut().flags.insert(83, 1);
    let offline = Faults {
        offline: true,
        ..Default::default()
    };
    let (io, _) = transport(&state, offline, 0);
    let (_, retry) = plan.restore_or_defer(io, Duration::from_secs(1));
    // The mouse lost its temporary reporting when the link came back.
    state.borrow_mut().flags.insert(83, 0);
    let (io, _) = transport(&state, Faults::default(), 0);
    let (report, _) = retry.unwrap().restore_or_defer(io, Duration::from_secs(1));
    assert_eq!(report["completed"], true, "{report}");
    assert_eq!(report["written"], json!([]));
    assert!(setters(&state.borrow()).is_empty());
}
#[test]
fn a_possibly_changed_setter_is_never_handed_back() {
    let (plan, state) = prepare(&[83]);
    state.borrow_mut().flags.insert(83, 1);
    let late = Faults {
        late_setter_reply: true,
        ..Default::default()
    };
    let (io, _) = transport(&state, late, 0);
    let (report, retry) = plan.restore_or_defer(io, Duration::from_millis(100));
    assert_eq!(report["attempted"], 83);
    assert!(retry.is_none());
}
