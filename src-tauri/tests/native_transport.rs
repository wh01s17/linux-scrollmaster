// SPDX-License-Identifier: GPL-3.0-or-later
use linux_scrollmaster::hidpp::{
    CodecError, Report, ReportKind, Session, SessionError, Transport, MAX_PENDING_REPORTS,
};
use std::{
    cell::RefCell,
    collections::VecDeque,
    io,
    rc::Rc,
    time::{Duration, Instant},
};

fn fixture(name: &str) -> Vec<u8> {
    include_str!("fixtures/hidpp-n1.txt")
        .lines()
        .filter_map(|line| line.split_once(" = "))
        .find(|(key, _)| *key == name)
        .unwrap()
        .1
        .split_whitespace()
        .map(|byte| u8::from_str_radix(byte, 16).unwrap())
        .collect()
}

struct Consumer {
    log: Rc<RefCell<Vec<Vec<u8>>>>,
    cancelled: Rc<std::cell::Cell<bool>>,
    fail_at: Option<usize>,
}
impl linux_scrollmaster::hidpp::ReportConsumer for Consumer {
    fn consume(&mut self, report: &Report) -> Result<(), String> {
        if self.fail_at == Some(self.log.borrow().len()) {
            return Err("consumer failed".into());
        }
        self.log.borrow_mut().push(report.bytes().to_vec());
        Ok(())
    }
    fn checkpoint(&mut self) -> Result<(), String> {
        if self.cancelled.get() {
            Err("cancelled".into())
        } else {
            Ok(())
        }
    }
}
fn consumer() -> Consumer {
    Consumer {
        log: Rc::default(),
        cancelled: Rc::default(),
        fail_at: None,
    }
}

#[test]
fn installed_consumer_gets_fifo_before_live_reports_without_duplicate_delivery() {
    let a = fixture("notification");
    let mut b = a.clone();
    b[4] = 41;
    let mut c = a.clone();
    c[4] = 42;
    let mut reply = fixture("ping_reply");
    reply[3] = 0x19;
    let (mut s, state) = session(vec![
        Ok(a.clone()),
        Ok(b.clone()),
        Ok(fixture("ping_reply")),
        Ok(c.clone()),
        Ok(reply),
    ]);
    ping(&mut s).unwrap();
    let handler = consumer();
    let log = handler.log.clone();
    let reads = state.borrow().deadlines.len();
    s.install_consumer(handler).unwrap();
    assert_eq!(*log.borrow(), vec![a.clone(), b.clone()]);
    assert_eq!(state.borrow().deadlines.len(), reads);
    assert_eq!(s.pending_len(), 0);
    ping(&mut s).unwrap();
    assert_eq!(*log.borrow(), vec![a, b, c]);
    assert_eq!(s.pending_len(), 0);
}

#[test]
fn active_consumer_handles_more_than_fifo_capacity_during_one_exchange() {
    let mut input: Vec<_> = (0..300).map(|_| Ok(fixture("notification"))).collect();
    input.push(Ok(fixture("ping_reply")));
    let (mut s, state) = session(input);
    let handler = consumer();
    let log = handler.log.clone();
    s.install_consumer(handler).unwrap();
    ping(&mut s).unwrap();
    assert_eq!(log.borrow().len(), 300);
    assert_eq!(s.pending_len(), 0);
    assert_eq!(state.borrow().writes.len(), 1);
    assert!(!s.is_closed());
}

#[test]
fn correlated_replies_and_protocol_errors_are_never_sent_to_consumer() {
    let error = vec![0x10, 1, 0xff, 0, 0x18, 9, 0];
    let mut reply = fixture("ping_reply");
    reply[3] = 0x19;
    let (mut s, _) = session(vec![Ok(error), Ok(fixture("notification")), Ok(reply)]);
    let handler = consumer();
    let log = handler.log.clone();
    s.install_consumer(handler).unwrap();
    assert!(matches!(ping(&mut s), Err(SessionError::Protocol(_))));
    assert!(log.borrow().is_empty());
    assert!(!s.is_closed());
    ping(&mut s).unwrap();
    assert_eq!(log.borrow().len(), 1);
}

