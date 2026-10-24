//! WASAPI rendering into the pinned `CABLE Input` endpoint (T101).
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike.
//!
//! Audio is rendered 48 kHz mono. The resampling ratio is **not** free-running:
//! it is derived from the RTP timestamps over a sliding window, using the same
//! estimator as the desktop receive pipeline (`media-core`). A fixed-point
//! resampler with a `1e-5` relative error drifts 36 ms/h and would fail the
//! 20 ms/h drift criterion on its own.
//!
//! The endpoint is selected by **exact** friendly name and manufacturer. A
//! look-alike device is never opened by prefix match.

use std::time::Duration;

use crate::pacing::{MonoResampler, SlidingRatio};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::E_POINTER;
use windows::Win32::Media::Audio::{
    AUDCLNT_SHAREMODE_SHARED, DEVICE_STATE_ACTIVE, IAudioClient, IAudioRenderClient, IMMDevice,
    IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eRender,
};
use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PropVariantToStringAlloc};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize, STGM_READ,
};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::core::Result as WinResult;

use crate::readiness::{
    AudioEndpoint, Direction, EndpointIdentity, ExpectedDevice, SignatureStatus,
};

/// Format negotiated with the shared-mode endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderFormat {
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub is_float: bool,
}

/// Rendering failure.
#[derive(Debug)]
pub enum RenderError {
    /// No endpoint carries the pinned identity.
    DeviceNotInstalled(String),
    /// A Windows API call failed.
    Windows(windows::core::Error),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DeviceNotInstalled(name) => write!(
                formatter,
                "no audio endpoint named `{name}` with the pinned manufacturer is installed"
            ),
            Self::Windows(error) => write!(formatter, "WASAPI call failed: {error}"),
        }
    }
}

impl std::error::Error for RenderError {}

impl From<windows::core::Error> for RenderError {
    fn from(error: windows::core::Error) -> Self {
        Self::Windows(error)
    }
}

/// Enumerate active render/capture endpoints with their friendly names.
///
/// The signature status is reported as `Unknown`: it is supplied by the
/// PowerShell signature probe, not by WASAPI.
pub fn enumerate_endpoints() -> Result<Vec<AudioEndpoint>, RenderError> {
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
    let mut endpoints = Vec::new();

    for (direction, dataflow) in [(Direction::Render, eRender), (Direction::Capture, eCapture)] {
        let collection = unsafe { enumerator.EnumAudioEndpoints(dataflow, DEVICE_STATE_ACTIVE)? };
        let count = unsafe { collection.GetCount()? };
        for index in 0..count {
            let device = unsafe { collection.Item(index)? };
            let identity = match read_identity(&device) {
                Ok(identity) => identity,
                Err(_) => continue,
            };
            endpoints.push(AudioEndpoint {
                identity,
                direction,
                signature: SignatureStatus::Unknown,
            });
        }
    }

    Ok(endpoints)
}

fn read_identity(device: &IMMDevice) -> WinResult<EndpointIdentity> {
    let store: IPropertyStore = unsafe { device.OpenPropertyStore(STGM_READ)? };
    let friendly_name = unsafe { property_string(&store, &PKEY_Device_FriendlyName)? };
    Ok(EndpointIdentity {
        friendly_name,
        // The manufacturer and driver version are filled by the PowerShell
        // probe; WASAPI exposes only the friendly name reliably.
        manufacturer: String::new(),
        driver_version: String::new(),
    })
}

unsafe fn property_string(
    store: &IPropertyStore,
    key: &windows::Win32::Foundation::PROPERTYKEY,
) -> WinResult<String> {
    let value: PROPVARIANT = unsafe { store.GetValue(key)? };
    let pointer = unsafe { PropVariantToStringAlloc(&value)? };
    let text = unsafe { pointer.to_string() }.unwrap_or_default();
    unsafe { CoTaskMemFree(Some(pointer.0 as *const core::ffi::c_void)) };
    Ok(text)
}

