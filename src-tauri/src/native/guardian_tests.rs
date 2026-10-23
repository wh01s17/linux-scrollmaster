// SPDX-License-Identifier: GPL-3.0-or-later
//! Real subprocesses around the independent synthetic N3 mouse; no hardware.
use super::*;
use crate::native::keyboard::GuardianKeyboard;
use crate::native::{
    actions::{Output, Runtime},
    events::Event,
    ActionBindings, DesktopAction, HeldModifiers, KeyboardAction,
};
use crate::native::{
    guardian::{serve_with_keyboard, Channel},
    GuardianConfig, HardwareLease, NativeGuardian,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const GUARD_TEST: &str = "native::buttons_tests::guardian_tests::guardian_fixture_child";
const OWNER_TEST: &str = "native::buttons_tests::guardian_tests::client_fixture_child";
#[path = "guardian_fixture_support.rs"]
mod fixture_support;
use fixture_support::*;
#[path = "guardian_fixture_config.rs"]
mod fixture_config;
use fixture_config::*;

fn fixture(path: &Path, mode: &str, duration: Duration) -> Command {
    mark_pending(path);
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", GUARD_TEST, "--nocapture"])
        .env("SCROLLMASTER_GUARD_FIXTURE", mode)
        .env("SCROLLMASTER_GUARD_DIR", path)
        .env("SCROLLMASTER_GUARD_MS", duration.as_millis().to_string());
    command
}
fn launch(path: &Path, mode: &str, duration: Duration) -> NativeGuardian {
    let config = mode_config(mode, duration);
    NativeGuardian::launch(
        fixture(path, mode, duration),
        &config,
        Duration::from_secs(3),
    )
    .unwrap()
}
struct FixtureMouse {
    io: Device,
    state: Rc<RefCell<State>>,
    pending: Option<Vec<u8>>,
    round: usize,
    armed: bool,
    mode: String,
    path: PathBuf,
    notifications: std::collections::VecDeque<Vec<u8>>,
    capture_waiting: bool,
    emitted: bool,
}
impl Drop for FixtureMouse {
    fn drop(&mut self) {
        log(&self.path, "transport-dropped");
    }
}
impl Transport for FixtureMouse {
    fn write_report(&mut self, p: &[u8], deadline: Instant) -> io::Result<()> {
        if self.mode == "disconnect" && self.armed {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        if p[2] == 0 && p[3] >> 4 == 1 {
            self.round += 1;
            self.capture_waiting = false;
            if self.mode == "capture-profile-drift" && self.round == 4 {
                self.state.borrow_mut().targets.insert(86, 86);
            }
        }
        if self.mode == "refresh-wait" && self.round == 3 {
            fs::write(self.path.join("refresh-started"), b"yes").unwrap();
        }
        if p[2] != 18 || p[3] >> 4 != 3 {
            return self.io.write_report(p, deadline);
        }
        let cid = u16::from_be_bytes([p[4], p[5]]);
        let valid: u16 = match cid {
            83 | 86 => 2,
            195 => 0x22,
            _ => panic!("Primary written"),
        };
        let enable = p[6] & 1 != 0;
        assert_eq!(
            p[6],
            if enable {
                valid as u8 | (valid as u8 >> 1)
            } else {
                valid as u8
            }
        );
        assert!(p[7..].iter().all(|b| *b == 0));
        assert!(!enable || self.mode.starts_with("capture-"));
        log(
            &self.path,
            &format!("{}:{cid}", if enable { "enable" } else { "clear" }),
        );
        if enable && cid == 83 && self.mode == "capture-wait" {
            self.capture_waiting = true;
            fs::write(self.path.join("activation-started"), b"yes").unwrap();
        }
        if self.mode == "gated" && cid == 83 {
            fs::write(self.path.join("restoring"), b"yes").unwrap();
            while !self.path.join("release").exists() {
                if Instant::now() >= deadline {
                    return Err(io::ErrorKind::TimedOut.into());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        let mut state = self.state.borrow_mut();
        state.wire.push(p.to_vec());
        let flags = state.flags.get(&cid).copied().unwrap_or(0);
        state.flags.insert(
            cid,
            if enable {
                flags | (valid >> 1)
            } else {
                flags & !(valid >> 1)
            },
        );
        let mut ack = frame(p, &p[4..10]);
        if self.mode == "bad-ack" || (self.mode == "capture-bad-ack" && enable && cid == 86) {
            ack[6] ^= 1;
        }
        self.pending = Some(ack);
        Ok(())
    }
    fn read_report(&mut self, deadline: Instant) -> io::Result<Vec<u8>> {
        if self.capture_waiting {
            // Quiet slices allow the owner's cancellation callback to see EOF.
            std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
            return Err(io::ErrorKind::TimedOut.into());
        }
        if self.mode == "refresh-wait" && self.round == 3 {
            std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
            return Err(io::ErrorKind::TimedOut.into());
        }
        if let Some(p) = self.pending.take() {
            return Ok(p);
        }
        if let Some(p) = self.notifications.pop_front() {
            return Ok(p);
        }
        if self.mode.starts_with("capture-") && !self.emitted && self.path.join("emit").exists() {
            self.emitted = true;
            self.notifications.extend([
                notification(&[]),
                notification(&[83, 86]),
                notification(&[83, 86]),
                notification(&[83, 195]),
                initial_raw(),
                frame(&[0x11, 1, 18, 0x10], &[0, 20, 0, 0]),
                notification(&[83]),
            ]);
            return Ok(self.notifications.pop_front().unwrap());
        }
        match self.io.read_report(deadline) {
            Ok(p) => {
                if self.round == 2 && self.state.borrow().reports == 6 && !self.armed {
                    self.armed = true;
                    if !self.mode.starts_with("capture-")
                        && !matches!(self.mode.as_str(), "normal" | "keyboard-open-error")
                    {
                        self.state
                            .borrow_mut()
                            .flags
                            .extend([(83, 1), (86, 1), (195, 0x11)]);
                    }
                    log(&self.path, "owner-setup-complete");
                    if self.mode.starts_with("keyboard-events") {
                        self.notifications.extend([
                            notification(&[]),
                            notification(&[83, 86]),
                            notification(&[83, 86]),
                            notification(&[83, 195]),
                            initial_raw(),
                            frame(&[0x11, 1, 18, 0x10], &[0, 20, 0, 0]),
                            notification(&[83]),
                        ]);
                    }
                }
                Ok(p)
            }
            Err(_) if self.round == 2 && self.mode == "panic" => {
                panic!("Synthetic passive receive panic")
            }
            Err(_) if self.mode == "disconnect" => Err(io::ErrorKind::BrokenPipe.into()),
            Err(_) => Err(io::ErrorKind::TimedOut.into()),
        }
    }
}

#[test]
fn guardian_fixture_child() {
    let Ok(mode) = std::env::var("SCROLLMASTER_GUARD_FIXTURE") else {
        return;
    };
    let path = PathBuf::from(std::env::var_os("SCROLLMASTER_GUARD_DIR").unwrap());
    let duration = Duration::from_millis(
        std::env::var("SCROLLMASTER_GUARD_MS")
            .unwrap()
            .parse()
            .unwrap(),
    );
    if mode == "slow-start" {
        std::thread::sleep(Duration::from_millis(120));
    }
    let lease = HardwareLease::at(&path).unwrap();
    let mut channel = Channel::inherited().unwrap();
    fs::write(
        path.join("session.json"),
        serde_json::to_vec(&json!({"pid":std::process::id(),
        "session":unsafe { libc::getsid(0) }}))
        .unwrap(),
    )
    .unwrap();
    let (io, state) = Device::new();
    if mode.starts_with("capture-profile") && mode != "capture-profile-mismatch" {
        state.borrow_mut().targets.insert(86, 83);
    }
    if mode == "intercepted" {
        state.borrow_mut().flags.insert(83, 1);
    }
    let keyboard_mode = mode.clone();
    let mouse = FixtureMouse {
        io,
        state: state.clone(),
        pending: None,
        round: 0,
        armed: false,
        mode,
        path: path.clone(),
        notifications: Default::default(),
        capture_waiting: false,
        emitted: false,
    };
    let config = if keyboard_mode == "capture-profile-unbound" {
        GuardianConfig::new("hidraw0", 1, "11223344", &[83, 195], duration)
            .unwrap()
            .with_capture_actions(&mode_bindings(&keyboard_mode))
            .unwrap()
    } else {
        mode_config(&keyboard_mode, duration)
    };
    let mut data = serve_with_keyboard(mouse, &config, &mut channel, lease, |_| {
        if keyboard_mode == "keyboard-open-error" {
            return Err("Synthetic keyboard unavailable".into());
        }
        if keyboard_mode.starts_with("keyboard-") || keyboard_mode.starts_with("capture-") {
            log(&path, "keyboard-open");
            Ok(Some(FixtureKeyboard {
                path: path.clone(),
                close_error: matches!(
                    keyboard_mode.as_str(),
                    "keyboard-close-error" | "capture-close-error"
                ),
                closed: false,
                engine: if keyboard_mode.starts_with("keyboard-events")
                    || keyboard_mode.starts_with("capture-")
                {
                    Some(Runtime::new(
                        FixtureOutput {
                            path: path.clone(),
                            mode: keyboard_mode.clone(),
                        },
                        ActionBindings::parse(&mode_bindings(&keyboard_mode)).unwrap(),
                        None,
                    )?)
                } else {
                    None
                },
            }))
        } else {
            Ok(None)
        }
    });
    data["synthetic"] = json!(true);
    data["final_flags"] = json!(state.borrow().flags);
    data["final_targets"] = json!(state.borrow().targets);
    fs::write(path.join("result.tmp"), serde_json::to_vec(&data).unwrap()).unwrap();
    fs::rename(path.join("result.tmp"), path.join("result.json")).unwrap();
}

#[path = "guardian_profile_tests.rs"]
mod profile_tests;
#[test]
fn client_fixture_child() {
    let Ok(mode) = std::env::var("SCROLLMASTER_GUARD_CLIENT") else {
        return;
    };
    let path = PathBuf::from(std::env::var_os("SCROLLMASTER_GUARD_DIR").unwrap());
    let mut client = launch(&path, &mode, Duration::from_secs(10));
    fs::write(path.join("client-ready"), b"yes").unwrap();
    if mode == "refresh-wait" {
        let _ = client.refresh(Duration::from_secs(5));
    }
    loop {
        std::thread::sleep(Duration::from_millis(100));
    }
}
#[test]
fn process_guardian_reads_status_refreshes_and_reaps_after_normal_stop() {
    let dir = Sandbox::new();
    let mut client = launch(&dir.0, "normal", Duration::from_secs(3));
    assert_eq!(client.ready()["registered"], json!([83, 86, 195]));
    let session: Value =
        serde_json::from_slice(&fs::read(dir.0.join("session.json")).unwrap()).unwrap();
    assert_eq!(
        session["pid"], session["session"],
        "Guardian has its own session"
    );
    assert_ne!(session["session"], unsafe { libc::getsid(0) });
    assert!(HardwareLease::at(&dir.0).is_err());
    assert_eq!(client.status().unwrap()["data"]["refreshes"], 0);
    assert_eq!(
        client.refresh(Duration::from_secs(1)).unwrap()["data"]["refreshes"],
        1
    );
    let report = client.finish().unwrap();
    assert_eq!(report["trigger"], "stop");
    assert_eq!(report["completed"], true, "{report}");
    assert_eq!(report["recovery"]["written"], json!([]));
    assert!(HardwareLease::at(&dir.0).is_ok());
    assert!(!fs::read_to_string(dir.0.join("wire.log"))
        .unwrap()
        .contains("clear:"));
}
#[test]
fn guardian_drops_active_session_then_clears_only_registered_flags() {
    let dir = Sandbox::new();
    let client = launch(&dir.0, "armed", Duration::from_secs(3));
    let report = client.finish().unwrap();
    assert_eq!(report["completed"], true);
    assert_eq!(report["recovery"]["written"], json!([83, 86, 195]));
    let final_data = result(&dir.0);
    assert_eq!(final_data["final_flags"], json!({"83":0,"86":0,"195":0}));
    let wire = fs::read_to_string(dir.0.join("wire.log")).unwrap();
    assert!(wire.find("owner-setup-complete").unwrap() < wire.find("clear:83").unwrap());
    assert!(wire.ends_with("transport-dropped\n"));
}
#[test]
fn dropped_client_does_not_kill_recovery_or_release_lease_prematurely() {
    let dir = Sandbox::new();
    let client = launch(&dir.0, "gated", Duration::from_secs(3));
    drop(client);
    wait_file(&dir.0.join("restoring"));
    assert!(
        HardwareLease::at(&dir.0).is_err(),
        "Lease must cover recovery"
    );
    fs::write(dir.0.join("release"), b"yes").unwrap();
    let report = result(&dir.0);
    assert_eq!(report["recovery"]["completed"], true);
    assert_eq!(report["final_flags"], json!({"83":0,"86":0,"195":0}));
    assert!(HardwareLease::at(&dir.0).is_ok());
}
#[test]
fn sigkill_of_client_recovers_even_during_a_query() {
    for mode in ["armed", "refresh-wait"] {
        let dir = Sandbox::new();
        let mut worker = dir.client(mode);
        wait_file(&dir.0.join("client-ready"));
        if mode == "refresh-wait" {
            wait_file(&dir.0.join("refresh-started"));
        }
        assert!(HardwareLease::at(&dir.0).is_err());
        worker.kill().unwrap();
        worker.wait().unwrap();
        let report = result(&dir.0);
        assert_eq!(report["recovery"]["completed"], true, "{report}");
        assert_eq!(report["final_flags"], json!({"83":0,"86":0,"195":0}));
        assert!(HardwareLease::at(&dir.0).is_ok());
    }
}
#[test]
fn malformed_or_oversized_commands_fail_closed_and_restore() {
    for command in [
        b"{\"id\":1,\"op\":\"enable\"}\n".to_vec(),
        b"{\"id\":2,\"op\":\"status\"}\n".to_vec(),
        b"{\"id\":1,\"op\":\"refresh\",\"timeout_ms\":0}\n".to_vec(),
        b"{\"id\":1,\"op\":\"status\",\"extra\":true}\n".to_vec(),
        b"[1,2,3]\n".to_vec(),
        b"broken\n".to_vec(),
        vec![b'x'; 300],
    ] {
        let dir = Sandbox::new();
        let mut client = launch(&dir.0, "armed", Duration::from_secs(3));
        client.send_test_bytes(&command);
        let report = client.finish().unwrap();
        assert_eq!(report["trigger"], "protocol-error");
        assert_eq!(report["completed"], false);
        assert_eq!(report["recovery"]["completed"], true);
        assert!(HardwareLease::at(&dir.0).is_ok());
    }
}
#[test]
fn automatic_deadline_preserves_final_evidence_without_a_stop_race() {
    for request_first in [false, true] {
        let dir = Sandbox::new();
        let mut client = launch(&dir.0, "armed", Duration::from_millis(60));
        wait_file(&dir.0.join("result.json"));
        if request_first {
            assert!(client.status().is_err());
        }
        let report = client.finish().unwrap();
        assert_eq!(report["trigger"], "deadline");
        assert_eq!(report["completed"], true, "{report}");
        assert_eq!(report["recovery"]["written"], json!([83, 86, 195]));
    }
}
#[test]
fn panic_and_disconnect_never_skip_terminal_recovery_or_fake_success() {
    for mode in ["panic", "disconnect", "bad-ack"] {
        let dir = Sandbox::new();
        let client = launch(&dir.0, mode, Duration::from_secs(3));
        if mode != "bad-ack" {
            // Wait for the injected idle failure; an immediate stop can arrive
            // before the first idle poll and legitimately complete normally.
            wait_file(&dir.0.join("result.json"));
        }
        let report = client.finish().unwrap();
        assert_eq!(report["completed"], false, "{mode}: {report}");
        if mode == "panic" {
            assert_eq!(report["recovery"]["completed"], true);
        } else {
            assert_eq!(report["recovery"]["completed"], false);
        }
        assert!(HardwareLease::at(&dir.0).is_ok());
    }
}
#[test]
fn failed_registration_never_sends_setters_or_announces_ready() {
    let dir = Sandbox::new();
    let error = NativeGuardian::launch(
        fixture(&dir.0, "intercepted", Duration::from_secs(1)),
        &config(Duration::from_secs(1)),
        Duration::from_secs(2),
    )
    .err()
    .unwrap();
    assert!(error.contains("registration-error"));
    assert_eq!(result(&dir.0)["recovery"], Value::Null);
    assert!(!fs::read_to_string(dir.0.join("wire.log"))
        .unwrap()
        .contains("clear:"));
    assert!(HardwareLease::at(&dir.0).is_ok());
}
#[test]
fn startup_timeout_closes_the_channel_and_allows_cleanup_without_killing_child() {
    let dir = Sandbox::new();
    assert!(NativeGuardian::launch(
        fixture(&dir.0, "slow-start", Duration::from_secs(1)),
        &config(Duration::from_secs(1)),
        Duration::from_millis(20)
    )
    .is_err());
    let report = result(&dir.0);
    assert_eq!(report["trigger"], "registration-error");
    assert!(!fs::read_to_string(dir.0.join("wire.log"))
        .unwrap()
        .contains("clear:"));
    assert!(HardwareLease::at(&dir.0).is_ok());
}
#[test]
fn invalid_refresh_arguments_preserve_client_and_guardian() {
    let dir = Sandbox::new();
    let mut client = launch(&dir.0, "normal", Duration::from_secs(3));
    for timeout in [Duration::ZERO, Duration::from_secs(21), Duration::MAX] {
        assert!(client.refresh(timeout).is_err());
    }
    assert_eq!(client.status().unwrap()["id"], 1);
    assert_eq!(client.finish().unwrap()["completed"], true);
}

struct FixtureKeyboard {
    path: PathBuf,
    close_error: bool,
    closed: bool,
    engine: Option<Runtime<FixtureOutput>>,
}
impl GuardianKeyboard for FixtureKeyboard {
    fn event(&mut self, event: &Event) -> Result<(), String> {
        self.engine
            .as_mut()
            .map_or(Ok(()), |engine| engine.event(event))
    }
    fn before_query(&mut self) -> Result<(), String> {
        self.engine
            .as_mut()
            .map_or(Ok(()), |engine| engine.before_query())
    }
    fn snapshot(&mut self, snapshot: &Value) -> Result<(), String> {
        self.engine
            .as_mut()
            .map_or(Ok(()), |engine| engine.snapshot(snapshot))
    }
    fn close(&mut self) -> Result<(), String> {
        assert!(
            !self.closed,
            "Keyboard release must be attempted exactly once"
        );
        self.closed = true;
        log(&self.path, "keyboard-release");
        if let Some(engine) = &mut self.engine {
            GuardianKeyboard::close(engine)?;
        }
        if self.close_error {
            Err("Synthetic keyboard release unconfirmed".into())
        } else {
            Ok(())
        }
    }
}
struct FixtureOutput {
    path: PathBuf,
    mode: String,
}
impl Output for FixtureOutput {
    fn require_desktop(&mut self) -> Result<(), String> {
        log(&self.path, "desktop-check");
        if self.mode == "capture-desktop-unavailable" {
            Err("Synthetic desktop unavailable".into())
        } else {
            Ok(())
        }
    }
    fn desktop(&mut self, action: DesktopAction) -> Result<(), String> {
        log(&self.path, &format!("desktop-dispatch:{action:?}"));
        if self.mode == "capture-desktop-error" {
            return Err("Synthetic desktop confirmation uncertain".into());
        }
        if action == DesktopAction::WorkspaceNext {
            fs::write(self.path.join("actions-complete"), b"yes").unwrap();
        }
        Ok(())
    }
    fn held(&mut self, held: HeldModifiers) -> Result<(), String> {
        log(&self.path, &format!("action-held:{held:?}"));
        Ok(())
    }
    fn tap(&mut self, action: &KeyboardAction) -> Result<(), String> {
        log(&self.path, &format!("action-tap:{:?}", action.code()));
        assert!(
            self.mode != "keyboard-events-panic",
            "Synthetic action panic"
        );
        if matches!(
            self.mode.as_str(),
            "keyboard-events-error" | "capture-action-error"
        ) {
            return Err("Synthetic action failed".into());
        }
        if action.code() == (47, 4) {
            fs::write(self.path.join("actions-complete"), b"yes").unwrap();
        }
        Ok(())
    }
    fn close(&mut self) -> Result<(), String> {
        log(&self.path, "action-release");
        Ok(())
    }
}
impl Drop for FixtureKeyboard {
    fn drop(&mut self) {
        // Keep the release invariant on normal exit without double-panicking
        // if an earlier fixture failure is already unwinding the stack.
        if !std::thread::panicking() {
            assert!(self.closed);
        }
        log(&self.path, "keyboard-dropped");
    }
}
fn assert_keyboard_before_mouse(path: &Path) {
    let wire = fs::read_to_string(path.join("wire.log")).unwrap();
    assert!(wire.find("keyboard-release").unwrap() < wire.find("keyboard-dropped").unwrap());
    assert!(wire.find("keyboard-dropped").unwrap() < wire.find("clear:83").unwrap());
    assert!(HardwareLease::at(path).is_ok());
}

#[test]
fn guardian_delivers_wire_events_to_the_action_engine_and_releases_before_recovery() {
    let dir = Sandbox::new();
    let client = launch(&dir.0, "keyboard-events", Duration::from_secs(3));
    assert_eq!(client.ready()["data"]["observation"]["sink_attached"], true);
    wait_file(&dir.0.join("actions-complete"));
    let report = client.finish().unwrap();
    assert_eq!(report["completed"], true, "{report}");
    // The paste now runs on the motion report (6th delivery), when the gesture
    // starts; the final release may still be in flight when the client finishes.
    let deliveries = report["owner"]["observation"]["sink_deliveries"]
        .as_u64()
        .unwrap();
    assert!((6..=7).contains(&deliveries), "{deliveries}");
    assert_eq!(report["keyboard"]["release_confirmed"], true);
    let wire = fs::read_to_string(dir.0.join("wire.log")).unwrap();
    assert_eq!(wire.matches("action-held:Ctrl").count(), 1);
    assert_eq!(wire.matches("action-tap:(46, 4)").count(), 1);
    assert_eq!(wire.matches("action-tap:(47, 4)").count(), 1);
    assert_eq!(wire.matches("action-release").count(), 1);
    assert!(wire.find("action-release").unwrap() < wire.find("clear:83").unwrap());
    assert_keyboard_before_mouse(&dir.0);
}

#[test]
fn capture_announces_ready_only_after_confirming_the_full_batch_and_then_runs_actions() {
    let dir = Sandbox::new();
    let mut client = launch(&dir.0, "capture-normal", Duration::from_secs(3));
    assert_eq!(client.ready()["data"]["mode"], "capture-owner");
    assert_eq!(
        client.ready()["data"]["activation"]["confirmed"],
        json!([83, 86, 195])
    );
    assert_eq!(client.ready()["data"]["observation"]["capture_armed"], true);
    assert_eq!(client.ready()["actions"], capture_bindings());
    let wire = fs::read_to_string(dir.0.join("wire.log")).unwrap();
    assert_eq!(wire.matches("enable:").count(), 3);
    assert!(!wire.contains("action-tap"));
    client.refresh(Duration::from_secs(1)).unwrap();
    fs::write(dir.0.join("emit"), b"yes").unwrap();
    wait_file(&dir.0.join("actions-complete"));
    let report = client.finish().unwrap();
    assert_eq!(report["completed"], true, "{report}");
    assert_eq!(report["mode"], "capture-guardian");
    assert_eq!(report["recovery"]["written"], json!([83, 86, 195]));
    assert_eq!(
        result(&dir.0)["final_flags"],
        json!({"83":0,"86":0,"195":0})
    );
    let wire = fs::read_to_string(dir.0.join("wire.log")).unwrap();
    assert_eq!(wire.matches("action-tap:(46, 4)").count(), 1);
    assert_eq!(wire.matches("action-tap:(47, 4)").count(), 1);
    assert_keyboard_before_mouse(&dir.0);
}
#[test]
fn capture_desktop_routes_live_side_and_gesture_actions_without_keyboard_taps() {
    let dir = Sandbox::new();
    let client = launch(&dir.0, "capture-desktop", Duration::from_secs(3));
    assert_eq!(client.ready()["actions"], mode_bindings("capture-desktop"));
    let wire = fs::read_to_string(dir.0.join("wire.log")).unwrap();
    assert!(wire.find("desktop-check").unwrap() < wire.find("enable:83").unwrap());
    assert!(!wire.contains("desktop-dispatch"));
    fs::write(dir.0.join("emit"), b"yes").unwrap();
    wait_file(&dir.0.join("actions-complete"));
    let report = client.finish().unwrap();
    assert_eq!(report["completed"], true, "{report}");
    assert_eq!(report["recovery"]["completed"], true);
    assert_eq!(
        result(&dir.0)["final_flags"],
        json!({"83":0,"86":0,"195":0})
    );
    let wire = fs::read_to_string(dir.0.join("wire.log")).unwrap();
    assert_eq!(wire.matches("desktop-dispatch:Fullscreen").count(), 1);
    assert_eq!(wire.matches("desktop-dispatch:WorkspaceNext").count(), 1);
    assert_eq!(wire.matches("action-held:Ctrl").count(), 1);
    assert!(!wire.contains("action-tap"));
    assert_keyboard_before_mouse(&dir.0);
}
#[test]
fn unavailable_desktop_prevents_capture_activation_and_ready() {
    let dir = Sandbox::new();
    let duration = Duration::from_secs(2);
    let error = NativeGuardian::launch(
        fixture(&dir.0, "capture-desktop-unavailable", duration),
        &mode_config("capture-desktop-unavailable", duration),
        Duration::from_secs(3),
    )
    .err()
    .unwrap();
    assert!(error.contains("Synthetic desktop unavailable"));
    let report = result(&dir.0);
    assert_eq!(report["completed"], false);
    assert_eq!(report["recovery"]["completed"], true);
    assert_eq!(report["final_flags"], json!({})); // Fixture default: zero, never written.
    assert_eq!(report["recovery"]["written"], json!([]));
    let wire = fs::read_to_string(dir.0.join("wire.log")).unwrap();
    assert!(!wire.contains("enable:") && !wire.contains("desktop-dispatch"));
    assert!(!wire.contains("clear:"));
    assert_eq!(wire.matches("action-release").count(), 1);
    assert!(wire.find("action-release").unwrap() < wire.find("transport-dropped").unwrap());
    assert!(HardwareLease::at(&dir.0).is_ok());
}
#[test]
fn uncertain_desktop_dispatch_closes_output_and_restores_capture_without_retry() {
    let dir = Sandbox::new();
    let client = launch(&dir.0, "capture-desktop-error", Duration::from_secs(3));
    fs::write(dir.0.join("emit"), b"yes").unwrap();
    wait_file(&dir.0.join("result.json"));
    let report = client.finish().unwrap();
    assert_eq!(report["completed"], false, "{report}");
    assert_eq!(report["recovery"]["completed"], true);
    assert_eq!(
        result(&dir.0)["final_flags"],
        json!({"83":0,"86":0,"195":0})
    );
    let wire = fs::read_to_string(dir.0.join("wire.log")).unwrap();
    assert_eq!(wire.matches("desktop-dispatch:").count(), 1);
    assert!(wire.contains("action-held:Ctrl"));
    assert!(!wire.contains("action-tap"));
    assert_eq!(wire.matches("action-release").count(), 1);
    assert!(wire.find("action-release").unwrap() < wire.find("clear:83").unwrap());
    assert_keyboard_before_mouse(&dir.0);
}
#[test]
fn failed_activation_never_announces_ready_and_restores_even_uncertain_controls() {
    let dir = Sandbox::new();
    let config = mode_config("capture-bad-ack", Duration::from_secs(2));
    let error = NativeGuardian::launch(
        fixture(&dir.0, "capture-bad-ack", Duration::from_secs(2)),
        &config,
        Duration::from_secs(3),
    )
    .err()
    .unwrap();
    assert!(error.contains("Capture ACK"));
    let report = result(&dir.0);
    assert_eq!(report["owner"]["activation"]["confirmed"], json!([83]));
    assert_eq!(report["owner"]["activation"]["attempted"], 86);
    assert_eq!(report["completed"], false);
    assert_eq!(report["recovery"]["written"], json!([83, 86]));
    assert_eq!(report["recovery"]["completed"], true);
    assert!(!fs::read_to_string(dir.0.join("wire.log"))
        .unwrap()
        .contains("action-tap"));
    assert_keyboard_before_mouse(&dir.0);
}
#[test]
fn sigkill_during_activation_or_with_live_modifiers_does_not_cancel_guardian_recovery() {
    for mode in ["capture-wait", "capture-normal"] {
        let dir = Sandbox::new();
        let mut client = dir.client(mode);
        if mode == "capture-wait" {
            wait_file(&dir.0.join("activation-started"));
        } else {
            wait_file(&dir.0.join("client-ready"));
            fs::write(dir.0.join("emit"), b"yes").unwrap();
            wait_file(&dir.0.join("actions-complete"));
        }
        client.kill().unwrap();
        client.wait().unwrap();
        let report = result(&dir.0);
        assert_eq!(report["completed"], false);
        assert_eq!(report["recovery"]["completed"], true, "{report}");
        assert_eq!(
            report["final_flags"],
            if mode == "capture-wait" {
                json!({"83":0})
            } else {
                json!({"83":0,"86":0,"195":0})
            }
        );
        assert_keyboard_before_mouse(&dir.0);
    }
}
#[test]
fn active_output_failure_and_unconfirmed_release_still_restore_real_setter_effects() {
    for mode in ["capture-action-error", "capture-close-error"] {
        let dir = Sandbox::new();
        let client = launch(&dir.0, mode, Duration::from_secs(2));
        if mode == "capture-action-error" {
            fs::write(dir.0.join("emit"), b"yes").unwrap();
            wait_file(&dir.0.join("result.json"));
        }
        let report = client.finish().unwrap();
        assert_eq!(report["completed"], false, "{report}");
        assert_eq!(report["recovery"]["completed"], true);
        assert_eq!(report["recovery"]["written"], json!([83, 86, 195]));
        assert_keyboard_before_mouse(&dir.0);
    }
}
#[test]
fn parent_rejects_a_passive_ready_when_explicit_capture_was_requested() {
    let dir = Sandbox::new();
    assert!(NativeGuardian::launch(
        fixture(&dir.0, "normal", Duration::from_secs(1)),
        &mode_config("capture-normal", Duration::from_secs(1)),
        Duration::from_secs(2)
    )
    .is_err());
    let report = result(&dir.0);
    assert_eq!(report["recovery"]["completed"], true);
    assert!(!fs::read_to_string(dir.0.join("wire.log"))
        .unwrap()
        .contains("enable:"));
}
#[test]
fn action_failure_and_panic_never_retry_or_skip_guardian_restoration() {
    for mode in ["keyboard-events-error", "keyboard-events-panic"] {
        let dir = Sandbox::new();
        let client = launch(&dir.0, mode, Duration::from_secs(3));
        wait_file(&dir.0.join("result.json"));
        let report = client.finish().unwrap();
        assert_eq!(report["completed"], false, "{report}");
        assert_eq!(
            report["owner"]["observation"]["stop_reason"],
            "output-error"
        );
        assert_eq!(report["recovery"]["completed"], true);
        let wire = fs::read_to_string(dir.0.join("wire.log")).unwrap();
        assert_eq!(wire.matches("action-tap:").count(), 1);
        assert_eq!(wire.matches("action-release").count(), 1);
        assert_keyboard_before_mouse(&dir.0);
    }
}
#[test]
fn sigkill_of_client_releases_an_action_modifier_before_mouse_recovery() {
    let dir = Sandbox::new();
    let mut client = dir.client("keyboard-events");
    wait_file(&dir.0.join("actions-complete"));
    client.kill().unwrap();
    client.wait().unwrap();
    let report = result(&dir.0);
    assert_eq!(report["completed"], false);
    assert_eq!(report["recovery"]["completed"], true);
    let wire = fs::read_to_string(dir.0.join("wire.log")).unwrap();
    assert_eq!(wire.matches("action-held:Ctrl").count(), 1);
    assert_eq!(wire.matches("action-release").count(), 1);
    assert!(wire.find("action-release").unwrap() < wire.find("clear:83").unwrap());
    assert_keyboard_before_mouse(&dir.0);
}
#[test]
fn deadline_and_protocol_failure_release_live_actions_without_client_intervention() {
    for invalid in [false, true] {
        let dir = Sandbox::new();
        let mut client = launch(&dir.0, "keyboard-events", Duration::from_millis(500));
        wait_file(&dir.0.join("actions-complete"));
        if invalid {
            client.send_test_bytes(b"{\"id\":1,\"op\":\"tap\"}\n");
        }
        wait_file(&dir.0.join("result.json"));
        let report = client.finish().unwrap();
        assert_eq!(
            report["trigger"],
            if invalid {
                "protocol-error"
            } else {
                "deadline"
            }
        );
        assert_eq!(report["completed"], !invalid);
        assert_eq!(report["recovery"]["completed"], true);
        assert_keyboard_before_mouse(&dir.0);
    }
}
#[test]
fn keyboard_release_precedes_mouse_restoration_even_when_not_confirmed() {
    for mode in ["keyboard-armed", "keyboard-close-error"] {
        let dir = Sandbox::new();
        let client = launch(&dir.0, mode, Duration::from_secs(3));
        assert_eq!(client.ready()["keyboard"]["ready"], true);
        let report = client.finish().unwrap();
        assert_eq!(report["keyboard"]["opened"], true);
        assert_eq!(
            report["keyboard"]["release_confirmed"],
            mode == "keyboard-armed"
        );
        assert_eq!(report["completed"], mode == "keyboard-armed");
        assert_eq!(report["recovery"]["completed"], true);
        assert_keyboard_before_mouse(&dir.0);
    }
}
#[test]
fn missing_keyboard_never_announces_ready_and_still_restores_the_baseline() {
    let dir = Sandbox::new();
    let duration = Duration::from_secs(1);
    let error = NativeGuardian::launch(
        fixture(&dir.0, "keyboard-open-error", duration),
        &config(duration).with_keyboard_check(),
        Duration::from_secs(3),
    )
    .err()
    .unwrap();
    assert!(error.contains("keyboard unavailable"));
    let report = result(&dir.0);
    assert_eq!(report["keyboard"]["opened"], false);
    assert_eq!(report["recovery"]["completed"], true);
    assert_eq!(report["recovery"]["written"], json!([]));
    assert!(HardwareLease::at(&dir.0).is_ok());
}
#[test]
fn keyboard_cleanup_survives_client_sigkill_before_firmware_restoration() {
    let dir = Sandbox::new();
    let mut worker = dir.client("keyboard-armed");
    wait_file(&dir.0.join("client-ready"));
    worker.kill().unwrap();
    worker.wait().unwrap();
    let report = result(&dir.0);
    assert_eq!(report["keyboard"]["release_confirmed"], true);
    assert_eq!(report["recovery"]["completed"], true);
    assert_keyboard_before_mouse(&dir.0);
}
#[test]
fn protocol_failure_releases_keyboard_before_restoring_and_preserves_the_error() {
    let dir = Sandbox::new();
    let mut client = launch(&dir.0, "keyboard-armed", Duration::from_secs(3));
    client.send_test_bytes(b"{\"op\":\"tap\",\"id\":1,\"action\":\"shortcut:copy\"}\n");
    let report = client.finish().unwrap();
    assert_eq!(report["trigger"], "protocol-error");
    assert_eq!(report["completed"], false);
    assert_eq!(report["keyboard"]["release_confirmed"], true);
    assert_eq!(report["recovery"]["completed"], true);
    assert_keyboard_before_mouse(&dir.0);
}
