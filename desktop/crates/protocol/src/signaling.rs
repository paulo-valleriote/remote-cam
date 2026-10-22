//! Strict parser and validator for the signaling message allowlist.
//!
//! Unknown fields are rejected, never ignored. A message that does not satisfy
//! `protocol/signaling.schema.json` carries exactly one closed reason code.

use serde_json::{Map, Value};

use crate::address::is_private_address;
use crate::hex::decode_hex_exact;
use crate::reason::ReasonCode;

/// Maximum encoded message size. A larger frame is refused.
pub const MAX_SIGNALING_MESSAGE_BYTES: usize = 16 * 1024;

const MESSAGE_TYPES: [&str; 10] = [
    "pair", "paired", "offer", "answer", "ice", "stats", "close", "error", "ping", "pong",
];

const CLOSE_CODES: [ReasonCode; 7] = [
    ReasonCode::EndedByMobile,
    ReasonCode::EndedByDesktop,
    ReasonCode::Superseded,
    ReasonCode::Timeout,
    ReasonCode::MediaFailure,
    ReasonCode::SecurityViolation,
    ReasonCode::InternalError,
];

const ERROR_CODES: [ReasonCode; 4] = [
    ReasonCode::InvalidMessage,
    ReasonCode::UnsupportedVersion,
    ReasonCode::RateLimited,
    ReasonCode::InternalError,
];

const THERMAL_STATUSES: [&str; 7] = [
    "none",
    "light",
    "moderate",
    "severe",
    "critical",
    "emergency",
    "shutdown",
];

/// Requested capture limits. Every field is optional; supplied values are
/// range-checked.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Limits {
    pub max_width: Option<u32>,
    pub max_height: Option<u32>,
    pub max_frame_rate: Option<u32>,
    pub max_video_bitrate_kbps: Option<u32>,
    pub max_audio_bitrate_kbps: Option<u32>,
}

/// One ICE candidate. Only UDP host candidates with a private address are valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub address: String,
    pub port: u16,
    pub component: u8,
    pub protocol: Option<String>,
}

/// Optional client metrics. Carries no identifiers.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Stats {
    pub fps: Option<f64>,
    pub video_bitrate_kbps: Option<f64>,
    pub rtt_ms: Option<f64>,
    pub loss_pct: Option<f64>,
    pub thermal_status: Option<String>,
}

/// A validated signaling message.
#[derive(Debug, Clone, PartialEq)]
pub enum SignalingMessage {
    Pair {
        token: [u8; 32],
        dtls_fingerprint: [u8; 32],
        limits: Limits,
    },
    Paired {
        session_id: [u8; 16],
        epoch: u64,
        dtls_fingerprint: [u8; 32],
        accepted_limits: Limits,
    },
    Offer {
        epoch: u64,
        sdp: String,
    },
    Answer {
        epoch: u64,
        sdp: String,
    },
    Ice {
        epoch: u64,
        candidates: Vec<Candidate>,
    },
    Stats(Stats),
    Close {
        code: ReasonCode,
    },
    Error {
        code: ReasonCode,
        message: Option<String>,
    },
    Ping,
    Pong,
}

/// A parse failure carrying the closed reason code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseError {
    pub code: ReasonCode,
}

impl ParseError {
    fn invalid() -> Self {
        Self {
            code: ReasonCode::InvalidMessage,
        }
    }

    fn unsupported_version() -> Self {
        Self {
            code: ReasonCode::UnsupportedVersion,
        }
    }
}

