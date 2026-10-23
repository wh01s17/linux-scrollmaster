// SPDX-License-Identifier: GPL-3.0-or-later
//! N4 recovery primitive. It can only clear registered temporary reporting flags.
//! No diversion enabling, action output, subprocess or automatic reconnection.
use super::buttons::{describe_session, reporting};
use crate::hidpp::{Report, ReportConsumer, ReportKind, Session, Transport};
use serde_json::{json, Value};
use std::{
    cell::Cell,
    collections::BTreeSet,
    rc::Rc,
    time::{Duration, Instant},
};

const MAX_RECOVERY_REPORTS: usize = 4096;

#[derive(Default)]
struct Progress {
    confirmed: Vec<u16>,
    written: Vec<u16>,
    attempted: Option<u16>,
}

/// Opaque, one-shot recovery registration from real uncached getters. Preparing
/// this plan neither enables reporting nor guarantees an independent guardian.
/// The caller must retain the hardware lease and ensure exclusive reader handoff
/// before giving a transport to `restore`; closing a pipe alone is not a handoff.
pub struct ReportingRecovery {
    slot: u8,
    id: String,
    layout: Value,
    controls: BTreeSet<u16>,
    dpi: Option<u16>,
}

fn deadline(timeout: Duration) -> Result<Instant, String> {
    if timeout.is_zero() || timeout > Duration::from_secs(20) {
        return Err("Recovery timeout must be greater than zero and at most 20 seconds".into());
    }
    Ok(Instant::now() + timeout)
}

// Compare hardware capabilities, not mutable reporting, mappings or scalar values.
fn layout(snapshot: &Value) -> Value {
    json!({"feature":snapshot["feature"], "count":snapshot["catalogue_count"],
        "controls":snapshot["controls"].as_array().map(|controls| controls.iter().map(|c|
            json!({"id":c["id"],"capabilities":c["capabilities"],"choices":c["choices"]})
        ).collect::<Vec<_>>())})
}

// Fixed recovery-only sink: no decoding or actions before identity is validated.
// A fresh recovery reader must not overflow the normal FIFO while buttons remain
// diverted. Count discarded unmatched reports and fail at a finite work limit.
struct RecoverySink(Rc<Cell<usize>>);
impl ReportConsumer for RecoverySink {
    fn consume(&mut self, _: &Report) -> Result<(), String> {
        let count = self.0.get();
        if count == MAX_RECOVERY_REPORTS {
            return Err("Recovery unmatched report limit reached".into());
        }
        self.0.set(count + 1);
        Ok(())
    }
}

impl ReportingRecovery {
    pub(super) fn initial_dpi(&self) -> Option<u16> {
        self.dpi
    }
    /// A private activation capability tied to this validated recovery record.
    /// Mutable reporting is checked by activation against its own expected state.
    pub(super) fn permits_activation(
        &self,
        snapshot: &Value,
        slot: u8,
        id: &str,
        controls: &[u16],
    ) -> Result<(), String> {
        if slot != self.slot
            || id != self.id
            || layout(snapshot) != self.layout
            || controls.iter().copied().collect::<BTreeSet<_>>() != self.controls
        {
            return Err(
                "Capture does not match the registered recovery identity, layout or controls"
                    .into(),
            );
        }
        Ok(())
    }
    /// Register only secondary CIDs 83/86/195, after checking their real reporting
    /// is zero. CID 195 registers both temporary diversion and RAW_XY; 83/86 only
    /// register temporary diversion. Invalid lists/timeouts fail before HID I/O.
    pub fn prepare<T: Transport>(
        session: &mut Session<T>,
        slot: u8,
        id: &str,
        controls: &[u16],
        timeout: Duration,
    ) -> Result<Self, String> {
        let deadline = deadline(timeout)?;
        let selected: BTreeSet<_> = controls.iter().copied().collect();
        if selected.is_empty()
            || selected.len() != controls.len()
            || !selected.iter().all(|c| [83, 86, 195].contains(c))
        {
            return Err("Register one to three unique secondary CIDs 83/86/195".into());
        }
        let snapshot = describe_session(session, slot, id, deadline)?;
        for cid in &selected {
            let control = snapshot["controls"]
                .as_array()
                .and_then(|a| {
                    a.iter()
                        .find(|c| c["id"].as_str() == Some(cid.to_string().as_str()))
                })
                .ok_or_else(|| format!("Recovery control not advertised: {cid}"))?;
            let flags = control["capabilities"]["flags"].as_u64().unwrap_or(0);
            if control["available"] != true
                || control["flags"] != 0
                || flags & 0x31 != 0x31
                || flags & 0x80 != 0
                || (*cid == 195 && flags & 0x100 == 0)
            {
                return Err(format!(
                    "Recovery control unavailable or already intercepted: {cid}"
                ));
            }
        }
        Ok(Self {
            slot,
            id: id.into(),
            layout: layout(&snapshot),
            controls: selected,
            dpi: snapshot["snapshot"]["device"]["settings"]["dpi"]["value"]
                .as_u64()
                .and_then(|v| u16::try_from(v).ok())
                .filter(|v| *v != 0),
        })
    }

