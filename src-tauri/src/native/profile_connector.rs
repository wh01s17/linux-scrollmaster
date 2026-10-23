// SPDX-License-Identifier: GPL-3.0-or-later
//! System acquisition policy, separate from the reconnecting owner's state.
use super::profile_capture::DeferredRecovery;
use super::profile_events::Cancellation;
use super::{discover, ensure_no_controllers, HardwareLease, NativeProfileBackend};
use crate::{
    hidpp::{Hidraw, Session},
    service::ProfileBackend,
};
use std::time::{Duration, Instant};

/// Each successful connection transfers one exclusive backend to the owner.
/// Implementations must neither retain another reader nor change reporting.
/// The owner retains Lease until AFTER the backend has been destroyed, including
/// failed handshakes. Acquisition and connection must not retry mutations.
/// A later request may reconnect only after the backend confirmed its close,
/// or handed a recovery that sent no setter back to this connector, which then
/// completes it under the lease before any new reader.
pub trait ProfileConnector {
    type Backend: ProfileBackend;
    type Lease;
    fn validate_changes(&self, changes: &crate::service::Changes) -> Result<(), String> {
        super::profile_backend::validate_native_changes(changes)
    }
    fn check_controllers(&mut self) -> Result<(), String>;
    fn acquire(&mut self) -> Result<Self::Lease, String>;
    fn connect(
        &mut self,
        slot: u8,
        identity: &str,
        deadline: Instant,
    ) -> Result<Self::Backend, String>;
    /// Called after the owner released its backend and lease on close.
    fn finish(&mut self, _deadline: Instant) -> Result<(), String> {
        Ok(())
    }
}

/// Explicit experimental owner only; not used by the active GUI/Python worker.
pub struct SystemProfileConnector;
impl ProfileConnector for SystemProfileConnector {
    type Backend = NativeProfileBackend<Hidraw>;
    type Lease = HardwareLease;
    fn check_controllers(&mut self) -> Result<(), String> {
        ensure_no_controllers()
    }
    fn acquire(&mut self) -> Result<HardwareLease, String> {
        HardwareLease::acquire()
    }
    fn connect(
        &mut self,
        slot: u8,
        identity: &str,
        deadline: Instant,
    ) -> Result<Self::Backend, String> {
        NativeProfileBackend::new(Session::new(open_transport(deadline)?), slot, identity)
    }
}
// The guarded owner supplies the private IPC peer check to both initialization
// and the installed consumer. It never creates a second HID reader or thread.
pub(super) struct GuardianProfileConnector(pub Cancellation);
impl ProfileConnector for GuardianProfileConnector {
    type Backend = NativeProfileBackend<Hidraw>;
    type Lease = HardwareLease;
    fn check_controllers(&mut self) -> Result<(), String> {
        ensure_no_controllers()
    }
    fn acquire(&mut self) -> Result<Self::Lease, String> {
        HardwareLease::acquire()
    }
    fn connect(
        &mut self,
        slot: u8,
        identity: &str,
        deadline: Instant,
    ) -> Result<Self::Backend, String> {
        if self.0.check() {
            return Err("Profile guardian client disconnected".into());
        }
        let transport = open_transport(deadline)?;
        let cancelled = self.0.clone();
        let session =
            Session::with_initialization_cancellation(transport, move || cancelled.check());
        NativeProfileBackend::with_cancellation(session, slot, identity, self.0.clone())
    }
}
// Only selected by explicit capture child startup, never via RPC.
pub(super) struct CaptureProfileConnector {
    cancelled: Cancellation,
    deferred: DeferredRecovery,
    terminal: Option<String>,
}
impl CaptureProfileConnector {
    pub(super) fn new(cancelled: Cancellation) -> Self {
        Self {
            cancelled,
            deferred: DeferredRecovery::default(),
            terminal: None,
        }
    }
    /// Complete a handed-over recovery on a fresh reader. It rereads and only
    /// clears our temporary bits; a setter that may have changed the device
    /// makes this connector terminal, exactly like an unconfirmed close.
    fn recover(&mut self, deadline: Instant) -> Result<(), String> {
        if let Some(error) = &self.terminal {
            return Err(error.clone());
        }
        if self.deferred.borrow().is_none() {
            return Ok(());
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or("Native owner request budget elapsed")?;
        let transport = open_transport(deadline)?;
        let plan = self.deferred.borrow_mut().take().unwrap();
        let (report, retry) =
            plan.restore_or_defer(transport, remaining.min(Duration::from_secs(20)));
        if report["completed"] == true {
            return Ok(());
        }
        let message = format!("Previous capture recovery pending: {report}");
        match retry {
            Some(plan) => *self.deferred.borrow_mut() = Some(plan),
            None => self.terminal = Some(message.clone()),
        }
        Err(message)
    }
}
impl ProfileConnector for CaptureProfileConnector {
    type Backend = super::profile_capture::GuardedProfileBackend<Hidraw>;
    type Lease = HardwareLease;
    fn validate_changes(&self, _changes: &crate::service::Changes) -> Result<(), String> {
        Ok(())
    }
    fn check_controllers(&mut self) -> Result<(), String> {
        ensure_no_controllers()
    }
    fn acquire(&mut self) -> Result<Self::Lease, String> {
        HardwareLease::acquire()
    }
    fn connect(
        &mut self,
        slot: u8,
        identity: &str,
        deadline: Instant,
    ) -> Result<Self::Backend, String> {
        if self.cancelled.check() {
            return Err("Profile guardian client disconnected".into());
        }
        self.recover(deadline)?;
        Ok(super::profile_capture::GuardedProfileBackend::new(
            open_transport(deadline)?,
            slot,
            identity,
            self.cancelled.clone(),
        )?
        .with_deferred(self.deferred.clone()))
    }
    fn finish(&mut self, deadline: Instant) -> Result<(), String> {
        if self.terminal.is_none() && self.deferred.borrow().is_none() {
            return Ok(());
        }
        let _lease = HardwareLease::acquire()?;
        self.recover(deadline)
    }
}
fn open_transport(deadline: Instant) -> Result<Hidraw, String> {
    let discovery = discover()?;
    check_time(deadline)?;
    // Never guess between receivers or retain a stale hidraw number. This
    // bounded policy permits renumbering only when discovery is unambiguous;
    // the owner's read-only handshake then verifies the selected Unit ID.
    if discovery.candidates.len() != 1 {
        return Err(format!(
            "Expected one validated Bolt interface, found {}; discovery warnings: {:?}",
            discovery.candidates.len(),
            discovery.warnings
        ));
    }
    let transport = discovery.candidates[0].open()?;
    check_time(deadline)?;
    Ok(transport)
}

pub(super) fn check_time(deadline: Instant) -> Result<(), String> {
    if Instant::now() >= deadline {
        Err("Native owner request budget elapsed".into())
    } else {
        Ok(())
    }
}
