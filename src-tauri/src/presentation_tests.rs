// SPDX-License-Identifier: GPL-3.0-or-later
//! Contract mapping only; inputs mirror native snapshots without hardware.
use super::*;

fn native() -> Value {
    json!({"id":"11223344","name":"MX Master 3S","connection":"Logi Bolt","slot":2,
        "protocol":{"major":4,"minor":5},
        "battery":{"available":true,"percentage":80,"level_code":4,"status":0},
        "settings":{"dpi":{"available":true,"value":1800,"raw":"0708","choices":[1000,1800]},
            "smart-shift":{"available":false,"value":null,"error":"timeout"}},
        "native_capture":{"active":true},
        "buttons":{"error":null,"controls":[
            {"id":"83","label":"Atrás","available":true,"value":"custom:ctrl+shift:KeyT","flags":1,
             "capabilities":{"flags":0x31},"choices":[{"value":83,"label":"Atrás"}]},
            {"id":"86","label":"Adelante","available":false,"value":null,"flags":1,"error":"Reporting is intercepted",
             "capabilities":{"flags":0x31},"choices":[{"value":86,"label":"Adelante"}]},
            {"id":"195","label":"Botón de gestos","available":true,"value":195,"flags":0,
             "capabilities":{"flags":0x131},"choices":[{"value":195,"label":"Botón de gestos"}]}]}})
}
fn values(control: &Value) -> Vec<Value> {
    control["choices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["value"].clone())
        .collect()
}

#[test]
fn device_fields_match_the_vue_mouse_contract() {
    let desktop = json!({"available":true,"error":null});
    let mouse = device(&native(), &desktop);
    assert_eq!(mouse["protocol"], 4.5);
    assert_eq!(mouse["battery"], 80);
    assert_eq!(
        mouse["settings"]["dpi"],
        json!({"value":1800,"available":true,"choices":[1000,1800]})
    );
    assert_eq!(mouse["settings"]["smart-shift"]["error"], "timeout");
    assert_eq!(mouse["native_capture"]["active"], true);
    assert_eq!(mouse["buttons"]["available"], true);
    assert_eq!(mouse["buttons"]["gesture_feedback"], Value::Null);
    let back = &mouse["buttons"]["controls"][0];
    let choices = values(back);
    assert_eq!(choices[0], 83);
    for action in [
        "modifier:alt",
        "modifier:ctrl",
        "shortcut:undo",
        "desktop:maximize",
    ] {
        assert!(choices.contains(&json!(action)), "{action}");
    }
    // A live custom chord is offered under its readable label.
    let custom = back["choices"].as_array().unwrap().last().unwrap();
    assert_eq!(
        custom,
        &json!({"value":"custom:ctrl+shift:KeyT","label":"Ctrl + Shift + T"})
    );
    assert_eq!(back["shortcuts"]["keys"].as_array().unwrap().len(), 63);
    assert_eq!(back["shortcuts"]["modifiers"][3]["value"], "super");
    assert_eq!(back["gestures_available"], false);
    let blocked = &mouse["buttons"]["controls"][1];
    assert_eq!(blocked["available"], false);
    assert_eq!(
        blocked["error"],
        "Este botón está interceptado. No se modificará."
    );
    let gesture = &mouse["buttons"]["controls"][2];
    assert_eq!(gesture["gestures_available"], true);
    assert_eq!(gesture["gesture_actions"].as_array().unwrap().len(), 14);
    assert_eq!(values(gesture), vec![json!(195)]);
}
#[test]
fn unavailable_desktop_disables_only_desktop_options() {
    let desktop = json!({"available":false,"error":"Missing Hyprland instance signature"});
    let mouse = device(&native(), &desktop);
    let back = &mouse["buttons"]["controls"][0];
    for choice in back["choices"].as_array().unwrap() {
        let is_desktop = choice["value"]
            .as_str()
            .is_some_and(|v| v.starts_with("desktop:"));
        assert_eq!(choice.get("disabled") == Some(&json!(true)), is_desktop);
    }
    assert_eq!(back["desktop_error"], "Missing Hyprland instance signature");
}
#[test]
fn non_divertable_controls_offer_only_native_targets() {
    let mut input = native();
    input["buttons"]["controls"][0]["capabilities"]["flags"] = json!(0x11);
    input["buttons"]["controls"][0]["value"] = json!(83);
    let mouse = device(&input, &json!({"available":true}));
    let back = &mouse["buttons"]["controls"][0];
    assert_eq!(values(back), vec![json!(83)]);
    assert!(back.get("shortcuts").is_none());
}
#[test]
fn only_canonical_custom_chords_receive_labels() {
    assert_eq!(custom_label("custom::F5").as_deref(), Some("F5"));
    assert_eq!(
        custom_label("custom:alt+super:ArrowLeft").as_deref(),
        Some("Alt + Super + ←")
    );
    for invalid in [
        "custom:alt+ctrl:KeyA",
        "custom:ctrl+ctrl:KeyA",
        "custom:hyper:KeyA",
        "custom:ctrl:Key1",
        "shortcut:copy",
        "custom:ctrl:KeyA:x",
    ] {
        assert_eq!(custom_label(invalid), None, "{invalid}");
    }
}
#[test]
fn offline_snapshot_gets_the_legacy_shape_and_a_diagnostic() {
    let mut data = json!({"devices":[],"profiles":[{"device":"11223344","name":"Work"}]});
    snapshot(&mut data, &json!({"available":false}), "Sin receptor");
    assert_eq!(data["diagnostic"], "Sin receptor");
    assert_eq!(data["warnings"], json!([]));
    assert_eq!(data["active"], json!({}));
    let mut online =
        json!({"devices":[native()],"warnings":["x"],"active":{},"profiles":[],"rules":[]});
    snapshot(&mut online, &json!({"available":true}), "unused");
    assert_eq!(online["diagnostic"], Value::Null);
    assert_eq!(online["devices"][0]["battery"], 80);
    assert_eq!(online["warnings"], json!(["x"]));
}
#[test]
fn unreachable_mouse_warning_is_shown_in_plain_language() {
    // Exact text observed with the mouse switched off (receiver error 4).
    let raw = "snapshot: HID++ session: Protocol(ErrorReply { marker: 143, feature: 0, function_and_software: 24, code: 4 }); confirmed: []; attempted: None";
    let other = "snapshot: HID++ session: Protocol(ErrorReply { marker: 143, feature: 0, function_and_software: 24, code: 9 })";
    let mut data = json!({"devices":[],"warnings":[raw, other]});
    snapshot(&mut data, &json!({"available":true}), "Sin mouse");
    assert_eq!(data["warnings"][0], UNREACHABLE);
    assert_eq!(data["warnings"][1], other);
}
#[test]
fn battery_charging_status_comes_from_the_1004_status_byte() {
    for (feature, status, expected) in [
        ("1004", 0, json!("discharging")),
        ("1004", 1, json!("charging")),
        ("1004", 2, json!("charging")),
        ("1004", 3, json!("full")),
        ("1004", 4, Value::Null),
        ("1000", 1, Value::Null),
    ] {
        let mut input = native();
        input["battery"]["feature"] = json!(feature);
        input["battery"]["status"] = json!(status);
        let mouse = device(&input, &json!({"available":true}));
        assert_eq!(mouse["battery_status"], expected, "{feature} {status}");
        assert_eq!(mouse["battery"], 80);
    }
}
