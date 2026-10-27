//! REC-61: RTP packets to NV12 frames with no network and no real clock. A fake
//! decoder stands in for OpenH264; the worker is joined, never slept on.

use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use remotecam_media_core::decoder::{DecodeError, VideoDecoder};
use remotecam_media_core::{DecodePipeline, FRAME_RING_SLOTS, Nv12Frame};
use rtc::rtp::Packet;
use rtc::rtp::header::Header;

const START_CODE: [u8; 4] = [0, 0, 0, 1];
const FAIL: u8 = 0xEE;
const NEEDS_MORE: u8 = 0xDD;

type Seen = Arc<Mutex<Vec<Vec<u8>>>>;

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

fn packet(payload: &[u8], marker: bool, timestamp: u32) -> Packet {
    Packet {
        header: Header {
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
    sink.push(&packet(&[0x67, 1, 1], false, 3_000));
    sink.push(&packet(&[0x65, 2, 2], true, 3_000));
    sink.push(&packet(&[0x41, 3, 3], true, 6_000));
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
    sink.push(&packet(&[0x65, FAIL, FAIL], true, 1));
    sink.push(&packet(&[0x65, NEEDS_MORE, NEEDS_MORE], true, 2));
    sink.push(&packet(&[0x65, 7, 7], true, 3));
    drop(sink);
    drain(&pipeline);

    assert_eq!(pipeline.stats().decode_errors(), 1);
    assert_eq!(pipeline.stats().decoded_frames(), 1);
    assert_eq!(pipeline.ring().pop().expect("frame").rtp_timestamp(), 3);
}

#[test]
fn a_lagging_consumer_keeps_the_newest_frames() {
    let (pipeline, mut sink, _seen) = pipeline(None);
    for timestamp in 1..=5_u32 {
        sink.push(&packet(&[0x65, 1, 1], true, timestamp));
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
fn a_stalled_decoder_drops_access_units_instead_of_blocking_the_network_side() {
    let (release, gate) = channel();
    let (pipeline, mut sink, _seen) = pipeline(Some(gate));
    let sent = 20_u32;
    for timestamp in 0..sent {
        sink.push(&packet(&[0x65, 1, 1], true, timestamp));
    }
    let dropped = pipeline.stats().access_units_dropped();
    assert!(
        dropped >= u64::from(sent) - 9,
        "queue of 8 plus one in flight"
    );

    drop(sink);
    for _ in 0..sent {
        let _ = release.send(());
    }
    drain(&pipeline);
    assert_eq!(
        pipeline.stats().decoded_frames() + pipeline.stats().access_units_dropped(),
        u64::from(sent)
    );
}

#[test]
fn a_malformed_payload_discards_the_partial_access_unit() {
    let (pipeline, mut sink, seen) = pipeline(None);
    sink.push(&packet(&[0x67, 1, 1], false, 1));
    sink.push(&packet(&[0x00], true, 1));
    sink.push(&packet(&[0x65, 9, 9], true, 2));
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
    sink.push(&packet(&[0x65, 1, 1], true, 11));
    pipeline.frame_available().await;
    assert_eq!(pipeline.ring().pop().expect("frame").rtp_timestamp(), 11);
    drop(sink);
    drain(&pipeline);
}