    /// Consume the registration and an exclusively owned transport. No reopen,
    /// enable bits, remaps or retry are possible here. The fresh reader validates
    /// identity/layout before any write. Replies and uncached readback must agree.
    /// On failure, `confirmed` records earlier successful readbacks, `attempted`
    /// identifies a possibly changed control, and `pending` includes that control.
    pub fn restore<T: Transport>(self, transport: T, timeout: Duration) -> Value {
        self.run_restore(transport, timeout)
    }
    /// `restore` that hands the record back when no setter can be in flight
    /// (`attempted` null): every control is confirmed or untouched, so a later
    /// attempt on a fresh reader rereads before deciding anything. Seen after
    /// resume: the mouse link was down (HID++ 1.0 error 4) and nothing was sent.
    /// A possibly changed setter consumes the record; it is never replayed.
    pub(super) fn restore_or_defer<T: Transport>(
        self,
        transport: T,
        timeout: Duration,
    ) -> (Value, Option<Self>) {
        let report = self.run_restore(transport, timeout);
        let retry = report["completed"] != true && report["attempted"].is_null();
        (report, retry.then_some(self))
    }
    fn run_restore<T: Transport>(&self, transport: T, timeout: Duration) -> Value {
        let discarded = Rc::new(Cell::new(0));
        let mut session = Session::new(transport);
        let mut progress = Progress::default();
        let result = deadline(timeout).and_then(|deadline| {
            session
                .install_consumer(RecoverySink(discarded.clone()))
                .map_err(|e| e.to_string())?;
            self.clear(&mut session, deadline, &mut progress)
        });
        // Release the descriptor before returning evidence, even on validation
        // failure. An unconfirmed setter is not automatically replayed.
        drop(session);
        self.evidence(
            "reporting-recovery",
            result,
            progress,
            json!(discarded.get()),
        )
    }

    /// Clear the same registered temporary bits through the caller's live
    /// session and its sole installed consumer, keeping the descriptor. The
    /// record is borrowed: on failure the caller keeps it for terminal restore.
    /// Output must already be released; this never enables or replays a setter.
    pub(super) fn release_live<T: Transport>(
        &self,
        session: &mut Session<T>,
        timeout: Duration,
    ) -> Value {
        let mut progress = Progress::default();
        let result = deadline(timeout).and_then(|deadline| {
            if !session.has_consumer() {
                return Err("Live release requires the resident consumer".into());
            }
            self.clear(session, deadline, &mut progress)
        });
        self.evidence("live-reporting-release", result, progress, Value::Null)
    }

    fn clear<T: Transport>(
        &self,
        session: &mut Session<T>,
        deadline: Instant,
        progress: &mut Progress,
    ) -> Result<(), String> {
        let snapshot = describe_session(session, self.slot, &self.id, deadline)?;
        if layout(&snapshot) != self.layout {
            return Err("Recovery feature or control capabilities changed; no setter sent".into());
        }
        let index = snapshot["feature"]["index"]
            .as_u64()
            .ok_or("Missing recovery feature")? as u8;
        let mut expected = Vec::new();
        for &cid in &self.controls {
            let (target, flags, _) = reporting(session, self.slot, index, cid, deadline)?;
            let owned = if cid == 195 { 0x11 } else { 0x01 };
            let restored = flags & !owned;
            if flags != restored {
                // Only change-valid bits for our temporary flags are set.
                // Zero remap retains the CURRENT mapping, including changes
                // made since registration. All unrelated flags are untouched.
                let from = cid.to_be_bytes();
                let params = [from[0], from[1], (owned << 1) as u8, 0, 0, 0];
                let remaining = deadline
                    .checked_duration_since(Instant::now())
                    .filter(|d| !d.is_zero())
                    .ok_or("Recovery budget elapsed")?;
                progress.attempted = Some(cid);
                let ack = session
                    .request(
                        ReportKind::Long,
                        self.slot,
                        index,
                        3,
                        &params,
                        remaining.min(Duration::from_secs(2)),
                    )
                    .map_err(|e| e.to_string())?;
                if ack.payload().get(..params.len()) != Some(params.as_slice()) {
                    return Err("Recovery ACK did not echo the fixed request".into());
                }
                let (after_target, after_flags, _) =
                    reporting(session, self.slot, index, cid, deadline)?;
                if after_target != target || after_flags != restored {
                    return Err(format!("Recovery not confirmed by hardware for CID {cid}"));
                }
                progress.written.push(cid);
            }
            progress.confirmed.push(cid);
            expected.push((cid, target, restored));
            progress.attempted = None;
        }
        // Recheck the whole registered set before declaring full recovery.
        progress.confirmed.clear();
        for (cid, target, flags) in expected {
            let (current_target, current_flags, _) =
                reporting(session, self.slot, index, cid, deadline)?;
            if (current_target, current_flags) != (target, flags) {
                return Err(format!(
                    "Recovery changed during final confirmation for CID {cid}"
                ));
            }
            progress.confirmed.push(cid);
        }
        Ok(())
    }

    fn evidence(
        &self,
        mode: &str,
        result: Result<(), String>,
        progress: Progress,
        discarded: Value,
    ) -> Value {
        let pending: Vec<_> = self
            .controls
            .iter()
            .filter(|c| !progress.confirmed.contains(c))
            .copied()
            .collect();
        json!({"stage":"N4", "experimental":true,"mode":mode,
            "device":self.id,"completed":result.is_ok(),"registered":self.controls,
            "confirmed":progress.confirmed,"written":progress.written,"attempted":progress.attempted,"pending":pending,
            "discarded_reports":discarded,"error":result.err(),"diversion_enabled_by_recovery":false})
    }
}
