//! Media Foundation virtual camera media source (T030).
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike.
//!
//! The source is hosted by the Windows Frame Server, not by the application, so
//! it cannot read the application's ring buffer directly. This spike therefore
//! feeds the source from the committed test pattern: the gate is about whether a
//! consumer application can discover the camera and receive stable frames, not
//! about cross-process frame transport, which belongs to the media phase.
//!
//! What is implemented:
//!
//! * `IMFMediaSource` with `CreatePresentationDescriptor`, `Start`, `Stop`,
//!   `Pause` and `Shutdown`, plus the `IMFMediaEventGenerator` surface required
//!   by the Frame Server;
//! * `IMFMediaStream` that pulls frames from a [`crate::ring_buffer`] and queues
//!   `MEMediaSample` events, with `MEStreamTick` when no frame is ready yet;
//! * NV12 1280x720@30 (default) and 640x480@30 (the second type consumers see);
//! * `IClassFactory` plus the `DllGetClassObject`/`DllCanUnloadNow` exports the
//!   COM registration points at.
//!
//! Deliberate limits, recorded in `docs/spike/gate-1-camera-injection.md`: a
//! blocked `RequestSample` waits at most one frame interval; there is no
//! per-stream format negotiation beyond the two announced types; and the
//! `IMFVirtualCamera` runtime object's lifetime is managed by
//! [`crate::registration`].

use std::ffi::c_void;
use std::sync::{Arc, Mutex, MutexGuard};

use windows::Win32::Foundation::{E_INVALIDARG, E_UNEXPECTED, S_OK};
use windows::Win32::Media::MediaFoundation::MFVideoFormat_H264;
use windows::Win32::Media::MediaFoundation::{
    IMFAsyncCallback, IMFAsyncResult, IMFMediaEvent, IMFMediaEventGenerator,
    IMFMediaEventGenerator_Impl, IMFMediaEventQueue, IMFMediaSource, IMFMediaSource_Impl,
    IMFMediaStream, IMFMediaStream_Impl, IMFMediaType, IMFPresentationDescriptor,
    IMFStreamDescriptor, MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS, MEMediaSample, MENewStream,
    MESourcePaused, MESourceStarted, MESourceStopped, MEStreamPaused, MEStreamStarted,
    MEStreamStopped, MEStreamTick, MEUpdatedStream, MF_E_INVALIDMEDIATYPE, MF_E_INVALIDREQUEST,
    MF_E_NO_MORE_TYPES, MF_E_SHUTDOWN, MF_EVENT_TYPE, MF_MT_ALL_SAMPLES_INDEPENDENT,
    MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE,
    MFCreateEventQueue, MFCreateMediaType, MFCreateMemoryBuffer, MFCreatePresentationDescriptor,
    MFCreateSample, MFCreateStreamDescriptor, MFMediaType_Video, MFVideoFormat_NV12,
    MFVideoInterlace_Progressive,
};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows::core::{
    BOOL, GUID, HRESULT, IUnknown, Interface, Ref, Result as WinResult, implement,
};

use crate::ring_buffer::{FrameReader, RingBuffer};
use crate::test_pattern::render_frame;

/// Stable CLSID of the spike media source. Never reused by the product.
pub const VIRTUAL_CAMERA_CLSID: GUID = GUID::from_u128(0x8f7c_1e10_5b2a_4f6e_9a3d_1c2b_3a4d_5e6f);

/// Frame rate announced by both media types.
pub const FRAME_RATE: u32 = 30;
/// Media types announced in this order: 1280x720 first, 640x480 second.
pub const SUPPORTED_SIZES: [(u32, u32); 2] = [(1280, 720), (640, 480)];
/// Longest a `RequestSample` may wait for a frame, in milliseconds.
const SAMPLE_WAIT_MS: u64 = 34;
/// Reference-time units per second.
const REFERENCE_TIME_PER_SECOND: i64 = 10_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceState {
    Stopped,
    Started,
    Paused,
    Shutdown,
}

