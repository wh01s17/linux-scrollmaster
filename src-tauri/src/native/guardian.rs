// SPDX-License-Identifier: GPL-3.0-or-later
//! Independent finite owner/recovery process. The client never owns HID or its lease.
use super::keyboard::{GuardianKeyboard, WaylandKeyboard};
use super::{discover, ensure_no_controllers, HardwareLease, PassiveEventOwner, ReportingRecovery};
use super::{events::SinkHandle, ActionBindings, GuardianConfig, NativeActions};
use crate::hidpp::{Session, Transport};
use serde_json::{json, Value};
use std::{
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::{net::UnixStream, process::CommandExt},
    },
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const CHANNEL_FD: RawFd = 3;
const MAX_REPLY: usize = 128 * 1024;
const MAX_COMMAND: usize = 256;
const IO_TIMEOUT: Duration = Duration::from_secs(2);

fn timed_out() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "Guardian IPC deadline elapsed")
}
fn wait(fd: RawFd, events: libc::c_short, deadline: Instant) -> io::Result<()> {
    loop {
        let left = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(timed_out)?;
        let mut p = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        // SAFETY: one initialized pollfd; the caller keeps its channel alive.
        let n = unsafe {
            libc::poll(
                &mut p,
                1,
                left.as_millis().saturating_add(1).min(i32::MAX as u128) as i32,
            )
        };
        if n < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        if Instant::now() >= deadline {
            return Err(timed_out());
        }
        if p.revents & (events | libc::POLLHUP | libc::POLLERR) != 0 {
            return Ok(());
        }
        if p.revents & libc::POLLNVAL != 0 {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
    }
}
pub(super) fn peer_gone(fd: RawFd) -> bool {
    let mut p = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: no read or write; the channel outlives every cancellation callback.
    let n = unsafe { libc::poll(&mut p, 1, 0) };
    (n < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted)
        || p.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0
}

pub(super) struct Channel {
    stream: UnixStream,
    buffer: Vec<u8>,
}
impl Channel {
    pub(super) fn fd(&self) -> RawFd {
        self.stream.as_raw_fd()
    }
    pub(super) fn try_receive_bytes(&mut self, limit: usize) -> io::Result<Option<Vec<u8>>> {
        self.try_frame(limit)
    }
    pub(super) fn wait_input_until(&self, deadline: Instant) -> io::Result<()> {
        wait(self.fd(), libc::POLLIN, deadline)
    }
    pub(super) fn inherited() -> Result<Self, String> {
        // Only IPC is inherited; hardware and lease are opened by the child.
        let fd = unsafe { libc::fcntl(CHANNEL_FD, libc::F_DUPFD_CLOEXEC, 64) };
        if fd < 0 {
            return Err(format!(
                "Missing private guardian channel: {}",
                io::Error::last_os_error()
            ));
        }
        let owned = unsafe { OwnedFd::from_raw_fd(fd) };
        let channel = Self::new(UnixStream::from(owned)).map_err(|e| e.to_string())?;
        channel
            .stream
            .peer_addr()
            .map_err(|_| "Guardian channel must be a Unix socket")?;
        let mut peer: libc::ucred = unsafe { std::mem::zeroed() };
        let mut size = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: initialized peer/size buffers, sized exactly for SO_PEERCRED.
        if unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut peer as *mut libc::ucred).cast(),
                &mut size,
            )
        } < 0
            || size as usize != std::mem::size_of::<libc::ucred>()
            || peer.uid != unsafe { libc::getuid() }
            || peer.pid != unsafe { libc::getppid() }
        {
            return Err("Guardian channel must belong to its launching parent and user".into());
        }
        // SAFETY: only changes exec inheritance in this fresh helper process.
        if unsafe { libc::fcntl(CHANNEL_FD, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return Err(io::Error::last_os_error().to_string());
        }
        Ok(channel)
    }
    pub(super) fn new(stream: UnixStream) -> io::Result<Self> {
        stream.set_nonblocking(true)?;
        Ok(Self {
            stream,
            buffer: vec![],
        })
    }
    fn send(&mut self, value: &Value, deadline: Instant) -> io::Result<()> {
        self.send_bounded(value, MAX_REPLY, deadline)
    }
    pub(super) fn send_bounded(
        &mut self,
        value: &Value,
        limit: usize,
        deadline: Instant,
    ) -> io::Result<()> {
        let mut bytes = serde_json::to_vec(value)?;
        if bytes.len() > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Guardian reply too large",
            ));
        }
        bytes.push(b'\n');
        let mut offset = 0;
        while offset < bytes.len() {
            wait(self.stream.as_raw_fd(), libc::POLLOUT, deadline)?;
            match self.stream.write(&bytes[offset..]) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(n) => offset += n,
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                    ) =>
                {
                    continue
                }
                Err(e) => return Err(e),
            }
        }
        if Instant::now() >= deadline {
            return Err(timed_out());
        }
        Ok(())
    }
    pub(super) fn try_receive(&mut self, limit: usize) -> io::Result<Option<Value>> {
        self.try_frame(limit)?
            .map(|bytes| serde_json::from_slice(&bytes).map_err(Into::into))
            .transpose()
    }
    fn try_frame(&mut self, limit: usize) -> io::Result<Option<Vec<u8>>> {
        loop {
            if let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
                if end > limit {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Guardian frame too large",
                    ));
                }
                let frame: Vec<_> = self.buffer.drain(..=end).collect();
                return Ok(Some(frame));
            }
            if self.buffer.len() > limit {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Guardian frame too large",
                ));
            }
            let mut bytes = [0; 1024];
            match self.stream.read(&mut bytes) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(n) => self.buffer.extend_from_slice(&bytes[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(None),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }
    fn receive(&mut self, deadline: Instant) -> io::Result<Value> {
        self.receive_bounded(MAX_REPLY, deadline)
    }
    pub(super) fn has_pending_bytes(&self) -> bool {
        !self.buffer.is_empty()
    }
    pub(super) fn receive_bounded(&mut self, limit: usize, deadline: Instant) -> io::Result<Value> {
        serde_json::from_slice(&self.receive_bytes_bounded(limit, deadline)?).map_err(Into::into)
    }
    pub(super) fn receive_bytes_bounded(
        &mut self,
        limit: usize,
        deadline: Instant,
    ) -> io::Result<Vec<u8>> {
        loop {
            if Instant::now() >= deadline {
                return Err(timed_out());
            }
            if let Some(frame) = self.try_frame(limit)? {
                return Ok(frame);
            }
            wait(self.stream.as_raw_fd(), libc::POLLIN, deadline)?;
        }
    }
}

