//! The product harness: a headless driver for the RemoteCam pairing path.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use remotecam_pairing::{
    EphemeralCertificate, PairingListener, PairingMaterialRequest, PinnedClient,
};
use remotecam_protocol::signaling::{
    Limits, MAX_SIGNALING_MESSAGE_BYTES, Sdp, SignalingMessage, parse_signaling,
};
use remotecam_protocol::{Endpoint, QrPayload};
use remotecam_session_core::testing::{ManualClock, SeededRandomSource};
use remotecam_session_core::{
    Clock, Effect, FIRST_MEDIA_FRAME_TIMEOUT_MILLIS, MediaFrame, MediaPort, RandomSource,
    SessionCore, SessionEvent, SessionState, Timestamp,
};
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::runtime::Builder;
use tokio::time::sleep;
use zeroize::{Zeroize, Zeroizing};

const PAIRING_SEED: u64 = 0;
const CERTIFICATE_SEED: u64 = 1;
const PAIRING_CLOCK_START: Timestamp = Timestamp::ZERO;
const FRAME_INTERVAL_MILLIS: u64 = 33;
const SYNTHETIC_FRAME_PAYLOAD_BYTES: usize = 16;
const QR_CLOCK_START_UNIX: u64 = 1_900_000_000;
const QR_EXPIRES_AT_UNIX: u64 = 2_000_000_000;
const DESKTOP_DTLS_FINGERPRINT: [u8; 32] = [0x44; 32];
const MOBILE_DTLS_FINGERPRINT: [u8; 32] = [0x55; 32];
const OFFER_SDP: &str = "v=0\r\no=- 1 1 IN IP4 0.0.0.0\r\ns=-\r\nt=0 0\r\n";
const ANSWER_SDP: &str = "v=0\r\no=- 1 1 IN IP4 0.0.0.0\r\ns=-\r\nt=0 0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\na=recvonly\r\n";

struct HarnessCertificateRandomSource(SeededRandomSource);

impl RandomSource for HarnessCertificateRandomSource {
    fn fill_bytes(&mut self, destination: &mut [u8]) {
        self.0.fill_bytes(destination);
    }
}

impl remotecam_pairing::certificate::CertificateRandomSource for HarnessCertificateRandomSource {}

/// Selects the product-harness scenario to execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioKind {
    Pairing,
    FirstFrameTimeout,
}

impl ScenarioKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Pairing => "pairing",
            Self::FirstFrameTimeout => "first_frame_timeout",
        }
    }
}

/// Inputs that make a harness scenario reproducible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenarioConfig {
    /// The scenario to execute.
    pub scenario: ScenarioKind,
    /// The seed shared by pairing and synthetic media generation.
    pub seed: u64,
    /// The initial instant on the scenario's logical clock.
    pub clock_start: Timestamp,
}

impl Default for ScenarioConfig {
    fn default() -> Self {
        Self {
            scenario: ScenarioKind::Pairing,
            seed: PAIRING_SEED,
            clock_start: PAIRING_CLOCK_START,
        }
    }
}

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

/// The session-level result observed while running a scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Connected,
    TimedOut,
    Failed,
}

/// One state transition captured by the scenario driver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StateTransition {
    pub from: &'static str,
    pub to: &'static str,
    pub at_millis: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<&'static str>,
}

/// The reportable metadata for one synthetic frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FrameObservation {
    pub sequence: u64,
    pub at_millis: u64,
}

/// A frame event discarded after the session timed out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscardedFrame {
    pub sequence: u64,
    pub at_millis: u64,
    pub reason: &'static str,
}

/// Resource lifecycle counts captured after a scenario closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ResourceCount {
    pub created: usize,
    pub active_after_close: usize,
}

/// Lifecycle counts for resources owned by the harness scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ResourceReport {
    pub listeners: ResourceCount,
    pub connections: ResourceCount,
    pub media_ports: ResourceCount,
    pub tasks: ResourceCount,
}

impl Default for ResourceReport {
    fn default() -> Self {
        let empty = ResourceCount {
            created: 0,
            active_after_close: 0,
        };
        Self {
            listeners: empty,
            connections: empty,
            media_ports: empty,
            tasks: empty,
        }
    }
}

