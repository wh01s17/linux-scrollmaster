// SPDX-License-Identifier: GPL-3.0-or-later
//! Shared process supervision and experimental native backend primitives.
//! The active GUI still launches the Python worker; native selection is explicit.
pub mod bridge;
pub mod hidpp;
#[cfg(target_os = "linux")]
pub mod native;
#[cfg(target_os = "linux")]
pub mod native_host;
pub mod presentation;
#[cfg(target_os = "linux")]
pub mod profiles;
#[cfg(target_os = "linux")]
pub mod service;
#[cfg(target_os = "linux")]
pub mod worker;
