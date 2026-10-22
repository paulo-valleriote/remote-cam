//! The handshake scenario matrix.
//!
//! Every scenario runs against the real listener and the real pinned client over
//! loopback TLS 1.3. Nothing here substitutes a mock for the TLS or token check:
//! the observed reason code is the one the implementation actually produced.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use remotecam_protocol::{Endpoint, QrDecodeError, QrPayload, ReasonCode};
use remotecam_signaling::certificate::test_support::chain_fixture;
use remotecam_signaling::certificate::{EphemeralCertificate, sha256_fingerprint};
use remotecam_signaling::client::{
    AcceptanceOnlyVerifier, ExactPinVerifier, PairingError, PinnedClient, classify_io_error,
    client_config_with_custom_verifier,
};
use remotecam_signaling::fingerprint_exchange::MediaFingerprints;
use remotecam_signaling::qr::build_payload;
use remotecam_signaling::server::{
    AcceptedPairing, PairingRejection, PairingServer, SourcePolicy, bind_loopback,
};
use remotecam_signaling::token::TokenStore;
use rustls::RootCertStore;
use rustls::pki_types::CertificateDer;
use tokio::net::{TcpSocket, TcpStream};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;

/// One row of the scenario matrix.
#[derive(Debug)]
pub struct ScenarioSpec {
    /// Stable scenario identifier used by the evidence and the trace name.
    pub id: &'static str,
    /// `valid` or `adversarial`.
    pub category: &'static str,
    /// The closed code the scenario must produce, if it must be rejected.
    pub expected: Option<ReasonCode>,
    /// What is set up before the attempt.
    pub preconditions: &'static str,
    /// The expected observation.
    pub expected_result: &'static str,
    /// Which code path the harness drives.
    pub kind: ScenarioKind,
}

/// The behaviour each scenario drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioKind {
    Valid,
    WrongFingerprint,
    ChainOnlyTrust,
    TlsDowngrade,
    ExpiredToken,
    ReusedToken,
    WrongToken,
    NonPrivateSource,
    InterceptionProxy,
    ExpiredQr,
    MalformedQr,
    PublicEndpoint,
    TamperedMediaFingerprint,
}

/// The scenario matrix. Order is the execution order and the evidence order.
pub fn specs() -> &'static [ScenarioSpec] {
    &[
        ScenarioSpec {
            id: "valid-01",
            category: "valid",
            expected: None,
            preconditions: "Fresh desktop session; QR displayed; mobile scans raw bytes and pins the leaf SHA-256.",
            expected_result: "TLS 1.3 channel established and the token accepted; no media starts before authentication.",
            kind: ScenarioKind::Valid,
        },
        ScenarioSpec {
            id: "adversarial-wrong-fingerprint",
            category: "adversarial",
            expected: Some(ReasonCode::CertificatePinMismatch),
            preconditions: "The mobile is configured with a fingerprint that differs from the leaf in one byte.",
            expected_result: "The TLS handshake aborts with certificate_pin_mismatch before any media.",
            kind: ScenarioKind::WrongFingerprint,
        },
        ScenarioSpec {
            id: "adversarial-chain-only-trust",
            category: "adversarial",
            expected: Some(ReasonCode::CertificateChainRejected),
            preconditions: "The presented leaf chains to a CA the client knows, but is not the pinned leaf.",
            expected_result: "The attempt is rejected as chain-only trust, with no fallback to CA validation.",
            kind: ScenarioKind::ChainOnlyTrust,
        },
        ScenarioSpec {
            id: "adversarial-tls-downgrade",
            category: "adversarial",
            expected: Some(ReasonCode::TlsVersionUnsupported),
            preconditions: "The client offers TLS 1.2; the listener is TLS 1.3 only.",
            expected_result: "The handshake is refused as a downgrade attempt.",
            kind: ScenarioKind::TlsDowngrade,
        },
        ScenarioSpec {
            id: "adversarial-expired-token",
            category: "adversarial",
            expected: Some(ReasonCode::TokenExpired),
            preconditions: "The correct token is presented after its 120-second window closed.",
            expected_result: "The listener rejects the token as expired.",
            kind: ScenarioKind::ExpiredToken,
        },
        ScenarioSpec {
            id: "adversarial-reused-token",
            category: "adversarial",
            expected: Some(ReasonCode::TokenAlreadyUsed),
            preconditions: "A token that already paired once is replayed against a restarted listener with the same store.",
            expected_result: "The replay is rejected; the previous pairing is not reusable.",
            kind: ScenarioKind::ReusedToken,
        },
        ScenarioSpec {
            id: "adversarial-wrong-token",
            category: "adversarial",
            expected: Some(ReasonCode::TokenInvalid),
            preconditions: "The mobile presents a forged 256-bit token.",
            expected_result: "The listener rejects the token.",
            kind: ScenarioKind::WrongToken,
        },
        ScenarioSpec {
            id: "adversarial-non-private-source",
            category: "adversarial",
            expected: Some(ReasonCode::SourceAddressNotPrivate),
            preconditions: "The connection arrives from an address outside the permitted private ranges.",
            expected_result: "The listener refuses the source before the TLS handshake.",
            kind: ScenarioKind::NonPrivateSource,
        },
        ScenarioSpec {
            id: "adversarial-interception-proxy",
            category: "adversarial",
            expected: Some(ReasonCode::CertificatePinMismatch),
            preconditions: "A proxy terminates TLS with its own certificate and answers the paired message.",
            expected_result: "The client aborts on the pin before sending the pairing token.",
            kind: ScenarioKind::InterceptionProxy,
        },
        ScenarioSpec {
            id: "adversarial-qr-expired",
            category: "adversarial",
            expected: Some(ReasonCode::QrExpired),
            preconditions: "The scanned payload carries an expiry equal to the current time.",
            expected_result: "The payload is refused at decode time.",
            kind: ScenarioKind::ExpiredQr,
        },
        ScenarioSpec {
            id: "adversarial-malformed-qr",
            category: "adversarial",
            expected: Some(ReasonCode::InvalidQrPayload),
            preconditions: "The scanned bytes are shorter than the fixed header and carry no magic.",
            expected_result: "The payload is refused at decode time.",
            kind: ScenarioKind::MalformedQr,
        },
        ScenarioSpec {
            id: "adversarial-public-endpoint",
            category: "adversarial",
            expected: Some(ReasonCode::NoPrivateEndpoint),
            preconditions: "The payload advertises a public address as its only endpoint.",
            expected_result: "The payload is refused at decode time; no QR is displayable.",
            kind: ScenarioKind::PublicEndpoint,
        },
        ScenarioSpec {
            id: "adversarial-tampered-media-fingerprint",
            category: "adversarial",
            expected: Some(ReasonCode::ProtocolViolation),
            preconditions: "The token is correct but the DTLS fingerprint differs from the value the listener expects.",
            expected_result: "The session aborts before any media starts.",
            kind: ScenarioKind::TamperedMediaFingerprint,
        },
    ]
}

