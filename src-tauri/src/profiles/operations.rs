// SPDX-License-Identifier: GPL-3.0-or-later
use super::{
    model::{normalized_name, valid_application, valid_device},
    Document, MAX_DOCUMENT_BYTES, MAX_PROFILES,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Clone, Debug)]
pub struct Management {
    pub document: Document,
    pub transfer: Option<Value>,
    pub changed: bool,
}
impl Document {
    /// Save only a complete, current description of the selected identity.
    /// This pure transformation never arms a profile or writes hardware.
    pub fn save_snapshot(
        &self,
        device: &str,
        name: &Value,
        snapshot: &Value,
    ) -> Result<Self, String> {
        if !valid_device(device) || snapshot["id"] != device {
            return Err("Snapshot identity differs from the selected device".into());
        }
        let name = normalized_name(name)?;
        let keys = [
            "dpi",
            "smart-shift",
            "scroll-ratchet",
            "hires-smooth-invert",
            "hires-smooth-resolution",
            "thumb-scroll-invert",
        ];
        let mut settings = serde_json::Map::new();
        for key in keys {
            let setting = &snapshot["settings"][key];
            if setting["available"] != true || setting.get("value").is_none() {
                return Err(format!(
                    "Complete current setting required before saving: {key}"
                ));
            }
            settings.insert(key.to_owned(), setting["value"].clone());
        }
        let old = self
            .profiles()
            .iter()
            .find(|p| p["device"] == device && p["name"] == name);
        let mut profile = old
            .cloned()
            .unwrap_or_else(|| json!({"device":device,"name":name}));
        profile["settings"] = Value::Object(settings);
        let mut version = self.version();
        if let Some(buttons) = snapshot.get("buttons") {
            if buttons.get("error").is_some_and(|error| !error.is_null()) {
                return Err("Button snapshot is incomplete".into());
            }
            let controls = buttons["controls"]
                .as_array()
                .filter(|c| !c.is_empty())
                .ok_or("Complete button catalogue required before saving")?;
            let mut values = serde_json::Map::new();
            for control in controls {
                let cid = control["id"]
                    .as_str()
                    .ok_or("Invalid saved control identity")?;
                if control["available"] != true
                    || control.get("value").is_none()
                    || values
                        .insert(cid.to_owned(), control["value"].clone())
                        .is_some()
                {
                    return Err("Button reading is unavailable or duplicated".into());
                }
            }
            // Do not drop a previously stored control if a partial catalogue
            // happens to be returned without an explicit error.
            if old
                .and_then(|p| p.get("buttons"))
                .and_then(Value::as_object)
                .is_some_and(|previous| previous.keys().any(|k| !values.contains_key(k)))
            {
                return Err("Saved button mapping is missing from the current catalogue".into());
            }
            let buttons = Value::Object(values);
            version = version.max(super::model::button_schema(&buttons)?);
            profile["buttons"] = buttons;
        } else if profile.get("buttons").is_some() {
            return Err("Cannot replace saved button mappings without a current reading".into());
        }
        let mut data = self.0.clone();
        let profiles = data["profiles"].as_array_mut().unwrap();
        profiles.retain(|p| p["device"] != device || p["name"] != name);
        profiles.push(profile);
        data["version"] = json!(version);
        Self::from_value(data)
    }
    /// A pure plan. Persist successfully before a future service acts on it.
    /// No request here can apply settings, enable capture or query hardware.
    pub fn manage(&self, request: &Value) -> Result<Management, String> {
        let device = request["device"]
            .as_str()
            .filter(|s| valid_device(s))
            .ok_or("Select a saved device identity")?;
        let op = request["op"]
            .as_str()
            .filter(|op| {
                [
                    "rename",
                    "delete",
                    "export",
                    "import_preview",
                    "import",
                    "rule_set",
                    "rule_delete",
                    "auto",
                ]
                .contains(op)
            })
            .ok_or("Local profile operation not permitted")?;
        let mut result = self.0.clone();
        let mut transfer = None;
        let mut changed = true;
        match op {
            "auto" => match request.get("name") {
                None | Some(Value::Null) => {
                    result["active"].as_object_mut().unwrap().remove(device);
                }
                Some(name)
                    if self
                        .profiles()
                        .iter()
                        .any(|p| p["device"] == device && p["name"] == *name) =>
                {
                    result["active"][device] = name.clone();
                }
                _ => return Err("Save a profile for this device before arming it".into()),
            },
            "rule_set" | "rule_delete" => {
                let app = request["application"]
                    .as_str()
                    .filter(|s| valid_application(s))
                    .ok_or("Select a valid application")?;
                let mut rules = self
                    .0
                    .get("rules")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                rules.retain(|r| r["device"] != device || r["application"] != app);
                if op == "rule_set" {
                    let name = request["name"]
                        .as_str()
                        .filter(|name| {
                            self.profiles()
                                .iter()
                                .any(|p| p["device"] == device && p["name"] == *name)
                        })
                        .ok_or("Select a saved profile for the application")?;
                    rules.push(json!({"device":device,"application":app,"profile":name}));
                }
                result["rules"] = json!(rules);
                result["version"] = json!(self.version().max(6));
            }
            "import_preview" | "import" => {
                let source = request
                    .get("document")
                    .filter(|v| v.is_object())
                    .ok_or("Invalid import document")?;
                if serde_json::to_vec(source).map_err(|e| e.to_string())?.len() > MAX_DOCUMENT_BYTES
                {
                    return Err("Import document exceeds 128 KiB".into());
                }
                // Never trust activation, rules or other metadata from imports.
                let incoming = Document::from_value(
                    json!({"version":source["version"],"profiles":source["profiles"],"active":{}}),
                )?;
                if incoming.profiles().is_empty()
                    || self.profiles().len() + incoming.profiles().len() > MAX_PROFILES
                {
                    return Err("Import is empty or exceeds the 100 profile limit".into());
                }
                let mut used: BTreeSet<String> = self
                    .profiles()
                    .iter()
                    .filter(|p| p["device"] == device)
                    .map(|p| p["name"].as_str().unwrap().to_owned())
                    .collect();
                let mut profiles = Vec::new();
                let mut summary = Vec::new();
                for profile in incoming.profiles() {
                    let original = normalized_name(&profile["name"])?;
                    let mut target = original.to_owned();
                    let mut count = 2;
                    while used.contains(&target) {
                        let suffix = format!(" (importado {count})");
                        target = original
                            .chars()
                            .take(60 - suffix.chars().count())
                            .collect::<String>()
                            + &suffix;
                        count += 1;
                    }
                    used.insert(target.clone());
                    let mut imported =
                        json!({"device":device,"name":target,"settings":profile["settings"]});
                    if let Some(buttons) = profile.get("buttons") {
                        imported["buttons"] = buttons.clone();
                    }
                    profiles.push(imported);
                    summary.push(json!({"name":original,"assigned_name":target}));
                }
                changed = op == "import";
                if changed {
                    result["profiles"].as_array_mut().unwrap().extend(profiles);
                    result["version"] = json!(self.version().max(incoming.version()));
                }
                transfer = Some(json!({"imports":summary}));
            }
            _ => {
                let index = self
                    .profiles()
                    .iter()
                    .position(|p| p["device"] == device && p["name"] == request["name"])
                    .ok_or("Profile not found for this device")?;
                let old = self.profiles()[index]["name"].as_str().unwrap();
                match op {
                    "export" => {
                        transfer = Some(
                            json!({"document":{"version":self.version(),"profiles":[self.profiles()[index]],"active":{}}}),
                        );
                        changed = false;
                    }
                    "rename" => {
                        let new = normalized_name(&request["new_name"])?;
                        if new == old {
                            changed = false;
                        } else {
                            if self
                                .profiles()
                                .iter()
                                .any(|p| p["device"] == device && p["name"] == new)
                            {
                                return Err("Profile name already exists".into());
                            }
                            result["profiles"][index]["name"] = json!(new);
                            if let Some(rules) =
                                result.get_mut("rules").and_then(Value::as_array_mut)
                            {
                                for rule in rules
                                    .iter_mut()
                                    .filter(|r| r["device"] == device && r["profile"] == old)
                                {
                                    rule["profile"] = json!(new);
                                }
                            }
                            if result["active"].get(device) == Some(&json!(old)) {
                                result["active"][device] = json!(new);
                            }
                        }
                    }
                    "delete" => {
                        result["profiles"].as_array_mut().unwrap().remove(index);
                        if let Some(rules) = result.get_mut("rules").and_then(Value::as_array_mut) {
                            rules.retain(|r| r["device"] != device || r["profile"] != old);
                        }
                        if result["active"].get(device) == Some(&json!(old)) {
                            result["active"].as_object_mut().unwrap().remove(device);
                        }
                    }
                    _ => unreachable!(),
                }
            }
        }
        Ok(Management {
            document: Document::from_value(result)?,
            transfer,
            changed,
        })
    }
}
