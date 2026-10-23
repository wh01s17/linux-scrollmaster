// SPDX-License-Identifier: GPL-3.0-or-later
//! Single-reader N4 delivery. Public observers remain passive; no reporting setters.
use super::buttons::describe_session;
use crate::hidpp::{Report, ReportConsumer, Session, Transport};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    time::{Duration, Instant},
};

const MAX_REPORTS: usize = 4096;
const MAX_EVENTS: usize = 128;
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Produced directly from validated wire frames, never reconstructed from logs.
pub(super) enum Event {
    Buttons {
        initial: bool,
        held: Vec<u16>,
        pressed: Vec<u16>,
        released: Vec<u16>,
    },
    RawXY {
        dx: i16,
        dy: i16,
        held: Vec<u16>,
        sources: Vec<u16>,
    },
}
impl Event {
    pub(super) fn evidence(&self) -> Value {
        match self {
            Self::Buttons {
                initial,
                held,
                pressed,
                released,
            } => {
                json!({"kind":"buttons","initial":initial,"held":held,"pressed":pressed,"released":released})
            }
            Self::RawXY { dx, dy, held, .. } => {
                json!({"kind":"raw-xy","dx":dx,"dy":dy,"held":held})
            }
        }
    }
}

/// Internal synchronous boundary. Preparation and query hooks cannot enable HID.
pub(super) trait EventSink {
    fn event(&mut self, _event: &Event) -> Result<(), String> {
        Ok(())
    }
    fn before_query(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn snapshot(&mut self, _snapshot: &Value) -> Result<(), String> {
        Ok(())
    }
    fn close(&mut self) -> Result<(), String>;
}
struct SinkResource {
    sink: Box<dyn EventSink>,
    closed: Option<Result<(), String>>,
}
impl SinkResource {
    fn close(&mut self) -> Result<(), String> {
        if let Some(result) = &self.closed {
            return result.clone();
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.sink.close()))
            .unwrap_or_else(|_| Err("Event sink close panicked".into()));
        self.closed = Some(result.clone());
        result
    }
}
impl Drop for SinkResource {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
/// Shared ownership of one output resource, with one cached close result even
/// when capture, guardian and panic cleanup all request termination.
#[derive(Clone)]
pub(super) struct SinkHandle(Rc<RefCell<SinkResource>>);
impl SinkHandle {
    pub(super) fn new(sink: impl EventSink + 'static) -> Self {
        Self(Rc::new(RefCell::new(SinkResource {
            sink: Box::new(sink),
            closed: None,
        })))
    }
    pub(super) fn with(
        &self,
        call: impl FnOnce(&mut dyn EventSink) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut resource = self.0.borrow_mut();
        if resource.closed.is_some() {
            return Err("Event sink is closed".into());
        }
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            call(resource.sink.as_mut())
        }))
        .unwrap_or_else(|_| Err("Event sink callback panicked".into()))
    }
    pub(super) fn snapshot(&self, snapshot: &Value) -> Result<(), String> {
        self.with(|sink| sink.snapshot(snapshot))
    }
    pub(super) fn close(&self) -> Result<(), String> {
        self.0.borrow_mut().close()
    }
}

