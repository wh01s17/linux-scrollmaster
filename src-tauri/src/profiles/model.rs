// SPDX-License-Identifier: GPL-3.0-or-later
use crate::native::ActionBindings;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

pub const MAX_DOCUMENT_BYTES: usize = 128 * 1024;
pub const MAX_PROFILES: usize = 100;

/// Validated schemas 1–10. Unknown document/profile metadata is retained.
/// Validation is structural: hardware capabilities must be checked before apply.
#[derive(Clone, Debug, PartialEq)]
pub struct Document(pub(super) Value);
impl Default for Document {
    fn default() -> Self {
        Self(json!({"version":1,"profiles":[],"active":{}}))
    }
}
impl Document {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_DOCUMENT_BYTES {
            return Err("Profile document exceeds 128 KiB".into());
        }
        let value = serde_json::from_slice(bytes)
            .map_err(|_| "Invalid profile JSON; original preserved")?;
        Self::from_value(value)
    }
    pub fn from_value(value: Value) -> Result<Self, String> {
        validate_document(&value)?;
        if !bounded_depth(&value, 0) {
            return Err("Profile JSON nesting exceeds the parser limit".into());
        }
        let bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_DOCUMENT_BYTES {
            return Err("Profile document exceeds 128 KiB".into());
        }
        // Rust-created metadata must remain readable after persistence, within
        // serde_json's normal recursion bound.
        serde_json::from_slice::<Value>(&bytes)
            .map_err(|_| "Profile JSON nesting exceeds the parser limit")?;
        Ok(Self(value))
    }
    pub fn as_value(&self) -> &Value {
        &self.0
    }
    pub fn version(&self) -> u64 {
        self.0["version"].as_u64().unwrap()
    }
    pub fn profiles(&self) -> &[Value] {
        self.0["profiles"].as_array().unwrap()
    }
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        let bytes = serde_json::to_vec_pretty(&self.0).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_DOCUMENT_BYTES {
            return Err("Encoded profiles exceed 128 KiB".into());
        }
        Ok(bytes)
    }
    /// Pure selection. Focus rules are effective only while a general profile
    /// is explicitly armed. None/unavailable focus selects the general profile.
    pub fn effective_profile(&self, device: &str, application: Option<&str>) -> Option<&Value> {
        let base = self.0["active"].get(device)?.as_str()?;
        let chosen = application
            .filter(|app| valid_application(app))
            .and_then(|app| {
                self.0
                    .get("rules")
                    .and_then(Value::as_array)?
                    .iter()
                    .find(|r| r["device"] == device && r["application"] == app)?["profile"]
                    .as_str()
            })
            .unwrap_or(base);
        self.profiles()
            .iter()
            .find(|p| p["device"] == device && p["name"] == chosen)
    }
}
fn bounded_depth(value: &Value, depth: usize) -> bool {
    match value {
        Value::Array(values) => depth < 128 && values.iter().all(|v| bounded_depth(v, depth + 1)),
        Value::Object(values) => {
            depth < 128 && values.values().all(|v| bounded_depth(v, depth + 1))
        }
        _ => true,
    }
}
pub(super) fn py_trim(value: &str) -> &str {
    value.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}