// A lexical borrow, not a duplicate descriptor. Dropping/closing an active
// Session releases this borrow before terminal recovery can borrow the transport.
struct Borrowed<'a, T>(&'a mut T);
impl<T: Transport> Transport for Borrowed<'_, T> {
    fn write_report(&mut self, p: &[u8], deadline: Instant) -> io::Result<()> {
        self.0.write_report(p, deadline)
    }
    fn read_report(&mut self, deadline: Instant) -> io::Result<Vec<u8>> {
        self.0.read_report(deadline)
    }
}

/// Start an independent helper with one private authenticated channel.
pub(super) fn spawn_channel(mut command: Command) -> Result<(Child, UnixStream), String> {
    let (parent, child_channel) = UnixStream::pair().map_err(|e| e.to_string())?;
    // A high CLOEXEC source cannot collide with stdio or target fd 3 in pre_exec.
    let inherited = unsafe { libc::fcntl(child_channel.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 64) };
    if inherited < 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    let inherited = unsafe { OwnedFd::from_raw_fd(inherited) };
    let source = inherited.as_raw_fd();
    // SAFETY: only async-signal-safe setsid/dup2 run between fork and exec. The owned
    // high source stays alive until spawn returns; the parent flags are untouched.
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() < 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::dup2(source, CHANNEL_FD) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command
        // Only our private IPC endpoint is inherited. A WAYLAND_SOCKET fd
        // from the client would be stale (or collide with fd 3) after exec;
        // the optional keyboard opens a fresh WAYLAND_DISPLAY connection.
        .env_remove("WAYLAND_SOCKET")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    drop(child_channel);
    drop(inherited);
    Ok((child, parent))
}

