use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use remotecam_pairing::throttle::{OriginThrottle, ThrottlePolicy};
use remotecam_pairing::{EphemeralCertificate, PairingListener, PinnedClient};
use remotecam_protocol::ReasonCode;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::task::JoinSet;

async fn listener() -> PairingListener {
    PairingListener::bind_loopback(EphemeralCertificate::generate().expect("certificate"))
        .await
        .expect("listener")
        .with_origin_throttle(OriginThrottle::with_policy(ThrottlePolicy {
            attempt_limit: usize::MAX,
            ..ThrottlePolicy::default()
        }))
}

#[tokio::test]
async fn a_closed_listener_refuses_the_peer_without_a_frame_and_never_asks_the_session() {
    let listener = listener().await;
    let address = listener.local_addr();
    listener.close();
    let asked = AtomicBool::new(false);

    let peer = tokio::spawn(async move {
        let mut socket = TcpStream::connect(address).await.expect("tcp connect");
        let mut received = Vec::new();
        socket
            .read_to_end(&mut received)
            .await
            .expect("the refused peer sees a close");
        received
    });
    let error = listener
        .accept_admitted(|| {
            asked.store(true, Ordering::SeqCst);
            Some(ReasonCode::Superseded)
        })
        .await
        .expect_err("a closed listener admits nobody");

    assert_eq!(error.code(), ReasonCode::ListenerClosed);
    assert!(peer.await.expect("peer task").is_empty());
    assert!(
        !asked.load(Ordering::SeqCst),
        "listener_closed wins over superseded"
    );
    assert_eq!(listener.attempts(), 1);
}

#[tokio::test]
async fn closing_between_accept_and_handshake_still_refuses_the_peer() {
    let listener = listener().await;
    let address = listener.local_addr();
    let fingerprint = listener.fingerprint();
    let peer = tokio::spawn(async move {
        PinnedClient::new(fingerprint)
            .expect("client")
            .connect(address)
            .await
    });

    let socket = listener.accept_socket().await.expect("tcp accept");
    listener.close();
    let error = listener
        .handshake(socket)
        .await
        .expect_err("the handshake is refused once the listener is closed");

    assert_eq!(error.code(), ReasonCode::ListenerClosed);
    assert!(peer.await.expect("peer task").is_err());
}

#[tokio::test]
async fn an_open_listener_keeps_admitting_peers() {
    let listener = listener().await;

    admit_one(&listener).await;
}

async fn admit_one(listener: &PairingListener) {
    let address = listener.local_addr();
    let fingerprint = listener.fingerprint();
    let peer = tokio::spawn(async move {
        PinnedClient::new(fingerprint)
            .expect("client")
            .connect(address)
            .await
            .is_ok()
    });

    listener.accept().await.expect("an open listener admits");

    assert!(peer.await.expect("peer task"));
}

async fn fail_one_handshake(listener: &PairingListener) {
    let address = listener.local_addr();
    let peer = tokio::spawn(async move {
        PinnedClient::new([0; 32])
            .expect("client")
            .connect(address)
            .await
    });

    listener
        .accept()
        .await
        .expect_err("a divergent pin fails the handshake");

    assert!(peer.await.expect("peer task").is_err());
}

#[tokio::test]
async fn ten_consecutive_failed_handshakes_close_the_listener() {
    let listener = listener().await;
    let address = listener.local_addr();
    for _ in 0..10 {
        fail_one_handshake(&listener).await;
    }

    let peer = tokio::spawn(async move {
        let mut socket = TcpStream::connect(address).await.expect("tcp connect");
        let mut received = Vec::new();
        socket
            .read_to_end(&mut received)
            .await
            .expect("the refused peer sees a close");
        received
    });
    let error = listener
        .accept()
        .await
        .expect_err("the tenth failure closed the listener");

    assert_eq!(error.code(), ReasonCode::ListenerClosed);
    assert!(peer.await.expect("peer task").is_empty());
}

#[tokio::test]
async fn concurrent_failed_handshakes_close_the_listener_once() {
    let listener = Arc::new(listener().await);
    let address = listener.local_addr();
    let fingerprint = [0; 32];
    let peers: Vec<_> = (0..10)
        .map(|_| {
            tokio::spawn(async move {
                PinnedClient::new(fingerprint)
                    .expect("client")
                    .connect(address)
                    .await
            })
        })
        .collect();

    let mut accepts = JoinSet::new();
    for _ in 0..10 {
        let listener = Arc::clone(&listener);
        accepts.spawn(async move { listener.accept().await });
    }
    while let Some(result) = accepts.join_next().await {
        result
            .expect("accept task")
            .expect_err("a divergent pin fails the handshake");
    }
    for peer in peers {
        assert!(peer.await.expect("peer task").is_err());
    }

    let peer = tokio::spawn(async move {
        let mut socket = TcpStream::connect(address).await.expect("tcp connect");
        let mut received = Vec::new();
        socket
            .read_to_end(&mut received)
            .await
            .expect("the refused peer sees a close");
        received
    });
    let error = listener
        .accept()
        .await
        .expect_err("ten concurrent failures closed the listener");

    assert_eq!(error.code(), ReasonCode::ListenerClosed);
    assert!(peer.await.expect("peer task").is_empty());
}

#[tokio::test]
async fn nine_failures_keep_the_listener_open() {
    let listener = listener().await;
    for _ in 0..9 {
        fail_one_handshake(&listener).await;
    }

    admit_one(&listener).await;
}

#[tokio::test]
async fn a_successful_handshake_resets_the_failure_count() {
    let listener = listener().await;
    for _ in 0..9 {
        fail_one_handshake(&listener).await;
    }
    admit_one(&listener).await;
    for _ in 0..9 {
        fail_one_handshake(&listener).await;
    }

    admit_one(&listener).await;
}