#[test]
fn consumer_failure_during_exchange_closes_owner_without_retry() {
    let (mut s, state) = session(vec![Ok(fixture("notification")), Ok(fixture("ping_reply"))]);
    let mut handler = consumer();
    handler.fail_at = Some(0);
    s.install_consumer(handler).unwrap();
    assert!(matches!(ping(&mut s), Err(SessionError::Consumer(_))));
    assert!(s.is_closed());
    assert!(state.borrow().dropped);
    assert_eq!(state.borrow().writes.len(), 1);
    assert!(matches!(ping(&mut s), Err(SessionError::Closed)));
}

#[test]
fn failed_fifo_delivery_keeps_the_undelivered_suffix_and_closes_owner() {
    let a = fixture("notification");
    let mut b = a.clone();
    b[4] = 43;
    let (mut s, state) = session(vec![
        Ok(a.clone()),
        Ok(b.clone()),
        Ok(fixture("ping_reply")),
    ]);
    ping(&mut s).unwrap();
    let mut handler = consumer();
    handler.fail_at = Some(1);
    let log = handler.log.clone();
    assert!(matches!(
        s.install_consumer(handler),
        Err(SessionError::Consumer(_))
    ));
    assert_eq!(*log.borrow(), vec![a]);
    assert!(s.is_closed());
    assert!(state.borrow().dropped);
    assert_eq!(s.pending_len(), 1);
    assert_eq!(s.pop_pending().unwrap().bytes(), b);
}

#[test]
fn consumer_panics_in_delivery_or_health_check_are_caught_and_close_owner() {
    struct Panics(bool);
    impl linux_scrollmaster::hidpp::ReportConsumer for Panics {
        fn consume(&mut self, _: &Report) -> Result<(), String> {
            panic!("synthetic consumer panic")
        }
        fn checkpoint(&mut self) -> Result<(), String> {
            if self.0 {
                panic!("synthetic checkpoint panic")
            }
            Ok(())
        }
    }
    for check in [false, true] {
        let (mut s, state) = session(vec![Ok(fixture("notification")), Ok(fixture("ping_reply"))]);
        if check {
            assert!(matches!(
                s.install_consumer(Panics(true)),
                Err(SessionError::Consumer(_))
            ));
            assert!(state.borrow().writes.is_empty());
        } else {
            s.install_consumer(Panics(false)).unwrap();
            assert!(matches!(ping(&mut s), Err(SessionError::Consumer(_))));
        }
        assert!(s.is_closed());
        assert!(state.borrow().dropped);
    }
}

#[test]
fn consumer_mode_rejects_competing_raw_reads_and_replacement_but_supports_idle_delivery() {
    let (mut s, state) = session(vec![
        Ok(fixture("notification")),
        Err(io::ErrorKind::TimedOut.into()),
    ]);
    assert!(matches!(
        s.poll_consumer(Duration::from_secs(1)),
        Err(SessionError::NoConsumer)
    ));
    let handler = consumer();
    let log = handler.log.clone();
    s.install_consumer(handler).unwrap();
    assert!(matches!(
        s.install_consumer(consumer()),
        Err(SessionError::ConsumerAlreadyInstalled)
    ));
    assert!(matches!(
        s.receive(Duration::from_secs(1)),
        Err(SessionError::ConsumerAlreadyInstalled)
    ));
    assert!(state.borrow().deadlines.is_empty());
    assert!(s.poll_consumer(Duration::from_secs(1)).unwrap());
    assert!(!s.poll_consumer(Duration::from_secs(1)).unwrap());
    assert_eq!(log.borrow().len(), 1);
    assert!(!s.is_closed());
    assert!(matches!(
        s.poll_consumer(Duration::ZERO),
        Err(SessionError::InvalidTimeout)
    ));
    assert!(!s.is_closed());
}

