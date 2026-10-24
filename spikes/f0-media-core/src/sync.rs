//! A/V synchronisation from the in-band source capture timestamp (T073).
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike.
//!
//! Both streams carry the source capture instant in an RTP header extension, so
//! the receiver can align playout against the sender's timeline rather than
//! against packet arrival. The per-stream playout delays produced here are the
//! inputs the drift sampler records.

/// Stream identity used by the synchroniser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    Audio,
    Video,
}

/// RTP one-byte header extension profile (RFC 8285).
pub const ONE_BYTE_EXTENSION_PROFILE: u16 = 0xBEDE;

/// Parsed RTP envelope needed to reach the extension payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtpHeader {
    pub sequence: u16,
    pub timestamp: u32,
    pub ssrc: u32,
    pub payload_type: u8,
    pub extension_profile: Option<u16>,
    /// Offset of the extension payload when an extension is present.
    pub extension_offset: usize,
    pub payload_offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtpParseError {
    TooShort,
    UnsupportedVersion,
    TruncatedExtension,
}

impl std::fmt::Display for RtpParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooShort => write!(formatter, "RTP packet is shorter than its header"),
            Self::UnsupportedVersion => write!(formatter, "RTP version is not 2"),
            Self::TruncatedExtension => write!(formatter, "RTP header extension is truncated"),
        }
    }
}

impl std::error::Error for RtpParseError {}

/// Parse the fixed RTP header and the extension envelope.
pub fn parse_rtp_header(packet: &[u8]) -> Result<RtpHeader, RtpParseError> {
    if packet.len() < 12 {
        return Err(RtpParseError::TooShort);
    }
    if packet[0] >> 6 != 2 {
        return Err(RtpParseError::UnsupportedVersion);
    }
    let padding = packet[0] & 0x20 != 0;
    let extension = packet[0] & 0x10 != 0;
    let csrc_count = (packet[0] & 0x0F) as usize;
    let mut offset = 12 + csrc_count * 4;
    if offset > packet.len() {
        return Err(RtpParseError::TooShort);
    }

    let mut extension_profile = None;
    let mut extension_offset = offset;
    if extension {
        if offset + 4 > packet.len() {
            return Err(RtpParseError::TruncatedExtension);
        }
        extension_profile = Some(u16::from_be_bytes([packet[offset], packet[offset + 1]]));
        let words = u16::from_be_bytes([packet[offset + 2], packet[offset + 3]]) as usize;
        offset += 4;
        extension_offset = offset;
        offset += words * 4;
        if offset > packet.len() {
            return Err(RtpParseError::TruncatedExtension);
        }
    }

    let payload_offset = if padding && offset < packet.len() {
        let padding_len = *packet.last().unwrap_or(&0) as usize;
        offset.max(packet.len().saturating_sub(padding_len))
    } else {
        offset
    };

    Ok(RtpHeader {
        sequence: u16::from_be_bytes([packet[2], packet[3]]),
        timestamp: u32::from_be_bytes([packet[4], packet[5], packet[6], packet[7]]),
        ssrc: u32::from_be_bytes([packet[8], packet[9], packet[10], packet[11]]),
        payload_type: packet[1] & 0x7F,
        extension_profile,
        extension_offset,
        payload_offset,
    })
}

/// Read an 8-byte source-timestamp extension by its one-byte id.
///
/// Returns nanoseconds since the Unix epoch, as written by the synthetic source
/// and by the mobile capture path.
pub fn read_source_timestamp_ns(
    packet: &[u8],
    header: &RtpHeader,
    extension_id: u8,
) -> Option<u64> {
    if header.extension_profile != Some(ONE_BYTE_EXTENSION_PROFILE) {
        return None;
    }
    let payload_end = header.payload_offset.min(packet.len());
    let mut cursor = header.extension_offset;
    while cursor < payload_end {
        let byte = packet[cursor];
        if byte == 0 {
            cursor += 1;
            continue;
        }
        let id = byte >> 4;
        let length = (byte & 0x0F) as usize + 1;
        cursor += 1;
        if cursor + length > payload_end {
            return None;
        }
        if id == extension_id && length == 8 {
            let mut value = [0_u8; 8];
            value.copy_from_slice(&packet[cursor..cursor + 8]);
            return Some(u64::from_be_bytes(value));
        }
        cursor += length;
    }
    None
}

/// Per-stream observation handed to the synchroniser.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StreamObservation {
    /// Source capture instant carried in the RTP header extension, nanoseconds.
    pub source_capture_ns: u64,
    /// Local arrival instant, nanoseconds on the receiver's monotonic clock.
    pub arrival_ns: u64,
}

/// Playout delays that cancel the difference between the two transit times.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayoutDelays {
    pub audio_delay_ms: f64,
    pub video_delay_ms: f64,
}

/// Synchroniser for one audio and one video stream.
#[derive(Debug, Clone)]
pub struct AvSync {
    audio: Option<StreamObservation>,
    video: Option<StreamObservation>,
    base_delay_ms: f64,
    max_correction_ms: f64,
}

impl AvSync {
    pub fn new(base_delay_ms: f64, max_correction_ms: f64) -> Self {
        Self {
            audio: None,
            video: None,
            base_delay_ms,
            max_correction_ms,
        }
    }

    pub fn observe(&mut self, stream: StreamKind, observation: StreamObservation) {
        match stream {
            StreamKind::Audio => self.audio = Some(observation),
            StreamKind::Video => self.video = Some(observation),
        }
    }

    /// Current playout delays, or `None` until both streams have been observed.
    ///
    /// The delay difference cancels the transit-time difference:
    /// `d_audio - d_video = transit_video - transit_audio`. The correction is
    /// clamped, so an implausible marker cannot stall one stream forever.
    pub fn playout_delays(&self) -> Option<PlayoutDelays> {
        let (audio, video) = (self.audio?, self.video?);
        let transit_audio = audio.arrival_ns as f64 - audio.source_capture_ns as f64;
        let transit_video = video.arrival_ns as f64 - video.source_capture_ns as f64;
        let correction_ms = ((transit_video - transit_audio) / 1_000_000.0)
            .clamp(-self.max_correction_ms, self.max_correction_ms);
        let audio_delay = (self.base_delay_ms + correction_ms).max(0.0);
        Some(PlayoutDelays {
            audio_delay_ms: audio_delay,
            video_delay_ms: self.base_delay_ms.max(0.0),
        })
    }

    /// Source-time difference between the matched audio and video items, ms.
    pub fn source_skew_ms(&self) -> Option<f64> {
        let (audio, video) = (self.audio?, self.video?);
        Some((audio.source_capture_ns as f64 - video.source_capture_ns as f64) / 1_000_000.0)
    }

    pub fn reset(&mut self) {
        self.audio = None;
        self.video = None;
    }
}

impl Default for AvSync {
    fn default() -> Self {
        Self::new(40.0, 200.0)
    }
}
