// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use crate::{native::GuardianConfig, profiles::Document};
use serde_json::json;
use std::time::Duration;

fn gestures() -> Value {
    json!({"type":"gestures","threshold":50,"units":"tenths-mm","actions":{
        "up":"media:volume-up","down":"custom:ctrl+shift:KeyZ","left":null,
        "right":"desktop:workspace-next","click":"shortcut:copy"}})
}
#[test]
fn mixed_profiles_keep_firmware_mappings_out_of_capture_and_preserve_values() {
    let mut source = json!({"83":86,"86":"modifier:ctrl","195":gestures()});
    let plan = ProfileButtonPlan::parse(&source).unwrap();
    assert_eq!(plan.native_mappings(), &json!({"83":86}));
    assert_eq!(plan.capture_controls(), vec![86, 195]);
    assert_eq!(plan.capture_bindings()["195"], gestures());
    assert!(plan.actions().uses_desktop());
    source["86"] = json!("modifier:alt");
    assert_eq!(plan.buttons()["86"], "modifier:ctrl");
}
#[test]
fn native_only_plans_cannot_create_a_capture_guardian() {
    let source = json!({"83":86,"86":83,"195":195});
    let plan = ProfileButtonPlan::parse(&source).unwrap();
    assert_eq!(plan.native_mappings(), &source);
    assert_eq!(plan.capture_bindings(), &json!({}));
    assert!(plan.capture_controls().is_empty());
    assert!(GuardianConfig::capture_profile_buttons(
        "hidraw0",
        1,
        "11223344",
        &source,
        Duration::from_secs(1)
    )
    .is_err());
}
#[test]
fn persisted_button_contract_rejects_experimental_extensions_and_malformed_values() {
    for value in [
        json!({}),
        json!(null),
        json!({"80":83}),
        json!({"083":83}),
        json!({"83":0}),
        json!({"83":65536}),
        json!({"83":true}),
        json!({"83":86.0}),
        json!({"83":"media:volume-up"}),
        json!({"195":"shortcut:copy"}),
        json!({"83":"desktop:exec"}),
        json!({"83":"custom:Ctrl+NoSuchKey"}),
        json!({"83":86,"86":"shell:echo hi"}),
    ] {
        assert!(ProfileButtonPlan::parse(&value).is_err(), "{value}");
    }
    let mut invalid = gestures();
    invalid["threshold"] = json!(9);
    assert!(ProfileButtonPlan::parse(&json!({"195":invalid})).is_err());
}
#[test]
fn native_confirmation_requires_unique_available_unintercepted_matching_controls() {
    let plan = ProfileButtonPlan::parse(&json!({"83":86,"86":"shortcut:copy"})).unwrap();
    let row = json!({"id":"83","available":true,"reported_target":86,"flags":0,"error":null});
    let valid = json!({"controls":[row]});
    plan.confirm_native(&valid).unwrap();
    for snapshot in [
        json!({}),
        json!({"controls":[]}),
        json!({"controls":[row,row]}),
        json!({"controls":[{"id":"83","available":true,"reported_target":83,"flags":0}]}),
    ] {
        assert!(plan.confirm_native(&snapshot).is_err());
    }
    for (field, value) in [
        ("available", json!(false)),
        ("flags", json!(1)),
        ("error", json!("unreadable")),
        ("reported_target", Value::Null),
    ] {
        let mut snapshot = valid.clone();
        snapshot["controls"][0][field] = value;
        assert!(plan.confirm_native(&snapshot).is_err(), "{snapshot}");
    }
}
#[test]
fn schema_two_to_ten_documents_compile_without_migrating_or_arming() {
    for (version, buttons) in [
        (2, json!({"83":86})),
        (3, json!({"83":"modifier:alt"})),
        (4, json!({"83":"shortcut:copy"})),
        (
            5,
            json!({"195":{"type":"gestures","threshold":20,"actions":{"up":null,"down":null,"left":null,"right":null,"click":null}}}),
        ),
        (
            7,
            json!({"195":{"type":"gestures","threshold":20,"actions":{"up":"media:volume-up","down":null,"left":null,"right":null,"click":null}}}),
        ),
        (
            8,
            json!({"195":{"type":"gestures","threshold":50,"units":"tenths-mm","actions":{"up":null,"down":null,"left":null,"right":null,"click":null}}}),
        ),
        (9, json!({"83":"custom:ctrl+shift:KeyZ"})),
        (10, json!({"83":86,"86":"desktop:fullscreen"})),
    ] {
        let value = json!({"version":version,"profiles":[{"device":"11223344","name":"test","settings":{"dpi":1850},"buttons":buttons}],"active":{}});
        let doc = Document::from_value(value.clone()).unwrap();
        let plan = ProfileButtonPlan::parse(&doc.profiles()[0]["buttons"]).unwrap();
        assert_eq!(plan.buttons(), &buttons);
        assert_eq!(doc.as_value(), &value);
        assert!(doc.effective_profile("11223344", None).is_none());
    }
}