#[test]
fn cancellation_is_checked_before_write_and_after_quiet_request_slices() {
    let (mut s, state) = session(vec![]);
    let handler = consumer();
    let cancelled = handler.cancelled.clone();
    s.install_consumer(handler).unwrap();
    cancelled.set(true);
    assert!(matches!(ping(&mut s), Err(SessionError::Consumer(_))));
    assert!(state.borrow().writes.is_empty());
    assert!(s.is_closed());
    struct Quiet {
        cancelled: Rc<std::cell::Cell<bool>>,
        reads: Rc<std::cell::Cell<usize>>,
    }
    impl Transport for Quiet {
        fn write_report(&mut self, _: &[u8], _: Instant) -> io::Result<()> {
            Ok(())
        }
        fn read_report(&mut self, deadline: Instant) -> io::Result<Vec<u8>> {
            assert!(
                deadline.saturating_duration_since(Instant::now()) <= Duration::from_millis(50)
            );
            self.reads.set(self.reads.get() + 1);
            std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
            self.cancelled.set(true);
            Err(io::ErrorKind::TimedOut.into())
        }
    }
    let handler = consumer();
    let reads = Rc::new(std::cell::Cell::new(0));
    let mut s = Session::new(Quiet {
        cancelled: handler.cancelled.clone(),
        reads: reads.clone(),
    });
    s.install_consumer(handler).unwrap();
    let start = Instant::now();
    assert!(matches!(ping(&mut s), Err(SessionError::Consumer(_))));
    assert_eq!(reads.get(), 1);
    assert!(s.is_closed());
    assert!(start.elapsed() < Duration::from_millis(500));
}

#[test]
fn slow_consumer_cannot_extend_the_request_deadline_or_accept_a_late_ack() {
    struct Slow;
    impl linux_scrollmaster::hidpp::ReportConsumer for Slow {
        fn consume(&mut self, _: &Report) -> Result<(), String> {
            std::thread::sleep(Duration::from_millis(20));
            Ok(())
        }
    }
    let (mut s, state) = session(vec![Ok(fixture("notification")), Ok(fixture("ping_reply"))]);
    s.install_consumer(Slow).unwrap();
    assert!(matches!(
        s.request(
            ReportKind::Short,
            1,
            0,
            1,
            &[0, 0, 0xa5],
            Duration::from_millis(5)
        ),
        Err(SessionError::Io(_))
    ));
    assert!(s.is_closed());
    assert_eq!(state.borrow().writes.len(), 1);
}

#[derive(Default)]
struct State {
    writes: Vec<Vec<u8>>,
    deadlines: Vec<Instant>,
    dropped: bool,
}
struct Script {
    state: Rc<RefCell<State>>,
    input: VecDeque<io::Result<Vec<u8>>>,
    write_error: Option<io::ErrorKind>,
}
impl Drop for Script {
    fn drop(&mut self) {
        self.state.borrow_mut().dropped = true;
    }
}
impl Transport for Script {
    fn write_report(&mut self, report: &[u8], deadline: Instant) -> io::Result<()> {
        let mut state = self.state.borrow_mut();
        state.writes.push(report.to_vec());
        state.deadlines.push(deadline);
        if let Some(kind) = self.write_error {
            return Err(io::Error::from(kind));
        }
        Ok(())
    }
    fn read_report(&mut self, deadline: Instant) -> io::Result<Vec<u8>> {
        self.state.borrow_mut().deadlines.push(deadline);
        self.input
            .pop_front()
            .unwrap_or_else(|| Err(io::ErrorKind::TimedOut.into()))
    }
}
fn session(input: Vec<io::Result<Vec<u8>>>) -> (Session<Script>, Rc<RefCell<State>>) {
    let state = Rc::new(RefCell::new(State::default()));
    (
        Session::new(Script {
            state: state.clone(),
            input: input.into(),
            write_error: None,
        }),
        state,
    )
}
fn ping<T: Transport>(s: &mut Session<T>) -> Result<Report, SessionError> {
    s.request(
        ReportKind::Short,
        1,
        0,
        1,
        &[0, 0, 0xa5],
        Duration::from_secs(1),
    )
}

