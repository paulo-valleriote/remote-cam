//! QR display policy: private endpoints only, 120-second expiry, byte mode.
//!
//! The QR payload itself is defined by the shared contract; this module owns the
//! desktop side's decisions about what may be advertised and how the symbol is
//! rendered. It refuses to display a QR when no private endpoint exists, because
//! a QR that cannot be reached is worse than no QR.

use std::fmt;
use std::time::Duration;

use qrcode::QrCode;
use qrcode::bits::Bits;
use qrcode::types::{EcLevel, Version};
use remotecam_protocol::{Endpoint, QrEncodeError, QrPayload};

/// How long a displayed payload stays valid.
pub const QR_VALIDITY: Duration = Duration::from_secs(120);

/// Failure while rendering the symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QrRenderError {
    /// The payload did not encode.
    Payload(QrEncodeError),
    /// The payload does not fit any QR version at error-correction level M.
    TooLarge {
        /// The number of payload bytes that had to be encoded.
        payload_bytes: usize,
    },
    /// The QR library rejected the symbol.
    Library(String),
}

impl fmt::Display for QrRenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Payload(error) => write!(formatter, "payload rejected: {error:?}"),
            Self::TooLarge { payload_bytes } => {
                write!(
                    formatter,
                    "payload of {payload_bytes} bytes does not fit a QR symbol"
                )
            }
            Self::Library(error) => write!(formatter, "QR rendering failed: {error}"),
        }
    }
}

impl std::error::Error for QrRenderError {}

/// A rendered payload ready to display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrDisplay {
    /// The payload whose bytes are encoded in the symbol.
    pub payload: QrPayload,
    /// The encoded payload bytes, for the conformance check.
    pub bytes: Vec<u8>,
    /// An SVG rendering of the symbol.
    pub svg: String,
}

/// Filters candidate endpoints down to the private ones, deduplicated and sorted.
///
/// The order is stable and independent of the caller's input order, so two runs
/// with the same addresses produce the same QR.
pub fn select_private_endpoints(candidates: &[Endpoint]) -> Vec<Endpoint> {
    let mut selected: Vec<Endpoint> = candidates
        .iter()
        .filter(|endpoint| endpoint_is_private(endpoint))
        .cloned()
        .collect();
    selected.sort_by_key(|endpoint| (endpoint.family, endpoint.address));
    selected.dedup_by_key(|endpoint| (endpoint.family, endpoint.address));
    selected
}

fn endpoint_is_private(endpoint: &Endpoint) -> bool {
    match endpoint.family {
        4 => {
            endpoint.address[..12].iter().all(|byte| *byte == 0) && {
                let [first, second, _, _] = [
                    endpoint.address[12],
                    endpoint.address[13],
                    endpoint.address[14],
                    endpoint.address[15],
                ];
                first == 10
                    || (first == 172 && (16..=31).contains(&second))
                    || (first == 192 && second == 168)
            }
        }
        6 => {
            let first = endpoint.address[0];
            let second = endpoint.address[1];
            (first == 0xfe && (second & 0xc0) == 0x80) || (first & 0xfe) == 0xfc
        }
        _ => false,
    }
}

/// Builds a payload with a 120-second expiry.
///
/// Returns [`QrEncodeError::NoEndpoints`] when the filtered endpoint list is
/// empty: the caller must not display a QR in that case.
pub fn build_payload(
    session_id: [u8; 16],
    epoch: u32,
    pairing_token: [u8; 32],
    tls_fingerprint: [u8; 32],
    candidates: &[Endpoint],
    now_unix: u64,
) -> Result<QrPayload, QrEncodeError> {
    let endpoints = select_private_endpoints(candidates);
    if endpoints.is_empty() {
        return Err(QrEncodeError::NoEndpoints);
    }
    Ok(QrPayload {
        session_id,
        epoch,
        expires_at: now_unix.saturating_add(QR_VALIDITY.as_secs()),
        pairing_token,
        tls_fingerprint,
        capabilities: 0,
        endpoints,
    })
}

/// Builds and renders a payload in one step.
pub fn build_display(
    session_id: [u8; 16],
    epoch: u32,
    pairing_token: [u8; 32],
    tls_fingerprint: [u8; 32],
    candidates: &[Endpoint],
    now_unix: u64,
) -> Result<QrDisplay, QrRenderError> {
    let payload = build_payload(
        session_id,
        epoch,
        pairing_token,
        tls_fingerprint,
        candidates,
        now_unix,
    )
    .map_err(QrRenderError::Payload)?;
    render(&payload)
}

/// Renders a payload as an SVG symbol using byte mode at error-correction M.
pub fn render(payload: &QrPayload) -> Result<QrDisplay, QrRenderError> {
    let bytes = payload.encode().map_err(QrRenderError::Payload)?;
    let code = byte_mode_symbol(&bytes)?;
    let svg = code
        .render::<qrcode::render::svg::Color>()
        .quiet_zone(true)
        .min_dimensions(512, 512)
        .build();
    Ok(QrDisplay {
        payload: payload.clone(),
        bytes,
        svg,
    })
}

/// Encodes the bytes in byte mode, choosing the smallest version that fits.
fn byte_mode_symbol(bytes: &[u8]) -> Result<QrCode, QrRenderError> {
    for version in 1..=40 {
        let mut bits = Bits::new(Version::Normal(version));
        let Ok(()) = bits.push_byte_data(bytes) else {
            continue;
        };
        if let Ok(code) = QrCode::with_bits(bits, EcLevel::M) {
            return Ok(code);
        }
    }
    Err(QrRenderError::TooLarge {
        payload_bytes: bytes.len(),
    })
}
