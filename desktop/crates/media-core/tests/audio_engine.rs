use std::sync::{Arc, Mutex};
use std::time::Duration;

use opus::{Application, Channels, Encoder};
use remotecam_media_core::audio::{FRAME_SAMPLES, OpusDecoder, SAMPLE_RATE_HZ};
use remotecam_media_core::audio_engine::{AudioEngine, AudioSink, FrameOrigin};
use rtc::rtp::Packet;
use rtc::rtp::header::Header;

type Frames = Arc<Mutex<Vec<Vec<i16>>>>;

struct RecordingSink(Frames);

impl AudioSink for RecordingSink {
    fn write(&mut self, pcm: &[i16], _origin: FrameOrigin) {
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

#[test]
fn a_timestamp_jump_backwards_beyond_two_seconds_re_anchors_too() {
    let (mut engine, frames) = engine();
    let mut phone = Phone::new();
    engine.ingest(phone.packet(500), ms(0));
    let restarted = phone.packet(0);
    engine.ingest(restarted, ms(40));
    assert_eq!(engine.stats().resyncs, 1);
    assert_eq!(engine.stats().late, 0);
    engine.advance(ms(100));
    assert_eq!(frames.lock().unwrap().len(), 1);
}

#[test]
fn a_packet_between_slots_is_played_once_and_a_stale_one_counts_late() {
    let (mut engine, frames) = engine();
    let mut phone = Phone::new();
    engine.ingest(phone.packet(0), ms(0));
    let mut off_grid = phone.packet(1);
    off_grid.header.timestamp += 100;
    let mut stale = phone.packet(1);
    stale.header.timestamp += 200;
    engine.ingest(off_grid, ms(10));
    engine.ingest(stale, ms(11));
    engine.advance(ms(120));
    assert_eq!(frames.lock().unwrap().len(), 4);
    assert_eq!(engine.stats().decoded, 2, "one packet per slot");
    assert_eq!(
        engine.stats().late,
        1,
        "the second one in the slot is stale"
    );
}

#[test]
fn a_muted_microphone_is_decoded_silence_and_the_stream_keeps_its_cadence() {
    let (mut engine, frames) = engine();
    let mut encoder =
        Encoder::new(SAMPLE_RATE_HZ, Channels::Mono, Application::Voip).expect("encoder");
    for frame in 0..50_u32 {
        let mut payload = vec![0_u8; 400];
        let length = encoder
            .encode(&[0_i16; FRAME_SAMPLES], &mut payload)
            .expect("encode");
        payload.truncate(length);
        let packet = Packet {
            header: Header {
                sequence_number: frame as u16,
                timestamp: frame * FRAME_SAMPLES as u32,
                ..Default::default()
            },
            payload: payload.into(),
        };
        engine.ingest(packet, ms(u64::from(frame) * 20));
    }
    engine.advance(ms(60 + 49 * 20));
    let played = frames.lock().unwrap().clone();
    assert_eq!(played.len(), 50);
    assert!(played.iter().all(|frame| frame.len() == FRAME_SAMPLES));
    let loudest = played
        .iter()
        .map(|frame| frame.iter().map(|s| s.unsigned_abs()).max().unwrap())
        .max()
        .unwrap();
    assert!(
        loudest < 64,
        "decoded mute stays near silence, peak {loudest}"
    );
    assert_eq!(engine.stats().decode_errors, 0);
}

#[test]
fn the_sink_is_told_where_each_frame_came_from() {
    struct Origins(Arc<Mutex<Vec<FrameOrigin>>>);
    impl AudioSink for Origins {
        fn write(&mut self, _pcm: &[i16], origin: FrameOrigin) {
            self.0.lock().unwrap().push(origin);
        }
    }
    let origins = Arc::new(Mutex::new(Vec::new()));
    let mut engine = AudioEngine::new(Box::new(Origins(Arc::clone(&origins)))).unwrap();
    let mut phone = Phone::new();
    engine.ingest(phone.packet(0), ms(0));
    engine.advance(ms(60 + 20 * 6));
    let seen = origins.lock().unwrap().clone();
    assert_eq!(seen[0], FrameOrigin::Decoded);
    assert!(seen[1..6].iter().all(|o| *o == FrameOrigin::Concealed));
    assert_eq!(seen[6], FrameOrigin::Silence);
}
