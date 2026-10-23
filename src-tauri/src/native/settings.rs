// SPDX-License-Identifier: GPL-3.0-or-later
//! N3 scalar settings only. No profiles, button diversion or generic dispatcher.
use super::diagnostic::diagnose_session;
use crate::hidpp::{ReportKind, Session, Transport};
use serde_json::{json, Map, Value};
use std::{
    fmt,
    time::{Duration, Instant},
};

const ORDER: &[&str] = &[
    "scroll-ratchet",
    "smart-shift",
    "dpi",
    "hires-smooth-invert",
    "hires-smooth-resolution",
    "thumb-scroll-invert",
];
#[derive(Debug)]
pub struct ApplyError {
    pub message: String,
    pub confirmed: Vec<String>,
    /// May have reached hardware; never automatically resent or claimed rolled back.
    pub attempted: Option<String>,
}
impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; confirmed: {:?}; attempted: {:?}",
            self.message, self.confirmed, self.attempted
        )
    }
}
impl std::error::Error for ApplyError {}
impl ApplyError {
    pub fn describe(&self) -> Value {
        json!({"ok":false,"error":self.message,"confirmed":self.confirmed,"attempted":self.attempted})
    }
}
fn error(message: impl ToString) -> ApplyError {
    ApplyError {
        message: message.to_string(),
        confirmed: vec![],
        attempted: None,
    }
}

/// Pure structural validation runs before any hardware access. Dynamic limits are
/// checked from a fresh diagnostic before the entire batch is allowed to write.
pub fn validate_changes(changes: &Value) -> Result<(), String> {
    let changes = changes
        .as_object()
        .filter(|c| !c.is_empty() && c.len() <= 6)
        .ok_or("Expected a nonempty settings object (maximum six keys)")?;
    for (key, value) in changes {
        let valid = match key.as_str() {
            "dpi" => value.as_u64().is_some_and(|v| (1..=65535).contains(&v)),
            "smart-shift" => value.as_u64().is_some_and(|v| (1..=50).contains(&v)),
            "scroll-ratchet" => matches!(value.as_u64(), Some(1 | 2)),
            "hires-smooth-invert" | "hires-smooth-resolution" | "thumb-scroll-invert" => {
                value.is_boolean()
            }
            _ => false,
        };
        if !valid {
            return Err(format!("Invalid or unsupported setting: {key}"));
        }
    }
    Ok(())
}

fn validate_capabilities(snapshot: &Value, changes: &Map<String, Value>) -> Result<(), String> {
    let settings = &snapshot["device"]["settings"];
    for (key, value) in changes {
        if settings[key]["available"] != true {
            return Err(format!("Setting is unavailable: {key}"));
        }
        if key == "dpi"
            && !settings[key]["choices"]
                .as_array()
                .is_some_and(|choices| choices.contains(value))
        {
            return Err("DPI not in the device's advertised choices".into());
        }
    }
    if changes.contains_key("smart-shift") {
        let mode = changes
            .get("scroll-ratchet")
            .unwrap_or(&settings["scroll-ratchet"]["value"]);
        if mode != 2 {
            return Err("Select ratcheted mode before setting a SmartShift threshold".into());
        }
    }
    Ok(())
}
fn feature(snapshot: &Value, id: &str) -> Result<u8, String> {
    snapshot["features"]
        .as_array()
        .and_then(|features| features.iter().find(|f| f["id"] == id))
        .filter(|f| f["flags"] == 0 && f["version"].as_u64().is_some_and(|v| v <= 4))
        .and_then(|f| f["index"].as_u64())
        .and_then(|index| u8::try_from(index).ok())
        .ok_or_else(|| format!("Feature unavailable for writing: {id}"))
}
fn raw(snapshot: &Value, key: &str) -> Result<Vec<u8>, String> {
    let text = snapshot["device"]["settings"][key]["raw"]
        .as_str()
        .ok_or("No fresh raw value for preserving hardware flags")?;
    if text.len() % 2 != 0 {
        return Err("Invalid raw value".into());
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).map_err(|_| "Invalid raw value".into()))
        .collect()
}
fn encode(snapshot: &Value, key: &str, value: &Value) -> Result<(u8, u8, Vec<u8>), String> {
    match key {
        "dpi" => {
            let dpi = value.as_u64().ok_or("Invalid DPI")? as u16;
            let bytes = dpi.to_be_bytes();
            Ok((feature(snapshot, "2201")?, 3, vec![0, bytes[0], bytes[1]]))
        }
        "smart-shift" | "scroll-ratchet" => {
            let enhanced = snapshot["features"]
                .as_array()
                .is_some_and(|features| features.iter().any(|f| f["id"] == "2111"));
            let index = feature(snapshot, if enhanced { "2111" } else { "2110" })?;
            let value = value.as_u64().ok_or("Invalid wheel value")? as u8;
            let parameters = if key == "scroll-ratchet" {
                vec![value]
            } else {
                vec![0, if value == 50 { 255 } else { value }]
            };
            Ok((index, if enhanced { 2 } else { 1 }, parameters))
        }
        "hires-smooth-invert" | "hires-smooth-resolution" => {
            let p = raw(snapshot, key)?;
            let current = *p.first().ok_or("Missing wheel mode")?;
            let bit = if key == "hires-smooth-invert" { 4 } else { 2 };
            let next = (current & !bit) | if value == true { bit } else { 0 };
            Ok((feature(snapshot, "2121")?, 2, vec![next]))
        }
        "thumb-scroll-invert" => {
            let p = raw(snapshot, key)?;
            if p.len() < 2 {
                return Err("Missing thumb wheel flags".into());
            }
            Ok((
                feature(snapshot, "2150")?,
                2,
                vec![p[0], (p[1] & !1) | u8::from(value == true)],
            ))
        }
        _ => Err("Unknown scalar setting".into()),
    }
}