/// Called only by the standalone example's --guardian-child startup branch.
/// The inherited private channel carries no paths, HID bytes or shell commands.
pub fn run_guardian_child(config: GuardianConfig) -> Result<Value, String> {
    let mut channel = Channel::inherited()?;
    let setup = (|| -> Result<_, String> {
        ensure_no_controllers()?;
        let lease = HardwareLease::acquire()?;
        ensure_no_controllers()?;
        let discovery = discover()?;
        let candidate = discovery
            .candidates
            .iter()
            .find(|c| c.name() == config.interface)
            .ok_or("Guardian interface not in supported Bolt catalogue")?;
        Ok((candidate.open()?, lease))
    })();
    let (transport, lease) = match setup {
        Ok(setup) => setup,
        Err(error) => {
            let _ = channel.send(&json!({"event":"closed","protocol":1,"data":{
                "completed":false,"trigger":"startup-error","error":error,"owner":null,"recovery":null}}), Instant::now() + IO_TIMEOUT);
            return Err(error);
        }
    };
    Ok(serve(transport, &config, &mut channel, lease))
}

pub(super) fn serve<T: Transport>(
    transport: T,
    config: &GuardianConfig,
    channel: &mut Channel,
    lease: HardwareLease,
) -> Value {
    serve_with_keyboard(transport, config, channel, lease, |dpi| {
        if config.keyboard_check {
            let bindings = config
                .actions
                .as_ref()
                .map_or_else(ActionBindings::default, |a| a.bindings.clone());
            NativeActions::new(WaylandKeyboard::open()?, bindings, dpi).map(Some)
        } else {
            Ok(None)
        }
    })
}
pub(super) fn serve_with_keyboard<T: Transport, K: GuardianKeyboard + 'static>(
    mut transport: T,
    config: &GuardianConfig,
    channel: &mut Channel,
    lease: HardwareLease,
    factory: impl FnOnce(Option<u16>) -> Result<Option<K>, String>,
) -> Value {
    let fd = channel.stream.as_raw_fd();
    let registration = {
        let mut session =
            Session::with_initialization_cancellation(Borrowed(&mut transport), move || {
                peer_gone(fd)
            });
        ReportingRecovery::prepare(
            &mut session,
            config.slot,
            &config.id,
            &config.controls,
            Duration::from_secs(20),
        )
    };
    let plan = match registration {
        Ok(plan) => plan,
        Err(error) => {
            drop(transport);
            drop(lease);
            let result = json!({"stage":"N4","experimental":true,"mode":config.mode(),"capture_requested":config.actions.is_some(),"diversion_enabled_by_guardian":false,
                "completed":false,"trigger":"registration-error","error":error,"owner":null,"recovery":null});
            let _ = channel.send(
                &json!({"event":"closed","protocol":1,"data":result}),
                Instant::now() + IO_TIMEOUT,
            );
            return result;
        }
    };
    let mut keyboard = None;
    // All active session borrows end (including on panic) before plan.restore.
    let active = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        keyboard = factory(plan.initial_dpi())?.map(SinkHandle::new);
        if keyboard.is_some() != config.keyboard_check {
            return Err("Guardian keyboard readiness does not match the explicit configuration".into());
        }
        let mut owner = PassiveEventOwner::open_with_sink(Borrowed(&mut transport), config.slot, &config.id, config.duration, move || peer_gone(fd), keyboard.clone())?;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if !owner.is_running() { return Err("Guardian event consumer failed during initialization".into()); }
            if let Some(actions) = &config.actions {
                if let Some(profile) = &actions.profile { profile.confirm_native(owner.last_snapshot())?; }
                owner.activate_registered(&actions.bindings,&plan)?;
                if let Some(profile) = &actions.profile { profile.confirm_native(owner.last_snapshot())?; }
            }
            channel.send(&json!({"event":"ready","protocol":1,"registered":config.controls,"data":owner.status(),
                "actions":config.actions.as_ref().map(|a| &a.wire),
                "profile_buttons":config.profile_buttons(),
                "keyboard":{"requested":config.keyboard_check,"ready":keyboard.is_some()}}), Instant::now() + IO_TIMEOUT).map_err(|e| e.to_string())?;
            Ok::<_, String>(active_loop(&mut owner, channel))
        })).unwrap_or_else(|_| Err("Guardian owner loop panicked".into()));
        let status = owner.stop();
        Ok::<_, String>((status, result))
    })).unwrap_or_else(|_| Err("Guardian initialization panicked".into()));
    let (owner, trigger, mut error) = match active {
        Ok((owner, Ok((trigger, error)))) => (owner, trigger, error),
        Ok((owner, Err(error))) => (owner, "owner-error", Some(error)),
        Err(error) => (Value::Null, "setup-error", Some(error)),
    };
    // Keyboard release must precede firmware recovery, and a release error must
    // never skip mouse restoration. The concrete wrapper's close/Drop are one-shot.
    let opened = keyboard.is_some();
    let released = keyboard.as_ref().map(|keyboard| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| keyboard.close()))
            .unwrap_or_else(|_| Err("Guardian keyboard close panicked".into()))
    });
    let keyboard_error = released.as_ref().and_then(|r| r.as_ref().err()).cloned();
    let keyboard_status = json!({"requested":config.keyboard_check,"opened":opened,
        "release_confirmed":released.as_ref().map(Result::is_ok),"error":keyboard_error,
        "initialization_error":if config.keyboard_check && !opened {error.clone()} else {None}});
    drop(keyboard);
    if error.is_none() {
        error = keyboard_error;
    }
    let recovery = plan.restore(transport, Duration::from_secs(20));
    // The child owns the same lease through preparation, capture AND recovery.
    drop(lease);
    let completed =
        recovery["completed"] == true && error.is_none() && matches!(trigger, "stop" | "deadline");
    let activation = &owner["activation"];
    let may_have_diverted = activation["setters_attempted"].as_u64().unwrap_or(0) > 0;
    let diverted = if !may_have_diverted {
        Some(false)
    } else if activation["confirmed"]
        .as_array()
        .is_some_and(|a| !a.is_empty())
    {
        Some(true)
    } else {
        None
    };
    let result = json!({"stage":"N4","experimental":true,"mode":config.mode(),"capture_requested":config.actions.is_some(),
        "diversion_enabled_by_guardian":diverted,"diversion_may_have_been_enabled":may_have_diverted,
        "completed":completed,"trigger":trigger,"error":error,"owner":owner,"keyboard":keyboard_status,"recovery":recovery,
        "profile_buttons":config.profile_buttons()});
    let _ = channel.send(
        &json!({"event":"closed","protocol":1,"data":result}),
        Instant::now() + IO_TIMEOUT,
    );
    result
}

