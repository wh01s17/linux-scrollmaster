// SPDX-License-Identifier: GPL-3.0-or-later
use super::{CodecError, ErrorReply, Report, ReportKind};
use std::{
    collections::VecDeque,
    fmt, io,
    time::{Duration, Instant},
};

/// Each operation must check the absolute monotonic deadline, including EINTR.
/// A kernel syscall already in progress may return later; do not retry that exchange.
/// Reads return exactly one report, never fragments joined as a byte stream.
pub trait Transport {
    fn write_report(&mut self, report: &[u8], deadline: Instant) -> io::Result<()>;
    fn read_report(&mut self, deadline: Instant) -> io::Result<Vec<u8>>;
}

pub const MAX_PENDING_REPORTS: usize = 128;
const CONSUMER_POLL: Duration = Duration::from_millis(50);

/// Synchronous delivery on the transport owner's thread. Implementations must
/// return promptly and must not perform HID I/O or re-enter the session.
/// Every unmatched report is delivered; filtering belongs to the consumer.
pub trait ReportConsumer {
    fn consume(&mut self, report: &Report) -> Result<(), String>;
    /// Cancellation/health check before I/O and between reports. During a request,
    /// quiet waits are sliced to 50 ms so this runs without waiting for a reply.
    fn checkpoint(&mut self) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug)]
pub enum SessionError {
    Codec(CodecError),
    Io(io::Error),
    Protocol(ErrorReply),
    QueueFull,
    InvalidTimeout,
    Closed,
    Consumer(String),
    ConsumerAlreadyInstalled,
    NoConsumer,
}
impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HID++ session: {self:?}")
    }
}
impl std::error::Error for SessionError {}

/// Owns its transport exclusively: no clone, exposed descriptor or background reader.
/// An uncertain exchange closes the transport. No automatic retry or reset API.
pub struct Session<T> {
    transport: Option<T>,
    software: u8,
    pending: VecDeque<Report>,
    consumer: Option<Box<dyn ReportConsumer>>,
    initialization_cancelled: Option<Box<dyn FnMut() -> bool>>,
}
impl<T: Transport> Session<T> {
    pub(crate) fn has_consumer(&self) -> bool {
        self.consumer.is_some() && !self.is_closed()
    }
    pub fn new(transport: T) -> Self {
        Self {
            transport: Some(transport),
            software: 8,
            pending: VecDeque::new(),
            consumer: None,
            initialization_cancelled: None,
        }
    }

    /// Cancellation before a trusted consumer can be installed. Unmatched reports
    /// still enter the bounded FIFO; this hook never interprets or consumes them.
    /// The installed consumer takes over cancellation when initialization ends.
    pub(crate) fn with_initialization_cancellation(
        transport: T,
        cancelled: impl FnMut() -> bool + 'static,
    ) -> Self {
        let mut session = Self::new(transport);
        session.initialization_cancelled = Some(Box::new(cancelled));
        session
    }

    pub fn is_closed(&self) -> bool {
        self.transport.is_none()
    }
    /// A trusted feature layer rejected identity/layout. Terminal, with no reopen
    /// or consumer replacement; release the unique reader before reporting error.
    pub(crate) fn invalidate(&mut self) {
        self.transport.take();
        self.pending.clear();
    }
    pub fn pop_pending(&mut self) -> Option<Report> {
        self.pending.pop_front()
    }

    /// Reports buffered during exchanges, in arrival order. Inspecting the count
    /// neither consumes reports nor reads the transport.
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Install action/event consumers only after identity/capabilities are trusted.
    /// A fixed recovery-only sink may discard/count reports before validation,
    /// provided it cannot decode events or trigger actions. Delivers the existing
    /// FIFO before new reads. There is no replacement/detachment API: a failed
    /// consumer closes the transport and must never be replayed automatically.
    pub fn install_consumer(
        &mut self,
        consumer: impl ReportConsumer + 'static,
    ) -> Result<(), SessionError> {
        if self.is_closed() {
            return Err(SessionError::Closed);
        }
        if self.consumer.is_some() {
            return Err(SessionError::ConsumerAlreadyInstalled);
        }
        self.consumer = Some(Box::new(consumer));
        self.initialization_cancelled = None;
        let result = (|| {
            self.consumer_checkpoint()?;
            while let Some(report) = self.pending.front().cloned() {
                self.consumer_checkpoint()?;
                self.deliver(&report)?;
                self.pending.pop_front();
            }
            Ok(())
        })();
        if result.is_err() {
            self.transport.take();
        }
        result
    }

