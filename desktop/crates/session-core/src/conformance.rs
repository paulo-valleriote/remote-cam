//! Replays the pinned handshake scenarios on the Session Core.
//!
//! The Kotlin Session Core replays the same `protocol/vectors` scenarios and
//! emits the same result document; `protocol/vectors/compare_conformance.py`
//! fails when the two disagree (ADR-0018).

use std::fs;
use std::path::{Path, PathBuf};

use remotecam_protocol::hex::decode_hex;
use remotecam_protocol::signaling::{Limits, SignalingMessage};
use remotecam_protocol::{QrPayload, ReasonCode};
use serde::{Deserialize, Serialize};

use crate::testing::{ManualClock, SeededRandomSource};
use crate::{Clock, Effect, MediaFrame, SessionCore, SessionEvent, Timestamp};

const DESKTOP_FINGERPRINT: [u8; 32] = [0x44; 32];
const PEER_FINGERPRINT: [u8; 32] = [0x55; 32];
const FORGED_TOKEN: [u8; 32] = [0xee; 32];
const SEED: u64 = 7;

/// Observed outcome for one handshake scenario.
///
/// Every field a layer does not decide is `None`, and the document always
/// carries the full key set so the two implementations compare byte for byte.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandshakeResult {
    pub id: String,
    pub layer: String,
    pub executed: bool,
    pub accepted: Option<bool>,
    pub reason_code: Option<String>,
    pub state: Option<String>,
    pub transition: Option<String>,
    pub epoch: Option<u32>,
    pub protocol_violations: Option<u64>,
}

/// The canonical document written by the `handshake-conformance` binary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandshakeReport {
    pub handshake: Vec<HandshakeResult>,
}

#[derive(Debug, Deserialize)]
struct ScenarioFile {
    scenarios: Vec<Scenario>,
}

#[derive(Debug, Deserialize)]
struct Scenario {
    id: String,
    kind: String,
    layer: Option<String>,
    steps: Option<Vec<Step>>,
    qr_vector: Option<String>,
    expected_outcome: String,
    expected_reason_code: Option<String>,
    expected_state: Option<String>,
    expected_transition: Option<String>,
    expected_epoch: Option<u32>,
    expected_protocol_violations: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct Step {
    action: String,
    token: Option<String>,
    millis: Option<u64>,
    epoch: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct QrVector {
    now_unix: u64,
    expected: QrExpectation,
}

#[derive(Debug, Deserialize)]
struct QrExpectation {
    payload_hex: String,
}

/// Locates the repository root from this crate's manifest directory.
pub fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("../../.."))
}

/// Runs the `handshake_conformance` command line.
///
/// Returns the document to print when `--output` is absent, `None` when it was
/// written to the requested file, and the failure text otherwise.
pub fn run_cli(arguments: &[String]) -> Result<Option<String>, String> {
    const USAGE: &str = "usage: handshake_conformance [--vectors <directory>] [--output <file>]";
    let mut vectors = repository_root().join("protocol/vectors");
    let mut output: Option<PathBuf> = None;
    let mut remaining = arguments.iter();
    while let Some(argument) = remaining.next() {
        match (argument.as_str(), remaining.next()) {
            ("--vectors", Some(value)) => vectors = PathBuf::from(value),
            ("--output", Some(value)) => output = Some(PathBuf::from(value)),
            _ => return Err(USAGE.to_owned()),
        }
    }

    let report = HandshakeReport {
        handshake: run_handshake_scenarios(&vectors)?,
    };
    let document = serde_json::to_string_pretty(&report)
        .map_err(|error| format!("cannot serialize result: {error}"))?;
    match output {
        Some(path) => {
            fs::write(&path, format!("{document}\n"))
                .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            Ok(None)
        }
        None => Ok(Some(document)),
    }
}

