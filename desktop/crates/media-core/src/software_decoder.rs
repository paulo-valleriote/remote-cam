//! Software H.264 decode (OpenH264) producing NV12 frames. Linux/CI only.

use openh264::decoder::Decoder;
use openh264::formats::YUVSource;

use crate::decoder::{DecodeError, VideoDecoder};
use crate::nv12::{Nv12Frame, i420_to_nv12};

pub struct SoftwareH264Decoder {
    decoder: Decoder,
}

impl SoftwareH264Decoder {
    pub fn new() -> Result<Self, DecodeError> {
        Decoder::new()
            .map(|decoder| Self { decoder })
            .map_err(|error| DecodeError::Decoder(error.to_string()))
    }
}

impl VideoDecoder for SoftwareH264Decoder {
    fn decode(
        &mut self,
        access_unit: &[u8],
        rtp_timestamp: u32,
    ) -> Result<Option<Nv12Frame>, DecodeError> {
        let Some(picture) = self
            .decoder
            .decode(access_unit)
            .map_err(|error| DecodeError::Decoder(error.to_string()))?
        else {
            return Ok(None);
        };
        let (width, height) = picture.dimensions();
        let data = i420_to_nv12(
            width,
            height,
            (picture.y(), picture.u(), picture.v()),
            picture.strides(),
        );
        Nv12Frame::new(width as u32, height as u32, rtp_timestamp, data)
            .map(Some)
            .map_err(DecodeError::Frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openh264::encoder::Encoder;
    use openh264::formats::YUVBuffer;

    use crate::testing::{PICTURE_HEIGHT, PICTURE_WIDTH, synthetic_picture};

    fn encoded_picture(index: u32) -> Vec<u8> {
        let mut encoder = Encoder::new().expect("encoder");
        let picture = YUVBuffer::from_vec(synthetic_picture(index), PICTURE_WIDTH, PICTURE_HEIGHT);
        encoder.encode(&picture).expect("encode").to_vec()
    }

    #[test]
    fn an_encoded_picture_decodes_to_nv12_with_its_rtp_timestamp() {
        let mut decoder = SoftwareH264Decoder::new().expect("decoder");
        let frame = decoder
            .decode(&encoded_picture(3), 4_242)
            .expect("decode")
            .expect("picture");
        assert_eq!(
            (frame.width() as usize, frame.height() as usize),
            (PICTURE_WIDTH, PICTURE_HEIGHT)
        );
        assert_eq!(frame.rtp_timestamp(), 4_242);
        assert_eq!(frame.data().len(), PICTURE_WIDTH * PICTURE_HEIGHT * 3 / 2);
    }

    #[test]
    fn parameter_sets_alone_produce_no_picture() {
        let mut decoder = SoftwareH264Decoder::new().expect("decoder");
        let sps = [0, 0, 0, 1, 0x67, 0x42, 0xE0, 0x1F, 0xDA, 0x05, 0x07, 0xE8];
        assert!(matches!(decoder.decode(&sps, 1), Ok(None) | Err(_)));
    }
}
