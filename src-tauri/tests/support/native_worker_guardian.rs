// SPDX-License-Identifier: GPL-3.0-or-later
//! Real executable, offline profile requests only. Never acquire physical HID.
use super::*;

fn guarded(dir: &Sandbox) -> Command {
    let mut command = dir.command();
    command.args(["--serve-guarded", "--slot", "2", "--device", ID]);
    command
}
#[test]
fn guarded_worker_preserves_offline_management_and_persistence() {
    let dir = Sandbox::new();
    dir.seed();
    let mut worker = Worker::start(guarded(&dir)).unwrap();
    assert_eq!(
        transact(
            &mut worker,
            json!({"op":"rename","device":ID,"name":"Work","new_name":"Office"})
        )["ok"],
        true
    );
    assert_eq!(
        transact(
            &mut worker,
            json!({"op":"auto","device":ID,"name":"Office"})
        )["data"]["active"][ID],
        "Office"
    );
    assert_eq!(
        transact(
            &mut worker,
            json!({"op":"export","device":ID,"name":"Office"})
        )["data"]["profiles"][0]["name"],
        "Office"
    );
    assert_eq!(
        transact(&mut worker, json!({"op":"shutdown"}))["data"]["stopped"],
        true
    );
    assert!(!dir.0.join("missing-runtime").exists());
}
#[test]
fn capture_worker_remains_offline_for_local_profile_operations() {
    let dir = Sandbox::new();
    dir.seed();
    let mut command = dir.command();
    command.args(["--serve-guarded-capture", "--slot", "2", "--device", ID]);
    let mut worker = Worker::start(command).unwrap();
    assert_eq!(
        transact(
            &mut worker,
            json!({"op":"export","device":ID,"name":"Work"})
        )["ok"],
        true
    );
    assert_eq!(
        transact(&mut worker, json!({"op":"shutdown"}))["data"]["stopped"],
        true
    );
    assert!(!dir.0.join("missing-runtime").exists());
}
#[test]
fn guarded_worker_rejects_software_before_disarming_or_acquisition() {
    let dir = Sandbox::new();
    let store = dir.store();
    let document=Document::from_value(json!({"version":3,"profiles":[{"device":ID,"name":"Software","settings":{"dpi":1850},"buttons":{"83":"modifier:ctrl"}}],"active":{ID:"Software"}})).unwrap();
    store.commit(&store.load().unwrap(), &document).unwrap();
    let before = dir.bytes();
    let mut worker = Worker::start(guarded(&dir)).unwrap();
    let reply = transact(
        &mut worker,
        json!({"op":"restore","device":ID,"name":"Software"}),
    );
    assert_eq!(reply["ok"], false);
    assert!(reply["error"]
        .as_str()
        .unwrap()
        .contains("Software assignments"));
    assert_eq!(dir.bytes(), before);
    assert!(!dir.0.join("missing-runtime").exists());
    transact(&mut worker, json!({"op":"shutdown"}));
}
#[test]
fn internal_profile_guardian_requires_the_authenticated_inherited_channel() {
    let dir = Sandbox::new();
    let mut command = dir.command();
    command.args(["--profile-guardian-child", "--slot", "2", "--device", ID]);
    let (success, replies) = raw(command, b"");
    assert!(!success);
    assert!(replies.is_empty());
    assert!(!dir.0.join("linux-scrollmaster").exists());
    assert!(!dir.0.join("missing-runtime").exists());
}
#[test]
fn internal_capture_guardian_also_requires_the_authenticated_channel() {
    let dir = Sandbox::new();
    let mut command = dir.command();
    command.args([
        "--profile-guardian-capture-child",
        "--slot",
        "2",
        "--device",
        ID,
    ]);
    let (success, replies) = raw(command, b"");
    assert!(!success && replies.is_empty());
    assert!(!dir.0.join("missing-runtime").exists());
}

#[test]
fn production_guardian_handshakes_rejects_software_and_shuts_down_without_hardware() {
    use std::{
        io::{BufRead, BufReader},
        os::{
            fd::{AsRawFd, FromRawFd, OwnedFd},
            unix::{net::UnixStream, process::CommandExt},
        },
    };
    for (flag, expected) in [
        ("--profile-guardian-child", "Software assignments"),
        ("--profile-guardian-capture-child", "deadline elapsed"),
    ] {
        let dir = Sandbox::new();
        let (parent, child_socket) = UnixStream::pair().unwrap();
        parent
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let fd = unsafe { libc::fcntl(child_socket.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 64) };
        assert!(fd >= 0);
        let inherited = unsafe { OwnedFd::from_raw_fd(fd) };
        let mut command = dir.command();
        command.args([flag, "--slot", "2", "--device", ID]);
        // Match the private launcher without exposing a test-only command API.
        unsafe {
            command.pre_exec(move || {
                if libc::setsid() < 0 || libc::dup2(fd, 3) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = ChildGuard(
            command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        drop(child_socket);
        drop(inherited);
        let mut channel = BufReader::new(parent);
        fn receive(channel: &mut BufReader<UnixStream>) -> Value {
            let mut line = String::new();
            channel.read_line(&mut line).unwrap();
            serde_json::from_str(&line).unwrap()
        }
        assert_eq!(
            receive(&mut channel),
            json!({"result":"ready","protocol":1,"device":ID})
        );
        channel.get_mut().write_all(b"{\"op\":\"prepare\",\"id\":1,\"deadline_ns\":0,\"settings\":{\"dpi\":1850},\"buttons\":{\"83\":\"modifier:ctrl\"}}\n").unwrap();
        let reply = receive(&mut channel);
        assert_eq!(reply["result"], "error");
        assert!(reply["error"].as_str().unwrap().contains(expected));
        channel
            .get_mut()
            .write_all(b"{\"op\":\"shutdown\",\"id\":2}\n")
            .unwrap();
        assert_eq!(receive(&mut channel), json!({"result":"stopped","id":2}));
        assert!(child.0.wait().unwrap().success());
        assert!(!dir.0.join("linux-scrollmaster").exists());
        assert!(!dir.0.join("missing-runtime").exists());
    }
}
