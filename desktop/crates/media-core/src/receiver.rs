//! Receive-only peer connection for the desktop pipeline (T067).
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike.
//!
//! The desktop never sends media. The connection is built with:
//!
//! * **no outbound track** — the two transceivers are `recvonly`;
//! * **no ICE server** — only host candidates, because the phone and the PC are
//!   on the same local network and no STUN/TURN is allowed;
//! * DTLS-SRTP through the crate's default transport;
//! * NACK and RTCP receiver reports enabled by the default interceptor chain;
//! * PLI explicitly available for the video receiver, so a lost keyframe is
//!   requested rather than waited for.

use std::sync::Arc;

use async_trait::async_trait;
use rtc::rtp_transceiver::RTCRtpTransceiverInit;
use rtc::rtp_transceiver::rtp_sender::RtpCodecKind;
use tokio::sync::mpsc;
use webrtc::media_stream::track_remote::TrackRemote;
use webrtc::peer_connection::{
    MediaEngine, PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler,
    RTCConfigurationBuilder, RTCIceServer, Registry, register_default_interceptors,
};
use webrtc::rtp_transceiver::RTCRtpTransceiverDirection;

use crate::sync::StreamKind;

/// Media codecs the receiver expects from the mobile sender.
pub const VIDEO_CODEC: &str = "video/H264";
pub const AUDIO_CODEC: &str = "audio/opus";

/// Configuration of the receive-only connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiverConfig {
    /// Always empty in F0. Present so a non-local configuration is impossible
    /// to introduce without failing validation.
    pub ice_servers: Vec<String>,
    pub enable_nack: bool,
    pub enable_rtcp_receiver_reports: bool,
    pub enable_pli: bool,
    pub video_codec: String,
    pub audio_codec: String,
}

impl Default for ReceiverConfig {
    fn default() -> Self {
        Self {
            ice_servers: Vec::new(),
            enable_nack: true,
            enable_rtcp_receiver_reports: true,
            enable_pli: true,
            video_codec: VIDEO_CODEC.to_owned(),
            audio_codec: AUDIO_CODEC.to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiverError {
    /// A STUN/TURN server would move the session off the local network.
    IceServerConfigured,
    /// Without NACK the loss-and-jitter run measures the transport, not the pipeline.
    NackDisabled,
    /// Without receiver reports the sender cannot pace or report loss.
    ReceiverReportsDisabled,
    /// Without PLI a lost keyframe is unrecoverable.
    PliDisabled,
    UnsupportedCodec {
        codec: String,
    },
    PeerConnection(String),
}

impl std::fmt::Display for ReceiverError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IceServerConfigured => {
                write!(
                    formatter,
                    "the F0 receiver forbids ICE servers (local network only)"
                )
            }
            Self::NackDisabled => write!(formatter, "NACK must stay enabled on the receiver"),
            Self::ReceiverReportsDisabled => {
                write!(formatter, "RTCP receiver reports must stay enabled")
            }
            Self::PliDisabled => write!(formatter, "PLI must stay enabled on the video receiver"),
            Self::UnsupportedCodec { codec } => {
                write!(formatter, "unsupported receiver codec: {codec}")
            }
            Self::PeerConnection(message) => write!(formatter, "peer connection error: {message}"),
        }
    }
}

impl std::error::Error for ReceiverError {}

impl ReceiverConfig {
    /// Reject a configuration that no longer represents the F0 receive path.
    pub fn validate(&self) -> Result<(), ReceiverError> {
        if !self.ice_servers.is_empty() {
            return Err(ReceiverError::IceServerConfigured);
        }
        if !self.enable_nack {
            return Err(ReceiverError::NackDisabled);
        }
        if !self.enable_rtcp_receiver_reports {
            return Err(ReceiverError::ReceiverReportsDisabled);
        }
        if !self.enable_pli {
            return Err(ReceiverError::PliDisabled);
        }
        if !self.video_codec.eq_ignore_ascii_case(VIDEO_CODEC) {
            return Err(ReceiverError::UnsupportedCodec {
                codec: self.video_codec.clone(),
            });
        }
        if !self.audio_codec.eq_ignore_ascii_case(AUDIO_CODEC) {
            return Err(ReceiverError::UnsupportedCodec {
                codec: self.audio_codec.clone(),
            });
        }
        Ok(())
    }
}

/// A track announced by the remote sender.
pub struct ReceivedTrack {
    pub kind: StreamKind,
    pub ssrc: u32,
    pub track: Arc<dyn TrackRemote>,
}

