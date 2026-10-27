// SPDX-License-Identifier: GPL-3.0-or-later
//! Tauri-side selection of the native worker: locate, launch, map replies to
//! the Vue contract and keep local profile management available offline.
//! Opt-in (`SCROLLMASTER_BACKEND=native` or the `native-default` build); it
//! never falls back to Solaar.
use crate::{
    bridge::Worker,
    native::{ApplyError, Located},
    presentation,
    profiles::ProfileStore,
    service::{Applied, Changes, ProfileBackend, ProfileService},
    worker::{serve_with_desktop, WorkerConfig},
};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

pub const BACKEND_ENV: &str = "SCROLLMASTER_BACKEND";
const LOCATE_TIMEOUT: Duration = Duration::from_secs(4);
const LOCATE_RETRY: Duration = Duration::from_secs(10);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(25);
const LOCAL: [&str; 10] = [
    "save_copy",
    "auto",
    "rename",
    "delete",
    "export",
    "import_preview",
    "import",
    "rule_set",
    "rule_delete",
    "applications",
];

/// Read once by the GUI. `native`/`solaar` choose explicitly; otherwise the
/// build default applies (legacy unless built with `native-default`).
pub fn native_selected() -> bool {
    selection(std::env::var(BACKEND_ENV).ok().as_deref())
}
fn selection(value: Option<&str>) -> bool {
    match value {
        Some("native") => true,
        Some("solaar") => false,
        _ => cfg!(feature = "native-default"),
    }
}

type Locate = dyn FnMut() -> Result<Located, String> + Send;
type Launch = dyn FnMut(&Located) -> Result<Worker, String> + Send;
type Desktop = dyn FnMut() -> Value + Send;
type Store = dyn Fn() -> Result<ProfileStore, String> + Send;
type Focus = dyn Fn() -> Box<dyn crate::native::DesktopSource> + Send;

