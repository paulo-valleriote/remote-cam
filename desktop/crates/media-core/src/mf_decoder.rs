//! Media Foundation H.264 decoder (REC-56, ADR-0025). Windows only; the only
//! `unsafe` in this crate, confined to the COM calls below.
//!
//! Discovery uses `MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER`.
//! `MFT_ENUM_FLAG_HARDWARE` is not trusted as proof: the empirical check is
//! `MediaFoundationFactory::open`, which only succeeds when the transform
//! accepts H.264 input and agrees to produce NV12 output.

use std::mem::ManuallyDrop;

use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Media::MediaFoundation::{
    IMF2DBuffer2, IMFActivate, IMFDXGIBuffer, IMFMediaBuffer, IMFMediaType, IMFSample,
    IMFTransform, MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE, MF_MT_FRAME_SIZE,
    MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_VERSION, MF2DBuffer_LockFlags_Read, MFCreateMediaType,
    MFCreateMemoryBuffer, MFCreateSample, MFGetService, MFMediaType_Video, MFSTARTUP_FULL,
    MFStartup, MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_FLAG_SYNCMFT,
    MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
    MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES,
    MFT_REGISTER_TYPE_INFO, MFTEnumEx, MFVideoFormat_H264, MFVideoFormat_NV12, MR_BUFFER_SERVICE,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree};
use windows::core::Interface;

use crate::decoder::{DecodeError, VideoDecoder};
use crate::decoder_selection::{DecoderBackend, DecoderFactory};
use crate::nv12::{Nv12Frame, pitched_nv12_to_packed};

/// 100 ns units per RTP tick at the 90 kHz video clock.
const HNS_PER_RTP_TICK: i64 = 10_000_000 / 90_000;

fn mf_error(context: &str, error: windows::core::Error) -> DecodeError {
    DecodeError::Decoder(format!("media foundation: {context}: {error}"))
}

pub struct MediaFoundationFactory;

impl DecoderFactory for MediaFoundationFactory {
    fn backend(&self) -> DecoderBackend {
        DecoderBackend::MediaFoundation
    }

    fn open(&self) -> Result<Box<dyn VideoDecoder>, DecodeError> {
        Ok(Box::new(MediaFoundationDecoder::new()?))
    }
}

pub struct MediaFoundationDecoder {
    transform: IMFTransform,
    provides_samples: bool,
    output_size: u32,
    width: u32,
    height: u32,
}

// SAFETY: the decoder is moved to the single decode thread and used only
// there; the MFT is created in the multithreaded apartment.
unsafe impl Send for MediaFoundationDecoder {}