/// Replays every `kind = handshake` scenario under `vectors_dir`.
///
/// A scenario whose observed outcome differs from its pinned expectation, or
/// that names an unknown layer or step, is an error: nothing is skipped.
pub fn run_handshake_scenarios(vectors_dir: &Path) -> Result<Vec<HandshakeResult>, String> {
    let path = vectors_dir.join("signaling/scenarios.json");
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let file: ScenarioFile =
        serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;

    let mut results = Vec::new();
    for scenario in file.scenarios.iter().filter(|s| s.kind == "handshake") {
        let result = match scenario.layer.as_deref() {
            Some("core") => replay_core(scenario)?,
            Some("qr") => replay_qr(scenario, vectors_dir)?,
            Some("transport") => HandshakeResult {
                id: scenario.id.clone(),
                layer: "transport".to_owned(),
                executed: false,
                accepted: None,
                reason_code: None,
                state: None,
                transition: None,
                epoch: None,
                protocol_violations: None,
            },
            Some(other) => return Err(format!("{}: unknown layer {other}", scenario.id)),
            None => return Err(format!("{}: scenario has no layer", scenario.id)),
        };
        check_expectation(scenario, &result)?;
        results.push(result);
    }
    Ok(results)
}

fn check_expectation(scenario: &Scenario, result: &HandshakeResult) -> Result<(), String> {
    if !result.executed {
        return Ok(());
    }
    let expected_accepted = scenario.expected_outcome == "accept";
    let mut mismatches = Vec::new();
    if result.accepted != Some(expected_accepted) {
        mismatches.push(format!(
            "accepted {:?} != {expected_accepted}",
            result.accepted
        ));
    }
    if result.reason_code != scenario.expected_reason_code {
        mismatches.push(format!(
            "reason_code {:?} != {:?}",
            result.reason_code, scenario.expected_reason_code
        ));
    }
    if result.layer == "core" {
        if result.state != scenario.expected_state {
            mismatches.push(format!(
                "state {:?} != {:?}",
                result.state, scenario.expected_state
            ));
        }
        if result.transition != scenario.expected_transition {
            mismatches.push(format!(
                "transition {:?} != {:?}",
                result.transition, scenario.expected_transition
            ));
        }
        if result.epoch != scenario.expected_epoch {
            mismatches.push(format!(
                "epoch {:?} != {:?}",
                result.epoch, scenario.expected_epoch
            ));
        }
        if result.protocol_violations != scenario.expected_protocol_violations {
            mismatches.push(format!(
                "protocol_violations {:?} != {:?}",
                result.protocol_violations, scenario.expected_protocol_violations
            ));
        }
    }
    if mismatches.is_empty() {
        Ok(())
    } else {
        Err(format!("{}: {}", scenario.id, mismatches.join("; ")))
    }
}

fn replay_qr(scenario: &Scenario, vectors_dir: &Path) -> Result<HandshakeResult, String> {
    let name = scenario
        .qr_vector
        .as_deref()
        .ok_or_else(|| format!("{}: qr scenario has no qr_vector", scenario.id))?;
    let path = vectors_dir.join("qr").join(format!("{name}.json"));
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("{}: cannot read {}: {error}", scenario.id, path.display()))?;
    let vector: QrVector =
        serde_json::from_str(&text).map_err(|error| format!("{}: {error}", scenario.id))?;
    let payload = decode_hex(&vector.expected.payload_hex)
        .ok_or_else(|| format!("{}: invalid payload hex", scenario.id))?;
    let (accepted, reason_code) = match QrPayload::decode(&payload, vector.now_unix) {
        Ok(_) => (true, None),
        Err(error) => (false, Some(error.code.as_str().to_owned())),
    };
    Ok(HandshakeResult {
        id: scenario.id.clone(),
        layer: "qr".to_owned(),
        executed: true,
        accepted: Some(accepted),
        reason_code,
        state: None,
        transition: None,
        epoch: None,
        protocol_violations: None,
    })
}

/// What the last replayed step decided.
struct Outcome {
    reason: Option<String>,
}

struct Replay {
    core: SessionCore<ManualClock, SeededRandomSource>,
    clock: ManualClock,
    tokens: Vec<[u8; 32]>,
}

impl Replay {
    fn new() -> Self {
        let clock = ManualClock::new(Timestamp::ZERO);
        Self {
            core: SessionCore::new(
                clock.clone(),
                SeededRandomSource::new(SEED),
                DESKTOP_FINGERPRINT,
            ),
            clock,
            tokens: Vec::new(),
        }
    }

