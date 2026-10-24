//! Software H.264 decode fallback (T070).
//!
//! **NON-PRODUCTION** — this adapter exists so the Linux CI can exercise the
//! handoff and the stall path without Media Foundation. It is **never** cited
//! for the latency gate: `openh264` cannot decode every high profile and the
//! gate requires hardware decode (PC-1). Its output is converted to the same
//! NV12 layout the hardware adapter produces, so the downstream code is shared.

use openh264::decoder::Decoder;
use openh264::formats::YUVSource;
use remotecam_injection::{Frame, FrameError};

/// Errors raised by the software adapter.
#[derive(Debug)]
pub enum SoftwareDecodeError {
    Decoder(String),
    Frame(FrameError),
}

impl std::fmt::Display for SoftwareDecodeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Decoder(message) => write!(formatter, "openh264 error: {message}"),
            Self::Frame(error) => write!(formatter, "frame error: {error}"),
        }
    }
}

impl std::error::Error for SoftwareDecodeError {}

/// Convert planar I420 to NV12, copying each row into a tight stride.
///
/// The chroma planes are interleaved as U,V. A swapped conversion would show as
/// green/magenta and is impossible by construction: the V plane is always
/// written to the odd byte of the chroma row.
pub fn i420_to_nv12(
    width: usize,
    height: usize,
    y_plane: &[u8],
    y_stride: usize,
    u_plane: &[u8],
    v_plane: &[u8],
    uv_stride: usize,
) -> Result<Vec<u8>, FrameError> {
    let luma_len = width * height;
    let chroma_len = luma_len / 2;
    let mut output = vec![0_u8; luma_len + chroma_len];

    for row in 0..height {
        let source = row * y_stride;
        let destination = row * width;
        output[destination..destination + width].copy_from_slice(&y_plane[source..source + width]);
    }

    for row in 0..height / 2 {
        let source = row * uv_stride;
        let destination = luma_len + row * width;
        for column in 0..width / 2 {
            output[destination + column * 2] = u_plane[source + column];
            output[destination + column * 2 + 1] = v_plane[source + column];
        }
    }

    Ok(output)
}

/// Thin wrapper around `openh264`'s decoder that emits [`Frame`]s.
pub struct SoftwareH264Decoder {
    decoder: Decoder,
    /// Marks every frame as non-product in the evidence.
    pub citation_allowed: bool,
}

impl SoftwareH264Decoder {
    pub fn new() -> Result<Self, SoftwareDecodeError> {
        let decoder =
            Decoder::new().map_err(|error| SoftwareDecodeError::Decoder(error.to_string()))?;
        Ok(Self {
            decoder,
            citation_allowed: false,
        })
    }

    /// Decode one Annex-B access unit.
    pub fn decode(
        &mut self,
        access_unit: &[u8],
        timestamp_ns: u64,
    ) -> Result<Option<Frame>, SoftwareDecodeError> {
        let decoded = self
            .decoder
            .decode(access_unit)
            .map_err(|error| SoftwareDecodeError::Decoder(error.to_string()))?;
        let Some(decoded) = decoded else {
            return Ok(None);
        };
        let (width, height) = decoded.dimensions();
        if !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return Err(SoftwareDecodeError::Frame(FrameError::OddDimension {
                width: width as u32,
                height: height as u32,
            }));
        }
        let (y_stride, uv_stride, _) = decoded.strides();
        let data = i420_to_nv12(
            width,
            height,
            decoded.y(),
            y_stride,
            decoded.u(),
            decoded.v(),
            uv_stride,
        )
        .map_err(SoftwareDecodeError::Frame)?;
        Frame::new_nv12(width as u32, height as u32, width, timestamp_ns, data)
            .map(Some)
            .map_err(SoftwareDecodeError::Frame)
    }
}
