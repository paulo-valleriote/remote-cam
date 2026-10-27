use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tokio::sync::Notify;
use tokio::sync::mpsc::{Receiver as RequestReceiver, Sender as RequestSender};

use crate::core::MediaCoreError;
use crate::decoder::VideoDecoder;
use crate::engine::{EngineStats, Output, ReceiveEngine};
use crate::frame_ring::FrameRing;
use crate::nv12::Nv12Frame;
use crate::recovery::RecoveryRequest;

const PACKET_QUEUE: usize = 1_024;
const RECOVERY_REQUEST_QUEUE: usize = 32;
const WORKER_TICK: Duration = Duration::from_millis(2);

macro_rules! counters {
    ($($field:ident),+ $(,)?) => {
        #[derive(Debug, Default)]
        pub struct MediaStats {
            $($field: AtomicU64,)+
        }

        impl MediaStats {
            $(pub fn $field(&self) -> u64 {
                self.$field.load(Ordering::Relaxed)
            })+
        }
    };
}

counters!(
    packets,
    packets_dropped,
    packets_lost,
    packets_late,
    decoded_frames,
    decode_errors,
    nack_requests,
    nacked_packets,
    plis,
    jitter_depth_micros,
);

struct Shared {
    ring: FrameRing<Nv12Frame>,
    stats: MediaStats,
    arrived: Notify,
}

pub trait ElapsedClock: Send + Sync {
    fn elapsed(&self) -> Duration;
}

pub struct SystemElapsed {
    origin: Instant,
}

impl SystemElapsed {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemElapsed {
    fn default() -> Self {
        Self::new()
    }
}

impl ElapsedClock for SystemElapsed {
    fn elapsed(&self) -> Duration {
        self.origin.elapsed()
    }
}

/// The network side of the pipeline.
pub struct PacketSink {
    packets: SyncSender<(rtc::rtp::Packet, Duration)>,
    clock: Arc<dyn ElapsedClock>,
    shared: Arc<Shared>,
}

impl PacketSink {
    pub fn push(&mut self, packet: &rtc::rtp::Packet) {
        self.shared.stats.packets.fetch_add(1, Ordering::Relaxed);
        if self
            .packets
            .try_send((packet.clone(), self.clock.elapsed()))
            .is_err()
        {
            self.shared
                .stats
                .packets_dropped
                .fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// The decode side: owns the worker thread, the ring and the counters.
pub struct DecodePipeline {
    shared: Arc<Shared>,
    stop_requested: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
    requests: Mutex<Option<RequestReceiver<RecoveryRequest>>>,
}

struct Worker {
    engine: ReceiveEngine,
    shared: Arc<Shared>,
    requests: RequestSender<RecoveryRequest>,
}

impl Worker {
    fn deliver(&self, output: Output) {
        let stats = &self.shared.stats;
        stats
            .decode_errors
            .fetch_add(output.decode_errors, Ordering::Relaxed);
        for frame in output.frames {
            stats.decoded_frames.fetch_add(1, Ordering::Relaxed);
            self.shared.ring.push(frame);
            self.shared.arrived.notify_one();
        }
        for request in output.requests {
            let _ = self.requests.try_send(request);
        }
        self.publish_stats();
    }

    fn publish_stats(&self) {
        let stats = &self.shared.stats;
        let EngineStats {
            jitter,
            recovery,
            depth,
        } = self.engine.stats();
        stats.packets_lost.store(jitter.lost, Ordering::Relaxed);
        stats.packets_late.store(jitter.late, Ordering::Relaxed);
        stats
            .nack_requests
            .store(recovery.nack_requests, Ordering::Relaxed);
        stats
            .nacked_packets
            .store(recovery.nacked_packets, Ordering::Relaxed);
        stats.plis.store(recovery.plis, Ordering::Relaxed);
        stats.jitter_depth_micros.store(
            u64::try_from(depth.as_micros()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
    }
}

fn decode_loop(
    mut worker: Worker,
    queue: Receiver<(rtc::rtp::Packet, Duration)>,
    clock: Arc<dyn ElapsedClock>,
    stop_requested: Arc<AtomicBool>,
) {
    while !stop_requested.load(Ordering::Acquire) {
        match queue.recv_timeout(WORKER_TICK) {
            Ok((packet, arrival)) => worker.engine.ingest(packet, arrival),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                let flushed = worker.engine.flush();
                worker.deliver(flushed);
                return;
            }
        }
        while let Ok((packet, arrival)) = queue.try_recv() {
            worker.engine.ingest(packet, arrival);
        }
        let output = worker.engine.advance(clock.elapsed());
        worker.deliver(output);
    }
}

impl DecodePipeline {
    pub fn new(decoder: Box<dyn VideoDecoder>) -> Result<(Self, PacketSink), MediaCoreError> {
        Self::with_clock(decoder, Arc::new(SystemElapsed::new()))
    }

    pub fn with_clock(
        decoder: Box<dyn VideoDecoder>,
        clock: Arc<dyn ElapsedClock>,
    ) -> Result<(Self, PacketSink), MediaCoreError> {
        let shared = Arc::new(Shared {
            ring: FrameRing::new(),
            stats: MediaStats::default(),
            arrived: Notify::new(),
        });
        let stop_requested = Arc::new(AtomicBool::new(false));
        let (packets, queue) = sync_channel(PACKET_QUEUE);
        let (request_sender, request_receiver) = tokio::sync::mpsc::channel(RECOVERY_REQUEST_QUEUE);
        let worker = Worker {
            engine: ReceiveEngine::new(decoder),
            shared: Arc::clone(&shared),
            requests: request_sender,
        };
        let handle = std::thread::Builder::new()
            .name("media-core-decode".to_owned())
            .spawn({
                let stop_requested = Arc::clone(&stop_requested);
                let clock = Arc::clone(&clock);
                move || decode_loop(worker, queue, clock, stop_requested)
            })
            .map_err(|error| MediaCoreError::Decoder(error.to_string()))?;
        let sink = PacketSink {
            packets,
            clock,
            shared: Arc::clone(&shared),
        };
        Ok((
            Self {
                shared,
                stop_requested,
                worker: Mutex::new(Some(handle)),
                requests: Mutex::new(Some(request_receiver)),
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

    pub fn take_recovery_requests(&self) -> Option<RequestReceiver<RecoveryRequest>> {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
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

    #[test]
    fn stats_start_at_zero() {
        let stats = MediaStats::default();
        assert_eq!(
            (
                stats.packets(),
                stats.packets_dropped(),
                stats.decoded_frames(),
                stats.decode_errors()
            ),
            (0, 0, 0, 0)
        );
    }
}