fn active_loop<T: Transport>(
    owner: &mut PassiveEventOwner<T>,
    channel: &mut Channel,
) -> (&'static str, Option<String>) {
    let mut next_id = 1u64;
    loop {
        if !owner.is_running() {
            let status = owner.status();
            let observation = &status["observation"];
            return if observation["completed"] == true {
                ("deadline", None)
            } else {
                (
                    "owner-error",
                    Some(
                        observation["error"]
                            .as_str()
                            .or_else(|| observation["sink_release_error"].as_str())
                            .unwrap_or("Passive owner closed before its deadline")
                            .into(),
                    ),
                )
            };
        }
        let command = match channel.try_receive(MAX_COMMAND) {
            Ok(command) => command,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::UnexpectedEof
                        | io::ErrorKind::BrokenPipe
                        | io::ErrorKind::ConnectionReset
                ) =>
            {
                return ("peer-lost", Some("Guardian client disconnected".into()))
            }
            Err(e) => return ("protocol-error", Some(e.to_string())),
        };
        if let Some(command) = command {
            let fields = command.as_object();
            let valid = command["id"].as_u64() == Some(next_id)
                && fields.is_some_and(|f| {
                    (f.len() == 2 && matches!(command["op"].as_str(), Some("status" | "stop")))
                        || (f.len() == 3
                            && command["op"] == "refresh"
                            && command["timeout_ms"]
                                .as_u64()
                                .is_some_and(|ms| (1..=20000).contains(&ms)))
                });
            if !valid {
                return (
                    "protocol-error",
                    Some("Invalid guardian command or sequence".into()),
                );
            }
            next_id += 1;
            if command["op"] == "stop" {
                return ("stop", None);
            }
            let result = if command["op"] == "refresh" {
                owner
                    .refresh(Duration::from_millis(
                        command["timeout_ms"].as_u64().unwrap(),
                    ))
                    .map(|_| ())
            } else {
                Ok(())
            };
            let reply = json!({"id":command["id"],"ok":result.is_ok(),"data":owner.status(),"error":result.err()});
            if let Err(e) = channel.send(&reply, Instant::now() + IO_TIMEOUT) {
                return ("peer-lost", Some(e.to_string()));
            }
        }
        let started = Instant::now();
        let _ = owner.poll(Duration::from_millis(50));
        // Synthetic/nonblocking transports may report quiet immediately.
        if started.elapsed() < Duration::from_millis(1) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

/// Experimental client for an example executable that dispatches --guardian-child.
/// Drop disconnects IPC and reaps asynchronously; it NEVER kills recovery.
pub struct NativeGuardian {
    child: Option<Child>,
    channel: Option<Channel>,
    ready: Value,
    terminal: Option<Value>,
    next_id: u64,
}
impl NativeGuardian {
    pub fn spawn(config: GuardianConfig) -> Result<Self, String> {
        ensure_no_controllers()?;
        let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
        command.args([
            "--guardian-child",
            &config.interface,
            &config.slot.to_string(),
            &config.id,
            &config
                .controls
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(","),
            &config.duration.as_millis().max(1).to_string(),
            if config.actions.as_ref().is_some_and(|a| a.profile.is_some()) {
                "capture-profile"
            } else if config.actions.is_some() {
                "capture"
            } else if config.keyboard_check {
                "keyboard-check"
            } else {
                "none"
            },
        ]);
        if let Some(actions) = &config.actions {
            let source = actions
                .profile
                .as_ref()
                .map_or(&actions.wire, |p| p.buttons());
            command.arg(serde_json::to_string(source).map_err(|e| e.to_string())?);
        }
        Self::launch(
            command,
            &config,
            Duration::from_secs(match &config.actions {
                // Include read-only compositor checks at construction and
                // accepted snapshots, without extending HID/session deadlines.
                Some(actions) if actions.bindings.uses_desktop() => 75,
                Some(_) => 65,
                None => 45,
            }),
        )
    }
    pub(super) fn launch(
        command: Command,
        config: &GuardianConfig,
        timeout: Duration,
    ) -> Result<Self, String> {
        let (child, parent) = spawn_channel(command)?;
        let mut client = Self {
            child: Some(child),
            channel: None,
            ready: Value::Null,
            terminal: None,
            next_id: 1,
        };
        client.channel = Some(Channel::new(parent).map_err(|e| e.to_string())?);
        let ready = client
            .channel
            .as_mut()
            .unwrap()
            .receive(Instant::now() + timeout)
            .map_err(|e| e.to_string())?;
        if ready["event"] != "ready"
            || ready["protocol"] != 1
            || ready["registered"] != json!(config.controls)
            || ready["data"]["device"] != config.id
            || ready["data"]["mode"]
                != if config.actions.is_some() {
                    "capture-owner"
                } else {
                    "passive-owner"
                }
            || ready["data"]["read_only"] != config.actions.is_none()
            || ready["data"]["observation"]["running"] != true
            || ready["keyboard"]["requested"] != config.keyboard_check
            || ready["keyboard"]["ready"] != config.keyboard_check
            || ready["actions"]
                != config
                    .actions
                    .as_ref()
                    .map_or(Value::Null, |a| a.wire.clone())
            || ready["profile_buttons"] != config.profile_buttons()
            || (config.actions.is_some()
                && (ready["data"]["activation"]["completed"] != true
                    || ready["data"]["activation"]["confirmed"] != json!(config.controls)
                    || ready["data"]["observation"]["capture_armed"] != true))
        {
            return Err(format!("Guardian registration was not confirmed: {ready}"));
        }
        client.ready = ready;
        Ok(client)
    }
    pub fn ready(&self) -> &Value {
        &self.ready
    }
    pub fn status(&mut self) -> Result<Value, String> {
        self.request("status", None)
    }
    pub fn refresh(&mut self, timeout: Duration) -> Result<Value, String> {
        if timeout.is_zero() || timeout > Duration::from_secs(20) {
            return Err("Invalid guardian refresh timeout".into());
        }
        self.request("refresh", Some(timeout))
    }
    fn request(&mut self, op: &str, timeout: Option<Duration>) -> Result<Value, String> {
        if self.terminal.is_some() {
            return Err("Guardian already closed; call finish for terminal evidence".into());
        }
        let id = self.next_id;
        self.next_id += 1;
        let mut command = json!({"op":op,"id":id});
        if let Some(timeout) = timeout {
            command["timeout_ms"] = json!(timeout.as_millis().max(1));
        }
        let result = (|| {
            let channel = self.channel.as_mut().ok_or("Guardian IPC is closed")?;
            // A finite owner may have sent its final report and closed before
            // this request. Read that report even when the subsequent send races
            // with peer shutdown; a broken write must not discard evidence.
            let reply = match channel.try_receive(MAX_REPLY).map_err(|e| e.to_string())? {
                Some(reply) => reply,
                None => {
                    let send_error = channel.send(&command, Instant::now() + IO_TIMEOUT).err();
                    let reply = channel
                        .receive(Instant::now() + timeout.unwrap_or_default() + IO_TIMEOUT)
                        .map_err(|e| e.to_string())?;
                    if send_error.is_some() && reply["event"] != "closed" {
                        return Err("Guardian write failed without terminal evidence".into());
                    }
                    reply
                }
            };
            if reply["event"] == "closed" {
                self.terminal = Some(reply);
                return Err("Guardian closed during the request; call finish for evidence".into());
            }
            if reply["id"] != id || !reply["ok"].is_boolean() || !reply["data"].is_object() {
                return Err("Invalid guardian response".into());
            }
            Ok(reply)
        })();
        if result.is_err() && self.terminal.is_none() {
            self.channel.take();
        }
        result
    }
    pub fn finish(mut self) -> Result<Value, String> {
        let deadline = Instant::now() + Duration::from_secs(25);
        let terminal = match self.terminal.take() {
            Some(terminal) => terminal,
            None => {
                let channel = self
                    .channel
                    .as_mut()
                    .ok_or("Guardian IPC was lost; recovery runs without client evidence")?;
                match channel.try_receive(MAX_REPLY).map_err(|e| e.to_string())? {
                    Some(terminal) => terminal,
                    None => {
                        let _ = channel.send(
                            &json!({"id":self.next_id,"op":"stop"}),
                            Instant::now() + IO_TIMEOUT,
                        );
                        channel.receive(deadline).map_err(|e| e.to_string())?
                    }
                }
            }
        };
        if terminal["event"] != "closed"
            || terminal["protocol"] != 1
            || !terminal["data"]["completed"].is_boolean()
            || !terminal["data"]["recovery"]["completed"].is_boolean()
            || terminal["data"]["recovery"]["device"] != self.ready["data"]["device"]
            || terminal["data"]["recovery"]["registered"] != self.ready["registered"]
            || terminal["data"]["recovery"]["diversion_enabled_by_recovery"] != false
        {
            return Err("Invalid guardian terminal evidence".into());
        }
        self.channel.take();
        let child = self.child.as_mut().ok_or("Guardian child missing")?;
        loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                if !status.success() {
                    return Err("Guardian exited unsuccessfully after its terminal report".into());
                }
                self.child.take();
                return Ok(terminal["data"].clone());
            }
            if Instant::now() >= deadline {
                return Err("Guardian exit still pending; recovery will not be killed".into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[cfg(test)]
    pub(super) fn send_test_bytes(&mut self, bytes: &[u8]) {
        self.channel
            .as_mut()
            .unwrap()
            .stream
            .write_all(bytes)
            .unwrap();
    }
}
impl Drop for NativeGuardian {
    fn drop(&mut self) {
        self.channel.take();
        if let Some(mut child) = self.child.take() {
            // Reap without killing a process that may still be restoring hardware.
            let _ = std::thread::Builder::new()
                .name("native-guardian-reaper".into())
                .spawn(move || {
                    let _ = child.wait();
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_rejects_paths_primary_controls_duplicates_and_invalid_bounds() {
        for (interface, slot, id, controls, duration) in [
            (
                "/dev/hidraw0",
                1,
                "11223344",
                vec![83],
                Duration::from_secs(1),
            ),
            (
                "../hidraw0",
                1,
                "11223344",
                vec![83],
                Duration::from_secs(1),
            ),
            ("hidraw0", 0, "11223344", vec![83], Duration::from_secs(1)),
            ("hidraw0", 7, "11223344", vec![83], Duration::from_secs(1)),
            ("hidraw0", 1, "aabbccdd", vec![83], Duration::from_secs(1)),
            ("hidraw0", 1, "1122334Z", vec![83], Duration::from_secs(1)),
            ("hidraw0", 1, "11223344", vec![], Duration::from_secs(1)),
            ("hidraw0", 1, "11223344", vec![80], Duration::from_secs(1)),
            (
                "hidraw0",
                1,
                "11223344",
                vec![83, 83],
                Duration::from_secs(1),
            ),
            ("hidraw0", 1, "11223344", vec![83], Duration::ZERO),
            ("hidraw0", 1, "11223344", vec![83], Duration::MAX),
        ] {
            assert!(GuardianConfig::new(interface, slot, id, &controls, duration).is_err());
        }
    }
    #[test]
    fn channel_preserves_partial_and_coalesced_frames_and_detects_eof() {
        let (mut peer, local) = UnixStream::pair().unwrap();
        let mut channel = Channel::new(local).unwrap();
        peer.write_all(b"{\"id\":").unwrap();
        assert!(channel.try_receive(MAX_COMMAND).unwrap().is_none());
        peer.write_all(b"1}\n{\"id\":2}\n").unwrap();
        assert_eq!(
            channel.receive(Instant::now() + IO_TIMEOUT).unwrap(),
            json!({"id":1})
        );
        assert_eq!(
            channel.receive(Instant::now() + IO_TIMEOUT).unwrap(),
            json!({"id":2})
        );
        drop(peer);
        assert_eq!(
            // Concurrent process fixtures may briefly hold a forked CLOEXEC
            // peer until exec. Wait within the normal IPC budget for EOF.
            channel
                .receive(Instant::now() + IO_TIMEOUT)
                .unwrap_err()
                .kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
    #[test]
    fn channel_bounds_incomplete_commands_and_outgoing_frames() {
        let (mut peer, local) = UnixStream::pair().unwrap();
        let mut channel = Channel::new(local).unwrap();
        peer.write_all(&[b'x'; MAX_COMMAND + 1]).unwrap();
        assert!(channel
            .try_receive(MAX_COMMAND)
            .unwrap_err()
            .to_string()
            .contains("too large"));
        assert!(channel
            .send(
                &json!({"large":"x".repeat(MAX_REPLY)}),
                Instant::now() + IO_TIMEOUT
            )
            .unwrap_err()
            .to_string()
            .contains("too large"));
    }
    #[test]
    fn channel_partial_frame_wait_obeys_absolute_deadline() {
        let (mut peer, local) = UnixStream::pair().unwrap();
        let mut channel = Channel::new(local).unwrap();
        peer.write_all(b"{\"id\":").unwrap();
        assert_eq!(
            channel
                .receive(Instant::now() + Duration::from_millis(20))
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        peer.write_all(b"1}\n").unwrap();
        assert_eq!(
            channel.receive(Instant::now() + IO_TIMEOUT).unwrap(),
            json!({"id":1})
        );
    }
    #[test]
    fn a_client_that_does_not_read_cannot_block_the_guardian_write_forever() {
        let (_peer, local) = UnixStream::pair().unwrap();
        let size: libc::c_int = 4096;
        // SAFETY: sized integer buffer and a live socket descriptor.
        assert_eq!(
            unsafe {
                libc::setsockopt(
                    local.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_SNDBUF,
                    (&size as *const libc::c_int).cast(),
                    std::mem::size_of_val(&size) as libc::socklen_t,
                )
            },
            0
        );
        let mut channel = Channel::new(local).unwrap();
        assert_eq!(
            channel
                .send(
                    &json!({"large":"x".repeat(64 * 1024)}),
                    Instant::now() + Duration::from_millis(20)
                )
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
    }
}