#[test]
fn independent_vectors_encode_and_decode() {
    assert_eq!(
        Report::request(ReportKind::Short, 1, 0, 1, 8, &[0, 0, 0xa5])
            .unwrap()
            .bytes(),
        fixture("ping_request")
    );
    assert_eq!(
        Report::request(ReportKind::Long, 1, 0, 0, 8, &[0x10, 0])
            .unwrap()
            .bytes(),
        fixture("feature_request")
    );
    for name in [
        "ping_request",
        "ping_reply",
        "feature_request",
        "feature_reply",
        "notification",
        "error_v1",
        "error_v2",
    ] {
        let bytes = fixture(name);
        assert_eq!(Report::decode(&bytes).unwrap().bytes(), bytes);
    }
}

#[test]
fn strict_report_lengths_and_ids() {
    for (id, valid) in [(0x10, 7), (0x11, 20)] {
        for len in 1..=65 {
            let mut bytes = vec![0; len];
            bytes[0] = id;
            assert_eq!(
                Report::decode(&bytes).is_ok(),
                len == valid,
                "id={id} len={len}"
            );
        }
    }
    for bytes in [vec![], vec![0x12; 64], vec![0; 7]] {
        assert!(matches!(
            Report::decode(&bytes),
            Err(CodecError::UnsupportedReport(_))
        ));
    }
}

#[test]
fn request_validates_nibbles_payload_and_padding() {
    for (device, feature, function, software) in [
        (0, 0, 1, 8),
        (1, 0xff, 1, 8),
        (1, 0x8f, 1, 8),
        (1, 0, 16, 8),
        (1, 0, 1, 0),
        (1, 0, 1, 16),
    ] {
        assert_eq!(
            Report::request(ReportKind::Short, device, feature, function, software, &[]),
            Err(CodecError::InvalidRequest)
        );
    }
    for kind in [ReportKind::Short, ReportKind::Long] {
        let max = kind.wire_len() - 4;
        assert!(Report::request(kind, 0xff, 0xfe, 15, 15, &vec![1; max]).is_ok());
        assert_eq!(
            Report::request(kind, 1, 0, 0, 8, &vec![0; max + 1]),
            Err(CodecError::PayloadTooLong)
        );
        assert!(
            Report::request(kind, 1, 0, 0, 8, &[1]).unwrap().payload()[1..]
                .iter()
                .all(|b| *b == 0)
        );
    }
}

#[test]
fn correlation_checks_device_feature_function_and_software() {
    let request = Report::decode(&fixture("ping_request")).unwrap();
    let reply = fixture("ping_reply");
    for index in 1..=3 {
        let mut other = reply.clone();
        other[index] ^= 1;
        assert!(!Report::decode(&other).unwrap().answers(&request));
    }
    let mut other_function = reply.clone();
    other_function[3] ^= 0x10;
    assert!(!Report::decode(&other_function).unwrap().answers(&request));
    let mut long = vec![0; 20];
    long[..7].copy_from_slice(&reply);
    long[0] = 0x11;
    assert!(Report::decode(&long).unwrap().answers(&request));
}

#[test]
fn notifications_and_foreign_replies_keep_arrival_order() {
    let note = fixture("notification");
    let mut foreign = fixture("ping_reply");
    foreign[1] = 2;
    let mut software_zero = fixture("ping_reply");
    software_zero[3] = 0x10;
    let (mut s, state) = session(vec![
        Ok(note.clone()),
        Ok(foreign.clone()),
        Ok(software_zero.clone()),
        Ok(fixture("ping_reply")),
    ]);
    assert_eq!(ping(&mut s).unwrap().bytes(), fixture("ping_reply"));
    for expected in [note, foreign, software_zero] {
        assert_eq!(s.pop_pending().unwrap().bytes(), expected);
    }
    assert_eq!(s.pop_pending(), None);
    let state = state.borrow();
    assert_eq!(state.writes, vec![fixture("ping_request")]);
    assert!(state.deadlines.iter().all(|d| *d == state.deadlines[0]));
}

