use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::fd::AsRawFd,
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

/// One serialized JSON-lines worker; killed and reaped on timeout, EOF or host shutdown.
pub struct Worker {
    child: Child,
    stdin: ChildStdin,
    responses: Receiver<Result<Value, String>>,
    failed: bool,
}
impl Worker {
    pub fn spawn(script: &Path) -> Result<Self, String> {
        let mut command = Command::new("/usr/bin/python");
        command.arg("-I").arg(script).arg("--serve");
        Self::start(command)
    }
    /// Explicit experimental launcher. No fallback or automatic selection of a
    /// backend; the active GUI still calls spawn for the legacy worker.
    #[cfg(target_os = "linux")]
    pub fn spawn_native(
        executable: &Path,
        config: &crate::worker::WorkerConfig,
    ) -> Result<Self, String> {
        let mut command = Command::new(executable);
        command.args([
            config.serve_flag(),
            "--slot",
            &config.slot().to_string(),
            "--device",
            config.identity(),
        ]);
        Self::start(command)
    }
    /// Trusted Rust launcher only: never construct a command from UI IPC fields.
    pub fn start(mut command: Command) -> Result<Self, String> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("No se pudo iniciar el servicio: {e}"))?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Sin canales al servicio".into());
        };
        // Bound the complete transaction, including a child that never reads
        // stdin. Blocking write_all before recv_timeout could stall indefinitely.
        let fd = stdin.as_raw_fd();
        // SAFETY: a live owned pipe descriptor; fcntl does not transfer ownership.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            let _ = child.kill();
            let _ = child.wait();
            return Err("No se pudo limitar la escritura al servicio".into());
        }
        let (tx, responses) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut bytes = Vec::new();
                let count = (&mut reader)
                    .take(2 * 1024 * 1024 + 1)
                    .read_until(b'\n', &mut bytes);
                match count {
                    Ok(0) => break,
                    Ok(n) if n <= 2 * 1024 * 1024 && bytes.last() == Some(&b'\n') => {
                        let parsed = serde_json::from_slice::<Value>(&bytes)
                            .map_err(|_| "Respuesta inválida del servicio".to_string())
                            .and_then(|v| {
                                if v.is_object() && v.get("ok").is_some_and(Value::is_boolean) {
                                    Ok(v)
                                } else {
                                    Err("Contrato del servicio inválido".into())
                                }
                            });
                        if tx.send(parsed).is_err() {
                            break;
                        }
                    }
                    _ => {
                        let _ = tx.send(Err(
                            "Respuesta del servicio truncada o demasiado grande".into()
                        ));
                        break;
                    }
                }
            }
        });
        Ok(Self {
            child,
            stdin,
            responses,
            failed: false,
        })
    }
    pub fn transact(&mut self, payload: &Value, timeout: Duration) -> Result<Value, String> {
        if self.failed {
            return Err("El servicio está cerrado; inicia otro antes de consultar".into());
        }
        let mut bytes = serde_json::to_vec(payload).map_err(|e| e.to_string())?;
        if bytes.len() > 256 * 1024 {
            return Err("Solicitud demasiado grande".into());
        }
        bytes.push(b'\n');
        let result = (|| {
            let deadline = Instant::now()
                .checked_add(timeout)
                .filter(|_| !timeout.is_zero())
                .ok_or("Plazo de servicio inválido")?;
            self.send(&bytes, deadline)?;
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or("El servicio agotó el plazo de envío")?;
            let response = self.responses.recv_timeout(remaining).map_err(|_| {
                "El servicio no respondió. Se reiniciará automáticamente en la siguiente consulta.".to_string()
            })??;
            if Instant::now() >= deadline {
                return Err("Respuesta fuera del plazo del servicio".into());
            }
            Ok(response)
        })();
        if result.is_err() {
            self.stop();
        }
        result
    }
    fn send(&mut self, mut bytes: &[u8], deadline: Instant) -> Result<(), String> {
        while !bytes.is_empty() {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or("El servicio no recibió la solicitud a tiempo")?;
            let mut fd = libc::pollfd {
                fd: self.stdin.as_raw_fd(),
                events: libc::POLLOUT,
                revents: 0,
            };
            let ms = remaining.as_millis().clamp(1, i32::MAX as u128) as i32;
            // SAFETY: poll gets one initialized descriptor valid for this call.
            let result = unsafe { libc::poll(&mut fd, 1, ms) };
            if result < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(std::io::Error::last_os_error().to_string());
            }
            if result == 0 {
                continue;
            }
            if fd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                return Err("Canal de entrada del servicio cerrado".into());
            }
            if Instant::now() >= deadline {
                return Err("El servicio agotó el plazo de envío".into());
            }
            match self.stdin.write(bytes) {
                Ok(0) => return Err("El servicio no acepta más datos".into()),
                Ok(n) => bytes = &bytes[n..],
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock
                    ) => {}
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(())
    }
    fn stop(&mut self) {
        if !self.failed {
            self.failed = true;
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn worker(code: &str) -> Worker {
        // Any Python 3 from PATH: these fixtures only echo JSON (CI images ship
        // python3 without /usr/bin/python). Production keeps the system path.
        let mut cmd = Command::new("python3");
        cmd.args(["-u", "-c", code]);
        Worker::start(cmd).unwrap()
    }
    #[test]
    fn handles_multiple_requests_without_restarting() {
        let mut w=worker("import sys,json\nfor line in sys.stdin: print(json.dumps({'ok':True,'echo':json.loads(line)}),flush=True)");
        let pid = w.child.id();
        for i in 0..3 {
            let p = serde_json::json!({"i":i});
            assert_eq!(w.transact(&p, Duration::from_secs(2)).unwrap()["echo"], p);
            assert_eq!(w.child.id(), pid);
        }
    }
    #[test]
    fn timeout_and_drop_reap_child() {
        let mut w = worker("import time; time.sleep(5)");
        assert!(w.transact(&Value::Null, Duration::from_millis(50)).is_err());
        // Drop performs kill + wait, not a detached background process.
        drop(w);
    }
    #[test]
    fn invalid_response_is_rejected() {
        let mut w = worker("import sys\nfor line in sys.stdin: print('[]',flush=True)");
        assert!(w.transact(&Value::Null, Duration::from_secs(2)).is_err());
        assert!(w.failed);
        assert!(w.child.try_wait().unwrap().is_some());
    }
    #[test]
    fn child_that_never_reads_cannot_block_a_full_pipe_past_the_deadline() {
        let mut w = worker("import time; time.sleep(5)");
        let start = Instant::now();
        assert!(w
            .transact(
                &serde_json::json!({"data":"x".repeat(200*1024)}),
                Duration::from_millis(100)
            )
            .is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(w.child.try_wait().unwrap().is_some());
        assert!(w
            .transact(&Value::Null, Duration::from_secs(1))
            .unwrap_err()
            .contains("cerrado"));
    }
    #[test]
    fn request_larger_than_pipe_capacity_arrives_once_and_intact() {
        let mut w=worker("import sys,json\nfor line in sys.stdin: print(json.dumps({'ok':True,'length':len(json.loads(line)['data'])}),flush=True)");
        for size in [200 * 1024, 1] {
            assert_eq!(
                w.transact(
                    &serde_json::json!({"data":"x".repeat(size)}),
                    Duration::from_secs(5)
                )
                .unwrap()["length"],
                size
            );
        }
        assert!(!w.failed);
    }
    #[test]
    fn eof_truncation_and_oversized_response_are_terminal_and_reaped() {
        for code in [
            "import sys; sys.stdin.readline()",
            "import sys; sys.stdin.readline(); print('{\"ok\":true}',end='',flush=True)",
            "import sys; sys.stdin.readline(); print('x'*(2*1024*1024+1),flush=True)",
        ] {
            let mut w = worker(code);
            assert!(w.transact(&Value::Null, Duration::from_secs(5)).is_err());
            assert!(w.failed);
            assert!(w.child.try_wait().unwrap().is_some());
        }
    }
    #[test]
    fn oversized_local_request_does_not_kill_an_unused_valid_worker() {
        let mut w =
            worker("import sys,json\nfor line in sys.stdin: print('{\"ok\":true}',flush=True)");
        assert!(w
            .transact(
                &Value::String("x".repeat(256 * 1024)),
                Duration::from_secs(2)
            )
            .is_err());
        assert!(!w.failed);
        assert_eq!(
            w.transact(&Value::Null, Duration::from_secs(2)).unwrap()["ok"],
            true
        );
    }
    #[test]
    fn service_rejection_is_a_valid_response_and_keeps_the_process() {
        let mut w=worker("import sys\nfor line in sys.stdin: print('{\"ok\":false,\"error\":\"invalid request\"}',flush=True)");
        let pid = w.child.id();
        for _ in 0..2 {
            assert_eq!(
                w.transact(&Value::Null, Duration::from_secs(2)).unwrap()["ok"],
                false
            );
        }
        assert!(!w.failed);
        assert_eq!(w.child.id(), pid);
    }
    #[test]
    fn zero_deadline_fails_before_sending_and_reaps() {
        let mut w = worker("import time; time.sleep(5)");
        assert!(w.transact(&Value::Null, Duration::ZERO).is_err());
        assert!(w.failed);
        assert!(w.child.try_wait().unwrap().is_some());
    }
}
