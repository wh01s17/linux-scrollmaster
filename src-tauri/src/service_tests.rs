// SPDX-License-Identifier: GPL-3.0-or-later
//! Service policy with temporary profiles and synthetic device state only.
use super::*;
use std::{
    cell::RefCell,
    fs,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

const ID: &str = "11223344";
struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "scrollmaster-service-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
    fn store(&self) -> ProfileStore {
        ProfileStore::at(self.0.join("linux-scrollmaster")).unwrap()
    }
    fn bytes(&self) -> Vec<u8> {
        fs::read(self.0.join("linux-scrollmaster/profiles.json")).unwrap()
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn snapshot() -> Value {
    json!({"id":ID,"settings":{
        "dpi":{"available":true,"value":1850},"smart-shift":{"available":true,"value":25},"scroll-ratchet":{"available":true,"value":2},
        "hires-smooth-invert":{"available":true,"value":false},"hires-smooth-resolution":{"available":true,"value":true},"thumb-scroll-invert":{"available":true,"value":false}
    },"buttons":{"error":null,"controls":[{"id":"83","available":true,"value":83},{"id":"86","available":true,"value":86},{"id":"195","available":true,"value":195}]}})
}
fn document(armed: bool) -> Document {
    Document::from_value(json!({"version":10,"profiles":[
        {"device":ID,"name":"Work","settings":{"dpi":1800},"buttons":{"83":80},"note":"keep"},
        {"device":ID,"name":"Game","settings":{"dpi":2400}},
        {"device":"55667788","name":"Other","settings":{"dpi":800}}
    ],"active":if armed {json!({ID:"Work"})} else {json!({})},"rules":[{"device":ID,"application":"game","profile":"Game"}],"metadata":{"keep":true}})).unwrap()
}
struct State {
    snapshot: Value,
    calls: Vec<&'static str>,
    fail_read: bool,
    fail_prepare: bool,
    fail_apply: bool,
    corrupt_receipt: bool,
    prepare_delay: bool,
    read_delay: bool,
    after_prepare: Option<Box<dyn FnOnce()>>,
    active_at_apply: Vec<Value>,
    store: ProfileStore,
}
struct Backend(Rc<RefCell<State>>);
impl ProfileBackend for Backend {
    fn identity(&self) -> &str {
        ID
    }
    fn snapshot(&mut self, _: Instant) -> Result<Value, String> {
        let mut s = self.0.borrow_mut();
        s.calls.push("snapshot");
        if s.read_delay {
            std::thread::sleep(Duration::from_millis(5));
        }
        if s.fail_read {
            Err("Synthetic device disconnected".into())
        } else {
            Ok(s.snapshot.clone())
        }
    }
    fn prepare(&mut self, _: &Changes, _: Instant) -> Result<(), String> {
        let mut s = self.0.borrow_mut();
        s.calls.push("prepare");
        if let Some(hook) = s.after_prepare.take() {
            hook();
        }
        if s.prepare_delay {
            std::thread::sleep(Duration::from_millis(5));
        }
        if s.fail_prepare {
            Err("Synthetic capability unavailable".into())
        } else {
            Ok(())
        }
    }
    fn apply(&mut self, changes: &Changes, _: Instant) -> Result<Applied, ApplyError> {
        let mut s = self.0.borrow_mut();
        s.calls.push("apply");
        let active = s.store.load().unwrap().document().as_value()["active"].clone();
        s.active_at_apply.push(active);
        let mut confirmed = Vec::new();
        if let Some(settings) = changes.settings() {
            for (key, value) in settings.as_object().unwrap() {
                if s.snapshot["settings"][key]["value"] != *value {
                    s.snapshot["settings"][key]["value"] = value.clone();
                    confirmed.push(key.clone());
                }
            }
        }
        if s.fail_apply {
            return Err(ApplyError {
                message: "Synthetic partial write".into(),
                confirmed,
                attempted: Some("button:83".into()),
            });
        }
        if let Some(buttons) = changes.buttons() {
            for (cid, value) in buttons.as_object().unwrap() {
                let control = s.snapshot["buttons"]["controls"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|c| c["id"] == *cid)
                    .unwrap();
                if control["value"] != *value {
                    control["value"] = value.clone();
                    confirmed.push(format!("button:{cid}"));
                }
            }
        }
        let mut snapshot = s.snapshot.clone();
        if s.corrupt_receipt {
            snapshot["settings"]["dpi"]["value"] = json!(0);
        }
        Ok(Applied {
            confirmed,
            snapshot,
        })
    }
}
fn fixture(armed: bool) -> (Sandbox, Rc<RefCell<State>>, ProfileService<Backend>) {
    let dir = Sandbox::new();
    let store = dir.store();
    store
        .commit(&store.load().unwrap(), &document(armed))
        .unwrap();
    let state = Rc::new(RefCell::new(State {
        snapshot: snapshot(),
        calls: vec![],
        fail_read: false,
        fail_prepare: false,
        fail_apply: false,
        corrupt_receipt: false,
        prepare_delay: false,
        read_delay: false,
        after_prepare: None,
        active_at_apply: vec![],
        store: store.clone(),
    }));
    let service =
        ProfileService::new(store, Backend(state.clone()), Duration::from_secs(2)).unwrap();
    (dir, state, service)
}
fn req(op: &str) -> Value {
    json!({"op":op,"device":ID,"name":"Work"})
}
fn set(dpi: u16) -> Value {
    json!({"op":"set","device":ID,"changes":{"dpi":dpi}})
}

#[test]
fn changes_are_closed_and_structurally_validated_without_io() {
    assert!(Changes::new(None, None).is_err());
    for value in [
        json!({}),
        json!({"dpi":true}),
        json!({"dpi":0}),
        json!({"unknown":1}),
    ] {
        assert!(Changes::new(Some(value), None).is_err());
    }
    for value in [
        json!({"80":81}),
        json!({"83":"shell:exec"}),
        json!({"83":false}),
    ] {
        assert!(Changes::new(None, Some(value)).is_err());
    }
    assert!(Changes::new(Some(json!({"dpi":1800})), Some(json!({"83":80}))).is_ok());
    assert!(Changes::new(None, Some(json!({"83":"desktop:fullscreen"}))).is_ok());
    // Backend must still prepare real output.
    let native = Changes::new(None, Some(json!({"83":83}))).unwrap();
    let mut current = snapshot();
    native.confirm(&current, ID).unwrap();
    current["buttons"]["error"] = json!("unconfirmed catalogue");
    assert!(native.confirm(&current, ID).is_err());
}
#[test]
fn manual_edits_persist_disarming_before_any_hardware_write() {
    let (dir, state, mut service) = fixture(true);
    let result = service.handle(&set(2000), None).unwrap();
    assert_eq!(result["active"], json!({}));
    assert_eq!(result["devices"][0]["settings"]["dpi"]["value"], 2000);
    assert_eq!(state.borrow().active_at_apply, vec![json!({})]);
    let data = dir.store().load().unwrap();
    assert_eq!(data.document().profiles()[0]["settings"]["dpi"], 1800);
    assert_eq!(
        data.document().as_value()["rules"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    service
        .handle(&json!({"op":"snapshot"}), Some("game"))
        .unwrap();
    assert_eq!(
        state
            .borrow()
            .calls
            .iter()
            .filter(|c| **c == "apply")
            .count(),
        1
    );
}
#[test]
fn explicit_restore_cannot_be_undone_by_a_different_armed_profile() {
    let (dir, state, mut service) = fixture(true);
    let mut request = req("restore");
    request["name"] = json!("Game");
    service.handle(&request, None).unwrap();
    assert_eq!(
        dir.store().load().unwrap().document().as_value()["active"],
        json!({})
    );
    let result = service.handle(&json!({"op":"snapshot"}), None).unwrap();
    assert_eq!(result["devices"][0]["settings"]["dpi"]["value"], 2400);
    assert_eq!(state.borrow().active_at_apply, vec![json!({})]);
}
#[test]
fn failed_preflight_keeps_profiles_and_never_writes_or_disarms() {
    let (dir, state, mut service) = fixture(true);
    let bytes = dir.bytes();
    state.borrow_mut().fail_prepare = true;
    assert_eq!(
        service.handle(&set(2000), None).unwrap_err().stage,
        "preflight"
    );
    assert_eq!(dir.bytes(), bytes);
    assert_eq!(state.borrow().calls, vec!["prepare"]);
}
#[test]
fn corrupted_profile_files_prevent_hardware_access() {
    let (dir, state, mut service) = fixture(true);
    fs::write(dir.0.join("linux-scrollmaster/profiles.json"), b"{broken").unwrap();
    assert_eq!(
        service.handle(&set(2000), None).unwrap_err().stage,
        "persistence"
    );
    assert!(state.borrow().calls.is_empty());
    assert_eq!(dir.bytes(), b"{broken");
}
#[test]
fn a_concurrent_profile_change_during_preflight_prevents_manual_writes() {
    for armed in [false, true] {
        let (dir, state, mut service) = fixture(armed);
        let path = dir.0.join("linux-scrollmaster/profiles.json");
        let newer = document(true).encode().unwrap();
        let expected = newer.clone();
        state.borrow_mut().after_prepare = Some(Box::new(move || {
            let mut bytes = newer;
            bytes.push(b'\n');
            fs::write(path, bytes).unwrap();
        }));
        assert_eq!(
            service.handle(&set(2000), None).unwrap_err().stage,
            "persistence"
        );
        assert!(!state.borrow().calls.contains(&"apply"));
        assert!(dir.bytes().starts_with(&expected));
        assert!(dir.store().load().unwrap().document().as_value()["active"]
            .get(ID)
            .is_some());
    }
}
#[test]
fn partial_apply_failure_leaves_manual_changes_disarmed_and_preserves_evidence() {
    let (dir, state, mut service) = fixture(true);
    state.borrow_mut().fail_apply = true;
    let error = service.handle(&set(2000), None).unwrap_err();
    assert_eq!(error.confirmed, vec!["dpi"]);
    assert_eq!(error.attempted.as_deref(), Some("button:83"));
    assert_eq!(error.stage, "apply");
    assert_eq!(
        dir.store().load().unwrap().document().as_value()["active"],
        json!({})
    );
    state.borrow_mut().fail_apply = false;
    service.handle(&json!({"op":"snapshot"}), None).unwrap();
    assert_eq!(
        state
            .borrow()
            .calls
            .iter()
            .filter(|c| **c == "apply")
            .count(),
        1
    );
}
#[test]
fn local_management_and_auto_work_offline_without_calling_the_backend() {
    let (dir, state, mut service) = fixture(false);
    state.borrow_mut().fail_read = true;
    service
        .handle(
            &json!({"op":"auto","device":"55667788","name":"Other"}),
            None,
        )
        .unwrap();
    let mut rename = req("rename");
    rename["new_name"] = json!("Renamed");
    service.handle(&rename, None).unwrap();
    let result = service
        .handle(&json!({"op":"export","device":ID,"name":"Renamed"}), None)
        .unwrap();
    assert_eq!(result["transfer"]["document"]["active"], json!({}));
    assert!(state.borrow().calls.is_empty());
    assert_eq!(
        dir.store().load().unwrap().document().as_value()["active"]["55667788"],
        "Other"
    );
}
#[test]
fn polling_reconciles_only_explicitly_armed_profiles_and_obeys_focus_rules() {
    let (_dir, state, mut service) = fixture(false);
    let result = service
        .handle(&json!({"op":"snapshot"}), Some("game"))
        .unwrap();
    assert_eq!(result["reconciliation"], json!([]));
    assert_eq!(state.borrow().calls, vec!["snapshot"]);
    service.handle(&req("auto"), None).unwrap();
    let general = service.handle(&json!({"op":"snapshot"}), None).unwrap();
    assert_eq!(general["reconciliation"][0]["profile"], "Work");
    let focused = service
        .handle(&json!({"op":"snapshot"}), Some("game"))
        .unwrap();
    assert_eq!(focused["devices"][0]["settings"]["dpi"]["value"], 2400);
    assert_eq!(focused["reconciliation"][0]["profile"], "Game");
    let missing = service
        .handle(&json!({"op":"snapshot"}), Some("unlisted"))
        .unwrap();
    assert_eq!(missing["devices"][0]["settings"]["dpi"]["value"], 1800);
}
#[test]
fn polls_compare_real_values_again_instead_of_caching_profile_success() {
    let (_dir, state, mut service) = fixture(true);
    service.handle(&json!({"op":"snapshot"}), None).unwrap();
    state.borrow_mut().snapshot["settings"]["dpi"]["value"] = json!(3200);
    let result = service.handle(&json!({"op":"snapshot"}), None).unwrap();
    assert_eq!(result["devices"][0]["settings"]["dpi"]["value"], 1800);
    assert_eq!(
        state
            .borrow()
            .calls
            .iter()
            .filter(|c| **c == "apply")
            .count(),
        2
    );
}
#[test]
fn disconnected_polls_keep_profiles_and_clear_device_values() {
    let (_dir, state, mut service) = fixture(true);
    state.borrow_mut().fail_read = true;
    let result = service.handle(&json!({"op":"snapshot"}), None).unwrap();
    assert_eq!(result["devices"], json!([]));
    assert_eq!(result["profiles"].as_array().unwrap().len(), 3);
    assert_eq!(result["reconciliation"], json!([]));
    assert!(!result["warnings"].as_array().unwrap().is_empty());
    state.borrow_mut().fail_read = false;
    let result = service.handle(&json!({"op":"snapshot"}), None).unwrap();
    assert_eq!(result["reconciliation"][0]["profile"], "Work");
}
#[test]
fn failed_automatic_application_clears_stale_values_and_reports_partial_results() {
    let (_dir, state, mut service) = fixture(true);
    state.borrow_mut().fail_apply = true;
    let result = service.handle(&json!({"op":"snapshot"}), None).unwrap();
    assert_eq!(result["devices"], json!([]));
    assert_eq!(result["reconciliation"], json!([]));
    assert_eq!(
        result["reconciliation_errors"][0]["confirmed"],
        json!(["dpi"])
    );
    assert_eq!(result["active"][ID], "Work");
}
#[test]
fn failed_automatic_preflight_does_not_claim_reconciliation() {
    let (_dir, state, mut service) = fixture(true);
    state.borrow_mut().fail_prepare = true;
    let result = service.handle(&json!({"op":"snapshot"}), None).unwrap();
    assert_eq!(result["devices"].as_array().unwrap().len(), 1);
    assert_eq!(result["reconciliation"], json!([]));
    assert_eq!(result["reconciliation_errors"][0]["stage"], "preflight");
    assert!(!state.borrow().calls.contains(&"apply"));
}
#[test]
fn saving_requires_complete_current_values_and_preserves_metadata_and_activation() {
    let (dir, state, mut service) = fixture(true);
    service.handle(&req("save"), None).unwrap();
    let loaded = dir.store().load().unwrap();
    let profile = loaded
        .document()
        .profiles()
        .iter()
        .find(|p| p["name"] == "Work")
        .unwrap();
    assert_eq!(profile["settings"].as_object().unwrap().len(), 6);
    assert_eq!(profile["buttons"], json!({"83":83,"86":86,"195":195}));
    assert_eq!(profile["note"], "keep");
    assert_eq!(loaded.document().as_value()["active"][ID], "Work");
    assert_eq!(state.borrow().calls, vec!["snapshot"]);
}
#[test]
fn incomplete_saves_never_overwrite_existing_profiles() {
    for fault in 0..7 {
        let (dir, state, mut service) = fixture(false);
        let bytes = dir.bytes();
        let mut s = state.borrow_mut();
        match fault {
            0 => s.snapshot["settings"]["dpi"]["available"] = json!(false),
            1 => s.snapshot["settings"]
                .as_object_mut()
                .unwrap()
                .remove("thumb-scroll-invert")
                .map(|_| ())
                .unwrap(),
            2 => s.snapshot["buttons"]["error"] = json!("incomplete"),
            3 => s.snapshot["buttons"]["controls"][0]["available"] = json!(false),
            4 => s.snapshot["buttons"]["controls"][1]["id"] = json!("83"),
            5 => {
                s.snapshot.as_object_mut().unwrap().remove("buttons");
            }
            6 => s.snapshot["settings"]["dpi"]["value"] = json!(true),
            _ => unreachable!(),
        };
        drop(s);
        assert!(service.handle(&req("save"), None).is_err(), "fault {fault}");
        assert_eq!(dir.bytes(), bytes);
        assert!(!state.borrow().calls.contains(&"apply"));
    }
}
#[test]
fn service_rejects_unknown_operations_and_wrong_device_before_hardware() {
    let (_dir, state, mut service) = fixture(false);
    for request in [
        json!(null),
        json!({}),
        json!({"op":"exec"}),
        json!({"op":"set","changes":{"dpi":1800}}),
        json!({"op":"restore","device":"55667788","name":"Other"}),
        json!({"op":"snapshot","device":"55667788"}),
    ] {
        assert!(service.handle(&request, None).is_err());
    }
    assert!(state.borrow().calls.is_empty());
}
#[test]
fn late_backend_results_cannot_extend_the_shared_request_deadline() {
    for during_read in [true, false] {
        let (dir, state, _) = fixture(false);
        state.borrow_mut().read_delay = during_read;
        state.borrow_mut().prepare_delay = !during_read;
        let mut service = ProfileService::new(
            dir.store(),
            Backend(state.clone()),
            Duration::from_millis(1),
        )
        .unwrap();
        let result = service.handle(&if during_read { req("save") } else { set(2000) }, None);
        assert_eq!(result.unwrap_err().stage, "deadline");
        assert!(!state.borrow().calls.contains(&"apply"));
    }
}
#[test]
fn a_success_receipt_with_wrong_values_is_not_reported_as_success() {
    let (_dir, state, mut service) = fixture(true);
    state.borrow_mut().corrupt_receipt = true;
    let error = service.handle(&set(2000), None).unwrap_err();
    assert_eq!(error.stage, "apply");
    assert_eq!(error.confirmed, vec!["dpi"]);
    assert!(error.message.contains("unconfirmed"));
}
#[test]
fn changed_identity_in_a_snapshot_is_not_saved_or_presented_as_the_device() {
    let (dir, state, mut service) = fixture(true);
    let bytes = dir.bytes();
    state.borrow_mut().snapshot["id"] = json!("55667788");
    assert_eq!(
        service.handle(&req("save"), None).unwrap_err().stage,
        "snapshot"
    );
    assert_eq!(dir.bytes(), bytes);
    let result = service.handle(&json!({"op":"snapshot"}), None).unwrap();
    assert_eq!(result["devices"], json!([]));
    assert!(!state.borrow().calls.contains(&"apply"));
}
