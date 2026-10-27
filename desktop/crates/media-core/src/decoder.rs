//! The decoder seam of the Media Core: access units in, NV12 frames out.
//!
//! The Windows product decodes with Media Foundation (F3-07). The OpenH264
//! implementation in `software_decoder` is Linux/CI only and is not compiled
//! on Windows, so it is never distributed (ADR-0021).

use std::fmt;

use crate::nv12::{FrameError, Nv12Frame};

#[derive(Debug)]
pub enum DecodeError {
    Decoder(String),
    Frame(FrameError),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decoder(message) => write!(formatter, "h264 decoder error: {message}"),
            Self::Frame(error) => write!(formatter, "decoded frame rejected: {error}"),
        }
    }
}

impl std::error::Error for DecodeError {}

pub trait VideoDecoder: Send {
    /// Decodes one Annex-B access unit. `Ok(None)` means the decoder needs more
    /// data (for example parameter sets without a picture).
    fn decode(
        &mut self,
        access_unit: &[u8],
        rtp_timestamp: u32,
    ) -> Result<Option<Nv12Frame>, DecodeError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_describe_their_cause() {
        assert!(DecodeError::Decoder("x".into()).to_string().contains("x"));
        let frame = DecodeError::Frame(FrameError::InvalidDimensions {
            width: 1,
            height: 1,
        });
        assert!(frame.to_string().contains("1x1"));
    }
}
