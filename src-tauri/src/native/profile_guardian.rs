// SPDX-License-Identifier: GPL-3.0-or-later
//! Lazy proxy: the independent child exclusively owns the native session/lease.
use super::{
    guardian::{spawn_channel, Channel},
    profile_backend::{validate_native_changes, validate_selection},
    profile_guardian_wire::{
        encode_deadline, Preparation, Reply, Request, REPLY_LIMIT, REQUEST_LIMIT,
    },
    ApplyError, NativeProfileOwner,
};
use crate::service::{Applied, Changes, ProfileBackend};
use serde_json::{json, Value};
use std::{
    process::{Child, Command},
    time::Instant,
};

struct Connection {
    child: Child,
    channel: Channel,
}
type Launcher = dyn FnMut(u8, &str) -> Result<Command, String>;
pub struct ProfileGuardianOwner {
    slot: u8,
    identity: String,
    connection: Option<Connection>,
    retiring: Option<Child>,
    prepared: Option<Preparation>,
    next_id: u64,
    launcher: Box<Launcher>,
    capture: bool,
    cleanup_error: Option<String>,
}
impl ProfileGuardianOwner {
    /// No process, HID, lease, compositor or profile-file I/O at construction.
    /// Requires the same executable to handle --profile-guardian-child at startup.
    pub fn system(slot: u8, identity: &str) -> Result<Self, String> {
        validate_selection(slot, identity)?;
        Ok(Self {
            slot,
            identity: identity.into(),
            connection: None,
            retiring: None,
            prepared: None,
            next_id: 1,
            capture: false,
            cleanup_error: None,
            launcher: Box::new(|slot, id| {
                let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
                command.args([
                    "--profile-guardian-child",
                    "--slot",
                    &slot.to_string(),
                    "--device",
                    id,
                ]);
                Ok(command)
            }),
        })
    }
    /// Explicit resident capture. The child owns transitions, cleanup and any
    /// reconnection; only an unknown child outcome blocks this proxy.
    pub fn system_capture(slot: u8, identity: &str) -> Result<Self, String> {
        let mut owner = Self::system(slot, identity)?;
        owner.capture = true;
        owner.launcher = Box::new(|slot, id| {
            let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
            command.args([
                "--profile-guardian-capture-child",
                "--slot",
                &slot.to_string(),
                "--device",
                id,
            ]);
            Ok(command)
        });
        Ok(owner)
    }
    /// Local process ownership evidence. This is not a device liveness reading.
    pub fn status(&self) -> Value {
        json!({"device":self.identity,"guardian_pid":self.connection.as_ref().map(|c|c.child.id()),
            "closing_pid":self.retiring.as_ref().map(Child::id)})
    }
    /// Close IPC without killing a child that may still own cleanup. A subsequent
    /// request cannot launch another owner until the previous child is reaped.
    pub fn disconnect(&mut self) {
        self.prepared = None;
        if let Some(connection) = self.connection.take() {
            drop(connection.channel);
            debug_assert!(self.retiring.is_none());
            self.retiring = Some(connection.child);
        }
    }
    fn fail(&mut self, message: impl ToString) -> String {
        if self.capture {
            self.cleanup_error = Some(format!(
                "Capture guardian outcome/cleanup unconfirmed: {}",
                message.to_string()
            ));
        }
        self.disconnect();
        message.to_string()
    }
    fn ensure(&mut self, deadline: Instant) -> Result<(), String> {
        if let Some(error) = &self.cleanup_error {
            return Err(error.clone());
        }
        encode_deadline(deadline)?;
        if let Some(child) = &mut self.retiring {
            match child.try_wait().map_err(|e| e.to_string())? {
                Some(_) => {
                    self.retiring = None;
                }
                None => {
                    return Err(
                        "Previous profile guardian is still closing; no new owner started".into(),
                    )
                }
            }
        }
        if self.connection.is_some() {
            return Ok(());
        }
        let command = (self.launcher)(self.slot, &self.identity)?;
        let (child, socket) = spawn_channel(command)?;
        let channel = match Channel::new(socket) {
            Ok(channel) => channel,
            Err(e) => {
                self.retiring = Some(child);
                return Err(e.to_string());
            }
        };
        self.connection = Some(Connection { child, channel });
        self.next_id = 1;
        let bytes = self
            .connection
            .as_mut()
            .unwrap()
            .channel
            .receive_bytes_bounded(REPLY_LIMIT, deadline)
            .map_err(|e| self.fail(e))?;
        match serde_json::from_slice::<Reply>(&bytes).map_err(|e| self.fail(e))? {
            Reply::Ready {
                protocol: 1,
                device,
            } if device == self.identity => Ok(()),
            _ => Err(self.fail("Profile guardian identity/protocol handshake failed")),
        }
    }
    fn request(&mut self, request: Request, deadline: Instant) -> Result<Reply, String> {
        let id = request.id();
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or("Profile guardian sequence exhausted")?;
        let value = serde_json::to_value(request).map_err(|e| e.to_string())?;
        let result = (|| {
            let connection = self
                .connection
                .as_mut()
                .ok_or("Profile guardian is closed")?;
            connection
                .channel
                .send_bounded(&value, REQUEST_LIMIT, deadline)
                .map_err(|e| e.to_string())?;
            let bytes = connection
                .channel
                .receive_bytes_bounded(REPLY_LIMIT, deadline)
                .map_err(|e| e.to_string())?;
            let reply: Reply = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if reply.id() != Some(id) || Instant::now() >= deadline {
                return Err("Profile guardian response sequence/deadline mismatch".into());
            }
            Ok(reply)
        })();
        result.map_err(|e: String| self.fail(e))
    }
    fn invalid(&mut self) -> String {
        self.fail("Unexpected profile guardian response")
    }
}
impl ProfileBackend for ProfileGuardianOwner {
    fn validate_changes(&self, changes: &Changes) -> Result<(), String> {
        if self.capture {
            Ok(())
        } else {
            validate_native_changes(changes)
        }
    }
    fn close(&mut self, deadline: Instant) -> Result<(), String> {
        self.prepared = None;
        if self.connection.is_some() {
            match self.request(Request::Shutdown { id: self.next_id }, deadline) {
                Ok(Reply::Stopped { .. }) => self.disconnect(),
                Ok(Reply::Error { error, .. }) => {
                    self.cleanup_error = Some(error.clone());
                    self.disconnect();
                    return Err(error);
                }
                Ok(_) => return Err(self.invalid()),
                Err(error) => {
                    return Err(format!(
                        "Profile guardian cleanup may still be running: {error}"
                    ))
                }
            }
        }
        while let Some(child) = &mut self.retiring {
            if child.try_wait().map_err(|e| e.to_string())?.is_some() {
                self.retiring = None;
                return self.cleanup_error.clone().map_or(Ok(()), Err);
            }
            if Instant::now() >= deadline {
                return Err(
                    "Profile guardian cleanup may still be running; shutdown is unconfirmed".into(),
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        self.cleanup_error.clone().map_or(Ok(()), Err)
    }
    fn identity(&self) -> &str {
        &self.identity
    }
    fn snapshot(&mut self, deadline: Instant) -> Result<Value, String> {
        self.prepared = None;
        self.ensure(deadline)?;
        let deadline_ns = encode_deadline(deadline)?;
        match self.request(
            Request::Snapshot {
                id: self.next_id,
                deadline_ns,
            },
            deadline,
        )? {
            Reply::Snapshot { device, .. }
                if device["id"] == self.identity && device["settings"].is_object() =>
            {
                Ok(device)
            }
            Reply::Error { error, .. } => Err(error),
            _ => Err(self.invalid()),
        }
    }
    fn prepare(&mut self, changes: &Changes, deadline: Instant) -> Result<(), String> {
        self.prepared = None;
        // Preserve offline policy: unsupported software never starts a process,
        // takes the lease or reads hardware, including when a profile is armed.
        self.validate_changes(changes)?;
        self.ensure(deadline)?;
        let id = self.next_id;
        let deadline_ns = encode_deadline(deadline)?;
        match self.request(
            Request::Prepare {
                id,
                deadline_ns,
                settings: changes.settings().cloned(),
                buttons: changes.buttons().cloned(),
            },
            deadline,
        )? {
            Reply::Prepared { ticket, .. } if ticket == id => {
                self.prepared = Some(Preparation {
                    ticket,
                    changes: changes.clone(),
                    deadline,
                });
                Ok(())
            }
            Reply::Error { error, .. } => Err(error),
            _ => Err(self.invalid()),
        }
    }
    fn apply(&mut self, changes: &Changes, deadline: Instant) -> Result<Applied, ApplyError> {
        let prepared = self.prepared.take().filter(|p| {
            p.deadline == deadline
                && p.changes.settings() == changes.settings()
                && p.changes.buttons() == changes.buttons()
        });
        let result = (|| {
            let p = prepared.ok_or_else(|| {
                unattempted("Apply requires the current one-use guardian preparation")
            })?;
            encode_deadline(deadline).map_err(unattempted)?;
            // Do not ensure, reconnect or restart here: disk disarming and apply
            // must use the very same prepared child's descriptor and deadline.
            match self
                .request(
                    Request::Apply {
                        id: self.next_id,
                        ticket: p.ticket,
                    },
                    deadline,
                )
                .map_err(|e| {
                    unattempted(format!(
                        "Profile guardian apply outcome uncertain; reread before retrying: {e}"
                    ))
                })? {
                Reply::Applied {
                    snapshot,
                    confirmed,
                    ..
                } => {
                    changes
                        .confirm(&snapshot, &self.identity)
                        .map_err(|message| ApplyError {
                            message,
                            confirmed: confirmed.clone(),
                            attempted: None,
                        })?;
                    Ok(Applied {
                        snapshot,
                        confirmed,
                    })
                }
                Reply::Error {
                    error,
                    confirmed,
                    attempted,
                    ..
                } => Err(ApplyError {
                    message: error,
                    confirmed,
                    attempted,
                }),
                _ => Err(unattempted(self.invalid())),
            }
        })();
        // A structured capture error was produced after the child finished its
        // own close/restore, or with capture still owned; keep that authority.
        // Lost IPC already went through `fail`, which is terminal for capture.
        if result.is_err() && !(self.capture && self.connection.is_some()) {
            self.disconnect();
        }
        result
    }
}
fn unattempted(message: impl ToString) -> ApplyError {
    ApplyError {
        message: message.to_string(),
        confirmed: vec![],
        attempted: None,
    }
}
impl Drop for ProfileGuardianOwner {
    fn drop(&mut self) {
        self.disconnect();
        if let Some(mut child) = self.retiring.take() {
            // No kill or blocking wait in the service path. Private IPC is CLOEXEC
            // in other children, so parent exit/SIGKILL also becomes EOF here.
            let _ = std::thread::Builder::new()
                .name("profile-guardian-reaper".into())
                .spawn(move || {
                    let _ = child.wait();
                });
        }
    }
}
pub fn run_profile_guardian_child(slot: u8, identity: &str) -> Result<(), String> {
    run_child(slot, identity, false)
}
pub fn run_profile_capture_child(slot: u8, identity: &str) -> Result<(), String> {
    run_child(slot, identity, true)
}
fn run_child(slot: u8, identity: &str, capture: bool) -> Result<(), String> {
    let channel = Channel::inherited()?;
    let fd = channel.fd();
    let cancelled =
        super::profile_events::Cancellation::new(move || super::guardian::peer_gone(fd));
    if capture {
        let owner = NativeProfileOwner::new(
            super::profile_connector::CaptureProfileConnector::new(cancelled),
            slot,
            identity,
        )?;
        return super::profile_guardian_server::serve(owner, channel);
    }
    let owner = NativeProfileOwner::new(
        super::profile_connector::GuardianProfileConnector(cancelled),
        slot,
        identity,
    )?;
    super::profile_guardian_server::serve(owner, channel)
}

#[cfg(test)]
#[path = "profile_guardian_tests.rs"]
mod tests;