struct SourceInner {
    buffer: RingBuffer,
    events: IMFMediaEventQueue,
    descriptor: Mutex<Option<IMFStreamDescriptor>>,
    stream: Mutex<Option<StreamHandles>>,
    self_source: Mutex<Option<IMFMediaSource>>,
    state: Mutex<SourceState>,
}

// SAFETY: every Media Foundation object held here is agile and internally
// synchronised, and all mutable state is behind a `Mutex`. The two marker impls
// are required because `windows` interfaces are not `Sync` by default.
unsafe impl Send for SourceInner {}
unsafe impl Sync for SourceInner {}
unsafe impl Send for StreamInner {}
unsafe impl Sync for StreamInner {}

/// The stream interface handed to the Frame Server and the state behind it.
struct StreamHandles {
    interface: IMFMediaStream,
    inner: Arc<StreamInner>,
}

struct StreamInner {
    source: Mutex<Option<IMFMediaSource>>,
    descriptor: IMFStreamDescriptor,
    events: IMFMediaEventQueue,
    reader: Mutex<FrameReader>,
    self_stream: Mutex<Option<IMFMediaStream>>,
    state: Mutex<SourceState>,
    fps: u32,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Create the media source with an external ring buffer and start feeding it
/// from the test pattern.
pub fn create_test_pattern_source() -> WinResult<IMFMediaSource> {
    let buffer = RingBuffer::new(4).map_err(|_| windows::core::Error::from(MF_E_INVALIDREQUEST))?;
    spawn_test_pattern(buffer.clone(), 0);

    let events = unsafe { MFCreateEventQueue()? };
    let inner = Arc::new(SourceInner {
        buffer,
        events,
        descriptor: Mutex::new(None),
        stream: Mutex::new(None),
        self_source: Mutex::new(None),
        state: Mutex::new(SourceState::Stopped),
    });
    let outer = VirtualCameraSource {
        inner: Arc::clone(&inner),
    };
    let source: IMFMediaSource = outer.into();
    *lock(&inner.self_source) = Some(source.clone());
    Ok(source)
}

fn spawn_test_pattern(buffer: RingBuffer, start_frame: u32) {
    let writer = buffer.writer();
    std::thread::spawn(move || {
        let interval = std::time::Duration::from_secs_f64(1.0 / FRAME_RATE as f64);
        let started = std::time::Instant::now();
        let mut frame_no = start_frame;
        loop {
            if writer.is_shutdown() {
                return;
            }
            let (width, height) = SUPPORTED_SIZES[0];
            if let Ok(mut frame) = render_frame(width, height, frame_no) {
                frame.timestamp_ns = started.elapsed().as_nanos() as u64;
                if writer.publish(frame).is_none() {
                    return;
                }
            }
            frame_no = frame_no.wrapping_add(1);
            std::thread::sleep(interval);
        }
    });
}

/// Build the NV12 media type announced to consumers.
fn nv12_type(width: u32, height: u32, fps: u32) -> WinResult<IMFMediaType> {
    let media_type = unsafe { MFCreateMediaType()? };
    unsafe {
        media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        media_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
        media_type.SetUINT64(&MF_MT_FRAME_SIZE, ((width as u64) << 32) | height as u64)?;
        media_type.SetUINT64(&MF_MT_FRAME_RATE, ((fps as u64) << 32) | 1)?;
        media_type.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
        media_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
    }
    Ok(media_type)
}

fn stream_descriptor(inner: &SourceInner) -> WinResult<IMFStreamDescriptor> {
    if let Some(descriptor) = lock(&inner.descriptor).as_ref() {
        return Ok(descriptor.clone());
    }
    let types = SUPPORTED_SIZES
        .iter()
        .map(|(width, height)| nv12_type(*width, *height, FRAME_RATE).map(Some))
        .collect::<WinResult<Vec<_>>>()?;
    let descriptor = unsafe { MFCreateStreamDescriptor(0, &types)? };
    *lock(&inner.descriptor) = Some(descriptor.clone());
    Ok(descriptor)
}

fn queue_unknown(
    events: &IMFMediaEventQueue,
    event: MF_EVENT_TYPE,
    value: &IUnknown,
) -> WinResult<()> {
    unsafe { events.QueueEventParamUnk(event.0 as u32, &GUID::zeroed(), S_OK, value) }
}

fn queue_status(events: &IMFMediaEventQueue, event: MF_EVENT_TYPE) -> WinResult<()> {
    unsafe { events.QueueEventParamVar(event.0 as u32, &GUID::zeroed(), S_OK, std::ptr::null()) }
}

fn create_stream(inner: &Arc<SourceInner>) -> WinResult<StreamHandles> {
    let descriptor = stream_descriptor(inner)?;
    let events = unsafe { MFCreateEventQueue()? };
    let stream_inner = Arc::new(StreamInner {
        source: Mutex::new(lock(&inner.self_source).clone()),
        descriptor,
        events,
        reader: Mutex::new(inner.buffer.reader()),
        self_stream: Mutex::new(None),
        state: Mutex::new(SourceState::Stopped),
        fps: FRAME_RATE,
    });
    let outer = VirtualCameraStream {
        inner: Arc::clone(&stream_inner),
    };
    let stream: IMFMediaStream = outer.into();
    *lock(&stream_inner.self_stream) = Some(stream.clone());
    Ok(StreamHandles {
        interface: stream,
        inner: stream_inner,
    })
}

#[implement(IMFMediaSource, IMFMediaEventGenerator)]
struct VirtualCameraSource {
    inner: Arc<SourceInner>,
}

impl IMFMediaSource_Impl for VirtualCameraSource_Impl {
    fn GetCharacteristics(&self) -> WinResult<u32> {
        Ok(0x0000_0001) // MFMEDIASOURCE_IS_LIVE
    }

