//! F0 desktop receive-pipeline harness (non-production).
//!
//! The binary in `src/main.rs` owns the run matrix. This library keeps the
//! deterministic pieces (relay, synthetic source, drift/stall analysis, CSV
//! writers) testable without a Windows reference machine.

pub mod drift_run;
pub mod latency_capture;
pub mod relay;
pub mod resource;
pub mod stall;
pub mod synthetic_source;
