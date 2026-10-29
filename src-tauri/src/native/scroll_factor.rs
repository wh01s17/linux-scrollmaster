// SPDX-License-Identifier: GPL-3.0-or-later
//! Scroll distance for the MX Master through Hyprland's per-device `scroll_factor`.
//! The mouse has no HID++ control for distance per notch; this is a runtime desktop
//! setting. Never edits Hyprland files and only touches the MX Master device entries.
use super::desktop::{environment_path, exchange_until, require_ok, supported_version};
use serde_json::Value;
use std::{
    path::Path,
    time::{Duration, Instant},
};

pub const MIN: f64 = 0.1;
pub const MAX: f64 = 5.0;
const TIMEOUT: Duration = Duration::from_secs(1);
const PROBE: &[u8] = b"/eval return assert(type(hl.device) == \"function\")";
/// Hyprland names the Bolt receiver's pointer after the receiver, Bluetooth after the mouse.
const RECEIVER: &str = "logitech-usb-receiver-mouse";
const BLUETOOTH: &str = "mx-master-3s";

#[derive(Clone, Debug, PartialEq)]
pub struct ScrollDevice {
    pub name: String,
    /// Hyprland reports -1 when the device has no own factor.
    pub factor: f64,
}

/// Two decimals inside the supported range. `false`/0 would stop scrolling: never sent.
pub fn normalize(value: f64) -> Result<f64, String> {
    if !value.is_finite() || !(MIN..=MAX).contains(&value) {
        return Err(format!(
            "La velocidad de desplazamiento debe estar entre {MIN} y {MAX}"
        ));
    }
    Ok((value * 100.0).round() / 100.0)
}
pub fn same(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.005
}

fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
}
fn targets(devices: &Value) -> Result<Vec<ScrollDevice>, String> {
    let mice = devices["mice"]
        .as_array()
        .ok_or("Respuesta de dispositivos de Hyprland no válida")?;
    Ok(mice
        .iter()
        .filter_map(|mouse| {
            let name = mouse["name"].as_str()?;
            let ours = name == RECEIVER || name.contains(BLUETOOTH);
            (ours && safe_name(name)).then(|| ScrollDevice {
                name: name.to_string(),
                factor: mouse["scrollFactor"].as_f64().unwrap_or(-1.0),
            })
        })
        .collect())
}
fn payload(name: &str, factor: f64) -> Vec<u8> {
    // `name` passed safe_name: no quotes, backslashes or Lua syntax.
    format!("/eval hl.device({{ name = \"{name}\", scroll_factor = {factor:.2} }})").into_bytes()
}
fn request(path: &Path, payload: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    exchange_until(path, payload, limit, Instant::now() + TIMEOUT)
}
fn preflight(path: &Path) -> Result<(), String> {
    let version: Value = serde_json::from_slice(&request(path, b"j/version", 1 << 20)?)
        .map_err(|_| "Respuesta de versión de Hyprland no válida")?;
    if !version["version"].as_str().is_some_and(supported_version) {
        return Err("La velocidad de desplazamiento requiere Hyprland 0.55, 0.56 o 0.57".into());
    }
    require_ok(&request(path, PROBE, 4096)?)
}
fn read_at(path: &Path) -> Result<Vec<ScrollDevice>, String> {
    preflight(path)?;
    let devices: Value = serde_json::from_slice(&request(path, b"j/devices", 1 << 20)?)
        .map_err(|_| "Respuesta de dispositivos de Hyprland no válida")?;
    targets(&devices)
}
fn global_at(path: &Path) -> Result<f64, String> {
    let reply: Value =
        serde_json::from_slice(&request(path, b"j/getoption input:scroll_factor", 4096)?)
            .map_err(|_| "Respuesta de opción de Hyprland no válida")?;
    reply["float"]
        .as_f64()
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or_else(|| "Hyprland no informó su velocidad global".into())
}
fn apply_at(path: &Path, factor: f64) -> Result<Vec<ScrollDevice>, String> {
    let devices = read_at(path)?;
    if devices.is_empty() {
        return Err("Hyprland no muestra el MX Master como dispositivo".into());
    }
    for device in &devices {
        require_ok(&request(path, &payload(&device.name, factor), 4096)?)?;
    }
    // Confirm by a fresh read, never by the request reply.
    let confirmed = read_at(path)?;
    if confirmed.is_empty() || confirmed.iter().any(|d| !same(d.factor, factor)) {
        return Err("Hyprland no confirmó la velocidad de desplazamiento".into());
    }
    Ok(confirmed)
}

/// MX Master entries in this session; empty when Hyprland does not list it.
pub fn read() -> Result<Vec<ScrollDevice>, String> {
    read_at(&environment_path()?)
}
pub fn apply(factor: f64) -> Result<Vec<ScrollDevice>, String> {
    apply_at(&environment_path()?, normalize(factor)?)
}
/// Back to the session's global `input:scroll_factor`.
pub fn reset() -> Result<Vec<ScrollDevice>, String> {
    let path = environment_path()?;
    let global = global_at(&path)?;
    apply_at(&path, (global * 100.0).round() / 100.0)
}

#[cfg(test)]
#[path = "scroll_factor_tests.rs"]
mod tests;
