// SPDX-License-Identifier: GPL-3.0-or-later
// Wire references and synthetic vector provenance: docs/native-transport.md.
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportKind {
    Short,
    Long,
}
impl ReportKind {
    pub fn wire_len(self) -> usize {
        match self {
            Self::Short => 7,
            Self::Long => 20,
        }
    }
    fn id(self) -> u8 {
        match self {
            Self::Short => 0x10,
            Self::Long => 0x11,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    UnsupportedReport(Option<u8>),
    Length { expected: usize, actual: usize },
    InvalidRequest,
    PayloadTooLong,
}
impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Invalid HID++ report: {self:?}")
    }
}
impl std::error::Error for CodecError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorReply {
    /// 0x8f (HID++ 1.0) or 0xff (HID++ 2.0). Preserve unknown error codes.
    pub marker: u8,
    pub feature: u8,
    pub function_and_software: u8,
    pub code: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report(Vec<u8>);
impl Report {
    pub fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        let expected = match bytes.first() {
            Some(0x10) => 7,
            Some(0x11) => 20,
            id => return Err(CodecError::UnsupportedReport(id.copied())),
        };
        if bytes.len() != expected {
            return Err(CodecError::Length {
                expected,
                actual: bytes.len(),
            });
        }
        Ok(Self(bytes.to_vec()))
    }

    pub fn request(
        kind: ReportKind,
        device: u8,
        feature: u8,
        function: u8,
        software: u8,
        payload: &[u8],
    ) -> Result<Self, CodecError> {
        // Error markers cannot be generated as feature requests.
        if device == 0
            || matches!(feature, 0x8f | 0xff)
            || function > 15
            || !(1..=15).contains(&software)
        {
            return Err(CodecError::InvalidRequest);
        }
        if payload.len() > kind.wire_len() - 4 {
            return Err(CodecError::PayloadTooLong);
        }
        let mut bytes = vec![0; kind.wire_len()];
        bytes[..4].copy_from_slice(&[kind.id(), device, feature, (function << 4) | software]);
        bytes[4..4 + payload.len()].copy_from_slice(payload);
        Ok(Self(bytes))
    }

    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
    pub fn device(&self) -> u8 {
        self.0[1]
    }
    pub fn feature(&self) -> u8 {
        self.0[2]
    }
    pub fn function_and_software(&self) -> u8 {
        self.0[3]
    }
    pub fn payload(&self) -> &[u8] {
        &self.0[4..]
    }

    pub fn error(&self) -> Option<ErrorReply> {
        match self.feature() {
            marker @ (0x8f | 0xff) => Some(ErrorReply {
                marker,
                feature: self.0[3],
                function_and_software: self.0[4],
                code: self.0[5],
            }),
            _ => None,
        }
    }

    /// Report size may differ between request and response.
    pub fn answers(&self, request: &Self) -> bool {
        if self.device() != request.device() {
            return false;
        }
        let (feature, function) = match self.error() {
            Some(error) => (error.feature, error.function_and_software),
            None => (self.feature(), self.function_and_software()),
        };
        feature == request.feature() && function == request.function_and_software()
    }
}
