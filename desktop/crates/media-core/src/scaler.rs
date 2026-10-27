//! NV12 fit-to-canvas scaling for the published output modes: nearest-neighbour,
//! aspect preserving, black bars. A frame that already has the canvas size is
//! never touched.

use crate::nv12::Nv12Frame;

const BLACK_LUMA: u8 = 16;
const NEUTRAL_CHROMA: u8 = 128;

/// Largest even size with the source aspect ratio that fits the canvas.
fn fitted_size(source: (u32, u32), canvas: (u32, u32)) -> (u32, u32) {
    let (source_width, source_height) = (u64::from(source.0), u64::from(source.1));
    let (canvas_width, canvas_height) = (u64::from(canvas.0), u64::from(canvas.1));
    let (width, height) = if source_width * canvas_height >= canvas_width * source_height {
        (canvas_width, canvas_width * source_height / source_width)
    } else {
        (canvas_height * source_width / source_height, canvas_height)
    };
    ((width & !1).max(2) as u32, (height & !1).max(2) as u32)
}

/// Fits `frame` into a `width` x `height` canvas.
pub fn fit(frame: &Nv12Frame, width: u32, height: u32) -> Nv12Frame {
    if (frame.width(), frame.height()) == (width, height) {
        return frame.clone();
    }
    let (fit_width, fit_height) = fitted_size((frame.width(), frame.height()), (width, height));
    let offset_x = (((width - fit_width) / 2) & !1) as usize;
    let offset_y = (((height - fit_height) / 2) & !1) as usize;
    let (canvas_w, canvas_h) = (width as usize, height as usize);
    let (fit_w, fit_h) = (fit_width as usize, fit_height as usize);
    let (source_w, source_h) = (frame.width() as usize, frame.height() as usize);

    let mut data = vec![BLACK_LUMA; canvas_w * canvas_h];
    data.resize(canvas_w * canvas_h * 3 / 2, NEUTRAL_CHROMA);

    for row in 0..fit_h {
        let source_row = row * source_h / fit_h;
        for column in 0..fit_w {
            let source_column = column * source_w / fit_w;
            data[(offset_y + row) * canvas_w + offset_x + column] =
                frame.luma()[source_row * source_w + source_column];
        }
    }
    let chroma_base = canvas_w * canvas_h;
    for row in 0..fit_h / 2 {
        let source_row = row * (source_h / 2) / (fit_h / 2);
        for column in 0..fit_w / 2 {
            let source_column = column * (source_w / 2) / (fit_w / 2);
            let from = source_row * source_w + source_column * 2;
            let to = chroma_base + (offset_y / 2 + row) * canvas_w + offset_x + column * 2;
            data[to..to + 2].copy_from_slice(&frame.chroma()[from..from + 2]);
        }
    }
    Nv12Frame::new(width, height, frame.rtp_timestamp(), data)
        .expect("canvas dimensions are even and the buffer is sized for them")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: u32, height: u32, luma: &[u8], chroma: &[u8]) -> Nv12Frame {
        let data = [luma, chroma].concat();
        Nv12Frame::new(width, height, 77, data).expect("frame")
    }

    #[test]
    fn upscaling_replicates_each_source_sample() {
        let source = frame(2, 2, &[1, 2, 3, 4], &[10, 20]);
        let scaled = fit(&source, 4, 4);
        assert_eq!((scaled.width(), scaled.height()), (4, 4));
        assert_eq!(
            scaled.luma(),
            [1, 1, 2, 2, 1, 1, 2, 2, 3, 3, 4, 4, 3, 3, 4, 4]
        );
        assert_eq!(scaled.chroma(), [10, 20, 10, 20, 10, 20, 10, 20]);
        assert_eq!(scaled.rtp_timestamp(), 77);
    }

    #[test]
    fn downscaling_picks_the_source_sample_under_each_output_sample() {
        let luma: Vec<u8> = (0..16).collect();
        let source = frame(4, 4, &luma, &[1, 2, 3, 4, 5, 6, 7, 8]);
        let scaled = fit(&source, 2, 2);
        assert_eq!(scaled.luma(), [0, 2, 8, 10]);
        assert_eq!(scaled.chroma(), [1, 2]);
    }

    #[test]
    fn a_narrower_source_is_centred_between_black_bars() {
        let source = frame(4, 4, &[50; 16], &[90; 8]);
        let scaled = fit(&source, 8, 4);
        let row: Vec<u8> = scaled.luma()[..8].to_vec();
        assert_eq!(row, [16, 16, 50, 50, 50, 50, 16, 16]);
        assert_eq!(&scaled.chroma()[..8], [128, 128, 90, 90, 90, 90, 128, 128]);
    }

    #[test]
    fn a_shorter_source_is_centred_between_black_bars_above_and_below() {
        let source = frame(4, 2, &[50; 8], &[90; 4]);
        let scaled = fit(&source, 4, 6);
        assert_eq!(&scaled.luma()[..8], [16; 8]);
        assert_eq!(&scaled.luma()[8..16], [50; 8]);
        assert_eq!(&scaled.luma()[16..], [16; 8]);
    }
}
