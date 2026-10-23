// SPDX-License-Identifier: GPL-3.0-or-later
//! N5 local profile core. No device/compositor I/O, reconciliation or activation.
mod model;
mod operations;
mod store;

pub(crate) use model::button_schema;
pub(crate) use model::valid_application;
pub use model::{Document, MAX_DOCUMENT_BYTES, MAX_PROFILES};
pub use operations::Management;
pub use store::{ProfileStore, Revision, StoreError};

#[cfg(test)]
mod tests;
