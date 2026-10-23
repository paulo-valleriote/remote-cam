//! T064 — teardown releases frames, closes handles and joins threads.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use remotecam_injection::Frame;
use remotecam_media_core::handoff::FrameHandoff;

fn frame() -> Frame {
    Frame::new_nv12(4, 4, 4, 0, vec![0x80; 4 * 4 + 8]).expect("valid frame")
}

#[test]
fn shutdown_wakes_a_blocked_reader_and_releases_pending_frames() {
    let handoff = Arc::new(FrameHandoff::new(8).expect("handoff"));
    let (epoch, mut reader) = handoff.reader();
    handoff.publish(epoch, frame()).expect("publish");

    let consumer = {
        let handoff = Arc::clone(&handoff);
        thread::spawn(move || {
            let _ = reader.next();
            handoff.shutdown();
            // A second call on a closed handoff must not block.
            assert!(reader.next().is_none());
        })
    };

    consumer.join().expect("consumer thread");
    assert_eq!(handoff.stats().buffer.depth, 0);
    assert!(handoff.stats().buffer.published >= 1);
}

#[test]
fn shutdown_is_idempotent_and_further_publications_fail() {
    let handoff = FrameHandoff::new(4).expect("handoff");
    let epoch = handoff.epoch();
    handoff.shutdown();
    handoff.shutdown();
    assert!(handoff.publish(epoch, frame()).is_err());
}

#[test]
fn a_producer_thread_joins_after_shutdown() {
    let handoff = Arc::new(FrameHandoff::new(32).expect("handoff"));
    let producer = {
        let handoff = Arc::clone(&handoff);
        thread::spawn(move || {
            let epoch = handoff.epoch();
            for _ in 0..100 {
                if handoff.publish(epoch, frame()).is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(1));
            }
        })
    };

    thread::sleep(Duration::from_millis(10));
    handoff.shutdown();
    producer.join().expect("producer must stop after shutdown");
    assert_eq!(handoff.stats().buffer.depth, 0);
}

#[cfg(not(windows))]
#[test]
fn the_software_decoder_is_labelled_non_product() {
    use remotecam_media_core::adapters::software_decoder::SoftwareH264Decoder;

    let decoder = SoftwareH264Decoder::new().expect("openh264 decoder");
    assert!(
        !decoder.citation_allowed,
        "the software decoder must never be cited for the latency gate"
    );
}

#[cfg(windows)]
#[test]
fn the_hardware_decoder_reports_its_precondition() {
    use remotecam_media_core::adapters::mf_decoder::verify_h264_hardware_decode;

    let verification = verify_h264_hardware_decode().expect("enumeration must not fail");
    // The result is evidence, not a test precondition: CI has no GPU.
    assert_eq!(verification.discovery_method, "mft_enum_syncmft");
    assert!(verification.sync_mft_count >= verification.hardware_mft_count);
}
