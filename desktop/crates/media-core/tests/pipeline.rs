//! REC-61: RTP packets to NV12 frames with no network and no real clock. A fake
//! decoder stands in for OpenH264; the worker is joined, never slept on.

use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use remotecam_media_core::decoder::{DecodeError, VideoDecoder};
use remotecam_media_core::pipeline::ElapsedClock;
use remotecam_media_core::recovery::RecoveryRequest;
use remotecam_media_core::{DecodePipeline, FRAME_RING_SLOTS, Nv12Frame};
use rtc::rtp::Packet;
use rtc::rtp::header::Header;

const START_CODE: [u8; 4] = [0, 0, 0, 1];
const FAIL: u8 = 0xEE;
const NEEDS_MORE: u8 = 0xDD;

type Seen = Arc<Mutex<Vec<Vec<u8>>>>;

#[derive(Default)]
struct FakeDecoder {
    seen: Seen,
    gate: Option<Receiver<()>>,
}

impl VideoDecoder for FakeDecoder {
    fn decode(
        &mut self,
        access_unit: &[u8],
        rtp_timestamp: u32,
    ) -> Result<Option<Nv12Frame>, DecodeError> {
        if let Some(gate) = &self.gate {
            let _ = gate.recv();
        }
        self.seen.lock().unwrap().push(access_unit.to_vec());
        match access_unit.get(5) {
            Some(&FAIL) => Err(DecodeError::Decoder("fake failure".to_owned())),
            Some(&NEEDS_MORE) => Ok(None),
            _ => Ok(Some(
                Nv12Frame::new(2, 2, rtp_timestamp, vec![0; 6]).expect("frame"),
            )),
        }
    }
}

fn pipeline(
    gate: Option<Receiver<()>>,
) -> (DecodePipeline, remotecam_media_core::PacketSink, Seen) {
    let seen = Seen::default();
    let decoder = FakeDecoder {
        seen: Arc::clone(&seen),
        gate,
    };
    let (pipeline, sink) = DecodePipeline::new(Box::new(decoder)).expect("pipeline");
    (pipeline, sink, seen)
}

#[derive(Default)]
struct Rtp(u16);

impl Rtp {
    fn next(&mut self, payload: &[u8], marker: bool, timestamp: u32) -> Packet {
        let sequence = self.0;
        self.0 = self.0.wrapping_add(1);
        packet(sequence, payload, marker, timestamp)
    }
}

fn packet(sequence: u16, payload: &[u8], marker: bool, timestamp: u32) -> Packet {
    Packet {
        header: Header {
            sequence_number: sequence,
            marker,
            timestamp,
            ..Default::default()
        },
        payload: payload.to_vec().into(),
    }
}

fn drain(pipeline: &DecodePipeline) {
    pipeline
        .take_worker()
        .expect("worker")
        .join()
        .expect("worker finishes once every sink is dropped");
}

#[test]
fn packets_become_ordered_frames_carrying_their_rtp_timestamps() {
    let (pipeline, mut sink, seen) = pipeline(None);
    let mut rtp = Rtp::default();
    sink.push(&rtp.next(&[0x67, 1, 1], false, 3_000));
    sink.push(&rtp.next(&[0x65, 2, 2], true, 3_000));
    sink.push(&rtp.next(&[0x41, 3, 3], true, 6_000));
    drop(sink);
    drain(&pipeline);

    assert_eq!(pipeline.stats().packets(), 3);
    assert_eq!(pipeline.stats().decoded_frames(), 2);
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        [
            [
                START_CODE.as_slice(),
                &[0x67, 1, 1],
                &START_CODE,
                &[0x65, 2, 2]
            ]
            .concat(),
            [START_CODE.as_slice(), &[0x41, 3, 3]].concat(),
        ]
    );
    assert_eq!(pipeline.ring().pop().expect("first").rtp_timestamp(), 3_000);
    assert_eq!(
        pipeline.ring().pop().expect("second").rtp_timestamp(),
        6_000
    );
    assert!(pipeline.ring().pop().is_none());
}

#[test]
fn decoder_failures_and_incomplete_pictures_do_not_stop_the_stream() {
    let (pipeline, mut sink, _seen) = pipeline(None);
    let mut rtp = Rtp::default();
    sink.push(&rtp.next(&[0x65, FAIL, FAIL], true, 1));
    sink.push(&rtp.next(&[0x65, NEEDS_MORE, NEEDS_MORE], true, 2));
    sink.push(&rtp.next(&[0x65, 7, 7], true, 3));
    drop(sink);
    drain(&pipeline);

    assert_eq!(pipeline.stats().decode_errors(), 1);
    assert_eq!(pipeline.stats().decoded_frames(), 1);
    assert_eq!(pipeline.ring().pop().expect("frame").rtp_timestamp(), 3);
}

#[test]
fn a_lagging_consumer_keeps_the_newest_frames() {
    let (pipeline, mut sink, _seen) = pipeline(None);
    let mut rtp = Rtp::default();
    for timestamp in 1..=5_u32 {
        sink.push(&rtp.next(&[0x65, 1, 1], true, timestamp));
    }
    drop(sink);
    drain(&pipeline);

    assert_eq!(pipeline.stats().decoded_frames(), 5);
    assert_eq!(pipeline.ring().dropped(), 5 - FRAME_RING_SLOTS as u64);
    let kept: Vec<u32> = std::iter::from_fn(|| pipeline.ring().pop())
        .map(|frame| frame.rtp_timestamp())
        .collect();
    assert_eq!(kept, [3, 4, 5]);
}