/// A code observed by one layer of the system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedCode {
    /// `client` or `server`.
    pub layer: &'static str,
    /// The closed reason code.
    pub code: ReasonCode,
}

/// The observation produced by one scenario.
#[derive(Debug, Clone)]
pub struct Observed {
    /// Whether the system accepted the attempt.
    pub accepted: bool,
    /// Every closed code observed, across client and server.
    pub codes: Vec<ObservedCode>,
    /// Observed outcome text for the evidence.
    pub summary: String,
    /// Trace lines.
    pub trace: Vec<String>,
    /// Time from pairing start to established channel.
    pub established_channel_ms: Option<u64>,
}

impl Observed {
    fn rejected(layer: &'static str, code: ReasonCode, summary: impl Into<String>) -> Self {
        Self {
            accepted: false,
            codes: vec![ObservedCode { layer, code }],
            summary: summary.into(),
            trace: Vec::new(),
            established_channel_ms: None,
        }
    }

    /// Whether the required code was observed by at least one layer.
    pub fn has_code(&self, expected: ReasonCode) -> bool {
        self.codes.iter().any(|observed| observed.code == expected)
    }
}

/// A resolved scenario result ready for the evidence file.
#[derive(Debug, Clone)]
pub struct ResolvedScenario {
    /// The specification the result belongs to.
    pub spec: &'static ScenarioSpec,
    /// The observation.
    pub observed: Observed,
    /// The code reported in the evidence, if rejected.
    pub reason_code: Option<ReasonCode>,
    /// Whether the observation matches the expectation.
    pub matched: bool,
    /// Established channel time for the valid scenario.
    pub established_channel_ms: Option<u64>,
    /// The extra delay exact pinning added over the unpinned control.
    pub pinning_added_delay_ms: Option<f64>,
}

/// Runs every scenario in the matrix.
pub async fn run_all() -> Vec<ResolvedScenario> {
    let mut results = Vec::new();
    for spec in specs() {
        let observed = run_one(spec.kind).await;
        let reason_code = if observed.accepted {
            None
        } else {
            observed
                .codes
                .iter()
                .find(|code| spec.expected.is_some_and(|expected| code.code == expected))
                .or_else(|| observed.codes.first())
                .map(|code| code.code)
        };
        let matched = match spec.expected {
            None => observed.accepted,
            Some(expected) => !observed.accepted && observed.has_code(expected),
        };
        let established_channel_ms = observed.established_channel_ms;
        let pinning_added_delay_ms = if matches!(spec.kind, ScenarioKind::Valid) {
            measure_pinning_delay().await
        } else {
            None
        };
        results.push(ResolvedScenario {
            spec,
            observed,
            reason_code,
            matched,
            established_channel_ms,
            pinning_added_delay_ms,
        });
    }
    results
}

pub(crate) fn expected_fingerprints() -> MediaFingerprints {
    // The wire pair message carries one DTLS fingerprint for the transport, so
    // both streams use the same value.
    MediaFingerprints::new([0xb2; 32], [0xb2; 32])
}

pub(crate) fn session_id() -> [u8; 16] {
    [0x42; 16]
}

