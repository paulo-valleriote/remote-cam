//! WASAPI render adapter with an RTP-paced resampler (T072).
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike.
//!
//! Audio is rendered 48 kHz mono into the default endpoint. The resampling
//! ratio is **not** free-running: it is derived from the RTP timestamps over a
//! sliding window, because a fixed-point resampler with a `1e-5` relative error
//! drifts 36 ms/h and fails the 20 ms/h criterion on its own.

use windows::Win32::Media::Audio::{
    AUDCLNT_SHAREMODE_SHARED, IAudioClient, IAudioRenderClient, IMMDeviceEnumerator,
    MMDeviceEnumerator, eConsole, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::core::Result as WinResult;

use crate::drift::SlidingRatio;
use crate::resample::MonoResampler;

/// Format of the shared-mode render endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderFormat {
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub is_float: bool,
}

/// WASAPI renderer for decoded mono audio.
pub struct WasapiRenderer {
    client: IAudioClient,
    render: IAudioRenderClient,
    format: RenderFormat,
    buffer_frames: u32,
    ratio: SlidingRatio,
    resampler: MonoResampler,
    last_rtp_timestamp: Option<u64>,
    last_written_frames: u64,
    com_initialized: bool,
}

impl WasapiRenderer {
    /// Open the default render endpoint in shared mode.
    pub fn open_default() -> WinResult<Self> {
        unsafe {
            let com_initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;

            let format_ptr = client.GetMixFormat()?;
            if format_ptr.is_null() {
                return Err(windows::core::Error::from(
                    windows::Win32::Foundation::E_POINTER,
                ));
            }
            let wave_format = *format_ptr;
            let is_float = wave_format.wFormatTag == 3
                || (wave_format.wFormatTag == 0xFFFE && wave_format.wBitsPerSample == 32);
            let format = RenderFormat {
                sample_rate_hz: wave_format.nSamplesPerSec,
                channels: wave_format.nChannels,
                bits_per_sample: wave_format.wBitsPerSample,
                is_float,
            };

            // 200 ms shared buffer: long enough to absorb a scheduling hiccup,
            // short enough that a stall is detected instead of hidden.
            let buffer_duration = 2_000_000_i64;
            client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                0,
                buffer_duration,
                0,
                format_ptr,
                None,
            )?;
            let buffer_frames = client.GetBufferSize()?;
            let render: IAudioRenderClient = client.GetService()?;
            client.Start()?;

            Ok(Self {
                client,
                render,
                format,
                buffer_frames,
                ratio: SlidingRatio::new(100),
                resampler: MonoResampler::new(),
                last_rtp_timestamp: None,
                last_written_frames: 0,
                com_initialized,
            })
        }
    }

    pub fn format(&self) -> RenderFormat {
        self.format
    }

    /// Render mono samples paced from the RTP timestamp of the last packet.
    ///
    /// `rtp_timestamp` is in the Opus 48 kHz clock. The rendered/elapsed ratio
    /// feeds the sliding estimator; a deviation is corrected by the resampler
    /// rather than by dropping or repeating whole buffers.
    pub fn write_mono(&mut self, samples: &[f32], rtp_timestamp: u64) -> WinResult<usize> {
        self.update_ratio(rtp_timestamp);
        let ratio = self.ratio.ratio();
        let resampled = self.resampler.process(samples, ratio);
        let written = self.render_frames(&resampled)?;
        self.last_written_frames = written as u64;
        Ok(written)
    }

    fn update_ratio(&mut self, rtp_timestamp: u64) {
        if let Some(previous) = self.last_rtp_timestamp
            && self.last_written_frames > 0
        {
            let rtp_delta =
                rtp_timestamp.saturating_sub(previous) as f64 / self.format.sample_rate_hz as f64;
            // RTP time is the reference. Frames the device actually consumed are
            // the measured side, so back-pressure or a device-clock deviation
            // moves the ratio instead of accumulating as drift.
            let rendered_delta =
                self.last_written_frames as f64 / self.format.sample_rate_hz as f64;
            self.ratio.push(rtp_delta, rendered_delta);
        }
        self.last_rtp_timestamp = Some(rtp_timestamp);
    }

    fn render_frames(&mut self, samples: &[f32]) -> WinResult<usize> {
        let channels = self.format.channels.max(1) as usize;
        let mut written = 0_usize;
        let mut pending = samples;

        while !pending.is_empty() {
            let padding = unsafe { self.client.GetCurrentPadding()? };
            let free = self.buffer_frames.saturating_sub(padding) as usize;
            if free == 0 {
                // A full shared buffer means the device is behind; stop here
                // rather than block the network thread.
                break;
            }
            let frames = free.min(pending.len());
            let bytes_per_sample = if self.format.is_float && self.format.bits_per_sample == 32 {
                4
            } else if self.format.bits_per_sample == 16 {
                2
            } else {
                return Err(windows::core::Error::from(
                    windows::Win32::Foundation::E_NOTIMPL,
                ));
            };
            let destination = unsafe { self.render.GetBuffer(frames as u32)? };
            let destination = unsafe {
                std::slice::from_raw_parts_mut(destination, frames * channels * bytes_per_sample)
            };

            if bytes_per_sample == 4 {
                for (index, sample) in pending[..frames].iter().enumerate() {
                    for channel in 0..channels {
                        let offset = (index * channels + channel) * 4;
                        destination[offset..offset + 4].copy_from_slice(&sample.to_le_bytes());
                    }
                }
            } else {
                for (index, sample) in pending[..frames].iter().enumerate() {
                    let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                    for channel in 0..channels {
                        let offset = (index * channels + channel) * 2;
                        destination[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
                    }
                }
            }

            unsafe { self.render.ReleaseBuffer(frames as u32, 0)? };
            written += frames;
            pending = &pending[frames..];
        }

        Ok(written)
    }
}

impl Drop for WasapiRenderer {
    fn drop(&mut self) {
        // Stop and release the stream before the COM apartment is uninitialised.
        unsafe {
            let _ = self.client.Stop();
        }
        if self.com_initialized {
            unsafe { CoUninitialize() };
        }
    }
}
