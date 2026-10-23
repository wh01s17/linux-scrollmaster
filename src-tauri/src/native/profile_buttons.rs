// SPDX-License-Identifier: GPL-3.0-or-later
//! Compile persisted button assignments without applying or arming a profile.
use super::ActionBindings;
use serde_json::{Map, Value};

/// A validated, immutable split of a profile's `buttons` map. Firmware mappings
/// remain separate from software capture. No device, desktop or profile I/O.
#[derive(Clone, Debug)]
pub struct ProfileButtonPlan {
    source: Value,
    native: Value,
    software: Value,
    bindings: ActionBindings,
}
impl ProfileButtonPlan {
    pub fn parse(buttons: &Value) -> Result<Self, String> {
        // Reuse the persisted contract (including no side-button media), rather
        // than accepting everything the experimental action engine can parse.
        crate::profiles::button_schema(buttons)?;
        let (mut native, mut software) = (Map::new(), Map::new());
        for (cid, target) in buttons.as_object().unwrap() {
            if target.is_u64() {
                native.insert(cid.clone(), target.clone());
            } else {
                software.insert(cid.clone(), target.clone());
            }
        }
        let software = Value::Object(software);
        let bindings = ActionBindings::parse(&software)?;
        Ok(Self {
            source: buttons.clone(),
            native: Value::Object(native),
            software,
            bindings,
        })
    }
    pub fn buttons(&self) -> &Value {
        &self.source
    }
    /// Empty object means no firmware assignment was requested.
    pub fn native_mappings(&self) -> &Value {
        &self.native
    }
    pub fn capture_bindings(&self) -> &Value {
        &self.software
    }
    pub fn actions(&self) -> &ActionBindings {
        &self.bindings
    }
    pub fn capture_controls(&self) -> Vec<u16> {
        self.bindings.controls()
    }
    /// Finite capture never applies native remaps. Require every requested
    /// firmware mapping to already match the same owner's fresh HID snapshot.
    /// Activation rechecks the complete catalogue before each setter and after
    /// the batch, so a mapping change cannot pass as an accepted profile.
    pub(super) fn confirm_native(&self, snapshot: &Value) -> Result<(), String> {
        let controls = snapshot["controls"]
            .as_array()
            .ok_or("Missing profile button catalogue")?;
        for (cid, target) in self.native.as_object().unwrap() {
            let found: Vec<_> = controls.iter().filter(|c| c["id"] == *cid).collect();
            if found.len() != 1
                || found[0]["available"] != true
                || found[0]["reported_target"] != *target
                || found[0]["flags"] != 0
                || found[0].get("error").is_some_and(|e| !e.is_null())
            {
                return Err(format!("Native profile mapping {cid} is not confirmed; finite capture does not apply firmware mappings"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "profile_buttons_tests.rs"]
mod tests;
