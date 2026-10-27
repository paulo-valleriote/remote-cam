use remotecam_media_core::loss_scenario::{LossReport, run_loss_profile};
use remotecam_media_core::output::MAX_REPEAT;
use remotecam_media_core::testing::{LossProfile, ProfileLabel, SyntheticPhone};
use remotecam_media_core::{FRAME_RING_SLOTS, MediaCore, RingMediaPort};
use remotecam_pairing::{
    EphemeralCertificate, PairingListener, PairingMaterialRequest, PinnedClient, UnixClock,
};
use remotecam_protocol::ReasonCode;
use remotecam_protocol::signaling::{
    AnswerSdp, Limits, MAX_SIGNALING_MESSAGE_BYTES, OfferSdp, SignalingMessage, parse_signaling,
};
use remotecam_protocol::{Endpoint, QrPayload};
use remotecam_session_core::testing::{ManualClock, SeededRandomSource};
use remotecam_session_core::{
    Clock, Effect, FIRST_MEDIA_FRAME_TIMEOUT_MILLIS, MediaFrame, MediaPort, RandomSource,
    SessionCore, SessionEvent, SessionState, Timestamp,
};
mod acceptance;

use serde::Serialize;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::{ffi::OsString, fmt, process::Command};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::runtime::Builder;
use zeroize::{Zeroize, Zeroizing};

const PAIRING_SEED: u64 = 0;
const CERTIFICATE_SEED: u64 = 1;
const PAIRING_CLOCK_START: Timestamp = Timestamp::ZERO;
const FRAME_INTERVAL_MILLIS: u64 = 33;
const SYNTHETIC_FRAME_PAYLOAD_BYTES: usize = 16;
const QR_CLOCK_START_UNIX: u64 = 1_900_000_000;
const DESKTOP_DTLS_FINGERPRINT: [u8; 32] = [0x44; 32];
const MOBILE_DTLS_FINGERPRINT: [u8; 32] = [0x55; 32];
const OFFER_SDP: &str = "v=0\r\no=- 1 1 IN IP4 0.0.0.0\r\ns=-\r\nt=0 0\r\n";

struct HarnessUnixClock;

impl UnixClock for HarnessUnixClock {
    fn now_unix(&self) -> u64 {
        QR_CLOCK_START_UNIX
    }
}
const ANSWER_SDP: &str = "v=0\r\no=- 1 1 IN IP4 0.0.0.0\r\ns=-\r\nt=0 0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\na=recvonly\r\n";

struct HarnessCertificateRandomSource(SeededRandomSource);

impl RandomSource for HarnessCertificateRandomSource {
    fn fill_bytes(&mut self, destination: &mut [u8]) {
        self.0.fill_bytes(destination);
    }
}

impl remotecam_pairing::certificate::CertificateRandomSource for HarnessCertificateRandomSource {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioKind {
    Pairing,
    PinMismatch,
    FirstFrameTimeout,
    QrExpiryRefused,
    TokenExpiryRefused,
    SingleUseRefused,
    ReplayRefused,
    StaleEpochDiscarded,
    ClockBackwardsWindow,
    RegenerationAfterRefusal,
    RealMediaFrame,
}

pub const REPORTED_SCENARIOS: [ScenarioKind; 10] = [
    ScenarioKind::Pairing,
    ScenarioKind::PinMismatch,
    ScenarioKind::QrExpiryRefused,
    ScenarioKind::TokenExpiryRefused,
    ScenarioKind::SingleUseRefused,
    ScenarioKind::ReplayRefused,
    ScenarioKind::StaleEpochDiscarded,
    ScenarioKind::ClockBackwardsWindow,
    ScenarioKind::RegenerationAfterRefusal,
    ScenarioKind::RealMediaFrame,
];

#[derive(Clone, Copy)]
struct ScenarioDefinition {
    name: &'static str,
    seed_salt: u64,
    expected_outcome: Outcome,
    expected_reason_code: Option<&'static str>,
}

impl ScenarioKind {
    const fn definition(self) -> ScenarioDefinition {
        match self {
            Self::Pairing => ScenarioDefinition {
                name: "pairing-happy-path",
                seed_salt: 0,
                expected_outcome: Outcome::Accepted,
                expected_reason_code: None,
            },
            Self::PinMismatch => ScenarioDefinition {
                name: "pin-mismatch-aborts",
                seed_salt: 0xa5a5_5a5a_d3c1_b7e9,
                expected_outcome: Outcome::Rejected,
                expected_reason_code: Some("certificate_pin_mismatch"),
            },
            Self::FirstFrameTimeout => ScenarioDefinition {
                name: "first_frame_timeout",
                seed_salt: 0,
                expected_outcome: Outcome::TimedOut,
                expected_reason_code: None,
            },
            Self::QrExpiryRefused => ScenarioDefinition {
                name: "qr-expiry-refused",
                seed_salt: 0x0001,
                expected_outcome: Outcome::Rejected,
                expected_reason_code: Some("qr_expired"),
            },
            Self::TokenExpiryRefused => ScenarioDefinition {
                name: "token-expiry-refused",
                seed_salt: 0x0002,
                expected_outcome: Outcome::Rejected,
                expected_reason_code: Some("token_expired"),
            },
            Self::SingleUseRefused => ScenarioDefinition {
                name: "single-use-refused",
                seed_salt: 0x0003,
                expected_outcome: Outcome::Rejected,
                expected_reason_code: Some("superseded"),
            },
            Self::ReplayRefused => ScenarioDefinition {
                name: "replay-refused",
                seed_salt: 0x0004,
                expected_outcome: Outcome::Rejected,
                expected_reason_code: Some("token_already_used"),
            },
            Self::StaleEpochDiscarded => ScenarioDefinition {
                name: "stale-epoch-discarded",
                seed_salt: 0x0005,
                expected_outcome: Outcome::Accepted,
                expected_reason_code: None,
            },
            Self::ClockBackwardsWindow => ScenarioDefinition {
                name: "clock-backwards-window",
                seed_salt: 0x0006,
                expected_outcome: Outcome::Rejected,
                expected_reason_code: Some("token_expired"),
            },
            Self::RegenerationAfterRefusal => ScenarioDefinition {
                name: "regeneration-after-refusal",
                seed_salt: 0x0007,
                expected_outcome: Outcome::Accepted,
                expected_reason_code: None,
            },
            Self::RealMediaFrame => ScenarioDefinition {
                name: "real-media-frame",
                seed_salt: 0x0008,
                expected_outcome: Outcome::Accepted,
                expected_reason_code: None,
            },
        }
    }