/// The deterministic, machine-readable result of one harness run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HarnessReport {
    pub report_version: u32,
    pub scenario: &'static str,
    pub outcome: Outcome,
    pub observed_state: &'static str,
    pub message_sequence: Vec<&'static str>,
    pub state_trace: Vec<StateTransition>,
    pub frames: Vec<FrameObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub late_frame_discarded: Option<DiscardedFrame>,
    pub resources: ResourceReport,
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
            report_version: 2,
            scenario: "pairing",
            outcome: if passed {
                Outcome::Connected
            } else {
                Outcome::Failed
            },
            observed_state: state.as_str(),
            message_sequence,
            state_trace: Vec::new(),
            frames: Vec::new(),
            late_frame_discarded: None,
            resources: ResourceReport::default(),
            verdict: if passed { Verdict::Pass } else { Verdict::Fail },
            reason_code,
            summary: Summary {
                scenarios: 1,
                passed: u32::from(passed),
                failed: u32::from(!passed),
            },
        }
    }

    fn invalid_qr_payload(state: SessionState) -> Self {
        Self::for_pairing_result(state, Some("invalid_qr_payload"), Vec::new())
    }

    /// Serialises the report as the single JSON line the harness prints.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

#[derive(Clone, Default)]
struct ResourceTracker {
    listeners: Arc<ResourceCounter>,
    connections: Arc<ResourceCounter>,
    media_ports: Arc<ResourceCounter>,
    tasks: Arc<ResourceCounter>,
}

#[derive(Default)]
struct ResourceCounter {
    created: AtomicUsize,
    active: AtomicUsize,
}

struct ResourceGuard(Arc<ResourceCounter>);

impl ResourceTracker {
    fn listener(&self) -> ResourceGuard {
        Self::track(&self.listeners)
    }

    fn connection(&self) -> ResourceGuard {
        Self::track(&self.connections)
    }

    fn media_port(&self) -> ResourceGuard {
        Self::track(&self.media_ports)
    }

    fn task(&self) -> ResourceGuard {
        Self::track(&self.tasks)
    }

    fn track(counter: &Arc<ResourceCounter>) -> ResourceGuard {
        counter.created.fetch_add(1, Ordering::SeqCst);
        counter.active.fetch_add(1, Ordering::SeqCst);
        ResourceGuard(Arc::clone(counter))
    }

    fn snapshot(&self) -> ResourceReport {
        fn count(counter: &ResourceCounter) -> ResourceCount {
            ResourceCount {
                created: counter.created.load(Ordering::SeqCst),
                active_after_close: counter.active.load(Ordering::SeqCst),
            }
        }

        ResourceReport {
            listeners: count(&self.listeners),
            connections: count(&self.connections),
            media_ports: count(&self.media_ports),
            tasks: count(&self.tasks),
        }
    }
}

impl Drop for ResourceGuard {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

struct SyntheticMediaPort {
    clock: ManualClock,
    seed: u64,
    next_sequence: u64,
    resources: ResourceTracker,
    active: Option<ResourceGuard>,
    payloads: Vec<[u8; SYNTHETIC_FRAME_PAYLOAD_BYTES]>,
}

impl SyntheticMediaPort {
    fn new(clock: ManualClock, seed: u64, resources: ResourceTracker) -> Self {
        Self {
            clock,
            seed,
            next_sequence: 0,
            resources,
            active: None,
            payloads: Vec::new(),
        }
    }

    #[cfg(test)]
    fn payloads(&self) -> &[[u8; SYNTHETIC_FRAME_PAYLOAD_BYTES]] {
        &self.payloads
    }
}

impl MediaPort for SyntheticMediaPort {
    fn start(&mut self) {
        if self.active.is_none() {
            self.active = Some(self.resources.media_port());
        }
    }

    fn next_frame(&mut self) -> Option<MediaFrame> {
        self.active.as_ref()?;
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.checked_add(1)?;

        let frame_seed = self.seed ^ sequence.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        let mut random = SeededRandomSource::new(frame_seed);
        let mut payload = [0; SYNTHETIC_FRAME_PAYLOAD_BYTES];
        random.fill_bytes(&mut payload);
        self.payloads.push(payload);

        Some(MediaFrame {
            sequence,
            timestamp: self.clock.now(),
        })
    }

