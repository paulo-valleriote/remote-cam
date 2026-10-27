//! The receive-only peer connection and the decode worker behind the Media Port.
//!
//! Threads: the WebRTC driver and the track task never decode and never wait
//! for the consumer. They hand complete access units to a dedicated decode
//! thread through a bounded queue (a full queue drops the unit) and the decode
//! thread publishes frames into the Frame Ring.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use async_trait::async_trait;
use rtc::peer_connection::sdp::RTCSessionDescription;
use rtc::rtp::codec::h264::H264Packet;
use rtc::rtp::packetizer::Depacketizer;
use rtc::rtp_transceiver::RTCRtpTransceiverInit;
use rtc::rtp_transceiver::rtp_sender::RtpCodecKind;
use tokio::sync::Notify;
use webrtc::media_stream::track_remote::{TrackRemote, TrackRemoteEvent};
use webrtc::peer_connection::{
    MediaEngine, PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler,
    RTCConfigurationBuilder, RTCIceGatheringState, RTCIceServer, Registry,
    register_default_interceptors,
};
use webrtc::rtp_transceiver::RTCRtpTransceiverDirection;

use crate::decoder::H264Decoder;
use crate::frame_ring::FrameRing;
use crate::nv12::Nv12Frame;

/// Access units waiting for the decode thread before new ones are dropped.
const ACCESS_UNIT_QUEUE: usize = 8;
const GATHERING_DEADLINE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaCoreError {
    PeerConnection(String),
    Decoder(String),
    GatheringTimedOut,
}

impl fmt::Display for MediaCoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PeerConnection(message) => write!(formatter, "peer connection error: {message}"),
            Self::Decoder(message) => write!(formatter, "decoder error: {message}"),
            Self::GatheringTimedOut => formatter.write_str("ICE gathering did not complete"),
        }
    }
}

impl std::error::Error for MediaCoreError {}

fn connection_error(error: impl fmt::Display) -> MediaCoreError {
    MediaCoreError::PeerConnection(error.to_string())
}

/// Counters observable by tests and the harness.
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

/// Reassembles RTP packets of one H.264 stream into Annex-B access units.
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

    /// Feeds one packet; returns the access unit when the marker bit closes it.
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

struct EventSink {
    shared: Arc<Shared>,
    access_units: SyncSender<AccessUnit>,
    gathered: Arc<Notify>,
}

#[async_trait]
impl PeerConnectionEventHandler for EventSink {
    async fn on_ice_gathering_state_change(&self, state: RTCIceGatheringState) {
        if state == RTCIceGatheringState::Complete {
            self.gathered.notify_one();
        }
    }

    async fn on_track(&self, track: Arc<dyn TrackRemote>) {
        if track.kind().await != RtpCodecKind::Video {
            return;
        }
        let shared = Arc::clone(&self.shared);
        let access_units = self.access_units.clone();
        tokio::spawn(async move {
            let mut assembler = AccessUnitAssembler::new();
            while let Some(event) = track.poll().await {
                let TrackRemoteEvent::OnRtpPacket(packet) = event else {
                    continue;
                };
                shared.stats.packets.fetch_add(1, Ordering::Relaxed);
                if let Some(unit) = assembler.push(&packet)
                    && access_units.try_send(unit).is_err()
                {
                    shared
                        .stats
                        .access_units_dropped
                        .fetch_add(1, Ordering::Relaxed);
                }
            }
        });
    }
}

