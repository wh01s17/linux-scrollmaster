// SPDX-License-Identifier: GPL-3.0-or-later
//! Experimental synchronous JSON-lines worker; no unsolicited polls or fallback.
use crate::{
    native::{
        DesktopSnapshot, DesktopSource, HyprlandFocus, NativeProfileOwner, NoDesktop,
        ProfileGuardianOwner,
    },
    profiles::ProfileStore,
    service::{ProfileBackend, ProfileService},
};
use serde::{
    de::{self, MapAccess, Visitor},
    Deserialize, Deserializer,
};
use serde_json::{json, Map, Value};
use std::{
    fmt,
    io::{BufRead, Read, Write},
    time::{Duration, Instant},
};

pub const MAX_REQUEST_BYTES: usize = 256 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
pub const REQUEST_BUDGET: Duration = Duration::from_secs(30);
pub const DESKTOP_BUDGET: Duration = crate::native::FOCUS_BUDGET;

#[derive(Debug)]
pub struct WorkerConfig {
    slot: u8,
    identity: String,
    guardian: bool,
    capture: bool,
}
impl WorkerConfig {
    pub fn new(slot: u8, identity: &str) -> Result<Self, String> {
        // Pure constructor: validates selection without opening any resource.
        NativeProfileOwner::system(slot, identity)?;
        Ok(Self {
            slot,
            identity: identity.into(),
            guardian: false,
            capture: false,
        })
    }
    pub fn slot(&self) -> u8 {
        self.slot
    }
    pub fn identity(&self) -> &str {
        &self.identity
    }
    /// Explicit experimental process isolation. Does not enable software capture.
    pub fn with_guardian(mut self) -> Self {
        self.guardian = true;
        self
    }
    pub fn serve_flag(&self) -> &'static str {
        if self.capture {
            "--serve-guarded-capture"
        } else if self.guardian {
            "--serve-guarded"
        } else {
            "--serve"
        }
    }
    /// Explicit authorization for capture in the independent owner.
    pub fn with_capture(mut self) -> Self {
        self.guardian = true;
        self.capture = true;
        self
    }
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let usage = "Uso: scrollmaster-native-worker --serve|--serve-guarded|--serve-guarded-capture --slot 1..6 --device UNITID; experimental, capture puede interceptar botones/emitir acciones.";
        if args.len() != 5
            || !matches!(
                args[0].as_str(),
                "--serve" | "--serve-guarded" | "--serve-guarded-capture"
            )
        {
            return Err(usage.into());
        }
        let mut slot = None;
        let mut identity = None;
        for pair in args[1..].as_chunks::<2>().0 {
            match pair[0].as_str() {
                "--slot"
                    if slot.is_none()
                        && matches!(pair[1].as_str(), "1" | "2" | "3" | "4" | "5" | "6") =>
                {
                    slot = pair[1].parse::<u8>().ok();
                }
                "--device" if identity.is_none() => identity = Some(pair[1].as_str()),
                _ => return Err(usage.into()),
            }
        }
        let mut config = Self::new(slot.ok_or(usage)?, identity.ok_or(usage)?)?;
        config.guardian = args[0] != "--serve";
        config.capture = args[0] == "--serve-guarded-capture";
        Ok(config)
    }
}

/// Startup flags handled by `main_with_args`, shared by the dedicated binary
/// and the GUI executable so the guardian child can re-execute `current_exe`.
pub const ENTRY_FLAGS: [&str; 5] = [
    "--serve",
    "--serve-guarded",
    "--serve-guarded-capture",
    "--profile-guardian-child",
    "--profile-guardian-capture-child",
];
/// Worker or private guardian child entry point; `args` exclude argv[0].
pub fn main_with_args(mut args: Vec<String>) -> Result<(), String> {
    let child = args.first().is_some_and(|arg| {
        matches!(
            arg.as_str(),
            "--profile-guardian-child" | "--profile-guardian-capture-child"
        )
    });
    let capture = args
        .first()
        .is_some_and(|arg| arg == "--profile-guardian-capture-child");
    if child {
        args[0] = "--serve".into();
    }
    let config = WorkerConfig::parse(&args)?;
    if !child {
        return run(config, std::io::stdin().lock(), std::io::stdout().lock());
    }
    if capture {
        crate::native::run_profile_capture_child(config.slot(), config.identity())
    } else {
        crate::native::run_profile_guardian_child(config.slot(), config.identity())
    }
}
/// Configuration is supplied by the trusted launcher, never from an IPC request.
pub fn run(config: WorkerConfig, input: impl BufRead, output: impl Write) -> Result<(), String> {
    let store = ProfileStore::from_environment().map_err(|e| e.to_string())?;
    if config.guardian {
        let owner = if config.capture {
            ProfileGuardianOwner::system_capture(config.slot, &config.identity)?
        } else {
            ProfileGuardianOwner::system(config.slot, &config.identity)?
        };
        let service = ProfileService::new(store, owner, REQUEST_BUDGET)?;
        return serve_with_desktop(service, HyprlandFocus::environment(), input, output);
    }
    let owner = NativeProfileOwner::system(config.slot, &config.identity)?;
    let service = ProfileService::new(store, owner, REQUEST_BUDGET)?;
    serve_with_desktop(service, HyprlandFocus::environment(), input, output)
}

