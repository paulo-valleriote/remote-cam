//! Virtual camera injection for RemoteCam.
//!
//! **NON-PRODUCTION**: this crate is a risk-reduction spike (F0). The ring
//! buffer and the test-pattern producer are platform neutral and are exercised
//! on the Linux CI; the Media Foundation media source and its registration are
//! Windows-only and are compiled for `x86_64-pc-windows-msvc` before they are
//! run on the reference machine.
//!
//! The crate is intentionally free of transport and UI concerns: it accepts
//! decoded NV12 frames and presents them to a consumer application.

pub mod ring_buffer;
pub mod test_pattern;

#[cfg(windows)]
pub mod mf_virtual_camera;

#[cfg(windows)]
pub mod registration;

pub use ring_buffer::{
    Frame, FrameError, FrameReader, FrameWriter, PublishedFrame, ReadTimeout, RingBuffer,
    RingBufferError, RingBufferStats,
};
