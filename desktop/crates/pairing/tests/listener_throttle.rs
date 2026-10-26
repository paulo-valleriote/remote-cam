use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use remotecam_pairing::{
    AdmittedSocket, DiagnosticEntry, EphemeralCertificate, PairingListener, PinnedClient,
};
use remotecam_protocol::ReasonCode;
use remotecam_session_core::testing::ManualClock;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpSocket, TcpStream};
use tokio::task::JoinSet;

const FIRST: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
#[cfg(any(target_os = "linux", windows))]
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

async fn attempt_from(
    listener: &PairingListener,
    origin: IpAddr,
) -> (Result<AdmittedSocket, ReasonCode>, TcpStream) {
    let client = connect_from(origin, listener.local_addr()).await;
    let verdict = listener.accept_socket().await.map_err(|error| error.code());
    (verdict, client)
}

async fn spend_budget(listener: &PairingListener, origin: IpAddr) -> Vec<TcpStream> {
    let mut clients = Vec::new();
    for _ in 0..5 {
        let (verdict, client) = attempt_from(listener, origin).await;
        assert!(verdict.is_ok(), "attempts within the limit are admitted");
        clients.push(client);
    }
    clients
}

#[tokio::test]
async fn the_sixth_connection_from_one_origin_is_refused_with_rate_limited_and_no_frame() {
    let clock = ManualClock::default();
    let listener = listener(&clock).await;
    let _clients = spend_budget(&listener, FIRST).await;

    let (verdict, mut refused) = attempt_from(&listener, FIRST).await;

    assert_eq!(verdict.expect_err("sixth attempt"), ReasonCode::RateLimited);
    let mut received = Vec::new();
    refused
        .read_to_end(&mut received)
        .await
        .expect("the refused peer sees a close");
    assert!(received.is_empty());
}

#[cfg(any(target_os = "linux", windows))]
#[tokio::test]
async fn a_second_origin_is_admitted_while_the_first_is_blocked() {
    let clock = ManualClock::default();
    let listener = listener(&clock).await;
    let _clients = spend_budget(&listener, FIRST).await;
    let (blocked, _blocked_client) = attempt_from(&listener, FIRST).await;
    assert_eq!(blocked.expect_err("blocked"), ReasonCode::RateLimited);

    let (verdict, _client) = attempt_from(&listener, SECOND).await;

    assert!(
        verdict.is_ok(),
        "throttle state is per origin, never global"
    );
}

#[tokio::test]
async fn the_blocked_origin_is_admitted_again_when_the_block_ends() {
    let clock = ManualClock::default();
    let listener = listener(&clock).await;
    let _clients = spend_budget(&listener, FIRST).await;
    let (blocked, _blocked_client) = attempt_from(&listener, FIRST).await;
    assert_eq!(blocked.expect_err("blocked"), ReasonCode::RateLimited);

    clock.advance(60_000);
    let (verdict, _client) = attempt_from(&listener, FIRST).await;

    assert!(verdict.is_ok());
}

#[cfg(any(target_os = "linux", windows))]
#[tokio::test]
async fn throttle_refusals_never_count_as_failed_handshakes() {
    let clock = ManualClock::default();
    let listener = listener(&clock).await;
    let mut clients = Vec::new();
    for _ in 0..30 {
        clients.push(attempt_from(&listener, FIRST).await);
    }

    let (verdict, _client) = attempt_from(&listener, SECOND).await;

    assert!(verdict.is_ok(), "rate_limited must not close the listener");
}

#[tokio::test]
async fn concurrent_attempts_from_one_origin_admit_exactly_the_limit() {
    let clock = ManualClock::default();
    let listener = Arc::new(listener(&clock).await);
    let address = listener.local_addr();
    let mut clients = Vec::new();
    for _ in 0..12 {
        clients.push(connect_from(FIRST, address).await);
    }

    let mut accepts = JoinSet::new();
    for _ in 0..12 {
        let listener = Arc::clone(&listener);
        accepts.spawn(async move { listener.accept_socket().await.map_err(|e| e.code()) });
    }
    let mut admitted = 0;
    let mut limited = 0;
    while let Some(result) = accepts.join_next().await {
        match result.expect("accept task") {
            Ok(_) => admitted += 1,
            Err(ReasonCode::RateLimited) => limited += 1,
            Err(other) => panic!("unexpected refusal {other:?}"),
        }
    }

    assert_eq!((admitted, limited), (5, 7));
}