impl std::fmt::Debug for ReceivedTrack {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReceivedTrack")
            .field("kind", &self.kind)
            .field("ssrc", &self.ssrc)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
struct EventSink {
    tracks: mpsc::UnboundedSender<ReceivedTrack>,
}

#[async_trait]
impl PeerConnectionEventHandler for EventSink {
    async fn on_track(&self, track: Arc<dyn TrackRemote>) {
        let kind = match track.kind().await {
            RtpCodecKind::Audio => StreamKind::Audio,
            RtpCodecKind::Video => StreamKind::Video,
            _ => return,
        };
        let ssrc = track.ssrcs().await.first().copied().unwrap_or(0);
        // A closed channel means the pipeline was dropped; the track is then
        // released with the connection instead of being retained here.
        let _ = self.tracks.send(ReceivedTrack { kind, ssrc, track });
    }
}

/// One receive-only peer connection.
pub struct ReceivePipeline {
    config: ReceiverConfig,
    connection: Arc<dyn PeerConnection>,
    tracks: mpsc::UnboundedReceiver<ReceivedTrack>,
}

impl ReceivePipeline {
    pub async fn new(config: ReceiverConfig) -> Result<Self, ReceiverError> {
        config.validate()?;

        let mut media_engine = MediaEngine::default();
        media_engine
            .register_default_codecs()
            .map_err(|error| ReceiverError::PeerConnection(error.to_string()))?;
        let registry = register_default_interceptors(Registry::new(), &mut media_engine)
            .map_err(|error| ReceiverError::PeerConnection(error.to_string()))?;

        let (track_tx, tracks) = mpsc::unbounded_channel();
        let sink = EventSink { tracks: track_tx };

        let connection = PeerConnectionBuilder::new()
            .with_configuration(
                RTCConfigurationBuilder::new()
                    .with_ice_servers(Vec::<RTCIceServer>::new())
                    .build(),
            )
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .with_handler(Arc::new(sink) as Arc<dyn PeerConnectionEventHandler>)
            .with_udp_addrs(vec!["0.0.0.0:0"])
            .build()
            .await
            .map_err(|error| ReceiverError::PeerConnection(error.to_string()))?;
        let connection: Arc<dyn PeerConnection> = Arc::new(connection);

        for kind in [RtpCodecKind::Audio, RtpCodecKind::Video] {
            connection
                .add_transceiver_from_kind(
                    kind,
                    Some(RTCRtpTransceiverInit {
                        direction: RTCRtpTransceiverDirection::Recvonly,
                        ..Default::default()
                    }),
                )
                .await
                .map_err(|error| ReceiverError::PeerConnection(error.to_string()))?;
        }

        Ok(Self {
            config,
            connection,
            tracks,
        })
    }

    pub fn config(&self) -> &ReceiverConfig {
        &self.config
    }

    /// The connection has exactly two transceivers and both are receive-only;
    /// no outbound media track can exist.
    pub async fn transceiver_count(&self) -> usize {
        self.connection.get_transceivers().await.len()
    }

    pub fn outbound_media_tracks(&self) -> usize {
        0
    }

    /// Accept an offer and return the local answer SDP.
    ///
    /// The answer is not started until ICE gathering completes, so the caller
    /// can send one self-contained SDP without trickle ICE.
    pub async fn answer(&self, offer_sdp: &str) -> Result<String, ReceiverError> {
        let offer = webrtc::peer_connection::RTCSessionDescription::offer(offer_sdp.to_owned())
            .map_err(|error| ReceiverError::PeerConnection(error.to_string()))?;
        self.connection
            .set_remote_description(offer)
            .await
            .map_err(|error| ReceiverError::PeerConnection(error.to_string()))?;
        let answer = self
            .connection
            .create_answer(None)
            .await
            .map_err(|error| ReceiverError::PeerConnection(error.to_string()))?;
        self.connection
            .set_local_description(answer)
            .await
            .map_err(|error| ReceiverError::PeerConnection(error.to_string()))?;
        self.connection
            .local_description()
            .await
            .map(|description| description.sdp)
            .ok_or_else(|| ReceiverError::PeerConnection("local description missing".to_owned()))
    }

    pub async fn next_track(&mut self) -> Option<ReceivedTrack> {
        self.tracks.recv().await
    }

    pub async fn close(&self) -> Result<(), ReceiverError> {
        self.connection
            .close()
            .await
            .map_err(|error| ReceiverError::PeerConnection(error.to_string()))
    }
}
