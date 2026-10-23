// SPDX-License-Identifier: GPL-3.0-or-later
//! Validated finite guardian startup configuration; construction performs no I/O.
use super::{discovery::valid_name, ActionBindings, ProfileButtonPlan};
use serde_json::Value;
use std::{collections::BTreeSet, time::Duration};

#[derive(Clone)]
pub struct GuardianConfig {
    pub(super) interface: String,
    pub(super) slot: u8,
    pub(super) id: String,
    pub(super) controls: Vec<u16>,
    pub(super) duration: Duration,
    pub(super) keyboard_check: bool,
    pub(super) actions: Option<CaptureActions>,
}
#[derive(Clone)]
pub(super) struct CaptureActions {
    pub bindings: ActionBindings,
    pub wire: Value,
    pub profile: Option<ProfileButtonPlan>,
}
impl GuardianConfig {
    pub fn new(
        interface: &str,
        slot: u8,
        id: &str,
        controls: &[u16],
        duration: Duration,
    ) -> Result<Self, String> {
        let selected: BTreeSet<_> = controls.iter().copied().collect();
        if !valid_name(interface)
            || !(1..=6).contains(&slot)
            || id.len() != 8
            || !id
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_lowercase())
            || selected.is_empty()
            || selected.len() != controls.len()
            || !selected.iter().all(|c| [83, 86, 195].contains(c))
            || duration.is_zero()
            || duration > Duration::from_secs(30)
        {
            return Err("Expected hidrawN, Bolt slot 1..6, uppercase Unit ID, unique secondary CIDs and duration 0..30 s (exclusive zero)".into());
        }
        Ok(Self {
            interface: interface.into(),
            slot,
            id: id.into(),
            controls: selected.into_iter().collect(),
            duration,
            keyboard_check: false,
            actions: None,
        })
    }
    /// Opt-in capability/lifecycle check. Creates a zero-state virtual keyboard,
    /// never binds buttons, emits shortcuts or enables diversion.
    pub fn with_keyboard_check(mut self) -> Self {
        self.keyboard_check = true;
        self.actions = None;
        self
    }
    /// Explicit finite capture. Exactly these CIDs must already be selected for
    /// recovery; no IPC command can add bindings or change them after startup.
    pub fn with_capture_actions(mut self, value: &Value) -> Result<Self, String> {
        let bindings = ActionBindings::parse(value)?;
        if bindings.controls().is_empty() || bindings.controls() != self.controls {
            return Err(
                "Capture assignments must exactly match the nonempty recovery control set".into(),
            );
        }
        if serde_json::to_vec(value).map_err(|e| e.to_string())?.len() > 8192 {
            return Err("Capture assignments exceed the bounded startup frame".into());
        }
        self.actions = Some(CaptureActions {
            bindings,
            wire: value.clone(),
            profile: None,
        });
        self.keyboard_check = true;
        Ok(self)
    }
    /// Opt-in finite capture from the persisted buttons contract. Only software
    /// controls are registered for recovery. Native mappings must already match
    /// live hardware; this does not apply scalar settings, save or arm a profile.
    pub fn capture_profile_buttons(
        interface: &str,
        slot: u8,
        id: &str,
        buttons: &Value,
        duration: Duration,
    ) -> Result<Self, String> {
        let plan = ProfileButtonPlan::parse(buttons)?;
        Self::new(interface, slot, id, &plan.capture_controls(), duration)?
            .with_profile_buttons(buttons)
    }
    /// The selected recovery set must exactly equal the profile's software CIDs.
    pub fn with_profile_buttons(self, buttons: &Value) -> Result<Self, String> {
        if serde_json::to_vec(buttons)
            .map_err(|e| e.to_string())?
            .len()
            > 8192
        {
            return Err("Profile button assignments exceed the bounded startup frame".into());
        }
        let plan = ProfileButtonPlan::parse(buttons)?;
        let mut config = self.with_capture_actions(plan.capture_bindings())?;
        config.actions.as_mut().unwrap().profile = Some(plan);
        Ok(config)
    }
    pub(super) fn profile_buttons(&self) -> Value {
        self.actions
            .as_ref()
            .and_then(|a| a.profile.as_ref())
            .map_or(Value::Null, |p| p.buttons().clone())
    }
    pub(super) fn mode(&self) -> &'static str {
        if self.actions.is_some() {
            "capture-guardian"
        } else {
            "passive-guardian"
        }
    }
}

#[cfg(test)]
mod capture_config_tests {
    use super::*;
    use serde_json::json;
    fn config(controls: &[u16]) -> GuardianConfig {
        GuardianConfig::new("hidraw0", 1, "11223344", controls, Duration::from_secs(1)).unwrap()
    }
    #[test]
    fn capture_requires_an_exact_nonempty_closed_assignment_set() {
        for value in [
            json!({}),
            json!({"80":"modifier:ctrl"}),
            json!({"83":"shell:echo hi"}),
            json!({"83":"desktop:exec"}),
        ] {
            assert!(config(&[83]).with_capture_actions(&value).is_err());
        }
        assert!(config(&[83, 86])
            .with_capture_actions(&json!({"83":"shortcut:copy"}))
            .is_err());
        assert!(config(&[83])
            .with_capture_actions(&json!({"83":"shortcut:copy"}))
            .is_ok());
        assert!(config(&[83])
            .with_capture_actions(&json!({"83":"desktop:fullscreen"}))
            .is_ok());
    }
    #[test]
    fn check_mode_cannot_keep_capture_assignments_and_startup_owns_its_validated_copy() {
        let mut value = json!({"83":"modifier:ctrl"});
        let capture = config(&[83]).with_capture_actions(&value).unwrap();
        value["83"] = json!("modifier:alt");
        assert_eq!(
            capture.actions.as_ref().unwrap().wire["83"],
            "modifier:ctrl"
        );
        let check = capture.with_keyboard_check();
        assert!(check.actions.is_none());
        assert!(check.keyboard_check);
        assert_eq!(check.mode(), "passive-guardian");
    }
    #[test]
    fn profile_registration_is_exact_and_never_adds_native_controls_to_recovery() {
        let buttons = json!({"83":86,"86":"shortcut:copy"});
        let capture = GuardianConfig::capture_profile_buttons(
            "hidraw0",
            1,
            "11223344",
            &buttons,
            Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(capture.controls, vec![86]);
        assert_eq!(capture.profile_buttons(), buttons);
        assert_eq!(
            capture.actions.as_ref().unwrap().wire,
            json!({"86":"shortcut:copy"})
        );
        assert!(config(&[83, 86]).with_profile_buttons(&buttons).is_err());
        assert!(config(&[83]).with_profile_buttons(&buttons).is_err());
    }
    #[test]
    fn replacing_profile_capture_never_retains_old_native_requirements() {
        let buttons = json!({"83":86,"86":"shortcut:copy"});
        let capture = config(&[86]).with_profile_buttons(&buttons).unwrap();
        assert_eq!(
            capture.clone().with_keyboard_check().profile_buttons(),
            Value::Null
        );
        let manual = capture
            .with_capture_actions(&json!({"86":"modifier:alt"}))
            .unwrap();
        assert_eq!(manual.profile_buttons(), Value::Null);
        assert_eq!(manual.actions.unwrap().wire, json!({"86":"modifier:alt"}));
    }
}
