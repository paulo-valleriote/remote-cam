use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use remotecam_protocol::ReasonCode;
use rtc::interceptor::{Interceptor, Packet, StreamInfo, TaggedPacket};
use rtc::sansio::Protocol;
use rtc::shared::error::Error;

/// What the desktop sent: control RTCP is allowed, media RTP never is (Receive-only Invariant).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboundKind {
    MediaRtp,
    ControlRtcp,
}

#[derive(Default)]
struct Counts {
    media_rtp: AtomicU64,
    control_rtcp: AtomicU64,
}

/// Counts outbound packets and fails on the first media RTP one.
///
/// Clones share the same counts, so the packet tap and the owner observe one state.
#[derive(Clone, Default)]
pub struct OutboundRtpCounter {
    counts: Arc<Counts>,
}

impl OutboundRtpCounter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one outbound packet. Media RTP is a failure carrying a closed reason code.
    pub fn record(&self, kind: OutboundKind) -> Result<(), ReasonCode> {
        match kind {
            OutboundKind::ControlRtcp => {
                self.counts.control_rtcp.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
            OutboundKind::MediaRtp => {
                self.counts.media_rtp.fetch_add(1, Ordering::SeqCst);
                Err(ReasonCode::SecurityViolation)
            }
        }
    }

    pub fn media_rtp(&self) -> u64 {
        self.counts.media_rtp.load(Ordering::SeqCst)
    }

    pub fn control_rtcp(&self) -> u64 {
        self.counts.control_rtcp.load(Ordering::SeqCst)
    }

    /// The standing verdict: once any media RTP was recorded it never clears.
    pub fn violation(&self) -> Option<ReasonCode> {
        (self.media_rtp() > 0).then_some(ReasonCode::SecurityViolation)
    }
}

/// Interceptor that watches every packet the desktop writes toward the wire.
///
/// Control RTCP passes through. Media RTP is counted as a violation and dropped, so it never
/// leaves the process even if something upstream tried to send it.
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
        match &msg.message.packet {
            Packet::Rtp(_) => {
                // The failure is the standing verdict on the counter; the packet is dropped.
                let _ = self.counter.record(OutboundKind::MediaRtp);
            }
            // An empty compound packet is an attribute carrier that never reaches the wire.
            Packet::Rtcp(packets) if packets.is_empty() => self.write_queue.push_back(msg),
            Packet::Rtcp(_) => {
                let _ = self.counter.record(OutboundKind::ControlRtcp);
                self.write_queue.push_back(msg);
            }
            _ => {
                // A packet kind this tap cannot classify is not allowed to pass.
                let _ = self.counter.record(OutboundKind::MediaRtp);
            }
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
