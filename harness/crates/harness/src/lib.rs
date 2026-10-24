//! The product harness: a headless driver for the RemoteCam Session Core.

use remotecam_protocol::signaling::{Limits, SignalingMessage, parse_signaling};
use remotecam_session_core::testing::{ManualClock, SeededRandomSource};
use remotecam_session_core::{Effect, SessionCore, SessionEvent, SessionState, Timestamp};
use serde::Serialize;

const PAIRING_SEED: u64 = 0;
const PAIRING_CLOCK_START: Timestamp = Timestamp::ZERO;
const DESKTOP_DTLS_FINGERPRINT: [u8; 32] = [0x44; 32];
const MOBILE_DTLS_FINGERPRINT: [u8; 32] = [0x55; 32];

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<&'static str>,
    pub summary: Summary,
}

impl HarnessReport {
    /// Builds the report for the pairing scenario from the observed state.
    pub fn for_session_state(state: SessionState) -> Self {
        if state == SessionState::Connected {
            Self::for_pairing_result(state, None)
        } else {
            Self::for_pairing_result(state, Some("internal_error"))
        }
    }

    fn for_pairing_result(state: SessionState, reason_code: Option<&'static str>) -> Self {
        let passed = reason_code.is_none();
        Self {
            report_version: 1,
            scenario: "pairing",
            observed_state: state.as_str(),
            verdict: if passed { Verdict::Pass } else { Verdict::Fail },
            reason_code,
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

/// Runs a deterministic authenticated pairing through the Session Core.
pub fn run() -> HarnessReport {
    let mut core = SessionCore::new(
        ManualClock::new(PAIRING_CLOCK_START),
        SeededRandomSource::new(PAIRING_SEED),
        DESKTOP_DTLS_FINGERPRINT,
    );

    let pairing_effects = match core.handle(SessionEvent::StartPairing) {
        Ok(effects) => effects,
        Err(error) => {
            return HarnessReport::for_pairing_result(core.state(), Some(error.code().as_str()));
        }
    };
    let [Effect::PresentPairing(material)] = pairing_effects.as_slice() else {
        return HarnessReport::for_pairing_result(core.state(), Some("internal_error"));
    };
    let session_id = material.session_id();
    let epoch = material.epoch();
    let pair_frame = SignalingMessage::Pair {
        token: *material.token().as_bytes(),
        dtls_fingerprint: MOBILE_DTLS_FINGERPRINT,
        limits: Limits::new(),
    };
    let pair_frame = match pair_frame.encode() {
        Ok(frame) => frame,
        Err(_) => {
            return HarnessReport::for_pairing_result(core.state(), Some("internal_error"));
        }
    };

    let peer_effects = match core.handle(SessionEvent::PeerConnected) {
        Ok(effects) => effects,
        Err(error) => {
            return HarnessReport::for_pairing_result(core.state(), Some(error.code().as_str()));
        }
    };
    let media_started_before_authentication = peer_effects.contains(&Effect::StartMedia);
    if media_started_before_authentication {
        return HarnessReport::for_pairing_result(core.state(), Some("internal_error"));
    }

    let paired_effect = match core.authenticate_first_frame(&pair_frame) {
        Ok(effect) => effect,
        Err(error) => {
            return HarnessReport::for_pairing_result(core.state(), Some(error.code().as_str()));
        }
    };
    let Effect::SendPaired {
        session_id: paired_session_id,
        epoch: paired_epoch,
        dtls_fingerprint,
        accepted_limits,
    } = paired_effect
    else {
        return HarnessReport::for_pairing_result(core.state(), Some("internal_error"));
    };
    let paired_frame = SignalingMessage::Paired {
        session_id: *paired_session_id.as_bytes(),
        epoch: paired_epoch,
        dtls_fingerprint,
        accepted_limits,
    };
    let paired_frame = match paired_frame.encode() {
        Ok(frame) => frame,
        Err(_) => {
            return HarnessReport::for_pairing_result(core.state(), Some("internal_error"));
        }
    };
    let paired_response_is_valid = matches!(
        parse_signaling(&paired_frame),
        Ok(SignalingMessage::Paired {
            session_id: paired_session_id,
            epoch: paired_epoch,
            dtls_fingerprint,
            accepted_limits,
        }) if paired_session_id == *session_id.as_bytes()
            && paired_epoch == epoch
            && dtls_fingerprint == DESKTOP_DTLS_FINGERPRINT
            && accepted_limits == Limits::new()
    );
    if !paired_response_is_valid {
        return HarnessReport::for_pairing_result(core.state(), Some("invalid_message"));
    }

    let media_effects = match core.handle(SessionEvent::NegotiationCompleted) {
        Ok(effects) => effects,
        Err(error) => {
            return HarnessReport::for_pairing_result(core.state(), Some(error.code().as_str()));
        }
    };
    let media_started_after_authentication = media_effects == [Effect::StartMedia];
    if !media_started_after_authentication {
        return HarnessReport::for_pairing_result(core.state(), Some("internal_error"));
    }
    if core.state() != SessionState::Connected {
        return HarnessReport::for_pairing_result(core.state(), Some("invalid_transition"));
    }
    HarnessReport::for_pairing_result(core.state(), None)
}
