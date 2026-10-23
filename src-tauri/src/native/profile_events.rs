// SPDX-License-Identifier: GPL-3.0-or-later
//! Bounded resident delivery. Output requires a registered guardian activation.
use super::events::{Decoder, Event, SinkHandle};
use crate::hidpp::{Report, ReportConsumer};
use serde_json::{json, Value};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

const HISTORY: usize = 64;
const REPORTS_PER_TURN: usize = 4096;

#[derive(Clone)]
pub(super) struct Cancellation(Rc<RefCell<Box<dyn FnMut() -> bool>>>);
impl Default for Cancellation {
    fn default() -> Self {
        Self::new(|| false)
    }
}
impl Cancellation {
    pub fn new(cancelled: impl FnMut() -> bool + 'static) -> Self {
        Self(Rc::new(RefCell::new(Box::new(cancelled))))
    }
    pub fn check(&self) -> bool {
        (self.0.borrow_mut())()
    }
}
struct State {
    decoder: Decoder,
    layout: Value,
    reports: u64,
    events: u64,
    ignored: u64,
    evicted: u64,
    recent: VecDeque<Value>,
    turn_reports: usize,
    initial: usize,
    phase: &'static str,
    error: Option<String>,
    sink: Option<SinkHandle>,
    live: bool,
    activation_buttons_seen: bool,
    binding: Option<Value>,
}
#[derive(Clone)]
pub(super) struct ProfileEvents {
    state: Rc<RefCell<State>>,
    cancelled: Cancellation,
}
fn layout(snapshot: &Value) -> Value {
    json!({"feature":snapshot["feature"],"count":snapshot["catalogue_count"],
        "controls":snapshot["controls"].as_array().map(|a|a.iter().map(|c|
            json!({"id":c["id"],"capabilities":c["capabilities"],"choices":c["choices"]})
        ).collect::<Vec<_>>())})
}
impl ProfileEvents {
    pub fn new(
        snapshot: &Value,
        slot: u8,
        queued: usize,
        cancelled: Cancellation,
    ) -> Result<Self, String> {
        Ok(Self {
            state: Rc::new(RefCell::new(State {
                decoder: Decoder::for_passive_profile(snapshot, slot)?,
                layout: layout(snapshot),
                reports: 0,
                events: 0,
                ignored: 0,
                evicted: 0,
                recent: VecDeque::new(),
                turn_reports: 0,
                initial: queued,
                phase: "initialization",
                error: None,
                sink: None,
                live: false,
                activation_buttons_seen: false,
                binding: None,
            })),
            cancelled,
        })
    }
    pub fn begin(&self, phase: &'static str) {
        let mut state = self.state.borrow_mut();
        state.phase = phase;
        state.turn_reports = 0;
    }
    pub fn validate_snapshot(&self, snapshot: &Value) -> Result<(), String> {
        if layout(snapshot) != self.state.borrow().layout {
            let error = "Resident notification feature/control layout changed".to_owned();
            self.fail(error.clone());
            return Err(error);
        }
        if self
            .state
            .borrow()
            .binding
            .as_ref()
            .is_some_and(|expected| *expected != super::owner::binding(snapshot))
        {
            let error = "Resident capture mapping/reporting changed".to_owned();
            self.fail(error.clone());
            return Err(error);
        }
        Ok(())
    }
    pub fn prepare_output(&self, sink: SinkHandle) -> Result<(), String> {
        let mut state = self.state.borrow_mut();
        if state.sink.is_some() || state.error.is_some() {
            return Err("Resident output is already prepared or terminal".into());
        }
        state.decoder.reset();
        state.sink = Some(sink);
        Ok(())
    }
    // Called immediately before EACH setter. Only a state after the final
    // setter can synchronize output; neither historical FIFO nor preflight can.
    pub fn before_setter(&self) {
        let mut state = self.state.borrow_mut();
        state.activation_buttons_seen = false;
        state.decoder.reset();
    }
    pub fn discard_output(&self) -> Result<(), String> {
        let mut state = self.state.borrow_mut();
        if state.binding.is_some() {
            return Err("Cannot discard active resident capture".into());
        }
        state.live = false;
        state.sink = None;
        state.decoder.reset();
        Ok(())
    }
    // Transition only: stop delivery and forget the armed binding BEFORE the
    // owner releases output and clears reporting through the same reader.
    pub fn disarm(&self) -> Result<(), String> {
        let mut state = self.state.borrow_mut();
        if let Some(error) = &state.error {
            return Err(error.clone());
        }
        state.live = false;
        state.binding = None;
        state.sink = None;
        state.activation_buttons_seen = false;
        state.decoder.reset();
        Ok(())
    }
    pub fn arm(&self, snapshot: &Value, deadline: std::time::Instant) -> Result<(), String> {
        let mut state = self.state.borrow_mut();
        if state.error.is_some() || state.sink.is_none() || state.live {
            return Err("Resident capture cannot arm this output".into());
        }
        let sink = state.sink.as_ref().unwrap();
        sink.snapshot(snapshot)?;
        if state.activation_buttons_seen {
            if let Some(event) = state.decoder.synchronization() {
                sink.with(|sink| sink.event(&event))?;
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err("Resident capture deadline elapsed before output was armed".into());
        }
        state.binding = Some(super::owner::binding(snapshot));
        state.live = true;
        Ok(())
    }
    pub fn before_query(&self) -> Result<(), String> {
        let state = self.state.borrow();
        state
            .sink
            .as_ref()
            .map_or(Ok(()), |sink| sink.with(|s| s.before_query()))
    }
    pub fn accept_snapshot(&self, snapshot: &Value) -> Result<(), String> {
        let state = self.state.borrow();
        state
            .sink
            .as_ref()
            .map_or(Ok(()), |sink| sink.snapshot(snapshot))
    }
    pub fn fail(&self, error: String) {
        let mut state = self.state.borrow_mut();
        if state.error.is_none() {
            state.error = Some(error);
        }
        state.live = false;
    }
    pub fn describe(&self, active: bool) -> Value {
        let state = self.state.borrow();
        json!({"mode":if state.binding.is_some(){"resident-profile-capture"}else{"passive-profile-events"},"active":active && state.error.is_none(),
            "reports":state.reports,"events":state.events,"ignored":state.ignored,
            "history_limit":HISTORY,"history_evicted":state.evicted,"recent":state.recent,
            "error":state.error,"actions_enabled":state.live && active,"diversion_enabled_by_backend":state.binding.is_some()})
    }
}
impl ReportConsumer for ProfileEvents {
    fn checkpoint(&mut self) -> Result<(), String> {
        let error = if self.cancelled.check() {
            Some("Profile guardian client disconnected".to_owned())
        } else {
            let state = self.state.borrow();
            state.error.clone().or_else(|| {
                (state.turn_reports >= REPORTS_PER_TURN)
                    .then(|| "Resident unmatched report budget exhausted".to_owned())
            })
        };
        if let Some(error) = error {
            self.fail(error.clone());
            return Err(error);
        }
        Ok(())
    }
    fn consume(&mut self, report: &Report) -> Result<(), String> {
        let mut state = self.state.borrow_mut();
        state.reports = state.reports.saturating_add(1);
        state.turn_reports += 1;
        let queued = state.initial > 0;
        state.initial = state.initial.saturating_sub(1);
        match state.decoder.decode_event(report) {
            Ok(Some(event)) => {
                if state.sink.is_some() && !queued && matches!(event, Event::Buttons { .. }) {
                    state.activation_buttons_seen = true;
                }
                let delivered = state.live && !queued;
                state.events = state.events.saturating_add(1);
                let mut value = event.evidence();
                value["sequence"] = json!(state.events);
                value["phase"] = json!(if queued {
                    "initialization"
                } else {
                    state.phase
                });
                value["queued_before_attachment"] = json!(queued);
                value["delivered_to_sink"] = json!(delivered);
                if state.recent.len() == HISTORY {
                    state.recent.pop_front();
                    state.evicted = state.evicted.saturating_add(1);
                }
                state.recent.push_back(value);
                if delivered {
                    if let Err(error) = state.sink.as_ref().unwrap().with(|s| s.event(&event)) {
                        state.error = Some(error.clone());
                        state.live = false;
                        return Err(error);
                    }
                }
                Ok(())
            }
            Ok(None) => {
                state.ignored = state.ignored.saturating_add(1);
                Ok(())
            }
            Err(error) => {
                state.error = Some(error.clone());
                Err(error)
            }
        }
    }
}

#[cfg(test)]
#[path = "profile_events_tests.rs"]
mod tests;