#[test]
fn both_error_formats_are_correlated_without_retry() {
    for name in ["error_v1", "error_v2"] {
        let (mut s, state) = session(vec![Ok(fixture(name))]);
        let Err(SessionError::Protocol(error)) = ping(&mut s) else {
            panic!("expected protocol error");
        };
        assert_eq!(error.feature, 0);
        assert_eq!(error.function_and_software, 0x18);
        assert_eq!(error.code, if name == "error_v1" { 1 } else { 8 });
        assert!(!s.is_closed());
        assert_eq!(state.borrow().writes.len(), 1);
    }
}

#[test]
fn unrelated_errors_do_not_finish_exchange_and_unknown_codes_survive() {
    let mut foreign = fixture("error_v2");
    foreign[4] = 0x19;
    let mut unknown = fixture("error_v2");
    unknown[5] = 0xe7;
    let (mut s, _) = session(vec![Ok(foreign.clone()), Ok(unknown)]);
    assert!(matches!(ping(&mut s), Err(SessionError::Protocol(error)) if error.code == 0xe7));
    assert_eq!(s.pop_pending().unwrap().bytes(), foreign);
}

#[test]
fn timeout_drops_owner_and_late_reply_cannot_be_reused() {
    let (mut s, state) = session(vec![
        Err(io::ErrorKind::TimedOut.into()),
        Ok(fixture("ping_reply")),
    ]);
    assert!(
        matches!(ping(&mut s), Err(SessionError::Io(error)) if error.kind() == io::ErrorKind::TimedOut)
    );
    assert!(state.borrow().dropped);
    assert!(matches!(ping(&mut s), Err(SessionError::Closed)));
    assert_eq!(state.borrow().writes.len(), 1);
}

#[test]
fn malformed_report_and_disconnect_close_session() {
    for input in [
        Ok(vec![0x10, 1]),
        Ok(vec![0x12; 64]),
        Err(io::ErrorKind::UnexpectedEof.into()),
        Err(io::ErrorKind::PermissionDenied.into()),
    ] {
        let (mut s, state) = session(vec![input]);
        assert!(ping(&mut s).is_err());
        assert!(s.is_closed());
        assert!(state.borrow().dropped);
    }
}

#[test]
fn failed_or_partial_write_is_never_retried() {
    for kind in [
        io::ErrorKind::WriteZero,
        io::ErrorKind::TimedOut,
        io::ErrorKind::BrokenPipe,
    ] {
        let state = Rc::new(RefCell::new(State::default()));
        let mut s = Session::new(Script {
            state: state.clone(),
            input: VecDeque::new(),
            write_error: Some(kind),
        });
        assert!(matches!(ping(&mut s), Err(SessionError::Io(_))));
        assert!(s.is_closed());
        assert_eq!(state.borrow().writes.len(), 1);
        assert_eq!(state.borrow().deadlines.len(), 1);
    }
}

#[test]
fn queue_overflow_is_explicit_and_preserves_previous_reports() {
    let (mut s, state) = session(
        (0..=MAX_PENDING_REPORTS)
            .map(|_| Ok(fixture("notification")))
            .collect(),
    );
    assert!(matches!(ping(&mut s), Err(SessionError::QueueFull)));
    assert!(state.borrow().dropped);
    assert_eq!(
        (0..MAX_PENDING_REPORTS)
            .filter_map(|_| s.pop_pending())
            .count(),
        MAX_PENDING_REPORTS
    );
    assert_eq!(s.pop_pending(), None);
}