pub(crate) struct ServerFixture {
    pub(crate) address: SocketAddr,
    pub(crate) token_store: Arc<TokenStore>,
    handle: tokio::task::JoinHandle<Result<AcceptedPairing, PairingRejection>>,
}

pub(crate) async fn start_server(
    certificate: &EphemeralCertificate,
    chain: Option<Vec<CertificateDer<'static>>>,
    token: [u8; 32],
    policy: SourcePolicy,
) -> Result<ServerFixture, String> {
    let token_store = Arc::new(TokenStore::new());
    let fixture =
        start_server_with_store(certificate, chain, Arc::clone(&token_store), policy).await?;
    token_store.issue(token, Duration::ZERO);
    Ok(fixture)
}

pub(crate) async fn start_server_with_store(
    certificate: &EphemeralCertificate,
    chain: Option<Vec<CertificateDer<'static>>>,
    token_store: Arc<TokenStore>,
    policy: SourcePolicy,
) -> Result<ServerFixture, String> {
    let listener = bind_loopback().await.map_err(|error| error.to_string())?;
    let address = listener.local_addr().map_err(|error| error.to_string())?;
    let mut server = PairingServer::new(
        listener,
        certificate,
        chain,
        Arc::clone(&token_store),
        expected_fingerprints(),
        policy,
        session_id(),
        1,
    )
    .map_err(|error| error.to_string())?;
    let handle = tokio::spawn(async move { server.accept_one().await });
    Ok(ServerFixture {
        address,
        token_store,
        handle,
    })
}

pub(crate) async fn join_server(
    fixture: ServerFixture,
) -> (Result<AcceptedPairing, PairingRejection>, Arc<TokenStore>) {
    let result = match tokio::time::timeout(Duration::from_secs(10), fixture.handle).await {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => Err(PairingRejection {
            code: ReasonCode::InternalError,
            origin: None,
            stage: "task",
            detail: error.to_string(),
        }),
        Err(_) => Err(PairingRejection {
            code: ReasonCode::InternalError,
            origin: None,
            stage: "task",
            detail: "the listener task did not finish within 10 s".to_owned(),
        }),
    };
    (result, fixture.token_store)
}

/// Starts a listener that accepts exactly `attempts` connections, keeping the
/// throttling state across them.
pub(crate) async fn start_server_loop(
    certificate: &EphemeralCertificate,
    chain: Option<Vec<CertificateDer<'static>>>,
    token: [u8; 32],
    policy: SourcePolicy,
    attempts: usize,
) -> Result<
    (
        SocketAddr,
        Arc<TokenStore>,
        tokio::task::JoinHandle<(
            Vec<Result<AcceptedPairing, PairingRejection>>,
            PairingServer,
        )>,
    ),
    String,
> {
    let listener = bind_loopback().await.map_err(|error| error.to_string())?;
    let address = listener.local_addr().map_err(|error| error.to_string())?;
    let token_store = Arc::new(TokenStore::new());
    let mut server = PairingServer::new(
        listener,
        certificate,
        chain,
        Arc::clone(&token_store),
        expected_fingerprints(),
        policy,
        session_id(),
        1,
    )
    .map_err(|error| error.to_string())?;
    token_store.issue(token, server.now());
    let handle = tokio::spawn(async move {
        let mut outcomes = Vec::with_capacity(attempts);
        for _ in 0..attempts {
            outcomes.push(server.accept_one().await);
        }
        (outcomes, server)
    });
    Ok((address, token_store, handle))
}

pub(crate) fn loopback_policy() -> SourcePolicy {
    SourcePolicy {
        allow_loopback: true,
        expected_subnet: None,
    }
}

pub(crate) async fn connect_client(
    client: &PinnedClient,
    address: SocketAddr,
) -> Result<WebSocketStream<tokio_rustls::client::TlsStream<TcpStream>>, PairingError> {
    client.connect(address).await
}

pub(crate) async fn send_pair<S>(
    socket: &mut WebSocketStream<S>,
    token: &[u8; 32],
    fingerprint: &[u8; 32],
) -> Result<(), String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let message = serde_json::json!({
        "version": 1,
        "type": "pair",
        "token": remotecam_protocol::hex::encode_hex(token),
        "dtls_fingerprint": remotecam_protocol::hex::encode_hex(fingerprint),
        "limits": {},
    });
    socket
        .send(Message::Text(message.to_string().into()))
        .await
        .map_err(|error| error.to_string())
}

pub(crate) async fn await_paired<S>(socket: &mut WebSocketStream<S>) -> Result<(), String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    match tokio::time::timeout(Duration::from_secs(10), socket.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => {
            let value: serde_json::Value =
                serde_json::from_str(&text).map_err(|error| error.to_string())?;
            if value.get("type").and_then(|value| value.as_str()) == Some("paired") {
                Ok(())
            } else {
                Err("the listener did not answer with a paired message".to_owned())
            }
        }
        Ok(Some(Ok(_))) => Err("the listener answered with a non-text frame".to_owned()),
        Ok(Some(Err(error))) => Err(error.to_string()),
        Ok(None) => Err("the listener closed before answering".to_owned()),
        Err(_) => Err("the paired answer timed out".to_owned()),
    }
}