/// Parses and validates one signaling frame.
pub fn parse_signaling(input: &[u8]) -> Result<SignalingMessage, ParseError> {
    if input.len() > MAX_SIGNALING_MESSAGE_BYTES {
        return Err(ParseError::invalid());
    }
    let value: Value = serde_json::from_slice(input).map_err(|_| ParseError::invalid())?;
    let map = value.as_object().ok_or_else(ParseError::invalid)?;

    parse_version(map)?;
    let message_type = parse_message_type(map)?;
    match message_type {
        "pair" => {
            check_keys(
                map,
                &["version", "type", "token", "dtls_fingerprint", "limits"],
            )?;
            let token = parse_fixed_hex::<32>(map, "token")?;
            let dtls_fingerprint = parse_fixed_hex::<32>(map, "dtls_fingerprint")?;
            let limits = parse_limits(required(map, "limits")?)?;
            Ok(SignalingMessage::Pair {
                token,
                dtls_fingerprint,
                limits,
            })
        }
        "paired" => {
            check_keys(
                map,
                &[
                    "version",
                    "type",
                    "session_id",
                    "epoch",
                    "dtls_fingerprint",
                    "accepted_limits",
                ],
            )?;
            let session_id = parse_fixed_hex::<16>(map, "session_id")?;
            let epoch = parse_epoch(map)?;
            let dtls_fingerprint = parse_fixed_hex::<32>(map, "dtls_fingerprint")?;
            let accepted_limits = parse_limits(required(map, "accepted_limits")?)?;
            Ok(SignalingMessage::Paired {
                session_id,
                epoch,
                dtls_fingerprint,
                accepted_limits,
            })
        }
        "offer" | "answer" => {
            check_keys(map, &["version", "type", "epoch", "sdp"])?;
            let epoch = parse_epoch(map)?;
            let sdp = parse_sdp(map)?;
            if message_type == "offer" {
                Ok(SignalingMessage::Offer { epoch, sdp })
            } else {
                Ok(SignalingMessage::Answer { epoch, sdp })
            }
        }
        "ice" => {
            check_keys(map, &["version", "type", "epoch", "candidates"])?;
            let epoch = parse_epoch(map)?;
            let candidates = parse_candidates(required(map, "candidates")?)?;
            Ok(SignalingMessage::Ice { epoch, candidates })
        }
        "stats" => {
            check_keys(
                map,
                &[
                    "version",
                    "type",
                    "fps",
                    "video_bitrate_kbps",
                    "rtt_ms",
                    "loss_pct",
                    "thermal_status",
                ],
            )?;
            Ok(SignalingMessage::Stats(parse_stats(map)?))
        }
        "close" => {
            check_keys(map, &["version", "type", "code"])?;
            let raw = parse_string(map, "code")?;
            let code = CLOSE_CODES
                .into_iter()
                .find(|candidate| candidate.as_str() == raw)
                .ok_or_else(ParseError::invalid)?;
            Ok(SignalingMessage::Close { code })
        }
        "error" => {
            check_keys(map, &["version", "type", "code", "message"])?;
            let raw = parse_string(map, "code")?;
            let code = ERROR_CODES
                .into_iter()
                .find(|candidate| candidate.as_str() == raw)
                .ok_or_else(ParseError::invalid)?;
            let message = match map.get("message") {
                Some(value) => {
                    let text = value.as_str().ok_or_else(ParseError::invalid)?;
                    if text.chars().count() > 200 {
                        return Err(ParseError::invalid());
                    }
                    Some(text.to_owned())
                }
                None => None,
            };
            Ok(SignalingMessage::Error { code, message })
        }
        "ping" => {
            check_keys(map, &["version", "type"])?;
            Ok(SignalingMessage::Ping)
        }
        "pong" => {
            check_keys(map, &["version", "type"])?;
            Ok(SignalingMessage::Pong)
        }
        _ => Err(ParseError::invalid()),
    }
}

fn required<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a Value, ParseError> {
    map.get(key).ok_or_else(ParseError::invalid)
}

fn check_keys(map: &Map<String, Value>, allowed: &[&str]) -> Result<(), ParseError> {
    if map.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(ParseError::invalid());
    }
    Ok(())
}

fn parse_version(map: &Map<String, Value>) -> Result<(), ParseError> {
    let version = required(map, "version")?
        .as_u64()
        .ok_or_else(ParseError::invalid)?;
    match version {
        1 => Ok(()),
        2.. => Err(ParseError::unsupported_version()),
        _ => Err(ParseError::invalid()),
    }
}

fn parse_message_type(map: &Map<String, Value>) -> Result<&str, ParseError> {
    let value = required(map, "type")?
        .as_str()
        .ok_or_else(ParseError::invalid)?;
    if !MESSAGE_TYPES.contains(&value) {
        return Err(ParseError::invalid());
    }
    Ok(value)
}

fn parse_fixed_hex<const N: usize>(
    map: &Map<String, Value>,
    key: &str,
) -> Result<[u8; N], ParseError> {
    let text = parse_string(map, key)?;
    let decoded = decode_hex_exact(text, N).ok_or_else(ParseError::invalid)?;
    let mut output = [0_u8; N];
    output.copy_from_slice(&decoded);
    Ok(output)
}

fn parse_string<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a str, ParseError> {
    required(map, key)?.as_str().ok_or_else(ParseError::invalid)
}

fn parse_epoch(map: &Map<String, Value>) -> Result<u64, ParseError> {
    required(map, "epoch")?
        .as_u64()
        .ok_or_else(ParseError::invalid)
}

fn parse_sdp(map: &Map<String, Value>) -> Result<String, ParseError> {
    let sdp = parse_string(map, "sdp")?;
    let length = sdp.chars().count();
    if !(1..=8192).contains(&length) {
        return Err(ParseError::invalid());
    }
    Ok(sdp.to_owned())
}

