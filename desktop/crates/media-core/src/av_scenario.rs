use std::collections::BTreeMap;
use std::time::Duration;

use opus::{Application, Channels, Encoder};
use rtc::rtp::Packet;
use rtc::rtp::header::Header;

use crate::audio::{FRAME_SAMPLES, SAMPLE_RATE_HZ};
use crate::audio_engine::{AudioEngine, AudioSink, FrameOrigin};
use crate::av_sync::{AudioClock, DRIFT_TARGET, SyncMode, VideoSync};
use crate::decoder::{DecodeError, VideoDecoder};
use crate::engine::ReceiveEngine;
use crate::nv12::Nv12Frame;
use crate::testing::{LossProfile, LossyLink, ProfileLabel};

const STEP: Duration = Duration::from_millis(1);
const AUDIO_INTERVAL: Duration = Duration::from_millis(20);
const VIDEO_INTERVAL_MICROS: u64 = 33_333;
const VIDEO_TICKS_PER_FRAME: u32 = 3_000;
const OPUS_PAYLOAD_TYPE: u8 = 111;
const VIDEO_PAYLOAD_TYPE: u8 = 96;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AvConfig {
    pub profile: LossProfile,
    pub seconds: u64,
    /// Whether the phone sends audio at all.
    pub audio: bool,
    /// Extra one-way delay on the video link only, to provoke a drift.
    pub video_delay: Duration,
}

impl AvConfig {
    pub fn nominal(seconds: u64) -> Self {
        Self {
            profile: LossProfile::NOMINAL,
            seconds,
            audio: true,
            video_delay: Duration::ZERO,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AvReport {
    pub label: ProfileLabel,
    pub seconds: u64,
    /// The mode of the last tick: `AudioMaster` whenever the audio clock was live.
    pub mode: SyncMode,
    pub video_frames_sent: u32,
    /// Frames presented, with or without an audio clock to measure against.
    pub presented: u64,
    /// Drift measurements delivered to the observer.
    pub measured: u64,
    pub free_presented: u64,
    pub skipped: u64,
    pub max_drift: Duration,
    pub max_frame_offset: Duration,
    /// Presented frames whose drift was above the 20 ms target.
    pub over_target: u64,
    /// The instant the observer stopped the run, if it did.
    pub stopped_at: Option<Duration>,
}

/// Turns every access unit into a tiny frame: the sync does not care about pixels, and
/// an hour of real H.264 would only slow the simulation down.
struct StubDecoder;

impl VideoDecoder for StubDecoder {
    fn decode(
        &mut self,
        _access_unit: &[u8],
        rtp_timestamp: u32,
    ) -> Result<Option<Nv12Frame>, DecodeError> {
        Nv12Frame::new(2, 2, rtp_timestamp, vec![0; 6])
            .map(Some)
            .map_err(DecodeError::Frame)
    }
}

struct NullSink;

impl AudioSink for NullSink {
    fn write(&mut self, _pcm: &[i16], _origin: FrameOrigin) {}
}

enum Arrival {
    Audio(Packet),
    Video(Packet),
}

fn audio_payload() -> Vec<u8> {
    let mut encoder =
        Encoder::new(SAMPLE_RATE_HZ, Channels::Mono, Application::Voip).expect("encoder");
    let pcm: Vec<i16> = (0..FRAME_SAMPLES)
        .map(|index| {
            let phase = index as f64 * 440.0 * std::f64::consts::TAU / f64::from(SAMPLE_RATE_HZ);
            (phase.sin() * 8_000.0) as i16
        })
        .collect();
    let mut payload = vec![0_u8; 400];
    let length = encoder.encode(&pcm, &mut payload).expect("encode");
    payload.truncate(length);
    payload
}

pub fn run_av_profile(config: AvConfig) -> AvReport {
    run_av_profile_observed(config, &mut |_, _| true)
}

/// Virtual-time run of both chains at once. `observer` receives each measured drift
/// with the instant it was measured and returns whether the run goes on.
pub fn run_av_profile_observed(
    config: AvConfig,
    observer: &mut dyn FnMut(Duration, Duration) -> bool,
) -> AvReport {
    let clock = AudioClock::new();
    let mut audio = AudioEngine::new(Box::new(NullSink))
        .expect("engine")
        .with_clock(clock.clone());
    let mut video = ReceiveEngine::new(Box::new(StubDecoder));
    let mut sync = VideoSync::new(clock);
    let mut audio_link = LossyLink::new(config.profile, 0x5eed_a0d1);
    let mut video_link = LossyLink::new(config.profile, 0x5eed_0b1d);
    let payload = audio_payload();
    let mut in_flight: BTreeMap<(Duration, u8, u32), Arrival> = BTreeMap::new();
    let total = Duration::from_secs(config.seconds);
    let (mut now, mut next_audio, mut next_video) = (Duration::ZERO, Duration::ZERO, 0_u64);
    let (mut audio_sent, mut video_sent) = (0_u32, 0_u32);
    let mut mode = SyncMode::VideoFree;
    let (mut over_target, mut presented, mut measured) = (0_u64, 0_u64, 0_u64);
    let mut stopped_at = None;
    while now < total && stopped_at.is_none() {
        if config.audio && now >= next_audio {
            let packet = Packet {
                header: Header {
                    sequence_number: audio_sent as u16,
                    timestamp: audio_sent.wrapping_mul(FRAME_SAMPLES as u32),
                    payload_type: OPUS_PAYLOAD_TYPE,
                    ..Default::default()
                },
                payload: payload.clone().into(),
            };
            if let Some(arrival) = audio_link.transmit(now) {
                in_flight.insert((arrival, 0, audio_sent), Arrival::Audio(packet));
            }
            audio_sent += 1;
            next_audio += AUDIO_INTERVAL;
        }
        if u64::try_from(now.as_micros()).expect("time") >= next_video {
            let packet = Packet {
                header: Header {
                    sequence_number: video_sent as u16,
                    timestamp: video_sent.wrapping_mul(VIDEO_TICKS_PER_FRAME),
                    marker: true,
                    payload_type: VIDEO_PAYLOAD_TYPE,
                    ..Default::default()
                },
                payload: vec![0x65, 0x88, 0x84].into(),
            };
            if let Some(arrival) = video_link.transmit(now) {
                in_flight.insert(
                    (arrival + config.video_delay, 1, video_sent),
                    Arrival::Video(packet),
                );
            }
            video_sent += 1;
            next_video += VIDEO_INTERVAL_MICROS;
        }
        while let Some(entry) = in_flight.first_entry() {
            if entry.key().0 > now {
                break;
            }
            match entry.remove() {
                Arrival::Audio(packet) => audio.ingest(packet, now),
                Arrival::Video(packet) => video.ingest(packet, now),
            }
        }
        audio.advance(now);
        for frame in video.advance(now).frames {
            sync.offer(frame);
        }
        let tick = sync.tick(now);
        mode = tick.mode;
        presented += u64::from(tick.frame.is_some());
        if let Some(drift) = tick.drift {
            measured += 1;
            over_target += u64::from(drift > DRIFT_TARGET);
            if !observer(now, drift) {
                stopped_at = Some(now);
            }
        }
        now += STEP;
    }
    let stats = sync.stats();
    AvReport {
        label: config.profile.label(),
        seconds: config.seconds,
        mode,
        video_frames_sent: video_sent,
        presented,
        measured,
        free_presented: stats.free_presented,
        skipped: stats.skipped,
        max_drift: stats.max_drift,
        max_frame_offset: stats.max_frame_offset,
        over_target,
        stopped_at,
    }
}