/// Only secondary controls discovered in the selected device's 1B04 catalogue.
/// The first button report synchronizes state; it never invents a press edge.
pub(super) struct Decoder {
    slot: u8,
    index: u8,
    controls: BTreeMap<u16, bool>,
    held: BTreeSet<u16>,
    synchronized: bool,
}
impl Decoder {
    pub(super) fn from_snapshot(snapshot: &Value, slot: u8) -> Result<Self, String> {
        Self::build(snapshot, slot, true)
    }
    /// Native-only profile sessions may have no divertable controls. Keep a
    /// passive decoder/filter without granting capture capability in that case.
    pub(super) fn for_passive_profile(snapshot: &Value, slot: u8) -> Result<Self, String> {
        Self::build(snapshot, slot, false)
    }
    fn build(snapshot: &Value, slot: u8, require_controls: bool) -> Result<Self, String> {
        let index = snapshot["feature"]["index"]
            .as_u64()
            .and_then(|v| u8::try_from(v).ok())
            .filter(|v| *v != 0 && !matches!(*v, 0x8f | 0xff))
            .ok_or("Invalid notification feature index")?;
        let mut controls = BTreeMap::new();
        for control in snapshot["controls"].as_array().ok_or("Missing controls")? {
            let cid = control["id"]
                .as_str()
                .and_then(|s| s.parse::<u16>().ok())
                .filter(|c| [83, 86, 195].contains(c))
                .ok_or("Unsupported notification control")?;
            let flags = control["capabilities"]["flags"]
                .as_u64()
                .ok_or("Missing control capabilities")?;
            if flags & 0x31 != 0x31 || flags & 0x80 != 0 {
                continue;
            }
            if controls.insert(cid, flags & 0x100 != 0).is_some() {
                return Err("Duplicate notification control".into());
            }
        }
        if !(1..=6).contains(&slot) || (require_controls && controls.is_empty()) {
            return Err("No supported divertable secondary controls".into());
        }
        Ok(Self {
            slot,
            index,
            controls,
            held: BTreeSet::new(),
            synchronized: false,
        })
    }

    #[cfg(test)]
    pub(super) fn decode(&mut self, report: &Report) -> Result<Option<Value>, String> {
        self.decode_event(report)
            .map(|event| event.map(|event| event.evidence()))
    }

    pub(super) fn decode_event(&mut self, report: &Report) -> Result<Option<Event>, String> {
        if report.device() != self.slot
            || report.feature() != self.index
            || report.function_and_software() & 0x0f != 0
            || report.error().is_some()
        {
            return Ok(None);
        }
        let p = report.payload();
        match report.function_and_software() >> 4 {
            0 => {
                if p.len() < 8 {
                    return Err("Truncated 1B04 button notification".into());
                }
                let mut next = BTreeSet::new();
                let mut ended = false;
                for pair in p[..8].as_chunks::<2>().0 {
                    let cid = u16::from_be_bytes([pair[0], pair[1]]);
                    if cid == 0 {
                        ended = true;
                    } else if ended || !self.controls.contains_key(&cid) || !next.insert(cid) {
                        return Err("Invalid, duplicate or unsupported notification CID".into());
                    }
                }
                let initial = !self.synchronized;
                let pressed: Vec<_> = if initial {
                    vec![]
                } else {
                    next.difference(&self.held).copied().collect()
                };
                let released: Vec<_> = self.held.difference(&next).copied().collect();
                self.held = next;
                self.synchronized = true;
                Ok(Some(Event::Buttons {
                    initial,
                    held: self.held.iter().copied().collect(),
                    pressed,
                    released,
                }))
            }
            1 => {
                if p.len() < 4 {
                    return Err("Truncated 1B04 raw XY notification".into());
                }
                // Motion has no CID; without a known raw-capable held source it
                // cannot be assigned to a gesture, even if it arrived first.
                if !self.held.iter().any(|cid| self.controls[cid]) {
                    return Ok(None);
                }
                Ok(Some(Event::RawXY {
                    dx: i16::from_be_bytes([p[0], p[1]]),
                    dy: i16::from_be_bytes([p[2], p[3]]),
                    held: self.held.iter().copied().collect(),
                    sources: self
                        .held
                        .iter()
                        .filter(|cid| self.controls[cid])
                        .copied()
                        .collect(),
                }))
            }
            _ => Ok(None),
        }
    }

    pub(super) fn reset(&mut self) -> Vec<u16> {
        self.synchronized = false;
        std::mem::take(&mut self.held).into_iter().collect()
    }
    pub(super) fn synchronization(&self) -> Option<Event> {
        self.synchronized.then(|| Event::Buttons {
            initial: true,
            held: self.held.iter().copied().collect(),
            pressed: vec![],
            released: vec![],
        })
    }
}

