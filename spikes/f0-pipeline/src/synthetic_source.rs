//! Timestamped synthetic source with a per-frame QR marker (T074).
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike.
//!
//! The source renders 1280×720 NV12 frames at 30 fps with a QR code carrying
//! `{epoch_ms}|{frame_no}`. The marker is drawn twice — top-left and
//! bottom-right — because a consumer preview may crop a corner. The latency
//! analyser decodes the marker from the consumer's **own** capture, so the
//! figure is capture-at-consumer, not a sender-side estimate.
//!
//! `verify-marker` encodes one frame with OpenH264, decodes it again and asks
//! the Python `zxing-cpp` decoder to read the payload, which proves the marker
//! survives the codec before any measurement starts.

use std::io::Write as _;
use std::path::Path;
use std::process::Command;

use openh264::decoder::Decoder;
use openh264::encoder::Encoder;
use openh264::formats::YUVSource;
use qrcode::{EcLevel, QrCode};
use remotecam_injection::{Frame, FrameError};

/// Default geometry and rate of the synthetic source.
pub const WIDTH: u32 = 1280;
pub const HEIGHT: u32 = 720;
pub const FRAME_RATE: u32 = 30;
pub const BITRATE_KBPS: u32 = 2_500;
pub const DEFAULT_DURATION_S: u64 = 600;

/// Configuration of one source run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntheticSourceConfig {
    pub source_id: String,
    pub width: u32,
    pub height: u32,
    pub frame_rate: u32,
    pub bitrate_kbps: u32,
    pub duration_s: u64,
}

impl Default for SyntheticSourceConfig {
    fn default() -> Self {
        Self {
            source_id: "src-01".to_owned(),
            width: WIDTH,
            height: HEIGHT,
            frame_rate: FRAME_RATE,
            bitrate_kbps: BITRATE_KBPS,
            duration_s: DEFAULT_DURATION_S,
        }
    }
}

impl SyntheticSourceConfig {
    /// Metadata contract for the run.
    pub fn metadata(&self, generation_command: &str) -> serde_json::Value {
        serde_json::json!({
            "source_id": self.source_id,
            "width": self.width,
            "height": self.height,
            "frame_rate": self.frame_rate,
            "bitrate_kbps": self.bitrate_kbps,
            "duration_s": self.duration_s,
            "marker_encoding": "{epoch_ms}|{frame_no}",
            "marker_region": "top-left and bottom-right, outside the likely centre crop",
            "generation_command": generation_command,
            "includes_mobile_capture": false,
            "includes_mobile_encode": false,
        })
    }
}

/// Marker payload for one frame.
pub fn marker_payload(epoch_ms: u64, frame_no: u64) -> String {
    format!("{epoch_ms}|{frame_no}")
}

/// Where the QR modules are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkerLayout {
    pub module_px: u32,
    pub quiet_zone: u32,
    pub modules: u32,
    pub top_left: (u32, u32),
    pub bottom_right: (u32, u32),
}

impl MarkerLayout {
    pub fn for_frame(width: u32, height: u32, modules: u32) -> Result<Self, FrameError> {
        let module_px = 4;
        let quiet_zone = 8;
        let size = modules * module_px + 2 * quiet_zone;
        if width < 2 * size || height < 2 * size {
            return Err(FrameError::MarkerDoesNotFit { width, height });
        }
        Ok(Self {
            module_px,
            quiet_zone,
            modules,
            top_left: (16, 16),
            bottom_right: (width - 16 - size, height - 16 - size),
        })
    }
}

/// One rendered frame plus its marker metadata.
pub struct SyntheticFrame {
    pub frame: Frame,
    pub epoch_ms: u64,
    pub frame_no: u64,
    pub payload: String,
    pub layout: MarkerLayout,
}

/// Render one NV12 frame.
pub fn render_frame(
    config: &SyntheticSourceConfig,
    epoch_ms: u64,
    frame_no: u64,
) -> Result<SyntheticFrame, FrameError> {
    let payload = marker_payload(epoch_ms, frame_no);
    let qr = QrCode::with_error_correction_level(payload.as_bytes(), EcLevel::M).map_err(|_| {
        FrameError::MarkerDoesNotFit {
            width: config.width,
            height: config.height,
        }
    })?;
    let modules = qr.width() as u32;
    let layout = MarkerLayout::for_frame(config.width, config.height, modules)?;
    let colors = qr.to_colors();

    let stride = config.width as usize;
    let mut data = vec![0x80_u8; Frame::expected_len(config.width, config.height, stride)?];
    let luma_len = stride * config.height as usize;
    // A light background doubles as the QR quiet zone: a grey quiet zone is
    // read as part of the symbol by some decoders.
    data[..luma_len].fill(0xE0);

    draw_qr(
        &mut data[..luma_len],
        stride,
        layout,
        layout.top_left,
        modules,
        &colors,
    );
    draw_qr(
        &mut data[..luma_len],
        stride,
        layout,
        layout.bottom_right,
        modules,
        &colors,
    );

    Ok(SyntheticFrame {
        frame: Frame::new_nv12(config.width, config.height, stride, 0, data)?,
        epoch_ms,
        frame_no,
        payload,
        layout,
    })
}