pub struct NativeHost {
    worker: Option<Worker>,
    locate: Box<Locate>,
    launch: Box<Launch>,
    desktop: Box<Desktop>,
    store: Box<Store>,
    focus: Box<Focus>,
    last_failure: Option<(Instant, String)>,
    last_devices: Option<Value>,
    resume_pending: bool,
}
impl NativeHost {
    /// No HID, process or file access until the first request.
    pub fn system() -> Result<Self, String> {
        let executable: PathBuf = std::env::current_exe().map_err(|e| e.to_string())?;
        Ok(Self::new(
            || crate::native::locate(LOCATE_TIMEOUT),
            move |located| {
                let config = WorkerConfig::new(located.slot, &located.identity)?.with_capture();
                Worker::spawn_native(&executable, &config)
            },
            crate::native::desktop_actions_status,
            || ProfileStore::from_environment().map_err(|e| e.to_string()),
        )
        .with_focus(|| Box::new(crate::native::HyprlandFocus::environment())))
    }
    /// Read-only focus/catalogue source for offline `applications` (default: none).
    pub fn with_focus(
        mut self,
        focus: impl Fn() -> Box<dyn crate::native::DesktopSource> + Send + 'static,
    ) -> Self {
        self.focus = Box::new(focus);
        self
    }
    pub fn new(
        locate: impl FnMut() -> Result<Located, String> + Send + 'static,
        launch: impl FnMut(&Located) -> Result<Worker, String> + Send + 'static,
        desktop: impl FnMut() -> Value + Send + 'static,
        store: impl Fn() -> Result<ProfileStore, String> + Send + 'static,
    ) -> Self {
        Self {
            worker: None,
            locate: Box::new(locate),
            launch: Box::new(launch),
            desktop: Box::new(desktop),
            store: Box::new(store),
            focus: Box::new(|| Box::new(crate::native::NoDesktop)),
            last_failure: None,
            last_devices: None,
            resume_pending: false,
        }
    }
    /// One serialized request. A transport failure drops (kills) the worker;
    /// its independent guardian restores HID. The request is never repeated.
    pub fn request(&mut self, payload: &Value, timeout: Duration) -> Result<Value, String> {
        let op = payload["op"].as_str().unwrap_or_default().to_owned();
        if op == "snapshot" && payload["resume"] == true {
            self.resume_pending = true;
        }
        if self.worker.is_none() {
            if let Err(error) = self.start() {
                return self.offline(payload, &op, error);
            }
        }
        let mut payload = payload.clone();
        if op == "snapshot" && self.resume_pending {
            payload["resume"] = json!(true);
        }
        let reply = self.worker.as_mut().unwrap().transact(&payload, timeout);
        let mut reply = match reply {
            Ok(reply) => reply,
            Err(error) => {
                self.worker = None;
                self.last_devices = None;
                return Err(error);
            }
        };
        if reply["ok"] == true {
            if op == "snapshot" {
                self.resume_pending = false;
            }
            let data = &mut reply["data"];
            if ["snapshot", "set", "remap", "restore", "save"].contains(&op.as_str()) {
                self.last_devices = Some(data["devices"].clone());
            } else if LOCAL.contains(&op.as_str()) {
                // Local operations do not read HID. Show the latest real reading
                // instead of an empty device list until the next poll.
                data["devices"] = self.last_devices.clone().unwrap_or_else(|| json!([]));
            }
            presentation::snapshot(data, &(self.desktop)(), NOT_FOUND);
        } else if !LOCAL.contains(&op.as_str()) {
            self.last_devices = None;
        }
        Ok(reply)
    }
    fn start(&mut self) -> Result<(), String> {
        if let Some((at, error)) = &self.last_failure {
            if at.elapsed() < LOCATE_RETRY {
                return Err(error.clone());
            }
        }
        let result = (self.locate)().and_then(|located| (self.launch)(&located));
        match result {
            Ok(worker) => {
                self.worker = Some(worker);
                self.last_failure = None;
                Ok(())
            }
            Err(error) => {
                self.last_failure = Some((Instant::now(), error.clone()));
                Err(error)
            }
        }
    }
    /// Without a located mouse: local profile operations via the same service
    /// and validation, and a read-only snapshot of stored profiles.
    fn offline(&mut self, payload: &Value, op: &str, error: String) -> Result<Value, String> {
        self.last_devices = None;
        let store = (self.store)()?;
        if !LOCAL.contains(&op) {
            if op != "snapshot" {
                return Ok(json!({"ok":false,"error":error}));
            }
            let revision = store.load().map_err(|e| e.to_string())?;
            let document = revision.document();
            let mut data = json!({"devices":[],"profiles":document.profiles(),
                "profile_version":document.version(),"active":document.as_value()["active"],
                "rules":document.as_value().get("rules").cloned().unwrap_or_else(|| json!([])),
                "warnings":[],"reconciliation":[]});
            presentation::snapshot(&mut data, &json!({"available":false}), &error);
            return Ok(json!({"ok":true,"data":data}));
        }
        let identity = match payload["device"].as_str() {
            Some(identity) => identity.to_owned(),
            None => return Ok(json!({"ok":false,"error":error})),
        };
        let service = ProfileService::new(
            store,
            Offline {
                identity,
                error: error.clone(),
            },
            Duration::from_secs(5),
        )?;
        let mut input = serde_json::to_vec(payload).map_err(|e| e.to_string())?;
        input.push(b'\n');
        let mut output = Vec::new();
        serve_with_desktop(service, (self.focus)(), input.as_slice(), &mut output)?;
        let mut reply: Value = serde_json::from_slice(&output).map_err(|e| e.to_string())?;
        if reply["ok"] == true {
            presentation::snapshot(&mut reply["data"], &json!({"available":false}), &error);
        }
        Ok(reply)
    }
    /// Confirmed shutdown so the guardian restores HID before the GUI exits.
    pub fn shutdown(&mut self) -> Result<(), String> {
        let Some(mut worker) = self.worker.take() else {
            return Ok(());
        };
        let reply = worker.transact(&json!({"op":"shutdown"}), SHUTDOWN_TIMEOUT)?;
        if reply["ok"] == true && reply["data"]["stopped"] == true {
            Ok(())
        } else {
            Err(reply["error"]
                .as_str()
                .unwrap_or("Native worker shutdown unconfirmed")
                .to_owned())
        }
    }
}
impl Drop for NativeHost {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
const NOT_FOUND: &str =
    "No se encontró un MX Master 3S activo. Enciéndelo, muévelo o comprueba el receptor Logi Bolt.";

/// Profile management without hardware. Any device request reports `error`.
struct Offline {
    identity: String,
    error: String,
}
impl ProfileBackend for Offline {
    fn validate_changes(&self, _: &Changes) -> Result<(), String> {
        Ok(())
    }
    fn identity(&self) -> &str {
        &self.identity
    }
    fn snapshot(&mut self, _: Instant) -> Result<Value, String> {
        Err(self.error.clone())
    }
    fn prepare(&mut self, _: &Changes, _: Instant) -> Result<(), String> {
        Err(self.error.clone())
    }
    fn apply(&mut self, _: &Changes, _: Instant) -> Result<Applied, ApplyError> {
        Err(ApplyError {
            message: self.error.clone(),
            confirmed: vec![],
            attempted: None,
        })
    }
}

#[cfg(test)]
#[path = "native_host_tests.rs"]
mod tests;
