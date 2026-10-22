//! Contract tests for the NV12 ring buffer (T026).
//!
//! The ring buffer is the only handoff between the network/media path and the
//! virtual camera. Its contract is deliberately small: a single writer, any
//! number of readers, newest-frame-wins when full, and a producer that is never
//! blocked by a slow or absent consumer.

use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use remotecam_injection::ring_buffer::{Frame, FrameError, RingBuffer, RingBufferError};

fn nv12(width: u32, height: u32, fill: u8, timestamp_ns: u64) -> Frame {
    let stride = width as usize;
    let len = Frame::expected_len(width, height, stride).expect("valid dimensions");
    Frame::new_nv12(width, height, stride, timestamp_ns, vec![fill; len]).expect("valid NV12 frame")
}

#[test]
fn newest_frame_overwrites_the_oldest_when_full() {
    let buffer = RingBuffer::new(2).expect("capacity is positive");
    let writer = buffer.writer();
    let mut reader = buffer.reader();

    for index in 1..=3 {
        writer
            .publish(nv12(640, 480, index as u8, index as u64))
            .expect("write accepted");
    }

    let stats = buffer.stats();
    assert_eq!(stats.depth, 2, "the buffer keeps exactly its capacity");
    assert_eq!(stats.published, 3);
    assert_eq!(stats.discarded, 1, "the oldest frame was evicted");

    let latest = reader.latest().expect("the newest frame is retained");
    assert_eq!(latest.sequence, 3);
    assert_eq!(latest.data[0], 3, "newest frame content wins");
}

#[test]
fn publish_sequence_is_monotonic_across_evictions() {
    let buffer = RingBuffer::new(1).expect("capacity is positive");
    let writer = buffer.writer();

    let mut sequences = Vec::new();
    for index in 0..64u64 {
        let published = writer
            .publish(nv12(320, 240, index as u8, index))
            .expect("write accepted");
        sequences.push(published.sequence);
    }

    assert!(
        sequences.windows(2).all(|pair| pair[1] == pair[0] + 1),
        "every published frame receives the next sequence number: {sequences:?}"
    );
    assert_eq!(buffer.stats().published, 64);
    assert_eq!(buffer.stats().discarded, 63);
}

#[test]
fn discarded_frame_count_tracks_every_overwritten_frame() {
    let buffer = RingBuffer::new(3).expect("capacity is positive");
    let writer = buffer.writer();

    for index in 0..10u64 {
        writer
            .publish(nv12(1280, 720, index as u8, index))
            .expect("write accepted");
    }

    assert_eq!(buffer.stats().discarded, 7);
    assert_eq!(buffer.stats().depth, 3);
}

#[test]
fn the_producer_is_never_blocked_by_an_absent_consumer() {
    let buffer = RingBuffer::new(2).expect("capacity is positive");
    let writer = buffer.writer();

    // No reader is ever created. A blocking implementation would deadlock here.
    let started = std::time::Instant::now();
    for index in 0..5_000u64 {
        writer
            .publish(nv12(1280, 720, index as u8, index))
            .expect("write accepted");
    }

    assert_eq!(buffer.stats().published, 5_000);
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "publishing 5 000 frames must not wait for a consumer"
    );
}

#[test]
fn readers_observe_every_retained_frame_in_order() {
    let buffer = RingBuffer::new(4).expect("capacity is positive");
    let writer = buffer.writer();
    let mut first = buffer.reader();
    let mut second = buffer.reader();

    for index in 1..=4u64 {
        writer
            .publish(nv12(640, 480, index as u8, index))
            .expect("write accepted");
    }

    let first_read: Vec<u64> = std::iter::from_fn(|| first.try_next())
        .map(|frame| frame.sequence)
        .collect();
    let second_read: Vec<u64> = std::iter::from_fn(|| second.try_next())
        .map(|frame| frame.sequence)
        .collect();

    assert_eq!(first_read, vec![1, 2, 3, 4]);
    assert_eq!(
        second_read,
        vec![1, 2, 3, 4],
        "each reader keeps its own cursor"
    );
}

