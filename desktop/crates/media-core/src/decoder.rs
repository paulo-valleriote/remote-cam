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
