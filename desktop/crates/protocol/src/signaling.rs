//! Strict parser and canonical encoder for the signaling message allowlist.
//!
//! Unknown fields are rejected, never ignored. A message that does not satisfy
//! `protocol/signaling.schema.json` carries exactly one closed reason code, and
//! a message that exists as a typed value always encodes to a frame the parser
//! accepts. The canonical field order is `version`, `type`, then the fields of
//! the message in schema order.

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};
use serde_json::{Map, Value};

use crate::address::is_private_address;
use crate::hex::{decode_hex_exact, encode_hex};
use crate::reason::ReasonCode;

/// Maximum encoded message size. A larger frame is refused.
pub const MAX_SIGNALING_MESSAGE_BYTES: usize = 16 * 1024;

const PROTOCOL_VERSION: u64 = 1;
const MAX_SDP_CHARS: usize = 8192;
const MAX_ERROR_MESSAGE_CHARS: usize = 200;
const MAX_ICE_CANDIDATES: usize = 64;

/// A value the contract forbids. Constructors return it instead of admitting a
/// value that could not be emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidValue;

/// Requested capture limits. Every field is optional; supplied values are
/// range-checked at construction.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct Limits {
    #[serde(skip_serializing_if = "Option::is_none")]
    max_width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_frame_rate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_video_bitrate_kbps: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_audio_bitrate_kbps: Option<u32>,
}

impl Limits {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_max_width(mut self, value: u32) -> Result<Self, InvalidValue> {
        if !(160..=3840).contains(&value) {
            return Err(InvalidValue);
        }
        self.max_width = Some(value);
        Ok(self)
    }

    pub fn with_max_height(mut self, value: u32) -> Result<Self, InvalidValue> {
        if !(120..=2160).contains(&value) {
            return Err(InvalidValue);
        }
        self.max_height = Some(value);
        Ok(self)
    }

    pub fn with_max_frame_rate(mut self, value: u32) -> Result<Self, InvalidValue> {
        if !(1..=60).contains(&value) {
            return Err(InvalidValue);
        }
        self.max_frame_rate = Some(value);
        Ok(self)
    }

    pub fn with_max_video_bitrate_kbps(mut self, value: u32) -> Result<Self, InvalidValue> {
        if !(200..=8000).contains(&value) {
            return Err(InvalidValue);
        }
        self.max_video_bitrate_kbps = Some(value);
        Ok(self)
    }

    pub fn with_max_audio_bitrate_kbps(mut self, value: u32) -> Result<Self, InvalidValue> {
        if !(8..=128).contains(&value) {
            return Err(InvalidValue);
        }
        self.max_audio_bitrate_kbps = Some(value);
        Ok(self)
    }
}

/// A session description body. The contract caps it at 8192 characters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Sdp(String);

impl Sdp {
    pub fn new(text: &str) -> Result<Self, InvalidValue> {
        if !(1..=MAX_SDP_CHARS).contains(&text.chars().count()) {
            return Err(InvalidValue);
        }
        Ok(Self(text.to_owned()))
    }
}

/// Human-readable diagnostic text carried by an `error` message. It never
/// carries host data, paths, user names or public IP addresses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ErrorText(String);

impl ErrorText {
    pub fn new(text: &str) -> Result<Self, InvalidValue> {
        if text.chars().count() > MAX_ERROR_MESSAGE_CHARS {
            return Err(InvalidValue);
        }
        Ok(Self(text.to_owned()))
    }
}

/// The ICE component id of a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IceComponent {
    Rtp,
    Rtcp,
}

impl IceComponent {
    fn as_u8(self) -> u8 {
        match self {
            Self::Rtp => 1,
            Self::Rtcp => 2,
        }
    }

    fn parse(value: u64) -> Option<Self> {
        match value {
            1 => Some(Self::Rtp),
            2 => Some(Self::Rtcp),
            _ => None,
        }
    }
}

impl Serialize for IceComponent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(self.as_u8())
    }
}

/// The only transport protocol the contract permits for a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportProtocol {
    Udp,
}

impl TransportProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Udp => "udp",
        }
    }
}

