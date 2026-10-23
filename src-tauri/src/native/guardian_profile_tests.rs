// SPDX-License-Identifier: GPL-3.0-or-later
//! Real guardian processes and handwritten HID++ mouse; no user resources.
use super::*;

#[test]
fn mixed_profile_captures_only_software_buttons_and_restores_without_remapping() {
    let dir = Sandbox::new();
    let client = launch(&dir.0, "capture-profile", Duration::from_secs(3));
    assert_eq!(client.ready()["registered"], json!([83, 195]));
    assert_eq!(client.ready()["profile_buttons"]["86"], 83);
    assert_eq!(client.ready()["actions"], mode_bindings("capture-profile"));
    fs::write(dir.0.join("emit"), b"yes").unwrap();
    wait_file(&dir.0.join("actions-complete"));
    let report = client.finish().unwrap();
    assert_eq!(report["completed"], true, "{report}");
    assert_eq!(report["recovery"]["written"], json!([83, 195]));
    let wire = fs::read_to_string(dir.0.join("wire.log")).unwrap();
    assert!(!wire.contains("enable:86"));
    assert!(!wire.contains("clear:86"));
    assert!(!wire.contains("action-tap:(46, 4)"));
    assert_eq!(wire.matches("action-tap:(47, 4)").count(), 1);
    assert_eq!(wire.matches("action-held:Ctrl").count(), 1);
    assert_eq!(result(&dir.0)["final_targets"]["86"], 83);
    assert_eq!(result(&dir.0)["final_flags"]["83"], 0);
    assert_eq!(result(&dir.0)["final_flags"]["195"], 0);
    assert_keyboard_before_mouse(&dir.0);
}
#[test]
fn mismatched_native_assignment_prevents_diversion_and_ready() {
    let dir = Sandbox::new();
    let duration = Duration::from_secs(2);
    assert!(NativeGuardian::launch(
        fixture(&dir.0, "capture-profile-mismatch", duration),
        &mode_config("capture-profile-mismatch", duration),
        duration
    )
    .is_err());
    let report = result(&dir.0);
    assert_eq!(report["completed"], false);
    assert!(report["error"]
        .as_str()
        .unwrap()
        .contains("Native profile mapping 86"));
    assert_eq!(report["recovery"]["completed"], true);
    assert!(!fs::read_to_string(dir.0.join("wire.log"))
        .unwrap()
        .contains("enable:"));
    assert!(report["final_targets"].as_object().unwrap().is_empty());
}
#[test]
fn native_mapping_drift_during_activation_stops_the_batch_and_recovers() {
    let dir = Sandbox::new();
    let duration = Duration::from_secs(2);
    assert!(NativeGuardian::launch(
        fixture(&dir.0, "capture-profile-drift", duration),
        &mode_config("capture-profile-drift", duration),
        duration
    )
    .is_err());
    let report = result(&dir.0);
    assert_eq!(report["completed"], false);
    assert_eq!(report["owner"]["activation"]["confirmed"], json!([83]));
    assert_eq!(report["recovery"]["completed"], true);
    let wire = fs::read_to_string(dir.0.join("wire.log")).unwrap();
    assert_eq!(wire.matches("enable:83").count(), 1);
    assert!(!wire.contains("enable:195"));
    assert!(!wire.contains("enable:86"));
    assert!(!wire.contains("action-tap"));
    assert_eq!(report["final_flags"]["83"], 0);
    // Recovery preserves the actual firmware target, including external drift.
    assert_eq!(report["final_targets"]["86"], 86);
}
#[test]
fn parent_rejects_ready_that_drops_native_profile_requirements() {
    let dir = Sandbox::new();
    let duration = Duration::from_secs(2);
    let config = mode_config("capture-profile", duration);
    // A child with identical software bindings but without native requirements.
    // Its readiness must not satisfy the stronger profile startup contract.
    let command = fixture(&dir.0, "capture-profile-unbound", duration);
    assert!(NativeGuardian::launch(command, &config, duration).is_err());
    let report = result(&dir.0);
    assert_eq!(report["recovery"]["completed"], true);
}
