// SPDX-License-Identifier: GPL-3.0-or-later
//! Closed Hyprland actions. Each operation resolves the current session anew;
//! an uncertain dispatcher reply is never retried.
use std::{
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{ffi::OsStrExt, net::UnixStream},
    },
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(1);
const PROBE: &[u8] = b"/eval return assert(type(hl.dsp.focus) == \"function\" and type(hl.dsp.window.fullscreen) == \"function\")";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopAction {
    WorkspacePrevious,
    WorkspaceNext,
    Fullscreen,
    Maximize,
}
impl DesktopAction {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "desktop:workspace-previous" => Ok(Self::WorkspacePrevious),
            "desktop:workspace-next" => Ok(Self::WorkspaceNext),
            "desktop:fullscreen" => Ok(Self::Fullscreen),
            "desktop:maximize" => Ok(Self::Maximize),
            _ => Err("Unknown desktop action".into()),
        }
    }
    fn payload(self) -> &'static [u8] {
        match self {
            Self::WorkspacePrevious => b"/dispatch hl.dsp.focus({ workspace = \"r-1\" })",
            Self::WorkspaceNext => b"/dispatch hl.dsp.focus({ workspace = \"r+1\" })",
            Self::Fullscreen => b"/dispatch hl.dsp.window.fullscreen({ mode = \"fullscreen\", action = \"toggle\" })",
            Self::Maximize => b"/dispatch hl.dsp.window.fullscreen({ mode = \"maximized\", action = \"toggle\" })",
        }
    }
}

pub(super) trait Dispatcher {
    fn require_actions(&mut self) -> Result<(), String>;
    fn dispatch(&mut self, action: DesktopAction) -> Result<(), String>;
}
pub(super) struct Hyprland<R = fn() -> Result<PathBuf, String>> {
    resolve: R,
    timeout: Duration,
}
impl Hyprland {
    pub(super) fn environment() -> Self {
        Self {
            resolve: environment_path,
            timeout: TIMEOUT,
        }
    }
}
impl<R: FnMut() -> Result<PathBuf, String>> Hyprland<R> {
    fn preflight(&self, path: &Path) -> Result<(), String> {
        let reply = exchange(path, b"j/version", 1024 * 1024, self.timeout)?;
        let version: serde_json::Value =
            serde_json::from_slice(&reply).map_err(|_| "Invalid Hyprland version reply")?;
        if !version["version"].as_str().is_some_and(supported_version) {
            return Err("Desktop actions require Hyprland 0.55, 0.56 or 0.57 with Lua IPC".into());
        }
        require_ok(&exchange(path, PROBE, 4096, self.timeout)?)
    }
}
impl<R: FnMut() -> Result<PathBuf, String>> Dispatcher for Hyprland<R> {
    fn require_actions(&mut self) -> Result<(), String> {
        let path = (self.resolve)()?;
        self.preflight(&path)
    }
    fn dispatch(&mut self, action: DesktopAction) -> Result<(), String> {
        // One target for this entire action, no session discovery/fallback.
        // Do not reuse a successful capability check from an earlier action.
        let path = (self.resolve)()?;
        self.preflight(&path)?;
        require_ok(&exchange(&path, action.payload(), 4096, self.timeout)?)
    }
}
/// Presentation-only capability: the same version check and side-effect-free
/// probe used before dispatch. Never dispatches or retains a session target.
pub fn desktop_actions_status() -> serde_json::Value {
    match Hyprland::environment().require_actions() {
        Ok(()) => serde_json::json!({"available":true,"error":null}),
        Err(error) => serde_json::json!({"available":false,"error":error}),
    }
}
fn session_path(runtime: &Path, signature: &str) -> Result<PathBuf, String> {
    let mut components = Path::new(signature).components();
    if !runtime.is_absolute()
        || signature.is_empty()
        || signature.bytes().any(|b| b == b'/' || b.is_ascii_control())
        || !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
    {
        return Err("Missing or invalid Hyprland session environment".into());
    }
    Ok(runtime.join("hypr").join(signature).join(".socket.sock"))
}
pub(super) fn environment_path() -> Result<PathBuf, String> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").ok_or("Missing XDG_RUNTIME_DIR")?;
    let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")
        .map_err(|_| "Missing Hyprland instance signature")?;
    session_path(Path::new(&runtime), &signature)
}
pub(super) fn supported_version(value: &str) -> bool {
    let value = value.strip_prefix('v').unwrap_or(value);
    let Some(rest) = value
        .strip_prefix("0.55.")
        .or_else(|| value.strip_prefix("0.56."))
        .or_else(|| value.strip_prefix("0.57."))
    else {
        return false;
    };
    let (patch, suffix) = rest
        .find(['-', '+'])
        .map_or((rest, None), |i| (&rest[..i], Some(&rest[i + 1..])));
    !patch.is_empty()
        && patch.bytes().all(|b| b.is_ascii_digit())
        && suffix.is_none_or(|s| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        })
}
pub(super) fn require_ok(reply: &[u8]) -> Result<(), String> {
    if reply.trim_ascii() == b"ok" {
        Ok(())
    } else {
        Err("Hyprland did not confirm the desktop request; it will not be retried".into())
    }
}

