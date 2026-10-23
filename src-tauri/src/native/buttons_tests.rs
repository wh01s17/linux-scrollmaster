// SPDX-License-Identifier: GPL-3.0-or-later
//! Hand-authored 1B04 responses, never live hardware. Scalar identity is synthetic.
use super::{apply_button_changes, read_buttons, tests::Mouse};
use crate::hidpp::Transport;
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    io,
    rc::Rc,
    time::{Duration, Instant},
};

#[path = "action_owner_tests.rs"]
mod action_owner_tests;
#[path = "activation_tests.rs"]
mod activation_tests;
#[path = "guardian_tests.rs"]
mod guardian_tests;
#[path = "owner_tests.rs"]
mod owner_tests;
#[path = "profile_backend_tests.rs"]
mod profile_backend_tests;
#[path = "recovery_tests.rs"]
mod recovery_tests;

struct State {
    table: Vec<Vec<u8>>,
    targets: BTreeMap<u16, u16>,
    flags: BTreeMap<u16, u16>,
    wire: Vec<Vec<u8>>,
    version: u8,
    ignore_set: bool,
    bad_ack: bool,
    bad_cid: bool,
    short_info: bool,
    short_reporting: bool,
    fail_source: Option<u16>,
    late_flags: bool,
    reports: usize,
}
impl State {
    fn new() -> Self {
        let mut table = Vec::new();
        for (cid, task, flags, group, mask, extra) in [
            (80u16, 1u16, 1, 1, 0, 0),
            (81, 2, 1, 1, 0, 0),
            (82, 3, 1, 1, 0, 0),
            (83, 4, 0x31, 1, 1, 0),
            (86, 5, 0x31, 1, 1, 0),
            (195, 6, 0x31, 1, 1, 1),
            (196, 7, 0x80, 1, 0, 0),
        ] {
            let c = cid.to_be_bytes();
            let t = task.to_be_bytes();
            table.push(vec![c[0], c[1], t[0], t[1], flags, 0, group, mask, extra]);
        }
        Self {
            table,
            targets: BTreeMap::new(),
            flags: BTreeMap::new(),
            wire: vec![],
            version: 5,
            ignore_set: false,
            bad_ack: false,
            bad_cid: false,
            short_info: false,
            short_reporting: false,
            fail_source: None,
            late_flags: false,
            reports: 0,
        }
    }
}
struct Device {
    base: Mouse,
    state: Rc<RefCell<State>>,
    pending: Option<Vec<u8>>,
}
impl Device {
    fn new() -> (Self, Rc<RefCell<State>>) {
        let mut base = Mouse::new();
        base.features.insert(0x1b04, 18);
        let state = Rc::new(RefCell::new(State::new()));
        (
            Self {
                base,
                state: state.clone(),
                pending: None,
            },
            state,
        )
    }
}
fn frame(request: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut report = vec![0; 20];
    report[..4].copy_from_slice(&request[..4]);
    report[0] = 0x11;
    report[4..4 + payload.len()].copy_from_slice(payload);
    report
}
impl Transport for Device {
    fn write_report(&mut self, p: &[u8], deadline: Instant) -> io::Result<()> {
        self.state.borrow_mut().wire.push(p.to_vec());
        if p[2] == 0 && p[3] >> 4 == 0 && p[4..6] == [0x1b, 4] {
            self.pending = Some(frame(p, &[18, 0, self.state.borrow().version]));
            return Ok(());
        }
        if p[2] != 18 {
            return self.base.write_report(p, deadline);
        }
        let mut state = self.state.borrow_mut();
        let function = p[3] >> 4;
        let payload = match function {
            0 => vec![state.table.len() as u8],
            1 => {
                let value = state.table[p[4] as usize].clone();
                if state.short_info {
                    let mut short = frame(p, &[]);
                    short[0] = 0x10;
                    short.truncate(7);
                    self.pending = Some(short);
                    return Ok(());
                }
                value
            }
            2 => {
                state.reports += 1;
                let cid = u16::from_be_bytes([p[4], p[5]]);
                let echo = if state.bad_cid { cid + 1 } else { cid }.to_be_bytes();
                let target = state.targets.get(&cid).copied().unwrap_or(0).to_be_bytes();
                let flags = if state.late_flags && state.reports > 3 {
                    1
                } else {
                    state.flags.get(&cid).copied().unwrap_or(0)
                };
                if state.short_reporting {
                    let mut short = frame(p, &[]);
                    short[0] = 0x10;
                    short.truncate(7);
                    self.pending = Some(short);
                    return Ok(());
                }
                vec![
                    echo[0],
                    echo[1],
                    flags as u8,
                    target[0],
                    target[1],
                    (flags >> 8) as u8,
                ]
            }
            3 => {
                assert_eq!(p[0], 0x11);
                assert_eq!(p[4..].len(), 16);
                assert_eq!(
                    p[6], 0,
                    "Reporting/change-valid low bits must never be sent"
                );
                assert!(
                    p[9..].iter().all(|b| *b == 0),
                    "High reporting bits/padding must stay zero"
                );
                let cid = u16::from_be_bytes([p[4], p[5]]);
                assert!([83, 86, 195].contains(&cid), "Primary control was written");
                if state.fail_source == Some(cid) {
                    return Err(io::ErrorKind::TimedOut.into());
                }
                let target = u16::from_be_bytes([p[7], p[8]]);
                if !state.ignore_set {
                    state.targets.insert(cid, target);
                }
                let mut echo = p[4..10].to_vec();
                if state.bad_ack {
                    echo[4] ^= 1;
                }
                echo
            }
            other => panic!("Non-catalogued 1B04 function {other}"),
        };
        self.pending = Some(frame(p, &payload));
        Ok(())
    }
    fn read_report(&mut self, deadline: Instant) -> io::Result<Vec<u8>> {
        if let Some(reply) = self.pending.take() {
            Ok(reply)
        } else {
            self.base.read_report(deadline)
        }
    }
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(2)
}
fn read(device: Device) -> Result<Value, String> {
    read_buttons(device, 1, "11223344", deadline())
}
fn apply(device: Device, changes: Value) -> Result<Value, super::ApplyError> {
    apply_button_changes(device, 1, "11223344", &changes, deadline())
}
fn setters(state: &State) -> Vec<Vec<u8>> {
    state
        .wire
        .iter()
        .filter(|p| p[2] == 18 && p[3] >> 4 == 3)
        .cloned()
        .collect()
}

