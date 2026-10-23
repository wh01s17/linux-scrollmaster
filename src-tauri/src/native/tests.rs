// SPDX-License-Identifier: GPL-3.0-or-later
//! Synthetic protocol emulator and filesystem fixtures; never opens real hardware.
use super::*;
use crate::hidpp::Transport;
use serde_json::Value;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs, io,
    os::unix::fs::{symlink, DirBuilderExt, PermissionsExt},
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

// Hand-authored dedicated vendor descriptor. Layout matches the passive C548
// sysfs report descriptor inspected on 2026-10-20. See docs/native-diagnostic.md.
const DESCRIPTOR: &[u8] = &[
    0x06, 0x00, 0xff, 0x09, 0x01, 0xa1, 0x01, 0x85, 0x10, 0x95, 0x06, 0x75, 0x08, 0x15, 0x00, 0x26,
    0xff, 0x00, 0x09, 0x01, 0x81, 0x00, 0x09, 0x01, 0x91, 0x00, 0xc0, 0x06, 0x00, 0xff, 0x09, 0x02,
    0xa1, 0x01, 0x85, 0x11, 0x95, 0x13, 0x75, 0x08, 0x15, 0x00, 0x26, 0xff, 0x00, 0x09, 0x02, 0x81,
    0x00, 0x09, 0x02, 0x91, 0x00, 0xc0,
];
static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "scrollmaster-n2-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
    fn interface(&self, name: &str, id: &str, descriptor: &[u8]) {
        let path = self.0.join(name);
        fs::create_dir_all(path.join("device")).unwrap();
        fs::write(
            path.join("device/uevent"),
            format!("HID_ID={id}\nHID_NAME=Test Receiver\n"),
        )
        .unwrap();
        fs::write(path.join("device/report_descriptor"), descriptor).unwrap();
        fs::write(path.join("dev"), "240:7\n").unwrap();
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn descriptor_accepts_only_exact_bidirectional_reports() {
    descriptor::validate(DESCRIPTOR).unwrap();
    for (offset, value) in [(2, 0x01), (8, 0x12), (10, 0x07), (12, 0x07), (24, 0x81)] {
        let mut broken = DESCRIPTOR.to_vec();
        broken[offset] = value;
        assert!(descriptor::validate(&broken).is_err(), "offset {offset}");
    }
}
#[test]
fn descriptor_rejects_truncation_unbalanced_stack_and_long_items() {
    for n in 0..DESCRIPTOR.len() {
        assert!(descriptor::validate(&DESCRIPTOR[..n]).is_err());
    }
    for suffix in [vec![0xa4], vec![0xb4], vec![0xc0], vec![0xfe, 0, 0]] {
        let mut bytes = DESCRIPTOR.to_vec();
        bytes.extend_from_slice(&suffix);
        assert!(descriptor::validate(&bytes).is_err());
    }
    assert!(descriptor::validate(&vec![0; 4097]).is_err());
}
#[test]
fn discovery_filters_bus_vendor_product_and_wrong_interfaces() {
    let root = Temp::new();
    root.interface("hidraw7", "0003:0000046D:0000C548", DESCRIPTOR);
    root.interface("hidraw0", "0005:0000046D:0000C548", DESCRIPTOR);
    root.interface("hidraw1", "0003:0000046D:0000C52B", DESCRIPTOR);
    root.interface("hidraw2", "0003:000003F0:0000C548", DESCRIPTOR);
    root.interface("hidraw3", "0003:0000046D:0000C548", &[0x05, 1, 0x09, 6]);
    root.interface("hidrawbad", "0003:0000046D:0000C548", DESCRIPTOR);
    let result = discovery::scan(&root.0, &root.0).unwrap();
    assert_eq!(result.candidates.len(), 1);
    assert_eq!(result.candidates[0].name(), "hidraw7");
    assert!(result.warnings.is_empty());
}
#[test]
fn discovery_warns_on_incomplete_metadata_without_inventing_a_device() {
    let root = Temp::new();
    root.interface("hidraw7", "0003:0000046D:0000C548", DESCRIPTOR);
    fs::write(root.0.join("hidraw7/dev"), "broken").unwrap();
    let result = discovery::scan(&root.0, &root.0).unwrap();
    assert!(result.candidates.is_empty());
    assert_eq!(result.warnings.len(), 1);
}
#[test]
fn open_rejects_recycled_interface_and_regular_file() {
    let root = Temp::new();
    root.interface("hidraw7", "0003:0000046D:0000C548", DESCRIPTOR);
    let candidate = discovery::scan(&root.0, &root.0)
        .unwrap()
        .candidates
        .remove(0);
    fs::write(
        root.0.join("hidraw7/device/uevent"),
        "HID_ID=0003:0000046D:0000C548\nHID_NAME=Changed\n",
    )
    .unwrap();
    assert!(candidate.open().err().unwrap().contains("changed"));
    // Original metadata restored, but its node is a directory, not a character device.
    root.interface("hidraw7", "0003:0000046D:0000C548", DESCRIPTOR);
    assert!(candidate.open().is_err());
}
#[test]
fn selection_never_accepts_arbitrary_paths() {
    for name in [
        "/dev/hidraw7",
        "../hidraw7",
        "hidraw",
        "hidraw7/evil",
        "hidraw-1",
        "hidraw７",
    ] {
        assert!(!discovery::valid_name(name));
    }
    assert!(discovery::valid_name("hidraw7"));
}
#[test]
fn shared_lease_excludes_other_owners_and_releases_on_drop() {
    let root = Temp::new();
    let lease = HardwareLease::at(&root.0).unwrap();
    assert!(HardwareLease::at(&root.0).is_err());
    drop(lease);
    assert!(HardwareLease::at(&root.0).is_ok());
}
#[test]
fn lease_preserves_files_and_rejects_symlinks_or_unsafe_directory() {
    let root = Temp::new();
    let other = root.0.join("sentinel");
    fs::write(&other, "keep").unwrap();
    symlink(&other, root.0.join("linux-scrollmaster-hardware.lock")).unwrap();
    assert!(HardwareLease::at(&root.0).is_err());
    assert_eq!(fs::read_to_string(other).unwrap(), "keep");
    fs::set_permissions(&root.0, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(HardwareLease::at(&root.0).is_err());
}
#[test]
fn process_guard_blocks_controllers_and_orphan_worker() {
    for name in ["solaar", "logid", "linux-scrollmas", "linux-scrollmaster"] {
        let root = Temp::new();
        fs::create_dir(root.0.join("123")).unwrap();
        fs::write(root.0.join("123/comm"), name).unwrap();
        assert!(access::check_processes(&root.0).is_err());
    }
    let root = Temp::new();
    fs::create_dir(root.0.join("123")).unwrap();
    fs::write(root.0.join("123/comm"), "python").unwrap();
    fs::write(
        root.0.join("123/cmdline"),
        b"python\0-I\0/project/bridge/service.py\0--serve\0",
    )
    .unwrap();
    assert!(access::check_processes(&root.0).is_err());
    fs::write(root.0.join("123/cmdline"), b"python\0unrelated.py\0").unwrap();
    assert!(access::check_processes(&root.0).is_ok());
}
#[test]
fn own_process_chain_is_not_a_competing_controller_but_others_are() {
    let root = Temp::new();
    // 300 = guardian child, 200 = native worker, 100 = GUI; 150 = another GUI.
    for (pid, comm, ppid) in [
        (300, "linux-scrollmas", 200),
        (200, "linux-scrollmas", 100),
        (100, "linux-scrollmas", 1),
        (150, "linux-scrollmas", 1),
    ] {
        fs::create_dir(root.0.join(pid.to_string())).unwrap();
        fs::write(root.0.join(format!("{pid}/comm")), comm).unwrap();
        fs::write(
            root.0.join(format!("{pid}/stat")),
            format!("{pid} (linux scroll) S {ppid} 1 1"),
        )
        .unwrap();
    }
    let chain = access::ancestors(&root.0, 300);
    assert_eq!(chain, vec![300, 200, 100]);
    assert!(access::check_processes_except(&root.0, &chain).is_err());
    fs::remove_dir_all(root.0.join("150")).unwrap();
    assert!(access::check_processes_except(&root.0, &chain).is_ok());
    // Without the exclusion every instance is still a controller.
    assert!(access::check_processes(&root.0).is_err());
}
#[test]
fn processes_exiting_during_the_scan_are_skipped_not_fatal() {
    // Observed on hardware: reading comm of an exiting process gives ESRCH.
    let esrch = std::io::Error::from_raw_os_error(libc::ESRCH);
    assert!(access::vanished(&esrch));
    assert!(access::vanished(&std::io::ErrorKind::NotFound.into()));
    assert!(!access::vanished(&std::io::Error::from_raw_os_error(
        libc::EACCES
    )));
}

type Wire = Rc<RefCell<Vec<Vec<u8>>>>;
pub(super) struct Mouse {
    pub(super) features: BTreeMap<u16, u8>,
    overrides: BTreeMap<(u16, u8), Vec<u8>>,
    flags: BTreeMap<u16, u8>,
    reply: Option<Vec<u8>>,
    wire: Wire,
    bad_echo: bool,
    truncate_info: bool,
    disconnect_at: Option<(u16, u8)>,
    pub(super) accept_writes: bool,
    ignore_writes: bool,
}
impl Mouse {
    pub(super) fn new() -> Self {
        Self {
            features: [
                (0x0003, 2),
                (0x0005, 4),
                (0x1004, 6),
                (0x2110, 8),
                (0x2121, 10),
                (0x2150, 12),
                (0x2201, 14),
            ]
            .into(),
            overrides: BTreeMap::new(),
            flags: BTreeMap::new(),
            reply: None,
            wire: Rc::default(),
            bad_echo: false,
            truncate_info: false,
            disconnect_at: None,
            accept_writes: false,
            ignore_writes: false,
        }
    }
    fn value(&self, id: u16, function: u8, request: &[u8]) -> Vec<u8> {
        if let Some(value) = self.overrides.get(&(id, function)) {
            return value.clone();
        }
        match (id, function) {
            (0x0003, 0) => vec![
                1, 0x11, 0x22, 0x33, 0x44, 0, 6, 0xb0, 0x34, 0xb0, 0x34, 0, 0,
            ],
            (0x0005, 0) => vec![12],
            (0x0005, 1) => b"MX Master 3S"[request[4] as usize..].to_vec(),
            (0x1004, 1) => vec![40, 4, 0, 0],
            (0x2110, 0) => vec![2, 10],
            (0x2121, 0) => vec![8, 0x0c],
            (0x2121, 1) => vec![2],
            (0x2150, 1) => vec![0, 1],
            (0x2201, 0) => vec![1],
            (0x2201, 1) => vec![0, 0, 200, 0xe0, 50, 0x1f, 0x40, 0, 0],
            (0x2201, 2) => vec![0, 0x07, 0x3a, 0x03, 0x20],
            other => panic!("Unexpected query {other:?}"),
        }
    }
}
impl Transport for Mouse {
    fn write_report(&mut self, request: &[u8], _: Instant) -> io::Result<()> {
        self.wire.borrow_mut().push(request.to_vec());
        let mut reply = vec![0; 20];
        reply[..4].copy_from_slice(&request[..4]);
        reply[0] = 0x11;
        let function = request[3] >> 4;
        let payload = if request[2] == 0 {
            if function == 1 {
                vec![4, 5, request[6] ^ u8::from(self.bad_echo)]
            } else {
                assert_eq!(function, 0);
                let id = u16::from_be_bytes([request[4], request[5]]);
                vec![
                    *self.features.get(&id).unwrap_or(&0),
                    *self.flags.get(&id).unwrap_or(&0),
                    2,
                ]
            }
        } else {
            let id = *self
                .features
                .iter()
                .find(|(_, index)| **index == request[2])
                .unwrap()
                .0;
            if self.disconnect_at == Some((id, function)) {
                return Err(io::ErrorKind::BrokenPipe.into());
            }
            if self.accept_writes
                && matches!(
                    (id, function),
                    (0x2201, 3) | (0x2110, 1) | (0x2121, 2) | (0x2150, 2)
                )
            {
                if !self.ignore_writes {
                    let (getter, value) = match id {
                        0x2201 => (2, vec![request[4], request[5], request[6], 3, 32]),
                        0x2110 => {
                            let mut p = self.value(id, 0, request);
                            if request[4] != 0 {
                                p[0] = request[4];
                            }
                            if request[5] != 0 {
                                p[1] = request[5];
                            }
                            (0, p)
                        }
                        0x2121 => (1, vec![request[4]]),
                        0x2150 => (1, vec![request[4], request[5]]),
                        _ => unreachable!(),
                    };
                    self.overrides.insert((id, getter), value);
                }
                self.reply = Some(reply);
                return Ok(());
            }
            if self.truncate_info && id == 0x0003 {
                reply = vec![0; 7];
                reply[..4].copy_from_slice(&request[..4]);
                reply[0] = 0x10;
                self.reply = Some(reply);
                return Ok(());
            }
            self.value(id, function, request)
        };
        reply[4..4 + payload.len()].copy_from_slice(&payload);
        self.reply = Some(reply);
        Ok(())
    }
    fn read_report(&mut self, _: Instant) -> io::Result<Vec<u8>> {
        self.reply.take().ok_or(io::ErrorKind::UnexpectedEof.into())
    }
}
fn read(mouse: Mouse) -> Result<Value, String> {
    diagnose(mouse, 1, Instant::now() + Duration::from_secs(1))
}

#[test]
fn diagnostic_reads_identity_battery_and_six_settings_with_fixed_getters() {
    let mouse = Mouse::new();
    let wire = mouse.wire.clone();
    let data = read(mouse).unwrap();
    assert_eq!(data["device"]["id"], "11223344");
    assert_eq!(data["device"]["name"], "MX Master 3S");
    assert_eq!(data["device"]["battery"]["percentage"], 40);
    let settings = &data["device"]["settings"];
    for (key, value) in [("dpi", 1850), ("smart-shift", 10), ("scroll-ratchet", 2)] {
        assert_eq!(settings[key]["value"], value);
    }
    assert_eq!(settings["hires-smooth-invert"]["value"], false);
    assert_eq!(settings["hires-smooth-resolution"]["value"], true);
    assert_eq!(settings["thumb-scroll-invert"]["value"], true);
    assert_eq!(settings["dpi"]["choices"].as_array().unwrap().len(), 157);
    for request in wire.borrow().iter() {
        assert_eq!(request[1], 1);
        assert_eq!(request[0], if request[2] == 0 { 0x10 } else { 0x11 });
        let allowed = match request[2] {
            0 => vec![0, 1],
            2 => vec![0],
            4 => vec![0, 1],
            6 => vec![1],
            8 => vec![0],
            10 => vec![0, 1],
            12 => vec![1],
            14 => vec![0, 1, 2],
            _ => panic!("unadvertised index"),
        };
        assert!(
            allowed.contains(&(request[3] >> 4)),
            "setter leaked onto wire: {request:?}"
        );
    }
    assert_eq!(data["read_only"], true);
    assert_eq!(data["experimental"], true);
}
#[test]
fn absent_or_hidden_features_are_unavailable_without_guessing_values() {
    let mut mouse = Mouse::new();
    mouse.features.remove(&0x2201);
    mouse.flags.insert(0x2150, 0x40);
    let data = read(mouse).unwrap();
    for key in ["dpi", "thumb-scroll-invert"] {
        assert_eq!(data["device"]["settings"][key]["available"], false);
        assert!(data["device"]["settings"][key]["value"].is_null());
    }
}
#[test]
fn bad_ping_or_truncated_identity_stops_before_setting_queries() {
    for truncated in [false, true] {
        let mut mouse = Mouse::new();
        mouse.bad_echo = !truncated;
        mouse.truncate_info = truncated;
        let wire = mouse.wire.clone();
        assert!(read(mouse).is_err());
        assert!(wire.borrow().iter().all(|p| p[2] == 0 || p[2] == 2));
    }
}
#[test]
fn wrong_model_or_missing_unit_id_is_rejected() {
    for info in [
        vec![1, 0, 0, 0, 0, 0, 4, 0xb0, 0x34, 0, 0, 0, 0],
        vec![1, 1, 2, 3, 4, 0, 4, 0xb0, 0x35, 0, 0, 0, 0],
    ] {
        let mut mouse = Mouse::new();
        mouse.overrides.insert((0x0003, 0), info);
        assert!(read(mouse).is_err());
    }
}
#[test]
fn unknown_device_name_and_duplicate_feature_indices_are_rejected() {
    let mut mouse = Mouse::new();
    mouse
        .overrides
        .insert((0x0005, 1), b"Other mouse!".to_vec());
    assert!(read(mouse).is_err());
    let mut mouse = Mouse::new();
    mouse.features.insert(0x2201, 10);
    assert!(read(mouse).unwrap_err().contains("Duplicate"));
}
#[test]
fn disconnect_during_optional_reads_invalidates_the_snapshot() {
    let mut mouse = Mouse::new();
    mouse.disconnect_at = Some((0x2121, 1));
    assert!(read(mouse).unwrap_err().contains("transport failed"));
}
#[test]
fn invalid_battery_and_sensor_echo_do_not_become_plausible_values() {
    let mut mouse = Mouse::new();
    mouse.overrides.insert((0x1004, 1), vec![255, 0, 0, 0]);
    mouse.overrides.insert((0x2201, 2), vec![1, 7, 58, 3, 32]);
    let data = read(mouse).unwrap();
    assert_eq!(data["device"]["battery"]["available"], false);
    assert_eq!(data["device"]["settings"]["dpi"]["available"], false);
}
#[test]
fn free_spin_marks_threshold_as_hidden_and_unknown_battery_stays_unknown() {
    let mut mouse = Mouse::new();
    mouse.overrides.insert((0x2110, 0), vec![1, 255]);
    mouse.overrides.insert((0x1004, 1), vec![0, 4, 0, 0]);
    let data = read(mouse).unwrap();
    assert_eq!(data["device"]["settings"]["smart-shift"]["value"], 1);
    assert_eq!(
        data["device"]["settings"]["smart-shift"]["threshold_hidden_in_freespin"],
        true
    );
    assert!(data["device"]["battery"]["percentage"].is_null());
}
#[test]
fn dpi_default_value_is_used_only_when_announced() {
    let mut mouse = Mouse::new();
    mouse.overrides.insert((0x2201, 2), vec![0, 0, 0, 3, 32]);
    assert_eq!(
        read(mouse).unwrap()["device"]["settings"]["dpi"]["value"],
        800
    );
    let mut mouse = Mouse::new();
    mouse.overrides.insert((0x2201, 2), vec![0, 0, 0, 3, 33]);
    assert_eq!(
        read(mouse).unwrap()["device"]["settings"]["dpi"]["available"],
        false
    );
}
#[test]
fn dpi_ranges_reject_zero_step_truncation_and_unterminated_lists() {
    for bytes in [
        vec![0xe0, 50, 0, 0],
        vec![0, 200, 0xe0, 0, 1, 244, 0, 0],
        vec![0, 200, 0xe0, 50],
        vec![0, 200, 0xe0, 50, 0, 199, 0, 0],
        vec![0, 200],
        vec![0, 0],
    ] {
        assert!(diagnostic::parse_dpi_list(&bytes).is_err(), "{bytes:?}");
    }
    assert_eq!(
        diagnostic::parse_dpi_list(&[0, 200, 0, 250, 0, 0]).unwrap(),
        vec![200, 250]
    );
}
#[test]
fn invalid_slot_or_expired_deadline_never_writes_a_request() {
    let mouse = Mouse::new();
    let wire = mouse.wire.clone();
    assert!(diagnose(mouse, 0, Instant::now() + Duration::from_secs(1)).is_err());
    assert!(wire.borrow().is_empty());
    let mouse = Mouse::new();
    let wire = mouse.wire.clone();
    assert!(diagnose(mouse, 1, Instant::now()).is_err());
    assert!(wire.borrow().is_empty());
}
#[test]
fn values_are_read_fresh_on_each_diagnostic() {
    assert_eq!(
        read(Mouse::new()).unwrap()["device"]["settings"]["dpi"]["value"],
        1850
    );
    let mut mouse = Mouse::new();
    mouse.overrides.insert((0x2201, 2), vec![0, 7, 8, 3, 32]);
    assert_eq!(
        read(mouse).unwrap()["device"]["settings"]["dpi"]["value"],
        1800
    );
}

#[test]
fn bolt_identity_accepts_ble_model_without_legacy_equad() {
    let mut mouse = Mouse::new();
    mouse.overrides.insert(
        (0x0003, 0),
        vec![1, 0x11, 0x22, 0x33, 0x44, 0, 2, 0xb0, 0x34, 0, 0, 0, 0],
    );
    assert_eq!(read(mouse).unwrap()["device"]["model"], "B034");
}

#[test]
fn pairing_metadata_locates_the_model_without_device_queries_or_setters() {
    struct Receiver {
        pending: Vec<u8>,
        wire: Wire,
    }
    impl Transport for Receiver {
        fn write_report(&mut self, p: &[u8], _: Instant) -> io::Result<()> {
            assert_eq!(&p[..4], &[0x10, 0xff, 0x83, 0xb5]);
            self.wire.borrow_mut().push(p.to_vec());
            if p[4] == 0x54 {
                self.pending = vec![0; 20];
                self.pending[..8].copy_from_slice(&[0x11, 0xff, 0x83, 0xb5, 0x54, 2, 0x34, 0xb0]);
            } else {
                self.pending = vec![0x10, 0xff, 0x8f, 0x83, 0xb5, 2, 0];
            }
            Ok(())
        }
        fn read_report(&mut self, _: Instant) -> io::Result<Vec<u8>> {
            Ok(std::mem::take(&mut self.pending))
        }
    }
    let wire: Wire = Rc::default();
    let result = paired_devices(
        Receiver {
            pending: vec![],
            wire: wire.clone(),
        },
        Instant::now() + Duration::from_secs(1),
    )
    .unwrap();
    assert_eq!(result["slots"][0]["slot"], 4);
    assert_eq!(result["slots"][0]["model"], "B034");
    assert_eq!(result["slots"].as_array().unwrap().len(), 1);
    assert_eq!(wire.borrow().len(), 6);
}

fn write(mouse: Mouse, changes: Value) -> Result<Value, ApplyError> {
    apply_changes(
        mouse,
        1,
        "11223344",
        &changes,
        Instant::now() + Duration::from_secs(2),
    )
}
fn is_setter(p: &[u8]) -> bool {
    matches!((p[2], p[3] >> 4), (14, 3) | (8, 1) | (10, 2) | (12, 2))
}

#[test]
fn settings_validate_types_and_unknown_keys_before_any_io() {
    for changes in [
        serde_json::json!({}),
        serde_json::json!({"dpi":true}),
        serde_json::json!({"dpi":1800.0}),
        serde_json::json!({"smart-shift":51}),
        serde_json::json!({"scroll-ratchet":0}),
        serde_json::json!({"thumb-scroll-invert":1}),
        serde_json::json!({"buttons":{}}),
    ] {
        let mouse = Mouse::new();
        let wire = mouse.wire.clone();
        assert!(write(mouse, changes).is_err());
        assert!(wire.borrow().is_empty());
    }
}
#[test]
fn settings_validate_whole_batch_against_advertised_choices_before_writing() {
    let mut mouse = Mouse::new();
    mouse.accept_writes = true;
    let wire = mouse.wire.clone();
    let error = write(mouse, serde_json::json!({"scroll-ratchet":1,"dpi":1825})).unwrap_err();
    assert!(error.confirmed.is_empty());
    assert!(error.attempted.is_none());
    assert!(!wire.borrow().iter().any(|p| is_setter(p)));
}
#[test]
fn settings_expected_identity_blocks_all_setters() {
    let mouse = Mouse::new();
    let wire = mouse.wire.clone();
    assert!(apply_changes(
        mouse,
        1,
        "55667788",
        &serde_json::json!({"dpi":1800}),
        Instant::now() + Duration::from_secs(1)
    )
    .is_err());
    assert!(!wire.borrow().iter().any(|p| is_setter(p)));
}
#[test]
fn scalar_setters_are_confirmed_by_fresh_reads_and_noops_are_skipped() {
    let mut mouse = Mouse::new();
    mouse.accept_writes = true;
    let wire = mouse.wire.clone();
    let result=write(mouse,serde_json::json!({"scroll-ratchet":2,"smart-shift":15,"dpi":1800,"hires-smooth-invert":true,"hires-smooth-resolution":false,"thumb-scroll-invert":false})).unwrap();
    assert_eq!(result["confirmed"].as_array().unwrap().len(), 5);
    assert_eq!(
        result["snapshot"]["device"]["settings"]["dpi"]["value"],
        1800
    );
    assert_eq!(wire.borrow().iter().filter(|p| is_setter(p)).count(), 5);
    assert!(
        wire.borrow()
            .iter()
            .filter(|p| p[2] == 14 && p[3] >> 4 == 2)
            .count()
            >= 6
    );
}
#[test]
fn ratchet_mode_precedes_smartshift_and_hidden_threshold_is_rejected() {
    let mut mouse = Mouse::new();
    mouse.accept_writes = true;
    mouse.overrides.insert((0x2110, 0), vec![1, 255]);
    let wire = mouse.wire.clone();
    write(
        mouse,
        serde_json::json!({"scroll-ratchet":2,"smart-shift":20}),
    )
    .unwrap();
    let writes: Vec<_> = wire
        .borrow()
        .iter()
        .filter(|p| is_setter(p))
        .cloned()
        .collect();
    assert_eq!(&writes[0][4..6], &[2, 0]);
    assert_eq!(&writes[1][4..6], &[0, 20]);
    let mut mouse = Mouse::new();
    mouse.overrides.insert((0x2110, 0), vec![1, 255]);
    let wire = mouse.wire.clone();
    assert!(write(mouse, serde_json::json!({"smart-shift":20})).is_err());
    assert!(!wire.borrow().iter().any(|p| is_setter(p)));
}
#[test]
fn writes_preserve_wheel_flags_and_never_enable_diversion() {
    let mut mouse = Mouse::new();
    mouse.accept_writes = true;
    mouse.overrides.insert((0x2121, 1), vec![0xa0]);
    mouse.overrides.insert((0x2150, 1), vec![0x80, 0xa1]);
    let wire = mouse.wire.clone();
    write(mouse,serde_json::json!({"hires-smooth-invert":true,"hires-smooth-resolution":true,"thumb-scroll-invert":false})).unwrap();
    let writes: Vec<_> = wire
        .borrow()
        .iter()
        .filter(|p| is_setter(p))
        .cloned()
        .collect();
    assert_eq!(writes[0][4], 0xa4);
    assert_eq!(writes[1][4], 0xa6);
    assert_eq!(&writes[2][4..6], &[0x80, 0xa0]);
}
#[test]
fn acknowledgment_without_changed_hardware_is_not_confirmation() {
    let mut mouse = Mouse::new();
    mouse.accept_writes = true;
    mouse.ignore_writes = true;
    let wire = mouse.wire.clone();
    let error = write(mouse, serde_json::json!({"dpi":1800})).unwrap_err();
    assert!(error.message.contains("did not confirm"));
    assert!(error.confirmed.is_empty());
    assert_eq!(error.attempted.as_deref(), Some("dpi"));
    assert_eq!(wire.borrow().iter().filter(|p| is_setter(p)).count(), 1);
}
#[test]
fn partial_failure_reports_confirmed_changes_without_retry_or_rollback_claim() {
    let mut mouse = Mouse::new();
    mouse.accept_writes = true;
    mouse.disconnect_at = Some((0x2201, 3));
    let wire = mouse.wire.clone();
    let error = write(mouse, serde_json::json!({"scroll-ratchet":1,"dpi":1800})).unwrap_err();
    assert_eq!(error.confirmed, vec!["scroll-ratchet"]);
    assert_eq!(error.attempted.as_deref(), Some("dpi"));
    assert_eq!(
        wire.borrow()
            .iter()
            .filter(|p| p[2] == 14 && p[3] >> 4 == 3)
            .count(),
        1
    );
}
#[test]
fn smartshift_maximum_uses_firmware_marker_and_confirms_semantic_value() {
    let mut mouse = Mouse::new();
    mouse.accept_writes = true;
    let wire = mouse.wire.clone();
    let result = write(mouse, serde_json::json!({"smart-shift":50})).unwrap();
    assert_eq!(
        result["snapshot"]["device"]["settings"]["smart-shift"]["value"],
        50
    );
    let wire = wire.borrow();
    let set = wire.iter().find(|p| is_setter(p)).unwrap();
    assert_eq!(&set[4..6], &[0, 255]);
}