    fn CreatePresentationDescriptor(&self) -> WinResult<IMFPresentationDescriptor> {
        let descriptor = stream_descriptor(&self.inner)?;
        unsafe { MFCreatePresentationDescriptor(Some(&[Some(descriptor)])) }
    }

    fn Start(
        &self,
        _descriptor: Ref<IMFPresentationDescriptor>,
        _time_format: *const GUID,
        _start_position: *const windows::Win32::System::Com::StructuredStorage::PROPVARIANT,
    ) -> WinResult<()> {
        if *lock(&self.inner.state) == SourceState::Shutdown {
            return Err(MF_E_SHUTDOWN.into());
        }
        *lock(&self.inner.state) = SourceState::Started;

        let (stream, stream_inner) = {
            let mut current = lock(&self.inner.stream);
            if current.is_none() {
                *current = Some(create_stream(&self.inner)?);
            }
            let handles = current.as_ref().expect("stream was just created");
            (handles.interface.clone(), Arc::clone(&handles.inner))
        };
        let stream_unknown: IUnknown = stream.cast()?;

        queue_status(&self.inner.events, MESourceStarted)?;
        queue_unknown(&self.inner.events, MENewStream, &stream_unknown)?;
        queue_unknown(&self.inner.events, MEUpdatedStream, &stream_unknown)?;

        *lock(&stream_inner.state) = SourceState::Started;
        queue_status(&stream_inner.events, MEStreamStarted)?;
        Ok(())
    }

    fn Stop(&self) -> WinResult<()> {
        if *lock(&self.inner.state) == SourceState::Shutdown {
            return Err(MF_E_SHUTDOWN.into());
        }
        *lock(&self.inner.state) = SourceState::Stopped;
        queue_status(&self.inner.events, MESourceStopped)?;
        if let Some(handles) = lock(&self.inner.stream).as_ref() {
            *lock(&handles.inner.state) = SourceState::Stopped;
            queue_status(&handles.inner.events, MEStreamStopped)?;
        }
        Ok(())
    }

    fn Pause(&self) -> WinResult<()> {
        if *lock(&self.inner.state) == SourceState::Shutdown {
            return Err(MF_E_SHUTDOWN.into());
        }
        *lock(&self.inner.state) = SourceState::Paused;
        queue_status(&self.inner.events, MESourcePaused)?;
        if let Some(handles) = lock(&self.inner.stream).as_ref() {
            *lock(&handles.inner.state) = SourceState::Paused;
            queue_status(&handles.inner.events, MEStreamPaused)?;
        }
        Ok(())
    }

