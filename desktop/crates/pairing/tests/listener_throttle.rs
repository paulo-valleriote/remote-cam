use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use remotecam_pairing::{EphemeralCertificate, PairingListener, PinnedClient, SecurityEvent};
use remotecam_protocol::ReasonCode;
use remotecam_session_core::testing::ManualClock;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpSocket, TcpStream};

const FIRST: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
const SECOND: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2));

async fn listener(clock: &ManualClock) -> PairingListener {
    PairingListener::bind_loopback(EphemeralCertificate::generate().expect("certificate"))
        .await
        .expect("listener")
        .with_clock(clock.clone())
}

async fn connect_from(origin: IpAddr, target: SocketAddr) -> TcpStream {
    let socket = TcpSocket::new_v4().expect("socket");
    socket
        .bind(SocketAddr::new(origin, 0))
        .expect("bind origin");
    socket.connect(target).await.expect("tcp connect")
}

async fn knock(
    listener: &PairingListener,
    origin: IpAddr,
) -> (Result<TcpStream, ReasonCode>, TcpStream) {
    let client = connect_from(origin, listener.local_addr()).await;
    let verdict = listener.accept_socket().await.map_err(|error| error.code());
    (verdict, client)
}

#[tokio::test]
async fn the_sixth_connection_from_one_origin_is_refused_with_rate_limited_and_no_frame() {
    let clock = ManualClock::default();
    let listener = listener(&clock).await;
    let mut held = Vec::new();
    for _ in 0..5 {
        let (verdict, client) = knock(&listener, FIRST).await;
        assert!(verdict.is_ok(), "attempts within the limit are admitted");
        held.push(client);
    }

    let (verdict, mut refused) = knock(&listener, FIRST).await;

    assert_eq!(verdict.expect_err("sixth attempt"), ReasonCode::RateLimited);
    let mut received = Vec::new();
    refused
        .read_to_end(&mut received)
        .await
        .expect("the refused peer sees a close");
    assert!(received.is_empty());
}

#[tokio::test]
async fn a_second_origin_is_admitted_while_the_first_is_blocked() {
    let clock = ManualClock::default();
    let listener = listener(&clock).await;
    let mut held = Vec::new();
    for _ in 0..6 {
        held.push(knock(&listener, FIRST).await);
    }

    let (verdict, _client) = knock(&listener, SECOND).await;

    assert!(
        verdict.is_ok(),
        "throttle state is per origin, never global"
    );
}

#[tokio::test]
async fn the_blocked_origin_is_admitted_again_when_the_block_ends() {
    let clock = ManualClock::default();
    let listener = listener(&clock).await;
    let mut held = Vec::new();
    for _ in 0..6 {
        held.push(knock(&listener, FIRST).await);
    }

    clock.advance(60_000);
    let (verdict, _client) = knock(&listener, FIRST).await;

    assert!(verdict.is_ok());
}

#[tokio::test]
async fn throttle_refusals_never_count_as_failed_handshakes() {
    let clock = ManualClock::default();
    let listener = listener(&clock).await;
    let mut held = Vec::new();
    for _ in 0..30 {
        held.push(knock(&listener, FIRST).await);
    }

    clock.advance(24 * 60 * 60 * 1_000);
    let (verdict, _client) = knock(&listener, SECOND).await;

    assert!(verdict.is_ok(), "rate_limited must not close the listener");
}

#[tokio::test]
async fn a_throttled_origin_is_recorded_in_the_local_diagnostic() {
    let clock = ManualClock::default();
    let listener = listener(&clock).await;
    let mut held = Vec::new();
    for _ in 0..6 {
        held.push(knock(&listener, FIRST).await);
    }

    let events = listener.drain_security_events();

    assert_eq!(
        events,
        [SecurityEvent {
            reason: ReasonCode::RateLimited,
            origin: Some(FIRST),
            blocked_for_millis: Some(60_000),
        }]
    );
    assert!(listener.drain_security_events().is_empty());
}

#[tokio::test]
async fn closing_after_ten_failures_is_recorded_without_fingerprint_or_token() {
    let clock = ManualClock::default();
    let listener = listener(&clock).await;
    let address = listener.local_addr();
    for round in 0..10 {
        clock.advance(round * 60_000);
        let peer = tokio::spawn(async move {
            PinnedClient::new([0; 32])
                .expect("client")
                .connect(address)
                .await
        });
        listener.accept().await.expect_err("divergent pin");
        assert!(peer.await.expect("peer task").is_err());
    }

    let events = listener.drain_security_events();

    let closed: Vec<_> = events
        .iter()
        .filter(|event| event.reason == ReasonCode::ListenerClosed)
        .collect();
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].origin, None);
    assert!(
        events
            .iter()
            .any(|event| event.reason == ReasonCode::CertificatePinMismatch
                || event.reason == ReasonCode::TlsHandshakeFailed),
        "each failed handshake is recorded with its closed code"
    );
    let rendered = format!("{events:?}");
    assert!(!rendered.contains(&remotecam_protocol::hex::encode_hex(
        &listener.fingerprint()
    )));
}