    fn stop(&mut self) {
        self.active = None;
    }
}

fn record_transition(
    trace: &mut Vec<StateTransition>,
    from: SessionState,
    to: SessionState,
    at: Timestamp,
    cause: Option<&'static str>,
) {
    if from != to {
        trace.push(StateTransition {
            from: from.as_str(),
            to: to.as_str(),
            at_millis: at.as_millis(),
            cause,
        });
    }
}

/// Runs the deterministic pairing scenario through a loopback TLS listener.
pub fn run() -> HarnessReport {
    run_with(ScenarioConfig::default())
}

/// Runs a deterministic harness scenario using the supplied seed and clock.
pub fn run_with(config: ScenarioConfig) -> HarnessReport {
    let runtime = match Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            let mut report = HarnessReport::for_pairing_result(
                SessionState::Idle,
                Some("internal_error"),
                Vec::new(),
            );
            report.scenario = config.scenario.as_str();
            return report;
        }
    };
    let resources = ResourceTracker::default();
    runtime.block_on(run_pairing(config, resources))
}

async fn run_pairing(config: ScenarioConfig, resources: ResourceTracker) -> HarnessReport {
    let mut report = run_pairing_inner(config, resources.clone()).await;
    report.scenario = config.scenario.as_str();
    report.resources = resources.snapshot();
    report
}

