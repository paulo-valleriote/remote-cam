//! The product harness: a headless driver for the RemoteCam pairing path.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use remotecam_pairing::{EphemeralCertificate, PairingListener, PinnedClient};
use remotecam_protocol::signaling::{
    Limits, MAX_SIGNALING_MESSAGE_BYTES, Sdp, SignalingMessage, parse_signaling,
};
use remotecam_protocol::{Endpoint, QrPayload};
use remotecam_session_core::testing::{ManualClock, SeededRandomSource};
use remotecam_session_core::{Effect, SessionCore, SessionEvent, SessionState, Timestamp};
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::runtime::Builder;
use zeroize::{Zeroize, Zeroizing};

const PAIRING_SEED: u64 = 0;
const PAIRING_CLOCK_START: Timestamp = Timestamp::ZERO;
const QR_CLOCK_START_UNIX: u64 = 1_900_000_000;
const QR_EXPIRES_AT_UNIX: u64 = 2_000_000_000;
const DESKTOP_DTLS_FINGERPRINT: [u8; 32] = [0x44; 32];
const MOBILE_DTLS_FINGERPRINT: [u8; 32] = [0x55; 32];
const OFFER_SDP: &str = "v=0\r\no=- 1 1 IN IP4 0.0.0.0\r\ns=-\r\nt=0 0\r\n";
const ANSWER_SDP: &str = "v=0\r\no=- 1 1 IN IP4 0.0.0.0\r\ns=-\r\nt=0 0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\na=recvonly\r\n";

/// The per-scenario outcome carried by the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Pass,
    Fail,
}

/// The report-level totals for the scenarios that ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub scenarios: u32,
    pub passed: u32,
    pub failed: u32,
}

/// The deterministic, machine-readable result of one harness run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HarnessReport {
    pub report_version: u32,
    pub scenario: &'static str,
    pub observed_state: &'static str,
    pub message_sequence: Vec<&'static str>,
    pub verdict: Verdict,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<&'static str>,
    pub summary: Summary,
}

impl HarnessReport {
    /// Builds a report for a session state without a transport observation.
    pub fn for_session_state(state: SessionState) -> Self {
        let reason_code = (state != SessionState::Connected).then_some("internal_error");
        Self::for_pairing_result(state, reason_code, Vec::new())
    }

    fn for_pairing_result(
        state: SessionState,
        reason_code: Option<&'static str>,
        message_sequence: Vec<&'static str>,
    ) -> Self {
        let passed = reason_code.is_none();
        Self {
            report_version: 1,
            scenario: "pairing",
            observed_state: state.as_str(),
            message_sequence,
            verdict: if passed { Verdict::Pass } else { Verdict::Fail },
            reason_code,
            summary: Summary {
                scenarios: 1,
                passed: u32::from(passed),
                failed: u32::from(!passed),
            },
        }
    }

