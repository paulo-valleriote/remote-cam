use std::sync::Arc;
use std::time::{Duration, Instant};

use openh264::encoder::Encoder;
use openh264::formats::YUVBuffer;
use rtc::media::Sample;
use rtc::media_stream::MediaStreamTrack;
use rtc::peer_connection::configuration::media_engine::MIME_TYPE_H264;
use rtc::peer_connection::sdp::RTCSessionDescription;
use rtc::rtp_transceiver::rtp_sender::{
    RTCRtpCodec, RTCRtpCodingParameters, RTCRtpEncodingParameters, RtpCodecKind,
};
use tokio::sync::Notify;
use webrtc::media_stream::track_local::TrackLocal;
use webrtc::media_stream::track_local::static_sample::TrackLocalStaticSample;
use webrtc::peer_connection::{
    MediaEngine, PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler,
    RTCConfigurationBuilder, RTCIceGatheringState, RTCIceServer, RTCPeerConnectionState, Registry,
    register_default_interceptors,
};
use webrtc::rtp_transceiver::RTCRtpTransceiverDirection;

use crate::core::MediaCoreError;

pub const PICTURE_WIDTH: usize = 64;
pub const PICTURE_HEIGHT: usize = 48;
const FRAME_DURATION: Duration = Duration::from_millis(33);
const SYNTHETIC_SSRC: u32 = 0x5eed_0001;
const WAIT: Duration = Duration::from_secs(10);
const H264_FMTP: &str = "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f";

fn error(error: impl std::fmt::Display) -> MediaCoreError {
    MediaCoreError::PeerConnection(error.to_string())
}

struct PhoneEvents {
    gathered: Arc<Notify>,
    connected: Arc<Notify>,
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for PhoneEvents {
    async fn on_ice_gathering_state_change(&self, state: RTCIceGatheringState) {
        if state == RTCIceGatheringState::Complete {
            self.gathered.notify_one();
        }
    }

    async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
        if state == RTCPeerConnectionState::Connected {
            self.connected.notify_one();
        }
    }
}

pub fn synthetic_picture(index: u32) -> Vec<u8> {
    let luma = PICTURE_WIDTH * PICTURE_HEIGHT;
    let mut yuv = vec![128_u8; luma + luma / 2];
    for (position, sample) in yuv[..luma].iter_mut().enumerate() {
        *sample = ((position % PICTURE_WIDTH) as u32 * 3 + index * 7) as u8;
    }
    yuv
}

pub struct SyntheticPhone {
    connection: Arc<dyn PeerConnection>,
    track: Arc<TrackLocalStaticSample>,
    payload_type: u8,
    encoder: Encoder,
    gathered: Arc<Notify>,
    connected: Arc<Notify>,
    sent: u32,
}

impl SyntheticPhone {
    pub async fn new(bind: &str) -> Result<Self, MediaCoreError> {
        let mut media_engine = MediaEngine::default();
        media_engine.register_default_codecs().map_err(error)?;
        let registry =
            register_default_interceptors(Registry::new(), &mut media_engine).map_err(error)?;
        let gathered = Arc::new(Notify::new());
        let connected = Arc::new(Notify::new());
        let events = PhoneEvents {
            gathered: Arc::clone(&gathered),
            connected: Arc::clone(&connected),
        };
        let connection = PeerConnectionBuilder::new()
            .with_configuration(
                RTCConfigurationBuilder::new()
                    .with_ice_servers(Vec::<RTCIceServer>::new())
                    .build(),
            )
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .with_handler(Arc::new(events) as Arc<dyn PeerConnectionEventHandler>)
            .with_udp_addrs(vec![bind.to_owned()])
            .build()
            .await
            .map_err(error)?;
        let connection: Arc<dyn PeerConnection> = Arc::new(connection);

        let codec = RTCRtpCodec {
            mime_type: MIME_TYPE_H264.to_owned(),
            clock_rate: 90_000,
            channels: 0,
            sdp_fmtp_line: H264_FMTP.to_owned(),
            rtcp_feedback: vec![],
        };
        let track = Arc::new(
            TrackLocalStaticSample::new(
                Instant::now(),
                MediaStreamTrack::new(
                    "synthetic-phone".to_owned(),
                    "synthetic-phone-video".to_owned(),
                    "synthetic-phone-video".to_owned(),
                    RtpCodecKind::Video,
                    vec![RTCRtpEncodingParameters {
                        rtp_coding_parameters: RTCRtpCodingParameters {
                            ssrc: Some(SYNTHETIC_SSRC),
                            ..Default::default()
                        },
                        codec,
                        ..Default::default()
                    }],
                ),
            )
            .map_err(error)?,
        );
        let sender = connection
            .add_track(Arc::clone(&track) as Arc<dyn TrackLocal>)
            .await
            .map_err(error)?;
        for transceiver in connection.get_transceivers().await {
            transceiver
                .set_direction(RTCRtpTransceiverDirection::Sendonly)
                .await
                .map_err(error)?;
        }
        let payload_type = sender
            .get_parameters()
            .await
            .map_err(error)?
            .rtp_parameters
            .codecs
            .first()
            .map(|codec| codec.payload_type)
            .ok_or_else(|| MediaCoreError::PeerConnection("sender has no codec".to_owned()))?;

        Ok(Self {
            connection,
            track,
            payload_type,
            encoder: Encoder::new()
                .map_err(|failure| MediaCoreError::Decoder(failure.to_string()))?,
            gathered,
            connected,
            sent: 0,
        })
    }

    pub async fn offer(&self) -> Result<String, MediaCoreError> {
        let offer = self.connection.create_offer(None).await.map_err(error)?;
        self.connection
            .set_local_description(offer)
            .await
            .map_err(error)?;
        tokio::time::timeout(WAIT, self.gathered.notified())
            .await
            .map_err(|_| MediaCoreError::GatheringTimedOut)?;
        self.connection
            .local_description()
            .await
            .map(|description| description.sdp)
            .ok_or_else(|| MediaCoreError::PeerConnection("local description missing".to_owned()))
    }

    pub async fn accept_answer(&self, answer_sdp: &str) -> Result<(), MediaCoreError> {
        let answer = RTCSessionDescription::answer(answer_sdp.to_owned()).map_err(error)?;
        self.connection
            .set_remote_description(answer)
            .await
            .map_err(error)
    }

    pub async fn connected(&self) -> Result<(), MediaCoreError> {
        tokio::time::timeout(WAIT, self.connected.notified())
            .await
            .map_err(|_| MediaCoreError::PeerConnection("connection timed out".to_owned()))
    }

    pub async fn send_frame(&mut self) -> Result<(), MediaCoreError> {
        let picture =
            YUVBuffer::from_vec(synthetic_picture(self.sent), PICTURE_WIDTH, PICTURE_HEIGHT);
        let encoded = self
            .encoder
            .encode(&picture)
            .map_err(|failure| MediaCoreError::Decoder(failure.to_string()))?
            .to_vec();
        self.sent += 1;
        self.track
            .sample_writer(SYNTHETIC_SSRC, self.payload_type)
            .write_sample(&Sample {
                data: encoded.into(),
                duration: FRAME_DURATION,
                ..Sample::new(Instant::now())
            })
            .await
            .map_err(error)
    }

    pub async fn close(&self) -> Result<(), MediaCoreError> {
        self.connection.close().await.map_err(error)
    }
}
