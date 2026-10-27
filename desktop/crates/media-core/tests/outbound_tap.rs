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
