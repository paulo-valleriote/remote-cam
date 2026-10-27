//! Receive-only Media Core (REC-50): RTP H.264 in, NV12 frames out, behind the
//! Session Core's `MediaPort`. The Session Core never sees webrtc-rs or OpenH264.

pub mod core;
pub mod decoder;
pub mod frame_ring;
pub mod nv12;
pub mod port;
pub mod testing;

pub use core::{MediaCore, MediaCoreError, MediaStats};
pub use decoder::{DecodeError, H264Decoder};
pub use frame_ring::{FRAME_RING_SLOTS, FrameRing};
pub use nv12::{FrameError, Nv12Frame};
pub use port::RingMediaPort;
