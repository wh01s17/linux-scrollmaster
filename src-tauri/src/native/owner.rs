// SPDX-License-Identifier: GPL-3.0-or-later
//! One persistent owner. Public observation is passive; internal capture requires
//! an opaque recovery registration. No generic dispatcher or reconnection.
use super::{
    buttons::describe_session,
    events::{Capture, Decoder, SinkHandle},
};
use crate::hidpp::{Session, Transport};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

/// Experimental finite owner: opens once, validates identity/capabilities, installs
/// one consumer and keeps it through refresh and idle reads. Callers still own
/// discovery, process-conflict checks and HardwareLease for the whole lifetime.
pub struct PassiveEventOwner<T: Transport> {
    capture: Capture<T>,
    slot: u8,
    id: String,
    snapshot: Value,
    binding: Value,
    refreshes: usize,
    refresh_attempts: usize,
    activation: Option<Value>,
}
pub(super) fn binding(snapshot: &Value) -> Value {
    json!({"feature":snapshot["feature"],"count":snapshot["catalogue_count"],"controls":snapshot["controls"].as_array().map(|controls|
        controls.iter().map(|c|json!({"id":c["id"],"capabilities":c["capabilities"],
            "target":c["reported_target"],"flags":c["flags"],"choices":c["choices"]})).collect::<Vec<_>>())})
}
impl<T: Transport> PassiveEventOwner<T> {
    pub fn open(
        transport: T,
        slot: u8,
        id: &str,
        duration: Duration,
        cancelled: impl FnMut() -> bool + 'static,
    ) -> Result<Self, String> {
        Self::open_with_sink(transport, slot, id, duration, cancelled, None)
    }
    pub(super) fn open_with_sink(
        transport: T,
        slot: u8,
        id: &str,
        duration: Duration,
        cancelled: impl FnMut() -> bool + 'static,
        sink: Option<SinkHandle>,
    ) -> Result<Self, String> {
        let result = (|| {
            if duration.is_zero() || duration > Duration::from_secs(30) {
                return Err(
                    "Observation duration must be greater than zero and at most 30 seconds".into(),
                );
            }
            if !(1..=6).contains(&slot) {
                return Err("Invalid observation slot".into());
            }
            let cancelled = Rc::new(RefCell::new(cancelled));
            let setup_cancelled = cancelled.clone();
            let mut session = Session::with_initialization_cancellation(transport, move || {
                (setup_cancelled.borrow_mut())()
            });
            let snapshot = describe_session(
                &mut session,
                slot,
                id,
                Instant::now() + Duration::from_secs(20),
            )?;
            let decoder = Decoder::from_snapshot(&snapshot, slot)?;
            if let Some(sink) = &sink {
                sink.snapshot(&snapshot)?;
            }
            let capture = Capture::attach_with_sink(
                session,
                decoder,
                duration,
                move || (cancelled.borrow_mut())(),
                sink.clone(),
            );
            Ok(Self {
                capture,
                slot,
                id: id.into(),
                binding: binding(&snapshot),
                snapshot,
                refreshes: 0,
                refresh_attempts: 0,
                activation: None,
            })
        })();
        if result.is_err() {
            if let Some(sink) = &sink {
                let _ = sink.close();
            }
        }
        result
    }
    pub fn is_running(&self) -> bool {
        self.capture.is_running()
    }
    pub(super) fn activate_registered(
        &mut self,
        actions: &super::ActionBindings,
        recovery: &super::ReportingRecovery,
    ) -> Result<(), String> {
        if self.activation.is_some() {
            return Err("Capture activation is one-shot".into());
        }
        let outcome = self.capture.activate_registered(
            self.slot,
            &self.id,
            &self.snapshot,
            actions,
            recovery,
        );
        self.activation = Some(outcome.evidence.clone());
        if let Some(snapshot) = outcome.snapshot {
            self.binding = binding(&snapshot);
            self.snapshot = snapshot;
            Ok(())
        } else {
            Err(outcome.evidence["error"]
                .as_str()
                .unwrap_or("Capture activation failed")
                .into())
        }
    }
    pub fn poll(&mut self, timeout: Duration) -> Result<bool, String> {
        self.capture.poll(timeout)
    }

    /// Same owned session/consumer for all getters. Failed or changed bindings
    /// close the owner and never replace the last validated snapshot.
    pub fn refresh(&mut self, timeout: Duration) -> Result<Value, String> {
        if timeout.is_zero() || timeout > Duration::from_secs(20) {
            return Err("Refresh timeout must be greater than zero and at most 20 seconds".into());
        }
        if !self.is_running() {
            return Err("Passive event owner is closed".into());
        }
        self.refresh_attempts += 1;
        let snapshot = self.capture.read_snapshot(self.slot, &self.id, timeout)?;
        if binding(&snapshot) != self.binding {
            let error="Device button feature, capabilities, mapping or reporting changed; passive owner closed".to_string();
            self.capture.fail("configuration-changed", error.clone());
            return Err(error);
        }
        self.capture.accept_snapshot(&snapshot)?;
        self.snapshot = snapshot;
        self.refreshes += 1;
        Ok(self.snapshot.clone())
    }
    /// This is cached evidence from the last fully validated query, not a getter.
    pub fn last_snapshot(&self) -> &Value {
        &self.snapshot
    }
    /// Immediate, idempotent local close. Does not write restoration to firmware.
    pub fn stop(&mut self) -> Value {
        self.capture.stop();
        self.status()
    }
    pub fn status(&self) -> Value {
        let attempted = self
            .activation
            .as_ref()
            .is_some_and(|a| a["setters_attempted"].as_u64().unwrap_or(0) > 0);
        let diverted = if !attempted {
            Some(false)
        } else if self
            .activation
            .as_ref()
            .is_some_and(|a| a["confirmed"].as_array().is_some_and(|c| !c.is_empty()))
        {
            Some(true)
        } else {
            None
        };
        json!({"stage":"N4","read_only":self.activation.is_none(),"experimental":true,"mode":if self.activation.is_some() {"capture-owner"} else {"passive-owner"},
            "device":self.id,"diversion_enabled_by_observer":diverted,"diversion_may_have_been_enabled":attempted,"snapshot":self.snapshot,
            "activation":self.activation,
            "refreshes":self.refreshes,"refresh_attempts":self.refresh_attempts,
            "observation":self.capture.status()})
    }
}