// Unlike Value's default object parser, reject repeated top-level keys so two
// spellings of op/device cannot silently select another operation or identity.
struct Request(Value);
impl<'de> Deserialize<'de> for Request {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Object;
        impl<'de> Visitor<'de> for Object {
            type Value = Request;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("request object")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Request, M::Error> {
                let mut fields = Map::new();
                while let Some((key, value)) = map.next_entry::<String, Value>()? {
                    if fields.insert(key, value).is_some() {
                        return Err(de::Error::custom("Repeated request field"));
                    }
                }
                Ok(Request(Value::Object(fields)))
            }
        }
        deserializer.deserialize_map(Object)
    }
}
fn decode(bytes: &[u8]) -> Result<Value, String> {
    let Request(request) =
        serde_json::from_slice(bytes).map_err(|e| format!("Invalid request JSON: {e}"))?;
    let op = request["op"].as_str().ok_or("Request operation required")?;
    let (required, optional): (&[&str], &[&str]) = match op {
        "snapshot" => (&["op"], &["device"]),
        "applications" => (&["op"], &["device"]),
        "shutdown" => (&["op"], &[]),
        "save" | "restore" | "delete" | "export" => (&["op", "device", "name"], &[]),
        "set" | "remap" => (&["op", "device", "changes"], &[]),
        "auto" => (&["op", "device"], &["name"]),
        "rename" => (&["op", "device", "name", "new_name"], &[]),
        "import" | "import_preview" => (&["op", "device", "document"], &[]),
        "rule_set" => (&["op", "device", "application", "name"], &[]),
        "rule_delete" => (&["op", "device", "application"], &[]),
        _ => return Err("Native worker operation not supported".into()),
    };
    let fields = request.as_object().unwrap();
    if required.iter().any(|k| !fields.contains_key(*k))
        || fields
            .keys()
            .any(|k| !required.contains(&k.as_str()) && !optional.contains(&k.as_str()))
    {
        return Err("Missing or unexpected request fields".into());
    }
    Ok(request)
}
fn error(stage: &str, message: impl ToString) -> Value {
    json!({"ok":false,"stage":stage,"error":message.to_string(),"experimental":true})
}
fn emit(output: &mut impl Write, response: &Value) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(response).map_err(|e| e.to_string())?;
    // Parent limit includes the newline. Preserve uncertainty if side effects
    // preceded a response that cannot fit; never claim that nothing happened.
    if bytes.len() >= MAX_RESPONSE_BYTES {
        bytes = serde_json::to_vec(&json!({"ok":false,"stage":"response","error":"Response exceeds worker limit; reread before deciding what to retry","outcome_uncertain":true,"experimental":true})).unwrap();
    }
    bytes.push(b'\n');
    output
        .write_all(&bytes)
        .and_then(|_| output.flush())
        .map_err(|e| e.to_string())
}

/// Own the service so every exit (including EOF/output failure) drops its reader
/// and lease. LF terminates a request; EOF never commits an unterminated frame.
/// Desktop-free embedding; production run uses serve_with_desktop instead.
pub fn serve<B: ProfileBackend>(
    service: ProfileService<B>,
    input: impl BufRead,
    output: impl Write,
) -> Result<(), String> {
    serve_with_desktop(service, NoDesktop, input, output)
}
/// Production/embedding entry point with a trusted, bounded desktop source.
pub fn serve_with_desktop<B: ProfileBackend, D: DesktopSource>(
    mut service: ProfileService<B>,
    mut desktop: D,
    mut input: impl BufRead,
    mut output: impl Write,
) -> Result<(), String> {
    loop {
        let mut bytes = Vec::new();
        let read = (&mut input)
            .take((MAX_REQUEST_BYTES + 2) as u64)
            .read_until(b'\n', &mut bytes);
        match read {
            Ok(0) => return Ok(()),
            Ok(_) if bytes.last() == Some(&b'\n') && bytes.len() <= MAX_REQUEST_BYTES + 1 => {}
            result => {
                let message = match result {
                    Err(e) => format!("Request read failed: {e}"),
                    _ => "Truncated or oversized request frame".into(),
                };
                drop(service);
                emit(&mut output, &error("framing", &message))?;
                return Err(message);
            }
        }
        let request = match decode(&bytes) {
            Ok(request) => request,
            Err(message) => {
                emit(&mut output, &error("validation", message))?;
                continue;
            }
        };
        if request["op"] == "shutdown" {
            let stopped = service.shutdown();
            drop(service);
            let response = match stopped {
                Ok(()) => {
                    json!({"ok":true,"data":{"stage":"N5","experimental":true,"stopped":true}})
                }
                Err(error) => error.describe(),
            };
            emit(&mut output, &response)?;
            return Ok(());
        }
        // Applications validates/loads profiles first, then queries the desktop;
        // snapshot needs observed focus before the service can choose a profile.
        let focus = (request["op"] == "snapshot").then(|| read_desktop(&mut desktop, false));
        let response = match service.handle(
            &request,
            focus.as_ref().and_then(DesktopSnapshot::active_application),
        ) {
            Ok(mut data) => {
                let focus = if request["op"] == "applications" {
                    Some(read_desktop(&mut desktop, true))
                } else {
                    focus
                };
                if let Some(focus) = focus {
                    data["desktop"] = focus.describe();
                }
                json!({"ok":true,"data":data})
            }
            Err(e) => e.describe(),
        };
        emit(&mut output, &response)?;
    }
}
fn read_desktop(desktop: &mut impl DesktopSource, include_apps: bool) -> DesktopSnapshot {
    let deadline = Instant::now() + DESKTOP_BUDGET;
    let snapshot = desktop.describe(include_apps, deadline);
    if Instant::now() >= deadline {
        DesktopSnapshot::unavailable("Desktop query budget elapsed")
    } else {
        snapshot
    }
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
