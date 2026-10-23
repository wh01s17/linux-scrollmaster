// SPDX-License-Identifier: GPL-3.0-or-later
//! Experimental native tools and profile backends, separate from the active worker.
mod access;
mod actions;
mod activation;
mod buttons;
mod descriptor;
mod desktop;
mod diagnostic;
mod discovery;
mod events;
mod focus;
mod guardian;
mod guardian_config;
mod keyboard;
mod locate;
mod owner;
mod pairing;
mod profile_backend;
mod profile_buttons;
mod profile_capture;
mod profile_connector;
mod profile_events;
mod profile_guardian;
mod profile_guardian_server;
mod profile_guardian_wire;
mod profile_owner;
mod recovery;
mod runner;
mod settings;

pub use access::{ensure_no_controllers, HardwareLease};
pub use actions::{ActionBindings, GestureDirection, GestureFeedback, NativeActions};
pub use buttons::{apply_button_changes, read_buttons, validate_button_changes};
pub use desktop::{desktop_actions_status, DesktopAction};
pub use diagnostic::diagnose;
pub use discovery::{discover, Candidate, Discovery};
pub use events::observe_notifications;
pub use focus::{DesktopSnapshot, DesktopSource, HyprlandFocus, NoDesktop, FOCUS_BUDGET};
pub use guardian::{run_guardian_child, NativeGuardian};
pub use guardian_config::GuardianConfig;
pub use keyboard::{HeldModifiers, KeyboardAction, WaylandKeyboard};
pub use locate::{locate, Located};
pub use owner::PassiveEventOwner;
pub use pairing::paired_devices;
pub(crate) use profile_backend::validate_native_changes as validate_profile_native_changes;
pub use profile_backend::NativeProfileBackend;
pub use profile_buttons::ProfileButtonPlan;
pub use profile_connector::{ProfileConnector, SystemProfileConnector};
pub use profile_guardian::{
    run_profile_capture_child, run_profile_guardian_child, ProfileGuardianOwner,
};
pub use profile_owner::NativeProfileOwner;
pub use recovery::ReportingRecovery;
pub use runner::{run_bounded, run_bounded_settings};
pub use settings::{apply_changes, validate_changes, ApplyError};

#[cfg(test)]
mod buttons_tests;
#[cfg(test)]
mod events_tests;
#[cfg(test)]
mod tests;
