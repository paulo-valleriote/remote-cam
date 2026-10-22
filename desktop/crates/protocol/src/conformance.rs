//! Vector-driven conformance harness shared by the integration test and the
//! `conformance` binary.
//!
//! Both language implementations execute the same vector files and emit the
//! same canonical result document, which `protocol/vectors/run_conformance.sh`
//! compares.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::hex::{decode_hex, decode_hex_exact, encode_hex};
use crate::qr::{Endpoint, QrPayload};
use crate::reason::{ReasonCategory, ReasonCode};
use crate::signaling::parse_signaling;

/// Canonical result document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConformanceReport {
    pub qr: Vec<QrResult>,
    pub signaling: Vec<SignalingResult>,
    pub reason_codes: ReasonCodeResult,
}

/// Observed outcome for one QR vector.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QrResult {
    pub id: String,
    pub accepted: bool,
    pub reason_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decoded_payload_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoded_payload_hex: Option<String>,
}

/// Observed outcome for one signaling scenario.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignalingResult {
    pub id: String,
    pub kind: String,
    pub accepted: bool,
    pub reason_code: Option<String>,
}

/// Observed state of the closed reason-code enumeration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReasonCodeResult {
    pub count: usize,
    pub ok: bool,
}

#[derive(Debug, Deserialize)]
struct QrVectorFile {
    id: String,
    now_unix: u64,
    expected: QrExpectation,
    encoder_input: Option<QrEncoderInput>,
}

#[derive(Debug, Deserialize)]
struct QrExpectation {
    accepted: bool,
    reason_code: Option<String>,
    payload_hex: String,
}

#[derive(Debug, Deserialize)]
struct QrEncoderInput {
    session_id_hex: String,
    epoch: u32,
    expires_at: u64,
    pairing_token_hex: String,
    tls_fingerprint_hex: String,
    capabilities: u16,
    endpoints: Vec<EndpointInput>,
}

#[derive(Debug, Deserialize)]
struct EndpointInput {
    family: u8,
    address: String,
    port: u16,
}

#[derive(Debug, Deserialize)]
struct ScenarioFile {
    scenarios: Vec<Scenario>,
}

#[derive(Debug, Deserialize)]
struct Scenario {
    id: String,
    kind: String,
    message: Option<serde_json::Value>,
    raw_frame: Option<String>,
    expected_outcome: String,
    expected_reason_code: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReasonCodeDocument {
    codes: Vec<ReasonCodeEntry>,
}

#[derive(Debug, Deserialize)]
struct ReasonCodeEntry {
    code: String,
    categories: Vec<String>,
    security_event: bool,
    user_actionable: bool,
}

impl ConformanceReport {
    /// True when every vector produced its pinned outcome.
    pub fn passed(&self) -> bool {
        self.failures().is_empty()
    }

    /// Human-readable list of divergences from the pinned outcomes.
    pub fn failures(&self) -> Vec<String> {
        let mut failures = Vec::new();
        for result in &self.qr {
            if !result.accepted && result.reason_code.is_none() {
                failures.push(format!("{}: rejected without a reason code", result.id));
            }
        }
        for result in &self.signaling {
            if !result.accepted && result.reason_code.is_none() {
                failures.push(format!("{}: rejected without a reason code", result.id));
            }
        }
        if !self.reason_codes.ok {
            failures.push("reason-code enumeration does not match the contract".to_owned());
        }
        failures
    }
}

/// Executes every vector under `vectors_dir`.
pub fn run_vectors(vectors_dir: &Path) -> Result<ConformanceReport, String> {
    let qr = run_qr_vectors(&vectors_dir.join("qr"))?;
    let signaling = run_signaling_scenarios(&vectors_dir.join("signaling/scenarios.json"))?;
    let reason_codes = check_reason_codes(&vectors_dir.join("../reason-codes.json"))?;
    Ok(ConformanceReport {
        qr,
        signaling,
        reason_codes,
    })
}

/// Locates the repository root from this crate's manifest directory.
pub fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("../../.."))
}

