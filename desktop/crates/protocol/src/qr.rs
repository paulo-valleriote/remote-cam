//! QR payload version 1: the compact binary payload described by
//! `protocol/qr-payload.md`.
//!
//! The payload is encoded and decoded as raw bytes. It is never treated as
//! UTF-8 text: the QR decoder must return the byte sequence unchanged.

use std::fmt;

use crate::address::{format_address, is_private_ipv4, is_private_ipv6, parse_private_address};
use crate::reason::ReasonCode;

/// ASCII magic at offset 0.
pub const MAGIC: &[u8; 4] = b"RCM1";
/// Payload version supported by this implementation.
pub const SUPPORTED_VERSION: u8 = 1;
/// Fixed header size before the endpoints block.
pub const HEADER_BYTES: usize = 102;
/// Size of one endpoint record.
pub const ENDPOINT_BYTES: usize = 19;
/// Maximum encoded payload size.
pub const MAX_PAYLOAD_BYTES: usize = 512;
/// Capability bits reserved in version 1.
const CAPABILITY_RESERVED_MASK: u16 = 0xffc0;

/// One endpoint record: family, 16-byte address and TCP port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// `4` or `6`.
    pub family: u8,
    /// Network-order address. The first twelve bytes are zero for IPv4.
    pub address: [u8; 16],
    /// TCP port of the ephemeral TLS listener.
    pub port: u16,
}

impl Endpoint {
    /// Builds an endpoint from a textual private address.
    pub fn from_address(family: u8, address: &str, port: u16) -> Option<Self> {
        let packed = parse_private_address(address)?;
        let matches_family = match family {
            4 => packed[..12].iter().all(|byte| *byte == 0),
            6 => true,
            _ => false,
        };
        matches_family.then_some(Self {
            family,
            address: packed,
            port,
        })
    }

    /// The textual form of the address.
    pub fn address_string(&self) -> Option<String> {
        format_address(self.family, &self.address)
    }

    /// The stable ordering used by the encoder: family first, then address bytes.
    fn sort_key(&self) -> (u8, [u8; 16]) {
        (self.family, self.address)
    }

    fn encode(&self) -> [u8; ENDPOINT_BYTES] {
        let mut record = [0_u8; ENDPOINT_BYTES];
        record[0] = self.family;
        record[1..17].copy_from_slice(&self.address);
        record[17..19].copy_from_slice(&self.port.to_be_bytes());
        record
    }

    fn decode(record: &[u8]) -> Result<Self, ReasonCode> {
        debug_assert_eq!(record.len(), ENDPOINT_BYTES);
        let family = record[0];
        let mut address = [0_u8; 16];
        address.copy_from_slice(&record[1..17]);
        let port = u16::from_be_bytes([record[17], record[18]]);
        match family {
            4 => {
                if address[..12].iter().any(|byte| *byte != 0)
                    || !is_private_ipv4([address[12], address[13], address[14], address[15]])
                {
                    return Err(ReasonCode::NoPrivateEndpoint);
                }
            }
            6 => {
                if !is_private_ipv6(&address) {
                    return Err(ReasonCode::NoPrivateEndpoint);
                }
            }
            _ => return Err(ReasonCode::InvalidQrPayload),
        }
        Ok(Self {
            family,
            address,
            port,
        })
    }
}

/// A decoded version 1 QR payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrPayload {
    /// Random 128-bit session identifier.
    pub session_id: [u8; 16],
    /// Monotonic session generation, strictly increasing across sessions.
    pub epoch: u32,
    /// Absolute expiry in seconds since the Unix epoch.
    pub expires_at: u64,
    /// 256-bit single-use pairing token.
    pub pairing_token: [u8; 32],
    /// SHA-256 of the DER-encoded leaf certificate.
    pub tls_fingerprint: [u8; 32],
    /// Capability bitmask.
    pub capabilities: u16,
    /// Advertised endpoints.
    pub endpoints: Vec<Endpoint>,
}

/// Failure while encoding a payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QrEncodeError {
    /// No endpoint was supplied; a desktop without a private address must not
    /// display a QR.
    NoEndpoints,
    /// The encoded payload would exceed [`MAX_PAYLOAD_BYTES`].
    TooLarge {
        /// Size the payload would have had.
        encoded_bytes: usize,
    },
    /// An endpoint address is not private or does not match its family.
    InvalidEndpoint,
}

/// Failure while decoding a payload. Carries the closed reason code the mobile
/// reports to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrDecodeError {
    /// Closed reason code from the contract.
    pub code: ReasonCode,
}

