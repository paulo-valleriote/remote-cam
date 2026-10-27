use std::sync::{Arc, Mutex};
use std::time::Duration;

use opus::{Application, Channels, Encoder};
use remotecam_media_core::audio::{FRAME_SAMPLES, OpusDecoder, SAMPLE_RATE_HZ};
use remotecam_media_core::audio_engine::{AudioEngine, AudioSink};
use rtc::rtp::Packet;
use rtc::rtp::header::Header;

type Frames = Arc<Mutex<Vec<Vec<i16>>>>;

struct RecordingSink(Frames);

impl AudioSink for RecordingSink {
    fn write(&mut self, pcm: &[i16]) {
        self.0.lock().expect("frames").push(pcm.to_vec());
    }
}

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

fn engine() -> (AudioEngine, Frames) {
    let frames = Frames::default();
    let engine = AudioEngine::new(Box::new(RecordingSink(Arc::clone(&frames)))).expect("engine");
    (engine, frames)
}

struct Phone {
    encoder: Encoder,
    sequence: u16,
}

impl Phone {
    fn new() -> Self {
        Self {
            encoder: Encoder::new(SAMPLE_RATE_HZ, Channels::Mono, Application::Voip)
                .expect("encoder"),
            sequence: 0,
        }
    }

    fn packet(&mut self, frame: u32) -> Packet {
        let samples: Vec<i16> = (0..FRAME_SAMPLES)
            .map(|index| {
                let position = frame as usize * FRAME_SAMPLES + index;
                let phase = position as f32 * 440.0 * std::f32::consts::TAU / 48_000.0;
                (phase.sin() * 8_000.0) as i16
            })
            .collect();
        let mut payload = vec![0_u8; 400];
        let length = self.encoder.encode(&samples, &mut payload).expect("encode");
        payload.truncate(length);
        let packet = Packet {
            header: Header {
                sequence_number: self.sequence,
                timestamp: frame * FRAME_SAMPLES as u32,
                payload_type: 111,
                ..Default::default()
            },
            payload: payload.into(),
        };
        self.sequence = self.sequence.wrapping_add(1);
        packet
    }
}

fn energy(samples: &[i16]) -> i64 {
    samples.iter().map(|sample| i64::from(*sample).abs()).sum()
}

#[test]
fn plays_each_frame_on_the_rtp_timestamp_schedule_after_the_buffer_depth() {
    let (mut engine, frames) = engine();
    let mut phone = Phone::new();
    let mut reference = OpusDecoder::new().expect("reference");
    let packets: Vec<_> = (0..3).map(|frame| phone.packet(frame)).collect();
    for (index, packet) in packets.iter().enumerate() {
        engine.ingest(packet.clone(), ms(index as u64 * 20));
    }
    engine.advance(ms(59));
    assert!(frames.lock().unwrap().is_empty(), "held for the depth");
    engine.advance(ms(60));
    assert_eq!(frames.lock().unwrap().len(), 1);
    engine.advance(ms(100));
    let played = frames.lock().unwrap().clone();
    assert_eq!(played.len(), 3);
    for (frame, packet) in played.iter().zip(&packets) {
        assert_eq!(frame, &reference.decode(&packet.payload).unwrap());
    }
}

#[test]
fn a_lost_packet_is_concealed_by_plc_and_never_requested_again() {
    let (mut engine, frames) = engine();
    let mut phone = Phone::new();
    for frame in 0..6 {
        let packet = phone.packet(frame);
        if frame != 3 {
            engine.ingest(packet, ms(u64::from(frame) * 20));
        }
    }
    engine.advance(ms(160));
    let played = frames.lock().unwrap().clone();
    assert_eq!(played.len(), 6, "the lost slot still produces a frame");
    assert!(
        energy(&played[3]) > 0,
        "PLC extrapolates, it is not silence"
    );
    let stats = engine.stats();
    assert_eq!(stats.concealed, 1);
    assert_eq!(stats.decoded, 5);
}