/// Validate the complete scalar batch against the current diagnostic, including
/// encoders, without sending setters. Used by the combined profile preflight.
pub(super) fn prepare_snapshot(snapshot: &Value, changes: &Value) -> Result<(), String> {
    validate_changes(changes)?;
    let changes = changes.as_object().unwrap();
    validate_capabilities(snapshot, changes)?;
    for (key, value) in changes {
        encode(snapshot, key, value)?;
    }
    Ok(())
}

/// Applies only the explicit batch to the expected stable identity. Values and
/// capabilities are read uncached before writing and after every acknowledged set.
/// A failure reports earlier confirmed changes and the uncertain attempted key.
pub fn apply_changes<T: Transport>(
    transport: T,
    slot: u8,
    expected_device: &str,
    changes: &Value,
    deadline: Instant,
) -> Result<Value, ApplyError> {
    apply_changes_session(
        &mut Session::new(transport),
        slot,
        expected_device,
        changes,
        deadline,
    )
}

/// Retain the same session and its bounded notification backlog across writes.
pub(super) fn apply_changes_session<T: Transport>(
    session: &mut Session<T>,
    slot: u8,
    expected_device: &str,
    changes: &Value,
    deadline: Instant,
) -> Result<Value, ApplyError> {
    validate_changes(changes).map_err(error)?;
    if expected_device.len() != 8
        || !expected_device
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_lowercase())
    {
        return Err(error("Expected an uppercase stable Unit ID, not a slot"));
    }
    let changes = changes
        .as_object()
        .ok_or_else(|| error("Invalid settings object"))?;
    let mut snapshot = diagnose_session(session, slot, deadline).map_err(error)?;
    if snapshot["device"]["id"] != expected_device {
        return Err(error(
            "Device identity differs from the selected Unit ID; nothing written",
        ));
    }
    prepare_snapshot(&snapshot, &Value::Object(changes.clone())).map_err(error)?;
    let mut confirmed: Vec<String> = Vec::new();
    for &key in ORDER {
        let Some(value) = changes.get(key) else {
            continue;
        };
        if snapshot["device"]["settings"][key]["value"] == *value {
            continue;
        }
        let context = |message: String| ApplyError {
            message,
            confirmed: confirmed.clone(),
            attempted: Some(key.into()),
        };
        let (index, function, parameters) = encode(&snapshot, key, value).map_err(context)?;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| context("Write budget elapsed".into()))?;
        session
            .request(
                ReportKind::Long,
                slot,
                index,
                function,
                &parameters,
                remaining.min(Duration::from_secs(2)),
            )
            .map_err(|e| context(e.to_string()))?;
        snapshot = diagnose_session(session, slot, deadline).map_err(context)?;
        if snapshot["device"]["id"] != expected_device {
            return Err(context("Identity changed during confirmation".into()));
        }
        if snapshot["device"]["settings"][key]["available"] != true
            || snapshot["device"]["settings"][key]["value"] != *value
        {
            return Err(context(format!("Hardware did not confirm setting: {key}")));
        }
        for previous in &confirmed {
            if snapshot["device"]["settings"][previous]["available"] != true
                || snapshot["device"]["settings"][previous]["value"] != changes[previous]
            {
                return Err(context(format!(
                    "Earlier confirmed setting changed: {previous}"
                )));
            }
        }
        confirmed.push(key.into());
    }
    Ok(
        json!({"stage":"N3","read_only":false,"experimental":true,"device":expected_device,"confirmed":confirmed,"snapshot":snapshot}),
    )
}