fn draw_qr(
    luma: &mut [u8],
    stride: usize,
    layout: MarkerLayout,
    origin: (u32, u32),
    modules: u32,
    colors: &[qrcode::Color],
) {
    let (origin_x, origin_y) = origin;
    for y in 0..modules {
        for x in 0..modules {
            let index = (y * modules + x) as usize;
            let dark = colors
                .get(index)
                .map(|color| *color == qrcode::Color::Dark)
                .unwrap_or(false);
            let value = if dark { 0x10 } else { 0xF0 };
            let start_x = origin_x + layout.quiet_zone + x * layout.module_px;
            let start_y = origin_y + layout.quiet_zone + y * layout.module_px;
            for row in start_y..start_y + layout.module_px {
                let offset = row as usize * stride + start_x as usize;
                luma[offset..offset + layout.module_px as usize].fill(value);
            }
        }
    }
}

/// Encode one NV12 frame as an Annex-B H.264 access unit.
pub fn encode_access_unit(frame: &Frame) -> Result<Vec<u8>, String> {
    let yuv = nv12_to_i420(frame);
    let source =
        openh264::formats::YUVBuffer::from_vec(yuv, frame.width as usize, frame.height as usize);
    let mut encoder = Encoder::new().map_err(|error| error.to_string())?;
    encoder.force_intra_frame();
    let stream = encoder.encode(&source).map_err(|error| error.to_string())?;
    Ok(stream.to_vec())
}

fn nv12_to_i420(frame: &Frame) -> Vec<u8> {
    let width = frame.width as usize;
    let height = frame.height as usize;
    let luma_len = width * height;
    let chroma_len = luma_len / 2;
    let mut yuv = vec![0_u8; luma_len + chroma_len];

    for row in 0..height {
        let source = row * frame.stride;
        yuv[row * width..(row + 1) * width].copy_from_slice(&frame.data[source..source + width]);
    }

    let chroma_start = frame.stride * height;
    for row in 0..height / 2 {
        let source = chroma_start + row * frame.stride;
        for column in 0..width / 2 {
            let index = source + column * 2;
            yuv[luma_len + row * (width / 2) + column] = frame.data[index];
            yuv[luma_len + chroma_len / 2 + row * (width / 2) + column] = frame.data[index + 1];
        }
    }
    yuv
}

/// Verify that one frame's QR marker survives encode/decode.
///
/// Writes a PGM of the decoded luma plane and invokes the Python `zxing-cpp`
/// decoder. The Python interpreter and package are part of the measurement
/// toolchain (`spikes/tools/.venv`), so a missing decoder is a loud error, not
/// a silent pass.
pub fn verify_marker_roundtrip(
    config: &SyntheticSourceConfig,
    python: &str,
) -> Result<String, String> {
    let rendered = render_frame(config, 1_700_000_000_000, 42).map_err(|e| e.to_string())?;
    let access_unit = encode_access_unit(&rendered.frame)?;
    let mut decoder = Decoder::new().map_err(|error| error.to_string())?;
    let decoded = decoder
        .decode(&access_unit)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "the decoder returned no frame".to_owned())?;
    let (width, height) = decoded.dimensions();

    let pgm = std::env::temp_dir().join("remotecam-marker.pgm");
    write_pgm(&pgm, width, height, decoded.y(), decoded.strides().0)?;

    let script = "import sys\nfrom pathlib import Path\nimport numpy as np\nimport zxingcpp\npath = Path(sys.argv[1])\ndata = path.read_bytes()\nheader_end = 0\nfields = []\nwhile len(fields) < 4:\n    while data[header_end:header_end+1].isspace(): header_end += 1\n    start = header_end\n    while not data[header_end:header_end+1].isspace(): header_end += 1\n    fields.append(data[start:header_end])\nheader_end += 1\nwidth, height = int(fields[1]), int(fields[2])\nimage = np.frombuffer(data[header_end:header_end+width*height], dtype=np.uint8).reshape(height, width)\nresult = zxingcpp.read_barcodes(image)\nprint(result[0].text if result else '')\n";
    let output = Command::new(python)
        .args(["-c", script])
        .arg(&pgm)
        .output()
        .map_err(|error| format!("cannot run {python}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "marker decoder failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let decoded_payload = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if decoded_payload != rendered.payload {
        return Err(format!(
            "marker payload changed through encode/decode: expected {:?}, decoded {:?}",
            rendered.payload, decoded_payload
        ));
    }
    Ok(decoded_payload)
}

/// Write a binary PGM (P5) with no rescaling.
pub fn write_pgm(
    path: &Path,
    width: usize,
    height: usize,
    luma: &[u8],
    stride: usize,
) -> Result<(), String> {
    let mut file = std::fs::File::create(path).map_err(|error| error.to_string())?;
    write!(file, "P5\n{width} {height}\n255\n").map_err(|error| error.to_string())?;
    for row in 0..height {
        let start = row * stride;
        file.write_all(&luma[start..start + width])
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}
