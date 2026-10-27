use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use opus::{Application, Channels, Encoder};
use rtc::rtp::Packet;
use rtc::rtp::header::Header;

use crate::audio::{FRAME_SAMPLES, SAMPLE_RATE_HZ};
use crate::audio_engine::{AudioEngine, AudioSink, FrameOrigin};
use crate::testing::{LossProfile, LossyLink, ProfileLabel};

const STEP: Duration = Duration::from_millis(1);
const FRAME_INTERVAL: Duration = Duration::from_millis(20);
const DROPOUT_SAMPLES: usize = (SAMPLE_RATE_HZ as usize) * 50 / 1_000;
/// Whole frames that reach the 50 ms threshold: 50 ms rounds up to three 20 ms frames.
const DROPOUT_FRAMES: usize = DROPOUT_SAMPLES.div_ceil(FRAME_SAMPLES);
const TONE_HZ: f64 = 440.0;
const TONE_PEAK: f64 = 8_000.0;
const OPUS_PAYLOAD_TYPE: u8 = 111;

/// Provenance detector: a run of frames with no audio at all. PLC is the specified
/// recovery and counts as audio; only frames the engine had to fill with digital
/// silence are a hole. It watches what the engine reports, where
/// `SilenceRunDetector` watches the samples themselves.
#[derive(Debug, Default)]
pub struct UnderrunDetector {
    run: usize,
    longest: usize,
    dropouts: u64,
}

impl UnderrunDetector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn observe(&mut self, origin: FrameOrigin) {
        if origin != FrameOrigin::Silence {
            self.run = 0;
            return;
        }
        self.run += 1;
        if self.run == DROPOUT_FRAMES {
            self.dropouts += 1;
        }
        self.longest = self.longest.max(self.run);
    }

    pub fn dropouts(&self) -> u64 {
        self.dropouts
    }

    pub fn longest_run(&self) -> Duration {
        Duration::from_micros(
            (self.longest * FRAME_SAMPLES) as u64 * 1_000_000 / u64::from(SAMPLE_RATE_HZ),
        )
    }
}

/// Content detector: a run of exact digital zero in a source that is never silent.
#[derive(Debug, Default)]
pub struct SilenceRunDetector {
    run: usize,
    longest: usize,
    dropouts: u64,
}

impl SilenceRunDetector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn observe(&mut self, pcm: &[i16]) {
        for sample in pcm {
            if *sample == 0 {
                self.run += 1;
                if self.run == DROPOUT_SAMPLES {
                    self.dropouts += 1;
                }
            } else {
                self.run = 0;
            }
            self.longest = self.longest.max(self.run);
        }
    }

    pub fn dropouts(&self) -> u64 {
        self.dropouts
    }

    pub fn longest_run(&self) -> Duration {
        Duration::from_micros(self.longest as u64 * 1_000_000 / u64::from(SAMPLE_RATE_HZ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioReport {
    pub label: ProfileLabel,
    pub seconds: u64,
    pub frames_sent: u32,
    pub frames_played: u64,
    pub decoded: u64,
    pub concealed: u64,
    pub silent: u64,
    pub late: u64,
    pub decode_errors: u64,
    pub underrun_dropouts: u64,
    pub silence_dropouts: u64,
    pub longest_underrun: Duration,
    pub longest_silent_run: Duration,
}

impl AudioReport {
    /// The two detectors reach the same verdict: both saw an outage or neither did.
    pub fn detectors_agree(&self) -> bool {
        (self.underrun_dropouts > 0) == (self.silence_dropouts > 0)
    }

    /// Both detectors must agree on zero, over at least ten minutes of the single
    /// nominal profile.
    pub fn passes_gate(&self) -> bool {
        self.label == ProfileLabel::Nominal
            && self.seconds >= 600
            && self.underrun_dropouts == 0
            && self.silence_dropouts == 0
            && self.detectors_agree()
            && self.decode_errors == 0
    }
}

#[derive(Default)]
struct Observed {
    played: u64,
    underrun: UnderrunDetector,
    silence: SilenceRunDetector,
}

struct DetectingSink(Arc<Mutex<Observed>>);

impl AudioSink for DetectingSink {
    fn write(&mut self, pcm: &[i16], origin: FrameOrigin) {
        let mut observed = self.0.lock().expect("observed");
        observed.played += 1;
        observed.underrun.observe(origin);
        observed.silence.observe(pcm);
    }
}

fn tone(frame: u32) -> Vec<i16> {
    (0..FRAME_SAMPLES)
        .map(|index| {
            let position = frame as usize * FRAME_SAMPLES + index;
            let phase =
                position as f64 * TONE_HZ * std::f64::consts::TAU / f64::from(SAMPLE_RATE_HZ);
            (phase.sin() * TONE_PEAK) as i16
        })
        .collect()
}

/// Virtual-time run: a synthetic phone sends a 20 ms Opus tone through a lossy link
/// and the `AudioEngine` plays it into two dropout detectors.
pub fn run_audio_profile(profile: LossProfile, seconds: u64) -> AudioReport {
    let observed = Arc::new(Mutex::new(Observed::default()));
    let mut engine =
        AudioEngine::new(Box::new(DetectingSink(Arc::clone(&observed)))).expect("engine");
    let mut encoder =
        Encoder::new(SAMPLE_RATE_HZ, Channels::Mono, Application::Voip).expect("encoder");
    let mut link = LossyLink::new(profile, 0x5eed_a0d1);
    let mut in_flight: BTreeMap<(Duration, u32), Packet> = BTreeMap::new();
    let total = Duration::from_secs(seconds);
    let (mut now, mut next_frame, mut frames_sent) = (Duration::ZERO, Duration::ZERO, 0_u32);
    while now < total {
        if now >= next_frame {
            let mut payload = vec![0_u8; 400];
            let length = encoder
                .encode(&tone(frames_sent), &mut payload)
                .expect("encode");
            payload.truncate(length);
            let packet = Packet {
                header: Header {
                    sequence_number: frames_sent as u16,
                    timestamp: frames_sent.wrapping_mul(FRAME_SAMPLES as u32),
                    payload_type: OPUS_PAYLOAD_TYPE,
                    ..Default::default()
                },
                payload: payload.into(),
            };
            if let Some(arrival) = link.transmit(now) {
                in_flight.insert((arrival, frames_sent), packet);
            }
            frames_sent += 1;
            next_frame += FRAME_INTERVAL;
        }
        while let Some(entry) = in_flight.first_entry() {
            if entry.key().0 > now {
                break;
            }
            engine.ingest(entry.remove(), now);
        }
        engine.advance(now);
        now += STEP;
    }
    let stats = engine.stats();
    let observed = observed.lock().expect("observed");
    AudioReport {
        label: profile.label(),
        seconds,
        frames_sent,
        frames_played: observed.played,
        decoded: stats.decoded,
        concealed: stats.concealed,
        silent: stats.silent,
        late: stats.late,
        decode_errors: stats.decode_errors,
        underrun_dropouts: observed.underrun.dropouts(),
        silence_dropouts: observed.silence.dropouts(),
        longest_underrun: observed.underrun.longest_run(),
        longest_silent_run: observed.silence.longest_run(),
    }
}