impl fmt::Display for QrEncodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoEndpoints => formatter.write_str("no private endpoints were supplied"),
            Self::TooLarge { encoded_bytes } => write!(
                formatter,
                "encoded payload would use {encoded_bytes} bytes, exceeding the {MAX_PAYLOAD_BYTES} byte limit"
            ),
            Self::InvalidEndpoint => formatter
                .write_str("one or more endpoints are not private or do not match the family"),
        }
    }
}

impl std::error::Error for QrEncodeError {}

impl fmt::Display for QrDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.code.as_str())
    }
}

impl std::error::Error for QrDecodeError {}

impl QrDecodeError {
    fn new(code: ReasonCode) -> Self {
        Self { code }
    }
}

impl QrPayload {
    /// Encodes the payload, sorting endpoints into the stable order required by
    /// the contract.
    pub fn encode(&self) -> Result<Vec<u8>, QrEncodeError> {
        if self.endpoints.is_empty() {
            return Err(QrEncodeError::NoEndpoints);
        }
        for endpoint in &self.endpoints {
            let family_matches = match endpoint.family {
                4 => endpoint.address[..12].iter().all(|byte| *byte == 0),
                6 => true,
                _ => false,
            };
            let address_private = match endpoint.family {
                4 => is_private_ipv4([
                    endpoint.address[12],
                    endpoint.address[13],
                    endpoint.address[14],
                    endpoint.address[15],
                ]),
                6 => is_private_ipv6(&endpoint.address),
                _ => false,
            };
            if !family_matches || !address_private {
                return Err(QrEncodeError::InvalidEndpoint);
            }
        }

        let mut endpoints: Vec<&Endpoint> = self.endpoints.iter().collect();
        endpoints.sort_by_key(|endpoint| endpoint.sort_key());

        let endpoints_len = endpoints.len() * ENDPOINT_BYTES;
        let encoded_bytes = HEADER_BYTES + endpoints_len;
        if encoded_bytes > MAX_PAYLOAD_BYTES {
            return Err(QrEncodeError::TooLarge { encoded_bytes });
        }

        let mut output = Vec::with_capacity(encoded_bytes);
        output.extend_from_slice(MAGIC);
        output.push(SUPPORTED_VERSION);
        output.push(0);
        output.extend_from_slice(&(endpoints_len as u16).to_be_bytes());
        output.extend_from_slice(&self.session_id);
        output.extend_from_slice(&self.epoch.to_be_bytes());
        output.extend_from_slice(&self.expires_at.to_be_bytes());
        output.extend_from_slice(&self.pairing_token);
        output.extend_from_slice(&self.tls_fingerprint);
        output.extend_from_slice(&self.capabilities.to_be_bytes());
        for endpoint in endpoints {
            output.extend_from_slice(&endpoint.encode());
        }
        Ok(output)
    }

