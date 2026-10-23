// SPDX-License-Identifier: GPL-3.0-or-later
//! Lifetime and diagnostic helpers for synthetic guardian subprocesses only.
use super::*;
use std::{
    io::Write,
    os::unix::fs::DirBuilderExt,
    process::{Child, ExitStatus, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};

const PENDING: &str = "guardian-starting";
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(25);

pub(super) struct Sandbox(pub(super) PathBuf);
impl Sandbox {
    pub(super) fn new() -> Self {
        static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "scrollmaster-guardian-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }

    pub(super) fn client(&self, mode: &str) -> FixtureClient<'_> {
        // Mark before spawn: even an early test failure must leave the directory
        // available to a guardian that has not published session.json yet.
        mark_pending(&self.0);
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", OWNER_TEST, "--nocapture"])
            .env("SCROLLMASTER_GUARD_CLIENT", mode)
            .env("SCROLLMASTER_GUARD_DIR", &self.0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        FixtureClient {
            child,
            _sandbox: self,
        }
    }

    fn cleanup(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        // result.json is published only after serve_with_keyboard has dropped
        // keyboard/transport and completed recovery. Never delete their logs
        // while the asynchronous production reaper is still doing that work.
        while self.0.join(PENDING).exists() && !self.0.join("result.json").exists() {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        match fs::remove_dir_all(&self.0) {
            Ok(()) => true,
            Err(error) => error.kind() == io::ErrorKind::NotFound,
        }
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        if !self.cleanup(CLEANUP_TIMEOUT) {
            let _ = writeln!(
                io::stderr(),
                "Guardian fixture cleanup pending; preserving {}",
                self.0.display()
            );
        }
    }
}

// The borrow ensures this client is stopped and reaped before its sandbox can
// be dropped, including when a wait_file assertion unwinds. Only the synthetic
// client is killed; its guardian remains free to finish restoration after EOF.
pub(super) struct FixtureClient<'a> {
    child: Child,
    _sandbox: &'a Sandbox,
}
impl FixtureClient<'_> {
    pub(super) fn kill(&mut self) -> io::Result<()> {
        self.child.kill()
    }
    pub(super) fn wait(&mut self) -> io::Result<ExitStatus> {
        self.child.wait()
    }
}
impl Drop for FixtureClient<'_> {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub(super) fn mark_pending(path: &Path) {
    fs::write(path.join(PENDING), b"pending").unwrap();
}
pub(super) fn wait_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "Timed out waiting for {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
pub(super) fn log(path: &Path, event: &str) {
    // Logging must not panic in Drop or interrupt resource release. Tests still
    // require the expected wire.log entries, so lost evidence fails assertions.
    let result = (|| -> io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path.join("wire.log"))?;
        writeln!(file, "{event}")
    })();
    if let Err(error) = result {
        let _ = writeln!(
            io::stderr(),
            "Guardian fixture log {}: {error}",
            path.display()
        );
    }
}
pub(super) fn result(path: &Path) -> Value {
    wait_file(&path.join("result.json"));
    // Published with rename so observers cannot read partial JSON.
    serde_json::from_slice(&fs::read(path.join("result.json")).unwrap()).unwrap()
}

#[test]
fn unwind_reaps_client_before_removing_guardian_sandbox() {
    let mut path = PathBuf::new();
    let mut pid = 0;
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let dir = Sandbox::new();
        path = dir.0.clone();
        let client = dir.client("keyboard-events");
        pid = client.child.id();
        wait_file(&dir.0.join("client-ready"));
        assert!(HardwareLease::at(&dir.0).is_err());
        panic!("Synthetic parent test failure");
    }));
    assert_eq!(
        failure.unwrap_err().downcast_ref::<&str>(),
        Some(&"Synthetic parent test failure")
    );
    assert!(
        !path.exists(),
        "Sandbox must be removed after guardian cleanup"
    );
    // A second wait must find no child: the guard already reaped it.
    let mut status = 0;
    assert_eq!(
        unsafe { libc::waitpid(pid as libc::pid_t, &mut status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

#[test]
fn pending_guardian_preserves_sandbox_until_final_report() {
    let dir = Sandbox::new();
    let path = dir.0.clone();
    mark_pending(&path);
    assert!(!dir.cleanup(Duration::ZERO));
    assert!(path.exists());
    fs::write(path.join("result.json"), b"{}").unwrap();
    drop(dir);
    assert!(!path.exists());
}

#[test]
fn missing_log_directory_during_unwind_does_not_abort() {
    const CHILD_TEST: &str = "native::buttons_tests::guardian_tests::fixture_support::missing_log_directory_during_unwind_does_not_abort";
    if std::env::var_os("SCROLLMASTER_CLEANUP_UNWIND_CHILD").is_none() {
        // A regression may abort rather than unwind; isolate it from the suite.
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", CHILD_TEST, "--nocapture"])
            .env("SCROLLMASTER_CLEANUP_UNWIND_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        return;
    }
    let dir = Sandbox::new();
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (io, state) = Device::new();
        let _mouse = FixtureMouse {
            io,
            state,
            pending: None,
            round: 0,
            armed: false,
            mode: "keyboard-events".into(),
            path: dir.0.clone(),
            notifications: Default::default(),
            capture_waiting: false,
            emitted: false,
        };
        let _keyboard = FixtureKeyboard {
            path: dir.0.clone(),
            close_error: false,
            closed: false,
            engine: Some(
                Runtime::new(
                    FixtureOutput {
                        path: dir.0.clone(),
                        mode: "keyboard-events".into(),
                    },
                    ActionBindings::parse(&mode_bindings("keyboard-events")).unwrap(),
                    None,
                )
                .unwrap(),
            ),
        };
        fs::remove_dir_all(&dir.0).unwrap();
        panic!("Synthetic failure after losing fixture directory");
    }));
    assert_eq!(
        failure.unwrap_err().downcast_ref::<&str>(),
        Some(&"Synthetic failure after losing fixture directory")
    );
}