struct CaptureState {
    decoder: Decoder,
    start: Instant,
    deadline: Instant,
    queued_at_start: usize,
    events: Vec<Value>,
    reports: usize,
    ignored: usize,
    stop_reason: Option<&'static str>,
    error: Option<String>,
    cleared_held: Vec<u16>,
    finished: bool,
    ended: Option<Instant>,
    during_query: bool,
    sink: Option<SinkHandle>,
    live: bool,
    delivered: usize,
    sink_release_error: Option<String>,
    during_activation: bool,
    capture_armed: bool,
    activation_buttons_seen: bool,
}
struct CaptureConsumer<C> {
    state: Rc<RefCell<CaptureState>>,
    cancelled: C,
}
impl<C: FnMut() -> bool> ReportConsumer for CaptureConsumer<C> {
    fn checkpoint(&mut self) -> Result<(), String> {
        let cancelled = (self.cancelled)();
        let mut state = self.state.borrow_mut();
        let reason = if let Some(reason) = state.stop_reason {
            Some(reason)
        } else if cancelled {
            Some("cancelled")
        } else if Instant::now() >= state.deadline {
            Some("deadline")
        } else if state.reports == MAX_REPORTS {
            Some("report-limit")
        } else if state.events.len() == MAX_EVENTS {
            Some("event-limit")
        } else {
            None
        };
        if let Some(reason) = reason {
            state.stop_reason = Some(reason);
            return Err(reason.into());
        }
        Ok(())
    }
    fn consume(&mut self, report: &Report) -> Result<(), String> {
        let mut state = self.state.borrow_mut();
        state.reports += 1;
        match state.decoder.decode_event(report) {
            Ok(Some(event)) => {
                if state.during_activation && matches!(event, Event::Buttons { .. }) {
                    state.activation_buttons_seen = true;
                }
                let delivered = state.live && state.sink.is_some();
                let mut evidence = event.evidence();
                evidence["elapsed_ms"] = json!(state.start.elapsed().as_millis());
                evidence["queued_before_observation"] =
                    json!(state.reports <= state.queued_at_start);
                evidence["during_query"] = json!(state.during_query);
                evidence["during_activation"] = json!(state.during_activation);
                evidence["delivered_to_sink"] = json!(delivered);
                state.events.push(evidence);
                if delivered {
                    state.delivered += 1;
                    if let Err(error) = state.sink.as_ref().unwrap().with(|sink| sink.event(&event))
                    {
                        state.stop_reason = Some("output-error");
                        state.error = Some(error.clone());
                        return Err(error);
                    }
                }
                Ok(())
            }
            Ok(None) => {
                state.ignored += 1;
                Ok(())
            }
            Err(error) => {
                state.stop_reason = Some("invalid-event");
                state.error = Some(error.clone());
                Err(error)
            }
        }
    }
}

