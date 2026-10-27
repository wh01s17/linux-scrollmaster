// SPDX-License-Identifier: GPL-3.0-or-later
//! Host policy with temporary profiles and scripted worker processes only.
use super::*;
use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    process::Command,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
};

const ID: &str = "11223344";
struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "scrollmaster-native-host-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        let store = ProfileStore::at(path.join("linux-scrollmaster")).unwrap();
        let document = crate::profiles::Document::from_value(json!({"version":6,
            "profiles":[{"device":ID,"name":"Work","settings":{"dpi":1800}}],"active":{}}))
        .unwrap();
        store.commit(&store.load().unwrap(), &document).unwrap();
        Self(path)
    }
    fn store(&self) -> impl Fn() -> Result<ProfileStore, String> + Send + 'static {
        let path = self.0.join("linux-scrollmaster");
        move || ProfileStore::at(path.clone()).map_err(|e| e.to_string())
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
// A scripted JSON-lines worker: snapshot -> device, shutdown -> stopped,
// anything else -> a local reply without devices (as the native worker does).
fn scripted() -> Result<Worker, String> {
    let device = r#"{"id":"11223344","name":"MX Master 3S","connection":"Logi Bolt","protocol":{"major":4,"minor":5},"battery":{"percentage":70},"settings":{},"buttons":{"controls":[],"error":null}}"#;
    let script = format!(
        r#"while IFS= read -r line; do case "$line" in
*'"snapshot"'*) printf '%s\n' '{{"ok":true,"data":{{"devices":[{device}],"profiles":[],"active":{{}},"warnings":[],"rules":[]}}}}';;
*'"shutdown"'*) printf '%s\n' '{{"ok":true,"data":{{"stopped":true}}}}'; exit 0;;
*'"exit"'*) exit 1;;
*) printf '%s\n' '{{"ok":true,"data":{{"devices":[],"profiles":[],"active":{{}},"warnings":[],"rules":[]}}}}';;
esac; done"#
    );
    let mut command = Command::new("/bin/sh");
    command.args(["-c", &script]);
    Worker::start(command)
}
fn host(dir: &Sandbox, online: bool) -> (NativeHost, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let (located, launched) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let (l, s) = (located.clone(), launched.clone());
    let host = NativeHost::new(
        move || {
            l.fetch_add(1, Ordering::SeqCst);
            if online {
                Ok(Located {
                    slot: 2,
                    identity: ID.into(),
                })
            } else {
                Err("No se encontró un receptor Logi Bolt compatible.".into())
            }
        },
        move |_| {
            s.fetch_add(1, Ordering::SeqCst);
            scripted()
        },
        || json!({"available":true,"error":null}),
        dir.store(),
    );
    (host, located, launched)
}
fn timeout() -> Duration {
    Duration::from_secs(5)
}

#[test]
fn offline_snapshot_lists_stored_profiles_with_a_diagnostic_and_no_device() {
    let dir = Sandbox::new();
    let (mut host, located, launched) = host(&dir, false);
    let reply = host.request(&json!({"op":"snapshot"}), timeout()).unwrap();
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["data"]["devices"], json!([]));
    assert_eq!(reply["data"]["profiles"][0]["name"], "Work");
    assert!(reply["data"]["diagnostic"]
        .as_str()
        .unwrap()
        .contains("receptor"));
    // Discovery is rate-limited while the mouse stays absent.
    host.request(&json!({"op":"snapshot"}), timeout()).unwrap();
    assert_eq!(located.load(Ordering::SeqCst), 1);
    assert_eq!(launched.load(Ordering::SeqCst), 0);
}

