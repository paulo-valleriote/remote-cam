use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use remotecam_protocol::ReasonCode;
use rtc::interceptor::Slot;
use rtc::peer_connection::sdp::RTCSessionDescription;
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

use crate::decoder::VideoDecoder;
use crate::frame_ring::FrameRing;
use crate::nv12::Nv12Frame;
use crate::outbound::{OutboundRtpCounter, OutboundRtpTap};
use crate::pipeline::{DecodePipeline, MediaStats, PacketSink};

const GATHERING_DEADLINE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaCoreError {
    PeerConnection(String),
    Decoder(String),
    GatheringTimedOut,
    ReceiveOnlyViolation(ReasonCode),
}

impl fmt::Display for MediaCoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PeerConnection(message) => write!(formatter, "peer connection error: {message}"),
            Self::Decoder(message) => write!(formatter, "decoder error: {message}"),
            Self::GatheringTimedOut => formatter.write_str("ICE gathering did not complete"),
            Self::ReceiveOnlyViolation(reason) => write!(
                formatter,
                "receive-only invariant violated: outbound media RTP ({})",
                reason.as_str()
            ),
        }
    }
}

impl std::error::Error for MediaCoreError {}

fn connection_error(error: impl fmt::Display) -> MediaCoreError {
    MediaCoreError::PeerConnection(error.to_string())
}

struct EventSink {
    packets: Mutex<Option<PacketSink>>,
    gathering_complete: Arc<Notify>,
}

#[async_trait]
impl PeerConnectionEventHandler for EventSink {
    async fn on_ice_gathering_state_change(&self, state: RTCIceGatheringState) {
        if state == RTCIceGatheringState::Complete {
            self.gathering_complete.notify_one();
        }
    }

    async fn on_track(&self, track: Arc<dyn TrackRemote>) {
        if track.kind().await != RtpCodecKind::Video {
            return;
        }
        let Some(mut packets) = self
            .packets
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        else {
            return;
        };
        tokio::spawn(async move {
            while let Some(event) = track.poll().await {
                if let TrackRemoteEvent::OnRtpPacket(packet) = event {
                    packets.push(&packet);
                }
            }
        });
    }
}

pub struct MediaCore {
    connection: Arc<dyn PeerConnection>,
    pipeline: DecodePipeline,
    gathering_complete: Arc<Notify>,
    outbound: OutboundRtpCounter,
}

impl MediaCore {
    #[cfg(not(windows))]
    pub async fn new(bind_address: &str) -> Result<Self, MediaCoreError> {
        let decoder = crate::software_decoder::SoftwareH264Decoder::new()
            .map_err(|error| MediaCoreError::Decoder(error.to_string()))?;
        Self::with_decoder(bind_address, Box::new(decoder)).await
    }

    pub async fn with_decoder(
        bind_address: &str,
        decoder: Box<dyn VideoDecoder>,
    ) -> Result<Self, MediaCoreError> {
        let mut media_engine = MediaEngine::default();
        media_engine
            .register_default_codecs()
            .map_err(connection_error)?;
        let outbound = OutboundRtpCounter::new();
        // Wire-most slot, free in the default chain: it sees every packet that really leaves.
        let registry = register_default_interceptors(
            Registry::new().with(
                Slot::CongestionControl,
                OutboundRtpTap::new(outbound.clone()),
            ),
            &mut media_engine,
        )
        .map_err(connection_error)?;

        let (pipeline, packets) = DecodePipeline::new(decoder)?;
        let gathering_complete = Arc::new(Notify::new());
        let sink = EventSink {
            packets: Mutex::new(Some(packets)),
            gathering_complete: Arc::clone(&gathering_complete),
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
            .with_udp_addrs(vec![bind_address.to_owned()])
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

        Ok(Self {
            connection,
            pipeline,
            gathering_complete,
            outbound,
        })
    }

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
        tokio::time::timeout(GATHERING_DEADLINE, self.gathering_complete.notified())
            .await
            .map_err(|_| MediaCoreError::GatheringTimedOut)?;
        self.connection
            .local_description()
            .await
            .map(|description| description.sdp)
            .ok_or_else(|| MediaCoreError::PeerConnection("local description missing".to_owned()))
    }

    pub async fn frame_available(&self) {
        self.pipeline.frame_available().await;
    }

    pub fn ring(&self) -> &FrameRing<Nv12Frame> {
        self.pipeline.ring()
    }

    pub fn stats(&self) -> &MediaStats {
        self.pipeline.stats()
    }

    pub fn outbound(&self) -> &OutboundRtpCounter {
        &self.outbound
    }

    /// Fails with the closed reason code once any media RTP was written toward the wire.
    pub fn assert_receive_only(&self) -> Result<(), MediaCoreError> {
        self.outbound.violation().map_or(Ok(()), |reason| {
            Err(MediaCoreError::ReceiveOnlyViolation(reason))
        })
    }

    pub async fn is_receive_only(&self) -> bool {
        let directions = self.directions().await;
        !directions.is_empty()
            && directions
                .iter()
                .all(|direction| *direction == Some(RTCRtpTransceiverDirection::Recvonly))
    }

    /// True when any transceiver may originate media. An unreadable direction counts as sending.
    pub async fn has_sending_transceiver(&self) -> bool {
        self.directions().await.iter().any(|direction| {
            !matches!(
                direction,
                Some(RTCRtpTransceiverDirection::Recvonly | RTCRtpTransceiverDirection::Inactive)
            )
        })
    }

    async fn directions(&self) -> Vec<Option<RTCRtpTransceiverDirection>> {
        let mut directions = Vec::new();
        for transceiver in self.connection.get_transceivers().await {
            directions.push(transceiver.direction().await.ok());
        }
        directions
    }

    pub async fn close(&self) -> Result<(), MediaCoreError> {
        self.pipeline.request_stop();
        if let Some(worker) = self.pipeline.take_worker() {
            let joined = tokio::task::spawn_blocking(move || worker.join())
                .await
                .map_err(|error| MediaCoreError::Decoder(error.to_string()))?;
            if joined.is_err() {
                self.pipeline.ring().clear();
                let _ = self.connection.close().await;
                return Err(MediaCoreError::Decoder("decode thread panicked".to_owned()));
            }
        }
        self.pipeline.ring().clear();
        self.connection.close().await.map_err(connection_error)?;
        self.assert_receive_only()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