impl Serialize for TransportProtocol {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

/// One ICE candidate. Only UDP host candidates with a private address are valid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Candidate {
    address: String,
    port: u16,
    component: IceComponent,
    #[serde(skip_serializing_if = "Option::is_none")]
    protocol: Option<TransportProtocol>,
}

impl Candidate {
    pub fn new(
        address: &str,
        port: u16,
        component: IceComponent,
        protocol: Option<TransportProtocol>,
    ) -> Result<Self, InvalidValue> {
        if !is_private_address(address) || port == 0 {
            return Err(InvalidValue);
        }
        Ok(Self {
            address: address.to_owned(),
            port,
            component,
            protocol,
        })
    }
}

/// The candidates of one `ice` message. The contract caps the list at 64.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(transparent)]
pub struct Candidates(Vec<Candidate>);

impl Candidates {
    pub fn new(candidates: Vec<Candidate>) -> Result<Self, InvalidValue> {
        if candidates.len() > MAX_ICE_CANDIDATES {
            return Err(InvalidValue);
        }
        Ok(Self(candidates))
    }
}

/// One value from the closed thermal-status enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThermalStatus {
    None,
    Light,
    Moderate,
    Severe,
    Critical,
    Emergency,
    Shutdown,
}

impl ThermalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Light => "light",
            Self::Moderate => "moderate",
            Self::Severe => "severe",
            Self::Critical => "critical",
            Self::Emergency => "emergency",
            Self::Shutdown => "shutdown",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::None,
            Self::Light,
            Self::Moderate,
            Self::Severe,
            Self::Critical,
            Self::Emergency,
            Self::Shutdown,
        ]
        .into_iter()
        .find(|candidate| candidate.as_str() == value)
    }
}

impl Serialize for ThermalStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

/// Optional client metrics. Carries no identifiers.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Stats {
    #[serde(skip_serializing_if = "Option::is_none")]
    fps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    video_bitrate_kbps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rtt_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    loss_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thermal_status: Option<ThermalStatus>,
}

impl Stats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_fps(mut self, value: f64) -> Result<Self, InvalidValue> {
        if !(0.0..=120.0).contains(&value) {
            return Err(InvalidValue);
        }
        self.fps = Some(value);
        Ok(self)
    }

    pub fn with_video_bitrate_kbps(mut self, value: f64) -> Result<Self, InvalidValue> {
        if !(value.is_finite() && value >= 0.0) {
            return Err(InvalidValue);
        }
        self.video_bitrate_kbps = Some(value);
        Ok(self)
    }

    pub fn with_rtt_ms(mut self, value: f64) -> Result<Self, InvalidValue> {
        if !(value.is_finite() && value >= 0.0) {
            return Err(InvalidValue);
        }
        self.rtt_ms = Some(value);
        Ok(self)
    }

    pub fn with_loss_pct(mut self, value: f64) -> Result<Self, InvalidValue> {
        if !(0.0..=100.0).contains(&value) {
            return Err(InvalidValue);
        }
        self.loss_pct = Some(value);
        Ok(self)
    }

    pub fn with_thermal_status(mut self, value: ThermalStatus) -> Self {
        self.thermal_status = Some(value);
        self
    }
}

/// The session-close codes a `close` message may carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseCode {
    EndedByMobile,
    EndedByDesktop,
    Superseded,
    Timeout,
    MediaFailure,
    SecurityViolation,
    InternalError,
}

impl CloseCode {
    const ALL: [Self; 7] = [
        Self::EndedByMobile,
        Self::EndedByDesktop,
        Self::Superseded,
        Self::Timeout,
        Self::MediaFailure,
        Self::SecurityViolation,
        Self::InternalError,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::EndedByMobile => "ended_by_mobile",
            Self::EndedByDesktop => "ended_by_desktop",
            Self::Superseded => "superseded",
            Self::Timeout => "timeout",
            Self::MediaFailure => "media_failure",
            Self::SecurityViolation => "security_violation",
            Self::InternalError => "internal_error",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|candidate| candidate.as_str() == value)
    }
}

impl Serialize for CloseCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

/// The signaling-error codes an `error` message may carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidMessage,
    UnsupportedVersion,
    RateLimited,
    InternalError,
}

