//! Shared protocol contract consumer for the desktop application.
//!
//! This crate implements the canonical contract in `protocol/`: the signaling
//! message allowlist, the closed reason-code set and the version 1 QR payload.
//! The Kotlin implementation in `mobile/protocol/` is independent and is
//! exercised against the same vectors by `protocol/vectors/run_conformance.sh`.

pub mod conformance;
pub mod hex;
pub mod qr;
pub mod reason;
pub mod signaling;

mod address;
mod sdp;

pub use qr::{Endpoint, QrDecodeError, QrEncodeError, QrPayload};
pub use reason::{ReasonCategory, ReasonCode};
pub use signaling::{
    AnswerSdp, Candidate, Candidates, CloseCode, EncodeError, ErrorCode, ErrorText, IceComponent,
    InvalidValue, Limits, MAX_SIGNALING_MESSAGE_BYTES, OfferSdp, ParseError, SignalingMessage,
    Stats, ThermalStatus, TransportProtocol, parse_signaling,
};
