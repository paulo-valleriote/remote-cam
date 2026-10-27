use std::sync::{Arc, Mutex};
use std::time::Duration;

use remotecam_media_core::Nv12Frame;
use remotecam_media_core::decoder::{DecodeError, VideoDecoder};
use remotecam_media_core::engine::ReceiveEngine;
use rtc::rtp::Packet;
use rtc::rtp::header::Header;

type Seen = Arc<Mutex<Vec<(Vec<u8>, u32)>>>;

struct RecordingDecoder {
    seen: Seen,
}

impl VideoDecoder for RecordingDecoder {
    fn decode(
        &mut self,
        access_unit: &[u8],
        rtp_timestamp: u32,
    ) -> Result<Option<Nv12Frame>, DecodeError> {
        self.seen
            .lock()
            .unwrap()
            .push((access_unit.to_vec(), rtp_timestamp));
        Ok(Some(
            Nv12Frame::new(2, 2, rtp_timestamp, vec![0; 6]).expect("frame"),
        ))
    }
}

fn engine() -> (ReceiveEngine, Seen) {
    let seen = Seen::default();
    let engine = ReceiveEngine::new(Box::new(RecordingDecoder {
        seen: Arc::clone(&seen),
    }));
    (engine, seen)
}

fn packet(sequence: u16, timestamp: u32, marker: bool, nal: u8) -> Packet {
    Packet {
        header: Header {
            sequence_number: sequence,
            timestamp,
            marker,
            ..Default::default()
        },
        payload: vec![0x65, nal, nal].into(),
    }
}

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

#[test]
fn a_lost_marker_packet_never_merges_the_next_frame_into_the_broken_one() {
    let (mut engine, seen) = engine();
    engine.ingest(packet(0, 0, false, 0xA1), ms(0));
    engine.ingest(packet(1, 0, true, 0xA2), ms(0));
    engine.ingest(packet(2, 3_000, false, 0xB1), ms(33));
    engine.ingest(packet(4, 6_000, false, 0xC1), ms(66));
    engine.ingest(packet(5, 6_000, true, 0xC2), ms(66));

    engine.advance(ms(1_000));
    engine.advance(ms(1_200));

    let seen = seen.lock().unwrap();
    let timestamps: Vec<u32> = seen.iter().map(|(_, timestamp)| *timestamp).collect();
    assert_eq!(timestamps, [0, 6_000]);
    assert_eq!(
        seen[1].0,
        [0, 0, 0, 1, 0x65, 0xC1, 0xC1, 0, 0, 0, 1, 0x65, 0xC2, 0xC2]
    );
}

#[test]
fn a_packet_given_up_as_lost_asks_for_a_keyframe_even_when_the_decoder_never_fails() {
    let (mut engine, _seen) = engine();
    engine.ingest(packet(0, 0, true, 0xA1), ms(0));
    engine.ingest(packet(2, 6_000, true, 0xC1), ms(66));

    let mut requested_keyframe = false;
    for now in (100..=400).step_by(10) {
        let output = engine.advance(ms(now));
        requested_keyframe |= output
            .requests
            .contains(&remotecam_media_core::recovery::RecoveryRequest::Pli);
    }

    assert_eq!(engine.stats().jitter.lost, 1);
    assert!(requested_keyframe);
    assert_eq!(engine.stats().recovery.plis, 1);
}
