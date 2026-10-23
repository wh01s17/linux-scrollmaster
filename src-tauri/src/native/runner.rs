// SPDX-License-Identifier: GPL-3.0-or-later
//! The CLI owns a separate process so a driver syscall cannot block its parent.
use serde_json::Value;
use std::{
    io::Read,
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

const MAX_OUTPUT: usize = 128 * 1024;
struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Used by the opt-in diagnostic example only. No command is accepted via IPC.
/// kill/wait still depends on the kernel returning from uninterruptible device I/O.
pub fn run_bounded(mut command: Command, timeout: Duration) -> Result<Value, String> {
    run_mode(&mut command, timeout, true)
}

/// Explicit N3 tool only; the read-only diagnostic keeps its stricter envelope.
pub fn run_bounded_settings(mut command: Command, timeout: Duration) -> Result<Value, String> {
    run_mode(&mut command, timeout, false)
}

fn run_mode(command: &mut Command, timeout: Duration, read_only: bool) -> Result<Value, String> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .filter(|_| !timeout.is_zero())
        .ok_or("Invalid diagnostic timeout")?;
    let mut child = ChildGuard(
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?,
    );
    let stdout = child.0.stdout.take().ok_or("No diagnostic output pipe")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take((MAX_OUTPUT + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())
            .and_then(|_| {
                if bytes.len() > MAX_OUTPUT {
                    Err("Diagnostic output too large".into())
                } else {
                    Ok(bytes)
                }
            });
        let _ = tx.send(result);
    });
    let mut output = None;
    loop {
        if Instant::now() >= deadline {
            return Err("Native diagnostic timed out; child terminated without retry".into());
        }
        if output.is_none() {
            match rx.try_recv() {
                Ok(Ok(bytes)) => output = Some(bytes),
                Ok(Err(error)) => return Err(error),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("Diagnostic reader failed".into())
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(status) = child.0.try_wait().map_err(|e| e.to_string())? {
            let bytes = match output.take() {
                Some(bytes) => bytes,
                None => rx
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .map_err(|_| "Incomplete diagnostic output")??,
            };
            let result: Value =
                serde_json::from_slice(&bytes).map_err(|_| "Invalid diagnostic JSON")?;
            if !result.is_object() || !result.get("ok").is_some_and(Value::is_boolean) {
                return Err("Invalid diagnostic envelope".into());
            }
            if result["ok"] == true
                && (!status.success()
                    || result["data"]["read_only"] != read_only
                    || (!read_only && result["data"]["stage"] != "N3"))
            {
                return Err("Invalid read-only diagnostic result".into());
            }
            return Ok(result);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn shell(code: &str) -> Command {
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", code]);
        cmd
    }
    #[test]
    fn child_result_is_validated() {
        assert_eq!(
            run_bounded(
                shell("printf '%s' '{\"ok\":true,\"data\":{\"read_only\":true}}'"),
                Duration::from_secs(1)
            )
            .unwrap()["ok"],
            true
        );
        for code in [
            "printf '%s' '[]'",
            "printf '%s' '{\"ok\":true,\"data\":{}}'",
            "printf '%s' '{\"ok\":true,\"data\":{\"read_only\":true}}'; exit 1",
        ] {
            assert!(run_bounded(shell(code), Duration::from_secs(1)).is_err());
        }
    }
    #[test]
    fn blocked_child_is_killed_and_reaped() {
        let start = Instant::now();
        assert!(
            run_bounded(shell("exec sleep 2"), Duration::from_millis(30))
                .unwrap_err()
                .contains("timed out")
        );
        assert!(start.elapsed() < Duration::from_secs(1));
    }
    #[test]
    fn output_is_bounded() {
        assert!(run_bounded(
            shell("exec head -c 131073 /dev/zero"),
            Duration::from_secs(1)
        )
        .unwrap_err()
        .contains("too large"));
    }

    #[test]
    fn settings_envelope_is_explicit_and_not_accepted_as_a_read_only_diagnostic() {
        let code = "printf '%s' '{\"ok\":true,\"data\":{\"stage\":\"N3\",\"read_only\":false}}'";
        assert!(run_bounded_settings(shell(code), Duration::from_secs(1)).is_ok());
        assert!(run_bounded(shell(code), Duration::from_secs(1)).is_err());
        assert!(run_bounded_settings(
            shell("printf '%s' '{\"ok\":true,\"data\":{\"read_only\":false}}'"),
            Duration::from_secs(1)
        )
        .is_err());
    }
}