#[test]
fn a_stalled_decoder_drops_packets_instead_of_blocking_the_network_side() {
    let (release, gate) = channel();
    let (pipeline, mut sink, _seen) = pipeline(Some(gate));
    let mut rtp = Rtp::default();
    let sent = 5_000_u32;
    for timestamp in 0..sent {
        sink.push(&rtp.next(&[0x65, 1, 1], true, timestamp));
    }
    assert!(
        pipeline.stats().packets_dropped() > 0,
        "the bounded packet queue sheds load; push never waits"
    );

    drop(sink);
    for _ in 0..sent {
        let _ = release.send(());
    }
    drain(&pipeline);
    assert!(pipeline.stats().decoded_frames() < u64::from(sent));
}

#[test]
fn packets_arriving_out_of_order_are_decoded_in_sequence_order() {
    let (pipeline, mut sink, seen) = pipeline(None);
    sink.push(&packet(0, &[0x65, 10, 10], true, 0));
    sink.push(&packet(2, &[0x41, 12, 12], true, 6_000));
    sink.push(&packet(1, &[0x41, 11, 11], true, 3_000));
    drop(sink);
    drain(&pipeline);

    let order: Vec<u8> = seen.lock().unwrap().iter().map(|unit| unit[5]).collect();
    assert_eq!(order, [10, 11, 12]);
}

#[test]
fn a_lost_packet_is_nacked_and_counted() {
    let (pipeline, mut sink, _seen) = pipeline(None);
    let mut requests = pipeline.take_recovery_requests().expect("requests");
    sink.push(&packet(0, &[0x65, 1, 1], true, 0));
    sink.push(&packet(2, &[0x41, 2, 2], true, 90_000));
    assert_eq!(
        requests.blocking_recv(),
        Some(RecoveryRequest::Nack(vec![1]))
    );
    drop(sink);
    drain(&pipeline);

    assert_eq!(pipeline.stats().nack_requests(), 1);
    assert_eq!(pipeline.stats().nacked_packets(), 1);
}

#[test]
fn a_decoder_that_lost_its_reference_asks_for_a_keyframe() {
    let (pipeline, mut sink, _seen) = pipeline(None);
    let mut requests = pipeline.take_recovery_requests().expect("requests");
    let mut rtp = Rtp::default();
    sink.push(&rtp.next(&[0x41, FAIL, FAIL], true, 0));
    assert_eq!(requests.blocking_recv(), Some(RecoveryRequest::Pli));
    drop(sink);
    drain(&pipeline);

    assert_eq!(pipeline.stats().plis(), 1);
}

#[test]
fn the_recovery_requests_end_with_the_session() {
    let (pipeline, sink, _seen) = pipeline(None);
    let mut requests = pipeline.take_recovery_requests().expect("requests");
    assert!(pipeline.take_recovery_requests().is_none(), "taken once");
    drop(sink);
    drain(&pipeline);
    assert_eq!(
        requests.blocking_recv(),
        None,
        "the worker released its sender"
    );
}

#[test]
fn a_malformed_payload_discards_the_partial_access_unit() {
    let (pipeline, mut sink, seen) = pipeline(None);
    let mut rtp = Rtp::default();
    sink.push(&rtp.next(&[0x67, 1, 1], false, 1));
    sink.push(&rtp.next(&[0x00], true, 1));
    sink.push(&rtp.next(&[0x65, 9, 9], true, 2));
    drop(sink);
    drain(&pipeline);

    assert_eq!(
        seen.lock().unwrap().as_slice(),
        [[START_CODE.as_slice(), &[0x65, 9, 9]].concat()]
    );
}

#[test]
fn stop_ends_the_worker_even_while_a_sink_is_alive() {
    let (pipeline, sink, _seen) = pipeline(None);
    pipeline.request_stop();
    drain(&pipeline);
    drop(sink);
    assert!(pipeline.take_worker().is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn frame_available_resolves_after_the_first_decoded_frame() {
    let (pipeline, mut sink, _seen) = pipeline(None);
    let mut rtp = Rtp::default();
    sink.push(&rtp.next(&[0x65, 1, 1], true, 11));
    pipeline.frame_available().await;
    assert_eq!(pipeline.ring().pop().expect("frame").rtp_timestamp(), 11);
    drop(sink);
    drain(&pipeline);
}

#[derive(Default)]
struct ManualElapsed(std::sync::atomic::AtomicU64);

impl ManualElapsed {
    fn set_millis(&self, millis: u64) {
        self.0
            .store(millis * 1_000, std::sync::atomic::Ordering::SeqCst);
    }
}

impl ElapsedClock for ManualElapsed {
    fn elapsed(&self) -> std::time::Duration {
        std::time::Duration::from_micros(self.0.load(std::sync::atomic::Ordering::SeqCst))
    }
}

fn jitter_depth_after(arrivals_millis: &[u64]) -> u64 {
    let clock = Arc::new(ManualElapsed::default());
    let (pipeline, mut sink) =
        DecodePipeline::with_clock(Box::new(FakeDecoder::default()), clock.clone())
            .expect("pipeline");
    let mut rtp = Rtp::default();
    for (index, arrival) in arrivals_millis.iter().enumerate() {
        clock.set_millis(*arrival);
        sink.push(&rtp.next(&[0x65, 1, 1], true, index as u32 * 9_000));
    }
    drop(sink);
    drain(&pipeline);
    pipeline.stats().jitter_depth_micros()
}

#[test]
fn the_jitter_depth_follows_the_injected_clock_not_the_wall_clock() {
    assert_eq!(jitter_depth_after(&[0, 100, 200, 300, 400]), 0);
    assert!(jitter_depth_after(&[0, 100, 250, 300, 460]) > 0);
}