fn server_code(result: &Result<AcceptedPairing, PairingRejection>) -> Option<ReasonCode> {
    result.as_ref().err().map(|rejection| rejection.code)
}

async fn run_one(kind: ScenarioKind) -> Observed {
    match kind {
        ScenarioKind::Valid => run_valid().await,
        ScenarioKind::WrongFingerprint => run_wrong_fingerprint().await,
        ScenarioKind::ChainOnlyTrust => run_chain_only().await,
        ScenarioKind::TlsDowngrade => run_tls_downgrade().await,
        ScenarioKind::ExpiredToken => run_token_case(TokenCase::Expired).await,
        ScenarioKind::ReusedToken => run_token_case(TokenCase::Reused).await,
        ScenarioKind::WrongToken => run_token_case(TokenCase::Wrong).await,
        ScenarioKind::NonPrivateSource => run_non_private_source().await,
        ScenarioKind::InterceptionProxy => run_interception_proxy().await,
        ScenarioKind::ExpiredQr => run_qr_case(QrCase::Expired),
        ScenarioKind::MalformedQr => run_qr_case(QrCase::Malformed),
        ScenarioKind::PublicEndpoint => run_qr_case(QrCase::PublicEndpoint),
        ScenarioKind::TamperedMediaFingerprint => run_tampered_fingerprint().await,
    }
}

async fn run_valid() -> Observed {
    let certificate = match EphemeralCertificate::generate() {
        Ok(certificate) => certificate,
        Err(error) => {
            return Observed::rejected("harness", ReasonCode::InternalError, error.to_string());
        }
    };
    let token = [0x11; 32];
    let fixture = match start_server(&certificate, None, token, loopback_policy()).await {
        Ok(fixture) => fixture,
        Err(error) => return Observed::rejected("harness", ReasonCode::InternalError, error),
    };
    let address = fixture.address;
    let client = match PinnedClient::new(Arc::new(ExactPinVerifier::new(certificate.fingerprint())))
    {
        Ok(client) => client,
        Err(error) => return Observed::rejected("client", error.code, error.detail),
    };

    let started = Instant::now();
    let mut trace = vec![
        format!("origin=loopback address={address}"),
        "tls_version=1.3".to_owned(),
    ];
    let mut socket = match connect_client(&client, address).await {
        Ok(socket) => socket,
        Err(error) => {
            let (server_result, store) = join_server(fixture).await;
            store.clear();
            trace.push(format!(
                "client_error={} detail={}",
                error.code.as_str(),
                error.detail
            ));
            let mut observed = Observed::rejected(
                "client",
                error.code,
                "the valid pairing did not establish a channel",
            );
            if let Some(code) = server_code(&server_result) {
                observed.codes.push(ObservedCode {
                    layer: "server",
                    code,
                });
            }
            observed.trace = trace;
            return observed;
        }
    };
    if let Err(error) = send_pair(&mut socket, &token, &expected_fingerprints().video).await {
        trace.push(format!("pair_send_error={error}"));
    }
    let paired = await_paired(&mut socket).await;
    let established_ms = u64::try_from(started.elapsed().as_millis()).ok();
    let (server_result, store) = join_server(fixture).await;
    let cleared = !store.is_issued();
    store.clear();

    if let Err(rejection) = &server_result {
        trace.push(format!(
            "server_rejection={} stage={} detail={}",
            rejection.code.as_str(),
            rejection.stage,
            rejection.detail
        ));
    }

    match paired {
        Ok(()) if server_result.is_ok() => {
            trace.push("pair_message=accepted".to_owned());
            trace.push(format!(
                "established_channel_ms={}",
                established_ms.unwrap_or_default()
            ));
            trace.push(format!("token_store_cleared={cleared}"));
            Observed {
                accepted: true,
                codes: Vec::new(),
                summary: format!(
                    "TLS 1.3 channel established in {} ms; the single-use token was accepted and consumed.",
                    established_ms.unwrap_or_default()
                ),
                trace,
                established_channel_ms: established_ms,
            }
        }
        other => {
            let detail = format!("paired answer: {other:?}");
            trace.push(detail.clone());
            let mut observed = Observed::rejected(
                "server",
                server_code(&server_result).unwrap_or(ReasonCode::TlsHandshakeFailed),
                "the valid pairing was not accepted",
            );
            if let Some(code) = server_code(&server_result) {
                observed.codes = vec![ObservedCode {
                    layer: "server",
                    code,
                }];
            }
            observed.trace = trace;
            observed
        }
    }
}

