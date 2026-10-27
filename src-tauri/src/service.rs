// SPDX-License-Identifier: GPL-3.0-or-later
//! N5 synchronous single-device profile service. No worker spawning or timers.
use crate::{
    native::ApplyError,
    profiles::{Document, ProfileStore, Revision, StoreError},
};
use serde_json::{json, Value};
use std::{
    fmt,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct Changes {
    settings: Option<Value>,
    buttons: Option<Value>,
}
impl Changes {
    pub fn new(settings: Option<Value>, buttons: Option<Value>) -> Result<Self, String> {
        if settings.is_none() && buttons.is_none() {
            return Err("No profile changes requested".into());
        }
        let mut profile = json!({"device":"validation","name":"validation","settings":settings.as_ref().cloned().unwrap_or_else(|| json!({"dpi":1}))});
        if let Some(buttons) = &buttons {
            profile["buttons"] = buttons.clone();
        }
        Document::from_value(json!({"version":10,"profiles":[profile],"active":{}}))?;
        Ok(Self { settings, buttons })
    }
    pub fn settings(&self) -> Option<&Value> {
        self.settings.as_ref()
    }
    pub fn buttons(&self) -> Option<&Value> {
        self.buttons.as_ref()
    }
    pub fn confirm(&self, snapshot: &Value, identity: &str) -> Result<(), String> {
        if snapshot["id"] != identity {
            return Err("Applied snapshot has a different device identity".into());
        }
        if let Some(settings) = &self.settings {
            for (key, value) in settings.as_object().unwrap() {
                let current = &snapshot["settings"][key];
                if current["available"] != true || current["value"] != *value {
                    return Err(format!("Final profile setting unconfirmed: {key}"));
                }
            }
        }
        if let Some(buttons) = &self.buttons {
            if snapshot["buttons"]
                .get("error")
                .is_some_and(|error| !error.is_null())
            {
                return Err("Final button catalogue has an error".into());
            }
            let controls = snapshot["buttons"]["controls"]
                .as_array()
                .ok_or("Missing final button catalogue")?;
            for (cid, value) in buttons.as_object().unwrap() {
                let found: Vec<_> = controls.iter().filter(|c| c["id"] == *cid).collect();
                if found.len() != 1 || found[0]["available"] != true || found[0]["value"] != *value
                {
                    return Err(format!("Final profile button unconfirmed: {cid}"));
                }
            }
        }
        Ok(())
    }
}
#[derive(Debug)]
pub struct Applied {
    pub confirmed: Vec<String>,
    pub snapshot: Value,
}

/// A backend owns one device/session. Preparation is read-only and must check
/// the WHOLE change set before apply. Apply must read back, never blindly retry.
/// The service independently checks the final requested values in its receipt.
pub trait ProfileBackend {
    /// Pure admission policy, checked before acquisition by process/owner layers.
    fn validate_changes(&self, changes: &Changes) -> Result<(), String> {
        crate::native::validate_profile_native_changes(changes)
    }
    fn identity(&self) -> &str;
    fn snapshot(&mut self, deadline: Instant) -> Result<Value, String>;
    fn prepare(&mut self, changes: &Changes, deadline: Instant) -> Result<(), String>;
    fn apply(&mut self, changes: &Changes, deadline: Instant) -> Result<Applied, ApplyError>;
    /// Passive idle delivery on the same reader, without acquiring hardware or
    /// invalidating a valid preparation. On stream error, release the session.
    fn poll(&mut self, _timeout: Duration) -> Result<bool, String> {
        Ok(false)
    }
    /// Confirm explicit shutdown. Drop remains the fail-safe for EOF/panic; a
    /// process owner must not acknowledge shutdown while its child holds HID.
    fn close(&mut self, _deadline: Instant) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug)]
