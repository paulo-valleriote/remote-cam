//! Platform adapters behind the media-core boundary.
//!
//! The state machine and every policy in this crate are platform neutral. These
//! modules are the only place that touches Media Foundation or WASAPI, so a
//! port or a test double replaces one file instead of the pipeline.

#[cfg(not(windows))]
pub mod software_decoder;

#[cfg(windows)]
pub mod mf_decoder;

#[cfg(windows)]
pub mod wasapi_render;