pub(super) fn valid_device(value: &str) -> bool {
    (1..=128).contains(&value.chars().count())
}
pub(super) fn normalized_name(value: &Value) -> Result<&str, String> {
    let name = value.as_str().map(py_trim).ok_or("Invalid profile name")?;
    if !(1..=60).contains(&name.chars().count()) {
        return Err("Profile names require 1–60 characters".into());
    }
    Ok(name)
}
pub(crate) fn valid_application(value: &str) -> bool {
    (1..=256).contains(&value.chars().count()) && value.chars().all(|c| c >= ' ')
}
fn validate_settings(value: &Value) -> Result<(), String> {
    let values = value
        .as_object()
        .filter(|o| !o.is_empty())
        .ok_or("Missing profile settings")?;
    for (key, value) in values {
        let valid = match key.as_str() {
            "dpi" => value.as_u64().is_some_and(|n| (1..=65535).contains(&n)),
            "smart-shift" => value.as_u64().is_some_and(|n| (1..=50).contains(&n)),
            "scroll-ratchet" => value.as_u64().is_some_and(|n| [1, 2].contains(&n)),
            "hires-smooth-invert" | "hires-smooth-resolution" | "thumb-scroll-invert" => {
                value.is_boolean()
            }
            _ => false,
        };
        if !valid {
            return Err(format!("Invalid saved setting: {key}"));
        }
    }
    Ok(())
}
fn action_schema(action: &str, gesture: bool) -> u64 {
    if action.starts_with("desktop:") {
        10
    } else if action.starts_with("custom:") {
        9
    } else if gesture && action.starts_with("media:") {
        7
    } else if !gesture && action.starts_with("shortcut:") {
        4
    } else {
        3
    }
}
/// Mirror the persisted format, which excludes media on side buttons even
/// though the experimental action engine supports them. No I/O in this parser.
pub(crate) fn button_schema(value: &Value) -> Result<u64, String> {
    let buttons = value
        .as_object()
        .filter(|o| !o.is_empty())
        .ok_or("Invalid profile buttons")?;
    let mut software = Map::new();
    let mut minimum = 2;
    for (cid, target) in buttons {
        if !["83", "86", "195"].contains(&cid.as_str()) {
            return Err("Unsupported profile control".into());
        }
        if target.as_u64().is_some_and(|n| (1..=65535).contains(&n)) {
            continue;
        }
        if let Some(action) = target.as_str() {
            if cid == "195" || action.starts_with("media:") {
                return Err("Invalid side button action".into());
            }
            minimum = minimum.max(action_schema(action, false));
        } else if cid == "195" && target.is_object() {
            minimum = minimum.max(if target.get("units").is_some() { 8 } else { 5 });
            if let Some(actions) = target["actions"].as_object() {
                for action in actions.values().filter_map(Value::as_str) {
                    minimum = minimum.max(action_schema(action, true));
                }
            }
        } else {
            return Err("Invalid profile button mapping".into());
        }
        software.insert(cid.clone(), target.clone());
    }
    ActionBindings::parse(&Value::Object(software))?;
    Ok(minimum)
}
fn validate_document(value: &Value) -> Result<(), String> {
    let version = value["version"]
        .as_u64()
        .filter(|v| (1..=10).contains(v))
        .ok_or("Unsupported profile schema")?;
    let profiles = value["profiles"]
        .as_array()
        .filter(|p| p.len() <= MAX_PROFILES)
        .ok_or("Invalid profile list or too many profiles")?;
    let active = value["active"]
        .as_object()
        .ok_or("Invalid active profile map")?;
    let mut identities = BTreeSet::new();
    for profile in profiles {
        let device = profile["device"]
            .as_str()
            .filter(|s| valid_device(s))
            .ok_or("Invalid saved device identity")?;
        normalized_name(&profile["name"])?;
        let name = profile["name"].as_str().unwrap(); // Preserve exact legacy whitespace.
        if !identities.insert((device, name)) {
            return Err("Duplicate saved profile".into());
        }
        validate_settings(&profile["settings"])?;
        if let Some(buttons) = profile.get("buttons") {
            if version < button_schema(buttons)? {
                return Err("Buttons require a newer profile schema".into());
            }
        }
    }
    for (device, name) in active {
        if !name
            .as_str()
            .is_some_and(|name| identities.contains(&(device.as_str(), name)))
        {
            return Err("Active profile does not exist for this device".into());
        }
    }
    if let Some(rules) = value.get("rules") {
        let rules = rules
            .as_array()
            .filter(|r| r.len() <= 100 && (r.is_empty() || version >= 6))
            .ok_or("Invalid application rules or schema")?;
        let mut used = BTreeSet::new();
        for rule in rules {
            let object = rule.as_object().ok_or("Invalid application rule")?;
            if object.len() != 3
                || object
                    .keys()
                    .any(|k| !["device", "application", "profile"].contains(&k.as_str()))
            {
                return Err("Invalid application rule fields".into());
            }
            let device = rule["device"].as_str().ok_or("Invalid rule device")?;
            let profile = rule["profile"].as_str().ok_or("Invalid rule profile")?;
            let application = rule["application"]
                .as_str()
                .filter(|s| valid_application(s))
                .ok_or("Invalid application identity")?;
            if !identities.contains(&(device, profile)) || !used.insert((device, application)) {
                return Err("Duplicate rule or missing profile reference".into());
            }
        }
    }
    Ok(())
}