fn parse_object(value: &Value) -> Result<&Map<String, Value>, ParseError> {
    value.as_object().ok_or_else(ParseError::invalid)
}

fn optional_u32(
    map: &Map<String, Value>,
    key: &str,
    minimum: u32,
    maximum: u32,
) -> Result<Option<u32>, ParseError> {
    match map.get(key) {
        None => Ok(None),
        Some(value) => {
            let number = value.as_u64().ok_or_else(ParseError::invalid)?;
            if !(u64::from(minimum)..=u64::from(maximum)).contains(&number) {
                return Err(ParseError::invalid());
            }
            Ok(Some(number as u32))
        }
    }
}

fn parse_limits(value: &Value) -> Result<Limits, ParseError> {
    let map = parse_object(value)?;
    check_keys(
        map,
        &[
            "max_width",
            "max_height",
            "max_frame_rate",
            "max_video_bitrate_kbps",
            "max_audio_bitrate_kbps",
        ],
    )?;
    Ok(Limits {
        max_width: optional_u32(map, "max_width", 160, 3840)?,
        max_height: optional_u32(map, "max_height", 120, 2160)?,
        max_frame_rate: optional_u32(map, "max_frame_rate", 1, 60)?,
        max_video_bitrate_kbps: optional_u32(map, "max_video_bitrate_kbps", 200, 8000)?,
        max_audio_bitrate_kbps: optional_u32(map, "max_audio_bitrate_kbps", 8, 128)?,
    })
}

fn parse_candidates(value: &Value) -> Result<Vec<Candidate>, ParseError> {
    let items = value.as_array().ok_or_else(ParseError::invalid)?;
    if items.len() > 64 {
        return Err(ParseError::invalid());
    }
    items.iter().map(parse_candidate).collect()
}

fn parse_candidate(value: &Value) -> Result<Candidate, ParseError> {
    let map = parse_object(value)?;
    check_keys(map, &["address", "port", "component", "protocol"])?;
    let address = parse_string(map, "address")?;
    if !is_private_address(address) {
        return Err(ParseError::invalid());
    }
    let port = required(map, "port")?
        .as_u64()
        .filter(|port| (1..=65535).contains(port))
        .ok_or_else(ParseError::invalid)?;
    let component = required(map, "component")?
        .as_u64()
        .filter(|component| (1..=2).contains(component))
        .ok_or_else(ParseError::invalid)?;
    let protocol = match map.get("protocol") {
        None => None,
        Some(value) => {
            let protocol = value.as_str().ok_or_else(ParseError::invalid)?;
            if protocol != "udp" {
                return Err(ParseError::invalid());
            }
            Some(protocol.to_owned())
        }
    };
    Ok(Candidate {
        address: address.to_owned(),
        port: port as u16,
        component: component as u8,
        protocol,
    })
}

fn optional_f64(
    map: &Map<String, Value>,
    key: &str,
    minimum: f64,
    maximum: f64,
) -> Result<Option<f64>, ParseError> {
    match map.get(key) {
        None => Ok(None),
        Some(value) => {
            let number = value.as_f64().ok_or_else(ParseError::invalid)?;
            if !(minimum..=maximum).contains(&number) {
                return Err(ParseError::invalid());
            }
            Ok(Some(number))
        }
    }
}