    fn Shutdown(&self) -> WinResult<()> {
        *lock(&self.inner.state) = SourceState::Shutdown;
        self.inner.buffer.shutdown();
        unsafe {
            self.inner.events.Shutdown()?;
        }
        if let Some(handles) = lock(&self.inner.stream).as_ref() {
            *lock(&handles.inner.state) = SourceState::Shutdown;
            unsafe {
                handles.inner.events.Shutdown()?;
            }
        }
        *lock(&self.inner.self_source) = None;
        Ok(())
    }
}

impl IMFMediaEventGenerator_Impl for VirtualCameraSource_Impl {
    fn GetEvent(&self, flags: MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS) -> WinResult<IMFMediaEvent> {
        unsafe { self.inner.events.GetEvent(flags.0) }
    }

    fn BeginGetEvent(
        &self,
        callback: Ref<IMFAsyncCallback>,
        state: Ref<IUnknown>,
    ) -> WinResult<()> {
        unsafe {
            self.inner
                .events
                .BeginGetEvent(callback.as_ref(), state.as_ref())
        }
    }

    fn EndGetEvent(&self, result: Ref<IMFAsyncResult>) -> WinResult<IMFMediaEvent> {
        unsafe { self.inner.events.EndGetEvent(result.as_ref()) }
    }

    fn QueueEvent(
        &self,
        event: u32,
        extended_type: *const GUID,
        status: HRESULT,
        value: *const windows::Win32::System::Com::StructuredStorage::PROPVARIANT,
    ) -> WinResult<()> {
        unsafe {
            self.inner
                .events
                .QueueEventParamVar(event, extended_type, status, value)
        }
    }
}

#[implement(IMFMediaStream, IMFMediaEventGenerator)]
struct VirtualCameraStream {
    inner: Arc<StreamInner>,
}

impl IMFMediaStream_Impl for VirtualCameraStream_Impl {
    fn GetMediaSource(&self) -> WinResult<IMFMediaSource> {
        lock(&self.inner.source)
            .clone()
            .ok_or_else(|| MF_E_SHUTDOWN.into())
    }

    fn GetStreamDescriptor(&self) -> WinResult<IMFStreamDescriptor> {
        Ok(self.inner.descriptor.clone())
    }

    fn RequestSample(&self, token: Ref<IUnknown>) -> WinResult<()> {
        let state = *lock(&self.inner.state);
        if state == SourceState::Shutdown {
            return Err(MF_E_SHUTDOWN.into());
        }
        if state != SourceState::Started {
            return Err(E_UNEXPECTED.into());
        }

        let frame = {
            let mut reader = lock(&self.inner.reader);
            match reader.next_timeout(std::time::Duration::from_millis(SAMPLE_WAIT_MS)) {
                Ok(Some(frame)) => frame,
                Ok(None) => return Err(MF_E_SHUTDOWN.into()),
                Err(_) => {
                    // No new picture: announce a stream tick instead of
                    // inventing a duplicate frame.
                    return unsafe {
                        self.inner.events.QueueEventParamVar(
                            MEStreamTick.0 as u32,
                            &GUID::zeroed(),
                            S_OK,
                            std::ptr::null(),
                        )
                    };
                }
            }
        };

        let sample = sample_from_frame(&frame, self.inner.fps)?;
        let sample_unknown: IUnknown = sample.cast()?;
        unsafe {
            self.inner.events.QueueEventParamUnk(
                MEMediaSample.0 as u32,
                &GUID::zeroed(),
                S_OK,
                token.as_ref().unwrap_or(&sample_unknown),
            )?;
        }
        Ok(())
    }
}

impl IMFMediaEventGenerator_Impl for VirtualCameraStream_Impl {
    fn GetEvent(&self, flags: MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS) -> WinResult<IMFMediaEvent> {
        unsafe { self.inner.events.GetEvent(flags.0) }
    }

