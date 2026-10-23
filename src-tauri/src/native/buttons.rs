// SPDX-License-Identifier: GPL-3.0-or-later
//! N3 native firmware remapping. No diversion flags, task injection or actions.
use super::{diagnostic::diagnose_session, ApplyError};
use crate::hidpp::{ReportKind, Session, Transport};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

const EDITABLE: &[u16] = &[83, 86, 195];
const MAX_CONTROLS: u8 = 64;
#[derive(Clone)]
struct Control {
    cid: u16,
    tid: u16,
    flags: u16,
    position: u8,
    group: u8,
    mask: u8,
    raw: Vec<u8>,
}
fn label(cid: u16) -> String {
    match cid {
        80 => "Clic izquierdo",
        81 => "Clic derecho",
        82 => "Clic central",
        83 => "Atrás",
        86 => "Adelante",
        195 => "Función nativa del botón de gestos",
        196 => "Cambiar modo de rueda",
        _ => return format!("Función nativa {cid:04X}"),
    }
    .into()
}
fn read<T: Transport>(
    session: &mut Session<T>,
    slot: u8,
    index: u8,
    function: u8,
    params: &[u8],
    deadline: Instant,
    needed: usize,
) -> Result<Vec<u8>, String> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or("Button diagnostic deadline elapsed")?;
    let reply = session
        .request(
            ReportKind::Long,
            slot,
            index,
            function,
            params,
            remaining.min(Duration::from_secs(2)),
        )
        .map_err(|e| e.to_string())?;
    if reply.payload().len() < needed {
        return Err("Truncated 1B04 response".into());
    }
    Ok(reply.payload().to_vec())
}
fn capabilities<T: Transport>(
    session: &mut Session<T>,
    slot: u8,
    index: u8,
    deadline: Instant,
) -> Result<BTreeMap<u16, Control>, String> {
    let count = read(session, slot, index, 0, &[], deadline, 1)?[0];
    if count == 0 || count > MAX_CONTROLS {
        return Err("Invalid or unsupported control count (1..64)".into());
    }
    let mut result = BTreeMap::new();
    for row in 0..count {
        let p = read(session, slot, index, 1, &[row], deadline, 9)?;
        let control = Control {
            cid: u16::from_be_bytes([p[0], p[1]]),
            tid: u16::from_be_bytes([p[2], p[3]]),
            flags: u16::from(p[4]) | (u16::from(p[8]) << 8),
            position: p[5],
            group: p[6],
            mask: p[7],
            raw: p,
        };
        if control.cid == 0 || control.group > 8 || result.insert(control.cid, control).is_some() {
            return Err("Invalid or duplicate CID/group in control table".into());
        }
    }
    Ok(result)
}
fn choices(control: &Control, table: &BTreeMap<u16, Control>) -> BTreeSet<u16> {
    let mut result = BTreeSet::from([control.cid]);
    for target in table.values() {
        if target.group != 0 && control.mask & (1 << (target.group - 1)) != 0 {
            result.insert(target.cid);
        }
    }
    result
}
pub(super) fn reporting<T: Transport>(
    session: &mut Session<T>,
    slot: u8,
    index: u8,
    cid: u16,
    deadline: Instant,
) -> Result<(u16, u16, Vec<u8>), String> {
    let p = read(session, slot, index, 2, &cid.to_be_bytes(), deadline, 6)?;
    if u16::from_be_bytes([p[0], p[1]]) != cid {
        return Err("Reporting response belongs to another CID".into());
    }
    let mapped = u16::from_be_bytes([p[3], p[4]]);
    let flags = u16::from(p[2]) | (u16::from(p[5]) << 8);
    Ok((if mapped == 0 { cid } else { mapped }, flags, p))
}
pub fn validate_button_changes(changes: &Value) -> Result<(), String> {
    let changes = changes
        .as_object()
        .filter(|c| !c.is_empty() && c.len() <= EDITABLE.len())
        .ok_or("Expected one to three native button assignments")?;
    for (source, target) in changes {
        if !EDITABLE.iter().any(|cid| source == &cid.to_string())
            || !target.as_u64().is_some_and(|v| (1..=65535).contains(&v))
        {
            return Err(
                "Only secondary CIDs 83/86/195 and integer native target CIDs are permitted".into(),
            );
        }
    }
    Ok(())
}
fn valid_identity(id: &str) -> bool {
    id.len() == 8
        && id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_lowercase())
}