fn wait(fd: libc::c_int, events: libc::c_short, deadline: Instant) -> Result<(), String> {
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or("Hyprland IPC timed out; no retry")?;
        let mut poll = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        // Round upward to avoid prematurely expiring a sub-millisecond budget.
        let millis = remaining
            .as_millis()
            .saturating_add(1)
            .min(i32::MAX as u128) as i32;
        let result = unsafe { libc::poll(&mut poll, 1, millis) };
        if result > 0 {
            if Instant::now() >= deadline {
                return Err("Hyprland IPC timed out; no retry".into());
            }
            if poll.revents & libc::POLLNVAL != 0 {
                return Err("Invalid Hyprland socket".into());
            }
            return Ok(()); // HUP/ERR are resolved by the following I/O/SO_ERROR.
        }
        if result < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return Err("Hyprland IPC poll failed".into());
        }
    }
}
fn connect(path: &Path, deadline: Instant) -> Result<UnixStream, String> {
    let bytes = path.as_os_str().as_bytes();
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.is_empty() || bytes.contains(&0) || bytes.len() >= address.sun_path.len() {
        return Err("Invalid Hyprland socket path".into());
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (target, byte) in address.sun_path.iter_mut().zip(bytes) {
        *target = *byte as libc::c_char;
    }
    let raw = unsafe {
        libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
            0,
        )
    };
    if raw < 0 {
        return Err("Cannot create Hyprland IPC socket".into());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let result = unsafe {
        libc::connect(
            fd.as_raw_fd(),
            (&address as *const libc::sockaddr_un).cast(),
            std::mem::size_of_val(&address) as libc::socklen_t,
        )
    };
    if result < 0 {
        if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINPROGRESS) {
            return Err("Cannot connect to this Hyprland session".into());
        }
        wait(fd.as_raw_fd(), libc::POLLOUT, deadline)?;
        let mut error: libc::c_int = 0;
        let mut size = std::mem::size_of_val(&error) as libc::socklen_t;
        if unsafe {
            libc::getsockopt(
                fd.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_ERROR,
                (&mut error as *mut libc::c_int).cast(),
                &mut size,
            )
        } != 0
            || error != 0
        {
            return Err("Cannot complete Hyprland IPC connection".into());
        }
    }
    let mut peer: libc::ucred = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of_val(&peer) as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            fd.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut peer as *mut libc::ucred).cast(),
            &mut size,
        )
    } != 0
        || peer.uid != unsafe { libc::geteuid() }
    {
        return Err("Hyprland socket belongs to another user".into());
    }
    Ok(UnixStream::from(fd))
}
fn exchange(
    path: &Path,
    payload: &[u8],
    limit: usize,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    let deadline = Instant::now() + timeout;
    exchange_until(path, payload, limit, deadline)
}
/// Internal shared transport. Callers supply fixed requests, never IPC strings.
pub(super) fn exchange_until(
    path: &Path,
    payload: &[u8],
    limit: usize,
    deadline: Instant,
) -> Result<Vec<u8>, String> {
    if Instant::now() >= deadline {
        return Err("Hyprland IPC timed out; no retry".into());
    }
    let mut socket = connect(path, deadline)?;
    let mut sent = 0;
    while sent < payload.len() {
        wait(socket.as_raw_fd(), libc::POLLOUT, deadline)?;
        let written = socket.write(&payload[sent..]);
        if Instant::now() >= deadline {
            return Err("Hyprland IPC timed out; no retry".into());
        }
        match written {
            Ok(0) => return Err("Hyprland IPC write closed".into()),
            Ok(n) => sent += n,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) => {}
            Err(_) => return Err("Hyprland IPC write failed; no retry".into()),
        }
    }
    let mut reply = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        wait(socket.as_raw_fd(), libc::POLLIN, deadline)?;
        let remaining = (limit + 1 - reply.len()).min(buffer.len());
        let received = socket.read(&mut buffer[..remaining]);
        if Instant::now() >= deadline {
            return Err("Hyprland IPC timed out; no retry".into());
        }
        match received {
            Ok(0) => return Ok(reply),
            Ok(n) => {
                reply.extend_from_slice(&buffer[..n]);
                if reply.len() > limit {
                    return Err("Hyprland IPC reply exceeds its limit".into());
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) => {}
            Err(_) => return Err("Hyprland IPC read failed; no retry".into()),
        }
    }
}

#[cfg(test)]
#[path = "desktop_tests.rs"]
mod tests;