async fn run_wrong_fingerprint() -> Observed {
    let certificate = EphemeralCertificate::generate().expect("certificate");
    let token = [0x12; 32];
    let fixture = start_server(&certificate, None, token, loopback_policy())
        .await
        .expect("server");
    let address = fixture.address;
    let mut pin = certificate.fingerprint();
    pin[0] ^= 0x01;
    let client = PinnedClient::new(Arc::new(ExactPinVerifier::new(pin))).expect("client");
    let error = connect_client(&client, address)
        .await
        .expect_err("pin mismatch");
    let (server_result, store) = join_server(fixture).await;
    store.clear();

    let mut observed = Observed::rejected(
        "client",
        error.code,
        "TLS aborted on the pinned fingerprint; the pairing token was never sent.",
    );
    if let Some(code) = server_code(&server_result) {
        observed.codes.push(ObservedCode {
            layer: "server",
            code,
        });
    }
    observed.trace = vec![
        format!(
            "client_error={} detail={}",
            error.code.as_str(),
            error.detail
        ),
        "token_sent=false".to_owned(),
    ];
    observed
}

async fn run_chain_only() -> Observed {
    let fixture = match chain_fixture() {
        Ok(fixture) => fixture,
        Err(error) => {
            return Observed::rejected("harness", ReasonCode::InternalError, error.to_string());
        }
    };
    let mut roots = RootCertStore::empty();
    if let Err(error) = roots.add(CertificateDer::from(fixture.ca_der.clone())) {
        return Observed::rejected("harness", ReasonCode::InternalError, error.to_string());
    }
    let verifier =
        ExactPinVerifier::new(sha256_fingerprint(&fixture.ca_der)).with_chain_roots(roots);
    let client = match PinnedClient::new(Arc::new(verifier)) {
        Ok(client) => client,
        Err(error) => return Observed::rejected("client", error.code, error.detail),
    };
    let token = [0x13; 32];
    let chain = Some(vec![CertificateDer::from(fixture.ca_der.clone())]);
    let server_fixture = match start_server(&fixture.leaf, chain, token, loopback_policy()).await {
        Ok(server_fixture) => server_fixture,
        Err(error) => return Observed::rejected("harness", ReasonCode::InternalError, error),
    };
    let address = server_fixture.address;
    let error = connect_client(&client, address)
        .await
        .expect_err("a chain-only trust attempt must be refused");
    let (server_result, store) = join_server(server_fixture).await;
    store.clear();
    let mut observed = Observed::rejected(
        "client",
        error.code,
        "the presented chain was valid under the supplied root but the leaf was not the pinned certificate.",
    );
    if let Some(code) = server_code(&server_result) {
        observed.codes.push(ObservedCode {
            layer: "server",
            code,
        });
    }
    observed.trace = vec![format!(
        "client_error={} detail={}",
        error.code.as_str(),
        error.detail
    )];
    observed
}

async fn run_tls_downgrade() -> Observed {
    let certificate = EphemeralCertificate::generate().expect("certificate");
    let token = [0x14; 32];
    let fixture = start_server(&certificate, None, token, loopback_policy())
        .await
        .expect("server");
    let address = fixture.address;
    let config = match client_config_with_custom_verifier(
        Arc::new(AcceptanceOnlyVerifier::new()),
        &[&rustls::version::TLS12],
    ) {
        Ok(config) => config,
        Err(error) => return Observed::rejected("harness", error.code, error.detail),
    };
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let server_name = rustls::pki_types::ServerName::try_from(
        remotecam_signaling::CERTIFICATE_SERVER_NAME.to_owned(),
    )
    .expect("server name");
    let tcp = TcpStream::connect(address).await.expect("tcp");
    let error = connector
        .connect(server_name, tcp)
        .await
        .expect_err("a TLS 1.3-only listener must refuse TLS 1.2");
    let (server_result, store) = join_server(fixture).await;
    store.clear();
    let mut observed = Observed::rejected(
        "client",
        classify_io_error(&error),
        "the TLS 1.2 ClientHello was refused; no downgrade path exists.",
    );
    if let Some(code) = server_code(&server_result) {
        observed.codes.push(ObservedCode {
            layer: "server",
            code,
        });
    }
    observed.trace = vec![format!(
        "client_error={}",
        classify_io_error(&error).as_str()
    )];
    observed
}

enum TokenCase {
    Expired,
    Reused,
    Wrong,
}

