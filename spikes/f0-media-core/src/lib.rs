//! Desktop receive pipeline for RemoteCam.
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike (FR-039). This crate is the
//! real risk surface of the product (receive-only WebRTC, adaptive jitter
//! buffer, hardware decode, handoff, A/V sync) but it is deliberately isolated
//! from the application shell and never merged into a product module.
//!
//! The crate is split along testability lines:
//!
//! * [`jitter_policy`], [`drift`], [`resample`], [`sync`] and [`handoff`] are
//!   pure or injected-clock state machines with deterministic tests;
//! * [`receiver`] owns the receive-only peer connection;
//! * [`adapters`] is the only Windows-facing code (Media Foundation decode and
//!   WASAPI render), behind a single module boundary.
//!
//! The Linux CI exercises everything except the Windows adapters. The latency
//! gate itself additionally requires the Windows 11 reference machine, the
//! hardware decoder verified by `verify_h264_hardware_decode`, and a consumer
//! capture; those are described in `docs/spike/gate-3-desktop-receive-pipeline.md`.

pub mod adapters;
pub mod drift;
pub mod handoff;
pub mod jitter_policy;
pub mod receiver;
pub mod resample;
pub mod sync;

pub use drift::{
    DriftComponents, DriftError, DriftEstimate, DriftSample, ResamplerModel, SlidingRatio,
    drift_verdict, estimate_drift,
};
pub use handoff::{
    FrameHandoff, HandoffError, HandoffStats, SessionEvent, SessionEvents, SessionSnapshot,
};
pub use jitter_policy::{
    JitterPolicy, JitterPolicyConfig, JitterStats, PacketDecision, PacketTiming, PlaybackState,
    TickDecision,
};
pub use receiver::{ReceivePipeline, ReceivedTrack, ReceiverConfig, ReceiverError};
pub use resample::MonoResampler;
pub use sync::{AvSync, PlayoutDelays, RtpHeader, RtpParseError, StreamKind, StreamObservation};