    pub const fn is_acceptance(self) -> bool {
        matches!(
            self,
            Self::QrExpiryRefused
                | Self::TokenExpiryRefused
                | Self::SingleUseRefused
                | Self::ReplayRefused
                | Self::StaleEpochDiscarded
                | Self::ClockBackwardsWindow
                | Self::RegenerationAfterRefusal
        )
    }

    const fn as_str(self) -> &'static str {
        self.definition().name
    }

    const fn seed(self, seed: u64) -> u64 {
        seed ^ self.definition().seed_salt
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenarioConfig {
    pub scenario: ScenarioKind,
    pub seed: u64,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunOptions {
    pub seed: u64,
    pub clock_start_millis: u64,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            seed: PAIRING_SEED,
            clock_start_millis: PAIRING_CLOCK_START.as_millis(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessError(String);

impl fmt::Display for HarnessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for HarnessError {}

fn ensure_success(success: bool, message: &'static str) -> Result<(), HarnessError> {
    if success {
        Ok(())
    } else {
        Err(HarnessError(message.to_owned()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnvironmentReport {
    pub os: &'static str,
    pub architecture: &'static str,
    pub rustc_version: String,
}

impl EnvironmentReport {
    fn current() -> Result<Self, HarnessError> {
        let output = Command::new("rustc")
            .arg("--version")
            .output()
            .map_err(|_| HarnessError("failed to execute rustc --version".to_owned()))?;
        ensure_success(output.status.success(), "rustc --version failed")?;
        let version = parse_rustc_version(&output.stdout)?;

        Ok(Self {
            os: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
            rustc_version: version,
        })
    }
}

fn parse_rustc_version(output: &[u8]) -> Result<String, HarnessError> {
    let output = String::from_utf8(output.to_vec())
        .map_err(|_| HarnessError("rustc --version returned an invalid version".to_owned()))?;
    let mut fields = output.split_whitespace();
    if fields.next() != Some("rustc") {
        return Err(HarnessError(
            "rustc --version returned an invalid version".to_owned(),
        ));
    }
    fields
        .next()
        .map(str::to_owned)
        .ok_or_else(|| HarnessError("rustc --version returned an invalid version".to_owned()))
}

fn current_commit() -> Result<String, HarnessError> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .map_err(|_| HarnessError("failed to execute git rev-parse HEAD".to_owned()))?;
    ensure_success(output.status.success(), "git rev-parse HEAD failed")?;

    parse_commit(&output.stdout)
}

fn parse_commit(output: &[u8]) -> Result<String, HarnessError> {
    let commit = String::from_utf8(output.to_vec())
        .ok()
        .map(|output| output.trim().to_owned())
        .filter(|commit| {
            matches!(commit.len(), 40 | 64) && commit.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        .ok_or_else(|| HarnessError("git rev-parse HEAD returned an invalid commit".to_owned()))?;
    Ok(commit)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Pass,
    Fail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub scenarios: u32,
    pub passed: u32,
    pub failed: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Accepted,
    Rejected,
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StateTransition {
    pub from: &'static str,
    pub to: &'static str,
    pub at_millis: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FrameObservation {
    pub sequence: u64,
    pub at_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscardedFrame {
    pub sequence: u64,
    pub at_millis: u64,
    pub reason: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ResourceCount {
    pub created: usize,
    pub active_after_close: usize,
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HarnessReport {
    pub scenario: &'static str,
    pub outcome: Outcome,
    pub observed_state: &'static str,
    pub message_sequence: Vec<&'static str>,
    pub state_trace: Vec<StateTransition>,
    pub frames: Vec<FrameObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub late_frame_discarded: Option<DiscardedFrame>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub issued_epochs: Vec<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol_violations: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub real_media: Option<RealMediaObservation>,
    pub resources: ResourceReport,
    pub verdict: Verdict,
    pub reason_code: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RealMediaObservation {
    pub pixel_format: &'static str,
    pub width: u32,
    pub height: u32,
    pub frames_decoded: u64,
    pub frames_dropped: u64,
    pub ring_slots: usize,
    pub receive_only: bool,
    pub loss_recovery: LossRecoveryObservation,
}

/// Video under the single nominal loss profile (TDD §5.5): NACK and PLI counts,
/// residual loss and the longest stretch without a fresh frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LossRecoveryObservation {
    pub profile: &'static str,
    pub duration_seconds: u64,
    pub packets_sent: u32,
    pub packets_lost: u64,
    pub packets_late: u64,
    pub nack_requests: u64,
    pub nacked_packets: u64,
    pub plis: u64,
    pub fresh_frames: u32,
    pub repeated_frames: u32,
    pub absent_ticks: u32,
    pub longest_without_fresh_millis: u64,
    pub jitter_depth_micros: u64,
}

const LOSS_RECOVERY_SECONDS: u64 = 10;

impl LossRecoveryObservation {
    fn from_report(report: &LossReport) -> Self {
        Self {
            profile: match report.label {
                ProfileLabel::Nominal => "nominal",
                ProfileLabel::OutOfProfile => "out_of_profile",
            },
            duration_seconds: report.seconds,
            packets_sent: report.packets_sent,
            packets_lost: report.packets_lost,
            packets_late: report.packets_late,
            nack_requests: report.nack_requests,
            nacked_packets: report.nacked_packets,
            plis: report.plis,
            fresh_frames: report.fresh_frames,
            repeated_frames: report.repeated_frames,
            absent_ticks: report.absent_ticks,
            longest_without_fresh_millis: u64::try_from(report.longest_without_fresh.as_millis())
                .unwrap_or(u64::MAX),
            jitter_depth_micros: u64::try_from(report.jitter_depth.as_micros()).unwrap_or(u64::MAX),
        }
    }

    /// A gate passes only on the nominal profile, with no absence and no freeze of 2 s or more.
    fn passes_gate(&self, report: &LossReport) -> bool {
        report.label == ProfileLabel::Nominal
            && report.absent_ticks == 0
            && report.longest_without_fresh < MAX_REPEAT
    }
}

impl HarnessReport {
    fn for_pairing_result(
        state: SessionState,
        reason_code: Option<&'static str>,
        message_sequence: Vec<&'static str>,
    ) -> Self {
        let outcome = if reason_code.is_some() {
            Outcome::Rejected
        } else {
            Outcome::Accepted
        };
        Self {
            scenario: ScenarioKind::Pairing.as_str(),
            outcome,
            observed_state: state.as_str(),
            message_sequence,
            state_trace: Vec::new(),
            frames: Vec::new(),
            late_frame_discarded: None,
            issued_epochs: Vec::new(),
            protocol_violations: None,
            real_media: None,
            resources: ResourceReport::default(),
            verdict: if outcome == Outcome::Accepted {
                Verdict::Pass
            } else {
                Verdict::Fail
            },
            reason_code,
        }
    }

    fn apply_scenario(&mut self, scenario: ScenarioKind) {
        let definition = scenario.definition();
        self.scenario = definition.name;
        self.verdict = if self.outcome == definition.expected_outcome
            && self.reason_code == definition.expected_reason_code
        {
            Verdict::Pass
        } else {
            Verdict::Fail
        };
    }
}

struct ScenarioFailure {
    scenario: ScenarioKind,
    state: SessionState,
    reason_code: &'static str,
    message_sequence: Vec<&'static str>,
}

impl ScenarioFailure {
    fn into_report(self) -> HarnessReport {
        let mut report = HarnessReport::for_pairing_result(
            self.state,
            Some(self.reason_code),
            self.message_sequence,
        );
        report.apply_scenario(self.scenario);
        report
    }
}

fn map_scenario_error<T, E>(
    result: Result<T, E>,
    scenario: ScenarioKind,
    state: SessionState,
    message_sequence: Vec<&'static str>,
    reason_code: impl FnOnce(E) -> &'static str,
) -> Result<T, ScenarioFailure> {
    result.map_err(|error| ScenarioFailure {
        scenario,
        state,
        reason_code: reason_code(error),
        message_sequence,
    })
}

fn failed_report(
    scenario: ScenarioKind,
    state: SessionState,
    reason_code: &'static str,
    message_sequence: Vec<&'static str>,
) -> Result<HarnessReport, ScenarioFailure> {
    Err(scenario_failure(
        scenario,
        state,
        reason_code,
        message_sequence,
    ))
}

fn scenario_failure(
    scenario: ScenarioKind,
    state: SessionState,
    reason_code: &'static str,
    message_sequence: Vec<&'static str>,
) -> ScenarioFailure {
    ScenarioFailure {
        scenario,
        state,
        reason_code,
        message_sequence,
    }
}

fn resolve_listener_address(
    endpoints: &[Endpoint],
    advertised_endpoint: &Endpoint,
    listener_address: std::net::SocketAddr,
) -> Result<std::net::SocketAddr, &'static str> {
    let Some(endpoint) = endpoints.first() else {
        return Err("invalid_qr_payload");
    };
    if endpoint.address_string().is_none() || endpoint != advertised_endpoint {
        return Err("invalid_qr_payload");
    }
    Ok(listener_address)
}

fn pairing_material(
    effects: &[Effect],
) -> Result<&remotecam_session_core::PairingMaterial, &'static str> {
    match effects {
        [Effect::PresentPairing(material)] => Ok(material),
        _ => Err("internal_error"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunReport {
    pub report_version: u32,
    pub contract_version: u32,
    pub commit: String,
    pub seed: u64,
    pub clock_start_millis: u64,
    pub environment: EnvironmentReport,
    pub scenarios: Vec<HarnessReport>,
    pub verdict: Verdict,
    pub summary: Summary,
}

impl RunReport {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    fn new(
        options: RunOptions,
        commit: String,
        environment: EnvironmentReport,
        scenarios: Vec<HarnessReport>,
    ) -> Self {
        let passed = scenarios
            .iter()
            .filter(|scenario| scenario.verdict == Verdict::Pass)
            .count() as u32;
        let failed = scenarios.len() as u32 - passed;
        Self {
            report_version: 6,
            contract_version: 1,
            commit,
            seed: options.seed,
            clock_start_millis: options.clock_start_millis,
            environment,
            scenarios,
            verdict: if failed == 0 {
                Verdict::Pass
            } else {
                Verdict::Fail
            },
            summary: Summary {
                scenarios: passed + failed,
                passed,
                failed,
            },
        }
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

pub fn run() -> Result<RunReport, HarnessError> {
    run_with_options(RunOptions::default())
}

pub fn run_from_args(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<RunReport, HarnessError> {
    let options = parse_run_options(arguments)?;
    run_with_options(options)
}

pub fn run_with_options(options: RunOptions) -> Result<RunReport, HarnessError> {
    let commit = current_commit()?;
    let environment = EnvironmentReport::current()?;
    let clock_start = Timestamp::from_millis(options.clock_start_millis);
    let scenarios = REPORTED_SCENARIOS
        .into_iter()
        .map(|scenario| {
            run_with(ScenarioConfig {
                scenario,
                seed: scenario.seed(options.seed),
                clock_start,
            })
        })
        .collect();

    Ok(RunReport::new(options, commit, environment, scenarios))
}

fn parse_run_options(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<RunOptions, HarnessError> {
    let mut options = RunOptions::default();
    let mut seed_seen = false;
    let mut clock_seen = false;
    let mut arguments = arguments.into_iter();

    while let Some(argument) = arguments.next() {
        let argument = argument
            .to_str()
            .ok_or_else(|| HarnessError("arguments must be valid UTF-8".to_owned()))?;
        let value = arguments
            .next()
            .ok_or_else(|| HarnessError(format!("missing value for {argument}")))?;
        let value = value
            .to_str()
            .ok_or_else(|| HarnessError("arguments must be valid UTF-8".to_owned()))?;
        match argument {
            "--seed" if !seed_seen => {
                options.seed = value
                    .parse()
                    .map_err(|_| HarnessError("invalid u64 value for --seed".to_owned()))?;
                seed_seen = true;
            }
            "--clock-start-millis" if !clock_seen => {
                options.clock_start_millis = value.parse().map_err(|_| {
                    HarnessError("invalid u64 value for --clock-start-millis".to_owned())
                })?;
                clock_seen = true;
            }
            "--seed" | "--clock-start-millis" => {
                return Err(HarnessError(format!("duplicate argument: {argument}")));
            }
            _ => return Err(HarnessError(format!("unknown argument: {argument}"))),
        }
    }

    Ok(options)
}

pub fn run_with(config: ScenarioConfig) -> HarnessReport {
    run_with_runtime(
        config,
        Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build(),
    )
}

fn run_with_runtime(
    config: ScenarioConfig,
    runtime: Result<tokio::runtime::Runtime, std::io::Error>,
) -> HarnessReport {
    let runtime = match runtime {
        Ok(runtime) => runtime,
        Err(_) => {
            let mut report = HarnessReport::for_pairing_result(
                SessionState::Idle,
                Some("internal_error"),
                Vec::new(),
            );
            report.apply_scenario(config.scenario);
            return report;
        }
    };
    let resources = ResourceTracker::default();
    runtime.block_on(run_pairing(config, resources))
}

async fn run_pairing(config: ScenarioConfig, resources: ResourceTracker) -> HarnessReport {
    if config.scenario.is_acceptance() {
        return acceptance::run(config, resources).await;
    }
    finish_pairing_result(
        config,
        resources.clone(),
        run_pairing_inner(config, resources).await,
    )
}

fn finish_pairing_result(
    config: ScenarioConfig,
    resources: ResourceTracker,
    result: Result<HarnessReport, ScenarioFailure>,
) -> HarnessReport {
    let mut report = match result {
        Ok(report) => report,
        Err(failure) => failure.into_report(),
    };
    report.scenario = config.scenario.as_str();
    report.resources = resources.snapshot();
    report.apply_scenario(config.scenario);
    report
}

async fn run_pairing_inner(
    config: ScenarioConfig,
    resources: ResourceTracker,
) -> Result<HarnessReport, ScenarioFailure> {
    let clock = ManualClock::new(config.clock_start);
    let mut core = SessionCore::new(
        clock.clone(),
        SeededRandomSource::new(config.seed),
        DESKTOP_DTLS_FINGERPRINT,
    );

    let pairing_effects = map_scenario_error(
        core.handle(SessionEvent::StartPairing),
        config.scenario,
        core.state(),
        Vec::new(),
        |error| error.code().as_str(),
    )?;
    let material = pairing_material(pairing_effects.as_slice())
        .map_err(|reason| scenario_failure(config.scenario, core.state(), reason, Vec::new()))?;

    let mut certificate_random =
        HarnessCertificateRandomSource(SeededRandomSource::new(config.seed ^ CERTIFICATE_SEED));
    let certificate = map_scenario_error(
        EphemeralCertificate::generate_with_random_source(&mut certificate_random),
        config.scenario,
        core.state(),
        Vec::new(),
        |_| "internal_error",
    )?;
    let listener = Arc::new(map_scenario_error(
        PairingListener::bind_loopback(certificate).await,
        config.scenario,
        core.state(),
        Vec::new(),
        |_| "internal_error",
    )?);
    let listener_guard = resources.listener();

    let listener_address = listener.local_addr();
    let advertised_endpoint =
        match Endpoint::from_address(4, "192.168.1.42", listener_address.port()) {
            Some(endpoint) => endpoint,
            None => {
                return failed_report(
                    config.scenario,
                    core.state(),
                    "invalid_qr_payload",
                    Vec::new(),
                );
            }
        };
    let pairing_material = map_scenario_error(
        listener.issue_material(
            PairingMaterialRequest::new(material, vec![advertised_endpoint.clone()]),
            &HarnessUnixClock,
        ),
        config.scenario,
        core.state(),
        Vec::new(),
        |_| "invalid_qr_payload",
    )?;

    let mut mobile_payload = map_scenario_error(
        QrPayload::decode(pairing_material.encoded_qr(), QR_CLOCK_START_UNIX),
        config.scenario,
        core.state(),
        Vec::new(),
        |error| error.code.as_str(),
    )?;
    let mobile_token = Zeroizing::new(mobile_payload.pairing_token);
    mobile_payload.pairing_token.zeroize();
    let address = resolve_listener_address(
        &mobile_payload.endpoints,
        &advertised_endpoint,
        listener_address,
    )
    .map_err(|reason| scenario_failure(config.scenario, core.state(), reason, Vec::new()))?;
    let session_id = mobile_payload.session_id;
    let epoch = u64::from(mobile_payload.epoch);
    let mut pinned_fingerprint = mobile_payload.tls_fingerprint;
    if config.scenario == ScenarioKind::PinMismatch {
        pinned_fingerprint[0] ^= 0xff;
    }
    let client = map_scenario_error(
        PinnedClient::new(pinned_fingerprint),
        config.scenario,
        core.state(),
        Vec::new(),
        |error| error.code().as_str(),
    )?;

    let mut pair_message = SignalingMessage::Pair {
        token: *mobile_token,
        dtls_fingerprint: MOBILE_DTLS_FINGERPRINT,
        limits: Limits::new(),
    };
    let pair_frame = pair_message.encode();
    if let SignalingMessage::Pair { token, .. } = &mut pair_message {
        token.zeroize();
    }
    let pair_frame = Zeroizing::new(map_scenario_error(
        pair_frame,
        config.scenario,
        core.state(),
        Vec::new(),
        |_| "internal_error",
    )?);

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
    let server_state_trace = state_trace.clone();
    let server = tokio::spawn(async move {
        let _task_guard = task_guard;
        desktop_session(core, desktop_listener, server_state_trace).await
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
            if config.scenario == ScenarioKind::PinMismatch {
                let client_reason = error.code().as_str();
                let server_reason = server
                    .await
                    .map(|result| result.err().unwrap_or("internal_error"))
                    .unwrap_or("internal_error");
                let reason_code = if server_reason == client_reason {
                    server_reason
                } else {
                    "internal_error"
                };
                let mut report = HarnessReport::for_pairing_result(
                    SessionState::PairingReady,
                    Some(reason_code),
                    Vec::new(),
                );
                report.state_trace = state_trace;
                return Ok(report);
            }
            server.abort();
            let _ = server.await;
            return failed_report(
                config.scenario,
                SessionState::PairingReady,
                error.code().as_str(),
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
        abort_server!(failed_report(
            config.scenario,
            SessionState::Handshaking,
            "internal_error",
            message_sequence,
        ));
    }
    message_sequence.push("pair");

    let paired_frame = match read_signaling_frame(&mut mobile).await {
        Ok(frame) => frame,
        Err(reason_code) => {
            abort_server!(failed_report(
                config.scenario,
                SessionState::Handshaking,
                reason_code,
                message_sequence,
            ));
        }
    };
    if let Err(reason_code) =
        validate_paired_frame(&paired_frame, session_id, epoch, DESKTOP_DTLS_FINGERPRINT)
    {
        abort_server!(failed_report(
            config.scenario,
            SessionState::Handshaking,
            reason_code,
            message_sequence,
        ));
    }
    message_sequence.push("paired");

    let offer_sdp = match OfferSdp::new(OFFER_SDP) {
        Ok(sdp) => sdp,
        Err(_) => abort_server!(failed_report(
            config.scenario,
            SessionState::Negotiating,
            "internal_error",
            message_sequence,
        )),
    };
    let offer_frame = match (SignalingMessage::Offer {
        epoch,
        sdp: offer_sdp,
    })
    .encode()
    {
        Ok(frame) => frame,
        Err(_) => abort_server!(failed_report(
            config.scenario,
            SessionState::Negotiating,
            "internal_error",
            message_sequence,
        )),
    };
    if write_signaling_frame(&mut mobile, &offer_frame)
        .await
        .is_err()
    {
        abort_server!(failed_report(
            config.scenario,
            SessionState::Negotiating,
            "internal_error",
            message_sequence,
        ));
    }
    message_sequence.push("offer");

    let answer_frame = match read_signaling_frame(&mut mobile).await {
        Ok(frame) => frame,
        Err(reason_code) => {
            abort_server!(failed_report(
                config.scenario,
                SessionState::Negotiating,
                reason_code,
                message_sequence,
            ));
        }
    };
    if let Err(reason_code) = validate_answer_frame(&answer_frame, epoch) {
        abort_server!(failed_report(
            config.scenario,
            SessionState::Negotiating,
            reason_code,
            message_sequence,
        ));
    }
    message_sequence.push("answer");

    drop(mobile);
    let DesktopRun {
        mut core,
        mut state_trace,
    } = match server.await {
        Ok(Ok(desktop_run)) => desktop_run,
        Ok(Err(reason_code)) => {
            return failed_report(
                config.scenario,
                SessionState::Negotiating,
                reason_code,
                message_sequence,
            );
        }
        Err(_) => {
            return failed_report(
                config.scenario,
                SessionState::Negotiating,
                "internal_error",
                message_sequence,
            );
        }
    };
    drop(mobile_channel);
    drop(listener);
    drop(listener_guard);
    drop(connection_guard);

    let mut synthetic_port = SyntheticMediaPort::new(clock.clone(), config.seed, resources.clone());
    let mut real_media = if config.scenario == ScenarioKind::RealMediaFrame {
        match RealMediaSession::start(&clock, &resources).await {
            Ok(session) => Some(session),
            Err(reason_code) => {
                return failed_report(config.scenario, core.state(), reason_code, message_sequence);
            }
        }
    } else {
        None
    };
    let media_port: &mut dyn MediaPort = match real_media.as_mut() {
        Some(session) => &mut session.port,
        None => &mut synthetic_port,
    };
    media_port.start();
    let mut frames = Vec::new();
    let mut report = match config.scenario {
        ScenarioKind::Pairing | ScenarioKind::PinMismatch | ScenarioKind::RealMediaFrame => {
            for _ in 0..2 {
                clock.advance(FRAME_INTERVAL_MILLIS);
                let Some(frame) = media_port.next_frame() else {
                    return failed_report(
                        config.scenario,
                        core.state(),
                        "internal_error",
                        message_sequence,
                    );
                };
                if frame.sequence == 0 {
                    let previous_state = core.state();
                    if core.handle(SessionEvent::FirstMediaFrame(frame)).is_err() {
                        return failed_report(
                            config.scenario,
                            core.state(),
                            "invalid_transition",
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
                    return failed_report(
                        config.scenario,
                        core.state(),
                        error.code().as_str(),
                        message_sequence,
                    );
                }
            };
            if effects != [Effect::End] {
                return failed_report(
                    config.scenario,
                    core.state(),
                    "internal_error",
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
        ScenarioKind::QrExpiryRefused
        | ScenarioKind::TokenExpiryRefused
        | ScenarioKind::SingleUseRefused
        | ScenarioKind::ReplayRefused
        | ScenarioKind::StaleEpochDiscarded
        | ScenarioKind::ClockBackwardsWindow
        | ScenarioKind::RegenerationAfterRefusal => {
            unreachable!("acceptance scenarios are dispatched before the pairing flow")
        }
        ScenarioKind::FirstFrameTimeout => {
            clock.advance(FIRST_MEDIA_FRAME_TIMEOUT_MILLIS + 1);
            let previous_state = core.state();
            let effects = match core.handle(SessionEvent::FirstFrameTimedOut) {
                Ok(effects) => effects,
                Err(error) => {
                    return failed_report(
                        config.scenario,
                        core.state(),
                        error.code().as_str(),
                        message_sequence,
                    );
                }
            };
            if effects != [Effect::End] {
                return failed_report(
                    config.scenario,
                    core.state(),
                    "internal_error",
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

            clock.advance(FRAME_INTERVAL_MILLIS - 1);
            let late_frame = MediaFrame {
                sequence: 0,
                timestamp: clock.now(),
            };
            if core
                .handle(SessionEvent::FirstMediaFrame(late_frame))
                .is_ok()
            {
                return failed_report(
                    config.scenario,
                    core.state(),
                    "invalid_transition",
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
    };
    if let Some(session) = real_media {
        match session.finish().await {
            Ok(observation) => report.real_media = Some(observation),
            Err(reason_code) => {
                return failed_report(
                    config.scenario,
                    SessionState::Ended,
                    reason_code,
                    report.message_sequence,
                );
            }
        }
    }
    Ok(report)
}

const MEDIA_BIND_ADDRESS: &str = "127.0.0.1:0";
const REAL_MEDIA_FRAMES: u64 = 2;
const REAL_MEDIA_DEADLINE: std::time::Duration = std::time::Duration::from_secs(15);

fn media_failure<E>(_: E) -> &'static str {
    "internal_error"
}

struct RealMediaSession {
    core: Arc<MediaCore>,
    phone: SyntheticPhone,
    port: RingMediaPort<ManualClock>,
    _guard: ResourceGuard,
}

impl RealMediaSession {
    async fn start(clock: &ManualClock, resources: &ResourceTracker) -> Result<Self, &'static str> {
        let core = Arc::new(
            MediaCore::new(MEDIA_BIND_ADDRESS)
                .await
                .map_err(media_failure)?,
        );
        let mut phone = SyntheticPhone::new(MEDIA_BIND_ADDRESS)
            .await
            .map_err(media_failure)?;
        let offer = phone.offer().await.map_err(media_failure)?;
        let answer = core.answer(&offer).await.map_err(media_failure)?;
        phone.accept_answer(&answer).await.map_err(media_failure)?;
        phone.connected().await.map_err(media_failure)?;
        for _ in 0..REAL_MEDIA_FRAMES {
            phone.send_frame().await.map_err(media_failure)?;
        }
        tokio::time::timeout(REAL_MEDIA_DEADLINE, async {
            while core.stats().decoded_frames() < REAL_MEDIA_FRAMES {
                tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            }
        })
        .await
        .map_err(media_failure)?;
        Ok(Self {
            port: RingMediaPort::new(Arc::clone(&core), clock.clone()),
            core,
            phone,
            _guard: resources.media_port(),
        })
    }

    async fn finish(self) -> Result<RealMediaObservation, &'static str> {
        let receive_only = self.core.is_receive_only().await;
        let (width, height) = self.port.last_dimensions().ok_or("internal_error")?;
        let loss = run_loss_profile(LossProfile::NOMINAL, LOSS_RECOVERY_SECONDS);
        let loss_recovery = LossRecoveryObservation::from_report(&loss);
        let observation = RealMediaObservation {
            pixel_format: "nv12",
            width,
            height,
            frames_decoded: self.core.stats().decoded_frames(),
            frames_dropped: self.core.ring().dropped(),
            ring_slots: FRAME_RING_SLOTS,
            receive_only,
            loss_recovery,
        };
        self.core.close().await.map_err(media_failure)?;
        self.phone.close().await.map_err(media_failure)?;
        if !loss_recovery.passes_gate(&loss) {
            return Err("media_failure");
        }
        Ok(observation)
    }
}

struct DesktopRun<C: Clock, R: RandomSource> {
    core: SessionCore<C, R>,
    state_trace: Vec<StateTransition>,
}

async fn desktop_session<C, R>(
    mut core: SessionCore<C, R>,
    listener: Arc<PairingListener>,
    mut state_trace: Vec<StateTransition>,
) -> Result<DesktopRun<C, R>, &'static str>
where
    C: Clock + Send + 'static,
    R: RandomSource + Send + 'static,
{
    let mut regenerated = Vec::new();
    run_desktop(&mut core, &listener, &mut state_trace, &mut regenerated).await?;
    Ok(DesktopRun { core, state_trace })
}

fn split_refusal(
    effects: Vec<Effect>,
    regenerated: &mut Vec<remotecam_session_core::PairingMaterial>,
) -> Result<Vec<Effect>, ReasonCode> {
    let mut refusal = None;
    let mut remaining = Vec::with_capacity(effects.len());
    for effect in effects {
        match effect {
            Effect::RejectPeer { reason } => refusal = Some(reason),
            Effect::PresentPairing(material) => regenerated.push(material),
            other => remaining.push(other),
        }
    }
    match refusal {
        Some(reason) => Err(reason),
        None => Ok(remaining),
    }
}

async fn run_desktop<C, R>(
    core: &mut SessionCore<C, R>,
    listener: &PairingListener,
    state_trace: &mut Vec<StateTransition>,
    regenerated: &mut Vec<remotecam_session_core::PairingMaterial>,
) -> Result<(), &'static str>
where
    C: Clock,
    R: RandomSource,
{
    let channel = listener
        .accept_admitted(|| {
            let previous_state = core.state();
            let effects = match core.handle(SessionEvent::PeerConnected) {
                Ok(effects) => effects,
                Err(_) => return None,
            };
            record_transition(
                state_trace,
                previous_state,
                core.state(),
                core.last_transition_at(),
                None,
            );
            split_refusal(effects, regenerated).err()
        })
        .await
        .map_err(|error| error.code().as_str())?;
    if !channel.is_tls13() {
        return Err("tls_version_unsupported");
    }

    let mut desktop = BufReader::new(channel);
    let pair_frame = read_signaling_frame(&mut desktop).await?;
    let previous_state = core.state();
    let paired_effects = core.authenticate_first_frame(&pair_frame);
    record_transition(
        state_trace,
        previous_state,
        core.state(),
        core.last_transition_at(),
        None,
    );
    let paired_effects = paired_effects.map_err(|error| error.code().as_str())?;
    let paired_effects =
        split_refusal(paired_effects, regenerated).map_err(|reason| reason.as_str())?;
    let (session_id, epoch, dtls_fingerprint, accepted_limits) = match paired_effects.as_slice() {
        [
            Effect::SendPaired {
                session_id,
                epoch,
                dtls_fingerprint,
                accepted_limits,
            },
        ] => (
            *session_id,
            *epoch,
            *dtls_fingerprint,
            accepted_limits.clone(),
        ),
        _ => return Err("internal_error"),
    };
    let paired_frame = SignalingMessage::Paired {
        session_id: *session_id.as_bytes(),
        epoch: u64::from(epoch),
        dtls_fingerprint,
        accepted_limits,
    }
    .encode()
    .map_err(|_| "internal_error")?;
    write_signaling_frame(&mut desktop, &paired_frame).await?;

    read_offer_frame(&mut desktop, core).await?;

    let answer_sdp = AnswerSdp::new(ANSWER_SDP).map_err(|_| "internal_error")?;
    let answer_frame = SignalingMessage::Answer {
        epoch: u64::from(core.epoch().ok_or("invalid_transition")?),
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
    Ok(())
}

async fn read_offer_frame<C, R>(
    reader: &mut (impl AsyncRead + Unpin),
    core: &mut SessionCore<C, R>,
) -> Result<(), &'static str>
where
    C: Clock,
    R: RandomSource,
{
    loop {
        let frame = read_signaling_frame(reader).await?;
        let offer_epoch = match core.admit_signaling_frame(&frame) {
            Ok(SignalingMessage::Offer { epoch, .. }) => epoch,
            Ok(_) => return Err("invalid_message"),
            Err(refusal) => return Err(refusal.code().as_str()),
        };
        let Ok(frame_epoch) = u32::try_from(offer_epoch) else {
            return Err("invalid_message");
        };
        if !core.admit_frame_epoch(frame_epoch) {
            continue;
        }
        if core.epoch() == Some(frame_epoch) {
            return Ok(());
        }
        return Err("invalid_message");
    }
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
            return Err("security_violation");
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

fn validate_paired_frame(
    frame: &[u8],
    session_id: [u8; 16],
    epoch: u64,
    fingerprint: [u8; 32],
) -> Result<(), &'static str> {
    match parse_signaling(frame) {
        Ok(SignalingMessage::Paired {
            session_id: paired_session_id,
            epoch: paired_epoch,
            dtls_fingerprint,
            accepted_limits,
        }) if paired_session_id == session_id
            && paired_epoch == epoch
            && dtls_fingerprint == fingerprint
            && accepted_limits == Limits::new() =>
        {
            Ok(())
        }
        Ok(_) => Err("invalid_message"),
        Err(error) => Err(error.code.as_str()),
    }
}

fn validate_answer_frame(frame: &[u8], epoch: u64) -> Result<(), &'static str> {
    match parse_signaling(frame) {
        Ok(SignalingMessage::Answer {
            epoch: answer_epoch,
            ..
        }) if answer_epoch == epoch => Ok(()),
        Ok(_) => Err("invalid_message"),
        Err(error) => Err(error.code.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use remotecam_session_core::PAIRING_WINDOW_MILLIS;

    fn test_runtime() -> tokio::runtime::Runtime {
        Builder::new_current_thread()
            .enable_io()
            .build()
            .expect("the test runtime can be created")
    }

    async fn connected_test_desktop() -> (
        tokio::task::JoinHandle<Result<DesktopRun<ManualClock, SeededRandomSource>, &'static str>>,
        remotecam_pairing::ClientChannel,
        Zeroizing<[u8; 32]>,
    ) {
        let certificate = EphemeralCertificate::generate().expect("certificate");
        let fingerprint = certificate.fingerprint();
        let listener = Arc::new(
            PairingListener::bind_loopback(certificate)
                .await
                .expect("listener"),
        );
        let mut core = SessionCore::new(
            ManualClock::new(Timestamp::ZERO),
            SeededRandomSource::new(42),
            DESKTOP_DTLS_FINGERPRINT,
        );
        let effects = core
            .handle(SessionEvent::StartPairing)
            .expect("pairing starts");
        let [Effect::PresentPairing(material)] = effects.as_slice() else {
            panic!("starting pairing presents pairing material");
        };
        let token = Zeroizing::new(*material.token().as_bytes());
        let server = tokio::spawn(desktop_session(core, Arc::clone(&listener), Vec::new()));
        let client = PinnedClient::new(fingerprint)
            .expect("client")
            .connect(listener.local_addr())
            .await
            .expect("TLS connection");
        (server, client, token)
    }

    fn test_pair_frame(token: &[u8; 32]) -> Zeroizing<Vec<u8>> {
        let mut message = SignalingMessage::Pair {
            token: *token,
            dtls_fingerprint: MOBILE_DTLS_FINGERPRINT,
            limits: Limits::new(),
        };
        let frame = message.encode().expect("pairing frame");
        if let SignalingMessage::Pair { token, .. } = &mut message {
            token.zeroize();
        }
        Zeroizing::new(frame)
    }

    #[test]
    fn metadata_parsers_reject_malformed_tool_output() {
        assert_eq!(parse_rustc_version(b"rustc 1.99.1\n").unwrap(), "1.99.1");
        assert!(parse_rustc_version(b"compiler 1.99.1").is_err());
        assert!(parse_rustc_version(b"rustc").is_err());
        assert!(parse_rustc_version(&[0xff]).is_err());

        assert_eq!(
            parse_commit(format!("{}\n", "a".repeat(40)).as_bytes()).unwrap(),
            "a".repeat(40)
        );
        assert!(parse_commit(b"not-a-commit\n").is_err());
        assert!(parse_commit(&[0xff]).is_err());
    }

    #[test]
    fn failed_metadata_commands_return_explicit_errors() {
        assert!(ensure_success(true, "command failed").is_ok());
        assert_eq!(
            ensure_success(false, "command failed")
                .unwrap_err()
                .to_string(),
            "command failed"
        );
    }

    #[test]
    fn listener_address_resolution_rejects_missing_or_unadvertised_endpoints() {
        let advertised = Endpoint::from_address(4, "192.168.1.42", 1234).expect("endpoint");
        let other = Endpoint::from_address(4, "192.168.1.43", 1234).expect("endpoint");
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], 4321));

        assert_eq!(
            resolve_listener_address(&[], &advertised, address),
            Err("invalid_qr_payload")
        );
        assert_eq!(
            resolve_listener_address(&[other], &advertised, address),
            Err("invalid_qr_payload")
        );
        assert_eq!(
            resolve_listener_address(std::slice::from_ref(&advertised), &advertised, address),
            Ok(address)
        );
    }

    #[test]
    fn pairing_effect_validation_requires_pairing_material() {
        assert!(matches!(pairing_material(&[]), Err("internal_error")));
        assert!(matches!(
            pairing_material(&[Effect::End]),
            Err("internal_error")
        ));
    }

    #[test]
    fn failed_scenarios_produce_a_failed_aggregate_report() {
        let failure = map_scenario_error::<(), _>(
            Err("bad pairing"),
            ScenarioKind::PinMismatch,
            SessionState::Idle,
            vec!["pair"],
            |_| "internal_error",
        )
        .unwrap_err();
        assert_eq!(failure.reason_code, "internal_error");
        assert_eq!(failure.message_sequence, ["pair"]);

        let report_failure = failed_report(
            ScenarioKind::PinMismatch,
            SessionState::Idle,
            "internal_error",
            vec!["pair"],
        )
        .unwrap_err();
        assert_eq!(report_failure.reason_code, failure.reason_code);
        let scenario = report_failure.into_report();
        assert_eq!(scenario.verdict, Verdict::Fail);
        assert_eq!(scenario.scenario, "pin-mismatch-aborts");

        let report = RunReport::new(
            RunOptions::default(),
            "a".repeat(40),
            EnvironmentReport {
                os: "test",
                architecture: "test",
                rustc_version: "test".to_owned(),
            },
            vec![scenario],
        );
        assert_eq!(report.verdict, Verdict::Fail);
        assert_eq!(
            report.summary,
            Summary {
                scenarios: 1,
                passed: 0,
                failed: 1,
            }
        );
    }

    #[test]
    fn failed_pairing_results_are_finalized_with_resource_counts() {
        let report = finish_pairing_result(
            ScenarioConfig {
                scenario: ScenarioKind::PinMismatch,
                seed: 0,
                clock_start: Timestamp::ZERO,
            },
            ResourceTracker::default(),
            failed_report(
                ScenarioKind::PinMismatch,
                SessionState::Idle,
                "internal_error",
                Vec::new(),
            ),
        );

        assert_eq!(report.scenario, "pin-mismatch-aborts");
        assert_eq!(report.reason_code, Some("internal_error"));
        assert_eq!(report.verdict, Verdict::Fail);
        assert_eq!(report.resources.listeners.created, 0);
    }

    #[test]
    fn runtime_creation_failure_is_reported_as_a_scenario_failure() {
        let report = run_with_runtime(
            ScenarioConfig::default(),
            Err(std::io::Error::other("test runtime failure")),
        );

        assert_eq!(report.outcome, Outcome::Rejected);
        assert_eq!(report.reason_code, Some("internal_error"));
        assert_eq!(report.verdict, Verdict::Fail);
    }

    #[test]
    fn argument_parser_reports_invalid_clock_and_non_utf8_arguments() {
        let error =
            parse_run_options([OsString::from("--clock-start-millis"), OsString::from("x")])
                .unwrap_err();
        assert_eq!(
            error.to_string(),
            "invalid u64 value for --clock-start-millis"
        );

        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            assert_eq!(
                parse_run_options([OsString::from_vec(vec![0xff])])
                    .unwrap_err()
                    .to_string(),
                "arguments must be valid UTF-8"
            );
            assert_eq!(
                parse_run_options([OsString::from("--seed"), OsString::from_vec(vec![0xff]),])
                    .unwrap_err()
                    .to_string(),
                "arguments must be valid UTF-8"
            );
        }
    }

    #[test]
    fn signaling_frame_io_rejects_truncation_oversize_and_closed_peers() {
        let runtime = Builder::new_current_thread()
            .enable_io()
            .build()
            .expect("the test runtime can be created");
        runtime.block_on(async {
            let (writer, mut reader) = tokio::io::duplex(1);
            drop(writer);
            assert_eq!(
                read_signaling_frame(&mut reader).await,
                Err("invalid_message")
            );

            let (mut writer, mut reader) = tokio::io::duplex(MAX_SIGNALING_MESSAGE_BYTES + 2);
            let mut oversized = vec![b'x'; MAX_SIGNALING_MESSAGE_BYTES + 1];
            oversized.push(b'\n');
            writer.write_all(&oversized).await.unwrap();
            assert_eq!(
                read_signaling_frame(&mut reader).await,
                Err("security_violation")
            );

            let (mut writer, reader) = tokio::io::duplex(1);
            drop(reader);
            assert_eq!(
                write_signaling_frame(&mut writer, &[b'x'; MAX_SIGNALING_MESSAGE_BYTES + 1]).await,
                Err("invalid_message")
            );

            let (mut writer, reader) = tokio::io::duplex(1);
            drop(reader);
            assert_eq!(
                write_signaling_frame(&mut writer, b"frame").await,
                Err("internal_error")
            );

            let (mut writer, mut reader) = tokio::io::duplex(5);
            let reader_task = tokio::spawn(async move {
                let mut frame = [0_u8; 5];
                reader.read_exact(&mut frame).await.unwrap();
                assert_eq!(&frame, b"frame");
            });
            assert_eq!(
                write_signaling_frame(&mut writer, b"frame").await,
                Err("internal_error")
            );
            reader_task.await.unwrap();
        });
    }

    #[test]
    fn response_validators_reject_mismatched_and_unparseable_messages() {
        let paired = SignalingMessage::Paired {
            session_id: [1; 16],
            epoch: 2,
            dtls_fingerprint: DESKTOP_DTLS_FINGERPRINT,
            accepted_limits: Limits::new(),
        };
        let paired = paired.encode().expect("paired frame");
        assert_eq!(
            validate_paired_frame(&paired, [1; 16], 2, DESKTOP_DTLS_FINGERPRINT),
            Ok(())
        );
        assert_eq!(
            validate_paired_frame(&paired, [2; 16], 2, DESKTOP_DTLS_FINGERPRINT),
            Err("invalid_message")
        );
        assert_eq!(
            validate_paired_frame(b"not-json", [1; 16], 2, DESKTOP_DTLS_FINGERPRINT),
            Err("invalid_message")
        );

        let answer = SignalingMessage::Answer {
            epoch: 2,
            sdp: AnswerSdp::new(ANSWER_SDP).expect("answer SDP"),
        };
        let answer = answer.encode().expect("answer frame");
        assert_eq!(validate_answer_frame(&answer, 2), Ok(()));
        assert_eq!(validate_answer_frame(&answer, 3), Err("invalid_message"));
        assert_eq!(
            validate_answer_frame(b"not-json", 2),
            Err("invalid_message")
        );
    }

    #[test]
    fn desktop_rejects_a_malformed_pairing_frame() {
        test_runtime().block_on(async {
            let (server, mut client, _) = connected_test_desktop().await;
            write_signaling_frame(&mut client, b"not-json")
                .await
                .expect("frame write");

            match server.await.expect("server task") {
                Err(reason) => assert_eq!(reason, "protocol_violation"),
                Ok(_) => panic!("malformed pairing unexpectedly succeeded"),
            }
        });
    }

    #[test]
    fn desktop_refuses_a_peer_after_the_qr_window_before_the_handshake() {
        test_runtime().block_on(async {
            let listener = Arc::new(
                PairingListener::bind_loopback(
                    EphemeralCertificate::generate().expect("certificate"),
                )
                .await
                .expect("listener"),
            );
            let clock = ManualClock::new(Timestamp::ZERO);
            let mut core = SessionCore::new(
                clock.clone(),
                SeededRandomSource::new(42),
                DESKTOP_DTLS_FINGERPRINT,
            );
            core.handle(SessionEvent::StartPairing)
                .expect("pairing starts");
            clock.advance(PAIRING_WINDOW_MILLIS);
            let server = tokio::spawn(desktop_session(core, Arc::clone(&listener), Vec::new()));

            let mut peer = tokio::net::TcpStream::connect(listener.local_addr())
                .await
                .expect("tcp connect");
            let mut received = Vec::new();
            peer.read_to_end(&mut received)
                .await
                .expect("the refused peer sees a close");

            assert!(received.is_empty());
            match server.await.expect("server task") {
                Err(reason) => assert_eq!(reason, "qr_expired"),
                Ok(_) => panic!("an expired qr unexpectedly reached the handshake"),
            }
        });
    }

    #[test]
    fn desktop_rejects_an_offer_with_a_different_epoch() {
        test_runtime().block_on(async {
            let (server, mut client, token) = connected_test_desktop().await;
            let pairing = test_pair_frame(&token);
            write_signaling_frame(&mut client, &pairing)
                .await
                .expect("pairing write");
            let mut client = BufReader::new(client);
            let _paired = read_signaling_frame(&mut client)
                .await
                .expect("paired response");
            let offer = SignalingMessage::Offer {
                epoch: 999,
                sdp: OfferSdp::new(OFFER_SDP).expect("offer SDP"),
            };
            write_signaling_frame(&mut client, &offer.encode().expect("offer frame"))
                .await
                .expect("offer write");

            match server.await.expect("server task") {
                Err(reason) => assert_eq!(reason, "invalid_message"),
                Ok(_) => panic!("wrong-epoch offer unexpectedly succeeded"),
            }
        });
    }

    #[test]
    fn desktop_rejects_an_unparseable_offer() {
        test_runtime().block_on(async {
            let (server, mut client, token) = connected_test_desktop().await;
            let pairing = test_pair_frame(&token);
            write_signaling_frame(&mut client, &pairing)
                .await
                .expect("pairing write");
            let mut client = BufReader::new(client);
            let _paired = read_signaling_frame(&mut client)
                .await
                .expect("paired response");
            write_signaling_frame(&mut client, b"not-json")
                .await
                .expect("offer write");

            match server.await.expect("server task") {
                Err(reason) => assert_eq!(reason, "invalid_message"),
                Ok(_) => panic!("unparseable offer unexpectedly succeeded"),
            }
        });
    }

    fn run_virtual(config: ScenarioConfig) -> HarnessReport {
        let runtime = Builder::new_current_thread()
            .enable_io()
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

        assert_eq!(report.outcome, Outcome::Accepted);
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
                && transition.at_millis == 100 + FIRST_MEDIA_FRAME_TIMEOUT_MILLIS + 1
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
