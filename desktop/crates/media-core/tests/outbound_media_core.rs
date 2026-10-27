#![cfg(not(windows))]

//! Continuous Receive-only Invariant assertion on the outbound path of a real `MediaCore`.

use std::time::Duration;

use remotecam_media_core::testing::SyntheticPhone;
use remotecam_media_core::{MediaCore, MediaCoreError, OutboundRtpTap};
use remotecam_protocol::ReasonCode;
use rtc::interceptor::{AttributedPacket, Packet, TaggedPacket};
use rtc::sansio::Protocol;
use rtc::shared::TransportContext;
use tokio::time::timeout;

const LOOPBACK: &str = "127.0.0.1:0";
const DEADLINE: Duration = Duration::from_secs(15);

fn media_packet() -> TaggedPacket {
    TaggedPacket {
        now: std::time::Instant::now(),
        transport: TransportContext::default(),
        message: AttributedPacket::new(Packet::Rtp(rtc::rtp::Packet::default())),
    }
}

async fn negotiated() -> (MediaCore, SyntheticPhone) {
    let core = MediaCore::new(LOOPBACK).await.expect("media core");
    let phone = SyntheticPhone::new(LOOPBACK).await.expect("phone");
    let offer = phone.offer().await.expect("offer");
    let answer = core.answer(&offer).await.expect("answer");
    phone.accept_answer(&answer).await.expect("accept answer");
    phone.connected().await.expect("connected");
    (core, phone)
}

#[tokio::test(flavor = "current_thread")]
async fn a_fresh_media_core_has_sent_nothing_and_holds_the_invariant() {
    let core = MediaCore::new(LOOPBACK).await.expect("media core");

    assert_eq!(core.outbound().media_rtp(), 0);
    assert_eq!(core.outbound().control_rtcp(), 0);
    assert_eq!(core.assert_receive_only(), Ok(()));
    core.close().await.expect("close");
}

#[tokio::test(flavor = "current_thread")]
async fn a_live_session_sends_control_rtcp_but_never_media_rtp() {
    let (core, mut phone) = negotiated().await;

    timeout(DEADLINE, async {
        while core.outbound().control_rtcp() == 0 {
            phone.send_frame().await.expect("send");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the desktop reports back over RTCP, observed on the real write path");

    assert_eq!(core.outbound().media_rtp(), 0);
    assert_eq!(core.assert_receive_only(), Ok(()));
    core.close().await.expect("close");
    phone.close().await.expect("phone close");
}

#[tokio::test(flavor = "current_thread")]
async fn outbound_media_rtp_makes_the_media_core_fail_with_a_reason_code() {
    let core = MediaCore::new(LOOPBACK).await.expect("media core");
    let mut tap = OutboundRtpTap::new(core.outbound().clone());

    tap.handle_write(media_packet()).unwrap();

    assert_eq!(
        core.assert_receive_only(),
        Err(MediaCoreError::ReceiveOnlyViolation(
            ReasonCode::SecurityViolation
        ))
    );
    assert_eq!(
        core.close().await,
        Err(MediaCoreError::ReceiveOnlyViolation(
            ReasonCode::SecurityViolation
        )),
        "teardown still happens, but the violation is not swallowed"
    );
}

#[test]
fn the_violation_error_names_the_closed_reason_code() {
    let error = MediaCoreError::ReceiveOnlyViolation(ReasonCode::SecurityViolation);

    assert!(error.to_string().contains("security_violation"));
}

#[tokio::test(flavor = "current_thread")]
async fn the_media_core_announces_a_violation_without_being_polled() {
    let core = MediaCore::new(LOOPBACK).await.expect("media core");
    let mut tap = OutboundRtpTap::new(core.outbound().clone());
    let announced = timeout(DEADLINE, core.receive_only_violated());

    tap.handle_write(media_packet()).unwrap();

    assert_eq!(
        announced.await.expect("announced"),
        MediaCoreError::ReceiveOnlyViolation(ReasonCode::SecurityViolation)
    );
    let _ = core.close().await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_violation_wins_over_a_failing_teardown() {
    let core = MediaCore::new(LOOPBACK).await.expect("media core");
    core.close().await.expect("first close");
    let mut tap = OutboundRtpTap::new(core.outbound().clone());
    tap.handle_write(media_packet()).unwrap();

    assert_eq!(
        core.close().await,
        Err(MediaCoreError::ReceiveOnlyViolation(
            ReasonCode::SecurityViolation
        ))
    );
}