fn run_qr_vectors(directory: &Path) -> Result<Vec<QrResult>, String> {
    let mut files: Vec<PathBuf> = fs::read_dir(directory)
        .map_err(|error| format!("cannot read {}: {error}", directory.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    files.sort();

    let mut results = Vec::new();
    for file in files {
        let text = fs::read_to_string(&file).map_err(|error| error.to_string())?;
        let vector: QrVectorFile =
            serde_json::from_str(&text).map_err(|error| format!("{}: {error}", file.display()))?;
        let payload = decode_hex(&vector.expected.payload_hex)
            .ok_or_else(|| format!("{}: invalid expected payload hex", vector.id))?;
        let observed = QrPayload::decode(&payload, vector.now_unix);

        let (accepted, reason_code, decoded_payload_hex, encoded_payload_hex) = match observed {
            Ok(decoded) => {
                let decoded_hex = decoded
                    .encode()
                    .map(|bytes| encode_hex(&bytes))
                    .map_err(|error| format!("{}: re-encode failed: {error:?}", vector.id))?;
                let encoded_hex = match &vector.encoder_input {
                    Some(input) => Some(encode_from_input(input).map_err(|error| {
                        format!("{}: encoder input invalid: {error:?}", vector.id)
                    })?),
                    None => None,
                };
                (true, None, Some(decoded_hex), encoded_hex)
            }
            Err(error) => (false, Some(error.code.as_str().to_owned()), None, None),
        };

        let expected_reason = vector.expected.reason_code.clone();
        if accepted != vector.expected.accepted || reason_code != expected_reason {
            return Err(format!(
                "{}: expected accepted={} reason={:?}, observed accepted={} reason={:?}",
                vector.id, vector.expected.accepted, expected_reason, accepted, reason_code
            ));
        }
        if let Some(encoded) = &encoded_payload_hex
            && encoded != &vector.expected.payload_hex
        {
            return Err(format!(
                "{}: encoder produced {encoded}, expected {}",
                vector.id, vector.expected.payload_hex
            ));
        }
        if let Some(decoded) = &decoded_payload_hex
            && decoded != &vector.expected.payload_hex
        {
            return Err(format!(
                "{}: decoded payload re-encodes to {decoded}, expected {}",
                vector.id, vector.expected.payload_hex
            ));
        }

        results.push(QrResult {
            id: vector.id,
            accepted,
            reason_code,
            decoded_payload_hex,
            encoded_payload_hex,
        });
    }
    Ok(results)
}

fn encode_from_input(input: &QrEncoderInput) -> Result<String, String> {
    let session_id = fixed::<16>(&input.session_id_hex, "session_id_hex")?;
    let pairing_token = fixed::<32>(&input.pairing_token_hex, "pairing_token_hex")?;
    let tls_fingerprint = fixed::<32>(&input.tls_fingerprint_hex, "tls_fingerprint_hex")?;
    let endpoints: Vec<Endpoint> = input
        .endpoints
        .iter()
        .map(|endpoint| {
            Endpoint::from_address(endpoint.family, &endpoint.address, endpoint.port)
                .ok_or_else(|| format!("invalid endpoint {}", endpoint.address))
        })
        .collect::<Result<_, _>>()?;
    let payload = QrPayload {
        session_id,
        epoch: input.epoch,
        expires_at: input.expires_at,
        pairing_token,
        tls_fingerprint,
        capabilities: input.capabilities,
        endpoints,
    };
    payload
        .encode()
        .map(|bytes| encode_hex(&bytes))
        .map_err(|error| format!("{error:?}"))
}

fn fixed<const N: usize>(text: &str, field: &str) -> Result<[u8; N], String> {
    let decoded =
        decode_hex_exact(text, N).ok_or_else(|| format!("{field} is not {N} bytes of hex"))?;
    let mut output = [0_u8; N];
    output.copy_from_slice(&decoded);
    Ok(output)
}

fn run_signaling_scenarios(path: &Path) -> Result<Vec<SignalingResult>, String> {
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let file: ScenarioFile = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    let mut results = Vec::new();
    for scenario in file.scenarios {
        if scenario.kind != "message" {
            continue;
        }
        let frame = match (&scenario.raw_frame, &scenario.message) {
            (Some(raw), _) => raw.clone().into_bytes(),
            (None, Some(message)) => {
                serde_json::to_vec(message).map_err(|error| error.to_string())?
            }
            (None, None) => return Err(format!("{}: no frame", scenario.id)),
        };
        let observed = parse_signaling(&frame);
        let (accepted, reason_code) = match observed {
            Ok(_) => (true, None),
            Err(error) => (false, Some(error.code.as_str().to_owned())),
        };
        let expected_accepted = scenario.expected_outcome == "accept";
        if accepted != expected_accepted || reason_code != scenario.expected_reason_code {
            return Err(format!(
                "{}: expected accepted={} reason={:?}, observed accepted={} reason={:?}",
                scenario.id,
                expected_accepted,
                scenario.expected_reason_code,
                accepted,
                reason_code
            ));
        }
        results.push(SignalingResult {
            id: scenario.id,
            kind: scenario.kind,
            accepted,
            reason_code,
        });
    }
    Ok(results)
}

fn check_reason_codes(path: &Path) -> Result<ReasonCodeResult, String> {
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let document: ReasonCodeDocument =
        serde_json::from_str(&text).map_err(|error| error.to_string())?;
    let mut ok = document.codes.len() == ReasonCode::ALL.len();
    for entry in &document.codes {
        let Some(code) = ReasonCode::parse(&entry.code) else {
            ok = false;
            continue;
        };
        let categories: Vec<ReasonCategory> = code.categories().to_vec();
        let spellings: Vec<&str> = categories.iter().map(|item| item.as_str()).collect();
        if spellings
            != entry
                .categories
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
            || code.is_security_event() != entry.security_event
            || code.is_user_actionable() != entry.user_actionable
        {
            ok = false;
        }
    }
    for code in ReasonCode::ALL {
        if !document
            .codes
            .iter()
            .any(|entry| entry.code == code.as_str())
        {
            ok = false;
        }
    }
    Ok(ReasonCodeResult {
        count: document.codes.len(),
        ok,
    })
}
