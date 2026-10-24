//! The product harness: a headless driver for the RemoteCam Session Core.

use remotecam_session_core::testing::{ManualClock, SeededRandomSource};
use remotecam_session_core::{SessionCore, SessionState, Timestamp};
use serde::Serialize;

/// The per-scenario outcome carried by the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Pass,
    Fail,
}

/// The report-level totals for the scenarios that ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub scenarios: u32,
    pub passed: u32,
    pub failed: u32,
}

/// The deterministic, machine-readable result of one harness run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HarnessReport {
    pub report_version: u32,
    pub scenario: &'static str,
    pub observed_state: &'static str,
    pub verdict: Verdict,
    pub summary: Summary,
}

impl HarnessReport {
    /// Builds the report for the scaffold scenario from the observed state.
    pub fn for_session_state(state: SessionState) -> Self {
        let passed = state == SessionState::Idle;
        Self {
            report_version: 1,
            scenario: "scaffold",
            observed_state: state.as_str(),
            verdict: if passed { Verdict::Pass } else { Verdict::Fail },
            summary: Summary {
                scenarios: 1,
                passed: u32::from(passed),
                failed: u32::from(!passed),
            },
        }
    }

    /// Serialises the report as the single JSON line the harness prints.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

/// Runs the scaffold scenario against a fresh Session Core.
pub fn run() -> HarnessReport {
    let core = SessionCore::new(
        ManualClock::new(Timestamp::ZERO),
        SeededRandomSource::new(0),
    );

    HarnessReport::for_session_state(core.state())
}
