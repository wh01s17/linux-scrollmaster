// SPDX-License-Identifier: GPL-3.0-or-later
//! Native worker replies -> the Vue `Snapshot` contract kept from the legacy
//! service. Pure data mapping: no HID, profile, compositor or process access.
use serde_json::{json, Map, Value};

const SHORTCUTS: [(&str, &str); 4] = [
    ("shortcut:copy", "Copiar (Ctrl+C)"),
    ("shortcut:paste", "Pegar (Ctrl+V)"),
    ("shortcut:cut", "Cortar (Ctrl+X)"),
    ("shortcut:undo", "Deshacer (Ctrl+Z)"),
];
const MEDIA: [(&str, &str); 6] = [
    ("media:play-pause", "Reproducir / Pausar"),
    ("media:previous", "Pista anterior"),
    ("media:next", "Pista siguiente"),
    ("media:volume-up", "Subir volumen"),
    ("media:volume-down", "Bajar volumen"),
    ("media:mute", "Silenciar / Activar sonido"),
];
const DESKTOP: [(&str, &str); 4] = [
    ("desktop:workspace-previous", "Espacio anterior · Hyprland"),
    ("desktop:workspace-next", "Espacio siguiente · Hyprland"),
    (
        "desktop:fullscreen",
        "Alternar pantalla completa · Hyprland",
    ),
    ("desktop:maximize", "Alternar maximizado · Hyprland"),
];
const MODIFIERS: [(&str, &str); 4] = [
    ("ctrl", "Ctrl"),
    ("alt", "Alt"),
    ("shift", "Shift"),
    ("super", "Super"),
];
const NAMED_KEYS: [(&str, &str); 15] = [
    ("Escape", "Escape"),
    ("Tab", "Tab"),
    ("Enter", "Enter"),
    ("Space", "Espacio"),
    ("Backspace", "Retroceso"),
    ("Delete", "Suprimir"),
    ("Insert", "Insertar"),
    ("Home", "Inicio"),
    ("End", "Fin"),
    ("PageUp", "Página arriba"),
    ("PageDown", "Página abajo"),
    ("ArrowUp", "↑"),
    ("ArrowDown", "↓"),
    ("ArrowLeft", "←"),
    ("ArrowRight", "→"),
];

