// SPDX-License-Identifier: GPL-3.0-or-later
//! Shared startup configurations for synthetic guardian processes.
use super::*;

pub(super) fn config(duration: Duration) -> GuardianConfig {
    GuardianConfig::new("hidraw0", 1, "11223344", &[83, 86, 195], duration).unwrap()
}
pub(super) fn capture_bindings() -> Value {
    json!({"83":"modifier:ctrl","86":"shortcut:copy","195":{
        "type":"gestures","threshold":20,"actions":{"up":null,"down":null,"left":null,"right":"shortcut:paste","click":null}
    }})
}
pub(super) fn mode_bindings(mode: &str) -> Value {
    let mut value = capture_bindings();
    if mode.starts_with("capture-profile") {
        value.as_object_mut().unwrap().remove("86");
    }
    if mode.starts_with("capture-desktop") {
        value["86"] = json!("desktop:fullscreen");
        value["195"]["actions"]["right"] = json!("desktop:workspace-next");
    }
    value
}
pub(super) fn mode_config(mode: &str, duration: Duration) -> GuardianConfig {
    if mode.starts_with("capture-profile") {
        let mut buttons = mode_bindings(mode);
        buttons["86"] = json!(83);
        GuardianConfig::capture_profile_buttons("hidraw0", 1, "11223344", &buttons, duration)
            .unwrap()
    } else if mode.starts_with("capture-") {
        config(duration)
            .with_capture_actions(&mode_bindings(mode))
            .unwrap()
    } else if mode.starts_with("keyboard-") {
        config(duration).with_keyboard_check()
    } else {
        config(duration)
    }
}