#[tokio::test]
async fn a_throttled_origin_is_recorded_in_the_local_diagnostic() {
    let clock = ManualClock::default();
    let listener = listener(&clock).await;
    let _clients = spend_budget(&listener, FIRST).await;
    let (_verdict, _client) = attempt_from(&listener, FIRST).await;

    let entries = listener.drain_diagnostics();

    assert_eq!(
        entries,
        [DiagnosticEntry {
            reason: ReasonCode::RateLimited,
            origin: Some(FIRST),
            blocked_for_millis: Some(60_000),
        }]
    );
    assert!(!entries[0].is_security_event());
    assert!(listener.drain_diagnostics().is_empty());
}

#[tokio::test]
async fn ten_failed_handshakes_are_recorded_with_their_code_and_one_listener_closed_entry() {
    let clock = ManualClock::default();
    let listener = listener(&clock).await;
    let address = listener.local_addr();
    for _ in 0..10 {
        clock.advance(60_000);
        let peer = tokio::spawn(async move {
            PinnedClient::new([0; 32])
                .expect("client")
                .connect(address)
                .await
        });
        listener.accept().await.expect_err("divergent pin");
        assert!(peer.await.expect("peer task").is_err());
    }

    let entries = listener.drain_diagnostics();

    let closed: Vec<_> = entries
        .iter()
        .filter(|entry| entry.reason == ReasonCode::ListenerClosed)
        .collect();
    assert_eq!(closed.len(), 1);
    assert!(closed[0].is_security_event());
    assert_eq!(closed[0].origin, None);
    let failures: Vec<_> = entries
        .iter()
        .filter(|entry| entry.reason != ReasonCode::ListenerClosed)
        .collect();
    assert_eq!(failures.len(), 10);
    assert!(failures.iter().all(|entry| entry.origin == Some(FIRST)));
    let rendered = format!("{entries:?}");
    assert!(!rendered.contains(&remotecam_protocol::hex::encode_hex(
        &listener.fingerprint()
    )));
}

#[tokio::test]
async fn closing_the_listener_by_hand_is_recorded_once() {
    let clock = ManualClock::default();
    let listener = listener(&clock).await;

    listener.close();
    listener.close();

    assert_eq!(
        listener.drain_diagnostics(),
        [DiagnosticEntry {
            reason: ReasonCode::ListenerClosed,
            origin: None,
            blocked_for_millis: None,
        }]
    );
}

#[tokio::test]
async fn a_handshake_refused_because_the_listener_closed_is_recorded_with_its_origin() {
    let clock = ManualClock::default();
    let listener = listener(&clock).await;
    let (verdict, _client) = attempt_from(&listener, FIRST).await;
    let socket = verdict.expect("admitted");
    listener.close();
    listener.drain_diagnostics();

    let error = listener.handshake(socket).await.expect_err("closed");

    assert_eq!(error.code(), ReasonCode::ListenerClosed);
    assert_eq!(
        listener.drain_diagnostics(),
        [DiagnosticEntry {
            reason: ReasonCode::ListenerClosed,
            origin: Some(FIRST),
            blocked_for_millis: None,
        }]
    );
}

#[tokio::test]
async fn the_diagnostic_log_is_bounded_and_counts_what_it_dropped() {
    let clock = ManualClock::default();
    let listener = listener(&clock).await;
    let _clients = spend_budget(&listener, FIRST).await;
    let mut refused = Vec::new();
    for _ in 0..300 {
        refused.push(attempt_from(&listener, FIRST).await);
        clock.advance(1);
    }
    let kept = listener.drain_diagnostics().len();

    assert_eq!(kept, 256);
    assert_eq!(listener.dropped_diagnostics(), 300 - 256);
}
