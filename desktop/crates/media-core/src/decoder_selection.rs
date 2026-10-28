//! Decoder backend selection (REC-56). Hardware is tried first; any failure to
//! open it, including the empirical NV12 check, falls back to software and is
//! recorded in the [`DecoderReport`] for the diagnostics.

use crate::decoder::{DecodeError, VideoDecoder};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecoderBackend {
    MediaFoundation,
    OpenH264,
}

impl DecoderBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MediaFoundation => "media_foundation",
            Self::OpenH264 => "openh264",
        }
    }
}

/// Outcome of the empirical hardware check that precedes any measurement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HardwareProbe {
    NotAttempted,
    Passed,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecoderReport {
    pub backend: DecoderBackend,
    pub hardware_probe: HardwareProbe,
}

/// One factory per backend. `open` must prove the backend produces NV12 before
/// returning, so a success here is the empirical check.
pub trait DecoderFactory {
    fn backend(&self) -> DecoderBackend;
    fn open(&self) -> Result<Box<dyn VideoDecoder>, DecodeError>;
}

pub struct SelectedDecoder {
    pub decoder: Box<dyn VideoDecoder>,
    pub report: DecoderReport,
}

pub fn select_decoder(
    hardware: Option<&dyn DecoderFactory>,
    software: Option<&dyn DecoderFactory>,
) -> Result<SelectedDecoder, DecodeError> {
    let hardware_probe = match hardware.map(|factory| (factory.backend(), factory.open())) {
        None => HardwareProbe::NotAttempted,
        Some((backend, Ok(decoder))) => {
            return Ok(SelectedDecoder {
                decoder,
                report: DecoderReport {
                    backend,
                    hardware_probe: HardwareProbe::Passed,
                },
            });
        }
        Some((_, Err(error))) => HardwareProbe::Failed(error.to_string()),
    };
    let Some(software) = software else {
        return Err(DecodeError::Decoder(match hardware_probe {
            HardwareProbe::Failed(reason) => {
                format!("no decoder backend available (hardware: {reason})")
            }
            _ => "no decoder backend available".to_owned(),
        }));
    };
    Ok(SelectedDecoder {
        decoder: software.open()?,
        report: DecoderReport {
            backend: software.backend(),
            hardware_probe,
        },
    })
}
