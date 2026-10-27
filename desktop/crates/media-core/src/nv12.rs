//! NV12 frames: a full-resolution Y plane followed by one interleaved U,V plane
//! at half resolution in both directions, with a tight stride.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    /// NV12 subsamples chroma by two, so both dimensions must be even and non-zero.
    InvalidDimensions { width: u32, height: u32 },
    /// The buffer does not hold exactly `width * height * 3 / 2` bytes.
    InvalidLength { expected: usize, actual: usize },
}

impl fmt::Display for FrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDimensions { width, height } => {
                write!(formatter, "invalid NV12 dimensions {width}x{height}")
            }
            Self::InvalidLength { expected, actual } => {
                write!(
                    formatter,
                    "NV12 buffer is {actual} bytes, expected {expected}"
                )
            }
        }
    }
}

impl std::error::Error for FrameError {}

/// A decoded video frame in NV12.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nv12Frame {
    width: u32,
    height: u32,
    rtp_timestamp: u32,
    data: Vec<u8>,
}

impl Nv12Frame {
    pub fn new(
        width: u32,
        height: u32,
        rtp_timestamp: u32,
        data: Vec<u8>,
    ) -> Result<Self, FrameError> {
        if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return Err(FrameError::InvalidDimensions { width, height });
        }
        let expected = width as usize * height as usize * 3 / 2;
        if data.len() != expected {
            return Err(FrameError::InvalidLength {
                expected,
                actual: data.len(),
            });
        }
        Ok(Self {
            width,
            height,
            rtp_timestamp,
            data,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// The 90 kHz RTP timestamp of the access unit this frame was decoded from.
    pub fn rtp_timestamp(&self) -> u32 {
        self.rtp_timestamp
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn luma(&self) -> &[u8] {
        &self.data[..self.width as usize * self.height as usize]
    }

    pub fn chroma(&self) -> &[u8] {
        &self.data[self.width as usize * self.height as usize..]
    }
}

/// Repacks planar I420 (with arbitrary strides) into a tight NV12 buffer.
/// U always lands on the even byte of a chroma row and V on the odd one.
pub fn i420_to_nv12(
    width: usize,
    height: usize,
    planes: (&[u8], &[u8], &[u8]),
    strides: (usize, usize, usize),
) -> Vec<u8> {
    let (y_plane, u_plane, v_plane) = planes;
    let (y_stride, u_stride, v_stride) = strides;
    let luma_len = width * height;
    let mut output = vec![0_u8; luma_len + luma_len / 2];

    for row in 0..height {
        output[row * width..(row + 1) * width]
            .copy_from_slice(&y_plane[row * y_stride..row * y_stride + width]);
    }
    for row in 0..height / 2 {
        let destination = luma_len + row * width;
        for column in 0..width / 2 {
            output[destination + column * 2] = u_plane[row * u_stride + column];
            output[destination + column * 2 + 1] = v_plane[row * v_stride + column];
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interleaves_chroma_as_u_then_v() {
        let y: Vec<u8> = (0..16).collect();
        let u = [100, 101, 102, 103];
        let v = [200, 201, 202, 203];
        let nv12 = i420_to_nv12(4, 4, (&y, &u, &v), (4, 2, 2));
        assert_eq!(&nv12[..16], y.as_slice());
        assert_eq!(&nv12[16..], &[100, 200, 101, 201, 102, 202, 103, 203]);
    }

    #[test]
    fn drops_stride_padding_from_every_plane() {
        // Strides are wider than the visible area; the padding bytes are 0xEE.
        let mut y = Vec::new();
        for row in 0..2_u8 {
            y.extend_from_slice(&[row * 2, row * 2 + 1, 0xEE, 0xEE]);
        }
        let u = [10, 0xEE, 0xEE];
        let v = [20, 0xEE, 0xEE];
        let nv12 = i420_to_nv12(2, 2, (&y, &u, &v), (4, 3, 3));
        assert_eq!(nv12, vec![0, 1, 2, 3, 10, 20]);
    }

    #[test]
    fn frame_accessors_split_the_planes() {
        let frame = Nv12Frame::new(2, 2, 9, vec![1, 2, 3, 4, 5, 6]).expect("frame");
        assert_eq!((frame.width(), frame.height()), (2, 2));
        assert_eq!(frame.rtp_timestamp(), 9);
        assert_eq!(frame.luma(), &[1, 2, 3, 4]);
        assert_eq!(frame.chroma(), &[5, 6]);
        assert_eq!(frame.data().len(), 6);
    }

    #[test]
    fn odd_zero_or_mismatched_frames_are_refused() {
        assert_eq!(
            Nv12Frame::new(3, 2, 0, vec![0; 9]),
            Err(FrameError::InvalidDimensions {
                width: 3,
                height: 2
            })
        );
        assert_eq!(
            Nv12Frame::new(0, 0, 0, Vec::new()),
            Err(FrameError::InvalidDimensions {
                width: 0,
                height: 0
            })
        );
        let error = Nv12Frame::new(2, 2, 0, vec![0; 5]).expect_err("short buffer");
        assert_eq!(
            error,
            FrameError::InvalidLength {
                expected: 6,
                actual: 5
            }
        );
        assert!(error.to_string().contains("expected 6"));
        assert!(
            FrameError::InvalidDimensions {
                width: 1,
                height: 1
            }
            .to_string()
            .contains("1x1")
        );
    }
}