async fn run_pairing_inner(config: ScenarioConfig, resources: ResourceTracker) -> HarnessReport {
    let clock = ManualClock::new(config.clock_start);
    let mut core = SessionCore::new(
        clock.clone(),
        SeededRandomSource::new(config.seed),
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

    let mut certificate_random =
        HarnessCertificateRandomSource(SeededRandomSource::new(CERTIFICATE_SEED));
    let certificate =
        match EphemeralCertificate::generate_with_random_source(&mut certificate_random) {
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
    let listener_guard = resources.listener();

    let listener_address = listener.local_addr();
    let advertised_endpoint =
        match Endpoint::from_address(4, "192.168.1.42", listener_address.port()) {
            Some(endpoint) => endpoint,
            None => {
                return HarnessReport::invalid_qr_payload(core.state());
            }
        };
    let pairing_material = match listener.issue_material(PairingMaterialRequest::new(
        material,
        QR_EXPIRES_AT_UNIX,
        vec![advertised_endpoint.clone()],
    )) {
        Ok(pairing_material) => pairing_material,
        Err(_) => {
            return HarnessReport::invalid_qr_payload(core.state());
        }
    };

    let mut mobile_payload =
        match QrPayload::decode(pairing_material.encoded_qr(), QR_CLOCK_START_UNIX) {
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
        return HarnessReport::invalid_qr_payload(core.state());
    };
    if endpoint.address_string().is_none() {
        return HarnessReport::invalid_qr_payload(core.state());
    }
    let address = if endpoint == &advertised_endpoint {
        listener_address
    } else {
        return HarnessReport::invalid_qr_payload(core.state());
    };
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

    let mut state_trace = Vec::new();
    record_transition(
        &mut state_trace,
        SessionState::Idle,
        SessionState::PairingReady,
        core.last_transition_at(),
        None,
    );
    let task_guard = resources.task();
    let desktop_listener = Arc::clone(&listener);
    let server = tokio::spawn(async move {
        let _task_guard = task_guard;
        run_desktop(core, desktop_listener, state_trace).await
    });
    macro_rules! abort_server {
        ($report:expr) => {{
            server.abort();
            let _ = server.await;
            return $report;
        }};
    }
    let mut mobile_channel = match client.connect(address).await {
        Ok(channel) => channel,
        Err(error) => {
            server.abort();
            let _ = server.await;
            return HarnessReport::for_pairing_result(
                SessionState::PairingReady,
                Some(error.code().as_str()),
                Vec::new(),
            );
        }
    };
    let connection_guard = resources.connection();
    let mut mobile = BufReader::new(&mut mobile_channel);
    let mut message_sequence = Vec::with_capacity(4);

    if write_signaling_frame(&mut mobile, &pair_frame)
        .await
        .is_err()
    {
        abort_server!(HarnessReport::for_pairing_result(
            SessionState::Handshaking,
            Some("internal_error"),
            message_sequence,
        ));
    }
    message_sequence.push("pair");

    let paired_frame = match read_signaling_frame(&mut mobile).await {
        Ok(frame) => frame,
        Err(reason_code) => {
            abort_server!(HarnessReport::for_pairing_result(
                SessionState::Handshaking,
                Some(reason_code),
                message_sequence,
            ));
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
            abort_server!(HarnessReport::for_pairing_result(
                SessionState::Handshaking,
                Some("invalid_message"),
                message_sequence,
            ));
        }
        Err(error) => {
            abort_server!(HarnessReport::for_pairing_result(
                SessionState::Handshaking,
                Some(error.code.as_str()),
                message_sequence,
            ));
        }
    }

    let offer_sdp = match Sdp::new(OFFER_SDP) {
        Ok(sdp) => sdp,
        Err(_) => {
            abort_server!(HarnessReport::for_pairing_result(
                SessionState::Negotiating,
                Some("internal_error"),
                message_sequence,
            ));
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
            abort_server!(HarnessReport::for_pairing_result(
                SessionState::Negotiating,
                Some("internal_error"),
                message_sequence,
            ));
        }
    };
    if write_signaling_frame(&mut mobile, &offer_frame)
        .await
        .is_err()
    {
        abort_server!(HarnessReport::for_pairing_result(
            SessionState::Negotiating,
            Some("internal_error"),
            message_sequence,
        ));
    }
    message_sequence.push("offer");

    let answer_frame = match read_signaling_frame(&mut mobile).await {
        Ok(frame) => frame,
        Err(reason_code) => {
            abort_server!(HarnessReport::for_pairing_result(
                SessionState::Negotiating,
                Some(reason_code),
                message_sequence,
            ));
        }
    };
    match parse_signaling(&answer_frame) {
        Ok(SignalingMessage::Answer {
            epoch: answer_epoch,
            ..
        }) if answer_epoch == epoch => message_sequence.push("answer"),
        Ok(_) => {
            abort_server!(HarnessReport::for_pairing_result(
                SessionState::Negotiating,
                Some("invalid_message"),
                message_sequence,
            ));
        }
        Err(error) => {
            abort_server!(HarnessReport::for_pairing_result(
                SessionState::Negotiating,
                Some(error.code.as_str()),
                message_sequence,
            ));
        }
    }

    drop(mobile);
    let DesktopRun {
        mut core,
        mut state_trace,
    } = match server.await {
        Ok(Ok(desktop_run)) => desktop_run,
        Ok(Err(reason_code)) => {
            return HarnessReport::for_pairing_result(
                SessionState::Negotiating,
                Some(reason_code),
                message_sequence,
            );
        }
        Err(_) => {
            return HarnessReport::for_pairing_result(
                SessionState::Negotiating,
                Some("internal_error"),
                message_sequence,
            );
        }
    };
    drop(mobile_channel);
    drop(listener);
    drop(listener_guard);
    drop(connection_guard);

    let mut media_port = SyntheticMediaPort::new(clock.clone(), config.seed, resources.clone());
    media_port.start();
    let mut frames = Vec::new();
    match config.scenario {
        ScenarioKind::Pairing => {
            for _ in 0..2 {
                sleep(Duration::from_millis(FRAME_INTERVAL_MILLIS)).await;
                clock.advance(FRAME_INTERVAL_MILLIS);
                let Some(frame) = media_port.next_frame() else {
                    return HarnessReport::for_pairing_result(
                        core.state(),
                        Some("internal_error"),
                        message_sequence,
                    );
                };
                if frame.sequence == 0 {
                    let previous_state = core.state();
                    if core.handle(SessionEvent::FirstMediaFrame(frame)).is_err() {
                        return HarnessReport::for_pairing_result(
                            core.state(),
                            Some("invalid_transition"),
                            message_sequence,
                        );
                    }
                    record_transition(
                        &mut state_trace,
                        previous_state,
                        core.state(),
                        core.last_transition_at(),
                        None,
                    );
                }
                frames.push(FrameObservation {
                    sequence: frame.sequence,
                    at_millis: frame.timestamp.as_millis(),
                });
            }

            let previous_state = core.state();
            let effects = match core.handle(SessionEvent::EndSession) {
                Ok(effects) => effects,
                Err(error) => {
                    return HarnessReport::for_pairing_result(
                        core.state(),
                        Some(error.code().as_str()),
                        message_sequence,
                    );
                }
            };
            if effects != [Effect::End] {
                return HarnessReport::for_pairing_result(
                    core.state(),
                    Some("internal_error"),
                    message_sequence,
                );
            }
            media_port.stop();
            record_transition(
                &mut state_trace,
                previous_state,
                core.state(),
                core.last_transition_at(),
                None,
            );
            let mut report =
                HarnessReport::for_pairing_result(SessionState::Ended, None, message_sequence);
            report.state_trace = state_trace;
            report.frames = frames;
            report
        }
        ScenarioKind::FirstFrameTimeout => {
            sleep(Duration::from_millis(FIRST_MEDIA_FRAME_TIMEOUT_MILLIS)).await;
            clock.advance(FIRST_MEDIA_FRAME_TIMEOUT_MILLIS);
            let previous_state = core.state();
            let effects = match core.handle(SessionEvent::FirstFrameTimedOut) {
                Ok(effects) => effects,
                Err(error) => {
                    return HarnessReport::for_pairing_result(
                        core.state(),
                        Some(error.code().as_str()),
                        message_sequence,
                    );
                }
            };
            if effects != [Effect::End] {
                return HarnessReport::for_pairing_result(
                    core.state(),
                    Some("internal_error"),
                    message_sequence,
                );
            }
            media_port.stop();
            record_transition(
                &mut state_trace,
                previous_state,
                core.state(),
                core.last_transition_at(),
                Some("timeout"),
            );

            sleep(Duration::from_millis(FRAME_INTERVAL_MILLIS)).await;
            clock.advance(FRAME_INTERVAL_MILLIS);
            let late_frame = MediaFrame {
                sequence: 0,
                timestamp: clock.now(),
            };
            if core
                .handle(SessionEvent::FirstMediaFrame(late_frame))
                .is_ok()
            {
                return HarnessReport::for_pairing_result(
                    core.state(),
                    Some("invalid_transition"),
                    message_sequence,
                );
            }

            let mut report =
                HarnessReport::for_pairing_result(SessionState::Idle, None, message_sequence);
            report.outcome = Outcome::TimedOut;
            report.state_trace = state_trace;
            report.late_frame_discarded = Some(DiscardedFrame {
                sequence: late_frame.sequence,
                at_millis: late_frame.timestamp.as_millis(),
                reason: "timeout",
            });
            report
        }
    }
}

struct DesktopRun<C: remotecam_session_core::Clock, R: RandomSource> {
    core: SessionCore<C, R>,
    state_trace: Vec<StateTransition>,
}

async fn run_desktop<C, R>(
    mut core: SessionCore<C, R>,
    listener: Arc<PairingListener>,
    mut state_trace: Vec<StateTransition>,
) -> Result<DesktopRun<C, R>, &'static str>
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

    let previous_state = core.state();
    let peer_effects = core
        .handle(SessionEvent::PeerConnected)
        .map_err(|error| error.code().as_str())?;
    record_transition(
        &mut state_trace,
        previous_state,
        core.state(),
        core.last_transition_at(),
        None,
    );
    if peer_effects.contains(&Effect::StartMedia) {
        return Err("internal_error");
    }
    let mut desktop = BufReader::new(channel);
    let pair_frame = read_signaling_frame(&mut desktop).await?;
    let previous_state = core.state();
    let paired_effect = core
        .authenticate_first_frame(&pair_frame)
        .map_err(|error| error.code().as_str())?;
    record_transition(
        &mut state_trace,
        previous_state,
        core.state(),
        core.last_transition_at(),
        None,
    );
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

    let answer_sdp = Sdp::new(ANSWER_SDP).map_err(|_| "internal_error")?;
    let answer_frame = SignalingMessage::Answer {
        epoch: core.epoch().ok_or("invalid_transition")?,
        sdp: answer_sdp,
    }
    .encode()
    .map_err(|_| "internal_error")?;
    write_signaling_frame(&mut desktop, &answer_frame).await?;
    let media_effects = core
        .handle(SessionEvent::NegotiationCompleted)
        .map_err(|error| error.code().as_str())?;
    if media_effects != [Effect::StartMedia] {
        return Err("internal_error");
    }
    Ok(DesktopRun { core, state_trace })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn run_virtual(config: ScenarioConfig) -> HarnessReport {
        let runtime = Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .start_paused(true)
            .build()
            .expect("the test runtime can be created");
        runtime.block_on(run_pairing(config, ResourceTracker::default()))
    }

    #[test]
    fn synthetic_payloads_repeat_for_the_same_seed_and_sequence() {
        let resources = ResourceTracker::default();
        let mut first =
            SyntheticMediaPort::new(ManualClock::new(Timestamp::ZERO), 42, resources.clone());
        let mut second =
            SyntheticMediaPort::new(ManualClock::new(Timestamp::ZERO), 42, resources.clone());
        let mut other_seed =
            SyntheticMediaPort::new(ManualClock::new(Timestamp::ZERO), 43, resources);
        first.start();
        second.start();
        other_seed.start();

        assert_eq!(first.next_frame(), second.next_frame());
        other_seed.next_frame();
        assert_eq!(first.payloads(), second.payloads());
        assert_ne!(first.payloads(), other_seed.payloads());
        assert_eq!(first.payloads()[0].len(), SYNTHETIC_FRAME_PAYLOAD_BYTES);
        first.stop();
        second.stop();
        other_seed.stop();
        assert_eq!(first.next_frame(), None);
    }

    #[test]
    fn successful_scenario_reaches_connected_and_releases_resources() {
        let config = ScenarioConfig::default();
        let report = run_virtual(config);

        assert_eq!(report.outcome, Outcome::Connected);
        assert_eq!(report.observed_state, "ended");
        assert_eq!(
            report.frames,
            [
                FrameObservation {
                    sequence: 0,
                    at_millis: FRAME_INTERVAL_MILLIS,
                },
                FrameObservation {
                    sequence: 1,
                    at_millis: 2 * FRAME_INTERVAL_MILLIS,
                },
            ]
        );
        assert!(report.state_trace.iter().any(|transition| {
            transition.from == "negotiating" && transition.to == "connected"
        }));
        assert_eq!(report.resources.listeners.active_after_close, 0);
        assert_eq!(report.resources.connections.active_after_close, 0);
        assert_eq!(report.resources.media_ports.created, 1);
        assert_eq!(report.resources.media_ports.active_after_close, 0);
        assert_eq!(report.resources.tasks.active_after_close, 0);
    }

    #[test]
    fn timeout_scenario_returns_to_idle_and_discards_a_late_frame() {
        let report = run_virtual(ScenarioConfig {
            scenario: ScenarioKind::FirstFrameTimeout,
            seed: 42,
            clock_start: Timestamp::from_millis(100),
        });

        assert_eq!(report.outcome, Outcome::TimedOut);
        assert_eq!(report.observed_state, "idle");
        assert_eq!(report.verdict, Verdict::Pass);
        assert!(report.state_trace.iter().any(|transition| {
            transition.from == "negotiating"
                && transition.to == "idle"
                && transition.at_millis == 100 + FIRST_MEDIA_FRAME_TIMEOUT_MILLIS
                && transition.cause == Some("timeout")
        }));
        assert_eq!(
            report.late_frame_discarded,
            Some(DiscardedFrame {
                sequence: 0,
                at_millis: 100 + FIRST_MEDIA_FRAME_TIMEOUT_MILLIS + FRAME_INTERVAL_MILLIS,
                reason: "timeout",
            })
        );
        assert_eq!(report.resources.listeners.active_after_close, 0);
        assert_eq!(report.resources.connections.active_after_close, 0);
        assert_eq!(report.resources.media_ports.created, 1);
        assert_eq!(report.resources.media_ports.active_after_close, 0);
        assert_eq!(report.resources.tasks.active_after_close, 0);
    }

    #[test]
    fn reports_are_identical_for_the_same_seed_and_clock() {
        let config = ScenarioConfig {
            scenario: ScenarioKind::Pairing,
            seed: 42,
            clock_start: Timestamp::from_millis(100),
        };

        assert_eq!(run_virtual(config), run_virtual(config));
    }
}
