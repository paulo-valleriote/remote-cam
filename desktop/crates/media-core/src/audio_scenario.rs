use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use opus::{Application, Channels, Encoder};
use rtc::rtp::Packet;
use rtc::rtp::header::Header;

use crate::audio::{FRAME_SAMPLES, SAMPLE_RATE_HZ};
use crate::audio_engine::{AudioEngine, AudioSink};
use crate::testing::{LossProfile, LossyLink, ProfileLabel};

const STEP: Duration = Duration::from_millis(1);
const FRAME_INTERVAL: Duration = Duration::from_millis(20);
const DROPOUT: Duration = Duration::from_millis(50);
const DROPOUT_SAMPLES: usize = (SAMPLE_RATE_HZ as usize) * 50 / 1_000;
const TONE_HZ: f32 = 440.0;
const TONE_PEAK: f32 = 8_000.0;
const OPUS_PAYLOAD_TYPE: u8 = 111;

/// Timing detector: the gap between two consecutive writes to the sink.
#[derive(Debug, Default)]
pub struct DeliveryGapDetector {
    last: Option<Duration>,
    longest: Duration,
    dropouts: u64,
}

impl DeliveryGapDetector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn observe(&mut self, now: Duration) {
        if let Some(last) = self.last {
            let gap = now.saturating_sub(last);
            self.longest = self.longest.max(gap);
            if gap >= DROPOUT {
                self.dropouts += 1;
            }
        }
        self.last = Some(now);
    }

    pub fn dropouts(&self) -> u64 {
        self.dropouts
    }

    pub fn longest_gap(&self) -> Duration {
        self.longest
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
    pub delivery_dropouts: u64,
    pub silence_dropouts: u64,
    pub longest_delivery_gap: Duration,
    pub longest_silent_run: Duration,
}

impl AudioReport {
    /// Both detectors must agree on zero, over at least ten minutes of the single
    /// nominal profile.
    pub fn passes_gate(&self) -> bool {
        self.label == ProfileLabel::Nominal
            && self.seconds >= 600
            && self.delivery_dropouts == 0
            && self.silence_dropouts == 0
            && self.decode_errors == 0
    }
}

#[derive(Default)]
struct Observed {
    now: Duration,
    played: u64,
    delivery: DeliveryGapDetector,
    silence: SilenceRunDetector,
}

struct DetectingSink(Arc<Mutex<Observed>>);

impl AudioSink for DetectingSink {
    fn write(&mut self, pcm: &[i16]) {
        let mut observed = self.0.lock().expect("observed");
        let now = observed.now;
        observed.played += 1;
        observed.delivery.observe(now);
        observed.silence.observe(pcm);
    }
}

fn tone(frame: u32) -> Vec<i16> {
    (0..FRAME_SAMPLES)
        .map(|index| {
            let position = frame as usize * FRAME_SAMPLES + index;
            let phase = position as f32 * TONE_HZ * std::f32::consts::TAU / SAMPLE_RATE_HZ as f32;
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
        observed.lock().expect("observed").now = now;
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
        delivery_dropouts: observed.delivery.dropouts(),
        silence_dropouts: observed.silence.dropouts(),
        longest_delivery_gap: observed.delivery.longest_gap(),
        longest_silent_run: observed.silence.longest_run(),
    }
}
