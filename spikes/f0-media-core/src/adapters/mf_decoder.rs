//! Media Foundation H.264 hardware decode adapter (T069, PC-1).
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike.
//!
//! The adapter is Windows-only. It:
//!
//! * discovers decoders with `MFTEnumEx(SYNCMFT|SORTANDFILTER)` and reports
//!   whether a hardware MFT is present;
//! * requires an NV12 output type — an 8-bit hardware decoder exposes NV12, and
//!   any other format is rejected rather than silently falling back;
//! * attaches an `IMFDXGIDeviceManager` to the transform;
//! * locks output buffers in the documented order
//!   `IMF2DBuffer2::Lock2DSize` → `IMF2DBuffer::Lock` → `IMFMediaBuffer::Lock`.

use std::ffi::c_void;
use std::mem::ManuallyDrop;

use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
};
use windows::Win32::Media::MediaFoundation::{
    IMF2DBuffer, IMF2DBuffer2, IMFActivate, IMFAttributes, IMFDXGIDeviceManager, IMFMediaBuffer,
    IMFMediaType, IMFSample, IMFTransform, MF_E_TRANSFORM_NEED_MORE_INPUT, MF_MT_MAJOR_TYPE,
    MF_MT_SUBTYPE, MF_SA_D3D11_AWARE, MF_VERSION, MF2DBuffer_LockFlags_Read, MFCreateAttributes,
    MFCreateDXGIDeviceManager, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
    MFMediaType_Video, MFSTARTUP_FULL, MFStartup, MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAG,
    MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_FLAG_SYNCMFT,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_SET_D3D_MANAGER, MFT_OUTPUT_DATA_BUFFER,
    MFT_OUTPUT_STREAM_INFO, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFTEnumEx, MFVideoFormat_H264,
    MFVideoFormat_NV12,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::core::{Interface, Result as WinResult};

use remotecam_injection::{Frame, FrameError};

/// Errors raised by the Media Foundation adapter.
#[derive(Debug)]
pub enum MfDecoderError {
    Platform(String),
    NoNv12Output,
    Frame(FrameError),
    Unexpected(String),
}

impl std::fmt::Display for MfDecoderError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Platform(message) => write!(formatter, "Media Foundation error: {message}"),
            Self::NoNv12Output => write!(
                formatter,
                "the selected decoder exposes no NV12 output type; hardware decode requires NV12"
            ),
            Self::Frame(error) => write!(formatter, "frame error: {error}"),
            Self::Unexpected(message) => write!(formatter, "unexpected decoder state: {message}"),
        }
    }
}

impl std::error::Error for MfDecoderError {}

fn platform(error: windows::core::Error) -> MfDecoderError {
    MfDecoderError::Platform(error.to_string())
}

/// Recorded result of the hardware-decode precondition (PC-1, Q-5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardwareDecodeVerification {
    pub decoder_found: bool,
    pub hardware_accelerated: bool,
    pub output_format_supported: &'static str,
    pub sync_mft_count: u32,
    pub hardware_mft_count: u32,
    pub discovery_method: &'static str,
}

impl HardwareDecodeVerification {
    /// True only when a hardware decoder was found and it produces NV12.
    pub fn is_admissible(&self) -> bool {
        self.decoder_found && self.hardware_accelerated && self.output_format_supported == "nv12"
    }
}

/// Enumerate H.264 decoder MFTs and test the first one's output type.
pub fn verify_h264_hardware_decode() -> WinResult<HardwareDecodeVerification> {
    unsafe {
        MFStartup(MF_VERSION, MFSTARTUP_FULL)?;

        let sync_flags = MFT_ENUM_FLAG(MFT_ENUM_FLAG_SYNCMFT.0 | MFT_ENUM_FLAG_SORTANDFILTER.0);
        let hardware_flags =
            MFT_ENUM_FLAG(MFT_ENUM_FLAG_HARDWARE.0 | MFT_ENUM_FLAG_SORTANDFILTER.0);

        let (activates, sync_count) = enumerate_decoders(sync_flags)?;
        let hardware_count = enumerate_decoders(hardware_flags).map(|(_, count)| count)?;

        let mut output_format_supported = "other";
        if let Some(activate) = activates.first() {
            let transform: IMFTransform = activate.ActivateObject()?;
            if supports_nv12(&transform)? {
                output_format_supported = "nv12";
            }
        }
        free_activates(activates);

        Ok(HardwareDecodeVerification {
            decoder_found: sync_count > 0,
            hardware_accelerated: hardware_count > 0,
            output_format_supported,
            sync_mft_count: sync_count,
            hardware_mft_count: hardware_count,
            discovery_method: "mft_enum_syncmft",
        })
    }
}