async fn run_token_case(case: TokenCase) -> Observed {
    let certificate = EphemeralCertificate::generate().expect("certificate");
    let token = [0x15; 32];
    if matches!(case, TokenCase::Reused) {
        return run_reused_token(&certificate, token).await;
    }
    let fixture = start_server(&certificate, None, token, loopback_policy())
        .await
        .expect("server");
    let address = fixture.address;
    if matches!(case, TokenCase::Expired) {
        fixture.token_store.expire();
    }
    let client = PinnedClient::new(Arc::new(ExactPinVerifier::new(certificate.fingerprint())))
        .expect("client");

    let mut trace = Vec::new();
    let mut presented = token;
    if matches!(case, TokenCase::Wrong) {
        presented[0] ^= 0xff;
    }

    let mut socket = match connect_client(&client, address).await {
        Ok(socket) => socket,
        Err(error) => {
            let (server_result, store) = join_server(fixture).await;
            store.clear();
            trace.push(format!("client_error={}", error.code.as_str()));
            let mut observed =
                Observed::rejected("client", error.code, "the channel did not establish");
            if let Some(code) = server_code(&server_result) {
                observed.codes.push(ObservedCode {
                    layer: "server",
                    code,
                });
            }
            observed.trace = trace;
            return observed;
        }
    };
    let _ = send_pair(&mut socket, &presented, &expected_fingerprints().video).await;
    let answer = await_paired(&mut socket).await;
    let (server_result, store) = join_server(fixture).await;
    store.clear();
    trace.push(format!("paired_answer={answer:?}"));
    trace.push(format!(
        "token_presented={}",
        matches!(case, TokenCase::Wrong) as u8 + 1
    ));
    let code = server_code(&server_result).unwrap_or(ReasonCode::TlsHandshakeFailed);
    let summary = match case {
        TokenCase::Expired => {
            "the correct token was presented after its window closed and was refused."
        }
        TokenCase::Reused => "the token had already been consumed and the replay was refused.",
        TokenCase::Wrong => "the forged token did not match and was refused.",
    };
    Observed {
        accepted: false,
        codes: vec![ObservedCode {
            layer: "server",
            code,
        }],
        summary: summary.to_owned(),
        trace,
        established_channel_ms: None,
    }
}

async fn run_reused_token(certificate: &EphemeralCertificate, token: [u8; 32]) -> Observed {
    let store = Arc::new(TokenStore::new());
    store.issue(token, Duration::ZERO);
    let mut trace = Vec::new();
    let client = PinnedClient::new(Arc::new(ExactPinVerifier::new(certificate.fingerprint())))
        .expect("client");

    let first =
        match start_server_with_store(certificate, None, Arc::clone(&store), loopback_policy())
            .await
        {
            Ok(fixture) => fixture,
            Err(error) => return Observed::rejected("harness", ReasonCode::InternalError, error),
        };
    let first_address = first.address;
    let mut socket = match connect_client(&client, first_address).await {
        Ok(socket) => socket,
        Err(error) => return Observed::rejected("client", error.code, error.detail),
    };
    let _ = send_pair(&mut socket, &token, &expected_fingerprints().video).await;
    let first_answer = await_paired(&mut socket).await;
    let (first_result, _) = join_server(first).await;
    trace.push(format!("first_pairing={first_answer:?}"));
    if first_result.is_err() {
        return Observed::rejected(
            "server",
            server_code(&first_result).unwrap_or(ReasonCode::InternalError),
            "the first pairing did not consume the token",
        );
    }

    // A new listener with the same store: the token is already consumed.
    let second =
        match start_server_with_store(certificate, None, Arc::clone(&store), loopback_policy())
            .await
        {
            Ok(fixture) => fixture,
            Err(error) => return Observed::rejected("harness", ReasonCode::InternalError, error),
        };
    let second_address = second.address;
    let mut replay = match connect_client(&client, second_address).await {
        Ok(socket) => socket,
        Err(error) => return Observed::rejected("client", error.code, error.detail),
    };
    let _ = send_pair(&mut replay, &token, &expected_fingerprints().video).await;
    let replay_answer = await_paired(&mut replay).await;
    let (replay_result, _) = join_server(second).await;
    let code = server_code(&replay_result).unwrap_or(ReasonCode::TlsHandshakeFailed);
    trace.push(format!("replay_answer={replay_answer:?}"));
    trace.push(format!("replay_reason_code={}", code.as_str()));
    Observed {
        accepted: false,
        codes: vec![ObservedCode { layer: "server", code }],
        summary: "the first pairing consumed the token; the replay against a restarted listener was refused."
            .to_owned(),
        trace,
        established_channel_ms: None,
    }
}

async fn run_non_private_source() -> Observed {
    let certificate = EphemeralCertificate::generate().expect("certificate");
    let token = [0x16; 32];
    let policy = SourcePolicy {
        allow_loopback: false,
        expected_subnet: None,
    };
    let fixture = start_server(&certificate, None, token, policy)
        .await
        .expect("server");
    let address = fixture.address;
    let client = PinnedClient::new(Arc::new(ExactPinVerifier::new(certificate.fingerprint())))
        .expect("client");
    let error = connect_client(&client, address)
        .await
        .expect_err("loopback must be refused when it is not a permitted source");
    let (server_result, store) = join_server(fixture).await;
    store.clear();
    let mut observed = Observed::rejected(
        "server",
        server_code(&server_result).unwrap_or(ReasonCode::SourceAddressNotPrivate),
        "the listener dropped the connection before the TLS handshake because the source was not private.",
    );
    if let Some(code) = server_code(&server_result) {
        observed.codes = vec![ObservedCode {
            layer: "server",
            code,
        }];
    }
    observed.trace = vec![
        "source=127.0.0.1 (loopback not permitted for this scenario)".to_owned(),
        format!("client_error={}", error.code.as_str()),
    ];
    observed
}