#[test]
fn invalid_request_or_timeout_never_writes_or_closes() {
    let (mut s, state) = session(vec![Ok(fixture("ping_reply"))]);
    assert!(matches!(
        s.request(ReportKind::Short, 0, 0, 1, &[], Duration::from_secs(1)),
        Err(SessionError::Codec(_))
    ));
    for timeout in [Duration::ZERO, Duration::MAX] {
        assert!(matches!(
            s.request(ReportKind::Short, 1, 0, 1, &[], timeout),
            Err(SessionError::InvalidTimeout)
        ));
    }
    assert!(state.borrow().writes.is_empty());
    assert!(!s.is_closed());
    assert!(ping(&mut s).is_ok());
}

#[test]
fn successful_requests_rotate_nonzero_software_ids() {
    let input = (0..20)
        .map(|i| {
            let mut b = fixture("ping_reply");
            b[3] = 0x10 | (8 + i % 8);
            Ok(b)
        })
        .collect();
    let (mut s, state) = session(input);
    for _ in 0..20 {
        ping(&mut s).unwrap();
    }
    assert_eq!(state.borrow().writes.len(), 20);
    assert!(!s.is_closed());
}

#[test]
fn idle_receive_uses_pending_first_and_quiet_timeout_is_normal() {
    let note = fixture("notification");
    let (mut s, state) = session(vec![
        Ok(note.clone()),
        Ok(fixture("ping_reply")),
        Ok(note.clone()),
        Err(io::ErrorKind::TimedOut.into()),
    ]);
    ping(&mut s).unwrap();
    let calls = state.borrow().deadlines.len();
    assert_eq!(
        s.receive(Duration::from_secs(1)).unwrap().unwrap().bytes(),
        note
    );
    assert_eq!(calls, state.borrow().deadlines.len());
    assert!(s.receive(Duration::from_secs(1)).unwrap().is_some());
    assert_eq!(s.receive(Duration::from_secs(1)).unwrap(), None);
    assert!(!s.is_closed());
    assert_eq!(state.borrow().writes.len(), 1);
}

#[test]
fn idle_eof_closes_and_releases_owner() {
    let (mut s, state) = session(vec![Err(io::ErrorKind::UnexpectedEof.into())]);
    assert!(s.receive(Duration::from_secs(1)).is_err());
    assert!(s.is_closed());
    assert!(state.borrow().dropped);
}

#[test]
fn late_idle_success_is_rejected_and_closes_owner() {
    struct Late;
    impl Transport for Late {
        fn write_report(&mut self, _: &[u8], _: Instant) -> io::Result<()> {
            panic!("Idle receive must not write")
        }
        fn read_report(&mut self, deadline: Instant) -> io::Result<Vec<u8>> {
            std::thread::sleep(
                deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(1),
            );
            Ok(fixture("notification"))
        }
    }
    let mut session = Session::new(Late);
    assert!(matches!(
        session.receive(Duration::from_millis(2)),
        Err(SessionError::Io(_))
    ));
    assert!(session.is_closed());
    assert!(matches!(
        session.receive(Duration::from_secs(1)),
        Err(SessionError::Closed)
    ));
}

#[test]
fn dropping_session_releases_transport() {
    let (s, state) = session(vec![]);
    assert!(!state.borrow().dropped);
    drop(s);
    assert!(state.borrow().dropped);
}

#[test]
fn late_success_is_rejected_after_total_exchange_deadline() {
    struct Late;
    impl Transport for Late {
        fn write_report(&mut self, _: &[u8], _: Instant) -> io::Result<()> {
            Ok(())
        }
        fn read_report(&mut self, deadline: Instant) -> io::Result<Vec<u8>> {
            if let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
                std::thread::sleep(remaining + Duration::from_millis(1));
            }
            Ok(fixture("ping_reply"))
        }
    }
    let mut s = Session::new(Late);
    assert!(
        matches!(ping(&mut s), Err(SessionError::Io(error)) if error.kind() == io::ErrorKind::TimedOut)
    );
    assert!(s.is_closed());
}

