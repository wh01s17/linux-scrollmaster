// SPDX-License-Identifier: GPL-3.0-or-later
//! Independent hand-authored notifications and in-memory transports, no device I/O.
use super::events::{collect, Decoder};
use crate::hidpp::{Report, Session, Transport};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    io,
    time::{Duration, Instant},
};

fn snapshot() -> Value {
    json!({"feature":{"index":18},"controls":[
        {"id":"83","capabilities":{"flags":0x31}},
        {"id":"86","capabilities":{"flags":0x31}},
        {"id":"195","capabilities":{"flags":0x131}}
    ]})
}
fn decoder() -> Decoder {
    Decoder::from_snapshot(&snapshot(), 2).unwrap()
}
fn frame(slot: u8, feature: u8, address: u8, payload: &[u8]) -> Report {
    let mut wire = vec![0; 20];
    wire[..4].copy_from_slice(&[0x11, slot, feature, address]);
    wire[4..4 + payload.len()].copy_from_slice(payload);
    Report::decode(&wire).unwrap()
}
fn buttons(cids: &[u16]) -> Report {
    let p: Vec<_> = cids.iter().flat_map(|c| c.to_be_bytes()).collect();
    frame(2, 18, 0, &p)
}
fn event(d: &mut Decoder, cids: &[u16]) -> Value {
    d.decode(&buttons(cids)).unwrap().unwrap()
}

#[test]
fn first_report_synchronizes_without_inventing_press_edges() {
    let mut d = decoder();
    let initial = event(&mut d, &[83]);
    assert_eq!(
        initial,
        json!({"kind":"buttons","initial":true,"held":[83],"pressed":[],"released":[]})
    );
    assert_eq!(event(&mut d, &[])["released"], json!([83]));
    assert_eq!(event(&mut d, &[83])["pressed"], json!([83]));
}
#[test]
fn unordered_button_lists_produce_set_edges_and_no_duplicate_presses() {
    let mut d = decoder();
    event(&mut d, &[]);
    assert_eq!(event(&mut d, &[195, 83])["pressed"], json!([83, 195]));
    let same = event(&mut d, &[83, 195]);
    assert_eq!(same["pressed"], json!([]));
    assert_eq!(same["released"], json!([]));
    let changed = event(&mut d, &[86, 195]);
    assert_eq!(changed["pressed"], json!([86]));
    assert_eq!(changed["released"], json!([83]));
}
#[test]
fn other_slots_features_replies_errors_and_unknown_events_do_not_change_state() {
    let mut d = decoder();
    event(&mut d, &[83]);
    for report in [
        frame(1, 18, 0, &[]),
        frame(2, 19, 0, &[]),
        frame(2, 18, 8, &[]),
        frame(2, 0xff, 18, &[0, 1]),
        frame(2, 18, 0x20, &[]),
    ] {
        assert!(d.decode(&report).unwrap().is_none());
    }
    assert_eq!(event(&mut d, &[])["released"], json!([83]));
}
#[test]
fn invalid_cid_lists_are_rejected_without_changing_prior_state() {
    for cids in [vec![83, 83], vec![83, 0, 86], vec![80], vec![65535]] {
        let mut d = decoder();
        event(&mut d, &[195]);
        assert!(d.decode(&buttons(&cids)).is_err());
        assert_eq!(event(&mut d, &[])["released"], json!([195]));
    }
}
#[test]
fn short_notifications_are_not_padded_into_plausible_events() {
    for address in [0, 0x10] {
        let report = Report::decode(&[0x10, 2, 18, address, 0, 0, 0]).unwrap();
        assert!(decoder().decode(&report).unwrap_err().contains("Truncated"));
    }
}
#[test]
fn raw_xy_uses_signed_big_endian_and_a_known_raw_capable_held_control() {
    let mut d = decoder();
    let motion = frame(2, 18, 0x10, &[0x80, 0, 0x7f, 0xff]);
    assert!(d.decode(&motion).unwrap().is_none());
    event(&mut d, &[83]);
    assert!(d.decode(&motion).unwrap().is_none());
    event(&mut d, &[195]);
    let raw = d.decode(&motion).unwrap().unwrap();
    assert_eq!(raw["dx"], -32768);
    assert_eq!(raw["dy"], 32767);
    let raw = d
        .decode(&frame(2, 18, 0x10, &[0xff, 0xff, 0, 1]))
        .unwrap()
        .unwrap();
    assert_eq!(raw["dx"], -1);
    assert_eq!(raw["dy"], 1);
    event(&mut d, &[]);
    assert!(d.decode(&motion).unwrap().is_none());
}
#[test]
fn decoder_checks_discovered_index_slots_capabilities_and_duplicates() {
    for index in [0, 143, 255, 256] {
        let mut s = snapshot();
        s["feature"]["index"] = json!(index);
        assert!(Decoder::from_snapshot(&s, 2).is_err());
    }
    for slot in [0, 7, 255] {
        assert!(Decoder::from_snapshot(&snapshot(), slot).is_err());
    }
    for flags in [1, 0x11, 0xb1] {
        let s =
            json!({"feature":{"index":18},"controls":[{"id":"83","capabilities":{"flags":flags}}]});
        assert!(Decoder::from_snapshot(&s, 2).is_err());
    }
    let mut s = snapshot();
    s["controls"][1] = s["controls"][0].clone();
    assert!(Decoder::from_snapshot(&s, 2).is_err());
    let mut s = snapshot();
    s["feature"]["index"] = json!(27);
    let mut d = Decoder::from_snapshot(&s, 2).unwrap();
    assert!(d.decode(&buttons(&[83])).unwrap().is_none());
    assert_eq!(
        d.decode(&frame(2, 27, 0, &[0, 83])).unwrap().unwrap()["held"],
        json!([83])
    );
}

