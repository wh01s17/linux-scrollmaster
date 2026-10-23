// SPDX-License-Identifier: GPL-3.0-or-later
//! Lazy, single-session owner. A subsequent request may reconnect; apply never does.
use super::{
    profile_backend::validate_selection, profile_connector::check_time, ApplyError,
    ProfileConnector, SystemProfileConnector,
};
use crate::service::{Applied, Changes, ProfileBackend};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

struct Connection<B, L> {
    // Fields drop in declaration order: the descriptor closes before the lease.
    backend: B,
    _lease: L,
}
struct Preparation {
    changes: Changes,
    deadline: Instant,
}

pub struct NativeProfileOwner<C: ProfileConnector = SystemProfileConnector> {
    connector: C,
    slot: u8,
    identity: String,
    connection: Option<Connection<C::Backend, C::Lease>>,
    prepared: Option<Preparation>,
    attempts: u64,
    connections: u64,
    last_error: Option<String>,
    cleanup_error: Option<String>,
}
impl NativeProfileOwner<SystemProfileConnector> {
    /// Pure construction: no discovery, lease, profile file, HID or controller I/O.
    pub fn system(slot: u8, identity: &str) -> Result<Self, String> {
        Self::new(SystemProfileConnector, slot, identity)
    }
}
impl<C: ProfileConnector> NativeProfileOwner<C> {
    pub fn new(connector: C, slot: u8, identity: &str) -> Result<Self, String> {
        validate_selection(slot, identity)?;
        Ok(Self {
            connector,
            slot,
            identity: identity.into(),
            connection: None,
            prepared: None,
            attempts: 0,
            connections: 0,
            last_error: None,
            cleanup_error: None,
        })
    }
    /// Local lifecycle evidence, not a cached hardware snapshot or liveness probe.
    pub fn status(&self) -> Value {
        json!({"device":self.identity,"slot":self.slot,"session_owned":self.connection.is_some(),
            "connection_attempts":self.attempts,"validated_connections":self.connections,"last_error":self.last_error})
    }
    /// Retire the backend under its lease; a capture connector may restore HID.
    pub fn disconnect(&mut self) {
        self.prepared = None;
        if let Some(mut connection) = self.connection.take() {
            if let Err(error) = connection
                .backend
                .close(Instant::now() + Duration::from_secs(20))
            {
                self.cleanup_error = Some(error);
            }
            drop(connection);
        }
    }
    fn fail(&mut self, message: String) -> String {
        self.disconnect();
        let message = match &self.cleanup_error {
            Some(error) => format!("{message}; {error}"),
            None => message,
        };
        self.last_error = Some(message.clone());
        message
    }
    fn guard(&mut self, deadline: Instant) -> Result<(), String> {
        // A failed request is never repeated. A later request may reconnect only
        // when the failed backend confirmed its close (capture: restored HID);
        // an unconfirmed close is terminal for this owner.
        if let Some(error) = &self.cleanup_error {
            return Err(error.clone());
        }
        check_time(deadline)?;
        self.connector.check_controllers()?;
        check_time(deadline)
    }
    fn validate_snapshot(&self, snapshot: &Value, deadline: Instant) -> Result<(), String> {
        check_time(deadline)?;
        if snapshot["id"] != self.identity || !snapshot["settings"].is_object() {
            return Err("Native owner handshake identity/settings mismatch".into());
        }
        Ok(())
    }
    /// Returns the handshake reading only for a newly connected session.
    fn ensure_connection(&mut self, deadline: Instant) -> Result<Option<Value>, String> {
        self.guard(deadline)?;
        if self.connection.is_some() {
            return Ok(None);
        }
        self.attempts = self.attempts.saturating_add(1);
        let lease = self.connector.acquire()?;
        self.guard(deadline)?;
        let backend = self
            .connector
            .connect(self.slot, &self.identity, deadline)?;
        // Keep the drop order even if time, identity or handshake validation fails.
        self.connection = Some(Connection {
            backend,
            _lease: lease,
        });
        self.guard(deadline)?;
        let backend = &mut self.connection.as_mut().unwrap().backend;
        if backend.identity() != self.identity {
            return Err("Connector returned a backend for another identity".into());
        }
        let snapshot = backend.snapshot(deadline)?;
        self.validate_snapshot(&snapshot, deadline)?;
        self.connections = self.connections.saturating_add(1);
        self.last_error = None;
        Ok(Some(snapshot))
    }
    fn read(&mut self, deadline: Instant) -> Result<Value, String> {
        if let Some(snapshot) = self.ensure_connection(deadline)? {
            return Ok(snapshot);
        }
        let snapshot = self
            .connection
            .as_mut()
            .unwrap()
            .backend
            .snapshot(deadline)?;
        self.validate_snapshot(&snapshot, deadline)?;
        self.last_error = None;
        Ok(snapshot)
    }
    fn preflight(&mut self, changes: &Changes, deadline: Instant) -> Result<(), String> {
        self.connector.validate_changes(changes)?;
        self.ensure_connection(deadline)?;
        self.connection
            .as_mut()
            .unwrap()
            .backend
            .prepare(changes, deadline)?;
        check_time(deadline)?;
        self.prepared = Some(Preparation {
            changes: changes.clone(),
            deadline,
        });
        self.last_error = None;
        Ok(())
    }
    fn apply_prepared(
        &mut self,
        changes: &Changes,
        deadline: Instant,
    ) -> Result<Applied, ApplyError> {
        // Take the ticket BEFORE any check: each preflight authorizes one apply
        // on the same session, change set and request deadline. Never reconnect
        // between service preflight, persistent disarming and the first setter.
        let prepared = self.prepared.take().filter(|p| {
            p.deadline == deadline
                && p.changes.settings() == changes.settings()
                && p.changes.buttons() == changes.buttons()
        });
        if prepared.is_none() || self.connection.is_none() {
            return Err(unattempted(
                "Apply requires preflight on the current session and request".into(),
            ));
        }
        self.guard(deadline).map_err(unattempted)?;
        let applied = self
            .connection
            .as_mut()
            .unwrap()
            .backend
            .apply(changes, deadline)?;
        check_time(deadline)
            .and_then(|()| changes.confirm(&applied.snapshot, &self.identity))
            .map_err(|message| ApplyError {
                message,
                confirmed: applied.confirmed.clone(),
                attempted: None,
            })?;
        self.last_error = None;
        Ok(applied)
    }
}
fn unattempted(message: String) -> ApplyError {
    ApplyError {
        message,
        confirmed: vec![],
        attempted: None,
    }
}
impl<C: ProfileConnector> ProfileBackend for NativeProfileOwner<C> {
    fn validate_changes(&self, changes: &Changes) -> Result<(), String> {
        self.connector.validate_changes(changes)
    }
    fn poll(&mut self, timeout: Duration) -> Result<bool, String> {
        if timeout.is_zero() || timeout > Duration::from_millis(50) {
            return Err(
                "Resident poll requires a duration greater than zero and at most 50 ms".into(),
            );
        }
        if self.connection.is_none() {
            return Ok(false);
        }
        let deadline = Instant::now() + timeout;
        let result = (|| {
            // Only the terminal state here: the controller scan reads all of
            // /proc and runs on every request and connection instead. Inside
            // a 50 ms slice it exceeded the budget under load (seen on resume)
            // and tore down a healthy capture.
            if let Some(error) = &self.cleanup_error {
                return Err(error.clone());
            }
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .filter(|d| !d.is_zero())
                .ok_or("Resident poll budget elapsed")?;
            self.connection.as_mut().unwrap().backend.poll(remaining)
        })();
        result.map_err(|message| self.fail(message))
    }
    fn close(&mut self, deadline: Instant) -> Result<(), String> {
        self.disconnect();
        if let Some(error) = &self.cleanup_error {
            return Err(error.clone());
        }
        // A recovery handed to the connector is not confirmed until it runs.
        self.connector.finish(deadline)
    }
    fn identity(&self) -> &str {
        &self.identity
    }
    fn snapshot(&mut self, deadline: Instant) -> Result<Value, String> {
        self.prepared = None;
        self.read(deadline).map_err(|message| self.fail(message))
    }
    fn prepare(&mut self, changes: &Changes, deadline: Instant) -> Result<(), String> {
        self.prepared = None;
        self.preflight(changes, deadline)
            .map_err(|message| self.fail(message))
    }
    fn apply(&mut self, changes: &Changes, deadline: Instant) -> Result<Applied, ApplyError> {
        self.apply_prepared(changes, deadline)
            .map_err(|error| ApplyError {
                message: self.fail(error.message),
                ..error
            })
    }
}

impl<C: ProfileConnector> Drop for NativeProfileOwner<C> {
    fn drop(&mut self) {
        self.disconnect();
    }
}

#[cfg(test)]
#[path = "profile_owner_tests.rs"]
mod tests;