fn parse_stats(map: &Map<String, Value>) -> Result<Stats, ParseError> {
    let fps = optional_f64(map, "fps", 0.0, 120.0)?;
    let video_bitrate_kbps = optional_f64(map, "video_bitrate_kbps", 0.0, f64::MAX)?;
    let rtt_ms = optional_f64(map, "rtt_ms", 0.0, f64::MAX)?;
    let loss_pct = optional_f64(map, "loss_pct", 0.0, 100.0)?;
    let thermal_status = match map.get("thermal_status") {
        None => None,
        Some(value) => {
            let status = value.as_str().ok_or_else(ParseError::invalid)?;
            if !THERMAL_STATUSES.contains(&status) {
                return Err(ParseError::invalid());
            }
            Some(status.to_owned())
        }
    };
    Ok(Stats {
        fps,
        video_bitrate_kbps,
        rtt_ms,
        loss_pct,
        thermal_status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<SignalingMessage, ParseError> {
        parse_signaling(text.as_bytes())
    }

    #[test]
    fn parses_every_message_type() {
        for text in [
            r#"{"version":1,"type":"pair","token":"0000000000000000000000000000000000000000000000000000000000000000","dtls_fingerprint":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","limits":{}}"#,
            r#"{"version":1,"type":"paired","session_id":"00112233445566778899aabbccddeeff","epoch":1,"dtls_fingerprint":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","accepted_limits":{}}"#,
            r#"{"version":1,"type":"offer","epoch":1,"sdp":"v=0"}"#,
            r#"{"version":1,"type":"answer","epoch":1,"sdp":"v=0"}"#,
            r#"{"version":1,"type":"ice","epoch":1,"candidates":[]}"#,
            r#"{"version":1,"type":"stats"}"#,
            r#"{"version":1,"type":"close","code":"timeout"}"#,
            r#"{"version":1,"type":"error","code":"internal_error"}"#,
            r#"{"version":1,"type":"ping"}"#,
            r#"{"version":1,"type":"pong"}"#,
        ] {
            assert!(parse(text).is_ok(), "should accept: {text}");
        }
    }

    #[test]
    fn rejects_newer_versions_with_the_dedicated_code() {
        assert_eq!(
            parse(r#"{"version":2,"type":"ping"}"#),
            Err(ParseError::unsupported_version())
        );
        assert_eq!(
            parse(r#"{"version":0,"type":"ping"}"#),
            Err(ParseError::invalid())
        );
    }

    #[test]
    fn rejects_unknown_fields_and_types() {
        assert_eq!(
            parse(r#"{"version":1,"type":"ping","x":1}"#),
            Err(ParseError::invalid())
        );
        assert_eq!(
            parse(r#"{"version":1,"type":"unknown"}"#),
            Err(ParseError::invalid())
        );
    }

    #[test]
    fn enforces_the_frame_size_cap() {
        let oversized = vec![b' '; MAX_SIGNALING_MESSAGE_BYTES + 1];
        assert_eq!(parse_signaling(&oversized), Err(ParseError::invalid()));
    }

    #[test]
    fn parses_limits_and_rejects_out_of_range_values() {
        let message = parse(
            r#"{"version":1,"type":"pair","token":"0000000000000000000000000000000000000000000000000000000000000000","dtls_fingerprint":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","limits":{"max_width":1280,"max_height":720}}"#,
        )
        .unwrap();
        assert!(matches!(message, SignalingMessage::Pair { .. }));

        assert!(parse(
            r#"{"version":1,"type":"pair","token":"0000000000000000000000000000000000000000000000000000000000000000","dtls_fingerprint":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","limits":{"max_width":9000}}"#,
        )
        .is_err());
    }

    #[test]
    fn parses_candidates_and_rejects_public_or_tcp_ones() {
        let valid = parse(
            r#"{"version":1,"type":"ice","epoch":1,"candidates":[{"address":"192.168.1.42","port":5000,"component":1,"protocol":"udp"}]}"#,
        );
        assert!(matches!(valid, Ok(SignalingMessage::Ice { .. })));

        for invalid in [
            r#"{"version":1,"type":"ice","epoch":1,"candidates":[{"address":"8.8.8.8","port":5000,"component":1}]}"#,
            r#"{"version":1,"type":"ice","epoch":1,"candidates":[{"address":"192.168.1.42","port":5000,"component":1,"protocol":"tcp"}]}"#,
            r#"{"version":1,"type":"ice","epoch":1,"candidates":[{"address":"192.168.1.42","port":5000,"component":3}]}"#,
        ] {
            assert!(parse(invalid).is_err(), "should reject: {invalid}");
        }
    }

    #[test]
    fn validates_stats_ranges_and_thermal_status() {
        assert!(parse(r#"{"version":1,"type":"stats","fps":120}"#).is_ok());
        assert!(parse(r#"{"version":1,"type":"stats","fps":121}"#).is_err());
        assert!(parse(r#"{"version":1,"type":"stats","thermal_status":"critical"}"#).is_ok());
        assert!(parse(r#"{"version":1,"type":"stats","thermal_status":"warm"}"#).is_err());
    }

    #[test]
    fn validates_error_text_length() {
        let short = format!(
            r#"{{"version":1,"type":"error","code":"internal_error","message":"{}"}}"#,
            "x".repeat(200)
        );
        assert!(parse(&short).is_ok());
        let long = format!(
            r#"{{"version":1,"type":"error","code":"internal_error","message":"{}"}}"#,
            "x".repeat(201)
        );
        assert!(parse(&long).is_err());
    }

    #[test]
    fn rejects_close_codes_outside_the_session_enumeration() {
        assert!(parse(r#"{"version":1,"type":"close","code":"rate_limited"}"#).is_err());
        assert!(parse(r#"{"version":1,"type":"error","code":"timeout"}"#).is_err());
    }
}