impl MediaFoundationDecoder {
    pub fn new() -> Result<Self, DecodeError> {
        // SAFETY: plain COM/MF initialisation; repeated calls are harmless and
        // an already-initialised apartment (S_FALSE) is not an error.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            MFStartup(MF_VERSION, MFSTARTUP_FULL).map_err(|e| mf_error("MFStartup", e))?;
        }
        let transform = activate_first_h264_decoder()?;
        let mut decoder = Self {
            transform,
            provides_samples: false,
            output_size: 0,
            width: 0,
            height: 0,
        };
        decoder.negotiate()?;
        // SAFETY: the transform is configured; these only start streaming.
        unsafe {
            decoder
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .map_err(|e| mf_error("begin streaming", e))?;
            decoder
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                .map_err(|e| mf_error("start of stream", e))?;
        }
        Ok(decoder)
    }

    /// Sets H.264 input and picks the NV12 output type. Failing here is the
    /// empirical "this transform cannot give NV12" result.
    fn negotiate(&mut self) -> Result<(), DecodeError> {
        // SAFETY: COM calls on a live transform and freshly created media types.
        unsafe {
            let input: IMFMediaType = MFCreateMediaType().map_err(|e| mf_error("input type", e))?;
            input
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .map_err(|e| mf_error("input major type", e))?;
            input
                .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)
                .map_err(|e| mf_error("input subtype", e))?;
            self.transform
                .SetInputType(0, &input, 0)
                .map_err(|e| mf_error("SetInputType(H264)", e))?;
        }
        self.select_nv12_output()
    }

    fn select_nv12_output(&mut self) -> Result<(), DecodeError> {
        // SAFETY: COM calls on a live transform; indices stop at MF_E_NO_MORE_TYPES.
        unsafe {
            let mut index = 0;
            let nv12 = loop {
                let candidate = self
                    .transform
                    .GetOutputAvailableType(0, index)
                    .map_err(|_| {
                        DecodeError::Decoder("media foundation: no NV12 output type".into())
                    })?;
                if candidate.GetGUID(&MF_MT_SUBTYPE).ok() == Some(MFVideoFormat_NV12) {
                    break candidate;
                }
                index += 1;
            };
            self.transform
                .SetOutputType(0, &nv12, 0)
                .map_err(|e| mf_error("SetOutputType(NV12)", e))?;
            let packed = nv12
                .GetUINT64(&MF_MT_FRAME_SIZE)
                .map_err(|e| mf_error("frame size", e))?;
            self.width = (packed >> 32) as u32;
            self.height = packed as u32;
            let info = self
                .transform
                .GetOutputStreamInfo(0)
                .map_err(|e| mf_error("output stream info", e))?;
            self.provides_samples = info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0;
            self.output_size = info.cbSize;
        }
        Ok(())
    }

    fn input_sample(access_unit: &[u8], rtp_timestamp: u32) -> Result<IMFSample, DecodeError> {
        // SAFETY: the buffer is locked, written within its max length and unlocked.
        unsafe {
            let buffer = MFCreateMemoryBuffer(access_unit.len() as u32)
                .map_err(|e| mf_error("input buffer", e))?;
            let mut data = std::ptr::null_mut();
            buffer
                .Lock(&mut data, None, None)
                .map_err(|e| mf_error("lock input", e))?;
            std::ptr::copy_nonoverlapping(access_unit.as_ptr(), data, access_unit.len());
            buffer.Unlock().map_err(|e| mf_error("unlock input", e))?;
            buffer
                .SetCurrentLength(access_unit.len() as u32)
                .map_err(|e| mf_error("input length", e))?;
            let sample = MFCreateSample().map_err(|e| mf_error("input sample", e))?;
            sample
                .AddBuffer(&buffer)
                .map_err(|e| mf_error("add input buffer", e))?;
            sample
                .SetSampleTime(i64::from(rtp_timestamp) * HNS_PER_RTP_TICK)
                .map_err(|e| mf_error("sample time", e))?;
            Ok(sample)
        }
    }

    fn pull_frame(&mut self, rtp_timestamp: u32) -> Result<Option<Nv12Frame>, DecodeError> {
        loop {
            // SAFETY: the output record owns at most one sample reference that is
            // released below; `pEvents` is not requested.
            let produced = unsafe {
                let provided = if self.provides_samples {
                    None
                } else {
                    let buffer = MFCreateMemoryBuffer(self.output_size)
                        .map_err(|e| mf_error("output buffer", e))?;
                    let sample = MFCreateSample().map_err(|e| mf_error("output sample", e))?;
                    sample
                        .AddBuffer(&buffer)
                        .map_err(|e| mf_error("add output buffer", e))?;
                    Some(sample)
                };
                let mut output = [MFT_OUTPUT_DATA_BUFFER {
                    dwStreamID: 0,
                    pSample: ManuallyDrop::new(provided),
                    dwStatus: 0,
                    pEvents: ManuallyDrop::new(None),
                }];
                let mut status = 0;
                let result = self.transform.ProcessOutput(0, &mut output, &mut status);
                let sample = ManuallyDrop::take(&mut output[0].pSample);
                drop(ManuallyDrop::take(&mut output[0].pEvents));
                (result, sample)
            };
            match produced {
                (Ok(()), Some(sample)) => return self.read_frame(&sample, rtp_timestamp).map(Some),
                (Ok(()), None) => return Ok(None),
                (Err(error), _) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                    return Ok(None);
                }
                (Err(error), _) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                    self.select_nv12_output()?;
                }
                (Err(error), _) => return Err(mf_error("ProcessOutput", error)),
            }
        }
    }

    fn read_frame(&self, sample: &IMFSample, rtp_timestamp: u32) -> Result<Nv12Frame, DecodeError> {
        // SAFETY: the 2-D buffer is locked for reading, copied while locked and
        // unlocked before returning; the slice never outlives the lock.
        let packed = unsafe {
            let buffer: IMFMediaBuffer = sample
                .ConvertToContiguousBuffer()
                .map_err(|e| mf_error("contiguous buffer", e))?;
            let two_d: IMF2DBuffer2 = buffer
                .cast()
                .map_err(|e| mf_error("IMF2DBuffer2 unavailable", e))?;
            let (mut scanline0, mut pitch, mut start, mut length) =
                (std::ptr::null_mut(), 0_i32, std::ptr::null_mut(), 0_u32);
            two_d
                .Lock2DSize(
                    MF2DBuffer_LockFlags_Read,
                    &mut scanline0,
                    &mut pitch,
                    &mut start,
                    &mut length,
                )
                .map_err(|e| mf_error("Lock2DSize", e))?;
            let packed = if pitch > 0 && scanline0 == start {
                let bytes = std::slice::from_raw_parts(start, length as usize);
                let pitch = pitch as usize;
                let chroma_rows =
                    (length as usize / pitch).saturating_sub(self.height as usize / 2);
                pitched_nv12_to_packed(self.width, self.height, pitch, chroma_rows, bytes)
                    .map_err(DecodeError::Frame)
            } else {
                Err(DecodeError::Decoder(
                    "media foundation: bottom-up or offset NV12 surface".into(),
                ))
            };
            two_d.Unlock2D().map_err(|e| mf_error("Unlock2D", e))?;
            packed?
        };
        Nv12Frame::new(self.width, self.height, rtp_timestamp, packed).map_err(DecodeError::Frame)
    }
}