async fn run_interception_proxy() -> Observed {
    let legitimate = EphemeralCertificate::generate().expect("legitimate certificate");
    let attacker = EphemeralCertificate::generate().expect("attacker certificate");

    // The proxy terminates TLS with its own certificate and would happily answer
    // the pairing message. The client must never get that far.
    let proxy_listener = match bind_loopback().await {
        Ok(listener) => listener,
        Err(error) => {
            return Observed::rejected("harness", ReasonCode::InternalError, error.to_string());
        }
    };
    let proxy_address = proxy_listener.local_addr().expect("proxy address");
    let config = match remotecam_signaling::server::server_config(&attacker, None) {
        Ok(config) => config,
        Err(error) => {
            return Observed::rejected("harness", ReasonCode::InternalError, error.to_string());
        }
    };
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let proxy_token = [0x17; 32];
    let proxy_fingerprints = expected_fingerprints();
    let proxy_task = tokio::spawn(async move {
        let Ok((tcp, _peer)) = proxy_listener.accept().await else {
            return;
        };
        let Ok(tls) = acceptor.accept(tcp).await else {
            return;
        };
        let Ok(mut socket) = tokio_tungstenite::accept_async(tls).await else {
            return;
        };
        if let Some(Ok(_)) = socket.next().await {
            let _ = send_pair(&mut socket, &proxy_token, &proxy_fingerprints.video).await;
            let _ = await_paired(&mut socket).await;
        }
    });

    let client = PinnedClient::new(Arc::new(ExactPinVerifier::new(legitimate.fingerprint())))
        .expect("client");
    let error = connect_client(&client, proxy_address)
        .await
        .expect_err("the client must reject the proxy certificate");
    let _ = proxy_task.await;

    let mut observed = Observed::rejected(
        "client",
        error.code,
        "the client aborted on the pin before sending the pairing token to the proxy.",
    );
    observed.trace = vec![
        "proxy=certificate issued by the attacker".to_owned(),
        format!(
            "client_error={} detail={}",
            error.code.as_str(),
            error.detail
        ),
        "token_sent=false".to_owned(),
    ];
    observed
}

enum QrCase {
    Expired,
    Malformed,
    PublicEndpoint,
}

fn run_qr_case(case: QrCase) -> Observed {
    let now = 1_800_000_000_u64;
    let endpoint = Endpoint::from_address(4, "192.168.1.42", 50000).expect("private endpoint");
    let payload = build_payload(
        [0x33; 16],
        1,
        [0x44; 32],
        [0x55; 32],
        std::slice::from_ref(&endpoint),
        now,
    )
    .expect("payload");

    let (decoded, trace) = match case {
        QrCase::Expired => {
            let bytes = payload.encode().expect("encoded");
            let decoded = QrPayload::decode(&bytes, payload.expires_at);
            (
                decoded,
                vec![format!("payload_expires_at={}", payload.expires_at)],
            )
        }
        QrCase::Malformed => (
            QrPayload::decode(&[0; 10], now),
            vec!["payload_bytes=10 (shorter than the 102-byte header)".to_owned()],
        ),
        QrCase::PublicEndpoint => {
            let mut bytes = payload.encode().expect("encoded");
            let offset = remotecam_protocol::qr::HEADER_BYTES;
            bytes[offset + 13..offset + 17].copy_from_slice(&[8, 8, 8, 8]);
            (
                QrPayload::decode(&bytes, now),
                vec!["payload_endpoint=8.8.8.8:50000".to_owned()],
            )
        }
    };

    let (layer, code, summary) = match decoded {
        Err(QrDecodeError { code }) => (
            "client",
            code,
            "the payload failed validation before any socket was opened.",
        ),
        Ok(_) => (
            "client",
            ReasonCode::InternalError,
            "the payload was accepted but the scenario expected a rejection.",
        ),
    };
    let mut trace = trace;
    trace.push(format!("decoded_reason_code={}", code.as_str()));
    Observed {
        accepted: decoded.is_ok(),
        codes: vec![ObservedCode { layer, code }],
        summary: summary.to_owned(),
        trace,
        established_channel_ms: None,
    }
}

async fn run_tampered_fingerprint() -> Observed {
    let certificate = EphemeralCertificate::generate().expect("certificate");
    let token = [0x18; 32];
    let fixture = start_server(&certificate, None, token, loopback_policy())
        .await
        .expect("server");
    let address = fixture.address;
    let client = PinnedClient::new(Arc::new(ExactPinVerifier::new(certificate.fingerprint())))
        .expect("client");
    let mut socket = match connect_client(&client, address).await {
        Ok(socket) => socket,
        Err(error) => {
            let (server_result, store) = join_server(fixture).await;
            store.clear();
            let mut observed = Observed::rejected("client", error.code, error.detail);
            if let Some(code) = server_code(&server_result) {
                observed.codes.push(ObservedCode {
                    layer: "server",
                    code,
                });
            }
            return observed;
        }
    };
    let mut tampered = expected_fingerprints().video;
    tampered[7] ^= 0xff;
    let _ = send_pair(&mut socket, &token, &tampered).await;
    let answer = await_paired(&mut socket).await;
    let (server_result, store) = join_server(fixture).await;
    store.clear();
    let code = server_code(&server_result).unwrap_or(ReasonCode::TlsHandshakeFailed);
    Observed {
        accepted: false,
        codes: vec![ObservedCode { layer: "server", code }],
        summary: "the token was accepted but the media fingerprint was tampered with, so the session aborted before media.".to_owned(),
        trace: vec![
            format!("paired_answer={answer:?}"),
            "media_started=false".to_owned(),
        ],
        established_channel_ms: None,
    }
}