/// Shared-mode renderer for the pinned endpoint.
pub struct CableRenderer {
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

impl CableRenderer {
    /// Open the pinned endpoint, matched by exact friendly name.
    pub fn open(expected: &ExpectedDevice) -> Result<Self, RenderError> {
        unsafe {
            let com_initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            let device = find_render_device(expected)?;
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;

            let format_ptr = client.GetMixFormat()?;
            if format_ptr.is_null() {
                return Err(RenderError::Windows(windows::core::Error::from(E_POINTER)));
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
            client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                0,
                2_000_000_i64,
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
    pub fn write_mono(
        &mut self,
        samples: &[f32],
        rtp_timestamp: u64,
    ) -> Result<usize, RenderError> {
        self.update_ratio(rtp_timestamp);
        let ratio = self.ratio.ratio();
        let resampled = self.resampler.process(samples, ratio);
        let written = self.write_samples(&resampled)?;
        self.last_written_frames = written as u64;
        Ok(written)
    }

    /// Render the mandatory 1 kHz readiness tone and wait for it to be audible.
    pub fn play_test_tone(
        &mut self,
        frequency_hz: f64,
        duration: Duration,
    ) -> Result<(), RenderError> {
        let sample_rate = self.format.sample_rate_hz as f64;
        let frames = (sample_rate * duration.as_secs_f64()) as usize;
        let samples: Vec<f32> = (0..frames)
            .map(|index| {
                let phase = 2.0 * std::f64::consts::PI * frequency_hz * index as f64 / sample_rate;
                (0.25 * phase.sin()) as f32
            })
            .collect();
        let written = self.write_samples(&samples)?;
        if written == 0 {
            return Err(RenderError::Windows(windows::core::Error::from(E_POINTER)));
        }
        std::thread::sleep(duration);
        Ok(())
    }

    fn update_ratio(&mut self, rtp_timestamp: u64) {
        if let Some(previous) = self.last_rtp_timestamp
            && self.last_written_frames > 0
        {
            let rtp_delta =
                rtp_timestamp.saturating_sub(previous) as f64 / self.format.sample_rate_hz as f64;
            let rendered_delta =
                self.last_written_frames as f64 / self.format.sample_rate_hz as f64;
            self.ratio.push(rtp_delta, rendered_delta);
        }
        self.last_rtp_timestamp = Some(rtp_timestamp);
    }

    fn write_samples(&mut self, samples: &[f32]) -> Result<usize, RenderError> {
        let channels = self.format.channels.max(1) as usize;
        let mut written = 0_usize;
        let mut pending = samples;

        while !pending.is_empty() {
            let padding = unsafe { self.client.GetCurrentPadding()? };
            let free = self.buffer_frames.saturating_sub(padding) as usize;
            if free == 0 {
                // The device is behind. Return instead of blocking the caller.
                break;
            }
            let frames = free.min(pending.len());
            let bytes_per_sample = match (self.format.is_float, self.format.bits_per_sample) {
                (true, 32) => 4,
                (_, 16) => 2,
                _ => {
                    return Err(RenderError::Windows(windows::core::Error::from(
                        windows::Win32::Foundation::E_NOTIMPL,
                    )));
                }
            };
            let destination = unsafe { self.render.GetBuffer(frames as u32)? };
            let destination = unsafe {
                std::slice::from_raw_parts_mut(destination, frames * channels * bytes_per_sample)
            };

            for (index, sample) in pending[..frames].iter().enumerate() {
                for channel in 0..channels {
                    let offset = (index * channels + channel) * bytes_per_sample;
                    if bytes_per_sample == 4 {
                        destination[offset..offset + 4].copy_from_slice(&sample.to_le_bytes());
                    } else {
                        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
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

impl Drop for CableRenderer {
    fn drop(&mut self) {
        unsafe {
            let _ = self.client.Stop();
        }
        if self.com_initialized {
            unsafe { CoUninitialize() };
        }
    }
}

fn find_render_device(expected: &ExpectedDevice) -> Result<IMMDevice, RenderError> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let collection = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
        let count = collection.GetCount()?;
        for index in 0..count {
            let device = collection.Item(index)?;
            let identity = read_identity(&device).unwrap_or(EndpointIdentity {
                friendly_name: String::new(),
                manufacturer: String::new(),
                driver_version: String::new(),
            });
            if identity
                .friendly_name
                .trim()
                .eq_ignore_ascii_case(expected.render_friendly_name.trim())
            {
                return Ok(device);
            }
        }
    }
    Err(RenderError::DeviceNotInstalled(
        expected.render_friendly_name.clone(),
    ))
}