impl VideoDecoder for MediaFoundationDecoder {
    fn decode(
        &mut self,
        access_unit: &[u8],
        rtp_timestamp: u32,
    ) -> Result<Option<Nv12Frame>, DecodeError> {
        let sample = Self::input_sample(access_unit, rtp_timestamp)?;
        // SAFETY: COM call on a live transform with a fully built sample.
        unsafe {
            self.transform
                .ProcessInput(0, &sample, 0)
                .map_err(|e| mf_error("ProcessInput", e))?;
        }
        self.pull_frame(rtp_timestamp)
    }
}

impl Drop for MediaFoundationDecoder {
    fn drop(&mut self) {
        // SAFETY: flush a live transform; failure on teardown is not actionable.
        unsafe {
            let _ = self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
        }
    }
}

/// GPU path: the D3D11 texture behind a decoded buffer, reached through the
/// `MR_BUFFER_SERVICE` service. Not used by [`MediaFoundationDecoder::decode`],
/// which copies to system memory; kept for the zero-copy consumer.
pub fn gpu_texture(buffer: &IMFMediaBuffer) -> Option<ID3D11Texture2D> {
    // SAFETY: `MFGetService` writes an owned interface pointer into `raw`.
    unsafe {
        let mut raw = std::ptr::null_mut();
        MFGetService(buffer, &MR_BUFFER_SERVICE, &IMFDXGIBuffer::IID, &mut raw).ok()?;
        let dxgi = IMFDXGIBuffer::from_raw(raw);
        let mut texture = std::ptr::null_mut();
        dxgi.GetResource(&ID3D11Texture2D::IID, &mut texture).ok()?;
        Some(ID3D11Texture2D::from_raw(texture))
    }
}

fn activate_first_h264_decoder() -> Result<IMFTransform, DecodeError> {
    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_H264,
    };
    let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0_u32;
    // SAFETY: MFTEnumEx allocates `count` activate pointers that we release and
    // free with CoTaskMemFree on every path.
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_DECODER,
            MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&input),
            None,
            &mut activates,
            &mut count,
        )
        .map_err(|e| mf_error("MFTEnumEx", e))?;
        let list = std::slice::from_raw_parts_mut(activates, count as usize);
        let first = list.first().and_then(Option::as_ref);
        let transform = match first {
            Some(activate) => activate
                .ActivateObject::<IMFTransform>()
                .map_err(|e| mf_error("ActivateObject", e)),
            None => Err(DecodeError::Decoder(
                "media foundation: no H.264 decoder transform".into(),
            )),
        };
        for slot in list.iter_mut() {
            drop(slot.take());
        }
        CoTaskMemFree(Some(activates.cast()));
        transform
    }
}
