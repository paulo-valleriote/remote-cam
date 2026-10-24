//! Virtual audio bridge: device readiness, degraded operation and WASAPI
//! rendering into a third-party virtual cable.
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike (FR-039). Nothing here merges
//! into a product module.
//!
//! Windows 11 provides no OS-level virtual microphone, so V1 depends on a
//! user-installed driver (research decision D-12). This crate does **not** ship
//! or redistribute that driver. It detects the pinned device by exact product
//! identity, verifies its signature status, confirms an audible 1 kHz test tone
//! before a session is considered audio-ready (FR-031), and keeps video-only
//! operation usable when any of those checks fail (FR-032).
//!
//! The split follows the crate's testability boundary:
//!
//! * [`readiness`] and [`degraded`] are platform neutral and take a device
//!   inventory and a tone sink through traits, so Linux CI exercises every
//!   branch;
//! * [`wasapi_render`] is the only Windows-facing module and renders 48 kHz
//!   mono, paced from the RTP timestamps established in `media-core`.

pub mod degraded;
pub mod pacing;
pub mod readiness;

#[cfg(windows)]
pub mod wasapi_render;

pub use degraded::{DegradedReport, SessionMode, degraded_report};
pub use readiness::{
    AudioEndpoint, AudioReadiness, DeviceInventory, DeviceState, Direction, EndpointIdentity,
    ExpectedDevice, SignatureStatus, TEST_TONE_HZ, TEST_TONE_MS, TestToneSink, assess,
    vb_cable_expected,
};

#[cfg(windows)]
pub use wasapi_render::{CableRenderer, RenderError};
