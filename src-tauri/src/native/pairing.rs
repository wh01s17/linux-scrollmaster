// SPDX-License-Identifier: GPL-3.0-or-later
use crate::hidpp::{Session, SessionError, Transport};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// Reads receiver pairing metadata. Does not probe/pair/unpair any device.
pub fn paired_devices<T: Transport>(transport: T, deadline: Instant) -> Result<Value, String> {
    let mut session = Session::new(transport);
    let mut slots = Vec::new();
    for slot in 1..=6 {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("Pairing metadata deadline elapsed")?;
        match session.bolt_pairing_info(slot, remaining.min(Duration::from_secs(2))) {
            Ok(report) => {
                let p = report.payload();
                if p.len() < 8 {
                    return Err("Truncated Bolt pairing metadata".into());
                }
                let model = format!("{:02X}{:02X}", p[3], p[2]);
                slots.push(json!({"slot":slot, "model":model, "kind_code":p[1]&15, "supported_model":model=="B034"}));
            }
            Err(SessionError::Protocol(error))
                if error.marker == 0x8f && matches!(error.code, 2 | 8) => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(json!({"read_only":true,"mode":"receiver-pairing-metadata","slots":slots}))
}
