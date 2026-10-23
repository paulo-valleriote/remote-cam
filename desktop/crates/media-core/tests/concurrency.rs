//! T063 — producer/consumer ordering, event idempotency and session replacement.

use std::sync::Arc;
use std::thread;

use remotecam_injection::Frame;
use remotecam_media_core::handoff::{FrameHandoff, HandoffError, SessionEvent, SessionEvents};

fn frame(sequence: u64) -> Frame {
    let mut data = vec![0x80_u8; 4 * 4 + (4 * 4) / 2];
    data[..4].copy_from_slice(&(sequence as u32).to_le_bytes());
    Frame::new_nv12(4, 4, 4, sequence * 33_000_000, data).expect("valid frame")
}

#[test]
fn consumer_sees_frames_in_publication_order() {
    let handoff = Arc::new(FrameHandoff::new(64).expect("handoff"));
    let (epoch, mut reader) = handoff.reader();

    let producer = {
        let handoff = Arc::clone(&handoff);
        thread::spawn(move || {
            for index in 1..=20 {
                handoff.publish(epoch, frame(index)).expect("publish");
            }
        })
    };

    let mut sequences = Vec::new();
    while sequences.len() < 20 {
        let frame = reader.next().expect("frame");
        sequences.push(frame.sequence);
    }
    producer.join().expect("producer thread");

    assert_eq!(sequences, (1..=20).collect::<Vec<_>>());
    assert_eq!(handoff.stats().published, 20);
}

#[test]
fn duplicated_session_events_are_applied_once() {
    let events = SessionEvents::new();
    assert!(events.apply(SessionEvent::Ended { epoch: 1 }));
    assert!(!events.apply(SessionEvent::Ended { epoch: 1 }));
    assert!(events.apply(SessionEvent::Released { epoch: 1 }));
    assert!(events.apply(SessionEvent::Ended { epoch: 2 }));
    assert_eq!(events.applied_count(), 3);
}

#[test]
fn a_stale_epoch_is_discarded_and_counted() {
    let handoff = FrameHandoff::new(8).expect("handoff");
    let epoch = handoff.epoch();
    let replacement = handoff.replace_session();

    assert_eq!(replacement.current_epoch, epoch + 1);
    let error = handoff
        .publish(epoch, frame(1))
        .expect_err("stale epoch must be rejected");
    assert!(matches!(error, HandoffError::StaleEpoch { .. }));

    let stats = handoff.stats();
    assert_eq!(stats.stale_discarded, 1);
    assert_eq!(stats.session_replacements, 1);
    assert_eq!(stats.current_epoch, epoch + 1);
}

#[test]
fn session_replacement_releases_pending_frames_and_never_overlaps_sessions() {
    let handoff = FrameHandoff::new(8).expect("handoff");
    let (old_epoch, old_reader) = handoff.reader();
    for index in 1..=3 {
        handoff.publish(old_epoch, frame(index)).expect("publish");
    }

    let replacement = handoff.replace_session();
    assert_eq!(replacement.discarded_pending, 3);
    assert_eq!(handoff.stats().discarded, 3);

    // The old session's reader is closed, not handed new-session frames.
    let mut old_reader = old_reader;
    assert!(old_reader.next().is_none());

    let (new_epoch, mut new_reader) = handoff.reader();
    assert_eq!(new_epoch, replacement.current_epoch);
    handoff.publish(new_epoch, frame(99)).expect("publish");
    let received = new_reader.next().expect("new session frame");
    assert_eq!(
        received.sequence, 1,
        "the new buffer starts its own sequence"
    );
}

#[test]
fn replacing_a_session_twice_keeps_the_epoch_monotonic() {
    let handoff = FrameHandoff::new(4).expect("handoff");
    let first = handoff.replace_session();
    let second = handoff.replace_session();
    assert_eq!(second.previous_epoch, first.current_epoch);
    assert!(second.current_epoch > first.current_epoch);
}
