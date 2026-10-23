// SPDX-License-Identifier: GPL-3.0-or-later
#![cfg(target_os = "linux")]
//! The GUI executable re-executed as the native worker (as NativeHost and the
//! guardian do). Local profile operations only; no window, HID or snapshot.
use linux_scrollmaster::{
    bridge::Worker,
    profiles::{Document, ProfileStore},
};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    process::{Command, Stdio},
    time::Duration,
};
const GUI: &str = env!("CARGO_BIN_EXE_linux-scrollmaster");
const ID: &str = "11223344";

#[test]
fn gui_binary_serves_the_native_worker_protocol_without_a_window() {
    let dir = std::env::temp_dir().join(format!("scrollmaster-gui-entry-{}", std::process::id()));
    fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
    let store = ProfileStore::at(dir.join("linux-scrollmaster")).unwrap();
    let document = Document::from_value(json!({"version":1,
        "profiles":[{"device":ID,"name":"Work","settings":{"dpi":1800}}],"active":{}}))
    .unwrap();
    store.commit(&store.load().unwrap(), &document).unwrap();
    let mut command = Command::new(GUI);
    command
        .args(["--serve-guarded-capture", "--slot", "2", "--device", ID])
        .env("XDG_CONFIG_HOME", &dir)
        .env("XDG_RUNTIME_DIR", dir.join("missing-runtime"))
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DISPLAY");
    let mut worker = Worker::start(command).unwrap();
    let renamed = worker
        .transact(
            &json!({"op":"rename","device":ID,"name":"Work","new_name":"Office"}),
            Duration::from_secs(5),
        )
        .unwrap();
    assert_eq!(renamed["ok"], true, "{renamed}");
    assert_eq!(renamed["data"]["profiles"][0]["name"], "Office");
    let stopped = worker
        .transact(&json!({"op":"shutdown"}), Duration::from_secs(5))
        .unwrap();
    assert_eq!(stopped["data"]["stopped"], true);
    // No runtime directory/lease was created: nothing acquired hardware.
    assert!(!dir.join("missing-runtime").exists());
    // Invalid worker arguments fail closed instead of starting the GUI.
    let status = Command::new(GUI)
        .args(["--serve", "--slot", "9"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!status.success());
    fs::remove_dir_all(&dir).unwrap();
}