#[test]
fn bolt_pairing_read_has_fixed_register_and_selector_echo() {
    let mut reply = vec![0; 20];
    reply[..8].copy_from_slice(&[0x11, 0xff, 0x83, 0xb5, 0x51, 2, 0x34, 0xb0]);
    let mut foreign = reply.clone();
    foreign[4] = 0x52;
    let (mut s, state) = session(vec![Ok(foreign.clone()), Ok(reply.clone())]);
    assert_eq!(
        s.bolt_pairing_info(1, Duration::from_secs(1))
            .unwrap()
            .bytes(),
        reply
    );
    assert_eq!(
        state.borrow().writes,
        vec![vec![0x10, 0xff, 0x83, 0xb5, 0x51, 0, 0]]
    );
    assert_eq!(s.pop_pending().unwrap().bytes(), foreign);
}

#[test]
fn missing_bolt_slot_is_a_correlated_error_without_retry() {
    let (mut s, state) = session(vec![Ok(vec![0x10, 0xff, 0x8f, 0x83, 0xb5, 2, 0])]);
    assert!(
        matches!(s.bolt_pairing_info(1,Duration::from_secs(1)),Err(SessionError::Protocol(error)) if error.code==2)
    );
    assert_eq!(state.borrow().writes.len(), 1);
    assert!(!s.is_closed());
}

#[test]
fn bolt_pairing_invalid_slot_never_writes() {
    let (mut s, state) = session(vec![]);
    for slot in [0, 7, 255] {
        assert!(s.bolt_pairing_info(slot, Duration::from_secs(1)).is_err());
    }
    assert!(state.borrow().writes.is_empty());
}

#[cfg(target_os = "linux")]
mod linux_io {
    use super::*;
    use linux_scrollmaster::hidpp::Hidraw;
    use std::{
        fs::File,
        os::{
            fd::OwnedFd,
            unix::net::{UnixDatagram, UnixStream},
        },
    };

