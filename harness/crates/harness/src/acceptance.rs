use super::*;
use remotecam_pairing::PairingMaterial as IssuedQr;
use remotecam_pairing::PairingMaterialRequest;
use remotecam_session_core::PAIRING_WINDOW_MILLIS;
use remotecam_session_core::PairingMaterial as CoreMaterial;
use std::net::SocketAddr;
use tokio::net::TcpStream;

const ADVERTISED_ADDRESS: &str = "192.168.1.42";
const FULL_SEQUENCE: [&str; 4] = ["pair", "paired", "offer", "answer"];

pub(super) async fn run(config: ScenarioConfig, resources: ResourceTracker) -> HarnessReport {
    let mut world = match World::new(config, &resources).await {
        Ok(world) => world,
        Err(reason) => return failed_scenario(config.scenario, SessionState::Idle, reason),
    };
    let mut report = match scenario_flow(config.scenario, &mut world).await {
        Ok(report) => report,
        Err(reason) => failed_scenario(config.scenario, world.core.state(), reason),
    };
    world.close();
    report.resources = resources.snapshot();
    report
}

async fn scenario_flow(
    scenario: ScenarioKind,
    world: &mut World,
) -> Result<HarnessReport, &'static str> {
    match scenario {
        ScenarioKind::QrExpiryRefused => qr_expiry_refused(world).await,
        ScenarioKind::TokenExpiryRefused => token_expiry_refused(world).await,
        ScenarioKind::SingleUseRefused => single_use_refused(world).await,
        ScenarioKind::ReplayRefused => replay_refused(world).await,
        ScenarioKind::StaleEpochDiscarded => stale_epoch_discarded(world).await,
        ScenarioKind::ClockBackwardsWindow => clock_backwards_window(world).await,
        ScenarioKind::RegenerationAfterRefusal => regeneration_after_refusal(world).await,
        ScenarioKind::Pairing | ScenarioKind::PinMismatch | ScenarioKind::FirstFrameTimeout => {
            Err("internal_error")
        }
    }
}

async fn qr_expiry_refused(world: &mut World) -> Result<HarnessReport, &'static str> {
    world.start_pairing()?;
    world.clock.advance(PAIRING_WINDOW_MILLIS);

    let address = world.listener.local_addr();
    let peer = tokio::spawn(async move {
        let mut socket = TcpStream::connect(address).await.ok()?;
        let mut received = Vec::new();
        socket.read_to_end(&mut received).await.ok()?;
        Some(received)
    });
    let desktop = run_desktop(
        &mut world.core,
        &world.listener,
        &mut world.trace,
        &mut world.regenerated,
    )
    .await;
    let received = peer.await.ok().flatten();
    world.absorb_regenerated();

    let silent = received.is_some_and(|bytes| bytes.is_empty());
    Ok(world.conclude(desktop, Vec::new(), silent && world.issued_epochs == [1, 2]))
}

async fn token_expiry_refused(world: &mut World) -> Result<HarnessReport, &'static str> {
    world.start_pairing()?;
    let qr = world.scan_pending()?;
    let attempt = world.attempt(&qr, Some(PAIRING_WINDOW_MILLIS)).await;

    let invariant = attempt.messages == ["pair"] && world.issued_epochs == [1, 2];
    Ok(world.conclude(attempt.desktop, attempt.messages, invariant))
}

async fn single_use_refused(world: &mut World) -> Result<HarnessReport, &'static str> {
    world.start_pairing()?;
    let qr = world.scan_pending()?;
    let first = world.attempt(&qr, None).await;
    let second = world.attempt(&qr, None).await;

    let invariant = first.desktop.is_ok()
        && first.messages == FULL_SEQUENCE
        && second.messages == ["pair"]
        && world.issued_epochs == [1];
    Ok(world.conclude(second.desktop, second.messages, invariant))
}

async fn replay_refused(world: &mut World) -> Result<HarnessReport, &'static str> {
    world.start_pairing()?;
    let captured = world.scan_pending()?;
    let first = world.attempt(&captured, None).await;
    world.expire_first_frame()?;
    world.start_pairing()?;
    let replay = world.attempt(&captured, Some(PAIRING_WINDOW_MILLIS)).await;

    let invariant = first.desktop.is_ok()
        && first.messages == FULL_SEQUENCE
        && replay.messages == ["pair"]
        && world.issued_epochs == [1, 2];
    Ok(world.conclude(replay.desktop, replay.messages, invariant))
}

