//! A deterministic loss scenario: a real H.264 encode crosses a lossy link into
//! the receive engine and the frame pacer, all on virtual time. Shared by the
//! integration tests and the product harness report.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use crate::SoftwareH264Decoder;
use crate::engine::ReceiveEngine;
use crate::output::{FramePacer, OutputMode, Paced};
use crate::recovery::RecoveryRequest;
use crate::testing::{
    LossProfile, LossyLink, PICTURE_HEIGHT, PICTURE_WIDTH, ProfileLabel, synthetic_picture,
};
use openh264::encoder::Encoder;
use openh264::formats::YUVBuffer;
use rtc::rtp::Packet;
use rtc::rtp::codec::h264::H264Payloader;
use rtc::rtp::header::Header;
use rtc::rtp::packetizer::Payloader;

const STEP: Duration = Duration::from_millis(1);
const FRAME_INTERVAL_MICROS: u64 = 33_333;
const MTU: usize = 1_200;
const FEEDBACK_DELAY: Duration = Duration::from_millis(30);

/// What a loss scenario observed, from the consumer's side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LossReport {
    pub label: ProfileLabel,
    pub seconds: u64,
    pub packets_sent: u32,
    pub fresh_frames: u32,
    pub repeated_frames: u32,
    /// Ticks after the first frame where the video was signalled absent.
    pub absent_ticks: u32,
    pub longest_without_fresh: Duration,
    pub packets_lost: u64,
    pub packets_late: u64,
    pub nack_requests: u64,
    pub nacked_packets: u64,
    pub plis: u64,
    pub jitter_depth: Duration,
}

enum Event {
    Media(Packet),
    Feedback(RecoveryRequest),
}

#[derive(Debug, Default)]
struct Tally {
    fresh: u32,
    repeated: u32,
    absent: u32,
    longest_without_fresh: Duration,
    packets_sent: u32,
}

struct Scenario {
    encoder: Encoder,
    payloader: H264Payloader,
    sequence: u16,
    history: HashMap<u16, Packet>,
    link: LossyLink,
    events: BTreeMap<(u64, u64), Event>,
    event_id: u64,
    keyframe_requested: bool,
}

impl Scenario {
    fn new(profile: LossProfile) -> Self {
        Self {
            encoder: Encoder::new().expect("encoder"),
            payloader: H264Payloader::default(),
            sequence: 0,
            history: HashMap::new(),
            link: LossyLink::new(profile, 0x5eed),
            events: BTreeMap::new(),
            event_id: 0,
            keyframe_requested: false,
        }
    }

    fn schedule(&mut self, at: Duration, event: Event) {
        self.event_id += 1;
        self.events.insert(
            (u64::try_from(at.as_micros()).expect("time"), self.event_id),
            event,
        );
    }

    fn send(&mut self, packet: Packet, now: Duration) {
        if let Some(arrival) = self.link.transmit(now) {
            self.schedule(arrival, Event::Media(packet));
        }
    }

    fn send_frame(&mut self, index: u32, now: Duration) -> u32 {
        if std::mem::take(&mut self.keyframe_requested) {
            self.encoder.force_intra_frame();
        }
        let picture = YUVBuffer::from_vec(synthetic_picture(index), PICTURE_WIDTH, PICTURE_HEIGHT);
        let encoded = self
            .encoder
            .encode(&picture)
            .expect("encode")
            .to_vec()
            .into();
        let payloads = self.payloader.payload(MTU, &encoded).expect("payload");
        let count = payloads.len();
        for (position, payload) in payloads.into_iter().enumerate() {
            let packet = Packet {
                header: Header {
                    sequence_number: self.sequence,
                    timestamp: index * 3_000,
                    marker: position + 1 == count,
                    payload_type: 96,
                    ..Default::default()
                },
                payload,
            };
            self.sequence = self.sequence.wrapping_add(1);
            self.history
                .insert(packet.header.sequence_number, packet.clone());
            self.send(packet, now);
        }
        count as u32
    }

    fn deliver_due(&mut self, now: Duration, engine: &mut ReceiveEngine) {
        let due: Vec<_> = self
            .events
            .range(..=(u64::try_from(now.as_micros()).expect("time"), u64::MAX))
            .map(|(key, _)| *key)
            .collect();
        for key in due {
            match self.events.remove(&key).expect("event") {
                Event::Media(packet) => engine.ingest(packet, now),
                Event::Feedback(RecoveryRequest::Pli) => self.keyframe_requested = true,
                Event::Feedback(RecoveryRequest::Nack(sequences)) => {
                    for sequence in sequences {
                        if let Some(packet) = self.history.get(&sequence).cloned() {
                            self.send(packet, now);
                        }
                    }
                }
            }
        }
    }
}

/// Streams a real H.264 encode through `profile` into the receive engine and the
/// frame pacer on virtual time, and reports what the consumer saw.
pub fn run_loss_profile(profile: LossProfile, seconds: u64) -> LossReport {
    let decoder = SoftwareH264Decoder::new().expect("decoder");
    let mut engine = ReceiveEngine::new(Box::new(decoder));
    let mut scenario = Scenario::new(profile);
    let mut pacer = FramePacer::new(OutputMode::Vga);
    let mut tally = Tally::default();
    let (mut next_frame, mut next_tick) = (0_u64, 0_u64);
    let mut frame_index = 0_u32;
    let mut last_fresh = Duration::ZERO;
    let total = Duration::from_secs(seconds);
    let mut now = Duration::ZERO;
    while now < total {
        let micros = u64::try_from(now.as_micros()).expect("time");
        if micros >= next_frame {
            tally.packets_sent += scenario.send_frame(frame_index, now);
            frame_index += 1;
            next_frame += FRAME_INTERVAL_MICROS;
        }
        scenario.deliver_due(now, &mut engine);
        let output = engine.advance(now);
        for frame in output.frames {
            pacer.offer(frame, now);
        }
        for request in output.requests {
            scenario.schedule(now + FEEDBACK_DELAY, Event::Feedback(request));
        }
        if micros >= next_tick {
            match pacer.tick(now) {
                Paced::Fresh(_) => {
                    tally.fresh += 1;
                    last_fresh = now;
                }
                Paced::Repeated(_) => tally.repeated += 1,
                // Before the first frame there is nothing to be absent from.
                Paced::Absent if tally.fresh > 0 => tally.absent += 1,
                Paced::Absent => {}
            }
            tally.longest_without_fresh = tally.longest_without_fresh.max(now - last_fresh);
            next_tick += FRAME_INTERVAL_MICROS;
        }
        now += STEP;
    }
    let stats = engine.stats();
    LossReport {
        label: profile.label(),
        seconds,
        packets_sent: tally.packets_sent,
        fresh_frames: tally.fresh,
        repeated_frames: tally.repeated,
        absent_ticks: tally.absent,
        longest_without_fresh: tally.longest_without_fresh,
        packets_lost: stats.jitter.lost,
        packets_late: stats.jitter.late,
        nack_requests: stats.recovery.nack_requests,
        nacked_packets: stats.recovery.nacked_packets,
        plis: stats.recovery.plis,
        jitter_depth: stats.depth,
    }
}
