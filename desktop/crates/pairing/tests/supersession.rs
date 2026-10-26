use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use remotecam_pairing::{ActiveSession, SUPERSEDE_BUDGET};
use remotecam_protocol::ReasonCode;
use remotecam_protocol::signaling::{CloseCode, Limits, SignalingMessage};
use remotecam_session_core::{Effect, SessionId};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::time::Instant;

const DESKTOP_FINGERPRINT: [u8; 32] = [0x44; 32];

type Log = Arc<Mutex<Vec<(&'static str, String)>>>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Behavior {
    Responsive,
    Blocked,
    Broken,
}

/// What the test can still do to a channel the desktop owns.
#[derive(Clone)]
struct Handle {
    behavior: Arc<Mutex<Behavior>>,
    dropped: Arc<AtomicBool>,
}

impl Handle {
    fn set(&self, behavior: Behavior) {
        *self.behavior.lock().unwrap() = behavior;
    }

    fn dropped(&self) -> bool {
        self.dropped.load(Ordering::SeqCst)
    }
}

/// A channel that records what the desktop does to it.
struct Recorder {
    label: &'static str,
    handle: Handle,
    log: Log,
}

impl Recorder {
    fn new(label: &'static str, log: &Log) -> (Self, Handle) {
        let handle = Handle {
            behavior: Arc::new(Mutex::new(Behavior::Responsive)),
            dropped: Arc::new(AtomicBool::new(false)),
        };
        (
            Self {
                label,
                handle: handle.clone(),
                log: Arc::clone(log),
            },
            handle,
        )
    }

    fn behavior(&self) -> Behavior {
        *self.handle.behavior.lock().unwrap()
    }

    fn record(&self, event: String) {
        self.log.lock().unwrap().push((self.label, event));
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.record("drop".to_owned());
        self.handle.dropped.store(true, Ordering::SeqCst);
    }
}