pub struct ServiceError {
    pub stage: &'static str,
    pub message: String,
    /// Historical confirmations, not a claim of rollback or current state.
    pub confirmed: Vec<String>,
    pub attempted: Option<String>,
    pub persistence_uncertain: bool,
}
impl ServiceError {
    fn new(stage: &'static str, message: impl ToString) -> Self {
        Self {
            stage,
            message: message.to_string(),
            confirmed: vec![],
            attempted: None,
            persistence_uncertain: false,
        }
    }
    fn storage(error: StoreError) -> Self {
        let uncertain = error.may_have_published();
        Self {
            persistence_uncertain: uncertain,
            ..Self::new("persistence", error)
        }
    }
    fn apply(error: ApplyError) -> Self {
        Self {
            stage: "apply",
            message: error.message,
            confirmed: error.confirmed,
            attempted: error.attempted,
            persistence_uncertain: false,
        }
    }
    pub fn describe(&self) -> Value {
        json!({"ok":false,"stage":self.stage,"error":self.message,"confirmed":self.confirmed,"attempted":self.attempted,"persistence_uncertain":self.persistence_uncertain})
    }
}
impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {}; confirmed: {:?}; attempted: {:?}",
            self.stage, self.message, self.confirmed, self.attempted
        )
    }
}
impl std::error::Error for ServiceError {}