    fn BeginGetEvent(
        &self,
        callback: Ref<IMFAsyncCallback>,
        state: Ref<IUnknown>,
    ) -> WinResult<()> {
        unsafe {
            self.inner
                .events
                .BeginGetEvent(callback.as_ref(), state.as_ref())
        }
    }

    fn EndGetEvent(&self, result: Ref<IMFAsyncResult>) -> WinResult<IMFMediaEvent> {
        unsafe { self.inner.events.EndGetEvent(result.as_ref()) }
    }

    fn QueueEvent(
        &self,
        event: u32,
        extended_type: *const GUID,
        status: HRESULT,
        value: *const windows::Win32::System::Com::StructuredStorage::PROPVARIANT,
    ) -> WinResult<()> {
        unsafe {
            self.inner
                .events
                .QueueEventParamVar(event, extended_type, status, value)
        }
    }
}

fn sample_from_frame(
    frame: &crate::ring_buffer::Frame,
    fps: u32,
) -> WinResult<windows::Win32::Media::MediaFoundation::IMFSample> {
    let length =
        u32::try_from(frame.data.len()).map_err(|_| windows::core::Error::from(E_INVALIDARG))?;
    let buffer = unsafe { MFCreateMemoryBuffer(length)? };
    unsafe {
        let mut target = std::ptr::null_mut();
        buffer.Lock(&mut target, None, None)?;
        std::ptr::copy_nonoverlapping(frame.data.as_ptr(), target, frame.data.len());
        buffer.Unlock()?;
        buffer.SetCurrentLength(length)?;
    }

    let sample = unsafe { MFCreateSample()? };
    unsafe {
        sample.AddBuffer(&buffer)?;
        sample.SetSampleTime((frame.timestamp_ns / 100) as i64)?;
        sample.SetSampleDuration(REFERENCE_TIME_PER_SECOND / fps as i64)?;
    }
    Ok(sample)
}

/// Class factory used by the registered COM server.
#[implement(IClassFactory)]
struct VirtualCameraClassFactory;

impl IClassFactory_Impl for VirtualCameraClassFactory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<IUnknown>,
        interface_id: *const GUID,
        object: *mut *mut c_void,
    ) -> WinResult<()> {
        if outer.is_some() {
            return Err(windows::Win32::Foundation::CLASS_E_NOAGGREGATION.into());
        }
        if interface_id.is_null() || object.is_null() {
            return Err(E_INVALIDARG.into());
        }
        let source = create_test_pattern_source()?;
        unsafe { source.query(interface_id, object) }.ok()
    }

    fn LockServer(&self, _lock: BOOL) -> WinResult<()> {
        Ok(())
    }
}

/// `DllGetClassObject` export. The COM registration points `InprocServer32` at
/// the DLL built from this crate.
///
/// # Safety
/// Called by COM with valid class and interface identifiers.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(
    class_id: *const GUID,
    interface_id: *const GUID,
    object: *mut *mut c_void,
) -> HRESULT {
    if class_id.is_null() || interface_id.is_null() || object.is_null() {
        return E_INVALIDARG;
    }
    if unsafe { *class_id } != VIRTUAL_CAMERA_CLSID {
        return windows::Win32::Foundation::CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = VirtualCameraClassFactory.into();
    unsafe { factory.query(interface_id, object) }
}

/// `DllCanUnloadNow` export.
#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    S_OK
}

/// Build the media type the consumer negotiated, for diagnostics.
pub fn describe_media_type(media_type: &IMFMediaType) -> WinResult<(u32, u32)> {
    unsafe {
        let size = media_type.GetUINT64(&MF_MT_FRAME_SIZE)?;
        Ok(((size >> 32) as u32, size as u32))
    }
}

/// The H.264 subtype is announced nowhere: this source is uncompressed NV12
/// only, and a consumer that asks for H.264 must fall back.
pub const UNSUPPORTED_SUBTYPE: GUID = MFVideoFormat_H264;

#[allow(dead_code)]
fn _assert_error_types() {
    let _ = MF_E_NO_MORE_TYPES;
    let _ = MF_E_INVALIDMEDIATYPE;
}