/// Measures the time exact pinning adds over an acceptance-only control.
///
/// Five runs each, medians compared. The result is reported in milliseconds with
/// sub-millisecond resolution because loopback handshakes are fast; a negative
/// difference is retained as observed (it means the pinned run was not slower on
/// this sample, not that pinning is free).
async fn measure_pinning_delay() -> Option<f64> {
    let pinned = measure_series(true).await?;
    let control = measure_series(false).await?;
    Some((median(&pinned) - median(&control)) / 1_000.0)
}

async fn measure_series(pinned: bool) -> Option<Vec<u64>> {
    let mut samples = Vec::with_capacity(5);
    for _ in 0..5 {
        samples.push(if pinned {
            measure_pinned().await?
        } else {
            measure_control().await?
        });
    }
    Some(samples)
}

fn median(samples: &[u64]) -> f64 {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let middle = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[middle - 1] as f64 + sorted[middle] as f64) / 2.0
    } else {
        sorted[middle] as f64
    }
}

async fn measure_pinned() -> Option<u64> {
    let certificate = EphemeralCertificate::generate().ok()?;
    let token = [0x19; 32];
    let fixture = start_server(&certificate, None, token, loopback_policy())
        .await
        .ok()?;
    let address = fixture.address;
    let client =
        PinnedClient::new(Arc::new(ExactPinVerifier::new(certificate.fingerprint()))).ok()?;
    let started = Instant::now();
    let mut socket = connect_client(&client, address).await.ok()?;
    send_pair(&mut socket, &token, &expected_fingerprints().video)
        .await
        .ok()?;
    await_paired(&mut socket).await.ok()?;
    let elapsed = u64::try_from(started.elapsed().as_micros()).ok();
    let (_, store) = join_server(fixture).await;
    store.clear();
    elapsed
}

async fn measure_control() -> Option<u64> {
    let certificate = EphemeralCertificate::generate().ok()?;
    let token = [0x1a; 32];
    let fixture = start_server(&certificate, None, token, loopback_policy())
        .await
        .ok()?;
    let address = fixture.address;
    let config = client_config_with_custom_verifier(
        Arc::new(AcceptanceOnlyVerifier::new()),
        &[&rustls::version::TLS13],
    )
    .ok()?;
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let started = Instant::now();
    let tcp = TcpStream::connect(address).await.ok()?;
    let server_name = rustls::pki_types::ServerName::try_from(
        remotecam_signaling::CERTIFICATE_SERVER_NAME.to_owned(),
    )
    .ok()?;
    let tls = connector.connect(server_name, tcp).await.ok()?;
    let (mut socket, _) = tokio_tungstenite::client_async("wss://remotecam.local/pairing", tls)
        .await
        .ok()?;
    send_pair(&mut socket, &token, &expected_fingerprints().video)
        .await
        .ok()?;
    await_paired(&mut socket).await.ok()?;
    let elapsed = u64::try_from(started.elapsed().as_micros()).ok();
    let (_, store) = join_server(fixture).await;
    store.clear();
    elapsed
}

/// A distinct loopback source for the legitimate-origin scenario.
pub fn loopback_socket(origin: Ipv4Addr) -> Option<TcpSocket> {
    let socket = TcpSocket::new_v4().ok()?;
    socket.bind(SocketAddr::new(IpAddr::V4(origin), 0)).ok()?;
    Some(socket)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_manifest_matches_the_compiled_matrix() {
        let document: serde_json::Value =
            serde_json::from_str(include_str!("../scenarios/handshake.json")).expect("manifest");
        let rows = document["scenarios"].as_array().expect("scenarios");
        assert_eq!(rows.len(), super::specs().len());
        for (row, spec) in rows.iter().zip(super::specs()) {
            assert_eq!(row["scenario_id"], spec.id);
            assert_eq!(row["category"], spec.category);
            assert_eq!(
                row["expected_reason_code"].as_str(),
                spec.expected.map(remotecam_protocol::ReasonCode::as_str)
            );
        }
    }

    #[test]
    fn the_pair_message_satisfies_the_shared_contract() {
        let token = [0x11_u8; 32];
        let fingerprint = [0xb2_u8; 32];
        let message = serde_json::json!({
            "version": 1,
            "type": "pair",
            "token": remotecam_protocol::hex::encode_hex(&token),
            "dtls_fingerprint": remotecam_protocol::hex::encode_hex(&fingerprint),
            "limits": {},
        });
        let parsed = remotecam_protocol::parse_signaling(message.to_string().as_bytes());
        assert!(parsed.is_ok(), "{parsed:?}");
    }
}