#[test]
fn startup_recovery_waits_for_a_mouse_and_is_forwarded_to_the_first_online_poll() {
    let dir = Sandbox::new();
    let (mut host, _, _) = host(&dir, false);
    host.request(&json!({"op":"snapshot","resume":true}), timeout())
        .unwrap();
    assert!(host.resume_pending);
    host.locate = Box::new(|| {
        Ok(Located {
            slot: 2,
            identity: ID.into(),
        })
    });
    host.last_failure = None;
    let script = r#"while IFS= read -r line; do case "$line" in
*'"shutdown"'*) printf '%s\n' '{"ok":true,"data":{"stopped":true}}'; exit 0;;
*'"resume":true'*) printf '%s\n' '{"ok":true,"data":{"devices":[],"profiles":[],"active":{},"warnings":[],"resume_seen":true}}';;
*) printf '%s\n' '{"ok":true,"data":{"devices":[],"profiles":[],"active":{},"warnings":[],"resume_seen":false}}';;
esac; done"#;
    host.launch = Box::new(move |_| {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]);
        Worker::start(command)
    });
    let resumed = host.request(&json!({"op":"snapshot"}), timeout()).unwrap();
    assert_eq!(resumed["data"]["resume_seen"], true);
    assert!(!host.resume_pending);
    let next = host.request(&json!({"op":"snapshot"}), timeout()).unwrap();
    assert_eq!(next["data"]["resume_seen"], false);
}
#[test]
fn offline_local_management_works_but_hardware_operations_are_refused() {
    let dir = Sandbox::new();
    let (mut host, _, launched) = host(&dir, false);
    let renamed = host
        .request(
            &json!({"op":"rename","device":ID,"name":"Work","new_name":"Office"}),
            timeout(),
        )
        .unwrap();
    assert_eq!(renamed["ok"], true, "{renamed}");
    assert_eq!(renamed["data"]["profiles"][0]["name"], "Office");
    let stored = (dir.store())().unwrap().load().unwrap();
    assert_eq!(stored.document().profiles()[0]["name"], "Office");
    let set = host
        .request(
            &json!({"op":"set","device":ID,"changes":{"dpi":1600}}),
            timeout(),
        )
        .unwrap();
    assert_eq!(set["ok"], false);
    assert!(set["error"].as_str().unwrap().contains("receptor"));
    // The same strict request validation as the worker applies offline.
    let extra = host
        .request(
            &json!({"op":"rename","device":ID,"name":"Office","new_name":"X","path":"/tmp"}),
            timeout(),
        )
        .unwrap();
    assert_eq!(extra["ok"], false);
    assert_eq!(launched.load(Ordering::SeqCst), 0);
}
#[test]
fn online_replies_follow_the_vue_contract_and_local_ops_keep_the_last_reading() {
    let dir = Sandbox::new();
    let (mut host, located, launched) = host(&dir, true);
    let reply = host.request(&json!({"op":"snapshot"}), timeout()).unwrap();
    let mouse = &reply["data"]["devices"][0];
    assert_eq!(mouse["battery"], 70);
    assert_eq!(mouse["protocol"], 4.5);
    assert_eq!(reply["data"]["diagnostic"], Value::Null);
    let local = host
        .request(
            &json!({"op":"rule_delete","device":ID,"application":"x"}),
            timeout(),
        )
        .unwrap();
    assert_eq!(local["data"]["devices"][0]["id"], ID);
    assert_eq!(located.load(Ordering::SeqCst), 1);
    assert_eq!(launched.load(Ordering::SeqCst), 1);
    host.shutdown().unwrap();
}
#[test]
fn a_failed_worker_is_dropped_never_retried_and_the_next_request_relocates() {
    let dir = Sandbox::new();
    let (mut host, located, launched) = host(&dir, true);
    host.request(&json!({"op":"snapshot"}), timeout()).unwrap();
    assert!(host.request(&json!({"op":"exit"}), timeout()).is_err());
    let reply = host.request(&json!({"op":"snapshot"}), timeout()).unwrap();
    assert_eq!(reply["data"]["devices"][0]["id"], ID);
    assert_eq!(located.load(Ordering::SeqCst), 2);
    assert_eq!(launched.load(Ordering::SeqCst), 2);
}
#[test]
fn backend_selection_is_explicit_and_unknown_values_keep_the_build_default() {
    assert!(selection(Some("native")));
    assert!(!selection(Some("solaar")));
    assert_eq!(selection(None), cfg!(feature = "native-default"));
    assert_eq!(selection(Some("Native")), cfg!(feature = "native-default"));
}
#[test]
fn offline_application_catalogue_still_comes_from_the_read_only_focus_source() {
    struct Catalogue;
    impl crate::native::DesktopSource for Catalogue {
        fn describe(&mut self, include_apps: bool, _: Instant) -> crate::native::DesktopSnapshot {
            assert!(include_apps);
            crate::native::DesktopSnapshot::from_values(
                &json!({"class":"Editor"}),
                Some(&json!([{"class":"Editor"},{"class":"GameApp"}])),
            )
            .unwrap()
        }
    }
    let dir = Sandbox::new();
    let (host, _, launched) = host(&dir, false);
    let mut host = host.with_focus(|| Box::new(Catalogue));
    let reply = host
        .request(&json!({"op":"applications","device":ID}), timeout())
        .unwrap();
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(
        reply["data"]["desktop"]["applications"],
        json!(["Editor", "GameApp"])
    );
    assert_eq!(launched.load(Ordering::SeqCst), 0);
}