unsafe fn enumerate_decoders(flags: MFT_ENUM_FLAG) -> WinResult<(Vec<IMFActivate>, u32)> {
    let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count: u32 = 0;
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_DECODER,
            flags,
            None,
            None,
            &mut activates,
            &mut count,
        )?;
    }
    if activates.is_null() || count == 0 {
        return Ok((Vec::new(), 0));
    }
    let slice = unsafe { std::slice::from_raw_parts(activates, count as usize) };
    let collected = slice.iter().flatten().cloned().collect();
    unsafe { CoTaskMemFree(Some(activates as *const c_void)) };
    Ok((collected, count))
}

unsafe fn free_activates(activates: Vec<IMFActivate>) {
    for activate in activates {
        let _ = unsafe { activate.ShutdownObject() };
        drop(activate);
    }
}

unsafe fn supports_nv12(transform: &IMFTransform) -> WinResult<bool> {
    for index in 0..16 {
        let media_type = match unsafe { transform.GetOutputAvailableType(0, index) } {
            Ok(media_type) => media_type,
            Err(_) => break,
        };
        if is_nv12(&media_type)? {
            return Ok(true);
        }
    }
    Ok(false)
}

unsafe fn is_nv12(media_type: &IMFMediaType) -> WinResult<bool> {
    let major = unsafe { media_type.GetGUID(&MF_MT_MAJOR_TYPE)? };
    let subtype = unsafe { media_type.GetGUID(&MF_MT_SUBTYPE)? };
    Ok(major == MFMediaType_Video && subtype == MFVideoFormat_NV12)
}

/// Hardware H.264 decoder producing NV12 frames.
pub struct MfH264Decoder {
    transform: IMFTransform,
    width: u32,
    height: u32,
    /// Keeps the device manager alive for the transform's lifetime.
    _device_manager: IMFDXGIDeviceManager,
}

