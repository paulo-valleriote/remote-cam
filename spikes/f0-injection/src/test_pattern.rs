//! Test-pattern producer for the virtual camera handoff.
//!
//! Each frame carries its own frame number as a machine-readable binary marker
//! drawn into the luma plane. Two copies exist — top-left and bottom-right — so
//! that a consumer which crops the preview still leaves one readable copy.
//! Duplicate frames, dropped frames and presentation stalls are therefore
//! observable in the consumer's own capture instead of being inferred from
//! timestamps alone.

use crate::ring_buffer::{Frame, FrameError};

/// Marker bit count: a 16-bit frame counter wraps every 65 536 frames.
pub const MARKER_BITS: usize = 16;
/// Fixed sync prefix. A cropped or blanked marker fails the sync and is
/// rejected instead of being misread as frame zero.
pub const MARKER_SYNC: u32 = 0b1010;
/// Bits in the sync prefix.
pub const MARKER_SYNC_BITS: usize = 4;
/// Total blocks drawn per marker.
pub const MARKER_TOTAL_BITS: usize = MARKER_SYNC_BITS + MARKER_BITS;
/// Margin between the marker and the frame edge, in pixels.
pub const MARKER_MARGIN: u32 = 8;
/// Preferred marker block edge in pixels.
pub const MARKER_BLOCK: u32 = 40;
/// Luma value treated as a set bit.
const WHITE: u8 = 0xF0;
/// Luma value treated as a clear bit.
const BLACK: u8 = 0x10;
/// Neutral NV12 chroma.
const NEUTRAL_CHROMA: u8 = 0x80;

/// Marker geometry for one frame size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkerLayout {
    pub block: u32,
    pub margin: u32,
}

impl MarkerLayout {
    /// Fit the marker into the frame, shrinking the block size if necessary.
    pub fn for_frame(width: u32, height: u32) -> Result<Self, FrameError> {
        if width < 2 * MARKER_MARGIN || height < 2 * MARKER_MARGIN + MARKER_BLOCK {
            return Err(FrameError::MarkerDoesNotFit { width, height });
        }
        let available = width - 2 * MARKER_MARGIN;
        let block = (available / MARKER_TOTAL_BITS as u32).clamp(1, MARKER_BLOCK);
        Ok(Self {
            block,
            margin: MARKER_MARGIN,
        })
    }

    pub fn marker_width(&self) -> u32 {
        self.block * MARKER_TOTAL_BITS as u32
    }
}

