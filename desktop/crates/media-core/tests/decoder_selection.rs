//! REC-56: backend selection with stand-in decoders. Runs on Linux/CI; the real
//! Media Foundation backend is exercised on the Reference Device.

use remotecam_media_core::decoder::{DecodeError, VideoDecoder};
use remotecam_media_core::decoder_selection::{
    DecoderBackend, DecoderFactory, HardwareProbe, select_decoder,
};
use remotecam_media_core::{MediaCore, Nv12Frame};

/// Stamps every frame with `marker` in the first luma byte so a test can tell
/// which backend produced it.
struct MarkedDecoder {
    marker: u8,
}

impl VideoDecoder for MarkedDecoder {
    fn decode(
        &mut self,
        _access_unit: &[u8],
        rtp_timestamp: u32,
    ) -> Result<Option<Nv12Frame>, DecodeError> {
        let mut data = vec![0; 6];
        data[0] = self.marker;
        Ok(Some(
            Nv12Frame::new(2, 2, rtp_timestamp, data).expect("frame"),
        ))
    }
}

struct StubFactory {
    backend: DecoderBackend,
    marker: u8,
    failure: Option<&'static str>,
}

impl StubFactory {
    fn working(backend: DecoderBackend, marker: u8) -> Self {
        Self {
            backend,
            marker,
            failure: None,
        }
    }

    fn broken(backend: DecoderBackend, reason: &'static str) -> Self {
        Self {
            backend,
            marker: 0,
            failure: Some(reason),
        }
    }
}

impl DecoderFactory for StubFactory {
    fn backend(&self) -> DecoderBackend {
        self.backend
    }

    fn open(&self) -> Result<Box<dyn VideoDecoder>, DecodeError> {
        match self.failure {
            Some(reason) => Err(DecodeError::Decoder(reason.to_owned())),
            None => Ok(Box::new(MarkedDecoder {
                marker: self.marker,
            })),
        }
    }
}

fn first_marker(decoder: &mut dyn VideoDecoder) -> u8 {
    decoder.decode(&[], 1).unwrap().unwrap().data()[0]
}

#[test]
fn a_working_hardware_backend_is_selected_and_reported() {
    let hardware = StubFactory::working(DecoderBackend::MediaFoundation, 0xAA);
    let software = StubFactory::working(DecoderBackend::OpenH264, 0xBB);

    let mut selected = select_decoder(Some(&hardware), Some(&software)).expect("selection");

    assert_eq!(selected.report.backend, DecoderBackend::MediaFoundation);
    assert_eq!(selected.report.hardware_probe, HardwareProbe::Passed);
    assert_eq!(first_marker(selected.decoder.as_mut()), 0xAA);
}

#[test]
fn a_failing_hardware_probe_falls_back_to_openh264_and_records_why() {
    let hardware = StubFactory::broken(DecoderBackend::MediaFoundation, "no NV12 output");
    let software = StubFactory::working(DecoderBackend::OpenH264, 0xBB);

    let mut selected = select_decoder(Some(&hardware), Some(&software)).expect("selection");

    assert_eq!(selected.report.backend, DecoderBackend::OpenH264);
    assert!(matches!(
        &selected.report.hardware_probe,
        HardwareProbe::Failed(reason) if reason.contains("no NV12 output")
    ));
    assert_eq!(first_marker(selected.decoder.as_mut()), 0xBB);
}

#[test]
fn without_a_hardware_candidate_the_probe_is_not_attempted() {
    let software = StubFactory::working(DecoderBackend::OpenH264, 0xBB);

    let selected = select_decoder(None, Some(&software)).expect("selection");

    assert_eq!(selected.report.backend, DecoderBackend::OpenH264);
    assert_eq!(selected.report.hardware_probe, HardwareProbe::NotAttempted);
}

#[test]
fn selection_fails_when_no_backend_can_open() {
    let hardware = StubFactory::broken(DecoderBackend::MediaFoundation, "hw down");
    let software = StubFactory::broken(DecoderBackend::OpenH264, "sw down");

    let error = select_decoder(Some(&hardware), Some(&software))
        .err()
        .expect("error");

    assert!(error.to_string().contains("sw down"));
}

#[test]
fn backends_have_stable_diagnostic_names() {
    assert_eq!(DecoderBackend::MediaFoundation.as_str(), "media_foundation");
    assert_eq!(DecoderBackend::OpenH264.as_str(), "openh264");
}

#[test]
fn selection_fails_without_any_software_fallback() {
    let hardware = StubFactory::broken(DecoderBackend::MediaFoundation, "hw down");

    let error = select_decoder(Some(&hardware), None).err().expect("error");

    let message = error.to_string();
    assert!(message.contains("hw down") && message.contains("no decoder backend"));
}

#[tokio::test]
async fn the_media_core_reports_the_backend_it_was_given() {
    let hardware = StubFactory::broken(DecoderBackend::MediaFoundation, "no NV12 output");
    let software = StubFactory::working(DecoderBackend::OpenH264, 0xBB);
    let selected = select_decoder(Some(&hardware), Some(&software)).expect("selection");

    let core = MediaCore::with_selection("127.0.0.1:0", selected)
        .await
        .expect("media core");

    let report = core.decoder_report().expect("report");
    assert_eq!(report.backend, DecoderBackend::OpenH264);
    assert!(matches!(report.hardware_probe, HardwareProbe::Failed(_)));
}

#[tokio::test]
async fn a_core_built_from_a_bare_decoder_has_no_backend_report() {
    let core = MediaCore::with_decoder("127.0.0.1:0", Box::new(MarkedDecoder { marker: 1 }))
        .await
        .expect("media core");

    assert!(core.decoder_report().is_none());
}

#[cfg(not(windows))]
#[tokio::test]
async fn the_default_linux_core_reports_openh264_without_a_hardware_probe() {
    let core = MediaCore::new("127.0.0.1:0").await.expect("media core");

    let report = core.decoder_report().expect("report");
    assert_eq!(report.backend, DecoderBackend::OpenH264);
    assert_eq!(report.hardware_probe, HardwareProbe::NotAttempted);
}