    fn collect(&mut self, effects: &[Effect]) -> Outcome {
        let mut reason = None;
        for effect in effects {
            match effect {
                Effect::PresentPairing(material) => {
                    self.tokens.push(*material.token().as_bytes());
                }
                Effect::RejectPeer { reason: code } if reason.is_none() => {
                    reason = Some(code.as_str().to_owned());
                }
                _ => {}
            }
        }
        Outcome { reason }
    }

    fn handle(&mut self, event: SessionEvent) -> Outcome {
        match self.core.handle(event) {
            Ok(effects) => self.collect(&effects),
            Err(error) => Outcome {
                reason: Some(error.code().as_str().to_owned()),
            },
        }
    }

    fn token(&self, which: &str, id: &str) -> Result<[u8; 32], String> {
        let presented = match which {
            "wrong" => return Ok(FORGED_TOKEN),
            "first" => self.tokens.first(),
            "latest" => self.tokens.last(),
            other => return Err(format!("{id}: unknown token reference {other}")),
        };
        presented
            .copied()
            .ok_or_else(|| format!("{id}: no pairing token has been presented yet"))
    }

    fn apply(&mut self, step: &Step, id: &str) -> Result<Outcome, String> {
        Ok(match step.action.as_str() {
            "start_pairing" => self.handle(SessionEvent::StartPairing),
            "peer_connected" => self.handle(SessionEvent::PeerConnected),
            "negotiation_completed" => self.handle(SessionEvent::NegotiationCompleted),
            "first_media_frame" => {
                let frame = MediaFrame {
                    sequence: 0,
                    timestamp: self.clock.now(),
                };
                self.handle(SessionEvent::FirstMediaFrame(frame))
            }
            "first_frame_timed_out" => self.handle(SessionEvent::FirstFrameTimedOut),
            "end_session" => self.handle(SessionEvent::EndSession),
            "advance_clock" => {
                let millis = step
                    .millis
                    .ok_or_else(|| format!("{id}: advance_clock needs millis"))?;
                self.clock.advance(millis);
                Outcome { reason: None }
            }
            "pair" => {
                let which = step.token.as_deref().unwrap_or("latest");
                let token = self.token(which, id)?;
                let frame = SignalingMessage::Pair {
                    token,
                    dtls_fingerprint: PEER_FINGERPRINT,
                    limits: Limits::new(),
                }
                .encode()
                .map_err(|error| format!("{id}: cannot encode pair: {error:?}"))?;
                match self.core.authenticate_first_frame(&frame) {
                    Ok(effects) => self.collect(&effects),
                    Err(error) => Outcome {
                        reason: Some(error.code().as_str().to_owned()),
                    },
                }
            }
            "admit_epoch" => {
                let epoch = step
                    .epoch
                    .ok_or_else(|| format!("{id}: admit_epoch needs epoch"))?;
                let reason = (!self.core.admit_frame_epoch(epoch))
                    .then(|| ReasonCode::ProtocolViolation.as_str().to_owned());
                Outcome { reason }
            }
            other => return Err(format!("{id}: unknown step action {other}")),
        })
    }
}

fn replay_core(scenario: &Scenario) -> Result<HandshakeResult, String> {
    let steps = scenario
        .steps
        .as_deref()
        .filter(|steps| !steps.is_empty())
        .ok_or_else(|| format!("{}: core scenario has no steps", scenario.id))?;
    let mut replay = Replay::new();
    let mut before = replay.core.state();
    let mut outcome = Outcome { reason: None };
    for step in steps {
        before = replay.core.state();
        outcome = replay.apply(step, &scenario.id)?;
    }
    let after = replay.core.state();
    Ok(HandshakeResult {
        id: scenario.id.clone(),
        layer: "core".to_owned(),
        executed: true,
        accepted: Some(outcome.reason.is_none()),
        reason_code: outcome.reason,
        state: Some(after.as_str().to_owned()),
        transition: Some(format!("{}->{}", before.as_str(), after.as_str())),
        epoch: replay.core.epoch(),
        protocol_violations: Some(replay.core.protocol_violations()),
    })
}