impl MfH264Decoder {
    /// Select the first synchronously enumerated decoder that accepts H.264 in
    /// and NV12 out, and attach the DXGI device manager.
    pub fn new(width: u32, height: u32) -> Result<Self, MfDecoderError> {
        unsafe {
            MFStartup(MF_VERSION, MFSTARTUP_FULL).map_err(platform)?;
            let (activates, _) = enumerate_decoders(MFT_ENUM_FLAG(
                MFT_ENUM_FLAG_SYNCMFT.0 | MFT_ENUM_FLAG_SORTANDFILTER.0,
            ))
            .map_err(platform)?;

            let mut selected = None;
            for activate in &activates {
                let transform: IMFTransform = activate.ActivateObject().map_err(platform)?;
                if configure_types(&transform, width, height).is_ok() {
                    selected = Some(transform);
                    break;
                }
            }
            free_activates(activates);

            let transform = selected.ok_or(MfDecoderError::NoNv12Output)?;
            let device_manager = create_dxgi_device_manager().map_err(platform)?;

            let mut attributes: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attributes, 2).map_err(platform)?;
            if let Some(attributes) = attributes {
                attributes
                    .SetUINT32(&MF_SA_D3D11_AWARE, 1)
                    .map_err(platform)?;
                transform
                    .ProcessMessage(
                        MFT_MESSAGE_SET_D3D_MANAGER,
                        device_manager.as_raw() as usize,
                    )
                    .map_err(platform)?;
            }
            transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .map_err(platform)?;

            Ok(Self {
                transform,
                width,
                height,
                _device_manager: device_manager,
            })
        }
    }

    /// Decode one Annex-B access unit into an NV12 frame.
    pub fn decode(
        &mut self,
        access_unit: &[u8],
        timestamp_ns: u64,
    ) -> Result<Option<Frame>, MfDecoderError> {
        unsafe {
            let sample = self.wrap_input(access_unit, timestamp_ns)?;
            self.transform
                .ProcessInput(0, &sample, 0)
                .map_err(platform)?;
            self.read_output(timestamp_ns)
        }
    }

    unsafe fn wrap_input(
        &self,
        access_unit: &[u8],
        timestamp_ns: u64,
    ) -> Result<IMFSample, MfDecoderError> {
        unsafe {
            let buffer = MFCreateMemoryBuffer(access_unit.len() as u32).map_err(platform)?;
            let mut destination: *mut u8 = std::ptr::null_mut();
            buffer
                .Lock(&mut destination, None, None)
                .map_err(platform)?;
            std::ptr::copy_nonoverlapping(access_unit.as_ptr(), destination, access_unit.len());
            buffer.Unlock().map_err(platform)?;
            buffer
                .SetCurrentLength(access_unit.len() as u32)
                .map_err(platform)?;

            let sample = MFCreateSample().map_err(platform)?;
            sample.AddBuffer(&buffer).map_err(platform)?;
            // Media Foundation reference times are 100 ns units.
            sample
                .SetSampleTime((timestamp_ns / 100) as i64)
                .map_err(platform)?;
            Ok(sample)
        }
    }

    unsafe fn read_output(&mut self, timestamp_ns: u64) -> Result<Option<Frame>, MfDecoderError> {
        unsafe {
            let info: MFT_OUTPUT_STREAM_INFO =
                self.transform.GetOutputStreamInfo(0).map_err(platform)?;
            let provides_samples =
                info.dwFlags & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) != 0;

            let fallback = if provides_samples {
                None
            } else {
                Some(self.allocate_output(info.cbSize)?)
            };
            let mut output = MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: ManuallyDrop::new(fallback),
                dwStatus: 0,
                pEvents: ManuallyDrop::new(None),
            };
            let mut status = 0_u32;
            match self
                .transform
                .ProcessOutput(0, std::slice::from_mut(&mut output), &mut status)
            {
                Ok(()) => {}
                Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                    // The normal answer when an access unit does not complete a
                    // picture yet.
                    return Ok(None);
                }
                Err(error) => {
                    return Err(MfDecoderError::Unexpected(error.to_string()));
                }
            }

            let sample = ManuallyDrop::take(&mut output.pSample);
            let Some(sample) = sample else {
                return Ok(None);
            };
            let buffer = sample.ConvertToContiguousBuffer().map_err(platform)?;
            let pixels = unsafe { copy_locked_nv12(&buffer, self.width, self.height) }
                .map_err(MfDecoderError::Frame)?;
            let sample_timestamp = sample
                .GetSampleTime()
                .map(|time| (time * 100).max(0) as u64)
                .unwrap_or(timestamp_ns);
            Frame::new_nv12(
                self.width,
                self.height,
                self.width as usize,
                sample_timestamp,
                pixels,
            )
            .map(Some)
            .map_err(MfDecoderError::Frame)
        }
    }

    unsafe fn allocate_output(&self, size: u32) -> Result<IMFSample, MfDecoderError> {
        unsafe {
            let buffer = MFCreateMemoryBuffer(size.max(1)).map_err(platform)?;
            let sample = MFCreateSample().map_err(platform)?;
            sample.AddBuffer(&buffer).map_err(platform)?;
            Ok(sample)
        }
    }
}

unsafe fn configure_types(transform: &IMFTransform, width: u32, height: u32) -> WinResult<()> {
    unsafe {
        let input = MFCreateMediaType()?;
        input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
        transform.SetInputType(0, &input, 0)?;

        for index in 0..16 {
            let output = match transform.GetOutputAvailableType(0, index) {
                Ok(output) => output,
                Err(_) => break,
            };
            if is_nv12(&output)? {
                transform.SetOutputType(0, &output, 0)?;
                let _ = (width, height);
                return Ok(());
            }
        }
        Err(windows::core::Error::from(
            windows::Win32::Foundation::E_FAIL,
        ))
    }
}

