// SPDX-License-Identifier: GPL-3.0-or-later
//! Independent owner's private RPC loop. No profile-file writer or software capture.
use super::{
    guardian::Channel,
    profile_guardian_wire::{
        decode_deadline, Preparation, Reply, Request, MAX_BUDGET, REPLY_LIMIT, REQUEST_LIMIT,
    },
};
use crate::service::{Changes, ProfileBackend};
use std::{
    io,
    time::{Duration, Instant},
};

pub(super) fn serve(mut backend: impl ProfileBackend, mut channel: Channel) -> Result<(), String> {
    send(
        &mut channel,
        Reply::Ready {
            protocol: 1,
            device: backend.identity().into(),
        },
    )?;
    let mut preparation: Option<Preparation> = None;
    let mut next_id = 1;
    let mut partial_deadline = None;
    let mut idle_error: Option<String> = None;
    loop {
        // Check IPC before EVERY passive HID turn, including under a busy event
        // stream. A partial frame has one absolute budget while events continue.
        let frame = match channel.try_receive_bytes(REQUEST_LIMIT) {
            Ok(Some(frame)) => {
                partial_deadline = None;
                frame
            }
            Ok(None) => {
                if channel.has_pending_bytes() && partial_deadline.is_none() {
                    partial_deadline = Some(Instant::now() + MAX_BUDGET);
                }
                if partial_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                    return Err("Partial profile guardian frame deadline elapsed".into());
                }
                let turn_end = Instant::now() + Duration::from_millis(50);
                let delivered = if idle_error.is_none() {
                    match backend.poll(Duration::from_millis(50)) {
                        Ok(delivered) => delivered,
                        Err(error) => {
                            preparation = None;
                            idle_error = Some(error);
                            false
                        }
                    }
                } else {
                    false
                };
                if !delivered && Instant::now() < turn_end {
                    match channel.wait_input_until(turn_end) {
                        Ok(()) => (),
                        Err(e) if e.kind() == io::ErrorKind::TimedOut => (),
                        Err(e) => return Err(e.to_string()),
                    }
                }
                continue;
            }
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof && !channel.has_pending_bytes() => {
                return Ok(())
            }
            Err(e) => return Err(e.to_string()),
        };
        let request: Request = serde_json::from_slice(&frame)
            .map_err(|e| format!("Invalid private profile request: {e}"))?;
        let id = request.id();
        if id != next_id {
            return Err("Profile guardian request sequence mismatch".into());
        }
        next_id = next_id
            .checked_add(1)
            .ok_or("Profile guardian sequence exhausted")?;
        // Idle failure has already released the owner's session/lease. Report
        // it once in-order on the next request; never reconnect or apply here.
        if !matches!(request, Request::Shutdown { .. }) {
            if let Some(error) = idle_error.take() {
                preparation = None;
                send(
                    &mut channel,
                    Reply::error(id, format!("Resident event stream failed: {error}")),
                )?;
                continue;
            }
        }
        let reply = match request {
            Request::Shutdown { .. } => {
                let closed = backend.close(Instant::now() + Duration::from_secs(20));
                drop(backend);
                return send(
                    &mut channel,
                    match closed {
                        Ok(()) => Reply::Stopped { id },
                        Err(error) => Reply::error(id, error),
                    },
                );
            }
            Request::Snapshot { deadline_ns, .. } => {
                preparation = None;
                let result: Result<serde_json::Value, String> = (|| {
                    let deadline = decode_deadline(deadline_ns)?;
                    let device = backend.snapshot(deadline)?;
                    check_time(deadline)?;
                    if device["id"] != backend.identity() || !device["settings"].is_object() {
                        return Err("Profile guardian snapshot identity/settings mismatch".into());
                    }
                    Ok(device)
                })();
                match result {
                    Ok(device) => Reply::Snapshot { id, device },
                    Err(e) => Reply::error(id, e),
                }
            }
            Request::Prepare {
                deadline_ns,
                settings,
                buttons,
                ..
            } => {
                preparation = None;
                let result: Result<Preparation, String> = (|| {
                    let changes = Changes::new(settings, buttons)?;
                    backend.validate_changes(&changes)?;
                    let deadline = decode_deadline(deadline_ns)?;
                    backend.prepare(&changes, deadline)?;
                    check_time(deadline)?;
                    Ok(Preparation {
                        ticket: id,
                        changes,
                        deadline,
                    })
                })();
                match result {
                    Ok(p) => {
                        preparation = Some(p);
                        Reply::Prepared { id, ticket: id }
                    }
                    Err(e) => Reply::error(id, e),
                }
            }
            Request::Apply { ticket, .. } => {
                // Consume authorization before any validation or mutation. Never
                // reconstruct the deadline or reconnect/repeat an uncertain apply.
                match preparation.take().filter(|p| p.ticket == ticket) {
                    None => Reply::error(id, "Apply requires the current one-use preparation"),
                    Some(p) => match check_time(p.deadline) {
                        Err(e) => Reply::error(id, e),
                        Ok(()) => match backend.apply(&p.changes, p.deadline) {
                            Err(e) => Reply::Error {
                                id,
                                error: e.message,
                                confirmed: e.confirmed,
                                attempted: e.attempted,
                            },
                            Ok(applied) => match check_time(p.deadline).and_then(|()| {
                                p.changes.confirm(&applied.snapshot, backend.identity())
                            }) {
                                Ok(()) => Reply::Applied {
                                    id,
                                    snapshot: applied.snapshot,
                                    confirmed: applied.confirmed,
                                },
                                Err(error) => Reply::Error {
                                    id,
                                    error,
                                    confirmed: applied.confirmed,
                                    attempted: None,
                                },
                            },
                        },
                    },
                }
            }
        };
        send(&mut channel, reply)?;
    }
}
fn check_time(deadline: Instant) -> Result<(), String> {
    if Instant::now() >= deadline {
        Err("Profile guardian request budget elapsed".into())
    } else {
        Ok(())
    }
}
fn send(channel: &mut Channel, reply: Reply) -> Result<(), String> {
    channel
        .send_bounded(
            &serde_json::to_value(reply).map_err(|e| e.to_string())?,
            REPLY_LIMIT,
            Instant::now() + Duration::from_secs(2),
        )
        .map_err(|e| e.to_string())
}