fn decode_loop(
    mut decoder: H264Decoder,
    queue: Receiver<AccessUnit>,
    shared: Arc<Shared>,
    stop: Arc<std::sync::atomic::AtomicBool>,
) {
    while !stop.load(Ordering::Acquire) {
        let unit = match queue.try_recv() {
            Ok(unit) => unit,
            Err(TryRecvError::Empty) => {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
            Err(TryRecvError::Disconnected) => return,
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

/// One receive-only session's media: peer connection, decode thread, Frame Ring.
pub struct MediaCore {
    connection: Arc<dyn PeerConnection>,
    shared: Arc<Shared>,
    gathered: Arc<Notify>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl MediaCore {
    /// Builds a receive-only connection bound to `bind` (host candidates only,
    /// no ICE servers) and starts the decode thread.
    pub async fn new(bind: &str) -> Result<Self, MediaCoreError> {
        let decoder =
            H264Decoder::new().map_err(|error| MediaCoreError::Decoder(error.to_string()))?;

        let mut media_engine = MediaEngine::default();
        media_engine
            .register_default_codecs()
            .map_err(connection_error)?;
        let registry = register_default_interceptors(Registry::new(), &mut media_engine)
            .map_err(connection_error)?;

        let shared = Arc::new(Shared {
            ring: FrameRing::new(),
            stats: MediaStats::default(),
            arrived: Notify::new(),
        });
        let gathered = Arc::new(Notify::new());
        let (access_units, queue) = sync_channel(ACCESS_UNIT_QUEUE);
        let sink = EventSink {
            shared: Arc::clone(&shared),
            access_units,
            gathered: Arc::clone(&gathered),
        };

        let connection = PeerConnectionBuilder::new()
            .with_configuration(
                RTCConfigurationBuilder::new()
                    .with_ice_servers(Vec::<RTCIceServer>::new())
                    .build(),
            )
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .with_handler(Arc::new(sink) as Arc<dyn PeerConnectionEventHandler>)
            .with_udp_addrs(vec![bind.to_owned()])
            .build()
            .await
            .map_err(connection_error)?;
        let connection: Arc<dyn PeerConnection> = Arc::new(connection);
        connection
            .add_transceiver_from_kind(
                RtpCodecKind::Video,
                Some(RTCRtpTransceiverInit {
                    direction: RTCRtpTransceiverDirection::Recvonly,
                    ..Default::default()
                }),
            )
            .await
            .map_err(connection_error)?;

        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker = std::thread::Builder::new()
            .name("media-core-decode".to_owned())
            .spawn({
                let shared = Arc::clone(&shared);
                let stop = Arc::clone(&stop);
                move || decode_loop(decoder, queue, shared, stop)
            })
            .map_err(|error| MediaCoreError::Decoder(error.to_string()))?;

        Ok(Self {
            connection,
            shared,
            gathered,
            stop,
            worker: Mutex::new(Some(worker)),
        })
    }

    /// Accepts the phone's offer and returns the complete answer SDP (ICE
    /// gathering finished, so no trickle candidates are needed).
    pub async fn answer(&self, offer_sdp: &str) -> Result<String, MediaCoreError> {
        let offer = RTCSessionDescription::offer(offer_sdp.to_owned()).map_err(connection_error)?;
        self.connection
            .set_remote_description(offer)
            .await
            .map_err(connection_error)?;
        let answer = self
            .connection
            .create_answer(None)
            .await
            .map_err(connection_error)?;
        self.connection
            .set_local_description(answer)
            .await
            .map_err(connection_error)?;
        tokio::time::timeout(GATHERING_DEADLINE, self.gathered.notified())
            .await
            .map_err(|_| MediaCoreError::GatheringTimedOut)?;
        self.connection
            .local_description()
            .await
            .map(|description| description.sdp)
            .ok_or_else(|| MediaCoreError::PeerConnection("local description missing".to_owned()))
    }

    /// Waits until a decoded frame is available in the ring.
    pub async fn frame_available(&self) {
        while self.shared.ring.is_empty() {
            self.shared.arrived.notified().await;
        }
    }

    pub fn ring(&self) -> &FrameRing<Nv12Frame> {
        &self.shared.ring
    }

    pub fn stats(&self) -> &MediaStats {
        &self.shared.stats
    }

    /// True when every transceiver is receive-only: the Media Core never
    /// originates media.
    pub async fn is_receive_only(&self) -> bool {
        let transceivers = self.connection.get_transceivers().await;
        let mut receive_only = !transceivers.is_empty();
        for transceiver in transceivers {
            receive_only &=
                transceiver.direction().await.ok() == Some(RTCRtpTransceiverDirection::Recvonly);
        }
        receive_only
    }

    pub async fn close(&self) -> Result<(), MediaCoreError> {
        self.stop.store(true, Ordering::Release);
        let worker = self
            .worker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(worker) = worker {
            let _ = tokio::task::spawn_blocking(move || worker.join()).await;
        }
        self.shared.ring.clear();
        self.connection.close().await.map_err(connection_error)
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
        // FU-A: indicator 0x7C (type 28, nri 3), header start / end with type 5.
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
    fn errors_describe_their_cause() {
        assert!(
            MediaCoreError::PeerConnection("boom".into())
                .to_string()
                .contains("boom")
        );
        assert!(
            MediaCoreError::Decoder("bad".into())
                .to_string()
                .contains("bad")
        );
        assert!(
            MediaCoreError::GatheringTimedOut
                .to_string()
                .contains("ICE")
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