#[test]
fn the_stream_continues_as_silence_when_the_phone_stops_sending() {
    let (mut engine, frames) = engine();
    let mut phone = Phone::new();
    for frame in 0..3 {
        engine.ingest(phone.packet(frame), ms(u64::from(frame) * 20));
    }
    engine.advance(ms(60 + 20 * 50));
    let played = frames.lock().unwrap().clone();
    assert_eq!(played.len(), 51, "one frame per 20 ms, never a hole");
    assert!(played.iter().all(|frame| frame.len() == FRAME_SAMPLES));
    assert!(
        played[10..].iter().all(|frame| energy(frame) == 0),
        "after the bounded PLC the output is digital silence"
    );
    assert_eq!(engine.stats().concealed, 5);
    assert_eq!(engine.stats().silent, 43);
}

#[test]
fn nothing_is_written_before_the_first_packet() {
    let (mut engine, frames) = engine();
    engine.advance(ms(1_000));
    assert!(frames.lock().unwrap().is_empty());
}

#[test]
fn a_packet_after_its_slot_is_late_and_a_repeat_is_a_duplicate() {
    let (mut engine, frames) = engine();
    let mut phone = Phone::new();
    let first = phone.packet(0);
    let second = phone.packet(1);
    engine.ingest(first.clone(), ms(0));
    engine.ingest(first, ms(1));
    engine.advance(ms(100));
    engine.ingest(second, ms(101));
    assert_eq!(frames.lock().unwrap().len(), 3);
    let stats = engine.stats();
    assert_eq!((stats.duplicates, stats.late), (1, 1));
}

#[test]
fn a_timestamp_jump_beyond_two_seconds_re_anchors_the_schedule() {
    let (mut engine, frames) = engine();
    let mut phone = Phone::new();
    engine.ingest(phone.packet(0), ms(0));
    let jumped = phone.packet(500);
    engine.ingest(jumped, ms(40));
    assert_eq!(engine.stats().resyncs, 1);
    engine.advance(ms(60));
    assert!(frames.lock().unwrap().is_empty(), "held for a fresh depth");
    engine.advance(ms(100));
    assert_eq!(frames.lock().unwrap().len(), 1);
}

#[test]
fn the_timestamp_wraps_without_breaking_the_schedule() {
    let (mut engine, frames) = engine();
    let mut phone = Phone::new();
    let start = u32::MAX - 2 * FRAME_SAMPLES as u32 + 1;
    for frame in 0..4_u32 {
        let mut packet = phone.packet(frame);
        packet.header.timestamp = start.wrapping_add(frame * FRAME_SAMPLES as u32);
        engine.ingest(packet, ms(u64::from(frame) * 20));
    }
    engine.advance(ms(120));
    assert_eq!(engine.stats().decoded, 4);
    assert_eq!(frames.lock().unwrap().len(), 4);
}

#[test]
fn a_corrupt_or_odd_sized_packet_is_concealed_not_forwarded() {
    let (mut engine, frames) = engine();
    let mut phone = Phone::new();
    engine.ingest(phone.packet(0), ms(0));
    let mut garbage = phone.packet(1);
    garbage.payload = vec![0xff, 0xff, 0xff].into();
    engine.ingest(garbage, ms(20));
    engine.advance(ms(80));
    assert_eq!(frames.lock().unwrap().len(), 2);
    let stats = engine.stats();
    assert_eq!((stats.decode_errors, stats.concealed), (1, 1));
}

#[test]
fn dropping_the_engine_releases_the_sink_and_writes_nothing_after() {
    let (mut engine, frames) = engine();
    let mut phone = Phone::new();
    engine.ingest(phone.packet(0), ms(0));
    engine.advance(ms(60));
    assert_eq!(Arc::strong_count(&frames), 2);
    drop(engine);
    assert_eq!(Arc::strong_count(&frames), 1, "the sink is released");
    assert_eq!(frames.lock().unwrap().len(), 1, "nothing is written after");
}
