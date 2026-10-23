// SPDX-License-Identifier: GPL-3.0-or-later
//! N1: framing and one-owner request/response transport, without feature semantics.
mod codec;
#[cfg(target_os = "linux")]
mod hidraw;
mod session;

pub use codec::{CodecError, ErrorReply, Report, ReportKind};
#[cfg(target_os = "linux")]
pub use hidraw::Hidraw;
pub use session::{ReportConsumer, Session, SessionError, Transport, MAX_PENDING_REPORTS};
