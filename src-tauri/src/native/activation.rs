// SPDX-License-Identifier: GPL-3.0-or-later
//! Fixed N4 setters, accessible only through a gated owner and recovery record.
use super::{
    buttons::{describe_session, reporting},
    owner::binding,
    ActionBindings, ReportingRecovery,
};
use crate::hidpp::{ReportKind, Session, Transport};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub(super) struct Activation {
    pub evidence: Value,
    pub snapshot: Option<Value>,
}
impl Activation {
    pub(super) fn failed(error: String) -> Self {
        Self {
            evidence: json!({"completed":false,"setters_attempted":0,"confirmed":[],"attempted":null,"error":error}),
            snapshot: None,
        }
    }
}

/// Never installs/replaces a consumer or retries a setter. Output must already
/// be gated; all notifications continue through the caller's sole consumer.
// Each argument is a distinct capability of one activation; grouping them would
// only hide which lease, recovery and deadline a caller authorized.
#[allow(clippy::too_many_arguments)]
pub(super) fn activate<T: Transport>(
    session: &mut Session<T>,
    slot: u8,
    id: &str,
    baseline: &Value,
    actions: &ActionBindings,
    recovery: &ReportingRecovery,
    deadline: Instant,
    mut before_setter: impl FnMut(),
) -> Activation {
    let controls = actions.controls();
    let mut confirmed = Vec::new();
    let mut attempted = None;
    let mut setters_attempted = 0usize;
    let mut expected = baseline.clone();
    let mut snapshot = None;
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
            if controls.is_empty() || !session.has_consumer() {
                return Err(
                    "Capture requires explicit assignments and an installed consumer".into(),
                );
            }
            recovery.permits_activation(baseline, slot, id, &controls)?;
            actions.validate_snapshot(baseline)?;
            if baseline["controls"]
                .as_array()
                .ok_or("Missing capture baseline")?
                .iter()
                .any(|c| c["flags"] != 0)
            {
                return Err("Capture baseline is already intercepted or has unknown flags".into());
            }
            for &cid in &controls {
                // Fresh identity, capabilities and reporting before EACH write.
                let current = describe_session(session, slot, id, deadline)?;
                recovery.permits_activation(&current, slot, id, &controls)?;
                actions.validate_snapshot(&current)?;
                if binding(&current) != binding(&expected) {
                    return Err(
                        "Capture configuration changed before activation; no further setter sent"
                            .into(),
                    );
                }
                let index = current["feature"]["index"]
                    .as_u64()
                    .ok_or("Missing capture feature")? as u8;
                let control = current["controls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|c| c["id"].as_str() == Some(cid.to_string().as_str()))
                    .ok_or("Missing capture control")?;
                if control["available"] != true {
                    return Err("Capture control is unavailable".into());
                }
                let (target, flags, _) = reporting(session, slot, index, cid, deadline)?;
                if flags != 0 || control["reported_target"] != target {
                    return Err("Capture reporting or mapping changed before setter".into());
                }
                // Temporary diversion (+ raw XY only for 195); remap zero retains
                // the current mapping. No persistent or unrelated valid bits.
                let owned = if cid == 195 { 0x11u8 } else { 0x01u8 };
                let from = cid.to_be_bytes();
                let params = [from[0], from[1], owned | (owned << 1), 0, 0, 0];
                let remaining = deadline
                    .checked_duration_since(Instant::now())
                    .filter(|d| !d.is_zero())
                    .ok_or("Capture activation budget elapsed")?;
                attempted = Some(cid);
                setters_attempted += 1;
                before_setter();
                let ack = session
                    .request(
                        ReportKind::Long,
                        slot,
                        index,
                        3,
                        &params,
                        remaining.min(Duration::from_secs(2)),
                    )
                    .map_err(|e| e.to_string())?;
                if ack.payload().get(..params.len()) != Some(params.as_slice()) {
                    return Err("Capture ACK did not echo the fixed request".into());
                }
                let (after_target, after_flags, _) =
                    reporting(session, slot, index, cid, deadline)?;
                if after_target != target || after_flags != u16::from(owned) {
                    return Err(format!("Capture not confirmed by hardware for CID {cid}"));
                }
                confirmed.push(cid);
                attempted = None;
                let row = expected["controls"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|c| c["id"].as_str() == Some(cid.to_string().as_str()))
                    .unwrap();
                row["flags"] = json!(owned);
            }
            let current = describe_session(session, slot, id, deadline)?;
            recovery.permits_activation(&current, slot, id, &controls)?;
            actions.validate_snapshot(&current)?;
            if binding(&current) != binding(&expected) {
                return Err("Capture changed during final confirmation".into());
            }
            snapshot = Some(current);
            Ok(())
        }))
        .unwrap_or_else(|_| Err("Capture activation panicked".into()));
    let pending: Vec<_> = controls
        .iter()
        .filter(|c| !confirmed.contains(c))
        .copied()
        .collect();
    Activation {
        evidence: json!({"completed":result.is_ok(),"confirmed":confirmed,"attempted":attempted,
        "setters_attempted":setters_attempted,"pending":pending,"error":result.err()}),
        snapshot,
    }
}