#[test]
fn real_groups_supply_native_choices_and_only_secondary_sources_are_exposed() {
    let (device, state) = Device::new();
    let data = read(device).unwrap();
    assert_eq!(
        data["controls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["83", "86", "195"]
    );
    for control in data["controls"].as_array().unwrap() {
        assert_eq!(control["choices"].as_array().unwrap().len(), 7);
        assert_eq!(control["available"], true);
        assert_eq!(
            control["value"].as_u64().unwrap().to_string(),
            control["id"].as_str().unwrap()
        );
    }
    assert_eq!(data["read_only"], true);
    assert!(setters(&state.borrow()).is_empty());
}
#[test]
fn group_masks_are_not_replaced_by_fixed_model_defaults() {
    let (device, state) = Device::new();
    state.borrow_mut().table[3][7] = 2;
    state.borrow_mut().table[6][6] = 2;
    let data = read(device).unwrap();
    let choices = data["controls"][0]["choices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["value"].as_u64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(choices, vec![83, 196]);
}
#[test]
fn table_duplicates_invalid_groups_counts_and_truncation_fail_closed() {
    for fault in 0..5 {
        let (device, state) = Device::new();
        match fault {
            0 => state.borrow_mut().table[4][1] = 83,
            1 => state.borrow_mut().table[0][6] = 9,
            2 => state.borrow_mut().table.clear(),
            3 => state.borrow_mut().table = vec![vec![0; 9]; 65],
            4 => state.borrow_mut().short_info = true,
            _ => unreachable!(),
        }
        assert!(read(device).is_err());
        assert!(setters(&state.borrow()).is_empty());
    }
}
#[test]
fn unsupported_revision_is_not_silently_accepted() {
    for version in [0, 1, 6, 255] {
        let (device, state) = Device::new();
        state.borrow_mut().version = version;
        assert!(read(device).is_err());
        assert!(!state.borrow().wire.iter().any(|p| p[2] == 18));
    }
}
#[test]
fn reporting_echo_and_length_are_strict_and_never_fall_back_to_factory() {
    for truncated in [false, true] {
        let (device, state) = Device::new();
        state.borrow_mut().short_reporting = truncated;
        state.borrow_mut().bad_cid = !truncated;
        assert!(read(device).is_err());
    }
}
#[test]
fn every_nonzero_reporting_flag_blocks_remapping() {
    for flags in [1, 4, 0x10, 0x40, 0x100, 0x200, 0x400, 0x8000] {
        let (device, state) = Device::new();
        state.borrow_mut().flags.insert(83, flags);
        assert!(apply(device, json!({"83":86})).is_err());
        assert!(setters(&state.borrow()).is_empty());
    }
}
#[test]
fn current_targets_outside_groups_are_unavailable() {
    let (device, state) = Device::new();
    state.borrow_mut().targets.insert(83, 999);
    let data = read(device).unwrap();
    assert_eq!(data["controls"][0]["available"], false);
    assert!(data["controls"][0]["value"].is_null());
}
#[test]
fn primary_sources_and_software_actions_are_rejected_before_io() {
    for changes in [
        json!({"80":83}),
        json!({"81":83}),
        json!({"82":83}),
        json!({"083":86}),
        json!({"83":true}),
        json!({"83":86.0}),
        json!({"83":"modifier:alt"}),
        json!({"195":{"type":"gesture"}}),
        json!({"83":0}),
        json!({}),
    ] {
        let (device, state) = Device::new();
        assert!(apply(device, changes).is_err());
        assert!(state.borrow().wire.is_empty());
    }
}
#[test]
fn full_batch_is_validated_before_the_first_setter() {
    let (device, state) = Device::new();
    let e = apply(device, json!({"83":86,"86":999})).unwrap_err();
    assert!(e.confirmed.is_empty());
    assert!(e.attempted.is_none());
    assert!(setters(&state.borrow()).is_empty());
}
#[test]
fn stable_identity_is_checked_before_button_queries() {
    let (device, state) = Device::new();
    assert!(apply_button_changes(device, 1, "55667788", &json!({"83":86}), deadline()).is_err());
    assert!(!state.borrow().wire.iter().any(|p| p[2] == 18));
}
#[test]
fn readback_confirms_mapping_and_no_reporting_bit_is_modified() {
    let (device, state) = Device::new();
    let result = apply(device, json!({"83":86})).unwrap();
    assert_eq!(result["confirmed"], json!(["button:83"]));
    assert_eq!(result["snapshot"]["controls"][0]["value"], 86);
    let sets = setters(&state.borrow());
    assert_eq!(sets.len(), 1);
    assert_eq!(&sets[0][4..10], &[0, 83, 0, 0, 86, 0]);
    assert!(
        state
            .borrow()
            .wire
            .iter()
            .filter(|p| p[2] == 18 && p[3] >> 4 == 2)
            .count()
            >= 7
    );
}
#[test]
fn restore_self_uses_explicit_cid_instead_of_zero_keep_current() {
    let (device, state) = Device::new();
    state.borrow_mut().targets.insert(83, 86);
    let result = apply(device, json!({"83":83})).unwrap();
    assert_eq!(result["snapshot"]["controls"][0]["value"], 83);
    assert_eq!(&setters(&state.borrow())[0][7..9], &[0, 83]);
}
#[test]
fn matching_native_mapping_is_a_noop() {
    let (device, state) = Device::new();
    assert_eq!(
        apply(device, json!({"83":83})).unwrap()["confirmed"],
        json!([])
    );
    assert!(setters(&state.borrow()).is_empty());
}
#[test]
fn ack_and_unchanged_firmware_are_not_confused_with_success() {
    let (device, state) = Device::new();
    state.borrow_mut().ignore_set = true;
    let error = apply(device, json!({"83":86})).unwrap_err();
    assert!(error.message.contains("not confirmed"));
    assert!(error.confirmed.is_empty());
    assert_eq!(setters(&state.borrow()).len(), 1);
}
#[test]
fn malformed_ack_never_reports_confirmation_or_retries() {
    let (device, state) = Device::new();
    state.borrow_mut().bad_ack = true;
    let error = apply(device, json!({"83":86})).unwrap_err();
    assert!(error.message.contains("ACK"));
    assert!(error.confirmed.is_empty());
    assert_eq!(error.attempted.as_deref(), Some("button:83"));
    assert_eq!(setters(&state.borrow()).len(), 1);
}
#[test]
fn partial_failure_reports_earlier_confirmations_and_uncertain_button() {
    let (device, state) = Device::new();
    state.borrow_mut().fail_source = Some(86);
    let error = apply(device, json!({"83":86,"86":83})).unwrap_err();
    assert_eq!(error.confirmed, vec!["button:83"]);
    assert_eq!(error.attempted.as_deref(), Some("button:86"));
    assert_eq!(setters(&state.borrow()).len(), 2);
}
#[test]
fn new_diversion_between_preflight_and_write_cancels_without_setter() {
    let (device, state) = Device::new();
    state.borrow_mut().late_flags = true;
    assert!(apply(device, json!({"83":86}))
        .unwrap_err()
        .message
        .contains("changed since preflight"));
    assert!(setters(&state.borrow()).is_empty());
}

#[test]
fn passive_observation_uses_one_owner_and_only_catalogued_getters() {
    struct Passive {
        device: Device,
        replying: bool,
        idle: std::collections::VecDeque<Vec<u8>>,
    }
    impl Transport for Passive {
        fn write_report(&mut self, p: &[u8], deadline: Instant) -> io::Result<()> {
            self.replying = true;
            self.device.write_report(p, deadline)
        }
        fn read_report(&mut self, deadline: Instant) -> io::Result<Vec<u8>> {
            if self.replying {
                self.replying = false;
                self.device.read_report(deadline)
            } else if let Some(next) = self.idle.pop_front() {
                Ok(next)
            } else {
                std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
                Err(io::ErrorKind::TimedOut.into())
            }
        }
    }
    let (device, state) = Device::new();
    let io = Passive {
        device,
        replying: false,
        idle: vec![
            frame(&[0x11, 1, 18, 0], &[0, 83]),
            frame(&[0x11, 1, 18, 0], &[]),
        ]
        .into(),
    };
    let data = super::observe_notifications(io, 1, "11223344", Duration::from_millis(10), || false)
        .unwrap();
    assert_eq!(data["stage"], "N4");
    assert_eq!(data["read_only"], true);
    assert_eq!(data["diversion_enabled_by_observer"], false);
    assert_eq!(data["observation"]["completed"], true);
    assert_eq!(data["observation"]["queued_at_start"], 0);
    assert_eq!(data["observation"]["events"].as_array().unwrap().len(), 2);
    assert!(data["observation"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .all(|event| event["queued_before_observation"] == false));
    assert_eq!(data["observation"]["events"][1]["released"], json!([83]));
    assert!(setters(&state.borrow()).is_empty());
    // The synthetic scalar device itself rejects any uncatalogued request. Check
    // every additional 1B04 packet explicitly: count/info/reporting getters only.
    assert!(state
        .borrow()
        .wire
        .iter()
        .filter(|p| p[2] == 18)
        .all(|p| p[3] >> 4 <= 2));
}

#[test]
fn passive_observation_identity_mismatch_never_enters_button_catalogue() {
    let (device, state) = Device::new();
    assert!(
        super::observe_notifications(device, 1, "55667788", Duration::from_millis(1), || false)
            .unwrap_err()
            .contains("identity")
    );
    assert!(state.borrow().wire.iter().all(|p| p[2] != 18));
}

// Insert independently constructed reports before selected replies. Scheduling
// across multiple requests catches getters that silently drain after each ACK.
struct Interleaved {
    device: Device,
    schedule: BTreeMap<(u8, u8, usize), Vec<Vec<u8>>>,
    seen: BTreeMap<(u8, u8), usize>,
    incoming: std::collections::VecDeque<Vec<u8>>,
    replying: bool,
}
impl Interleaved {
    fn new(device: Device, schedule: BTreeMap<(u8, u8, usize), Vec<Vec<u8>>>) -> Self {
        Self {
            device,
            schedule,
            seen: BTreeMap::new(),
            incoming: Default::default(),
            replying: false,
        }
    }
}
impl Transport for Interleaved {
    fn write_report(&mut self, p: &[u8], deadline: Instant) -> io::Result<()> {
        self.device.write_report(p, deadline)?;
        let key = (p[2], p[3] >> 4);
        let n = self.seen.entry(key).or_default();
        *n += 1;
        self.incoming.extend(
            self.schedule
                .remove(&(key.0, key.1, *n))
                .unwrap_or_default(),
        );
        self.replying = true;
        Ok(())
    }
    fn read_report(&mut self, deadline: Instant) -> io::Result<Vec<u8>> {
        if let Some(report) = self.incoming.pop_front() {
            return Ok(report);
        }
        if self.replying {
            self.replying = false;
            return self.device.read_report(deadline);
        }
        std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        Err(io::ErrorKind::TimedOut.into())
    }
}
/// First raw XY after a gesture press on 1B04 v5 (measured on the reference mouse):
/// pre-press motion that must be skipped. Opposite to the test strokes.
fn initial_raw() -> Vec<u8> {
    frame(&[0x11, 1, 18, 0x10], &[0xf9, 0xff, 0xff, 0x9a])
}
fn notification(cids: &[u16]) -> Vec<u8> {
    frame(
        &[0x11, 1, 18, 0],
        &cids
            .iter()
            .flat_map(|c| c.to_be_bytes())
            .collect::<Vec<_>>(),
    )
}
fn pending(session: &mut crate::hidpp::Session<Interleaved>) -> Vec<Vec<u8>> {
    let mut reports = vec![];
    while let Some(report) = session.pop_pending() {
        reports.push(report.bytes().to_vec());
    }
    reports
}

#[test]
fn observer_delivers_notifications_interleaved_with_identity_and_button_getters_in_order() {
    let (device, state) = Device::new();
    let motion = frame(&[0x11, 1, 18, 0x10], &[0xff, 0xff, 0, 2]);
    let foreign = frame(&[0x11, 2, 18, 0], &[]);
    let io = Interleaved::new(
        device,
        [
            ((0, 1, 1), vec![notification(&[]), foreign]),
            ((18, 2, 1), vec![notification(&[195]), motion]),
            ((18, 2, 3), vec![notification(&[])]),
        ]
        .into(),
    );
    let result =
        super::observe_notifications(io, 1, "11223344", Duration::from_millis(5), || false)
            .unwrap();
    let observation = &result["observation"];
    assert_eq!(observation["completed"], true);
    assert_eq!(observation["queued_at_start"], 5);
    assert_eq!(observation["reports"], 5);
    assert_eq!(observation["ignored"], 1);
    let events = observation["events"].as_array().unwrap();
    assert_eq!(events.len(), 4);
    assert_eq!(events[0]["initial"], true);
    assert_eq!(events[1]["pressed"], json!([195]));
    assert_eq!(events[2]["kind"], "raw-xy");
    assert_eq!(events[2]["dx"], -1);
    assert_eq!(events[3]["released"], json!([195]));
    assert!(events
        .iter()
        .all(|e| e["queued_before_observation"] == true));
    assert!(setters(&state.borrow()).is_empty());
}

#[test]
fn scalar_write_keeps_preflight_ack_and_confirmation_events_on_the_same_session() {
    let (mut device, state) = Device::new();
    device.base.accept_writes = true;
    let expected = vec![notification(&[]), notification(&[83]), notification(&[])];
    let io = Interleaved::new(
        device,
        [
            ((0, 1, 1), vec![expected[0].clone()]),
            ((14, 3, 1), vec![expected[1].clone()]),
            ((0, 1, 2), vec![expected[2].clone()]),
        ]
        .into(),
    );
    let mut session = crate::hidpp::Session::new(io);
    let data = super::settings::apply_changes_session(
        &mut session,
        1,
        "11223344",
        &json!({"dpi":1800}),
        deadline(),
    )
    .unwrap();
    assert_eq!(data["confirmed"], json!(["dpi"]));
    assert_eq!(data["snapshot"]["device"]["settings"]["dpi"]["value"], 1800);
    assert_eq!(session.pending_len(), 3);
    assert_eq!(pending(&mut session), expected);
    assert!(!session.is_closed());
    assert_eq!(
        state
            .borrow()
            .wire
            .iter()
            .filter(|p| p[2] == 14 && p[3] >> 4 == 3)
            .count(),
        1
    );
}

#[test]
fn button_write_keeps_getter_ack_and_readback_notifications_without_retry() {
    let (device, state) = Device::new();
    let expected = vec![notification(&[]), notification(&[83]), notification(&[])];
    let io = Interleaved::new(
        device,
        [
            ((18, 1, 1), vec![expected[0].clone()]),
            ((18, 3, 1), vec![expected[1].clone()]),
            ((18, 0, 2), vec![expected[2].clone()]),
        ]
        .into(),
    );
    let mut session = crate::hidpp::Session::new(io);
    let data = super::buttons::apply_button_changes_session(
        &mut session,
        1,
        "11223344",
        &json!({"83":86}),
        deadline(),
    )
    .unwrap();
    assert_eq!(data["confirmed"], json!(["button:83"]));
    assert_eq!(session.pending_len(), 3);
    assert_eq!(pending(&mut session), expected);
    assert_eq!(setters(&state.borrow()).len(), 1);
}

#[test]
fn queue_capacity_is_shared_across_getters_and_overflow_aborts_before_a_setter() {
    let (device, state) = Device::new();
    let io = Interleaved::new(
        device,
        [
            ((0, 0, 1), vec![notification(&[83]); 80]),
            ((0, 0, 2), vec![notification(&[]); 49]),
        ]
        .into(),
    );
    let mut session = crate::hidpp::Session::new(io);
    let error = super::buttons::apply_button_changes_session(
        &mut session,
        1,
        "11223344",
        &json!({"83":86}),
        deadline(),
    )
    .unwrap_err();
    assert!(error.message.contains("QueueFull"));
    assert!(error.confirmed.is_empty());
    assert!(error.attempted.is_none());
    assert!(session.is_closed());
    assert_eq!(session.pending_len(), 128);
    assert_eq!(
        pending(&mut session),
        [vec![notification(&[83]); 80], vec![notification(&[]); 48]].concat()
    );
    assert!(setters(&state.borrow()).is_empty());
}

#[test]
fn overflow_during_optional_scalar_read_cannot_become_a_valid_partial_snapshot() {
    let (device, _) = Device::new();
    let io = Interleaved::new(device, [((12, 1, 1), vec![notification(&[]); 129])].into());
    let mut session = crate::hidpp::Session::new(io);
    let error = super::diagnostic::diagnose_session(&mut session, 1, deadline()).unwrap_err();
    assert!(error.contains("transport failed"));
    assert!(session.is_closed());
    assert_eq!(session.pending_len(), 128);
}

#[test]
fn overflow_after_button_ack_reports_uncertain_attempt_and_retains_prior_events() {
    let (device, state) = Device::new();
    let io = Interleaved::new(
        device,
        [
            ((18, 3, 1), vec![notification(&[83]); 128]),
            ((0, 1, 2), vec![notification(&[])]),
        ]
        .into(),
    );
    let mut session = crate::hidpp::Session::new(io);
    let error = super::buttons::apply_button_changes_session(
        &mut session,
        1,
        "11223344",
        &json!({"83":86}),
        deadline(),
    )
    .unwrap_err();
    assert!(error.confirmed.is_empty());
    assert_eq!(error.attempted.as_deref(), Some("button:83"));
    assert!(session.is_closed());
    assert_eq!(session.pending_len(), 128);
    assert_eq!(setters(&state.borrow()).len(), 1);
    assert_eq!(
        state.borrow().targets[&83],
        86,
        "The uncertain write may already have taken effect"
    );
}

#[test]
fn exactly_full_queue_allows_matching_replies_and_can_be_drained_without_loss() {
    let (device, state) = Device::new();
    let expected = [vec![notification(&[83]); 64], vec![notification(&[]); 64]].concat();
    let io = Interleaved::new(
        device,
        [
            ((0, 1, 1), expected[..64].to_vec()),
            ((18, 2, 1), expected[64..].to_vec()),
        ]
        .into(),
    );
    let mut session = crate::hidpp::Session::new(io);
    let data = super::buttons::describe_session(&mut session, 1, "11223344", deadline()).unwrap();
    assert_eq!(data["device"], "11223344");
    assert!(!session.is_closed());
    assert_eq!(session.pending_len(), 128);
    let writes = state.borrow().wire.len();
    assert_eq!(pending(&mut session), expected);
    assert_eq!(session.pending_len(), 0);
    assert_eq!(state.borrow().wire.len(), writes);
}

struct LiveConsumer {
    reports: Rc<RefCell<Vec<Vec<u8>>>>,
    fail: bool,
}
impl crate::hidpp::ReportConsumer for LiveConsumer {
    fn consume(&mut self, report: &crate::hidpp::Report) -> Result<(), String> {
        if self.fail {
            return Err("synthetic consumer failure".into());
        }
        self.reports.borrow_mut().push(report.bytes().to_vec());
        Ok(())
    }
}

#[test]
fn trusted_session_consumes_more_than_queue_capacity_during_a_full_diagnostic() {
    let (device, state) = Device::new();
    let expected = vec![notification(&[]); 300];
    let io = Interleaved::new(device, [((0, 1, 2), expected.clone())].into());
    let mut session = crate::hidpp::Session::new(io);
    // Initial capabilities/identity are validated before installing any consumer.
    super::buttons::describe_session(&mut session, 1, "11223344", deadline()).unwrap();
    let reports = Rc::default();
    session
        .install_consumer(LiveConsumer {
            reports: Rc::clone(&reports),
            fail: false,
        })
        .unwrap();
    let data = super::buttons::describe_session(&mut session, 1, "11223344", deadline()).unwrap();
    assert_eq!(data["device"], "11223344");
    assert_eq!(*reports.borrow(), expected);
    assert_eq!(session.pending_len(), 0);
    assert!(!session.is_closed());
    assert!(setters(&state.borrow()).is_empty());
}

#[test]
fn same_consumer_handles_scalar_and_button_writes_and_their_confirmation_queries() {
    let (mut device, state) = Device::new();
    device.base.accept_writes = true;
    let expected = [
        notification(&[83]),
        notification(&[]),
        notification(&[195]),
        notification(&[]),
    ];
    let io = Interleaved::new(
        device,
        [
            ((14, 3, 1), vec![expected[0].clone()]),
            ((0, 1, 2), vec![expected[1].clone()]),
            ((18, 3, 1), vec![expected[2].clone()]),
            ((18, 0, 3), vec![expected[3].clone()]),
        ]
        .into(),
    );
    let mut session = crate::hidpp::Session::new(io);
    super::buttons::describe_session(&mut session, 1, "11223344", deadline()).unwrap();
    // The second ping precedes the scalar setter, so use the actual arrival order.
    let reports = Rc::default();
    session
        .install_consumer(LiveConsumer {
            reports: Rc::clone(&reports),
            fail: false,
        })
        .unwrap();
    let scalar = super::settings::apply_changes_session(
        &mut session,
        1,
        "11223344",
        &json!({"dpi":1800}),
        deadline(),
    )
    .unwrap();
    assert_eq!(scalar["confirmed"], json!(["dpi"]));
    let buttons = super::buttons::apply_button_changes_session(
        &mut session,
        1,
        "11223344",
        &json!({"83":86}),
        deadline(),
    )
    .unwrap();
    assert_eq!(buttons["confirmed"], json!(["button:83"]));
    // Full 1B04 catalogues: baseline, button preflight, button readback.
    assert_eq!(
        *reports.borrow(),
        vec![
            expected[1].clone(),
            expected[0].clone(),
            expected[2].clone(),
            expected[3].clone()
        ]
    );
    assert_eq!(session.pending_len(), 0);
    assert_eq!(setters(&state.borrow()).len(), 1);
}

#[test]
fn consumer_failure_after_a_setter_keeps_the_attempt_uncertain_and_never_retries() {
    for scalar in [false, true] {
        let (mut device, state) = Device::new();
        device.base.accept_writes = true;
        let (feature, function) = if scalar { (14, 3) } else { (18, 3) };
        let io = Interleaved::new(
            device,
            [((feature, function, 1), vec![notification(&[83])])].into(),
        );
        let mut session = crate::hidpp::Session::new(io);
        super::buttons::describe_session(&mut session, 1, "11223344", deadline()).unwrap();
        session
            .install_consumer(LiveConsumer {
                reports: Rc::default(),
                fail: true,
            })
            .unwrap();
        let error = if scalar {
            super::settings::apply_changes_session(
                &mut session,
                1,
                "11223344",
                &json!({"dpi":1800}),
                deadline(),
            )
            .unwrap_err()
        } else {
            super::buttons::apply_button_changes_session(
                &mut session,
                1,
                "11223344",
                &json!({"83":86}),
                deadline(),
            )
            .unwrap_err()
        };
        assert!(error.message.contains("consumer failure"));
        assert!(error.confirmed.is_empty());
        assert_eq!(
            error.attempted.as_deref(),
            Some(if scalar { "dpi" } else { "button:83" })
        );
        assert!(session.is_closed());
        assert_eq!(
            state
                .borrow()
                .wire
                .iter()
                .filter(|p| p[2] == feature && p[3] >> 4 == function)
                .count(),
            1
        );
        if !scalar {
            assert_eq!(state.borrow().targets[&83], 86);
        }
    }
}