async fn stale_epoch_discarded(world: &mut World) -> Result<HarnessReport, &'static str> {
    world.start_pairing()?;
    let first_qr = world.scan_pending()?;
    let first = world.attempt(&first_qr, None).await;
    world.expire_first_frame()?;
    world.start_pairing()?;
    let active_qr = world.scan_pending()?;
    let active = world.attempt(&active_qr, None).await;

    let stale_admitted = world.core.admit_frame_epoch(1);
    let violations = world.core.protocol_violations();
    let active_admitted = world.core.admit_frame_epoch(2);

    let invariant = first.desktop.is_ok()
        && active.desktop.is_ok()
        && active.messages == FULL_SEQUENCE
        && !stale_admitted
        && active_admitted
        && violations == 1
        && world.core.state() == SessionState::Negotiating
        && world.issued_epochs == [1, 2];
    let mut report = world.conclude(active.desktop, active.messages, invariant);
    report.protocol_violations = Some(violations);
    Ok(report)
}

async fn clock_backwards_window(world: &mut World) -> Result<HarnessReport, &'static str> {
    world.clock.advance(10_000);
    let issued_at = world.clock.now().as_millis();
    world.start_pairing()?;
    world
        .clock
        .set(Timestamp::from_millis(issued_at.saturating_sub(5_000)));
    world.clock.set(Timestamp::from_millis(
        issued_at + PAIRING_WINDOW_MILLIS - 1,
    ));
    let qr = world.scan_pending()?;
    let attempt = world.attempt(&qr, Some(1)).await;

    let invariant = attempt.messages == ["pair"] && world.issued_epochs == [1, 2];
    Ok(world.conclude(attempt.desktop, attempt.messages, invariant))
}

async fn regeneration_after_refusal(world: &mut World) -> Result<HarnessReport, &'static str> {
    world.start_pairing()?;
    let expired = world.scan_pending()?;
    world.clock.advance(PAIRING_WINDOW_MILLIS);
    let refused = world.attempt(&expired, None).await;
    let fresh = world.scan_pending()?;
    let accepted = world.attempt(&fresh, None).await;

    let invariant = refused.desktop == Err("qr_expired")
        && refused.messages.is_empty()
        && accepted.desktop.is_ok()
        && accepted.messages == FULL_SEQUENCE
        && world.issued_epochs == [1, 2];
    Ok(world.conclude(accepted.desktop, accepted.messages, invariant))
}

#[derive(Clone)]
struct MobileQr {
    token: Zeroizing<[u8; 32]>,
    session_id: [u8; 16],
    epoch: u64,
    fingerprint: [u8; 32],
    address: SocketAddr,
}

struct Attempt {
    desktop: Result<(), &'static str>,
    messages: Vec<&'static str>,
}

struct World {
    scenario: ScenarioKind,
    clock: ManualClock,
    core: SessionCore<ManualClock, SeededRandomSource>,
    listener: Arc<PairingListener>,
    trace: Vec<StateTransition>,
    issued_epochs: Vec<u64>,
    regenerated: Vec<CoreMaterial>,
    pending: Option<CoreMaterial>,
    listener_guard: Option<ResourceGuard>,
}

impl World {
    async fn new(
        config: ScenarioConfig,
        resources: &ResourceTracker,
    ) -> Result<Self, &'static str> {
        let clock = ManualClock::new(config.clock_start);
        let core = SessionCore::new(
            clock.clone(),
            SeededRandomSource::new(config.seed),
            DESKTOP_DTLS_FINGERPRINT,
        );
        let mut certificate_random =
            HarnessCertificateRandomSource(SeededRandomSource::new(config.seed ^ CERTIFICATE_SEED));
        let certificate =
            EphemeralCertificate::generate_with_random_source(&mut certificate_random)
                .map_err(|_| "internal_error")?;
        let listener = PairingListener::bind_loopback(certificate)
            .await
            .map_err(|_| "internal_error")?;
        Ok(Self {
            scenario: config.scenario,
            clock,
            core,
            listener: Arc::new(listener),
            trace: Vec::new(),
            issued_epochs: Vec::new(),
            regenerated: Vec::new(),
            pending: None,
            listener_guard: Some(resources.listener()),
        })
    }

    fn close(&mut self) {
        self.listener_guard.take();
    }

    fn start_pairing(&mut self) -> Result<(), &'static str> {
        let previous = self.core.state();
        let effects = self
            .core
            .handle(SessionEvent::StartPairing)
            .map_err(|error| error.code().as_str())?;
        record_transition(
            &mut self.trace,
            previous,
            self.core.state(),
            self.core.last_transition_at(),
            None,
        );
        let material = match <[Effect; 1]>::try_from(effects) {
            Ok([Effect::PresentPairing(material)]) => material,
            _ => return Err("internal_error"),
        };
        self.issued_epochs.push(u64::from(material.epoch()));
        self.pending = Some(material);
        Ok(())
    }

    fn expire_first_frame(&mut self) -> Result<(), &'static str> {
        self.clock.advance(FIRST_MEDIA_FRAME_TIMEOUT_MILLIS + 1);
        let previous = self.core.state();
        let effects = self
            .core
            .handle(SessionEvent::FirstFrameTimedOut)
            .map_err(|error| error.code().as_str())?;
        if effects != [Effect::End] {
            return Err("internal_error");
        }
        record_transition(
            &mut self.trace,
            previous,
            self.core.state(),
            self.core.last_transition_at(),
            Some("timeout"),
        );
        Ok(())
    }

    fn scan_pending(&mut self) -> Result<MobileQr, &'static str> {
        let material = self.pending.take().ok_or("internal_error")?;
        scan_qr(&self.listener, &material)
    }

    fn absorb_regenerated(&mut self) {
        for material in std::mem::take(&mut self.regenerated) {
            self.issued_epochs.push(u64::from(material.epoch()));
            self.pending = Some(material);
        }
    }

    async fn attempt(&mut self, qr: &MobileQr, pause_before_pair: Option<u64>) -> Attempt {
        let peer = tokio::spawn(mobile_exchange(
            qr.clone(),
            self.clock.clone(),
            pause_before_pair,
        ));
        let desktop = run_desktop(
            &mut self.core,
            &self.listener,
            &mut self.trace,
            &mut self.regenerated,
        )
        .await;
        let messages = peer.await.unwrap_or_default();
        self.absorb_regenerated();
        Attempt { desktop, messages }
    }

    fn conclude(
        &self,
        desktop: Result<(), &'static str>,
        messages: Vec<&'static str>,
        expectation_met: bool,
    ) -> HarnessReport {
        let mut report =
            HarnessReport::for_pairing_result(self.core.state(), desktop.err(), messages);
        report.state_trace = self.trace.clone();
        report.issued_epochs = self.issued_epochs.clone();
        report.apply_scenario(self.scenario);
        if !expectation_met {
            report.verdict = Verdict::Fail;
        }
        report
    }
}