impl ErrorCode {
    const ALL: [Self; 4] = [
        Self::InvalidMessage,
        Self::UnsupportedVersion,
        Self::RateLimited,
        Self::InternalError,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidMessage => "invalid_message",
            Self::UnsupportedVersion => "unsupported_version",
            Self::RateLimited => "rate_limited",
            Self::InternalError => "internal_error",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|candidate| candidate.as_str() == value)
    }
}

impl Serialize for ErrorCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
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
        sdp: Sdp,
    },
    Answer {
        epoch: u64,
        sdp: Sdp,
    },
    Ice {
        epoch: u64,
        candidates: Candidates,
    },
    Stats(Stats),
    Close {
        code: CloseCode,
    },
    Error {
        code: ErrorCode,
        message: Option<ErrorText>,
    },
    Ping,
    Pong,
}

/// Failure while encoding a message into a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeError {
    /// The canonical frame exceeds [`MAX_SIGNALING_MESSAGE_BYTES`].
    FrameTooLarge,
}

impl SignalingMessage {
    /// Encodes the message in canonical field order. Optional fields that were
    /// never supplied are omitted.
    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        let encoded = serde_json::to_vec(&CanonicalFrame(self))
            .expect("a validated signaling message always serializes");
        if encoded.len() > MAX_SIGNALING_MESSAGE_BYTES {
            return Err(EncodeError::FrameTooLarge);
        }
        Ok(encoded)
    }
}

struct CanonicalFrame<'a>(&'a SignalingMessage);

struct HexBytes<'a>(&'a [u8]);

impl Serialize for HexBytes<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&encode_hex(self.0))
    }
}

