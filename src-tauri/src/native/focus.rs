// SPDX-License-Identifier: GPL-3.0-or-later
//! Read-only focus/catalogue. No dispatcher, Lua probe or shell command.
use super::desktop::{environment_path, exchange_until};
use crate::profiles::valid_application;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    time::{Duration, Instant},
};

pub const FOCUS_BUDGET: Duration = Duration::from_secs(1);
const MAX_REPLY: usize = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct DesktopSnapshot {
    active: Option<String>,
    applications: Option<Vec<String>>,
    error: Option<String>,
}
impl DesktopSnapshot {
    /// Validate the compositor's response shapes, retaining only exact class IDs.
    /// Invalid class values represent no usable identity, not invented matches.
    pub fn from_values(active: &Value, clients: Option<&Value>) -> Result<Self, String> {
        let active = active.as_object().ok_or("Invalid Hyprland focus reply")?;
        let active = active
            .get("class")
            .and_then(Value::as_str)
            .filter(|s| valid_application(s))
            .map(str::to_owned);
        let applications = clients
            .map(|value| {
                let clients = value
                    .as_array()
                    .ok_or("Invalid Hyprland application catalogue")?;
                let mut classes = BTreeSet::new();
                for client in clients {
                    let fields = client
                        .as_object()
                        .ok_or("Invalid Hyprland application catalogue")?;
                    if let Some(class) = fields
                        .get("class")
                        .and_then(Value::as_str)
                        .filter(|s| valid_application(s))
                    {
                        classes.insert(class.to_owned());
                    }
                }
                let mut classes: Vec<_> = classes.into_iter().collect();
                // Sorting affects presentation only; rule matching and deduplication
                // remain exact, case-sensitive and preserve Unicode class IDs.
                classes.sort_by_cached_key(|s| (s.to_lowercase(), s.clone()));
                Ok::<_, String>(classes)
            })
            .transpose()?;
        Ok(Self {
            active,
            applications,
            error: None,
        })
    }
    pub fn unavailable(message: impl ToString) -> Self {
        Self {
            active: None,
            applications: None,
            error: Some(message.to_string()),
        }
    }
    pub fn active_application(&self) -> Option<&str> {
        self.active.as_deref()
    }
    pub fn describe(&self) -> Value {
        let mut value = json!({"available":self.error.is_none(),"name":"Hyprland","active_application":self.active});
        if let Some(applications) = &self.applications {
            value["applications"] = json!(applications);
        }
        if let Some(error) = &self.error {
            value["error"] = json!(error);
        }
        value
    }
}

/// Trusted adapter only. No request may supply a compositor socket, command or
/// observed focus. Implementations must honor the single absolute deadline.
pub trait DesktopSource {
    fn describe(&mut self, include_apps: bool, deadline: Instant) -> DesktopSnapshot;
}
impl<T: DesktopSource + ?Sized> DesktopSource for Box<T> {
    fn describe(&mut self, include_apps: bool, deadline: Instant) -> DesktopSnapshot {
        (**self).describe(include_apps, deadline)
    }
}
/// Explicit offline adapter for embedding/testing without desktop I/O.
pub struct NoDesktop;
impl DesktopSource for NoDesktop {
    fn describe(&mut self, _: bool, _: Instant) -> DesktopSnapshot {
        DesktopSnapshot::unavailable("Desktop queries are disabled for this service")
    }
}
pub struct HyprlandFocus<R = fn() -> Result<PathBuf, String>> {
    resolve: R,
    budget: Duration,
}
impl HyprlandFocus {
    /// Pure construction. Resolve the current session on each actual query.
    pub fn environment() -> Self {
        Self {
            resolve: environment_path,
            budget: FOCUS_BUDGET,
        }
    }
}
impl<R: FnMut() -> Result<PathBuf, String>> HyprlandFocus<R> {
    fn read(&mut self, include_apps: bool, deadline: Instant) -> Result<DesktopSnapshot, String> {
        if Instant::now() >= deadline {
            return Err("Desktop query budget elapsed".into());
        }
        let path = (self.resolve)()?;
        let active = exchange_until(&path, b"j/activewindow", MAX_REPLY, deadline)?;
        let active: Value =
            serde_json::from_slice(&active).map_err(|_| "Invalid Hyprland focus JSON")?;
        // Reject a malformed active response before opening another connection.
        DesktopSnapshot::from_values(&active, None)?;
        let clients = if include_apps {
            let bytes = exchange_until(&path, b"j/clients", MAX_REPLY, deadline)?;
            Some(
                serde_json::from_slice::<Value>(&bytes)
                    .map_err(|_| "Invalid Hyprland clients JSON")?,
            )
        } else {
            None
        };
        let result = DesktopSnapshot::from_values(&active, clients.as_ref())?;
        if Instant::now() >= deadline {
            return Err("Desktop query budget elapsed".into());
        }
        Ok(result)
    }
}
impl<R: FnMut() -> Result<PathBuf, String>> DesktopSource for HyprlandFocus<R> {
    fn describe(&mut self, include_apps: bool, deadline: Instant) -> DesktopSnapshot {
        let deadline = deadline.min(Instant::now() + self.budget);
        self.read(include_apps, deadline)
            .unwrap_or_else(DesktopSnapshot::unavailable)
    }
}

#[cfg(test)]
#[path = "focus_tests.rs"]
mod tests;