    /// Serialises the report as the single JSON line the harness prints.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

/// Runs the deterministic pairing scenario through a loopback TLS listener.
pub fn run() -> HarnessReport {
    let runtime = match Builder::new_current_thread().enable_io().build() {
        Ok(runtime) => runtime,
        Err(_) => {
            return HarnessReport::for_pairing_result(
                SessionState::Idle,
                Some("internal_error"),
                Vec::new(),
            );
        }
    };
    runtime.block_on(run_pairing())
}

async fn run_pairing() -> HarnessReport {
    let mut core = SessionCore::new(
        ManualClock::new(PAIRING_CLOCK_START),
        SeededRandomSource::new(PAIRING_SEED),
        DESKTOP_DTLS_FINGERPRINT,
    );

    let pairing_effects = match core.handle(SessionEvent::StartPairing) {
        Ok(effects) => effects,
        Err(error) => {
            return HarnessReport::for_pairing_result(
                core.state(),
                Some(error.code().as_str()),
                Vec::new(),
            );
        }
    };
    let [Effect::PresentPairing(material)] = pairing_effects.as_slice() else {
        return HarnessReport::for_pairing_result(core.state(), Some("internal_error"), Vec::new());
    };

    let certificate = match EphemeralCertificate::generate() {
        Ok(certificate) => certificate,
        Err(_) => {
            return HarnessReport::for_pairing_result(
                core.state(),
                Some("internal_error"),
                Vec::new(),
            );
        }
    };
    let listener = match PairingListener::bind_loopback(certificate).await {
        Ok(listener) => Arc::new(listener),
        Err(_) => {
            return HarnessReport::for_pairing_result(
                core.state(),
                Some("internal_error"),
                Vec::new(),
            );
        }
    };

    let address = listener.local_addr();
    let endpoint = match Endpoint::from_address(4, "192.168.1.42", address.port()) {
        Some(endpoint) => endpoint,
        None => {
            return HarnessReport::for_pairing_result(
                core.state(),
                Some("invalid_qr_payload"),
                Vec::new(),
            );
        }
    };
    let epoch = match u32::try_from(material.epoch()) {
        Ok(epoch) => epoch,
        Err(_) => {
            return HarnessReport::for_pairing_result(
                core.state(),
                Some("invalid_qr_payload"),
                Vec::new(),
            );
        }
    };
    let mut qr_payload = QrPayload {
        session_id: *material.session_id().as_bytes(),
        epoch,
        expires_at: QR_EXPIRES_AT_UNIX,
        pairing_token: *material.token().as_bytes(),
        tls_fingerprint: listener.fingerprint(),
        capabilities: remotecam_pairing::DESKTOP_CAPABILITIES,
        endpoints: vec![endpoint],
    };
    let encoded_qr = qr_payload.encode();
    qr_payload.pairing_token.zeroize();
    let encoded_qr = match encoded_qr {
        Ok(encoded_qr) => Zeroizing::new(encoded_qr),
        Err(_) => {
            return HarnessReport::for_pairing_result(
                core.state(),
                Some("invalid_qr_payload"),
                Vec::new(),
            );
        }
    };

    let mut mobile_payload = match QrPayload::decode(&encoded_qr, QR_CLOCK_START_UNIX) {
        Ok(payload) => payload,
        Err(error) => {
            return HarnessReport::for_pairing_result(
                core.state(),
                Some(error.code.as_str()),
                Vec::new(),
            );
        }
    };
    let mobile_token = Zeroizing::new(mobile_payload.pairing_token);
    mobile_payload.pairing_token.zeroize();
    let Some(endpoint) = mobile_payload.endpoints.first() else {
        return HarnessReport::for_pairing_result(
            core.state(),
            Some("invalid_qr_payload"),
            Vec::new(),
        );
    };
    if endpoint.address_string().is_none() {
        return HarnessReport::for_pairing_result(
            core.state(),
            Some("invalid_qr_payload"),
            Vec::new(),
        );
    }
    // QR v1 excludes loopback addresses, so the synthetic client routes the
    // decoded endpoint's port to the listener bound on the loopback interface.
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, endpoint.port));
    let session_id = mobile_payload.session_id;
    let epoch = u64::from(mobile_payload.epoch);
    let client = match PinnedClient::new(mobile_payload.tls_fingerprint) {
        Ok(client) => client,
        Err(error) => {
            return HarnessReport::for_pairing_result(
                core.state(),
                Some(error.code().as_str()),
                Vec::new(),
            );
        }
    };

    let mut pair_message = SignalingMessage::Pair {
        token: *mobile_token,
        dtls_fingerprint: MOBILE_DTLS_FINGERPRINT,
        limits: Limits::new(),
    };
    let pair_frame = pair_message.encode();
    if let SignalingMessage::Pair { token, .. } = &mut pair_message {
        token.zeroize();
    }
    let pair_frame = match pair_frame {
        Ok(frame) => Zeroizing::new(frame),
        Err(_) => {
            return HarnessReport::for_pairing_result(
                core.state(),
                Some("internal_error"),
                Vec::new(),
            );
        }
    };

    let server = tokio::spawn(run_desktop(core, listener));
    let mut mobile_channel = match client.connect(address).await {
        Ok(channel) => channel,
        Err(error) => {
            server.abort();
            return HarnessReport::for_pairing_result(
                SessionState::PairingReady,
                Some(error.code().as_str()),
                Vec::new(),
            );
        }
    };
    let mut mobile = BufReader::new(&mut mobile_channel);
    let mut message_sequence = Vec::with_capacity(4);

    if write_signaling_frame(&mut mobile, &pair_frame)
        .await
        .is_err()
    {
        server.abort();
        return HarnessReport::for_pairing_result(
            SessionState::Handshaking,
            Some("internal_error"),
            message_sequence,
        );
    }
    message_sequence.push("pair");

    let paired_frame = match read_signaling_frame(&mut mobile).await {
        Ok(frame) => frame,
        Err(reason_code) => {
            server.abort();
            return HarnessReport::for_pairing_result(
                SessionState::Handshaking,
                Some(reason_code),
                message_sequence,
            );
        }
    };
    match parse_signaling(&paired_frame) {
        Ok(SignalingMessage::Paired {
            session_id: paired_session_id,
            epoch: paired_epoch,
            dtls_fingerprint,
            accepted_limits,
        }) if paired_session_id == session_id
            && paired_epoch == epoch
            && dtls_fingerprint == DESKTOP_DTLS_FINGERPRINT
            && accepted_limits == Limits::new() =>
        {
            message_sequence.push("paired");
        }
        Ok(_) => {
            server.abort();
            return HarnessReport::for_pairing_result(
                SessionState::Handshaking,
                Some("invalid_message"),
                message_sequence,
            );
        }
        Err(error) => {
            server.abort();
            return HarnessReport::for_pairing_result(
                SessionState::Handshaking,
                Some(error.code.as_str()),
                message_sequence,
            );
        }
    }

    let offer_sdp = match Sdp::new(OFFER_SDP) {
        Ok(sdp) => sdp,
        Err(_) => {
            server.abort();
            return HarnessReport::for_pairing_result(
                SessionState::Negotiating,
                Some("internal_error"),
                message_sequence,
            );
        }
    };
    let offer_frame = match (SignalingMessage::Offer {
        epoch,
        sdp: offer_sdp,
    })
    .encode()
    {
        Ok(frame) => frame,
        Err(_) => {
            server.abort();
            return HarnessReport::for_pairing_result(
                SessionState::Negotiating,
                Some("internal_error"),
                message_sequence,
            );
        }
    };
    if write_signaling_frame(&mut mobile, &offer_frame)
        .await
        .is_err()
    {
        server.abort();
        return HarnessReport::for_pairing_result(
            SessionState::Negotiating,
            Some("internal_error"),
            message_sequence,
        );
    }
    message_sequence.push("offer");

    let answer_frame = match read_signaling_frame(&mut mobile).await {
        Ok(frame) => frame,
        Err(reason_code) => {
            server.abort();
            return HarnessReport::for_pairing_result(
                SessionState::Negotiating,
                Some(reason_code),
                message_sequence,
            );
        }
    };
    match parse_signaling(&answer_frame) {
        Ok(SignalingMessage::Answer {
            epoch: answer_epoch,
            ..
        }) if answer_epoch == epoch => message_sequence.push("answer"),
        Ok(_) => {
            server.abort();
            return HarnessReport::for_pairing_result(
                SessionState::Negotiating,
                Some("invalid_message"),
                message_sequence,
            );
        }
        Err(error) => {
            server.abort();
            return HarnessReport::for_pairing_result(
                SessionState::Negotiating,
                Some(error.code.as_str()),
                message_sequence,
            );
        }
    }

    match server.await {
        Ok(Ok(SessionState::Connected)) => {
            HarnessReport::for_pairing_result(SessionState::Connected, None, message_sequence)
        }
        Ok(Ok(state)) => {
            HarnessReport::for_pairing_result(state, Some("invalid_transition"), message_sequence)
        }
        Ok(Err(reason_code)) => HarnessReport::for_pairing_result(
            SessionState::Negotiating,
            Some(reason_code),
            message_sequence,
        ),
        Err(_) => HarnessReport::for_pairing_result(
            SessionState::Negotiating,
            Some("internal_error"),
            message_sequence,
        ),
    }
}

