// SPDX-License-Identifier: GPL-3.0-or-later
//! Focus-selected profiles -> service -> reconnecting owner -> resident capture.
//! Temporary profile files and synthetic HID++ only; no compositor or hardware.
use super::*;
use crate::{
    native::{NativeProfileOwner, ProfileConnector},
    profiles::{Document, ProfileStore},
    service::ProfileService,
};
use std::{fs, os::unix::fs::DirBuilderExt, path::PathBuf};

const ID: &str = "11223344";
struct Sandbox(PathBuf);
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
// Every connection shares one synthetic mouse, like reopening the same hidraw.
struct Synthetic {
    state: Rc<RefCell<State>>,
    log: Rc<RefCell<Vec<String>>>,
    drop_clear: Rc<Cell<bool>>,
    connections: Rc<Cell<usize>>,
}
impl ProfileConnector for Synthetic {
    type Backend = GuardedProfileBackend<Observed>;
    type Lease = ();
    fn validate_changes(&self, _: &Changes) -> Result<(), String> {
        Ok(())
    }
    fn check_controllers(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn acquire(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn connect(&mut self, slot: u8, id: &str, _: Instant) -> Result<Self::Backend, String> {
        self.connections.set(self.connections.get() + 1);
        let (mut device, _) = Device::new();
        device.state = self.state.clone();
        device.base.accept_writes = true;
        let machine = Machine {
            device,
            pending: None,
            events: Default::default(),
            idle: Default::default(),
            faults: Faults::default(),
            round: 0,
            report_reads: Default::default(),
            cancelled: Rc::new(Cell::new(false)),
            enabled: false,
        };
        let output_log = self.log.clone();
        GuardedProfileBackend::with_output(
            Observed {
                machine,
                log: self.log.clone(),
                drop_clear: self.drop_clear.clone(),
            },
            slot,
            id,
            Cancellation::default(),
            move |bindings, dpi| {
                output_log.borrow_mut().push("output-open".into());
                Runtime::new(
                    OutputState {
                        log: output_log.clone(),
                        fail_event: Rc::new(Cell::new(false)),
                        fail_close: Rc::new(Cell::new(false)),
                    },
                    bindings,
                    dpi,
                )
                .map(SinkHandle::new)
            },
        )
    }
}
struct Harness {
    _dir: Sandbox,
    service: ProfileService<NativeProfileOwner<Synthetic>>,
    state: Rc<RefCell<State>>,
    log: Rc<RefCell<Vec<String>>>,
    drop_clear: Rc<Cell<bool>>,
    connections: Rc<Cell<usize>>,
}
impl Harness {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "scrollmaster-capture-service-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        let store = ProfileStore::at(path.join("linux-scrollmaster")).unwrap();
        let document = Document::from_value(json!({"version":10,"profiles":[
            {"device":ID,"name":"Work","settings":{"dpi":1800},"buttons":{"83":"modifier:ctrl"}},
            {"device":ID,"name":"Game","settings":{"dpi":1800},"buttons":{"83":"modifier:alt"}},
            {"device":ID,"name":"Plain","settings":{"dpi":1800},"buttons":{"83":83,"86":81}}
        ],"active":{ID:"Work"},"rules":[
            {"device":ID,"application":"GameApp","profile":"Game"},
            {"device":ID,"application":"Editor","profile":"Plain"}
        ]}))
        .unwrap();
        store.commit(&store.load().unwrap(), &document).unwrap();
        let (_, state) = Device::new();
        let (log, drop_clear, connections) = Default::default();
        let connector = Synthetic {
            state: state.clone(),
            log: Rc::clone(&log),
            drop_clear: Rc::clone(&drop_clear),
            connections: Rc::clone(&connections),
        };
        let owner = NativeProfileOwner::new(connector, 1, ID).unwrap();
        Self {
            _dir: Sandbox(path),
            service: ProfileService::new(store, owner, Duration::from_secs(5)).unwrap(),
            state,
            log,
            drop_clear,
            connections,
        }
    }
    fn focus(&mut self, application: Option<&str>) -> Value {
        self.service
            .handle(&json!({"op":"snapshot"}), application)
            .unwrap()
    }
    fn count(&self, entry: &str) -> usize {
        self.log.borrow().iter().filter(|s| *s == entry).count()
    }
}
fn control<'a>(reply: &'a Value, id: &str) -> &'a Value {
    reply["devices"][0]["buttons"]["controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id)
        .unwrap()
}

#[test]
fn focus_changes_switch_software_and_native_plans_under_one_session() {
    let mut h = Harness::new();
    let work = h.focus(None);
    assert_eq!(work["reconciliation"][0]["profile"], "Work");
    assert_eq!(control(&work, "83")["value"], "modifier:ctrl");
    assert_eq!(h.state.borrow().flags.get(&83), Some(&1));

    let game = h.focus(Some("GameApp"));
    assert_eq!(game["reconciliation"][0]["profile"], "Game");
    assert_eq!(control(&game, "83")["value"], "modifier:alt");
    assert_eq!(h.count("wire:83:2"), 1);
    assert_eq!(h.count("wire:83:3"), 2);

    let plain = h.focus(Some("Editor"));
    assert_eq!(plain["reconciliation"][0]["profile"], "Plain");
    assert_eq!(plain["devices"][0]["native_capture"]["active"], false);
    assert_eq!(h.state.borrow().flags.get(&83), Some(&0));
    assert_eq!(h.state.borrow().targets.get(&86), Some(&81));

    // Unmatched focus returns to the armed general profile and re-captures.
    let back = h.focus(Some("Other"));
    assert_eq!(back["reconciliation"][0]["profile"], "Work");
    assert_eq!(control(&back, "83")["value"], "modifier:ctrl");
    // Same focus again: identical plan, no capture setter is repeated.
    h.focus(Some("Other"));
    assert_eq!(h.count("wire:83:3"), 3);
    assert_eq!(h.connections.get(), 1);
    assert_eq!(h.count("descriptor-close"), 0);

    h.service.shutdown().unwrap();
    assert_eq!(h.state.borrow().flags.get(&83), Some(&0));
    assert_eq!(h.count("descriptor-close"), 1);
    assert_eq!(h.count("output-open"), h.count("output-close"));
}
#[test]
fn failed_transition_restores_and_only_the_next_request_reconnects() {
    let mut h = Harness::new();
    h.focus(None);
    h.drop_clear.set(true);
    let failed = h.focus(Some("GameApp"));
    assert!(failed["devices"].as_array().unwrap().is_empty());
    assert!(failed["reconciliation_errors"][0]["error"]
        .as_str()
        .unwrap()
        .contains("release unconfirmed"));
    // Terminal cleanup restored through the retained descriptor; nothing replayed.
    assert_eq!(h.state.borrow().flags.get(&83), Some(&0));
    assert_eq!(h.count("descriptor-close"), 1);
    assert_eq!(h.count("wire:83:3"), 1);
    assert_eq!(h.connections.get(), 1);

    // A new request is a fresh preflight on a fresh session, not a retry.
    let game = h.focus(Some("GameApp"));
    assert_eq!(game["reconciliation"][0]["profile"], "Game");
    assert_eq!(control(&game, "83")["value"], "modifier:alt");
    assert_eq!(h.connections.get(), 2);
    h.service.shutdown().unwrap();
    assert_eq!(h.state.borrow().flags.get(&83), Some(&0));
}
