use std::time::Instant;

use remotecam_media_core::{OutboundRtpCounter, OutboundRtpTap};
use remotecam_protocol::ReasonCode;
use rtc::interceptor::{AttributedPacket, Packet, TaggedPacket};
use rtc::sansio::Protocol;
use rtc::shared::TransportContext;

fn tagged(packet: Packet) -> TaggedPacket {
    TaggedPacket {
        now: Instant::now(),
        transport: TransportContext::default(),
        message: AttributedPacket::new(packet),
    }
}

fn media() -> TaggedPacket {
    tagged(Packet::Rtp(rtc::rtp::Packet::default()))
}

fn control() -> TaggedPacket {
    let report: Box<dyn rtc::rtcp::Packet> =
        Box::new(rtc::rtcp::receiver_report::ReceiverReport::default());
    tagged(Packet::Rtcp(vec![report]))
}

#[test]
fn control_rtcp_passes_through_and_is_counted_as_control() {
    let counter = OutboundRtpCounter::new();
    let mut tap = OutboundRtpTap::new(counter.clone());

    tap.handle_write(control()).unwrap();

    assert!(tap.poll_write().is_some());
    assert_eq!(counter.control_rtcp(), 1);
    assert_eq!(counter.violation(), None);
}

#[test]
fn media_rtp_is_a_violation_and_never_reaches_the_wire() {
    let counter = OutboundRtpCounter::new();
    let mut tap = OutboundRtpTap::new(counter.clone());

    tap.handle_write(media()).unwrap();

    assert!(tap.poll_write().is_none());
    assert_eq!(counter.media_rtp(), 1);
    assert_eq!(counter.violation(), Some(ReasonCode::SecurityViolation));
}

#[test]
fn inbound_packets_are_forwarded_untouched_and_not_counted() {
    let counter = OutboundRtpCounter::new();
    let mut tap = OutboundRtpTap::new(counter.clone());

    tap.handle_read(media()).unwrap();
    tap.handle_read(control()).unwrap();

    assert!(tap.poll_read().is_some());
    assert!(tap.poll_read().is_some());
    assert!(tap.poll_read().is_none());
    assert_eq!(counter.media_rtp() + counter.control_rtcp(), 0);
}

#[test]
fn an_empty_attribute_carrier_is_not_a_wire_packet_and_is_not_counted() {
    let counter = OutboundRtpCounter::new();
    let mut tap = OutboundRtpTap::new(counter.clone());

    tap.handle_write(tagged(Packet::Rtcp(vec![]))).unwrap();

    assert!(tap.poll_write().is_some());
    assert_eq!(counter.control_rtcp(), 0);
}

#[test]
fn order_is_preserved_across_a_mixed_write_burst() {
    let counter = OutboundRtpCounter::new();
    let mut tap = OutboundRtpTap::new(counter.clone());

    tap.handle_write(control()).unwrap();
    tap.handle_write(media()).unwrap();
    tap.handle_write(control()).unwrap();

    let forwarded: Vec<_> = std::iter::from_fn(|| tap.poll_write()).collect();
    assert_eq!(forwarded.len(), 2);
    assert!(
        forwarded
            .iter()
            .all(|out| matches!(out.message.packet, Packet::Rtcp(_)))
    );
    assert_eq!(counter.control_rtcp(), 2);
    assert_eq!(counter.media_rtp(), 1);
}

#[test]
fn taps_on_many_threads_share_one_verdict_and_forward_only_control() {
    const THREADS: usize = 8;
    const PER_THREAD: usize = 500;
    let counter = OutboundRtpCounter::new();

    let workers: Vec<_> = (0..THREADS)
        .map(|_| {
            let counter = counter.clone();
            std::thread::spawn(move || {
                let mut tap = OutboundRtpTap::new(counter);
                let mut forwarded = 0;
                for _ in 0..PER_THREAD {
                    tap.handle_write(control()).unwrap();
                    tap.handle_write(media()).unwrap();
                    while let Some(out) = tap.poll_write() {
                        assert!(matches!(out.message.packet, Packet::Rtcp(_)));
                        forwarded += 1;
                    }
                }
                forwarded
            })
        })
        .collect();
    let forwarded: usize = workers.into_iter().map(|w| w.join().unwrap()).sum();

    assert_eq!(forwarded, THREADS * PER_THREAD);
    assert_eq!(counter.control_rtcp(), (THREADS * PER_THREAD) as u64);
    assert_eq!(counter.media_rtp(), (THREADS * PER_THREAD) as u64);
    assert_eq!(counter.violation(), Some(ReasonCode::SecurityViolation));
}

#[test]
fn a_violation_precedes_every_later_packet_in_event_order() {
    let counter = OutboundRtpCounter::new();
    let mut tap = OutboundRtpTap::new(counter.clone());

    tap.handle_write(control()).unwrap();
    assert_eq!(counter.violation(), None);
    tap.handle_write(media()).unwrap();
    assert_eq!(counter.violation(), Some(ReasonCode::SecurityViolation));
    tap.handle_write(control()).unwrap();

    assert_eq!(counter.violation(), Some(ReasonCode::SecurityViolation));
    assert_eq!((counter.control_rtcp(), counter.media_rtp()), (2, 1));
}

#[test]
fn the_registry_media_core_uses_catches_media_rtp_written_through_its_chain() {
    let counter = OutboundRtpCounter::new();
    let mut engine = webrtc::peer_connection::MediaEngine::default();
    engine.register_default_codecs().unwrap();
    let mut chain = remotecam_media_core::receive_only_registry(&mut engine, &counter)
        .unwrap()
        .build();

    chain.handle_write(media()).unwrap();
    chain.handle_write(control()).unwrap();

    let leaving: Vec<_> = std::iter::from_fn(|| chain.poll_write()).collect();
    assert!(
        leaving
            .iter()
            .all(|out| !matches!(out.message.packet, Packet::Rtp(_))),
        "no media RTP leaves the real chain"
    );
    assert_eq!(counter.media_rtp(), 1);
    assert_eq!(counter.violation(), Some(ReasonCode::SecurityViolation));
}