/// Shared engine for the finite observer and the persistent passive owner.
pub(super) struct Capture<T: Transport> {
    session: Option<Session<T>>,
    state: Rc<RefCell<CaptureState>>,
}
impl<T: Transport> Capture<T> {
    #[cfg(test)]
    pub(super) fn attach(
        session: Session<T>,
        decoder: Decoder,
        duration: Duration,
        cancelled: impl FnMut() -> bool + 'static,
    ) -> Self {
        Self::attach_with_sink(session, decoder, duration, cancelled, None)
    }
    pub(super) fn attach_with_sink(
        mut session: Session<T>,
        decoder: Decoder,
        duration: Duration,
        cancelled: impl FnMut() -> bool + 'static,
        sink: Option<SinkHandle>,
    ) -> Self {
        let start = Instant::now();
        let state = Rc::new(RefCell::new(CaptureState {
            decoder,
            start,
            deadline: start + duration,
            queued_at_start: session.pending_len(),
            events: vec![],
            reports: 0,
            ignored: 0,
            stop_reason: None,
            error: None,
            cleared_held: vec![],
            finished: false,
            ended: None,
            during_query: false,
            sink,
            live: false,
            delivered: 0,
            sink_release_error: None,
            during_activation: false,
            capture_armed: false,
            activation_buttons_seen: false,
        }));
        let result = session.install_consumer(CaptureConsumer {
            state: state.clone(),
            cancelled,
        });
        let mut capture = Self {
            session: Some(session),
            state,
        };
        if let Err(error) = result {
            capture.fail("stream-error", error.to_string());
        } else {
            // The entire initialization FIFO has been recorded and discarded
            // for output. A fresh button list will synchronize the action engine.
            capture.state.borrow_mut().live = true;
        }
        capture
    }
    pub(super) fn is_running(&self) -> bool {
        self.session.is_some()
    }
    pub(super) fn poll(&mut self, timeout: Duration) -> Result<bool, String> {
        if timeout.is_zero() {
            return Err("Invalid poll timeout".into());
        }
        let session = self
            .session
            .as_mut()
            .ok_or("Passive event owner is closed")?;
        let remaining = self
            .state
            .borrow()
            .deadline
            .saturating_duration_since(Instant::now());
        let result = session.poll_consumer(
            remaining
                .max(Duration::from_nanos(1))
                .min(timeout)
                .min(POLL_INTERVAL),
        );
        match result {
            Ok(delivered) => Ok(delivered),
            Err(error) => {
                let error = error.to_string();
                self.fail("stream-error", error.clone());
                Err(error)
            }
        }
    }
    pub(super) fn read_snapshot(
        &mut self,
        slot: u8,
        id: &str,
        timeout: Duration,
    ) -> Result<Value, String> {
        let preparation = self
            .state
            .borrow()
            .sink
            .as_ref()
            .map_or(Ok(()), |sink| sink.with(|sink| sink.before_query()));
        if let Err(error) = preparation {
            self.fail("output-error", error.clone());
            return Err(error);
        }
        let session = self
            .session
            .as_mut()
            .ok_or("Passive event owner is closed")?;
        let deadline = self.state.borrow().deadline.min(Instant::now() + timeout);
        self.state.borrow_mut().during_query = true;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            describe_session(session, slot, id, deadline)
        }))
        .unwrap_or_else(|_| Err("Passive refresh panicked".into()));
        self.state.borrow_mut().during_query = false;
        match result {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                // A refresh that did not finish is not a successful observation,
                // even if the overall observation deadline caused the failure.
                if self.state.borrow().stop_reason == Some("deadline") {
                    self.state.borrow_mut().stop_reason = Some("query-timeout");
                    self.state.borrow_mut().error = Some(error.clone());
                }
                self.fail("query-error", error.clone());
                Err(error)
            }
        }
    }
    pub(super) fn fail(&mut self, reason: &'static str, error: String) {
        self.terminate(reason, Some(error));
    }
    fn terminate(&mut self, reason: &'static str, error: Option<String>) {
        // Drop the unique transport before exposing the terminal state. No I/O,
        // reconnection or replay is attempted during stop or destruction.
        self.session.take();
        let mut state = self.state.borrow_mut();
        if state.stop_reason.is_none() {
            state.stop_reason = Some(reason);
            state.error = error;
        }
        if !state.finished {
            state.sink_release_error = state.sink.as_ref().and_then(|sink| sink.close().err());
            state.cleared_held = state.decoder.reset();
            state.finished = true;
            state.ended = Some(Instant::now());
        }
    }
    pub(super) fn stop(&mut self) {
        self.terminate("stopped", None);
    }
    pub(super) fn accept_snapshot(&mut self, snapshot: &Value) -> Result<(), String> {
        let result = self
            .state
            .borrow()
            .sink
            .as_ref()
            .map_or(Ok(()), |sink| sink.snapshot(snapshot));
        if let Err(error) = &result {
            self.fail("output-error", error.clone());
        }
        result
    }
    pub(super) fn activate_registered(
        &mut self,
        slot: u8,
        id: &str,
        baseline: &Value,
        actions: &super::ActionBindings,
        recovery: &super::ReportingRecovery,
    ) -> super::activation::Activation {
        let prepared = {
            let state = self.state.borrow();
            state.sink.is_some() && self.is_running() && state.delivered == 0
        };
        if !prepared {
            let error = "Capture requires a fresh prepared output and running consumer".to_string();
            self.fail("activation-error", error.clone());
            return super::activation::Activation::failed(error);
        }
        let deadline = {
            let mut state = self.state.borrow_mut();
            state.live = false;
            state.during_activation = true;
            state.deadline.min(Instant::now() + Duration::from_secs(20))
        };
        let mut outcome = super::activation::activate(
            self.session.as_mut().unwrap(),
            slot,
            id,
            baseline,
            actions,
            recovery,
            deadline,
            || {
                self.state.borrow_mut().activation_buttons_seen = false;
            },
        );
        self.state.borrow_mut().during_activation = false;
        let hardware_confirmed = outcome.evidence["completed"] == true;
        outcome.evidence["hardware_confirmed"] = json!(hardware_confirmed);
        if let Some(snapshot) = &outcome.snapshot {
            let result = self.accept_snapshot(snapshot).and_then(|()| {
                let state = self.state.borrow();
                if Instant::now() >= state.deadline {
                    return Err("Capture activation budget elapsed before arming output".into());
                }
                // Only a state received after the LAST setter can seed the full
                // selected set. Setup/preflight history cannot prove held state.
                if let Some(event) = state
                    .decoder
                    .synchronization()
                    .filter(|_| state.activation_buttons_seen)
                {
                    state
                        .sink
                        .as_ref()
                        .unwrap()
                        .with(|sink| sink.event(&event))?;
                }
                if Instant::now() >= state.deadline {
                    return Err(
                        "Capture activation budget elapsed while synchronizing output".into(),
                    );
                }
                Ok(())
            });
            if let Err(error) = result {
                outcome.evidence["completed"] = json!(false);
                outcome.evidence["error"] = json!(error);
                outcome.snapshot = None;
            }
        }
        if outcome.evidence["completed"] == true {
            let mut state = self.state.borrow_mut();
            state.live = true;
            state.capture_armed = true;
        } else {
            let mut state = self.state.borrow_mut();
            if state.stop_reason == Some("deadline") {
                state.stop_reason = Some("activation-timeout");
                state.error = outcome.evidence["error"].as_str().map(str::to_owned);
            }
            drop(state);
            self.fail(
                "activation-error",
                outcome.evidence["error"]
                    .as_str()
                    .unwrap_or("Capture activation failed")
                    .into(),
            );
        }
        outcome
    }
    pub(super) fn status(&self) -> Value {
        let state = self.state.borrow();
        json!({"completed":state.stop_reason==Some("deadline") && state.sink_release_error.is_none(),"stop_reason":state.stop_reason,
            "error":state.error,"reports":state.reports,"ignored":state.ignored,"events":state.events,
            "queued_at_start":state.queued_at_start,"cleared_held":state.cleared_held,
            "sink_attached":state.sink.is_some(),"sink_deliveries":state.delivered,"sink_release_error":state.sink_release_error,
            "capture_armed":state.capture_armed,
            "running":self.is_running(),"elapsed_ms":state.ended.unwrap_or_else(Instant::now).duration_since(state.start).as_millis()})
    }
}
impl<T: Transport> Drop for Capture<T> {
    fn drop(&mut self) {
        self.terminate("dropped", None);
    }
}

#[cfg(test)]
pub(super) fn collect<T: Transport>(
    session: Session<T>,
    decoder: Decoder,
    duration: Duration,
    cancelled: impl FnMut() -> bool + 'static,
) -> Value {
    let mut capture = Capture::attach(session, decoder, duration, cancelled);
    while capture.is_running() {
        let _ = capture.poll(POLL_INTERVAL);
    }
    capture.status()
}

/// Finite compatibility wrapper around the same persistent owner used for refresh.
pub fn observe_notifications<T: Transport>(
    transport: T,
    slot: u8,
    id: &str,
    duration: Duration,
    cancelled: impl FnMut() -> bool + 'static,
) -> Result<Value, String> {
    let mut owner =
        super::owner::PassiveEventOwner::open(transport, slot, id, duration, cancelled)?;
    while owner.is_running() {
        let _ = owner.poll(POLL_INTERVAL);
    }
    let mut data = owner.status();
    data["mode"] = json!("passive-notifications");
    Ok(data)
}
