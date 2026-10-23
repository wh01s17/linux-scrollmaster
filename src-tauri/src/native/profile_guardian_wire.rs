// SPDX-License-Identifier: GPL-3.0-or-later
//! Closed private profile RPC. Absolute CLOCK_MONOTONIC deadlines cross exec.
use crate::service::Changes;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{Duration, Instant};

pub(super) const REQUEST_LIMIT: usize = 8192;
pub(super) const REPLY_LIMIT: usize = 2 * 1024 * 1024 - 1;
pub(super) const MAX_BUDGET: Duration = Duration::from_secs(30);

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Request {
    Snapshot {
        id: u64,
        deadline_ns: u64,
    },
    Prepare {
        id: u64,
        deadline_ns: u64,
        settings: Option<Value>,
        buttons: Option<Value>,
    },
    Apply {
        id: u64,
        ticket: u64,
    },
    Shutdown {
        id: u64,
    },
}
impl Request {
    pub fn id(&self) -> u64 {
        match self {
            Self::Snapshot { id, .. }
            | Self::Prepare { id, .. }
            | Self::Apply { id, .. }
            | Self::Shutdown { id } => *id,
        }
    }
}
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Reply {
    Ready {
        protocol: u8,
        device: String,
    },
    Snapshot {
        id: u64,
        device: Value,
    },
    Prepared {
        id: u64,
        ticket: u64,
    },
    Applied {
        id: u64,
        snapshot: Value,
        confirmed: Vec<String>,
    },
    Error {
        id: u64,
        error: String,
        confirmed: Vec<String>,
        attempted: Option<String>,
    },
    Stopped {
        id: u64,
    },
}
impl Reply {
    pub fn id(&self) -> Option<u64> {
        match self {
            Self::Ready { .. } => None,
            Self::Snapshot { id, .. }
            | Self::Prepared { id, .. }
            | Self::Applied { id, .. }
            | Self::Error { id, .. }
            | Self::Stopped { id } => Some(*id),
        }
    }
    pub fn error(id: u64, error: impl ToString) -> Self {
        Self::Error {
            id,
            error: error.to_string(),
            confirmed: vec![],
            attempted: None,
        }
    }
}
pub(super) struct Preparation {
    pub ticket: u64,
    pub changes: Changes,
    pub deadline: Instant,
}
fn monotonic_ns() -> Result<u64, String> {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: initialized output buffer for this process's kernel monotonic clock.
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    u64::try_from(time.tv_sec)
        .ok()
        .and_then(|s| s.checked_mul(1_000_000_000))
        .and_then(|n| n.checked_add(time.tv_nsec as u64))
        .ok_or_else(|| "Invalid monotonic clock".into())
}
pub(super) fn encode_deadline(deadline: Instant) -> Result<u64, String> {
    // Clock read precedes Instant::now: conversion is conservative and does not
    // grant transmission time to the child. No wall-clock/time-zone dependency.
    let clock = monotonic_ns()?;
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero() && *d <= MAX_BUDGET)
        .ok_or("Profile guardian budget must be greater than zero and at most 30 seconds")?;
    clock
        .checked_add(remaining.as_nanos() as u64)
        .ok_or_else(|| "Deadline overflow".into())
}
pub(super) fn decode_deadline(clock: u64) -> Result<Instant, String> {
    // Sample Instant BEFORE the clock, keeping the local equivalent conservative.
    let now = Instant::now();
    let remaining = clock
        .checked_sub(monotonic_ns()?)
        .filter(|n| *n > 0)
        .map(Duration::from_nanos)
        .filter(|d| *d <= MAX_BUDGET)
        .ok_or("Profile guardian deadline elapsed or exceeds budget")?;
    Ok(now + remaining)
}