/// Explicitly constructed experimental service; never automatically attached to
/// Tauri/Python. Use NativeProfileOwner or retain the device lease externally;
/// integration must exclude other profile writers for the service lifetime.
pub struct ProfileService<B: ProfileBackend> {
    store: ProfileStore,
    backend: B,
    budget: Duration,
}
impl<B: ProfileBackend> ProfileService<B> {
    pub fn shutdown(&mut self) -> Result<(), ServiceError> {
        self.backend
            .close(Instant::now() + self.budget)
            .map_err(|e| ServiceError::new("shutdown", e))
    }
    pub fn new(store: ProfileStore, backend: B, budget: Duration) -> Result<Self, String> {
        if budget.is_zero()
            || budget > Duration::from_secs(30)
            || backend.identity().is_empty()
            || backend.identity().chars().count() > 128
        {
            return Err("Invalid service identity or request budget".into());
        }
        Ok(Self {
            store,
            backend,
            budget,
        })
    }
    fn check_time(deadline: Instant) -> Result<(), ServiceError> {
        if Instant::now() >= deadline {
            Err(ServiceError::new(
                "deadline",
                "Profile request budget elapsed",
            ))
        } else {
            Ok(())
        }
    }
    fn read(&mut self, id: &str, deadline: Instant) -> Result<Value, ServiceError> {
        Self::check_time(deadline)?;
        let snapshot = self
            .backend
            .snapshot(deadline)
            .map_err(|e| ServiceError::new("snapshot", e))?;
        Self::check_time(deadline)?;
        if snapshot["id"] != id || !snapshot["settings"].is_object() {
            return Err(ServiceError::new(
                "snapshot",
                "Current snapshot identity/settings do not match the selected device",
            ));
        }
        Ok(snapshot)
    }
    fn prepare(&mut self, changes: &Changes, deadline: Instant) -> Result<(), ServiceError> {
        Self::check_time(deadline)?;
        self.backend
            .prepare(changes, deadline)
            .map_err(|e| ServiceError::new("preflight", e))?;
        Self::check_time(deadline)
    }
    fn apply(
        &mut self,
        changes: &Changes,
        id: &str,
        deadline: Instant,
    ) -> Result<Applied, ServiceError> {
        Self::check_time(deadline)?;
        let applied = self
            .backend
            .apply(changes, deadline)
            .map_err(ServiceError::apply)?;
        let confirmation = Self::check_time(deadline)
            .map_err(|e| e.message)
            .and_then(|()| changes.confirm(&applied.snapshot, id));
        if let Err(message) = confirmation {
            return Err(ServiceError::apply(ApplyError {
                message,
                confirmed: applied.confirmed,
                attempted: None,
            }));
        }
        Ok(applied)
    }
    fn disarm(&self, revision: &Revision, id: &str) -> Result<Revision, ServiceError> {
        self.store
            .ensure_current(revision)
            .map_err(ServiceError::storage)?;
        if revision.document().as_value()["active"].get(id).is_none() {
            return Ok(revision.clone());
        }
        let document = revision
            .document()
            .manage(&json!({"op":"auto","device":id,"name":null}))
            .map_err(|e| ServiceError::new("validation", e))?
            .document;
        self.store
            .commit(revision, &document)
            .map_err(ServiceError::storage)
    }
    fn response(
        document: &Document,
        devices: Vec<Value>,
        warnings: Vec<String>,
        reconciliation: Vec<Value>,
        failures: Vec<Value>,
        transfer: Option<Value>,
    ) -> Value {
        json!({"experimental":true,"stage":"N5","devices":devices,"profiles":document.profiles(),"profile_version":document.version(),"active":document.as_value()["active"],"rules":document.as_value().get("rules").cloned().unwrap_or_else(||json!([])),"warnings":warnings,"reconciliation":reconciliation,"reconciliation_errors":failures,"transfer":transfer})
    }
    pub fn handle(
        &mut self,
        request: &Value,
        application: Option<&str>,
    ) -> Result<Value, ServiceError> {
        let deadline = Instant::now() + self.budget;
        let op = request
            .as_object()
            .and_then(|r| r.get("op"))
            .and_then(Value::as_str)
            .ok_or_else(|| ServiceError::new("validation", "Invalid profile service request"))?;
        let local = [
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
        ]
        .contains(&op);
        if !local && !["snapshot", "save", "set", "remap", "restore"].contains(&op) {
            return Err(ServiceError::new(
                "validation",
                "Profile service operation not supported",
            ));
        }
        let mut revision = self.store.load().map_err(ServiceError::storage)?;
        let autosave = request.get("autosave").map_or(Ok(false), |v| {
            v.as_bool()
                .ok_or_else(|| ServiceError::new("validation", "Invalid autosave flag"))
        })?;
        if op == "applications" {
            // The worker adds a read-only desktop catalogue. Do not acquire HID,
            // reconcile an armed profile or rewrite the store for this request.
            return Ok(Self::response(
                revision.document(),
                vec![],
                vec![],
                vec![],
                vec![],
                None,
            ));
        }
        if local {
            let result = revision
                .document()
                .manage(request)
                .map_err(|e| ServiceError::new("validation", e))?;
            if result.changed {
                self.store
                    .commit(&revision, &result.document)
                    .map_err(ServiceError::storage)?;
            }
            return Ok(Self::response(
                &result.document,
                vec![],
                vec![],
                vec![],
                vec![],
                result.transfer,
            ));
        }
        let id = self.backend.identity().to_owned();
        if (op != "snapshot" || request.get("device").is_some()) && request["device"] != id {
            return Err(ServiceError::new(
                "validation",
                "Select the identity owned by this service",
            ));
        }
        if op == "snapshot" {
            let resume = request.get("resume").map_or(Ok(false), |v| {
                v.as_bool()
                    .ok_or_else(|| ServiceError::new("validation", "Invalid resume flag"))
            })?;
            if resume && revision.document().as_value()["active"].get(&id).is_none() {
                let name = revision
                    .document()
                    .profiles()
                    .iter()
                    .find(|p| p["device"] == id && p["name"] == crate::profiles::CURRENT_PROFILE)
                    .or_else(|| {
                        revision
                            .document()
                            .profiles()
                            .iter()
                            .rfind(|p| p["device"] == id)
                    })
                    .map(|p| p["name"].clone());
                if let Some(name) = name {
                    let document = revision
                        .document()
                        .manage(&json!({"op":"auto","device":id,"name":name}))
                        .map_err(|e| ServiceError::new("validation", e))?
                        .document;
                    revision = self
                        .store
                        .commit(&revision, &document)
                        .map_err(ServiceError::storage)?;
                }
            }
            let mut warnings = Vec::new();
            let mut reconciliation = Vec::new();
            let mut failures = Vec::new();
            let mut device = match self.read(&id, deadline) {
                Ok(device) => Some(device),
                Err(error) => {
                    warnings.push(error.to_string());
                    failures.push(error.describe());
                    None
                }
            };
            if device.is_some() {
                if let Some(profile) = revision.document().effective_profile(&id, application) {
                    let name = profile["name"].clone();
                    let changes = Changes::new(
                        Some(profile["settings"].clone()),
                        profile.get("buttons").cloned(),
                    )
                    .map_err(|e| ServiceError::new("validation", e))?;
                    // The fresh snapshot is already a confirmation. Avoid a
                    // second full preflight/readback when nothing has drifted.
                    if changes.confirm(device.as_ref().unwrap(), &id).is_ok() {
                        reconciliation.push(json!({"device":id,"profile":name,"applied":[]}));
                        return Ok(Self::response(
                            revision.document(),
                            device.into_iter().collect(),
                            warnings,
                            reconciliation,
                            failures,
                            None,
                        ));
                    }
                    let prepared = self.prepare(&changes, deadline).and_then(|()| {
                        self.store
                            .ensure_current(&revision)
                            .map_err(ServiceError::storage)
                    });
                    match prepared {
                        Err(error) => {
                            warnings.push(error.to_string());
                            failures.push(error.describe());
                        }
                        Ok(()) => {
                            match self.apply(&changes, &id, deadline) {
                                Ok(applied) => {
                                    device = Some(applied.snapshot);
                                    reconciliation.push(json!({"device":id,"profile":name,"applied":applied.confirmed}));
                                }
                                Err(error) => {
                                    device = None;
                                    warnings.push(error.to_string());
                                    failures.push(error.describe());
                                }
                            }
                        }
                    }
                }
            }
            return Ok(Self::response(
                revision.document(),
                device.into_iter().collect(),
                warnings,
                reconciliation,
                failures,
                None,
            ));
        }
        if op == "save" {
            let snapshot = self.read(&id, deadline)?;
            let mut document = revision
                .document()
                .save_snapshot(&id, &request["name"], &snapshot)
                .map_err(|e| ServiceError::new("validation", e))?;
            if autosave {
                document = document
                    .save_snapshot(&id, &json!(crate::profiles::CURRENT_PROFILE), &snapshot)
                    .map_err(|e| ServiceError::new("validation", e))?
                    .manage(
                        &json!({"op":"auto","device":id,"name":crate::profiles::CURRENT_PROFILE}),
                    )
                    .map_err(|e| ServiceError::new("validation", e))?
                    .document;
            }
            Self::check_time(deadline)?;
            self.store
                .commit(&revision, &document)
                .map_err(ServiceError::storage)?;
            return Ok(Self::response(
                &document,
                vec![snapshot],
                vec![],
                vec![],
                vec![],
                None,
            ));
        }
        let changes = match op {
            "set" => Changes::new(Some(request["changes"].clone()), None),
            "remap" => Changes::new(None, Some(request["changes"].clone())),
            "restore" => {
                let profile = revision
                    .document()
                    .profiles()
                    .iter()
                    .find(|p| p["device"] == id && p["name"] == request["name"])
                    .ok_or_else(|| {
                        ServiceError::new("validation", "Profile not found for the selected device")
                    })?;
                Changes::new(
                    Some(profile["settings"].clone()),
                    profile.get("buttons").cloned(),
                )
            }
            _ => unreachable!(),
        }
        .map_err(|e| ServiceError::new("validation", e))?;
        self.prepare(&changes, deadline)?;
        revision = self.disarm(&revision, &id)?;
        Self::check_time(deadline)?;
        self.store
            .ensure_current(&revision)
            .map_err(ServiceError::storage)?;
        let applied = self.apply(&changes, &id, deadline)?;
        if autosave {
            let saved = (|| {
                let document = revision
                    .document()
                    .save_snapshot(
                        &id,
                        &json!(crate::profiles::CURRENT_PROFILE),
                        &applied.snapshot,
                    )
                    .map_err(|e| ServiceError::new("autosave", e))?;
                let document = document
                    .manage(
                        &json!({"op":"auto","device":id,"name":crate::profiles::CURRENT_PROFILE}),
                    )
                    .map_err(|e| ServiceError::new("autosave", e))?
                    .document;
                Self::check_time(deadline)?;
                self.store
                    .commit(&revision, &document)
                    .map_err(ServiceError::storage)
            })();
            revision = saved.map_err(|mut error| {
                error.message = format!(
                    "El cambio se aplicó, pero no se pudo guardar el perfil actual: {}",
                    error.message
                );
                error.confirmed = applied.confirmed.clone();
                error
            })?;
        }
        Ok(Self::response(
            revision.document(),
            vec![applied.snapshot],
            vec![],
            vec![],
            vec![],
            None,
        ))
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