/// Render one NV12 test frame with `frame_no` encoded in its marker.
pub fn render_frame(width: u32, height: u32, frame_no: u32) -> Result<Frame, FrameError> {
    let stride = width as usize;
    let len = Frame::expected_len(width, height, stride)?;
    let mut data = vec![NEUTRAL_CHROMA; len];
    let luma_len = stride * height as usize;
    data[..luma_len].fill(0);

    let layout = MarkerLayout::for_frame(width, height)?;
    draw_marker(
        &mut data[..luma_len],
        stride,
        width,
        height,
        &layout,
        frame_no,
        Corner::TopLeft,
    );
    draw_marker(
        &mut data[..luma_len],
        stride,
        width,
        height,
        &layout,
        frame_no,
        Corner::BottomRight,
    );

    Frame::new_nv12(width, height, stride, 0, data)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Corner {
    TopLeft,
    BottomRight,
}

#[allow(clippy::too_many_arguments)]
fn draw_marker(
    luma: &mut [u8],
    stride: usize,
    width: u32,
    height: u32,
    layout: &MarkerLayout,
    frame_no: u32,
    corner: Corner,
) {
    let (origin_x, origin_y) = match corner {
        Corner::TopLeft => (layout.margin, layout.margin),
        Corner::BottomRight => (
            width - layout.margin - layout.marker_width(),
            height - layout.margin - layout.block,
        ),
    };

    let bits = (MARKER_SYNC << MARKER_BITS) | (frame_no & 0xFFFF);
    for bit in 0..MARKER_TOTAL_BITS {
        let set = (bits >> (MARKER_TOTAL_BITS - 1 - bit)) & 1 == 1;
        let value = if set { WHITE } else { BLACK };
        let start_x = origin_x + bit as u32 * layout.block;
        for y in origin_y..origin_y + layout.block {
            let row = y as usize * stride;
            let start = row + start_x as usize;
            luma[start..start + layout.block as usize].fill(value);
        }
    }

    // Border the marker so the block boundaries survive chroma subsampling and
    // a consumer's own scaling in the captured frame.
    for bit in 0..=MARKER_TOTAL_BITS {
        let x = origin_x + bit as u32 * layout.block;
        if x < width {
            for y in origin_y.saturating_sub(1)..=(origin_y + layout.block).min(height - 1) {
                let row = y as usize * stride;
                luma[row + x as usize] = 0x00;
            }
        }
    }
}

/// Read the frame number back from a rendered frame.
///
/// The top-left copy is preferred; the bottom-right copy is used when the first
/// is unreadable (for example because a consumer cropped the frame).
pub fn read_marker(frame: &Frame) -> Option<u32> {
    let layout = MarkerLayout::for_frame(frame.width, frame.height).ok()?;
    let luma = frame.luma();

    [Corner::TopLeft, Corner::BottomRight]
        .into_iter()
        .find_map(|corner| {
            read_marker_at(
                luma,
                frame.stride,
                frame.width,
                frame.height,
                &layout,
                corner,
            )
        })
}

fn read_marker_at(
    luma: &[u8],
    stride: usize,
    width: u32,
    height: u32,
    layout: &MarkerLayout,
    corner: Corner,
) -> Option<u32> {
    let (origin_x, origin_y) = match corner {
        Corner::TopLeft => (layout.margin, layout.margin),
        Corner::BottomRight => (
            width - layout.margin - layout.marker_width(),
            height - layout.margin - layout.block,
        ),
    };
    let block = layout.block;
    if block < 4 {
        return None;
    }

    let mut raw = 0u32;
    for bit in 0..MARKER_TOTAL_BITS {
        let center_x = origin_x + bit as u32 * block + block / 2;
        let center_y = origin_y + block / 2;
        let index = center_y as usize * stride + center_x as usize;
        let sample = *luma.get(index)?;
        raw = (raw << 1) | u32::from(sample > NEUTRAL_CHROMA);
    }
    let sync = raw >> MARKER_BITS;
    if sync != MARKER_SYNC {
        return None;
    }
    Some(raw & 0xFFFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rendered_marker_round_trips_at_both_supported_sizes() {
        for (width, height) in [(1280u32, 720u32), (640, 480)] {
            for frame_no in [0u32, 1, 42, 65_535] {
                let frame = render_frame(width, height, frame_no).expect("frame renders");
                assert_eq!(
                    read_marker(&frame),
                    Some(frame_no),
                    "{width}x{height} frame {frame_no}"
                );
            }
        }
    }

    #[test]
    fn the_duplicate_marker_survives_a_cropped_primary_marker() {
        let mut frame = render_frame(1280, 720, 1234).expect("frame renders");
        let layout = MarkerLayout::for_frame(frame.width, frame.height).expect("layout fits");
        let stride = frame.stride;
        let luma_len = stride * frame.height as usize;
        for y in 0..(layout.margin + layout.block + 2) {
            let row = y as usize * stride;
            frame.data[row..row + (layout.margin + layout.marker_width() + 2) as usize].fill(0x00);
        }
        assert_eq!(read_marker(&frame), Some(1234));
        assert_eq!(frame.luma().len(), luma_len);
    }

    #[test]
    fn chroma_is_neutral_and_the_luma_plane_is_untouched_outside_the_markers() {
        let frame = render_frame(640, 480, 9).expect("frame renders");
        assert!(frame.chroma().iter().all(|value| *value == NEUTRAL_CHROMA));
        let layout = MarkerLayout::for_frame(640, 480).expect("layout fits");
        let top_marker_end = (layout.margin + layout.block + 2) as usize;
        let bottom_marker_start = (frame.height - layout.margin - layout.block - 2) as usize;
        for row in top_marker_end..bottom_marker_start {
            let line = &frame.luma()[row * frame.stride..(row + 1) * frame.stride];
            assert!(
                line.iter().all(|value| *value == 0),
                "row {row} stays black between the two markers"
            );
        }
    }
}
