//! Packets in, NV12 frames out, with no network: depacketize RTP into access
//! units, decode them on a dedicated thread and publish into the Frame Ring.
//! The network task only calls [`PacketSink::push`]; it never decodes and never
//! waits for the consumer (a full queue drops the access unit).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use rtc::rtp::codec::h264::H264Packet;
use rtc::rtp::packetizer::Depacketizer;
use tokio::sync::Notify;

use crate::core::MediaCoreError;
use crate::decoder::VideoDecoder;
use crate::frame_ring::FrameRing;
use crate::nv12::Nv12Frame;

const ACCESS_UNIT_QUEUE: usize = 8;
const DECODE_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Default)]
pub struct MediaStats {
    packets: AtomicU64,
    access_units_dropped: AtomicU64,
    decoded_frames: AtomicU64,
    decode_errors: AtomicU64,
}

impl MediaStats {
    pub fn packets(&self) -> u64 {
        self.packets.load(Ordering::Relaxed)
    }

    pub fn access_units_dropped(&self) -> u64 {
        self.access_units_dropped.load(Ordering::Relaxed)
    }

    pub fn decoded_frames(&self) -> u64 {
        self.decoded_frames.load(Ordering::Relaxed)
    }

    pub fn decode_errors(&self) -> u64 {
        self.decode_errors.load(Ordering::Relaxed)
    }
}

struct AccessUnit {
    rtp_timestamp: u32,
    annex_b: Vec<u8>,
}

struct AccessUnitAssembler {
    depacketizer: H264Packet,
    pending: Vec<u8>,
}

impl AccessUnitAssembler {
    fn new() -> Self {
        Self {
            depacketizer: H264Packet::default(),
            pending: Vec::new(),
        }
    }

    fn push(&mut self, packet: &rtc::rtp::Packet) -> Option<AccessUnit> {
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

struct Shared {
    ring: FrameRing<Nv12Frame>,
    stats: MediaStats,
    arrived: Notify,
}

/// The network side of the pipeline.
pub struct PacketSink {
    assembler: AccessUnitAssembler,
    access_units: SyncSender<AccessUnit>,
    shared: Arc<Shared>,
}

impl PacketSink {
    pub fn push(&mut self, packet: &rtc::rtp::Packet) {
        self.shared.stats.packets.fetch_add(1, Ordering::Relaxed);
        if let Some(unit) = self.assembler.push(packet)
            && self.access_units.try_send(unit).is_err()
        {
            self.shared
                .stats
                .access_units_dropped
                .fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// The decode side: owns the worker thread, the ring and the counters.
pub struct DecodePipeline {
    shared: Arc<Shared>,
    stop_requested: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

fn decode_loop(
    mut decoder: Box<dyn VideoDecoder>,
    queue: Receiver<AccessUnit>,
    shared: Arc<Shared>,
    stop_requested: Arc<AtomicBool>,
) {
    while !stop_requested.load(Ordering::Acquire) {
        let unit = match queue.recv_timeout(DECODE_POLL_INTERVAL) {
            Ok(unit) => unit,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        match decoder.decode(&unit.annex_b, unit.rtp_timestamp) {
            Ok(Some(frame)) => {
                shared.stats.decoded_frames.fetch_add(1, Ordering::Relaxed);
                shared.ring.push(frame);
                shared.arrived.notify_one();
            }
            Ok(None) => {}
            Err(_) => {
                shared.stats.decode_errors.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

impl DecodePipeline {
    pub fn new(decoder: Box<dyn VideoDecoder>) -> Result<(Self, PacketSink), MediaCoreError> {
        let shared = Arc::new(Shared {
            ring: FrameRing::new(),
            stats: MediaStats::default(),
            arrived: Notify::new(),
        });
        let stop_requested = Arc::new(AtomicBool::new(false));
        let (access_units, queue) = sync_channel(ACCESS_UNIT_QUEUE);
        let worker = std::thread::Builder::new()
            .name("media-core-decode".to_owned())
            .spawn({
                let shared = Arc::clone(&shared);
                let stop_requested = Arc::clone(&stop_requested);
                move || decode_loop(decoder, queue, shared, stop_requested)
            })
            .map_err(|error| MediaCoreError::Decoder(error.to_string()))?;
        let sink = PacketSink {
            assembler: AccessUnitAssembler::new(),
            access_units,
            shared: Arc::clone(&shared),
        };
        Ok((
            Self {
                shared,
                stop_requested,
                worker: Mutex::new(Some(worker)),
            },
            sink,
        ))
    }

    pub fn ring(&self) -> &FrameRing<Nv12Frame> {
        &self.shared.ring
    }

    pub fn stats(&self) -> &MediaStats {
        &self.shared.stats
    }

    /// Resolves once the ring holds a frame.
    pub async fn frame_available(&self) {
        while self.shared.ring.is_empty() {
            self.shared.arrived.notified().await;
        }
    }

    pub fn request_stop(&self) {
        self.stop_requested.store(true, Ordering::Release);
    }

    /// The worker handle, once. Without `request_stop` the worker finishes after
    /// it has decoded everything queued and every [`PacketSink`] is dropped.
    pub fn take_worker(&self) -> Option<JoinHandle<()>> {
        self.worker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
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

    #[test]
    fn stats_start_at_zero() {
        let stats = MediaStats::default();
        assert_eq!(
            (
                stats.packets(),
                stats.access_units_dropped(),
                stats.decoded_frames(),
                stats.decode_errors()
            ),
            (0, 0, 0, 0)
        );
    }
}