impl Serialize for CanonicalFrame<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self.0 {
            SignalingMessage::Pair {
                token,
                dtls_fingerprint,
                limits,
            } => {
                let mut frame = serializer.serialize_struct("pair", 5)?;
                frame.serialize_field("version", &PROTOCOL_VERSION)?;
                frame.serialize_field("type", "pair")?;
                frame.serialize_field("token", &HexBytes(token))?;
                frame.serialize_field("dtls_fingerprint", &HexBytes(dtls_fingerprint))?;
                frame.serialize_field("limits", limits)?;
                frame.end()
            }
            SignalingMessage::Paired {
                session_id,
                epoch,
                dtls_fingerprint,
                accepted_limits,
            } => {
                let mut frame = serializer.serialize_struct("paired", 6)?;
                frame.serialize_field("version", &PROTOCOL_VERSION)?;
                frame.serialize_field("type", "paired")?;
                frame.serialize_field("session_id", &HexBytes(session_id))?;
                frame.serialize_field("epoch", epoch)?;
                frame.serialize_field("dtls_fingerprint", &HexBytes(dtls_fingerprint))?;
                frame.serialize_field("accepted_limits", accepted_limits)?;
                frame.end()
            }
            SignalingMessage::Offer { epoch, sdp } => {
                let mut frame = serializer.serialize_struct("offer", 4)?;
                frame.serialize_field("version", &PROTOCOL_VERSION)?;
                frame.serialize_field("type", "offer")?;
                frame.serialize_field("epoch", epoch)?;
                frame.serialize_field("sdp", sdp)?;
                frame.end()
            }
            SignalingMessage::Answer { epoch, sdp } => {
                let mut frame = serializer.serialize_struct("answer", 4)?;
                frame.serialize_field("version", &PROTOCOL_VERSION)?;
                frame.serialize_field("type", "answer")?;
                frame.serialize_field("epoch", epoch)?;
                frame.serialize_field("sdp", sdp)?;
                frame.end()
            }
            SignalingMessage::Ice { epoch, candidates } => {
                let mut frame = serializer.serialize_struct("ice", 4)?;
                frame.serialize_field("version", &PROTOCOL_VERSION)?;
                frame.serialize_field("type", "ice")?;
                frame.serialize_field("epoch", epoch)?;
                frame.serialize_field("candidates", candidates)?;
                frame.end()
            }
            SignalingMessage::Stats(stats) => {
                let mut frame = serializer.serialize_struct("stats", 7)?;
                frame.serialize_field("version", &PROTOCOL_VERSION)?;
                frame.serialize_field("type", "stats")?;
                if let Some(fps) = stats.fps {
                    frame.serialize_field("fps", &fps)?;
                }
                if let Some(video_bitrate_kbps) = stats.video_bitrate_kbps {
                    frame.serialize_field("video_bitrate_kbps", &video_bitrate_kbps)?;
                }
                if let Some(rtt_ms) = stats.rtt_ms {
                    frame.serialize_field("rtt_ms", &rtt_ms)?;
                }
                if let Some(loss_pct) = stats.loss_pct {
                    frame.serialize_field("loss_pct", &loss_pct)?;
                }
                if let Some(thermal_status) = stats.thermal_status {
                    frame.serialize_field("thermal_status", &thermal_status)?;
                }
                frame.end()
            }
            SignalingMessage::Close { code } => {
                let mut frame = serializer.serialize_struct("close", 3)?;
                frame.serialize_field("version", &PROTOCOL_VERSION)?;
                frame.serialize_field("type", "close")?;
                frame.serialize_field("code", code)?;
                frame.end()
            }
            SignalingMessage::Error { code, message } => {
                let mut frame = serializer.serialize_struct("error", 4)?;
                frame.serialize_field("version", &PROTOCOL_VERSION)?;
                frame.serialize_field("type", "error")?;
                frame.serialize_field("code", code)?;
                if let Some(message) = message {
                    frame.serialize_field("message", message)?;
                }
                frame.end()
            }
            SignalingMessage::Ping => {
                let mut frame = serializer.serialize_struct("ping", 2)?;
                frame.serialize_field("version", &PROTOCOL_VERSION)?;
                frame.serialize_field("type", "ping")?;
                frame.end()
            }
            SignalingMessage::Pong => {
                let mut frame = serializer.serialize_struct("pong", 2)?;
                frame.serialize_field("version", &PROTOCOL_VERSION)?;
                frame.serialize_field("type", "pong")?;
                frame.end()
            }
        }
    }
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
            let sdp = Sdp::new(parse_string(map, "sdp")?).map_err(|_| ParseError::invalid())?;
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
            let code =
                CloseCode::parse(parse_string(map, "code")?).ok_or_else(ParseError::invalid)?;
            Ok(SignalingMessage::Close { code })
        }
        "error" => {
            check_keys(map, &["version", "type", "code", "message"])?;
            let code =
                ErrorCode::parse(parse_string(map, "code")?).ok_or_else(ParseError::invalid)?;
            let message = match map.get("message") {
                Some(value) => {
                    let text = value.as_str().ok_or_else(ParseError::invalid)?;
                    Some(ErrorText::new(text).map_err(|_| ParseError::invalid())?)
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
    required(map, "type")?
        .as_str()
        .ok_or_else(ParseError::invalid)
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

fn parse_object(value: &Value) -> Result<&Map<String, Value>, ParseError> {
    value.as_object().ok_or_else(ParseError::invalid)
}

fn optional_u32(map: &Map<String, Value>, key: &str) -> Result<Option<u32>, ParseError> {
    match map.get(key) {
        None => Ok(None),
        Some(value) => {
            let number = value.as_u64().ok_or_else(ParseError::invalid)?;
            u32::try_from(number)
                .map(Some)
                .map_err(|_| ParseError::invalid())
        }
    }
}

fn optional_f64(map: &Map<String, Value>, key: &str) -> Result<Option<f64>, ParseError> {
    match map.get(key) {
        None => Ok(None),
        Some(value) => value.as_f64().map(Some).ok_or_else(ParseError::invalid),
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
    let mut limits = Limits::new();
    if let Some(width) = optional_u32(map, "max_width")? {
        limits = limits
            .with_max_width(width)
            .map_err(|_| ParseError::invalid())?;
    }
    if let Some(height) = optional_u32(map, "max_height")? {
        limits = limits
            .with_max_height(height)
            .map_err(|_| ParseError::invalid())?;
    }
    if let Some(frame_rate) = optional_u32(map, "max_frame_rate")? {
        limits = limits
            .with_max_frame_rate(frame_rate)
            .map_err(|_| ParseError::invalid())?;
    }
    if let Some(video_bitrate) = optional_u32(map, "max_video_bitrate_kbps")? {
        limits = limits
            .with_max_video_bitrate_kbps(video_bitrate)
            .map_err(|_| ParseError::invalid())?;
    }
    if let Some(audio_bitrate) = optional_u32(map, "max_audio_bitrate_kbps")? {
        limits = limits
            .with_max_audio_bitrate_kbps(audio_bitrate)
            .map_err(|_| ParseError::invalid())?;
    }
    Ok(limits)
}

fn parse_candidates(value: &Value) -> Result<Candidates, ParseError> {
    let items = value.as_array().ok_or_else(ParseError::invalid)?;
    let candidates = items
        .iter()
        .map(parse_candidate)
        .collect::<Result<Vec<_>, _>>()?;
    Candidates::new(candidates).map_err(|_| ParseError::invalid())
}

fn parse_candidate(value: &Value) -> Result<Candidate, ParseError> {
    let map = parse_object(value)?;
    check_keys(map, &["address", "port", "component", "protocol"])?;
    let address = parse_string(map, "address")?;
    let port = required(map, "port")?
        .as_u64()
        .and_then(|port| u16::try_from(port).ok())
        .ok_or_else(ParseError::invalid)?;
    let component = required(map, "component")?
        .as_u64()
        .and_then(IceComponent::parse)
        .ok_or_else(ParseError::invalid)?;
    let protocol = match map.get("protocol") {
        None => None,
        Some(value) => {
            if value.as_str() != Some(TransportProtocol::Udp.as_str()) {
                return Err(ParseError::invalid());
            }
            Some(TransportProtocol::Udp)
        }
    };
    Candidate::new(address, port, component, protocol).map_err(|_| ParseError::invalid())
}

fn parse_stats(map: &Map<String, Value>) -> Result<Stats, ParseError> {
    let mut stats = Stats::new();
    if let Some(fps) = optional_f64(map, "fps")? {
        stats = stats.with_fps(fps).map_err(|_| ParseError::invalid())?;
    }
    if let Some(video_bitrate) = optional_f64(map, "video_bitrate_kbps")? {
        stats = stats
            .with_video_bitrate_kbps(video_bitrate)
            .map_err(|_| ParseError::invalid())?;
    }
    if let Some(rtt) = optional_f64(map, "rtt_ms")? {
        stats = stats.with_rtt_ms(rtt).map_err(|_| ParseError::invalid())?;
    }
    if let Some(loss) = optional_f64(map, "loss_pct")? {
        stats = stats
            .with_loss_pct(loss)
            .map_err(|_| ParseError::invalid())?;
    }
    if let Some(value) = map.get("thermal_status") {
        let status = value
            .as_str()
            .and_then(ThermalStatus::parse)
            .ok_or_else(ParseError::invalid)?;
        stats = stats.with_thermal_status(status);
    }
    Ok(stats)
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
        assert!(parse(
            r#"{"version":1,"type":"pair","token":"0000000000000000000000000000000000000000000000000000000000000000","dtls_fingerprint":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","limits":{"max_width":1280,"zoom":2}}"#,
        )
        .is_err());
        assert!(parse(
            r#"{"version":1,"type":"pair","token":"0000000000000000000000000000000000000000000000000000000000000000","dtls_fingerprint":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","limits":{"max_width":true}}"#,
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
            r#"{"version":1,"type":"ice","epoch":1,"candidates":[{"address":"192.168.1.42","port":0,"component":1}]}"#,
            r#"{"version":1,"type":"ice","epoch":1,"candidates":[{"address":"192.168.1.42","port":65536,"component":1}]}"#,
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

    #[test]
    fn rejects_sdp_outside_the_declared_length() {
        assert!(parse(r#"{"version":1,"type":"offer","epoch":1,"sdp":""}"#).is_err());
        let oversized = format!(
            r#"{{"version":1,"type":"offer","epoch":1,"sdp":"{}"}}"#,
            "a".repeat(MAX_SDP_CHARS + 1)
        );
        assert!(parse(&oversized).is_err());
    }
}