/// Uncached snapshot, after verifying the device on this same exclusively owned session.
pub(super) fn describe_session<T: Transport>(
    session: &mut Session<T>,
    slot: u8,
    id: &str,
    deadline: Instant,
) -> Result<Value, String> {
    if !valid_identity(id) {
        return Err("Expected uppercase stable Unit ID".into());
    }
    let snapshot = diagnose_session(session, slot, deadline)?;
    if snapshot["device"]["id"] != id {
        return Err("Button device identity does not match selected Unit ID".into());
    }
    let feature = snapshot["features"]
        .as_array()
        .and_then(|a| a.iter().find(|f| f["id"] == "1B04"))
        .ok_or("1B04 not advertised")?;
    if feature["flags"] != 0
        || !feature["version"]
            .as_u64()
            .is_some_and(|v| (2..=5).contains(&v))
    {
        return Err("Unsupported or hidden/obsolete 1B04 version".into());
    }
    let index = feature["index"]
        .as_u64()
        .and_then(|v| u8::try_from(v).ok())
        .filter(|v| *v != 0 && !matches!(*v, 0x8f | 0xff))
        .ok_or("Invalid 1B04 index")?;
    let table = capabilities(session, slot, index, deadline)?;
    let mut controls = Vec::new();
    for &cid in EDITABLE {
        let Some(control) = table.get(&cid) else {
            continue;
        };
        // Require a physical, explicitly reprogrammable mouse control.
        if control.flags & 0x11 != 0x11 || control.flags & 0x80 != 0 {
            continue;
        }
        let targets = choices(control, &table);
        let (target, flags, raw) = reporting(session, slot, index, cid, deadline)?;
        let error = if flags != 0 {
            Some("Reporting is intercepted or has unknown flags; native remapping is blocked")
        } else if !targets.contains(&target) {
            Some("Current target is not in the advertised mapping groups")
        } else {
            None
        };
        controls.push(json!({"id":cid.to_string(),"label":label(cid),"available":error.is_none(),"value":if error.is_none(){Some(target)}else{None},"reported_target":target,
            "flags":flags,"raw":hex(&raw),"error":error,"choices":targets.iter().map(|&value|json!({"value":value,"label":label(value)})).collect::<Vec<_>>(),
            "capabilities":{"task":control.tid,"flags":control.flags,"position":control.position,"group":control.group,"group_mask":control.mask,"raw":hex(&control.raw)}}));
    }
    if controls.is_empty() {
        return Err("No supported secondary reprogrammable mouse controls".into());
    }
    Ok(
        json!({"stage":"N3","read_only":true,"experimental":true,"mode":"native-buttons","device":id,"feature":feature,"catalogue_count":table.len(),"controls":controls,"snapshot":snapshot}),
    )
}
pub fn read_buttons<T: Transport>(
    transport: T,
    slot: u8,
    id: &str,
    deadline: Instant,
) -> Result<Value, String> {
    describe_session(&mut Session::new(transport), slot, id, deadline)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}