/// Same order as the legacy catalogue: letters, digits 1..0, F1..F12, named keys.
fn keys() -> Vec<(String, String)> {
    let mut keys: Vec<_> = ('A'..='Z')
        .map(|c| (format!("Key{c}"), c.to_string()))
        .collect();
    keys.extend(
        "1234567890"
            .chars()
            .map(|d| (format!("Digit{d}"), d.to_string())),
    );
    keys.extend((1..=12).map(|i| (format!("F{i}"), format!("F{i}"))));
    keys.extend(
        NAMED_KEYS
            .iter()
            .map(|(k, l)| (k.to_string(), l.to_string())),
    );
    keys
}
fn catalogue() -> Value {
    json!({"keys":keys().into_iter().map(|(value,label)|json!({"value":value,"label":label})).collect::<Vec<_>>(),
        "modifiers":MODIFIERS.iter().map(|(value,label)|json!({"value":value,"label":label})).collect::<Vec<_>>()})
}
/// Label for the canonical `custom:<ordered modifiers>:<named key>` form only.
fn custom_label(action: &str) -> Option<String> {
    let parts: Vec<_> = action.split(':').collect();
    let [prefix, modifiers, key] = parts.as_slice() else {
        return None;
    };
    let key = keys().into_iter().find(|(k, _)| k == key)?.1;
    let listed: Vec<_> = if modifiers.is_empty() {
        vec![]
    } else {
        modifiers.split('+').collect()
    };
    let ordered: Vec<_> = MODIFIERS
        .iter()
        .filter(|(m, _)| listed.contains(m))
        .collect();
    if *prefix != "custom" || ordered.len() != listed.len() || action.len() > 80 {
        return None;
    }
    if ordered.iter().zip(&listed).any(|((m, _), l)| m != l) {
        return None;
    }
    let mut labels: Vec<_> = ordered.iter().map(|(_, l)| l.to_string()).collect();
    labels.push(key);
    Some(labels.join(" + "))
}
fn option(value: &str, label: &str, desktop: &Value) -> Value {
    if value.starts_with("desktop:") {
        json!({"value":value,"label":label,"disabled":desktop["available"] != true})
    } else {
        json!({"value":value,"label":label})
    }
}
fn control(native: &Value, desktop: &Value) -> Value {
    let id = native["id"].as_str().unwrap_or_default();
    let flags = native["capabilities"]["flags"].as_u64().unwrap_or(0);
    let divertable = flags & 0x31 == 0x31 && flags & 0x80 == 0;
    let side = id == "83" || id == "86";
    let gestures = id == "195" && divertable && flags & 0x100 != 0;
    let mut choices: Vec<Value> = native["choices"].as_array().cloned().unwrap_or_default();
    if side && divertable {
        choices.push(json!({"value":"modifier:alt","label":"Alt (mantener)"}));
        choices.push(json!({"value":"modifier:ctrl","label":"Ctrl (mantener)"}));
        choices.extend(SHORTCUTS.iter().map(|(v, l)| option(v, l, desktop)));
        choices.extend(DESKTOP.iter().map(|(v, l)| option(v, l, desktop)));
    }
    let value = native["value"].clone();
    if let Some(label) = value.as_str().and_then(custom_label) {
        choices.push(json!({"value":value,"label":label}));
    }
    let mut item = json!({"id":id,"label":native["label"],"value":value,
        "available":native["available"] == true,"choices":choices,
        "gestures_available":gestures,"desktop_error":desktop["error"],
        "gesture_actions":if gestures {
            SHORTCUTS.iter().chain(&MEDIA).chain(&DESKTOP).map(|(v,l)|option(v,l,desktop)).collect::<Vec<_>>()
        } else { vec![] }});
    if divertable && (side || gestures) {
        item["shortcuts"] = catalogue();
    }
    if native["available"] != true {
        item["error"] = if native["flags"].as_u64().is_some_and(|f| f != 0) {
            json!("Este botón está interceptado. No se modificará.")
        } else {
            native["error"].clone()
        };
    }
    item
}
/// HID++ 1004 charging status byte: 0 discharging, 1 charging, 2 slow charging,
/// 3 charge complete. Anything else (errors, 1000 devices) stays unknown.
fn battery_status(battery: &Value) -> Value {
    if battery["feature"] != "1004" {
        return Value::Null;
    }
    match battery["status"].as_u64() {
        Some(0) => json!("discharging"),
        Some(1 | 2) => json!("charging"),
        Some(3) => json!("full"),
        _ => Value::Null,
    }
}
/// Map one native device description. Evidence fields are retained unchanged.
pub fn device(native: &Value, desktop: &Value) -> Value {
    let protocol = &native["protocol"];
    let protocol = match (protocol["major"].as_u64(), protocol["minor"].as_u64()) {
        (Some(major), Some(minor)) => json!(major as f64 + minor as f64 / 10.0),
        _ => protocol.clone(),
    };
    let settings: Map<String, Value> = native["settings"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, s)| {
            let mut item = json!({"value":s["value"],"available":s["available"] == true});
            if s.get("choices").is_some() {
                item["choices"] = s["choices"].clone();
            }
            if s.get("error").is_some_and(|e| !e.is_null()) {
                item["error"] = s["error"].clone();
            }
            (name.clone(), item)
        })
        .collect();
    let controls: Vec<_> = native["buttons"]["controls"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| control(c, desktop))
        .collect();
    let mut result = native.clone();
    result["protocol"] = protocol;
    result["battery"] = native["battery"]["percentage"].clone();
    result["battery_status"] = battery_status(&native["battery"]);
    result["settings"] = Value::Object(settings);
    result["buttons"] = json!({"available":!controls.is_empty(),"controls":controls,
        "error":native["buttons"]["error"],"gesture_feedback":null});
    result
}
const UNREACHABLE: &str =
    "El mouse no responde: está apagado, en reposo o fuera de alcance. Se reintentará automáticamente.";
/// The receiver answers HID++ 1.0 error 4 (connection request failed) for a
/// paired mouse whose link is down, observed when it is off or after resume.
fn unreachable_mouse(warning: &str) -> bool {
    warning.contains("ErrorReply { marker: 143,") && warning.contains(" code: 4 }")
}
/// Fill the legacy snapshot fields the UI reads (devices, diagnostic, warnings).
pub fn snapshot(data: &mut Value, desktop: &Value, diagnostic: &str) {
    let devices: Vec<_> = data["devices"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| device(d, desktop))
        .collect();
    data["diagnostic"] = if devices.is_empty() {
        json!(diagnostic)
    } else {
        Value::Null
    };
    data["devices"] = json!(devices);
    for key in ["warnings", "profiles", "rules"] {
        if !data[key].is_array() {
            data[key] = json!([]);
        }
    }
    for warning in data["warnings"].as_array_mut().into_iter().flatten() {
        if warning.as_str().is_some_and(unreachable_mouse) {
            *warning = json!(UNREACHABLE);
        }
    }
    if !data["active"].is_object() {
        data["active"] = json!({});
    }
}

#[cfg(test)]
#[path = "presentation_tests.rs"]
mod tests;