#[test]
fn a_blocked_reader_wakes_when_a_frame_is_published() {
    let buffer = Arc::new(RingBuffer::new(2).expect("capacity is positive"));
    let writer = buffer.writer();
    let mut reader = buffer.reader();
    let (sender, receiver) = mpsc::channel();

    let handle = thread::spawn(move || {
        let frame = reader.next().expect("a frame arrives before shutdown");
        sender.send(frame.sequence).expect("test channel alive");
    });

    thread::sleep(Duration::from_millis(20));
    writer
        .publish(nv12(640, 480, 7, 7))
        .expect("write accepted");

    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(2)),
        Ok(1),
        "the waiting reader receives the published frame"
    );
    handle.join().expect("reader thread exits");
}

#[test]
fn shutdown_releases_queued_frames_and_wakes_waiting_readers() {
    let buffer = Arc::new(RingBuffer::new(4).expect("capacity is positive"));
    let writer = buffer.writer();
    let mut reader = buffer.reader();

    for index in 1..=3u64 {
        writer
            .publish(nv12(640, 480, index as u8, index))
            .expect("write accepted");
    }
    assert_eq!(buffer.stats().depth, 3);

    let (sender, receiver) = mpsc::channel();
    let waiter = buffer.reader();
    let handle = thread::spawn(move || {
        let mut waiter = waiter;
        sender
            .send(waiter.next().is_none())
            .expect("test channel alive");
    });

    buffer.shutdown();

    assert!(buffer.is_shutdown());
    assert_eq!(
        buffer.stats().depth,
        0,
        "shutdown releases every queued frame instead of leaking the buffer"
    );
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(2)),
        Ok(true),
        "a waiting reader is woken and observes closure"
    );
    assert!(
        reader.next().is_none(),
        "draining returns nothing after close"
    );
    assert!(
        writer.publish(nv12(640, 480, 9, 9)).is_none(),
        "the producer is refused after close"
    );

    handle.join().expect("waiter thread exits");
}

#[test]
fn a_timed_read_reports_a_timeout_without_closing_the_buffer() {
    let buffer = RingBuffer::new(1).expect("capacity is positive");
    let mut reader = buffer.reader();

    assert!(reader.next_timeout(Duration::from_millis(10)).is_err());
    assert!(!buffer.is_shutdown());

    buffer.shutdown();
    assert_eq!(reader.next_timeout(Duration::from_millis(10)), Ok(None));
}

#[test]
fn shutdown_is_idempotent_and_safe_before_any_publish() {
    let buffer = RingBuffer::new(1).expect("capacity is positive");

    buffer.shutdown();
    buffer.shutdown();

    assert!(buffer.is_shutdown());
    assert_eq!(buffer.stats().depth, 0);
}

#[test]
fn malformed_frames_are_rejected_before_they_enter_the_buffer() {
    assert_eq!(
        Frame::new_nv12(0, 480, 0, 0, Vec::new()),
        Err(FrameError::ZeroDimension)
    );
    assert_eq!(
        Frame::new_nv12(641, 480, 641, 0, vec![0; 641 * 480 * 3 / 2]),
        Err(FrameError::OddDimension {
            width: 641,
            height: 480
        })
    );
    assert_eq!(
        Frame::new_nv12(640, 480, 320, 0, vec![0; 640 * 480 * 3 / 2]),
        Err(FrameError::StrideTooSmall {
            stride: 320,
            width: 640
        })
    );
    assert_eq!(
        Frame::new_nv12(640, 480, 640, 0, vec![0; 16]),
        Err(FrameError::WrongLength {
            expected: 640 * 480 * 3 / 2,
            actual: 16
        })
    );
}

#[test]
fn a_zero_capacity_buffer_is_refused() {
    assert_eq!(
        RingBuffer::new(0).err(),
        Some(RingBufferError::ZeroCapacity)
    );
}

#[test]
fn a_stride_larger_than_the_width_is_accepted_and_planes_are_exposed() {
    let width = 640u32;
    let height = 480u32;
    let stride = 704usize;
    let len = Frame::expected_len(width, height, stride).expect("valid dimensions");
    let frame = Frame::new_nv12(width, height, stride, 42, vec![0x11; len]).expect("valid frame");

    assert_eq!(frame.luma().len(), stride * height as usize);
    assert_eq!(frame.chroma().len(), stride * height as usize / 2);
    assert_eq!(frame.timestamp_ns, 42);
    assert_eq!(frame.sequence, 0, "the buffer assigns the sequence");
}
