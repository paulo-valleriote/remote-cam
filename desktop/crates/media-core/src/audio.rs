use std::fmt;

use opus::{Channels, Decoder};

pub const SAMPLE_RATE_HZ: u32 = 48_000;
pub const FRAME_SAMPLES: usize = 960;
const MAX_FRAME_SAMPLES: usize = 5_760;

#[derive(Debug)]
pub enum AudioError {
    Decoder(String),
    EmptyPacket,
}

impl fmt::Display for AudioError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decoder(message) => write!(formatter, "opus decoder error: {message}"),
            Self::EmptyPacket => write!(formatter, "empty opus packet"),
        }
    }
}

impl std::error::Error for AudioError {}

fn decoder_error(error: opus::Error) -> AudioError {
    AudioError::Decoder(error.to_string())
}

/// Opus 48 kHz mono to signed 16-bit PCM. Knows nothing about the output device.
pub struct OpusDecoder {
    inner: Decoder,
    last_frame_samples: usize,
    decoded_any: bool,
}

impl OpusDecoder {
    pub fn new() -> Result<Self, AudioError> {
        Ok(Self {
            inner: Decoder::new(SAMPLE_RATE_HZ, Channels::Mono).map_err(decoder_error)?,
            last_frame_samples: FRAME_SAMPLES,
            decoded_any: false,
        })
    }

    pub fn decode(&mut self, packet: &[u8]) -> Result<Vec<i16>, AudioError> {
        if packet.is_empty() {
            // libopus reads an empty payload as a loss and conceals silently; loss
            // enters only through `conceal`.
            return Err(AudioError::EmptyPacket);
        }
        let mut pcm = vec![0_i16; MAX_FRAME_SAMPLES];
        let samples = self
            .inner
            .decode(packet, &mut pcm, false)
            .map_err(decoder_error)?;
        pcm.truncate(samples);
        self.last_frame_samples = samples;
        self.decoded_any = true;
        Ok(pcm)
    }

    /// Packet loss concealment: extrapolates the previous audio for one frame.
    pub fn conceal(&mut self) -> Result<Vec<i16>, AudioError> {
        if !self.decoded_any {
            return Ok(vec![0; self.last_frame_samples]);
        }
        let mut pcm = vec![0_i16; self.last_frame_samples];
        let samples = self
            .inner
            .decode(&[], &mut pcm, false)
            .map_err(decoder_error)?;
        pcm.truncate(samples);
        Ok(pcm)
    }
}