fn create_dxgi_device_manager() -> WinResult<IMFDXGIDeviceManager> {
    unsafe {
        let mut device: Option<ID3D11Device> = None;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            windows::Win32::Foundation::HMODULE::default(),
            D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )?;
        let device =
            device.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))?;

        let mut token = 0_u32;
        let mut manager: Option<IMFDXGIDeviceManager> = None;
        MFCreateDXGIDeviceManager(&mut token, &mut manager)?;
        let manager = manager
            .ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))?;
        manager.ResetDevice(&device, token)?;
        Ok(manager)
    }
}

/// Copy an output buffer into a tight NV12 image, using the documented lock
/// order so a DXGI-backed buffer is read through its 2-D interface first.
unsafe fn copy_locked_nv12(
    buffer: &IMFMediaBuffer,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, FrameError> {
    let expected = Frame::expected_len(width, height, width as usize)?;

    if let Ok(buffer_2d) = buffer.cast::<IMF2DBuffer2>() {
        let mut scanline: *mut u8 = std::ptr::null_mut();
        let mut pitch = 0_i32;
        let mut start: *mut u8 = std::ptr::null_mut();
        let mut length = 0_u32;
        if unsafe {
            buffer_2d.Lock2DSize(
                MF2DBuffer_LockFlags_Read,
                &mut scanline,
                &mut pitch,
                &mut start,
                &mut length,
            )
        }
        .is_ok()
        {
            let copied =
                unsafe { copy_planes(start, length as usize, pitch, width, height, expected) };
            let _ = unsafe { buffer_2d.Unlock2D() };
            return copied;
        }
    }

    if let Ok(buffer_2d) = buffer.cast::<IMF2DBuffer>() {
        let mut scanline: *mut u8 = std::ptr::null_mut();
        let mut pitch = 0_i32;
        if unsafe { buffer_2d.Lock2D(&mut scanline, &mut pitch) }.is_ok() {
            let copied = unsafe { copy_planes(scanline, 0, pitch, width, height, expected) };
            let _ = unsafe { buffer_2d.Unlock2D() };
            return copied;
        }
    }

    let mut data: *mut u8 = std::ptr::null_mut();
    let mut current_length = 0_u32;
    unsafe {
        buffer
            .Lock(&mut data, None, Some(&mut current_length))
            .map_err(|_| FrameError::ZeroDimension)?;
    }
    let copied = unsafe {
        copy_planes(
            data,
            current_length as usize,
            width as i32,
            width,
            height,
            expected,
        )
    };
    let _ = unsafe { buffer.Unlock() };
    copied
}

unsafe fn copy_planes(
    start: *mut u8,
    length: usize,
    pitch: i32,
    width: u32,
    height: u32,
    expected: usize,
) -> Result<Vec<u8>, FrameError> {
    if start.is_null() || pitch <= 0 || (pitch as usize) < width as usize {
        return Err(FrameError::StrideTooSmall {
            stride: pitch.max(0) as usize,
            width,
        });
    }
    let pitch = pitch as usize;
    let luma_size = pitch * height as usize;
    let chroma_size = pitch * (height as usize / 2);
    if length != 0 && length < luma_size + chroma_size {
        return Err(FrameError::WrongLength {
            expected: expected.max(length),
            actual: length,
        });
    }

    let mut output = vec![0_u8; expected];
    for row in 0..height as usize {
        let source = unsafe { std::slice::from_raw_parts(start.add(row * pitch), width as usize) };
        output[row * width as usize..(row + 1) * width as usize].copy_from_slice(source);
    }
    let chroma_base = luma_size;
    let destination_base = width as usize * height as usize;
    for row in 0..height as usize / 2 {
        let source = unsafe {
            std::slice::from_raw_parts(start.add(chroma_base + row * pitch), width as usize)
        };
        output[destination_base + row * width as usize
            ..destination_base + (row + 1) * width as usize]
            .copy_from_slice(source);
    }
    Ok(output)
}
