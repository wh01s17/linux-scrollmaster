// SPDX-License-Identifier: GPL-3.0-or-later
//! Guarded resident capture. Construct only inside the independent HID owner.
use super::{
    activation, buttons,
    events::SinkHandle,
    profile_backend::{device_description, validate_selection},
    profile_connector::check_time,
    profile_events::Cancellation,
    settings, ActionBindings, ApplyError, NativeActions, NativeProfileBackend, ProfileButtonPlan,
    ReportingRecovery, WaylandKeyboard,
};
use crate::{
    hidpp::{Session, Transport},
    service::{Applied, Changes, ProfileBackend},
};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    io,
    rc::Rc,
    time::{Duration, Instant},
};

/// Recovery registration handed from a closed backend to its connector.
pub(super) type DeferredRecovery = Rc<RefCell<Option<ReportingRecovery>>>;

// The Session is the ONLY reader. Retain the original descriptor after a failed
// session drops its reader token so recovery can take ownership without reopening.
// This token is private and cannot be cloned or obtained a second time.
struct Reader<T>(Rc<RefCell<Option<T>>>);
impl<T: Transport> Transport for Reader<T> {
    fn write_report(&mut self, bytes: &[u8], deadline: Instant) -> io::Result<()> {
        self.0
            .borrow_mut()
            .as_mut()
            .ok_or_else(|| io::Error::other("Transport retired"))?
            .write_report(bytes, deadline)
    }
    fn read_report(&mut self, deadline: Instant) -> io::Result<Vec<u8>> {
        self.0
            .borrow_mut()
            .as_mut()
            .ok_or_else(|| io::Error::other("Transport retired"))?
            .read_report(deadline)
    }
}
type OutputFactory = dyn FnMut(ActionBindings, Option<u16>) -> Result<SinkHandle, String>;
struct Prepared {
    changes: Changes,
    deadline: Instant,
    plan: Option<ProfileButtonPlan>,
    recovery: Option<ReportingRecovery>,
    // Replacement plan: output opened at preflight, attached only after release.
    transition: bool,
    output: Option<SinkHandle>,
}
pub(super) struct GuardedProfileBackend<T: Transport> {
    inner: Option<NativeProfileBackend<Reader<T>>>,
    transport: Rc<RefCell<Option<T>>>,
    slot: u8,
    identity: String,
    prepared: Option<Prepared>,
    active: Option<ProfileButtonPlan>,
    recovery: Option<ReportingRecovery>,
    sink: Option<SinkHandle>,
    activation: Option<Value>,
    release: Option<Value>,
    closed: Option<Result<(), String>>,
    factory: Box<OutputFactory>,
    /// Shared with the connector: a recovery that sent no setter is handed
    /// over for the next connection instead of making the owner terminal.
    deferred: Option<DeferredRecovery>,
}
impl<T: Transport> GuardedProfileBackend<T> {
    pub fn new(
        transport: T,
        slot: u8,
        identity: &str,
        cancelled: Cancellation,
    ) -> Result<Self, String> {
        Self::with_output(transport, slot, identity, cancelled, |bindings, dpi| {
            NativeActions::new(WaylandKeyboard::open()?, bindings, dpi).map(SinkHandle::new)
        })
    }
    pub(super) fn with_output(
        transport: T,
        slot: u8,
        identity: &str,
        cancelled: Cancellation,
        factory: impl FnMut(ActionBindings, Option<u16>) -> Result<SinkHandle, String> + 'static,
    ) -> Result<Self, String> {
        validate_selection(slot, identity)?;
        let transport = Rc::new(RefCell::new(Some(transport)));
        let initialization = cancelled.clone();
        let session =
            Session::with_initialization_cancellation(Reader(transport.clone()), move || {
                initialization.check()
            });
        let inner = NativeProfileBackend::with_cancellation(session, slot, identity, cancelled)?;
        Ok(Self {
            inner: Some(inner),
            transport,
            slot,
            identity: identity.into(),
            prepared: None,
            active: None,
            recovery: None,
            sink: None,
            activation: None,
            release: None,
            closed: None,
            factory: Box::new(factory),
            deferred: None,
        })
    }
    pub(super) fn with_deferred(mut self, deferred: DeferredRecovery) -> Self {
        self.deferred = Some(deferred);
        self
    }
    fn inner(&mut self) -> Result<&mut NativeProfileBackend<Reader<T>>, String> {
        self.inner
            .as_mut()
            .ok_or_else(|| "Guarded profile session is closed".into())
    }
    fn describe(&self, snapshot: &Value) -> Value {
        let mut device = device_description(snapshot);
        device["native_events"] = self.inner.as_ref().unwrap().event_status();
        if let Some(plan) = &self.active {
            for control in device["buttons"]["controls"].as_array_mut().unwrap() {
                if let Some(value) = plan.capture_bindings().get(control["id"].as_str().unwrap()) {
                    control["value"] = value.clone();
                    control["available"] = json!(true);
                    control["software_owner"] = json!("profile-guardian");
                }
            }
        }
        device["native_capture"] = json!({"active":self.active.is_some(),"activation":self.activation,
            "recovery_registered":self.recovery.is_some(),"button_transitions_supported":true,
            "last_release":self.release});
        device
    }
    fn validate(
        &mut self,
        changes: &Changes,
        deadline: Instant,
    ) -> Result<(Value, Option<ProfileButtonPlan>), String> {
        check_time(deadline)?;
        let plan = changes
            .buttons()
            .map(|buttons| ProfileButtonPlan::parse(&self.merge(buttons)))
            .transpose()?;
        let snapshot = self.inner()?.current(deadline)?;
        if let Some(settings) = changes.settings() {
            settings::prepare_snapshot(&snapshot["snapshot"], settings)?;
        }
        if let Some(plan) = &plan {
            if !plan.native_mappings().as_object().unwrap().is_empty() {
                // Controls captured by THIS owner are released before any remap;
                // judge their advertised targets as if already released.
                let mut view = snapshot.clone();
                if let Some(active) = &self.active {
                    for control in view["controls"].as_array_mut().into_iter().flatten() {
                        let id = control["id"].as_str().unwrap_or_default();
                        if active.capture_bindings().get(id).is_some() {
                            control["available"] = json!(true);
                        }
                    }
                }
                buttons::prepare_snapshot(&view, plan.native_mappings())?;
            }
            if !plan.capture_controls().is_empty() {
                plan.actions().validate_snapshot(&snapshot)?;
                if self.active.is_none()
                    && snapshot["controls"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|c| c["flags"] != 0)
                {
                    return Err("Resident capture baseline is already intercepted".into());
                }
            }
        }
        check_time(deadline)?;
        Ok((snapshot, plan))
    }
    // Per-button semantics, as in the legacy service: controls absent from the
    // request keep their live software binding. Native targets are persistent.
    fn merge(&self, buttons: &Value) -> Value {
        let mut merged = self
            .active
            .as_ref()
            .map_or_else(|| json!({}), |plan| plan.capture_bindings().clone());
        for (id, value) in buttons.as_object().into_iter().flatten() {
            merged[id] = value.clone();
        }
        merged
    }
    // A different button object replaces the live capture under this owner:
    // release it completely, apply native values, then register/activate anew.
    fn transition(&self, plan: Option<&ProfileButtonPlan>) -> bool {
        matches!((&self.active, plan), (Some(active), Some(next)) if active.buttons() != next.buttons())
    }
    fn discard_preparation(&mut self) -> Result<(), String> {
        if let Some(output) = self.prepared.take().and_then(|p| p.output) {
            output.close()?;
        }
        if self.active.is_none() {
            if let Some(sink) = self.sink.take() {
                sink.close()?;
            }
            if let Some(events) = self.inner()?.events.as_ref() {
                events.discard_output()?;
            }
        }
        Ok(())
    }
    fn preflight(&mut self, changes: &Changes, deadline: Instant) -> Result<(), String> {
        self.inner()?.begin("prepare");
        self.discard_preparation()?;
        let (snapshot, plan) = self.validate(changes, deadline)?;
        let mut recovery = None;
        let transition = self.transition(plan.as_ref());
        let mut output = None;
        if transition {
            // Preflight keyboard/desktop before any write; nothing is attached
            // or released until apply consumes this same ticket.
            if let Some(plan) = plan.as_ref().filter(|p| !p.capture_controls().is_empty()) {
                let sink = (self.factory)(
                    plan.actions().clone(),
                    plan.actions().validate_snapshot(&snapshot)?,
                )?;
                output = Some(sink.clone());
                sink.snapshot(&snapshot)?;
            }
        } else if self.active.is_none() {
            if let Some(plan) = plan.as_ref().filter(|p| !p.capture_controls().is_empty()) {
                let remaining = deadline
                    .checked_duration_since(Instant::now())
                    .ok_or("Capture preparation deadline elapsed")?;
                let slot = self.slot;
                let identity = self.identity.clone();
                recovery = Some(ReportingRecovery::prepare(
                    &mut self.inner()?.session,
                    slot,
                    &identity,
                    &plan.capture_controls(),
                    remaining.min(Duration::from_secs(20)),
                )?);
                let sink = (self.factory)(
                    plan.actions().clone(),
                    plan.actions().validate_snapshot(&snapshot)?,
                )?;
                self.sink = Some(sink.clone());
                sink.snapshot(&snapshot)?;
                self.inner()?
                    .events
                    .as_ref()
                    .unwrap()
                    .prepare_output(sink)?;
            }
        }
        check_time(deadline)?;
        self.prepared = Some(Prepared {
            changes: changes.clone(),
            deadline,
            plan,
            recovery,
            transition,
            output,
        });
        Ok(())
    }
    fn apply_prepared(
        &mut self,
        changes: &Changes,
        deadline: Instant,
    ) -> Result<Applied, ApplyError> {
        self.inner()
            .map_err(|e| error(e, vec![], None))?
            .begin("apply");
        let p = self
            .prepared
            .take()
            .filter(|p| {
                p.deadline == deadline
                    && p.changes.settings() == changes.settings()
                    && p.changes.buttons() == changes.buttons()
            })
            .ok_or_else(|| {
                error(
                    "Apply requires the current guarded preparation",
                    vec![],
                    None,
                )
            })?;
        let (current, _) = self
            .validate(changes, deadline)
            .map_err(|e| error(e, vec![], None))?;
        if let Some(recovery) = &p.recovery {
            recovery
                .permits_activation(
                    &current,
                    self.slot,
                    &self.identity,
                    &p.plan.as_ref().unwrap().capture_controls(),
                )
                .map_err(|e| error(e, vec![], None))?;
        }
        let mut p = p;
        if p.transition {
            self.release_active(deadline)
                .map_err(|e| error(e, vec![], None))?;
        }
        let native_buttons = p
            .plan
            .as_ref()
            .map(ProfileButtonPlan::native_mappings)
            .filter(|b| !b.as_object().unwrap().is_empty())
            .cloned();
        let mut confirmed = vec![];
        if changes.settings().is_some() || native_buttons.is_some() {
            let native = Changes::new(changes.settings().cloned(), native_buttons)
                .map_err(|e| error(e, vec![], None))?;
            confirmed = self
                .inner()
                .map_err(|e| error(e, vec![], None))?
                .apply(&native, deadline)?
                .confirmed;
        }
        if let Some(output) = p.output.take() {
            // The released baseline is now zero: register fresh recovery from
            // uncached getters and attach the preflighted output, still gated.
            p.recovery = Some(
                self.attach_replacement(p.plan.as_ref().unwrap(), output, deadline)
                    .map_err(|e| error(e, confirmed.clone(), None))?,
            );
        }
        if let Some(recovery) = p.recovery {
            // Store BEFORE any activation setter so error, panic and EOF cleanup
            // cannot lose an uncertain write. Native remaps remain persistent.
            self.recovery = Some(recovery);
            let baseline = self
                .inner()
                .map_err(|e| error(e, confirmed.clone(), None))?
                .current(deadline)
                .map_err(|e| error(e, confirmed.clone(), None))?;
            let plan = p.plan.unwrap();
            let inner = self.inner.as_mut().unwrap();
            let events = inner.events.as_ref().unwrap().clone();
            let outcome = activation::activate(
                &mut inner.session,
                self.slot,
                &self.identity,
                &baseline,
                plan.actions(),
                self.recovery.as_ref().unwrap(),
                deadline,
                || events.before_setter(),
            );
            self.activation = Some(outcome.evidence.clone());
            let snapshot = outcome.snapshot.ok_or_else(|| {
                let mut historical = confirmed.clone();
                historical.extend(
                    outcome.evidence["confirmed"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|c| format!("button:{}", c.as_u64().unwrap())),
                );
                error(
                    outcome.evidence["error"]
                        .as_str()
                        .unwrap_or("Resident activation failed"),
                    historical,
                    outcome.evidence["attempted"]
                        .as_u64()
                        .map(|c| format!("button:{c}")),
                )
            })?;
            // All requested native values must still match the final activation
            // snapshot BEFORE output is armed, including scalars changed earlier.
            let mut prospective = device_description(&snapshot);
            for control in prospective["buttons"]["controls"].as_array_mut().unwrap() {
                if let Some(value) = plan.capture_bindings().get(control["id"].as_str().unwrap()) {
                    control["value"] = value.clone();
                    control["available"] = json!(true);
                }
            }
            changes
                .confirm(&prospective, &self.identity)
                .map_err(|e| error(e, confirmed.clone(), None))?;
            events
                .arm(&snapshot, deadline)
                .map_err(|e| error(e, confirmed.clone(), None))?;
            confirmed.extend(
                plan.capture_controls()
                    .iter()
                    .map(|c| format!("button:{c}")),
            );
            self.active = Some(plan);
        }
        let snapshot = self
            .inner()
            .map_err(|e| error(e, confirmed.clone(), None))?
            .current(deadline)
            .map_err(|e| error(e, confirmed.clone(), None))?;
        let snapshot = self.describe(&snapshot);
        changes
            .confirm(&snapshot, &self.identity)
            .map_err(|e| error(e, confirmed.clone(), None))?;
        Ok(Applied {
            confirmed,
            snapshot,
        })
    }
    fn release_active(&mut self, deadline: Instant) -> Result<(), String> {
        let inner = self
            .inner
            .as_mut()
            .ok_or("Guarded profile session is closed")?;
        inner.events.as_ref().unwrap().disarm()?;
        // A failed release keeps its cached error for the terminal cleanup.
        if let Some(sink) = &self.sink {
            sink.close()?;
        }
        self.sink = None;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or("Capture transition deadline elapsed")?;
        let evidence = self
            .recovery
            .as_ref()
            .ok_or("Active capture has no recovery record")?
            .release_live(&mut inner.session, remaining.min(Duration::from_secs(20)));
        self.release = Some(evidence.clone());
        if evidence["completed"] != true {
            // Keep the record: cleanup re-reads and clears only what remains.
            return Err(format!("Resident capture release unconfirmed: {evidence}"));
        }
        self.recovery = None;
        self.active = None;
        self.activation = None;
        Ok(())
    }
    fn attach_replacement(
        &mut self,
        plan: &ProfileButtonPlan,
        output: SinkHandle,
        deadline: Instant,
    ) -> Result<ReportingRecovery, String> {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("Capture preparation deadline elapsed")?;
        let (slot, identity) = (self.slot, self.identity.clone());
        let inner = self.inner()?;
        let recovery = ReportingRecovery::prepare(
            &mut inner.session,
            slot,
            &identity,
            &plan.capture_controls(),
            remaining.min(Duration::from_secs(20)),
        )?;
        let snapshot = inner.current(deadline)?;
        output.snapshot(&snapshot)?;
        self.sink = Some(output.clone());
        self.inner()?
            .events
            .as_ref()
            .unwrap()
            .prepare_output(output)?;
        Ok(recovery)
    }
    fn cleanup(&mut self) -> Result<(), String> {
        if let Some(result) = &self.closed {
            return result.clone();
        }
        let prepared = self.prepared.take().and_then(|p| p.output);
        // Release output, retire the sole active reader, then restore using the
        // retained descriptor. The outer owner still holds its hardware lease.
        let release = self.sink.take().map_or(Ok(()), |sink| sink.close());
        let release = release.and(prepared.map_or(Ok(()), |sink| sink.close()));
        drop(self.inner.take());
        let transport = self.transport.borrow_mut().take();
        let deferred = self.deferred.clone();
        let mut handed_over = false;
        let recovery = self.recovery.take().map(|plan| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let transport = transport.unwrap();
                let Some(slot) = &deferred else {
                    return plan.restore(transport, Duration::from_secs(20));
                };
                let (report, retry) = plan.restore_or_defer(transport, Duration::from_secs(20));
                if let Some(plan) = retry {
                    *slot.borrow_mut() = Some(plan);
                    handed_over = true;
                }
                report
            }))
            .unwrap_or_else(|_| json!({"completed":false,"error":"Resident recovery panicked"}))
        });
        self.active = None;
        let result = match (release, recovery) {
            (Ok(()), None) => Ok(()),
            (Ok(()), Some(ref r)) if r["completed"] == true => Ok(()),
            // The connector completes it before any other connection or exit.
            (Ok(()), Some(_)) if handed_over => Ok(()),
            (release, recovery) => Err(format!(
                "Resident cleanup unconfirmed: {}",
                json!({"output_error":release.err(),"recovery":recovery})
            )),
        };
        self.closed = Some(result.clone());
        result
    }
    fn fail(&mut self, error: String) -> String {
        match self.cleanup() {
            Ok(()) => error,
            Err(cleanup) => format!("{error}; {cleanup}"),
        }
    }
}
fn error(
    message: impl Into<String>,
    confirmed: Vec<String>,
    attempted: Option<String>,
) -> ApplyError {
    ApplyError {
        message: message.into(),
        confirmed,
        attempted,
    }
}
impl<T: Transport> ProfileBackend for GuardedProfileBackend<T> {
    fn validate_changes(&self, _changes: &Changes) -> Result<(), String> {
        Ok(())
    }
    fn identity(&self) -> &str {
        &self.identity
    }
    fn snapshot(&mut self, deadline: Instant) -> Result<Value, String> {
        self.inner()?.begin("snapshot");
        let result = self
            .inner()
            .and_then(|inner| inner.current(deadline))
            .map(|s| self.describe(&s));
        result.map_err(|e| self.fail(e))
    }
    fn prepare(&mut self, changes: &Changes, deadline: Instant) -> Result<(), String> {
        self.preflight(changes, deadline).map_err(|e| self.fail(e))
    }
    fn apply(&mut self, changes: &Changes, deadline: Instant) -> Result<Applied, ApplyError> {
        self.apply_prepared(changes, deadline).map_err(|mut e| {
            e.message = self.fail(e.message);
            e
        })
    }
    fn poll(&mut self, timeout: Duration) -> Result<bool, String> {
        self.inner()?.poll(timeout).map_err(|e| self.fail(e))
    }
    fn close(&mut self, _deadline: Instant) -> Result<(), String> {
        self.cleanup()
    }
}
impl<T: Transport> Drop for GuardedProfileBackend<T> {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
