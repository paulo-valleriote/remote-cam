use std::sync::atomic::{AtomicBool, Ordering};

use remotecam_pairing::{EphemeralCertificate, PairingListener, PinnedClient};
use remotecam_protocol::ReasonCode;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

async fn listener() -> PairingListener {
    PairingListener::bind_loopback(EphemeralCertificate::generate().expect("certificate"))
        .await
        .expect("listener")
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
