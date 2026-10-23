// SPDX-License-Identifier: GPL-3.0-or-later
use super::store::Stage;
use super::*;
use serde_json::{json, Value};
use std::{
    fs,
    os::unix::fs::{symlink, DirBuilderExt, MetadataExt, PermissionsExt},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

fn base() -> Value {
    json!({"version":1,"profiles":[{"device":"unit-a","name":"Work","settings":{"dpi":1850}}],"active":{}})
}
fn doc(value: Value) -> Document {
    Document::from_value(value).unwrap()
}
fn gesture(action: Value) -> Value {
    json!({"type":"gestures","threshold":20,"actions":{"up":action,"down":null,"left":null,"right":null,"click":null}})
}
fn request(op: &str) -> Value {
    json!({"op":op,"device":"unit-a","name":"Work"})
}
fn with_rules() -> Document {
    let mut value = base();
    value["version"] = json!(6);
    value["profiles"].as_array_mut().unwrap().extend([
        json!({"device":"unit-a","name":"Game","settings":{"dpi":2400}}),
        json!({"device":"unit-b","name":"Work","settings":{"dpi":800}}),
    ]);
    value["active"] = json!({"unit-a":"Work","unit-b":"Work"});
    value["rules"] = json!([
        {"device":"unit-a","application":"game","profile":"Game"},
        {"device":"unit-a","application":"editor","profile":"Work"},
        {"device":"unit-b","application":"editor","profile":"Work"},
    ]);
    doc(value)
}
/// A 0700 directory regardless of the umask: the store rejects group-writable
/// ones, and some systems (Ubuntu with `su`) default to 002.
fn private_dir(path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
    fs::DirBuilder::new().mode(0o700).create(path)
}
/// Like `fs::write`, but a new file is 0600 whatever the umask (see `private_dir`).
fn private_file(path: impl AsRef<std::path::Path>, bytes: impl AsRef<[u8]>) -> std::io::Result<()> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?
        .write_all(bytes.as_ref())
}
struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "scrollmaster-profiles-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
    fn store(&self) -> ProfileStore {
        ProfileStore::at(self.0.join("linux-scrollmaster")).unwrap()
    }
    fn seed(&self) -> (ProfileStore, Revision) {
        let store = self.store();
        let old = store.load().unwrap();
        let revision = store.commit(&old, &doc(base())).unwrap();
        (store, revision)
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn all_schema_gates_preserve_legacy_values_without_migration() {
    for (buttons, required) in [
        (json!({"83":80}), 2),
        (json!({"83":"modifier:ctrl"}), 3),
        (json!({"86":"shortcut:copy"}), 4),
        (json!({"195":gesture(json!("shortcut:paste"))}), 5),
        (json!({"195":gesture(json!("media:mute"))}), 7),
        (
            json!({"195":{ "type":"gestures","threshold":10,"units":"tenths-mm","actions":{"up":null,"down":null,"left":null,"right":null,"click":null}}}),
            8,
        ),
        (json!({"83":"custom:ctrl+alt+shift+super:KeyS"}), 9),
        (json!({"195":gesture(json!("custom::F12"))}), 9),
        (json!({"86":"desktop:fullscreen"}), 10),
        (json!({"195":gesture(json!("desktop:workspace-next"))}), 10),
    ] {
        for version in 1..=10 {
            let mut value = base();
            value["version"] = json!(version);
            value["profiles"][0]["buttons"] = buttons.clone();
            let result = Document::from_value(value.clone());
            assert_eq!(result.is_ok(), version >= required, "v{version}: {buttons}");
            if let Ok(result) = result {
                assert_eq!(result.as_value(), &value);
            }
        }
    }
}
#[test]
fn numeric_settings_and_button_mappings_are_strictly_typed() {
    for (key, good, bad) in [
        (
            "dpi",
            vec![json!(1), json!(65535)],
            vec![
                json!(0),
                json!(65536),
                json!(true),
                json!(1800.0),
                json!("1800"),
            ],
        ),
        (
            "smart-shift",
            vec![json!(1), json!(50)],
            vec![json!(0), json!(51), json!(true), json!(1.0)],
        ),
        (
            "scroll-ratchet",
            vec![json!(1), json!(2)],
            vec![json!(0), json!(3), json!(true), json!(2.0)],
        ),
        (
            "hires-smooth-invert",
            vec![json!(true), json!(false)],
            vec![json!(1), json!(null), json!("false")],
        ),
        (
            "hires-smooth-resolution",
            vec![json!(true), json!(false)],
            vec![json!(0)],
        ),
        (
            "thumb-scroll-invert",
            vec![json!(true), json!(false)],
            vec![json!(1)],
        ),
    ] {
        for (valid, values) in [(true, good), (false, bad)] {
            for value in values {
                let mut data = base();
                data["profiles"][0]["settings"] = json!({key:value});
                assert_eq!(Document::from_value(data).is_ok(), valid, "{key}: {value}");
            }
        }
    }
    for buttons in [
        json!({}),
        json!({"80":81}),
        json!({"83":0}),
        json!({"83":65536}),
        json!({"83":true}),
        json!({"83":80.0}),
        json!({"83":"media:mute"}),
        json!({"195":"modifier:ctrl"}),
        json!({"83":"custom:alt+ctrl:KeyS"}),
        json!({"86":"desktop:exec"}),
    ] {
        let mut data = base();
        data["version"] = json!(10);
        data["profiles"][0]["buttons"] = buttons.clone();
        assert!(Document::from_value(data).is_err(), "{buttons}");
    }
}
#[test]
fn invalid_documents_references_and_duplicates_are_rejected() {
    for value in [
        json!(null),
        json!([]),
        json!({}),
        json!({"version":true,"profiles":[],"active":{}}),
        json!({"version":11,"profiles":[],"active":{}}),
        json!({"version":1.0,"profiles":[],"active":{}}),
    ] {
        assert!(Document::from_value(value).is_err());
    }
    for (field, value) in [
        ("device", json!("")),
        ("device", json!("x".repeat(129))),
        ("name", json!(" ")),
        ("name", json!("x".repeat(61))),
        ("settings", json!({})),
        ("settings", json!({"unknown":true})),
    ] {
        let mut data = base();
        data["profiles"][0][field] = value;
        assert!(Document::from_value(data).is_err());
    }
    let mut data = base();
    data["active"] = json!({"other-device":"Work"});
    assert!(Document::from_value(data).is_err());
    let mut data = base();
    let duplicate = data["profiles"][0].clone();
    data["profiles"].as_array_mut().unwrap().push(duplicate);
    assert!(Document::from_value(data).is_err());
}
#[test]
fn unicode_character_limits_whitespace_and_unknown_metadata_are_preserved() {
    let mut value = base();
    value["profiles"][0]["device"] = json!("鼠".repeat(128));
    value["profiles"][0]["name"] = json!(format!("\u{1c} {} \u{1f}", "🖱".repeat(60)));
    value["profiles"][0]["future-metadata"] = json!({"note":"conservar","x":[1,2,3]});
    value["extension"] = json!({"author":"prueba"});
    let parsed = doc(value.clone());
    assert_eq!(
        Document::parse(&parsed.encode().unwrap())
            .unwrap()
            .as_value(),
        &value
    );
}
#[test]
fn malformed_oversized_or_non_utf8_json_is_rejected() {
    for bytes in [
        b"{broken".to_vec(),
        vec![0xff],
        vec![b' '; MAX_DOCUMENT_BYTES + 1],
        br#"{"version":1,"profiles":[],"active":{},"x":NaN}"#.to_vec(),
    ] {
        assert!(Document::parse(&bytes).is_err());
    }
    let mut data = base();
    data["padding"] = json!("x".repeat(MAX_DOCUMENT_BYTES));
    assert!(Document::from_value(data).is_err());
    let mut nested = Value::Null;
    for _ in 0..150 {
        nested = json!([nested]);
    }
    let mut data = base();
    data["extension"] = nested;
    assert!(Document::from_value(data).is_err());
}
#[test]
fn count_and_application_rule_boundaries_are_enforced() {
    let mut data = base();
    data["profiles"] = json!((0..100)
        .map(|n| json!({"device":"a","name":n.to_string(),"settings":{"dpi":1}}))
        .collect::<Vec<_>>());
    assert!(Document::from_value(data.clone()).is_ok());
    data["profiles"]
        .as_array_mut()
        .unwrap()
        .push(json!({"device":"a","name":"extra","settings":{"dpi":1}}));
    assert!(Document::from_value(data).is_err());
    let valid = with_rules();
    for rules in [
        json!(null),
        json!([{"device":"unit-a","application":"","profile":"Work"}]),
        json!([{"device":"unit-a","application":"a\nb","profile":"Work"}]),
        json!([{"device":"unit-a","application":"x","profile":"absent"}]),
        json!([{"device":"unit-a","application":"x","profile":"Work","extra":true}]),
        json!([valid.0["rules"][0], valid.0["rules"][0]]),
    ] {
        let mut data = valid.0.clone();
        data["rules"] = rules;
        assert!(Document::from_value(data).is_err());
    }
    let mut data = valid.0.clone();
    data["version"] = json!(5);
    assert!(Document::from_value(data).is_err());
}
#[test]
fn rename_updates_only_matching_device_active_and_rule_references() {
    let document = with_rules();
    let mut req = request("rename");
    req["new_name"] = json!("  Renamed  ");
    let changed = document.manage(&req).unwrap();
    assert!(changed.changed);
    assert_eq!(
        changed.document.0["active"],
        json!({"unit-a":"Renamed","unit-b":"Work"})
    );
    assert_eq!(changed.document.0["rules"][1]["profile"], "Renamed");
    assert_eq!(changed.document.0["rules"][2]["profile"], "Work");
    assert_eq!(document.0["profiles"][0]["name"], "Work");
    req["new_name"] = json!("Game");
    assert!(document.manage(&req).is_err());
    req["new_name"] = json!("Work");
    assert!(!document.manage(&req).unwrap().changed);
}
#[test]
fn delete_disarms_and_removes_only_matching_references() {
    let document = with_rules();
    let result = document.manage(&request("delete")).unwrap();
    assert_eq!(result.document.0["active"], json!({"unit-b":"Work"}));
    assert_eq!(result.document.profiles().len(), 2);
    assert_eq!(result.document.0["rules"].as_array().unwrap().len(), 2);
    assert_eq!(result.document.0["rules"][1]["device"], "unit-b");
    assert_eq!(document.profiles().len(), 3);
}
#[test]
fn export_preserves_profile_contents_without_activation_or_rules() {
    let document = with_rules();
    let result = document.manage(&request("export")).unwrap();
    assert!(!result.changed);
    assert_eq!(result.document, document);
    let export = &result.transfer.unwrap()["document"];
    assert_eq!(export["active"], json!({}));
    assert!(export.get("rules").is_none());
    assert_eq!(export["profiles"], json!([document.profiles()[0]]));
    assert_eq!(export["version"], 6);
}
#[test]
fn import_preview_and_commit_share_names_but_never_import_activation_or_rules() {
    let document = with_rules();
    let mut incoming = base();
    incoming["active"] = json!({"unit-a":"Work"});
    incoming["rules"] = json!("ignore untrusted rules");
    let mut req = request("import_preview");
    req["document"] = incoming;
    let preview = document.manage(&req).unwrap();
    assert!(!preview.changed);
    assert_eq!(preview.document, document);
    assert_eq!(
        preview.transfer.as_ref().unwrap()["imports"][0]["assigned_name"],
        "Work (importado 2)"
    );
    req["op"] = json!("import");
    let imported = document.manage(&req).unwrap();
    assert_eq!(imported.transfer, preview.transfer);
    assert_eq!(imported.document.0["active"], document.0["active"]);
    assert_eq!(imported.document.0["rules"], document.0["rules"]);
    assert_eq!(imported.document.version(), 6);
    assert_eq!(imported.document.profiles().len(), 4);
    let again = imported.document.manage(&req).unwrap();
    assert_eq!(
        again.transfer.unwrap()["imports"][0]["assigned_name"],
        "Work (importado 3)"
    );
}
#[test]
fn imports_retarget_devices_and_truncate_colliding_unicode_names_by_characters() {
    let mut incoming = base();
    incoming["profiles"][0]["name"] = json!("🖱".repeat(60));
    let document = doc(incoming.clone());
    let mut req = request("import");
    req["document"] = incoming;
    let result = document.manage(&req).unwrap();
    let name = result.document.profiles()[1]["name"].as_str().unwrap();
    assert_eq!(name.chars().count(), 60);
    assert!(name.ends_with(" (importado 2)"));
    req["device"] = json!("unit-new");
    let result = document.manage(&req).unwrap();
    assert_eq!(result.document.profiles()[1]["device"], "unit-new");
    assert!(result.document.0["active"].as_object().unwrap().is_empty());
}
#[test]
fn import_schema_upgrade_preserves_existing_profiles_and_metadata() {
    let mut data = base();
    data["extra"] = json!({"keep":true});
    let document = doc(data.clone());
    let mut incoming = base();
    incoming["version"] = json!(10);
    incoming["profiles"][0]["buttons"] = json!({"83":"desktop:fullscreen"});
    let result = document
        .manage(&json!({"op":"import","device":"new","document":incoming}))
        .unwrap();
    assert_eq!(result.document.version(), 10);
    assert_eq!(result.document.0["extra"], data["extra"]);
    assert_eq!(result.document.profiles()[0], data["profiles"][0]);
}
#[test]
fn invalid_management_and_imports_leave_the_original_unchanged() {
    let document = doc(base());
    let original = document.clone();
    for request in [
        json!(null),
        json!({"op":"set","device":"unit-a"}),
        json!({"op":"delete","device":"other","name":"Work"}),
        json!({"op":"rename","device":"unit-a","name":"Work","new_name":" "}),
        json!({"op":"import","device":"unit-a","document":{"version":1,"profiles":[]}}),
        json!({"op":"import","device":"unit-a","document":{"version":11,"profiles":[]}}),
        json!({"op":"import","device":"unit-a","document":{"version":1,"profiles":[],"padding":"x".repeat(MAX_DOCUMENT_BYTES)}}),
    ] {
        assert!(document.manage(&request).is_err(), "{request}");
        assert_eq!(document, original);
    }
}
#[test]
fn application_rules_require_arming_and_fall_back_to_the_general_profile() {
    let document = with_rules();
    assert_eq!(
        document.effective_profile("unit-a", Some("game")).unwrap()["name"],
        "Game"
    );
    assert_eq!(
        document.effective_profile("unit-a", None).unwrap()["name"],
        "Work"
    );
    assert_eq!(
        document
            .effective_profile("unit-a", Some("unlisted"))
            .unwrap()["name"],
        "Work"
    );
    assert!(document
        .effective_profile("missing", Some("game"))
        .is_none());
    let disarmed = document
        .manage(&json!({"op":"auto","device":"unit-a","name":null}))
        .unwrap()
        .document;
    assert!(disarmed.effective_profile("unit-a", Some("game")).is_none());
    let armed = disarmed
        .manage(&json!({"op":"auto","device":"unit-a","name":"Game"}))
        .unwrap()
        .document;
    assert_eq!(
        armed.effective_profile("unit-a", None).unwrap()["name"],
        "Game"
    );
    assert!(armed
        .manage(&json!({"op":"auto","device":"unit-a","name":"missing"}))
        .is_err());
}
#[test]
fn setting_deleting_and_replacing_rules_never_arms_profiles() {
    let document = doc(base());
    let mut req = request("rule_set");
    req["application"] = json!("editor");
    let result = document.manage(&req).unwrap().document;
    assert_eq!(result.version(), 6);
    assert_eq!(result.0["active"], json!({}));
    assert_eq!(result.0["rules"].as_array().unwrap().len(), 1);
    let result = result.manage(&req).unwrap().document;
    assert_eq!(result.0["rules"].as_array().unwrap().len(), 1);
    req["op"] = json!("rule_delete");
    let result = result.manage(&req).unwrap().document;
    assert_eq!(result.0["rules"], json!([]));
}

#[test]
fn absent_store_reads_are_side_effect_free_and_keep_the_legacy_location_name() {
    let dir = Sandbox::new();
    let store = dir.store();
    let revision = store.load().unwrap();
    assert_eq!(revision.document(), &Document::default());
    assert!(!dir.0.join("linux-scrollmaster").exists());
    assert!(ProfileStore::at("relative/linux-scrollmaster").is_err());
}
#[test]
fn atomic_commit_roundtrips_private_files_and_preserves_metadata() {
    let dir = Sandbox::new();
    let store = dir.store();
    let absent = store.load().unwrap();
    let mut value = base();
    value["metadata"] = json!({"note":"conservar"});
    let document = doc(value);
    let revision = store.commit(&absent, &document).unwrap();
    assert_eq!(revision.document(), &document);
    assert_eq!(store.load().unwrap().document(), &document);
    assert_eq!(
        fs::metadata(dir.0.join("linux-scrollmaster"))
            .unwrap()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(dir.0.join("linux-scrollmaster/profiles.json"))
            .unwrap()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(dir.0.join("linux-scrollmaster/.profiles-native.lock"))
            .unwrap()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::read_dir(dir.0.join("linux-scrollmaster"))
            .unwrap()
            .count(),
        2
    );
}
#[test]
fn loading_legacy_profiles_does_not_rewrite_bytes_or_create_a_lock() {
    let dir = Sandbox::new();
    let path = dir.0.join("linux-scrollmaster");
    private_dir(&path).unwrap();
    let bytes = b"{ \"version\": 1, \"profiles\": [], \"active\": {} }\n";
    private_file(path.join("profiles.json"), bytes).unwrap();
    assert_eq!(dir.store().load().unwrap().document().version(), 1);
    assert_eq!(fs::read(path.join("profiles.json")).unwrap(), bytes);
    assert_eq!(fs::read_dir(path).unwrap().count(), 1);
}
#[test]
fn malformed_and_oversized_files_are_preserved_without_repair() {
    let dir = Sandbox::new();
    let path = dir.0.join("linux-scrollmaster");
    private_dir(&path).unwrap();
    for bytes in [
        b"{broken".to_vec(),
        vec![0xff],
        vec![b'x'; MAX_DOCUMENT_BYTES + 1],
        br#"{"version":99,"profiles":[],"active":{}}"#.to_vec(),
    ] {
        private_file(path.join("profiles.json"), &bytes).unwrap();
        assert!(dir.store().load().is_err());
        assert_eq!(fs::read(path.join("profiles.json")).unwrap(), bytes);
        assert_eq!(fs::read_dir(&path).unwrap().count(), 1);
    }
}
#[test]
fn stale_and_cross_store_revisions_cannot_overwrite_profiles() {
    let dir = Sandbox::new();
    let (store, old) = dir.seed();
    let stale = store.load().unwrap();
    let mut value = base();
    value["profiles"][0]["settings"]["dpi"] = json!(2400);
    let newer = doc(value);
    store.commit(&old, &newer).unwrap();
    assert_eq!(
        store.commit(&stale, &doc(base())).unwrap_err(),
        StoreError::Conflict
    );
    assert_eq!(store.load().unwrap().document(), &newer);
    let other = Sandbox::new();
    assert_eq!(
        other.store().commit(&old, &newer).unwrap_err(),
        StoreError::Conflict
    );
    assert!(!other.0.join("linux-scrollmaster").exists());
}
#[test]
fn whitespace_only_external_edits_are_detected_as_conflicts() {
    let dir = Sandbox::new();
    let (store, old) = dir.seed();
    let path = dir.0.join("linux-scrollmaster/profiles.json");
    let mut bytes = fs::read(&path).unwrap();
    bytes.push(b'\n');
    private_file(&path, &bytes).unwrap();
    assert_eq!(
        store.commit(&old, &doc(base())).unwrap_err(),
        StoreError::Conflict
    );
    assert_eq!(fs::read(path).unwrap(), bytes);
}
#[test]
fn failed_writes_before_publication_preserve_bytes_and_remove_temporary_files() {
    let dir = Sandbox::new();
    let (store, old) = dir.seed();
    let path = dir.0.join("linux-scrollmaster/profiles.json");
    let bytes = fs::read(&path).unwrap();
    let failure = store
        .commit_with(&old, &doc(base()), |stage| {
            assert_eq!(stage, Stage::Prepared);
            Err(std::io::Error::other("synthetic disk failure"))
        })
        .unwrap_err();
    assert!(!failure.may_have_published());
    assert_eq!(fs::read(path).unwrap(), bytes);
    assert_eq!(
        fs::read_dir(dir.0.join("linux-scrollmaster"))
            .unwrap()
            .count(),
        2
    );
}
#[test]
fn changes_during_temporary_writes_are_not_overwritten() {
    let dir = Sandbox::new();
    let (store, old) = dir.seed();
    let path = dir.0.join("linux-scrollmaster/profiles.json");
    let changed = b"changed externally";
    let error = store
        .commit_with(&old, &doc(base()), |stage| {
            assert_eq!(stage, Stage::Prepared);
            private_file(&path, changed)
        })
        .unwrap_err();
    assert_eq!(error, StoreError::Conflict);
    assert_eq!(fs::read(path).unwrap(), changed);
    assert_eq!(
        fs::read_dir(dir.0.join("linux-scrollmaster"))
            .unwrap()
            .count(),
        2
    );
}
#[test]
fn failure_after_rename_reports_publication_without_rolling_back() {
    let dir = Sandbox::new();
    let (store, old) = dir.seed();
    let mut data = base();
    data["profiles"][0]["name"] = json!("Published");
    let new = doc(data);
    let error = store
        .commit_with(&old, &new, |stage| {
            if stage == Stage::Published {
                Err(std::io::Error::other("synthetic directory sync failure"))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    assert!(error.may_have_published());
    assert_eq!(store.load().unwrap().document(), &new);
    assert_eq!(store.commit(&old, &new).unwrap_err(), StoreError::Conflict);
    assert_eq!(
        fs::read_dir(dir.0.join("linux-scrollmaster"))
            .unwrap()
            .count(),
        2
    );
}
#[test]
fn replaced_directories_invalidate_existing_revisions() {
    let dir = Sandbox::new();
    let (store, old) = dir.seed();
    fs::rename(dir.0.join("linux-scrollmaster"), dir.0.join("original")).unwrap();
    private_dir(dir.0.join("linux-scrollmaster")).unwrap();
    fs::copy(
        dir.0.join("original/profiles.json"),
        dir.0.join("linux-scrollmaster/profiles.json"),
    )
    .unwrap();
    assert_eq!(
        store.commit(&old, &doc(base())).unwrap_err(),
        StoreError::Conflict
    );
    assert_eq!(
        fs::read_dir(dir.0.join("linux-scrollmaster"))
            .unwrap()
            .count(),
        1
    );
}
#[test]
fn a_second_native_writer_is_rejected_while_the_first_holds_the_lock() {
    let dir = Sandbox::new();
    let (store, old) = dir.seed();
    store
        .commit_with(&old, &doc(base()), |stage| {
            if stage == Stage::Prepared {
                assert_eq!(
                    store.commit(&old, &doc(base())).unwrap_err(),
                    StoreError::Busy
                );
            }
            Ok(())
        })
        .unwrap();
}
#[test]
fn symlink_directory_file_and_lock_are_rejected_without_touching_targets() {
    for target in ["directory", "profiles.json", ".profiles-native.lock"] {
        let dir = Sandbox::new();
        let other = dir.0.join("other");
        private_dir(&other).unwrap();
        private_file(other.join("keep"), b"intact").unwrap();
        let store = dir.store();
        let absent = store.load().unwrap();
        if target == "directory" {
            symlink(&other, dir.0.join("linux-scrollmaster")).unwrap();
            assert!(store.load().is_err());
        } else {
            private_dir(dir.0.join("linux-scrollmaster")).unwrap();
            symlink(
                other.join("keep"),
                dir.0.join("linux-scrollmaster").join(target),
            )
            .unwrap();
        }
        assert!(store.commit(&absent, &doc(base())).is_err());
        assert_eq!(fs::read(other.join("keep")).unwrap(), b"intact");
    }
}
#[test]
fn unsafe_permissions_hard_links_and_nonregular_profile_files_are_rejected() {
    let dir = Sandbox::new();
    let (store, revision) = dir.seed();
    let path = dir.0.join("linux-scrollmaster/profiles.json");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
    assert!(store.load().is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::hard_link(&path, dir.0.join("alias")).unwrap();
    assert!(store.load().is_err());
    fs::remove_file(dir.0.join("alias")).unwrap();
    fs::set_permissions(
        dir.0.join("linux-scrollmaster"),
        fs::Permissions::from_mode(0o777),
    )
    .unwrap();
    assert!(store.commit(&revision, &doc(base())).is_err());
    fs::set_permissions(
        dir.0.join("linux-scrollmaster"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    fs::remove_file(&path).unwrap();
    private_dir(&path).unwrap();
    assert!(store.load().is_err());
}
#[test]
fn oversized_pretty_encoding_fails_before_creating_a_profile_directory() {
    let dir = Sandbox::new();
    let store = dir.store();
    let absent = store.load().unwrap();
    let mut value = base();
    value["padding"] = json!(vec![0; 40000]);
    let document = doc(value);
    assert!(document.encode().is_err());
    assert!(store.commit(&absent, &document).is_err());
    assert!(!dir.0.join("linux-scrollmaster").exists());
}

#[test]
fn shared_hand_authored_vectors_match_the_legacy_profile_contract() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/native-profiles.json")).unwrap();
    for case in fixture["documents"].as_array().unwrap() {
        let result = Document::from_value(case["document"].clone());
        assert_eq!(
            result.is_ok(),
            case["valid"].as_bool().unwrap(),
            "{}: {result:?}",
            case["name"]
        );
        if let Ok(document) = result {
            assert_eq!(document.as_value(), &case["document"]);
        }
    }
    for case in fixture["operations"].as_array().unwrap() {
        let original = doc(case["document"].clone());
        let result = original.manage(&case["request"]).unwrap();
        assert_eq!(
            result.document.as_value(),
            &case["result"],
            "{}",
            case["name"]
        );
        assert_eq!(
            result.transfer.unwrap_or(Value::Null),
            case["transfer"],
            "{}",
            case["name"]
        );
        assert_eq!(result.changed, case["changed"].as_bool().unwrap());
        assert_eq!(original.as_value(), &case["document"]);
    }
}

#[test]
fn directory_replacement_during_preparation_stops_publication() {
    let dir = Sandbox::new();
    let (store, old) = dir.seed();
    let original = fs::read(dir.0.join("linux-scrollmaster/profiles.json")).unwrap();
    let error = store
        .commit_with(&old, &doc(base()), |stage| {
            assert_eq!(stage, Stage::Prepared);
            fs::rename(dir.0.join("linux-scrollmaster"), dir.0.join("moved"))?;
            private_dir(dir.0.join("linux-scrollmaster"))
        })
        .unwrap_err();
    assert_eq!(error, StoreError::Conflict);
    assert!(!error.may_have_published());
    assert_eq!(
        fs::read(dir.0.join("moved/profiles.json")).unwrap(),
        original
    );
    assert_eq!(
        fs::read_dir(dir.0.join("linux-scrollmaster"))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(fs::read_dir(dir.0.join("moved")).unwrap().count(), 2);
}
#[test]
fn directory_replacement_after_rename_is_not_reported_as_confirmed() {
    let dir = Sandbox::new();
    let (store, old) = dir.seed();
    let error = store
        .commit_with(&old, &doc(base()), |stage| {
            if stage == Stage::Published {
                fs::rename(dir.0.join("linux-scrollmaster"), dir.0.join("moved"))?;
                private_dir(dir.0.join("linux-scrollmaster"))?;
            }
            Ok(())
        })
        .unwrap_err();
    assert!(error.may_have_published());
    assert!(!dir.0.join("linux-scrollmaster/profiles.json").exists());
    assert!(dir.0.join("moved/profiles.json").is_file());
}

#[test]
fn offline_store_import_and_rename_are_persisted_without_arming_profiles() {
    let dir = Sandbox::new();
    let store = dir.store();
    let mut incoming = base();
    incoming["active"] = json!({"unit-a":"Work"});
    let result = store
        .manage(&json!({"op":"import","device":"unit-a","document":incoming}))
        .unwrap();
    assert!(result.changed);
    assert_eq!(store.load().unwrap().document(), &result.document);
    assert_eq!(result.document.as_value()["active"], json!({}));
    let mut rename = request("rename");
    rename["new_name"] = json!("Persisted");
    let result = store.manage(&rename).unwrap();
    assert_eq!(
        store.load().unwrap().document().profiles()[0]["name"],
        "Persisted"
    );
    assert_eq!(result.document.as_value()["active"], json!({}));
}
#[test]
fn offline_store_previews_exports_and_noop_edits_do_not_rewrite_or_lock() {
    let dir = Sandbox::new();
    let store = dir.store();
    let preview = json!({"op":"import_preview","device":"unit-a","document":base()});
    assert!(!store.manage(&preview).unwrap().changed);
    assert!(!dir.0.join("linux-scrollmaster").exists());
    private_dir(dir.0.join("linux-scrollmaster")).unwrap();
    let path = dir.0.join("linux-scrollmaster/profiles.json");
    let mut bytes = doc(base()).encode().unwrap();
    bytes.push(b'\n');
    private_file(&path, &bytes).unwrap();
    let mut rename = request("rename");
    rename["new_name"] = json!("Work");
    for req in [preview, request("export"), rename] {
        assert!(!store.manage(&req).unwrap().changed);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(
            fs::read_dir(dir.0.join("linux-scrollmaster"))
                .unwrap()
                .count(),
            1
        );
    }
    assert!(store
        .manage(&json!({"op":"restore","device":"unit-a","name":"Work"}))
        .is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn unknown_numeric_metadata_keeps_its_precision_through_read_edit_and_commit() {
    let bytes = br#"{"version":1,"profiles":[{"device":"unit-a","name":"Work","settings":{"dpi":800},"counter":18446744073709551617001}],"active":{},"decimal":0.12345678901234567890123456789}"#;
    let document = Document::parse(bytes).unwrap();
    let dir = Sandbox::new();
    let store = dir.store();
    store.commit(&store.load().unwrap(), &document).unwrap();
    let mut rename = request("rename");
    rename["new_name"] = json!("Renamed");
    let result = store.manage(&rename).unwrap().document;
    assert_eq!(
        result.as_value()["profiles"][0]["counter"].to_string(),
        "18446744073709551617001"
    );
    assert_eq!(
        result.as_value()["decimal"].to_string(),
        "0.12345678901234567890123456789"
    );
    let disk = fs::read_to_string(dir.0.join("linux-scrollmaster/profiles.json")).unwrap();
    assert!(disk.contains("18446744073709551617001"));
    assert!(disk.contains("0.12345678901234567890123456789"));
}