    fn pair() -> (Hidraw, UnixDatagram) {
        let (owner, peer) = UnixDatagram::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let fd: OwnedFd = owner.into();
        (Hidraw::from_file(File::from(fd)).unwrap(), peer)
    }
    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(1)
    }

    #[test]
    fn session_uses_real_io_with_interleaved_reports() {
        let (io, peer) = pair();
        let responder = std::thread::spawn(move || {
            let mut bytes = [0; 64];
            let n = peer.recv(&mut bytes).unwrap();
            assert_eq!(&bytes[..n], fixture("ping_request"));
            peer.send(&fixture("notification")).unwrap();
            peer.send(&fixture("ping_reply")).unwrap();
        });
        let mut s = Session::new(io);
        assert_eq!(ping(&mut s).unwrap().bytes(), fixture("ping_reply"));
        assert_eq!(s.pop_pending().unwrap().bytes(), fixture("notification"));
        responder.join().unwrap();
    }

    #[test]
    fn real_io_keeps_report_boundaries_and_report_id() {
        let (mut io, peer) = pair();
        io.write_report(&fixture("ping_request"), deadline())
            .unwrap();
        let mut buffer = [0; 64];
        let n = peer.recv(&mut buffer).unwrap();
        assert_eq!(&buffer[..n], fixture("ping_request"));
        for name in ["notification", "ping_reply"] {
            peer.send(&fixture(name)).unwrap();
        }
        assert_eq!(io.read_report(deadline()).unwrap(), fixture("notification"));
        assert_eq!(io.read_report(deadline()).unwrap(), fixture("ping_reply"));
    }

    #[test]
    fn real_io_timeout_and_expired_write_are_bounded() {
        let (mut io, peer) = pair();
        let start = Instant::now();
        assert_eq!(
            io.read_report(start + Duration::from_millis(15))
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(start.elapsed() < Duration::from_secs(1));
        assert_eq!(
            io.write_report(&fixture("ping_request"), Instant::now())
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        peer.set_nonblocking(true).unwrap();
        assert_eq!(
            peer.recv(&mut [0; 64]).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn real_io_backpressure_obeys_write_deadline() {
        let (owner, peer) = UnixDatagram::pair().unwrap();
        owner.set_nonblocking(true).unwrap();
        loop {
            match owner.send(&fixture("ping_request")) {
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                other => panic!("unexpected send: {other:?}"),
            }
        }
        let fd: OwnedFd = owner.into();
        let mut io = Hidraw::from_file(File::from(fd)).unwrap();
        assert_eq!(
            io.write_report(
                &fixture("ping_request"),
                Instant::now() + Duration::from_millis(15)
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::TimedOut
        );
        drop(peer);
    }

    #[test]
    fn real_io_eof_and_oversized_report_fail_explicitly() {
        let (owner, peer) = UnixStream::pair().unwrap();
        let fd: OwnedFd = owner.into();
        let mut io = Hidraw::from_file(File::from(fd)).unwrap();
        drop(peer);
        assert!(matches!(
            io.read_report(deadline()).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof | io::ErrorKind::BrokenPipe
        ));
        let (mut io, peer) = pair();
        let mut oversized = vec![0; 64];
        oversized[0] = 0x11;
        peer.send(&oversized).unwrap();
        let bytes = io.read_report(deadline()).unwrap();
        assert!(matches!(
            Report::decode(&bytes),
            Err(CodecError::Length { .. })
        ));
    }
}
#[test]
fn a_received_report_is_delivered_even_when_delivery_outlasts_the_poll_budget() {
    struct Slow(Rc<RefCell<usize>>);
    impl linux_scrollmaster::hidpp::ReportConsumer for Slow {
        fn consume(&mut self, _: &Report) -> Result<(), String> {
            std::thread::sleep(Duration::from_millis(20));
            *self.0.borrow_mut() += 1;
            Ok(())
        }
    }
    let (mut s, _) = session(vec![
        Ok(fixture("notification")),
        Err(io::ErrorKind::TimedOut.into()),
    ]);
    let delivered = Rc::new(RefCell::new(0));
    s.install_consumer(Slow(delivered.clone())).unwrap();
    assert!(s.poll_consumer(Duration::from_millis(5)).unwrap());
    assert_eq!(*delivered.borrow(), 1);
    assert!(!s.is_closed());
    assert!(!s.poll_consumer(Duration::from_millis(5)).unwrap());
}

/// Delivers each scripted report just after the wait slice it was given,
/// reproducing a read that completes at the poll boundary (seen on hardware).
struct AtBoundary(VecDeque<Vec<u8>>);
impl Transport for AtBoundary {
    fn write_report(&mut self, _: &[u8], _: Instant) -> io::Result<()> {
        Ok(())
    }
    fn read_report(&mut self, deadline: Instant) -> io::Result<Vec<u8>> {
        let report = self.0.pop_front().ok_or(io::ErrorKind::TimedOut)?;
        std::thread::sleep(
            deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(1),
        );
        Ok(report)
    }
}

#[test]
fn resident_poll_keeps_a_report_read_at_the_slice_boundary() {
    let mut s = Session::new(AtBoundary(vec![fixture("notification")].into()));
    let log = consumer();
    let delivered = log.log.clone();
    s.install_consumer(log).unwrap();
    assert!(s.poll_consumer(Duration::from_millis(5)).unwrap());
    assert_eq!(delivered.borrow().len(), 1);
    assert!(!s.is_closed());
}

#[test]
fn request_survives_an_event_read_after_one_wait_slice() {
    // Each read overruns its 50 ms consumer slice but stays inside the 1 s request.
    let mut s = Session::new(AtBoundary(
        vec![fixture("notification"), fixture("ping_reply")].into(),
    ));
    let log = consumer();
    let delivered = log.log.clone();
    s.install_consumer(log).unwrap();
    assert_eq!(ping(&mut s).unwrap().bytes(), fixture("ping_reply"));
    assert_eq!(delivered.borrow().len(), 1);
    assert!(!s.is_closed());
}
