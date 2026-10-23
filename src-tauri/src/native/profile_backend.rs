// SPDX-License-Identifier: GPL-3.0-or-later
//! N5 adapter for native mappings/scalars on one already-owned HID++ session.
//! Software assignments require the guardian lifecycle and are rejected here.
use super::{
    buttons,
    profile_events::{Cancellation, ProfileEvents},
    settings, ApplyError,
};
use crate::{
    hidpp::{Session, Transport},
    service::{Applied, Changes, ProfileBackend},
};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub struct NativeProfileBackend<T: Transport> {
    pub(super) session: Session<T>,
    slot: u8,
    identity: String,
    pub(super) events: Option<ProfileEvents>,
    cancelled: Cancellation,
    phase: &'static str,
}
impl<T: Transport> NativeProfileBackend<T> {
    /// Moves the unique session into this backend. The outer owner must retain
    /// its hardware lease and exclude other controllers for this whole lifetime.
    /// No HID interface is opened and no reporting flag is enabled by construction.
    pub fn new(session: Session<T>, slot: u8, identity: &str) -> Result<Self, String> {
        Self::with_cancellation(session, slot, identity, Cancellation::default())
    }
    pub(super) fn with_cancellation(
        session: Session<T>,
        slot: u8,
        identity: &str,
        cancelled: Cancellation,
    ) -> Result<Self, String> {
        validate_selection(slot, identity)?;
        Ok(Self {
            session,
            slot,
            identity: identity.to_owned(),
            events: None,
            cancelled,
            phase: "snapshot",
        })
    }
    pub(super) fn current(&mut self, deadline: Instant) -> Result<Value, String> {
        let result: Result<Value, String> = (|| {
            check_time(deadline)?;
            if let Some(events) = &self.events {
                events.before_query()?;
            }
            let snapshot =
                buttons::describe_session(&mut self.session, self.slot, &self.identity, deadline)?;
            check_time(deadline)?;
            if let Some(events) = &self.events {
                events.validate_snapshot(&snapshot)?;
                events.accept_snapshot(&snapshot)?;
            } else {
                let events = ProfileEvents::new(
                    &snapshot,
                    self.slot,
                    self.session.pending_len(),
                    self.cancelled.clone(),
                )?;
                self.events = Some(events.clone());
                self.session
                    .install_consumer(events.clone())
                    .map_err(|e| e.to_string())?;
                events.begin(self.phase);
            }
            Ok(snapshot)
        })();
        if let Err(error) = &result {
            self.invalidate(error);
        }
        result
    }
    pub(super) fn begin(&mut self, phase: &'static str) {
        self.phase = phase;
        if let Some(events) = &self.events {
            events.begin(phase);
        }
    }
    fn invalidate(&mut self, error: &str) {
        self.session.invalidate();
        if let Some(events) = &self.events {
            events.fail(error.into());
        }
    }
    /// Local bounded history from the last accepted session, never a HID getter.
    pub fn event_status(&self) -> Value {
        self.events.as_ref().map_or_else(||json!({"mode":"passive-profile-events","initialized":false,"active":false,"actions_enabled":false,"diversion_enabled_by_backend":false}),
            |events| {let mut value=events.describe(self.session.has_consumer());value["initialized"]=json!(true);value})
    }
    fn description(&self, snapshot: &Value) -> Value {
        let mut device = device_description(snapshot);
        device["native_events"] = self.event_status();
        device
    }
    fn prepare_current(&mut self, changes: &Changes, deadline: Instant) -> Result<(), String> {
        // Fail before getters/setters if the guardian/output lifecycle is needed.
        validate_native_changes(changes)?;
        let snapshot = self.current(deadline)?;
        if let Some(settings) = changes.settings() {
            settings::prepare_snapshot(&snapshot["snapshot"], settings)?;
        }
        if let Some(changes) = changes.buttons() {
            buttons::prepare_snapshot(&snapshot, changes)?;
        }
        check_time(deadline)
    }
}
pub(super) fn validate_selection(slot: u8, identity: &str) -> Result<(), String> {
    if !(1..=6).contains(&slot)
        || identity.len() != 8
        || !identity
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_lowercase())
    {
        return Err("Expected Bolt slot 1–6 and uppercase stable Unit ID".into());
    }
    Ok(())
}
pub(crate) fn validate_native_changes(changes: &Changes) -> Result<(), String> {
    if let Some(buttons) = changes.buttons() {
        buttons::validate_button_changes(buttons).map_err(|_| "Software assignments are not yet supported by the profile session backend; use the opt-in guardian separately")?;
    }
    Ok(())
}
fn check_time(deadline: Instant) -> Result<(), String> {
    if Instant::now() >= deadline {
        Err("Profile hardware budget elapsed".into())
    } else {
        Ok(())
    }
}
pub(super) fn device_description(snapshot: &Value) -> Value {
    let mut device = snapshot["snapshot"]["device"].clone();
    device["buttons"] = json!({"controls":snapshot["controls"],"error":null});
    device
}
impl<T: Transport> ProfileBackend for NativeProfileBackend<T> {
    fn identity(&self) -> &str {
        &self.identity
    }
    fn snapshot(&mut self, deadline: Instant) -> Result<Value, String> {
        self.begin("snapshot");
        self.current(deadline).map(|v| self.description(&v))
    }
    fn prepare(&mut self, changes: &Changes, deadline: Instant) -> Result<(), String> {
        self.begin("prepare");
        self.prepare_current(changes, deadline)
    }
    fn apply(&mut self, changes: &Changes, deadline: Instant) -> Result<Applied, ApplyError> {
        self.begin("apply");
        let result: Result<Applied, ApplyError> = (|| {
            self.prepare_current(changes, deadline)
                .map_err(|message| ApplyError {
                    message,
                    confirmed: vec![],
                    attempted: None,
                })?;
            let mut confirmed = Vec::new();
            if let Some(settings) = changes.settings() {
                let result = settings::apply_changes_session(
                    &mut self.session,
                    self.slot,
                    &self.identity,
                    settings,
                    deadline,
                )?;
                confirmed.extend(
                    result["confirmed"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|k| k.as_str().unwrap().to_owned()),
                );
            }
            if let Some(buttons) = changes.buttons() {
                let result = buttons::apply_button_changes_session(
                    &mut self.session,
                    self.slot,
                    &self.identity,
                    buttons,
                    deadline,
                )
                .map_err(|mut error| {
                    let mut previous = confirmed.clone();
                    previous.append(&mut error.confirmed);
                    error.confirmed = previous;
                    error
                })?;
                confirmed.extend(
                    result["confirmed"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|k| k.as_str().unwrap().to_owned()),
                );
            }
            // Recheck the entire requested set after both batches: native remaps can
            // interleave with diagnostics and must not hide an earlier scalar drift.
            let snapshot = self.current(deadline).map_err(|message| ApplyError {
                message,
                confirmed: confirmed.clone(),
                attempted: None,
            })?;
            let snapshot = self.description(&snapshot);
            changes
                .confirm(&snapshot, &self.identity)
                .map_err(|message| ApplyError {
                    message,
                    confirmed: confirmed.clone(),
                    attempted: None,
                })?;
            Ok(Applied {
                confirmed,
                snapshot,
            })
        })();
        if let Err(error) = &result {
            self.invalidate(&error.message);
        }
        result
    }
    fn poll(&mut self, timeout: Duration) -> Result<bool, String> {
        if timeout.is_zero() || timeout > Duration::from_millis(50) {
            return Err(
                "Resident poll requires a duration greater than zero and at most 50 ms".into(),
            );
        }
        if self.events.is_none() {
            return Ok(false);
        }
        self.begin("idle");
        self.session.poll_consumer(timeout).map_err(|e| {
            let error = e.to_string();
            self.invalidate(&error);
            error
        })
    }
}