fn control<'a>(snapshot: &'a Value, cid: &str) -> Result<&'a Value, String> {
    snapshot["controls"]
        .as_array()
        .and_then(|a| a.iter().find(|c| c["id"] == cid))
        .filter(|c| c["available"] == true)
        .ok_or_else(|| format!("Button unavailable: {cid}"))
}
fn allowed(snapshot: &Value, source: &str, target: &Value) -> Result<(), String> {
    let c = control(snapshot, source)?;
    if !c["choices"]
        .as_array()
        .is_some_and(|a| a.iter().any(|choice| choice["value"] == *target))
    {
        return Err(format!("Target not advertised for button {source}"));
    }
    Ok(())
}
pub(super) fn prepare_snapshot(snapshot: &Value, changes: &Value) -> Result<(), String> {
    validate_button_changes(changes)?;
    for (source, target) in changes.as_object().unwrap() {
        allowed(snapshot, source, target)?;
    }
    Ok(())
}
/// Only remap bytes are set; all reporting values and change-valid bits stay zero.
/// The complete batch is checked first. ACK and subsequent uncached reporting must agree.
pub fn apply_button_changes<T: Transport>(
    transport: T,
    slot: u8,
    id: &str,
    changes: &Value,
    deadline: Instant,
) -> Result<Value, ApplyError> {
    apply_button_changes_session(&mut Session::new(transport), slot, id, changes, deadline)
}

/// Borrow the existing owner so notifications survive preflight and readback.
/// This does not enable diversion or constitute a continuous action consumer.
pub(super) fn apply_button_changes_session<T: Transport>(
    session: &mut Session<T>,
    slot: u8,
    id: &str,
    changes: &Value,
    deadline: Instant,
) -> Result<Value, ApplyError> {
    let before_error = |message: String| ApplyError {
        message,
        confirmed: vec![],
        attempted: None,
    };
    validate_button_changes(changes).map_err(before_error)?;
    if !valid_identity(id) {
        return Err(before_error("Expected uppercase stable Unit ID".into()));
    }
    let changes = changes
        .as_object()
        .ok_or_else(|| before_error("Invalid assignments".into()))?;
    let mut snapshot = describe_session(session, slot, id, deadline).map_err(before_error)?;
    prepare_snapshot(&snapshot, &Value::Object(changes.clone())).map_err(before_error)?;
    let mut confirmed = Vec::<String>::new();
    for (source, target) in changes {
        let c = control(&snapshot, source).map_err(|message| ApplyError {
            message,
            confirmed: confirmed.clone(),
            attempted: None,
        })?;
        if c["value"] == *target {
            continue;
        }
        let fail = |message: String| ApplyError {
            message,
            confirmed: confirmed.clone(),
            attempted: Some(format!("button:{source}")),
        };
        allowed(&snapshot, source, target).map_err(fail)?;
        let index = snapshot["feature"]["index"].as_u64().unwrap() as u8;
        let cid = source
            .parse::<u16>()
            .map_err(|_| fail("Invalid source".into()))?;
        let (current, flags, _) = reporting(session, slot, index, cid, deadline).map_err(fail)?;
        if flags != 0 || c["value"] != current {
            return Err(fail(
                "Reporting changed since preflight; no setter sent".into(),
            ));
        }
        let from = cid.to_be_bytes();
        let to = (target.as_u64().unwrap() as u16).to_be_bytes();
        let params = [from[0], from[1], 0, to[0], to[1], 0];
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| fail("Button write budget elapsed".into()))?;
        let ack = session
            .request(
                ReportKind::Long,
                slot,
                index,
                3,
                &params,
                remaining.min(Duration::from_secs(2)),
            )
            .map_err(|e| fail(e.to_string()))?;
        if ack.payload().get(..params.len()) != Some(params.as_slice()) {
            return Err(fail(
                "Native remap ACK did not echo the fixed request".into(),
            ));
        }
        snapshot = describe_session(session, slot, id, deadline).map_err(fail)?;
        if control(&snapshot, source).map_err(fail)?["value"] != *target {
            return Err(fail("Native mapping was not confirmed by hardware".into()));
        }
        for previous in &confirmed {
            let previous = previous.strip_prefix("button:").unwrap();
            if control(&snapshot, previous).map_err(fail)?["value"] != changes[previous] {
                return Err(fail(format!(
                    "Earlier confirmed button changed: {previous}"
                )));
            }
        }
        confirmed.push(format!("button:{source}"));
    }
    Ok(
        json!({"stage":"N3","read_only":false,"experimental":true,"mode":"native-button-remap","device":id,"confirmed":confirmed,"snapshot":snapshot}),
    )
}