    /// Decodes a payload, applying validation rules Q-1…Q-8 in order.
    pub fn decode(input: &[u8], now_unix: u64) -> Result<Self, QrDecodeError> {
        if input.len() < HEADER_BYTES {
            return Err(QrDecodeError::new(ReasonCode::InvalidQrPayload));
        }
        if &input[..4] != MAGIC {
            return Err(QrDecodeError::new(ReasonCode::InvalidQrPayload));
        }
        if input[4] != SUPPORTED_VERSION {
            return Err(QrDecodeError::new(ReasonCode::UnsupportedQrVersion));
        }
        if input[5] != 0 {
            return Err(QrDecodeError::new(ReasonCode::InvalidQrPayload));
        }

        let capabilities = u16::from_be_bytes([input[100], input[101]]);
        if capabilities & CAPABILITY_RESERVED_MASK != 0 {
            return Err(QrDecodeError::new(ReasonCode::InvalidQrPayload));
        }

        let endpoints_len = usize::from(u16::from_be_bytes([input[6], input[7]]));
        let expected_len = HEADER_BYTES + endpoints_len;
        if input.len() != expected_len {
            return Err(QrDecodeError::new(ReasonCode::InvalidQrPayload));
        }
        if endpoints_len == 0 || !endpoints_len.is_multiple_of(ENDPOINT_BYTES) {
            return Err(QrDecodeError::new(ReasonCode::InvalidQrPayload));
        }

        let (records, remainder) = input[HEADER_BYTES..].as_chunks::<ENDPOINT_BYTES>();
        if !remainder.is_empty() {
            return Err(QrDecodeError::new(ReasonCode::InvalidQrPayload));
        }
        let mut endpoints = Vec::with_capacity(records.len());
        for record in records {
            endpoints.push(Endpoint::decode(record).map_err(QrDecodeError::new)?);
        }

        let expires_at = u64::from_be_bytes(input[28..36].try_into().expect("eight bytes"));
        if expires_at <= now_unix {
            return Err(QrDecodeError::new(ReasonCode::QrExpired));
        }

        let mut session_id = [0_u8; 16];
        session_id.copy_from_slice(&input[8..24]);
        let mut pairing_token = [0_u8; 32];
        pairing_token.copy_from_slice(&input[36..68]);
        let mut tls_fingerprint = [0_u8; 32];
        tls_fingerprint.copy_from_slice(&input[68..100]);

        Ok(Self {
            session_id,
            epoch: u32::from_be_bytes(input[24..28].try_into().expect("four bytes")),
            expires_at,
            pairing_token,
            tls_fingerprint,
            capabilities,
            endpoints,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> QrPayload {
        QrPayload {
            session_id: [1; 16],
            epoch: 7,
            expires_at: 2_000_000_000,
            pairing_token: [2; 32],
            tls_fingerprint: [3; 32],
            capabilities: 0x0011,
            endpoints: vec![Endpoint::from_address(4, "192.168.1.42", 51820).unwrap()],
        }
    }

    #[test]
    fn round_trips_a_minimal_payload() {
        let encoded = payload().encode().unwrap();
        assert_eq!(encoded.len(), HEADER_BYTES + ENDPOINT_BYTES);
        let decoded = QrPayload::decode(&encoded, 1_900_000_000).unwrap();
        assert_eq!(decoded, payload());
    }

    #[test]
    fn endpoint_order_is_stable_and_independent_of_input_order() {
        let mut first = payload();
        first.endpoints = vec![
            Endpoint::from_address(6, "fe80::1", 3).unwrap(),
            Endpoint::from_address(4, "10.0.0.2", 2).unwrap(),
            Endpoint::from_address(4, "10.0.0.1", 1).unwrap(),
        ];
        let mut second = first.clone();
        second.endpoints.reverse();
        assert_eq!(first.encode().unwrap(), second.encode().unwrap());
    }

    #[test]
    fn rejects_payloads_that_exceed_the_size_bound() {
        let mut oversized = payload();
        oversized.endpoints = (0..22)
            .map(|index| Endpoint::from_address(4, "10.0.0.1", index).unwrap())
            .collect();
        assert_eq!(
            oversized.encode(),
            Err(QrEncodeError::TooLarge {
                encoded_bytes: HEADER_BYTES + 22 * ENDPOINT_BYTES
            })
        );
    }

    #[test]
    fn rejects_missing_or_public_endpoints_on_encode() {
        let mut empty = payload();
        empty.endpoints.clear();
        assert_eq!(empty.encode(), Err(QrEncodeError::NoEndpoints));

        let mut public = payload();
        public.endpoints = vec![Endpoint {
            family: 4,
            address: [0; 16],
            port: 1,
        }];
        assert_eq!(public.encode(), Err(QrEncodeError::InvalidEndpoint));
    }

    #[test]
    fn rejects_a_non_private_decoded_endpoint() {
        let mut encoded = payload().encode().unwrap();
        encoded[HEADER_BYTES + 1..HEADER_BYTES + 17].copy_from_slice(&[0; 16]);
        encoded[HEADER_BYTES + 13..HEADER_BYTES + 17].copy_from_slice(&[8, 8, 8, 8]);
        assert_eq!(
            QrPayload::decode(&encoded, 1_900_000_000),
            Err(QrDecodeError::new(ReasonCode::NoPrivateEndpoint))
        );
    }

    #[test]
    fn rejects_expired_payloads_at_the_boundary() {
        let encoded = payload().encode().unwrap();
        assert_eq!(
            QrPayload::decode(&encoded, 2_000_000_000),
            Err(QrDecodeError::new(ReasonCode::QrExpired))
        );
        assert!(QrPayload::decode(&encoded, 1_999_999_999).is_ok());
    }

    #[test]
    fn rejects_a_payload_shorter_than_the_header() {
        assert_eq!(
            QrPayload::decode(&[0; 10], 0),
            Err(QrDecodeError::new(ReasonCode::InvalidQrPayload))
        );
    }

    #[test]
    fn endpoint_text_round_trips() {
        let endpoint = Endpoint::from_address(6, "fe80::1", 1).unwrap();
        assert_eq!(endpoint.address_string().as_deref(), Some("fe80::1"));
        assert_eq!(Endpoint::from_address(5, "10.0.0.1", 1), None);
        assert_eq!(Endpoint::from_address(4, "fe80::1", 1), None);
    }
}
