use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use remotecam_protocol::ReasonCode;
use rtc::interceptor::{Interceptor, Packet, StreamInfo, TaggedPacket};
use rtc::sansio::Protocol;
use rtc::shared::error::Error;
use tokio::sync::Notify;

const RECEIVE_ONLY_VIOLATION: ReasonCode = ReasonCode::SecurityViolation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboundKind {
    MediaRtp,
    ControlRtcp,
    Unclassified,
}

#[derive(Default)]
struct Counts {
    media_rtp: AtomicU64,
    control_rtcp: AtomicU64,
    unclassified: AtomicU64,
    violated: Notify,
}

#[derive(Clone, Default)]
pub struct OutboundRtpCounter {
    counts: Arc<Counts>,
}

impl OutboundRtpCounter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&self, kind: OutboundKind) -> Result<(), ReasonCode> {
        let forbidden = match kind {
            OutboundKind::ControlRtcp => {
                self.counts.control_rtcp.fetch_add(1, Ordering::SeqCst);
                return Ok(());
            }
            OutboundKind::MediaRtp => &self.counts.media_rtp,
            OutboundKind::Unclassified => &self.counts.unclassified,
        };
        forbidden.fetch_add(1, Ordering::SeqCst);
        self.counts.violated.notify_waiters();
        Err(RECEIVE_ONLY_VIOLATION)
    }

    pub fn media_rtp(&self) -> u64 {
        self.counts.media_rtp.load(Ordering::SeqCst)
    }

    pub fn control_rtcp(&self) -> u64 {
        self.counts.control_rtcp.load(Ordering::SeqCst)
    }

    pub fn unclassified(&self) -> u64 {
        self.counts.unclassified.load(Ordering::SeqCst)
    }

    pub fn violation(&self) -> Option<ReasonCode> {
        (self.media_rtp() + self.unclassified() > 0).then_some(RECEIVE_ONLY_VIOLATION)
    }

    pub async fn violated(&self) -> ReasonCode {
        loop {
            let notified = self.counts.violated.notified();
            if let Some(reason) = self.violation() {
                return reason;
            }
            notified.await;
        }
    }
}

fn is_attribute_carrier(packets: &[Box<dyn rtc::rtcp::Packet>]) -> bool {
    packets.is_empty()
}

fn classify(packet: &Packet) -> Option<OutboundKind> {
    match packet {
        Packet::Rtp(_) => Some(OutboundKind::MediaRtp),
        Packet::Rtcp(packets) if is_attribute_carrier(packets) => None,
        Packet::Rtcp(_) => Some(OutboundKind::ControlRtcp),
        _ => Some(OutboundKind::Unclassified),
    }
}

pub struct OutboundRtpTap {
    counter: OutboundRtpCounter,
    read_queue: VecDeque<TaggedPacket>,
    write_queue: VecDeque<TaggedPacket>,
}

impl OutboundRtpTap {
    pub fn new(counter: OutboundRtpCounter) -> Self {
        Self {
            counter,
            read_queue: VecDeque::new(),
            write_queue: VecDeque::new(),
        }
    }
}

impl Protocol<TaggedPacket, TaggedPacket, ()> for OutboundRtpTap {
    type Rout = TaggedPacket;
    type Wout = TaggedPacket;
    type Eout = ();
    type Error = Error;
    type Time = Instant;

    fn handle_read(&mut self, msg: TaggedPacket) -> Result<(), Self::Error> {
        self.read_queue.push_back(msg);
        Ok(())
    }

    fn poll_read(&mut self) -> Option<Self::Rout> {
        self.read_queue.pop_front()
    }

    fn handle_write(&mut self, msg: TaggedPacket) -> Result<(), Self::Error> {
        let allowed =
            classify(&msg.message.packet).map_or(Ok(()), |kind| self.counter.record(kind));
        if allowed.is_ok() {
            self.write_queue.push_back(msg);
        }
        Ok(())
    }

    fn poll_write(&mut self) -> Option<Self::Wout> {
        self.write_queue.pop_front()
    }
}

impl Interceptor for OutboundRtpTap {
    fn bind_local_stream(&mut self, _info: &StreamInfo) {}
    fn unbind_local_stream(&mut self, _info: &StreamInfo) {}
    fn bind_remote_stream(&mut self, _info: &StreamInfo) {}
    fn unbind_remote_stream(&mut self, _info: &StreamInfo) {}
}