impl AsyncRead for Recorder {
    fn poll_read(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        _buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for Recorder {
    fn poll_write(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.behavior() {
            Behavior::Blocked => Poll::Pending,
            Behavior::Broken => Poll::Ready(Err(io::ErrorKind::BrokenPipe.into())),
            Behavior::Responsive => {
                self.record(format!("write {}", String::from_utf8_lossy(buffer)));
                Poll::Ready(Ok(buffer.len()))
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.behavior() {
            Behavior::Blocked => Poll::Pending,
            _ => {
                self.record("shutdown".to_owned());
                Poll::Ready(Ok(()))
            }
        }
    }
}

fn paired(identity: u8, epoch: u32) -> Effect {
    Effect::SendPaired {
        session_id: SessionId::from_bytes([identity; 16]),
        epoch,
        dtls_fingerprint: DESKTOP_FINGERPRINT,
        accepted_limits: Limits::new(),
    }
}

fn paired_text(identity: u8, epoch: u32) -> String {
    let frame = SignalingMessage::Paired {
        session_id: [identity; 16],
        epoch: u64::from(epoch),
        dtls_fingerprint: DESKTOP_FINGERPRINT,
        accepted_limits: Limits::new(),
    }
    .encode()
    .expect("paired encodes");
    format!("{}\n", String::from_utf8(frame).expect("utf-8"))
}

fn close_text() -> String {
    let frame = SignalingMessage::Close {
        code: CloseCode::Superseded,
    }
    .encode()
    .expect("close encodes");
    format!("{}\n", String::from_utf8(frame).expect("utf-8"))
}

fn supersession(identity: u8, epoch: u32) -> Vec<Effect> {
    vec![
        Effect::SendClose {
            reason: CloseCode::Superseded,
        },
        Effect::End,
        paired(identity, epoch),
    ]
}

/// Joins the writes of one frame so the order test does not depend on how a frame is chunked.
fn events(log: &Log) -> Vec<(&'static str, String)> {
    let mut merged: Vec<(&'static str, String)> = Vec::new();
    for (label, event) in log.lock().unwrap().iter() {
        match (merged.last_mut(), event.strip_prefix("write ")) {
            (Some((last, previous)), Some(data))
                if last == label && previous.starts_with("write ") =>
            {
                previous.push_str(data);
            }
            _ => merged.push((label, event.clone())),
        }
    }
    merged
}

#[tokio::test]
async fn the_first_pairing_installs_the_peer_and_the_second_replaces_it_in_order() {
    let log = Log::default();
    let (first, _) = Recorder::new("first", &log);
    let (second, _) = Recorder::new("second", &log);
    let mut session = ActiveSession::new();
    assert!(!session.is_occupied());

    session
        .apply(vec![paired(1, 1)], first)
        .await
        .expect("the first peer is paired");
    assert!(session.is_occupied());
    log.lock().unwrap().clear();

    session
        .apply(supersession(2, 2), second)
        .await
        .expect("the successor is paired");

    assert_eq!(
        events(&log),
        vec![
            ("first", format!("write {}", close_text())),
            ("first", "shutdown".to_owned()),
            ("first", "drop".to_owned()),
            ("second", format!("write {}", paired_text(2, 2))),
        ]
    );
    assert!(session.is_occupied());
}

async fn replaced_while(behavior: Behavior) -> (Duration, Log, Handle, ActiveSession<Recorder>) {
    let log = Log::default();
    let (first, first_handle) = Recorder::new("first", &log);
    let (second, _) = Recorder::new("second", &log);
    let mut session = ActiveSession::new();
    session
        .apply(vec![paired(1, 1)], first)
        .await
        .expect("the first peer is paired");
    first_handle.set(behavior);
    log.lock().unwrap().clear();

    let started = Instant::now();
    session
        .apply(supersession(2, 2), second)
        .await
        .expect("the successor is paired whatever the predecessor does");
    (started.elapsed(), log, first_handle, session)
}

#[tokio::test(start_paused = true)]
async fn a_predecessor_that_never_reads_does_not_delay_the_successor_past_the_budget() {
    let (elapsed, log, first, session) = replaced_while(Behavior::Blocked).await;

    assert!(elapsed <= SUPERSEDE_BUDGET, "took {elapsed:?}");
    assert!(first.dropped());
    assert_eq!(
        events(&log),
        vec![
            ("first", "drop".to_owned()),
            ("second", format!("write {}", paired_text(2, 2))),
        ]
    );
    assert!(session.is_occupied());
}

#[tokio::test]
async fn a_predecessor_that_is_already_gone_is_dropped_before_the_successor_is_paired() {
    let (_, log, first, session) = replaced_while(Behavior::Broken).await;

    assert!(first.dropped());
    assert_eq!(
        events(&log),
        vec![
            ("first", "shutdown".to_owned()),
            ("first", "drop".to_owned()),
            ("second", format!("write {}", paired_text(2, 2))),
        ]
    );
    assert!(session.is_occupied());
}

#[tokio::test]
async fn effects_the_session_does_not_own_are_refused() {
    let log = Log::default();
    let (peer, _) = Recorder::new("peer", &log);
    let mut session = ActiveSession::new();

    let error = session
        .apply(vec![Effect::StartMedia], peer)
        .await
        .expect_err("starting media is not a pairing effect");

    assert_eq!(error.code(), ReasonCode::InternalError);
    assert!(!session.is_occupied());
}

#[tokio::test]
async fn a_successor_that_cannot_be_paired_leaves_no_session_behind() {
    let log = Log::default();
    let (first, _) = Recorder::new("first", &log);
    let (second, second_handle) = Recorder::new("second", &log);
    let mut session = ActiveSession::new();
    session
        .apply(vec![paired(1, 1)], first)
        .await
        .expect("the first peer is paired");
    second_handle.set(Behavior::Broken);

    let error = session
        .apply(supersession(2, 2), second)
        .await
        .expect_err("the successor cannot be written to");

    assert_eq!(error.code(), ReasonCode::InternalError);
    assert!(!session.is_occupied());
    assert!(second_handle.dropped());
}

mod over_tls {
    use std::sync::Arc;

    use remotecam_pairing::{
        ActiveSession, EphemeralCertificate, PairingListener, PinnedClient, ServerChannel,
        read_frame, write_frame,
    };
    use remotecam_protocol::signaling::{Limits, SignalingMessage};
    use remotecam_session_core::testing::{ManualClock, SeededRandomSource};
    use remotecam_session_core::{
        Effect, MediaFrame, PairingError, PairingMaterial, SessionCore, SessionEvent, SessionState,
        Timestamp,
    };
    use tokio::task::JoinHandle;

    use super::{DESKTOP_FINGERPRINT, close_text};

    type Core = SessionCore<ManualClock, SeededRandomSource>;

    struct Desktop {
        core: Core,
        listener: Arc<PairingListener>,
        session: ActiveSession<ServerChannel>,
    }

    async fn desktop() -> Desktop {
        let core = SessionCore::new(
            ManualClock::new(Timestamp::ZERO),
            SeededRandomSource::new(7),
            DESKTOP_FINGERPRINT,
        );
        let listener = Arc::new(
            PairingListener::bind_loopback(EphemeralCertificate::generate().expect("certificate"))
                .await
                .expect("listener"),
        );
        Desktop {
            core,
            listener,
            session: ActiveSession::new(),
        }
    }

    impl Desktop {
        fn present_pairing(&mut self) -> PairingMaterial {
            let mut effects = self
                .core
                .handle(SessionEvent::StartPairing)
                .expect("starting pairing is valid");
            let Some(Effect::PresentPairing(material)) = effects.pop() else {
                panic!("starting pairing presents its material");
            };
            material
        }

        /// A phone that scans `material`, pairs, and reports every frame until its channel ends.
        fn phone(&self, material: &PairingMaterial) -> JoinHandle<Vec<String>> {
            let address = self.listener.local_addr();
            let fingerprint = self.listener.fingerprint();
            let token = *material.token().as_bytes();
            tokio::spawn(async move {
                let client = PinnedClient::new(fingerprint).expect("client");
                let mut channel = client.connect(address).await.expect("handshake");
                let pair = SignalingMessage::Pair {
                    token,
                    dtls_fingerprint: [0x55; 32],
                    limits: Limits::new(),
                }
                .encode()
                .expect("pair encodes");
                write_frame(&mut channel, &pair).await.expect("pair sent");
                let mut frames = Vec::new();
                while let Ok(frame) = read_frame(&mut channel).await {
                    frames.push(format!(
                        "{}\n",
                        String::from_utf8(frame.to_vec()).expect("utf-8")
                    ));
                }
                frames
            })
        }

        /// Accepts the next peer and returns its channel and the first frame it sent.
        async fn accept(&mut self) -> (ServerChannel, Vec<u8>) {
            let core = &mut self.core;
            let mut channel = self
                .listener
                .accept_admitted(|| {
                    let effects = core.handle(SessionEvent::PeerConnected).expect("admitted");
                    assert!(effects.is_empty());
                    None
                })
                .await
                .expect("the peer is admitted");
            let frame = read_frame(&mut channel).await.expect("pair frame");
            (channel, frame.to_vec())
        }

        async fn pair(&mut self, channel: ServerChannel, frame: &[u8]) {
            let effects = self
                .core
                .authenticate_first_frame(frame)
                .expect("the token authenticates");
            self.session
                .apply(effects, channel)
                .await
                .expect("the peer is paired");
        }

        fn media_flowing(&mut self) {
            self.core
                .handle(SessionEvent::NegotiationCompleted)
                .expect("negotiation completes");
            self.core
                .handle(SessionEvent::FirstMediaFrame(MediaFrame {
                    sequence: 0,
                    timestamp: Timestamp::ZERO,
                }))
                .expect("the first frame arrives on time");
            assert_eq!(self.core.state(), SessionState::Connected);
        }

        async fn connected_peer(&mut self) -> JoinHandle<Vec<String>> {
            let material = self.present_pairing();
            let phone = self.phone(&material);
            let (channel, frame) = self.accept().await;
            self.pair(channel, &frame).await;
            self.media_flowing();
            phone
        }
    }

    fn paired_frame_for(desktop: &Desktop, material: &PairingMaterial) -> String {
        let _ = desktop;
        let frame = SignalingMessage::Paired {
            session_id: *material.session_id().as_bytes(),
            epoch: u64::from(material.epoch()),
            dtls_fingerprint: DESKTOP_FINGERPRINT,
            accepted_limits: Limits::new(),
        }
        .encode()
        .expect("paired encodes");
        format!("{}\n", String::from_utf8(frame).expect("utf-8"))
    }

    #[tokio::test]
    async fn a_new_qr_scanned_while_connected_replaces_the_connected_peer() {
        let mut desktop = desktop().await;
        let first = desktop.present_pairing();
        let connected = desktop.phone(&first);
        let (channel, frame) = desktop.accept().await;
        desktop.pair(channel, &frame).await;
        desktop.media_flowing();

        let second = desktop.present_pairing();
        let successor = desktop.phone(&second);
        let (channel, frame) = desktop.accept().await;
        desktop.pair(channel, &frame).await;

        assert_eq!(
            connected.await.expect("first phone"),
            vec![paired_frame_for(&desktop, &first), close_text()]
        );
        assert_eq!(desktop.core.state(), SessionState::Negotiating);
        assert_eq!(desktop.core.retired_state(), Some(SessionState::Superseded));
        assert!(desktop.session.is_occupied());

        let expected = paired_frame_for(&desktop, &second);
        drop(desktop);
        assert_eq!(successor.await.expect("second phone"), vec![expected]);
    }

    #[tokio::test]
    async fn the_peer_that_lost_the_race_for_the_new_qr_is_closed_as_superseded() {
        let mut desktop = desktop().await;
        let connected = desktop.connected_peer().await;
        let second = desktop.present_pairing();
        let winner = desktop.phone(&second);
        let (winner_channel, winner_frame) = desktop.accept().await;
        let loser = desktop.phone(&second);
        let (loser_channel, loser_frame) = desktop.accept().await;

        desktop.pair(winner_channel, &winner_frame).await;
        let refusal = desktop
            .core
            .authenticate_first_frame(&loser_frame)
            .expect_err("the consumed token cannot pair twice");
        assert_eq!(refusal, PairingError::Superseded);
        let error = loser_channel.reject(refusal.code()).await;

        assert_eq!(error.code(), PairingError::Superseded.code());
        assert_eq!(loser.await.expect("loser phone"), vec![close_text()]);
        assert_eq!(desktop.core.state(), SessionState::Negotiating);
        assert!(desktop.session.is_occupied());

        let expected = paired_frame_for(&desktop, &second);
        let replaced = connected.await.expect("first phone");
        assert_eq!(replaced.last(), Some(&close_text()));
        drop(desktop);
        assert_eq!(winner.await.expect("winner phone"), vec![expected]);
    }
}