async fn run_desktop<C, R>(
    mut core: SessionCore<C, R>,
    listener: Arc<PairingListener>,
) -> Result<SessionState, &'static str>
where
    C: remotecam_session_core::Clock + Send + 'static,
    R: remotecam_session_core::RandomSource + Send + 'static,
{
    let channel = listener
        .accept()
        .await
        .map_err(|error| error.code().as_str())?;
    if !channel.is_tls13() {
        return Err("tls_version_unsupported");
    }

    let peer_effects = core
        .handle(SessionEvent::PeerConnected)
        .map_err(|error| error.code().as_str())?;
    if peer_effects.contains(&Effect::StartMedia) {
        return Err("internal_error");
    }
    let mut desktop = BufReader::new(channel);
    let pair_frame = read_signaling_frame(&mut desktop).await?;
    let paired_effect = core
        .authenticate_first_frame(&pair_frame)
        .map_err(|error| error.code().as_str())?;
    let Effect::SendPaired {
        session_id,
        epoch,
        dtls_fingerprint,
        accepted_limits,
    } = paired_effect
    else {
        return Err("internal_error");
    };
    let paired_frame = SignalingMessage::Paired {
        session_id: *session_id.as_bytes(),
        epoch,
        dtls_fingerprint,
        accepted_limits,
    }
    .encode()
    .map_err(|_| "internal_error")?;
    write_signaling_frame(&mut desktop, &paired_frame).await?;

    let offer_frame = read_signaling_frame(&mut desktop).await?;
    match parse_signaling(&offer_frame) {
        Ok(SignalingMessage::Offer {
            epoch: offer_epoch, ..
        }) if core.epoch() == Some(offer_epoch) => {}
        Ok(_) => return Err("invalid_message"),
        Err(error) => return Err(error.code.as_str()),
    }

    let media_effects = core
        .handle(SessionEvent::NegotiationCompleted)
        .map_err(|error| error.code().as_str())?;
    if media_effects != [Effect::StartMedia] {
        return Err("internal_error");
    }
    let answer_sdp = Sdp::new(ANSWER_SDP).map_err(|_| "internal_error")?;
    let answer_frame = SignalingMessage::Answer {
        epoch: core.epoch().ok_or("invalid_transition")?,
        sdp: answer_sdp,
    }
    .encode()
    .map_err(|_| "internal_error")?;
    write_signaling_frame(&mut desktop, &answer_frame).await?;
    Ok(core.state())
}

async fn read_signaling_frame(
    reader: &mut (impl AsyncRead + Unpin),
) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    let mut frame = Zeroizing::new(Vec::new());
    loop {
        let mut byte = [0_u8; 1];
        reader
            .read_exact(&mut byte)
            .await
            .map_err(|_| "invalid_message")?;
        if byte[0] == b'\n' {
            return Ok(frame);
        }
        if frame.len() == MAX_SIGNALING_MESSAGE_BYTES {
            return Err("invalid_message");
        }
        frame.push(byte[0]);
    }
}

async fn write_signaling_frame(
    writer: &mut (impl AsyncWrite + Unpin),
    frame: &[u8],
) -> Result<(), &'static str> {
    if frame.len() > MAX_SIGNALING_MESSAGE_BYTES {
        return Err("invalid_message");
    }
    writer
        .write_all(frame)
        .await
        .map_err(|_| "internal_error")?;
    writer
        .write_all(b"\n")
        .await
        .map_err(|_| "internal_error")?;
    writer.flush().await.map_err(|_| "internal_error")
}