    fn consumer_checkpoint(&mut self) -> Result<(), SessionError> {
        if let Some(cancelled) = &mut self.initialization_cancelled {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(cancelled)) {
                Ok(false) => (),
                Ok(true) => return Err(SessionError::Consumer("Initialization cancelled".into())),
                Err(_) => {
                    return Err(SessionError::Consumer(
                        "Initialization cancellation callback panicked".into(),
                    ))
                }
            }
        }
        if let Some(consumer) = &mut self.consumer {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| consumer.checkpoint()))
                .unwrap_or_else(|_| Err("Report consumer checkpoint panicked".into()))
                .map_err(SessionError::Consumer)?;
        }
        Ok(())
    }
    fn deliver(&mut self, report: &Report) -> Result<(), SessionError> {
        let consumer = self.consumer.as_mut().ok_or(SessionError::NoConsumer)?;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| consumer.consume(report)))
            .unwrap_or_else(|_| Err("Report consumer panicked".into()))
            .map_err(SessionError::Consumer)
    }

    /// Pending reports include broadcasts, application events and unmatched replies.
    /// Their interpretation belongs to the future feature/event layer, not this codec.
    fn enqueue(&mut self, report: Report) -> Result<(), SessionError> {
        if self.pending.len() == MAX_PENDING_REPORTS {
            return Err(SessionError::QueueFull);
        }
        self.pending.push_back(report);
        Ok(())
    }

    pub fn request(
        &mut self,
        kind: ReportKind,
        device: u8,
        feature: u8,
        function: u8,
        payload: &[u8],
        timeout: Duration,
    ) -> Result<Report, SessionError> {
        if self.is_closed() {
            return Err(SessionError::Closed);
        }
        let request = Report::request(kind, device, feature, function, self.software, payload)
            .map_err(SessionError::Codec)?;
        let deadline = Instant::now()
            .checked_add(timeout)
            .filter(|_| !timeout.is_zero())
            .ok_or(SessionError::InvalidTimeout)?;
        self.software = if self.software == 15 {
            8
        } else {
            self.software + 1
        };
        self.checked_exchange(&request, deadline, None)
    }

    /// Read only the Bolt pairing-information register (no pairing/unpairing API).
    /// Selector echo is part of correlation, since all slots share the same header.
    pub fn bolt_pairing_info(
        &mut self,
        slot: u8,
        timeout: Duration,
    ) -> Result<Report, SessionError> {
        if self.is_closed() {
            return Err(SessionError::Closed);
        }
        if !(1..=6).contains(&slot) {
            return Err(SessionError::Codec(CodecError::InvalidRequest));
        }
        let selector = 0x50 + slot;
        let request = Report::decode(&[0x10, 0xff, 0x83, 0xb5, selector, 0, 0])
            .map_err(SessionError::Codec)?;
        let deadline = Instant::now()
            .checked_add(timeout)
            .filter(|_| !timeout.is_zero())
            .ok_or(SessionError::InvalidTimeout)?;
        self.checked_exchange(&request, deadline, Some(selector))
    }

    fn checked_exchange(
        &mut self,
        request: &Report,
        deadline: Instant,
        selector: Option<u8>,
    ) -> Result<Report, SessionError> {
        let result = self.exchange(request, deadline, selector);
        if result.is_err() && !matches!(result, Err(SessionError::Protocol(_))) {
            self.transport.take();
        }
        result
    }

    fn exchange(
        &mut self,
        request: &Report,
        deadline: Instant,
        selector: Option<u8>,
    ) -> Result<Report, SessionError> {
        self.consumer_checkpoint()?;
        check_deadline(deadline).map_err(SessionError::Io)?;
        self.transport
            .as_mut()
            .ok_or(SessionError::Closed)?
            .write_report(request.bytes(), deadline)
            .map_err(SessionError::Io)?;
        loop {
            self.consumer_checkpoint()?;
            check_deadline(deadline).map_err(SessionError::Io)?;
            let interruptible = self.consumer.is_some() || self.initialization_cancelled.is_some();
            let read_deadline = if interruptible {
                deadline.min(Instant::now() + CONSUMER_POLL)
            } else {
                deadline
            };
            let bytes = match self
                .transport
                .as_mut()
                .ok_or(SessionError::Closed)?
                .read_report(read_deadline)
            {
                Ok(bytes) => bytes,
                Err(e)
                    if e.kind() == io::ErrorKind::TimedOut
                        && interruptible
                        && Instant::now() < deadline =>
                {
                    continue
                }
                Err(e) => return Err(SessionError::Io(e)),
            };
            self.consumer_checkpoint()?;
            // read_deadline only bounds one interruptible wait slice; a report
            // read just after it is still inside the request budget.
            check_deadline(deadline).map_err(SessionError::Io)?;
            let report = Report::decode(&bytes).map_err(SessionError::Codec)?;
            if report.answers(request)
                && (report.error().is_some() || selector.is_none_or(|s| report.payload()[0] == s))
            {
                return match report.error() {
                    Some(error) => Err(SessionError::Protocol(error)),
                    None => Ok(report),
                };
            }
            if self.consumer.is_some() {
                self.deliver(&report)?;
                check_deadline(deadline).map_err(SessionError::Io)?;
            } else {
                self.enqueue(report)?;
            }
        }
    }

    /// Idle reads use the same owner. A quiet interval is normal; other failures close it.
    pub fn receive(&mut self, timeout: Duration) -> Result<Option<Report>, SessionError> {
        if self.consumer.is_some() {
            return Err(SessionError::ConsumerAlreadyInstalled);
        }
        self.receive_raw(timeout, true)
    }

    /// Idle delivery for an installed consumer; true means one report delivered,
    /// false is a quiet interval. Raw receive cannot compete with this consumer.
    pub fn poll_consumer(&mut self, timeout: Duration) -> Result<bool, SessionError> {
        if self.is_closed() {
            return Err(SessionError::Closed);
        }
        if self.consumer.is_none() {
            return Err(SessionError::NoConsumer);
        }
        let deadline = Instant::now()
            .checked_add(timeout)
            .filter(|_| !timeout.is_zero())
            .ok_or(SessionError::InvalidTimeout)?;
        let result = (|| {
            self.consumer_checkpoint()?;
            check_deadline(deadline).map_err(SessionError::Io)?;
            let report = self.receive_raw(timeout.min(CONSUMER_POLL), false)?;
            self.consumer_checkpoint()?;
            if let Some(report) = report {
                // The budget bounds WAITING. A report already removed from the
                // FIFO is always delivered: dropping it (e.g. a button release)
                // and closing the reader because of scheduler latency is worse.
                self.deliver(&report)?;
                Ok(true)
            } else {
                Ok(false)
            }
        })();
        if result.is_err() {
            self.transport.take();
        }
        result
    }

    /// `strict` rejects a report read after the wait budget (idle `receive`).
    /// Resident polling waits in slices: a report already read at a slice
    /// boundary is decoded, never dropped together with the reader.
    fn receive_raw(
        &mut self,
        timeout: Duration,
        strict: bool,
    ) -> Result<Option<Report>, SessionError> {
        if self.is_closed() {
            return Err(SessionError::Closed);
        }
        let deadline = Instant::now()
            .checked_add(timeout)
            .filter(|_| !timeout.is_zero())
            .ok_or(SessionError::InvalidTimeout)?;
        if let Some(report) = self.pop_pending() {
            return Ok(Some(report));
        }
        let bytes = self
            .transport
            .as_mut()
            .ok_or(SessionError::Closed)?
            .read_report(deadline);
        let result = match bytes {
            Ok(bytes) => (if strict {
                check_deadline(deadline).map(|_| ())
            } else {
                Ok(())
            })
            .map_err(SessionError::Io)
            .and_then(|_| {
                Report::decode(&bytes)
                    .map(Some)
                    .map_err(SessionError::Codec)
            }),
            Err(error) if error.kind() == io::ErrorKind::TimedOut => return Ok(None),
            Err(error) => Err(SessionError::Io(error)),
        };
        if result.is_err() {
            self.transport.take();
        }
        result
    }
}

pub(super) fn check_deadline(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "HID++ deadline elapsed"))
}