struct Script {
    input: VecDeque<io::Result<Vec<u8>>>,
    reads: usize,
}
impl Transport for Script {
    fn write_report(&mut self, _: &[u8], _: Instant) -> io::Result<()> {
        panic!("Passive collection must not write any requests")
    }
    fn read_report(&mut self, deadline: Instant) -> io::Result<Vec<u8>> {
        self.reads += 1;
        if let Some(next) = self.input.pop_front() {
            return next;
        }
        // Match the deadline contract without busy-spinning on quiet intervals.
        std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        Err(io::ErrorKind::TimedOut.into())
    }
}
fn collect_script(input: Vec<io::Result<Vec<u8>>>) -> Value {
    let session = Session::new(Script {
        input: input.into(),
        reads: 0,
    });
    collect(session, decoder(), Duration::from_millis(100), || false)
}
#[test]
fn quiet_intervals_complete_normally_without_writes() {
    let result = collect_script(vec![]);
    assert_eq!(result["completed"], true);
    assert_eq!(result["stop_reason"], "deadline");
    assert_eq!(result["reports"], 0);
    assert_eq!(result["events"], json!([]));
}
#[test]
fn stream_faults_preserve_partial_evidence_and_clear_held_state() {
    let result = collect_script(vec![
        Ok(buttons(&[195]).bytes().to_vec()),
        Err(io::ErrorKind::BrokenPipe.into()),
    ]);
    assert_eq!(result["completed"], false);
    assert_eq!(result["stop_reason"], "stream-error");
    assert_eq!(result["cleared_held"], json!([195]));
    assert_eq!(result["events"].as_array().unwrap().len(), 1);
    assert!(result["error"].is_string());
}
#[test]
fn invalid_events_or_codec_frames_stop_without_synthesizing_release_events() {
    for (input, reason) in [
        (buttons(&[83, 83]).bytes().to_vec(), "invalid-event"),
        (vec![0x11], "stream-error"),
    ] {
        let result = collect_script(vec![Ok(buttons(&[83]).bytes().to_vec()), Ok(input)]);
        assert_eq!(result["stop_reason"], reason);
        assert_eq!(result["completed"], false);
        assert_eq!(result["cleared_held"], json!([83]));
        assert_eq!(result["events"].as_array().unwrap().len(), 1);
    }
}
#[test]
fn cancellation_is_checked_before_reads_and_after_a_wait() {
    let session = Session::new(Script {
        input: VecDeque::new(),
        reads: 0,
    });
    let start = Instant::now();
    let result = collect(session, decoder(), Duration::from_secs(1), || true);
    assert_eq!(result["stop_reason"], "cancelled");
    assert!(start.elapsed() < Duration::from_millis(100));
    let session = Session::new(Script {
        input: vec![Ok(buttons(&[83]).bytes().to_vec())].into(),
        reads: 0,
    });
    let mut polls = 0;
    let result = collect(session, decoder(), Duration::from_secs(1), move || {
        polls += 1;
        polls == 2
    });
    assert_eq!(result["stop_reason"], "cancelled");
    assert_eq!(result["reports"], 0);
    assert_eq!(result["events"], json!([]));
}
#[test]
fn event_and_report_limits_stop_floods_without_silent_drops() {
    for (report, count, reason) in [
        (buttons(&[]), 128, "event-limit"),
        (frame(1, 18, 0, &[]), 4096, "report-limit"),
    ] {
        let result = collect_script((0..=count).map(|_| Ok(report.bytes().to_vec())).collect());
        assert_eq!(result["stop_reason"], reason);
        assert_eq!(result["reports"], count);
        assert_eq!(result["completed"], false);
    }
}
#[test]
fn invalid_duration_slot_or_early_cancel_fail_before_any_hardware_io() {
    for (duration, slot, cancel) in [
        (Duration::ZERO, 2, false),
        (Duration::from_secs(31), 2, false),
        (Duration::from_secs(1), 0, false),
        (Duration::from_secs(1), 2, true),
    ] {
        struct NoIo;
        impl Transport for NoIo {
            fn write_report(&mut self, _: &[u8], _: Instant) -> io::Result<()> {
                panic!("Unexpected I/O")
            }
            fn read_report(&mut self, _: Instant) -> io::Result<Vec<u8>> {
                panic!("Unexpected I/O")
            }
        }
        assert!(
            super::observe_notifications(NoIo, slot, "11223344", duration, move || cancel).is_err()
        );
    }
}