fn failed_scenario(
    scenario: ScenarioKind,
    state: SessionState,
    reason: &'static str,
) -> HarnessReport {
    let mut report = HarnessReport::for_pairing_result(state, Some(reason), Vec::new());
    report.apply_scenario(scenario);
    report
}

fn scan_qr(listener: &PairingListener, material: &CoreMaterial) -> Result<MobileQr, &'static str> {
    let advertised = Endpoint::from_address(4, ADVERTISED_ADDRESS, listener.local_addr().port())
        .ok_or("invalid_qr_payload")?;
    let issued: IssuedQr = listener
        .issue_material(
            PairingMaterialRequest::new(material, vec![advertised.clone()]),
            &HarnessUnixClock,
        )
        .map_err(|_| "invalid_qr_payload")?;
    let payload = QrPayload::decode(issued.encoded_qr(), QR_CLOCK_START_UNIX)
        .map_err(|error| error.code.as_str())?;
    let address = resolve_listener_address(&payload.endpoints, &advertised, listener.local_addr())?;
    Ok(MobileQr {
        token: Zeroizing::new(payload.pairing_token),
        session_id: payload.session_id,
        epoch: u64::from(payload.epoch),
        fingerprint: payload.tls_fingerprint,
        address,
    })
}

async fn mobile_exchange(
    qr: MobileQr,
    clock: ManualClock,
    pause_before_pair: Option<u64>,
) -> Vec<&'static str> {
    let mut messages = Vec::new();
    let Ok(client) = PinnedClient::new(qr.fingerprint) else {
        return messages;
    };
    let Ok(channel) = client.connect(qr.address).await else {
        return messages;
    };
    let mut mobile = BufReader::new(channel);
    if let Some(millis) = pause_before_pair {
        clock.advance(millis);
    }

    let Ok(pair_frame) = encode_pair_frame(&qr.token) else {
        return messages;
    };
    if write_signaling_frame(&mut mobile, &pair_frame)
        .await
        .is_err()
    {
        return messages;
    }
    messages.push("pair");

    let Ok(paired) = read_signaling_frame(&mut mobile).await else {
        return messages;
    };
    if validate_paired_frame(&paired, qr.session_id, qr.epoch, DESKTOP_DTLS_FINGERPRINT).is_err() {
        return messages;
    }
    messages.push("paired");

    let Ok(offer_sdp) = Sdp::new(OFFER_SDP) else {
        return messages;
    };
    let Ok(offer_frame) = (SignalingMessage::Offer {
        epoch: qr.epoch,
        sdp: offer_sdp,
    })
    .encode() else {
        return messages;
    };
    if write_signaling_frame(&mut mobile, &offer_frame)
        .await
        .is_err()
    {
        return messages;
    }
    messages.push("offer");

    let Ok(answer) = read_signaling_frame(&mut mobile).await else {
        return messages;
    };
    if validate_answer_frame(&answer, qr.epoch).is_ok() {
        messages.push("answer");
    }
    messages
}

fn encode_pair_frame(token: &[u8; 32]) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    let mut message = SignalingMessage::Pair {
        token: *token,
        dtls_fingerprint: MOBILE_DTLS_FINGERPRINT,
        limits: Limits::new(),
    };
    let frame = message.encode().map_err(|_| "internal_error");
    if let SignalingMessage::Pair { token, .. } = &mut message {
        token.zeroize();
    }
    frame.map(Zeroizing::new)
}
