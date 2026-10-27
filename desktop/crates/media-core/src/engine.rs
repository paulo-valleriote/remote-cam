use std::time::Duration;

use rtc::rtp::Packet;
use rtc::rtp::codec::h264::H264Packet;
use rtc::rtp::packetizer::Depacketizer;

use crate::decoder::VideoDecoder;
use crate::jitter_buffer::{JitterBuffer, JitterStats};
use crate::nv12::Nv12Frame;
use crate::recovery::{Recovery, RecoveryRequest, RecoveryStats};

struct AccessUnit {
    rtp_timestamp: u32,
    annex_b: Vec<u8>,
}

struct AccessUnitAssembler {
    depacketizer: H264Packet,
    pending: Vec<u8>,
    pending_timestamp: u32,
}

impl AccessUnitAssembler {
    fn new() -> Self {
        Self {
            depacketizer: H264Packet::default(),
            pending: Vec::new(),
            pending_timestamp: 0,
        }
    }

    fn push(&mut self, packet: &Packet) -> Option<AccessUnit> {
        if packet.header.timestamp != self.pending_timestamp {
            self.pending.clear();
            self.pending_timestamp = packet.header.timestamp;
        }
        match self.depacketizer.depacketize(&packet.payload) {
            Ok(nalus) => self.pending.extend_from_slice(&nalus),
            Err(_) => self.pending.clear(),
        }
        if !packet.header.marker || self.pending.is_empty() {
            return None;
        }
        Some(AccessUnit {
            rtp_timestamp: packet.header.timestamp,
            annex_b: std::mem::take(&mut self.pending),
        })
    }
}

#[derive(Debug, Default)]
pub struct Output {
    pub frames: Vec<Nv12Frame>,
    pub requests: Vec<RecoveryRequest>,
    pub decode_errors: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct EngineStats {
    pub jitter: JitterStats,
    pub recovery: RecoveryStats,
    pub depth: Duration,
}

pub struct ReceiveEngine {
    decoder: Box<dyn VideoDecoder>,
    jitter: JitterBuffer,
    assembler: AccessUnitAssembler,
    recovery: Recovery,
    reference_lost: bool,
    lost_seen: u64,
}

impl ReceiveEngine {
    pub fn new(decoder: Box<dyn VideoDecoder>) -> Self {
        Self {
            decoder,
            jitter: JitterBuffer::new(),
            assembler: AccessUnitAssembler::new(),
            recovery: Recovery::new(),
            reference_lost: false,
            lost_seen: 0,
        }
    }

    pub fn ingest(&mut self, packet: Packet, arrival: Duration) {
        self.jitter.push(packet, arrival);
    }

    pub fn advance(&mut self, now: Duration) -> Output {
        let due = self.jitter.poll(now);
        let mut output = self.release(due);
        let lost = self.jitter.stats().lost;
        if lost > self.lost_seen {
            self.lost_seen = lost;
            self.reference_lost = true;
        }
        let missing = self.jitter.missing();
        output
            .requests
            .extend(self.recovery.poll(&missing, self.reference_lost, now));
        self.reference_lost = false;
        output
    }

    pub fn flush(&mut self) -> Output {
        let held = self.jitter.flush();
        self.release(held)
    }

    pub fn stats(&self) -> EngineStats {
        EngineStats {
            jitter: self.jitter.stats(),
            recovery: self.recovery.stats(),
            depth: self.jitter.target_depth(),
        }
    }

    fn release(&mut self, packets: Vec<Packet>) -> Output {
        let mut output = Output::default();
        for packet in packets {
            let Some(unit) = self.assembler.push(&packet) else {
                continue;
            };
            match self.decoder.decode(&unit.annex_b, unit.rtp_timestamp) {
                Ok(Some(frame)) => {
                    self.reference_lost = false;
                    output.frames.push(frame);
                }
                Ok(None) => {}
                Err(_) => {
                    self.reference_lost = true;
                    output.decode_errors += 1;
                }
            }
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtc::rtp::Packet;
    use rtc::rtp::header::Header;

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

    #[test]
    fn a_single_nal_packet_with_the_marker_closes_an_access_unit() {
        let mut assembler = AccessUnitAssembler::new();
        let unit = assembler
            .push(&packet(&[0x65, 0xAA, 0xBB], true, 9_000))
            .expect("access unit");
        assert_eq!(unit.rtp_timestamp, 9_000);
        assert_eq!(unit.annex_b, [0, 0, 0, 1, 0x65, 0xAA, 0xBB]);
    }

    #[test]
    fn nals_accumulate_until_the_marker_bit() {
        let mut assembler = AccessUnitAssembler::new();
        assert!(assembler.push(&packet(&[0x67, 1, 2], false, 5)).is_none());
        let unit = assembler
            .push(&packet(&[0x65, 3, 4], true, 5))
            .expect("access unit");
        assert_eq!(
            unit.annex_b,
            [0, 0, 0, 1, 0x67, 1, 2, 0, 0, 0, 1, 0x65, 3, 4]
        );
    }

    #[test]
    fn a_fragmented_nal_is_reassembled_across_fu_a_packets() {
        let mut assembler = AccessUnitAssembler::new();
        assert!(
            assembler
                .push(&packet(&[0x7C, 0x85, 1, 2], false, 1))
                .is_none()
        );
        let unit = assembler
            .push(&packet(&[0x7C, 0x45, 3, 4], true, 1))
            .expect("access unit");
        assert_eq!(unit.annex_b, [0, 0, 0, 1, 0x65, 1, 2, 3, 4]);
    }

    #[test]
    fn a_malformed_payload_discards_the_partial_access_unit() {
        let mut assembler = AccessUnitAssembler::new();
        assert!(assembler.push(&packet(&[0x67, 1, 2], false, 1)).is_none());
        assert!(assembler.push(&packet(&[0x00], true, 1)).is_none());
        assert!(
            assembler
                .push(&packet(&[0x65, 9, 9], true, 2))
                .is_some_and(|unit| { unit.annex_b == [0, 0, 0, 1, 0x65, 9, 9] })
        );
    }
}
