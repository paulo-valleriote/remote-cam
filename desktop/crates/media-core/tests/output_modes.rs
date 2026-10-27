#![cfg(not(windows))]

//! REC-52: the Media Port publishes a requested mode at 30 fps. The synthetic
//! phone sends 64x48 pictures, so every published frame is rescaled; a manual
//! clock stands in for the consumer's 30 fps cadence.

use std::sync::Arc;
use std::time::Duration;

use remotecam_media_core::output::OutputMode;
use remotecam_media_core::testing::SyntheticPhone;
use remotecam_media_core::{MediaCore, RingMediaPort};
use remotecam_session_core::testing::ManualClock;
use remotecam_session_core::{MediaPort, Timestamp};
use tokio::time::timeout;

const LOOPBACK: &str = "127.0.0.1:0";
const DEADLINE: Duration = Duration::from_secs(15);

async fn started(
    mode: OutputMode,
) -> (
    Arc<MediaCore>,
    SyntheticPhone,
    RingMediaPort<ManualClock>,
    ManualClock,
) {
    let core = Arc::new(MediaCore::new(LOOPBACK).await.expect("media core"));
    let phone = SyntheticPhone::new(LOOPBACK).await.expect("phone");
    let answer = core
        .answer(&phone.offer().await.expect("offer"))
        .await
        .expect("answer");
    phone.accept_answer(&answer).await.expect("accept");
    phone.connected().await.expect("connected");
    let clock = ManualClock::new(Timestamp::from_millis(1_000));
    let mut port = RingMediaPort::new(Arc::clone(&core), clock.clone()).with_output_mode(mode);
    port.start();
    (core, phone, port, clock)
}

#[tokio::test(flavor = "current_thread")]
async fn every_published_mode_is_delivered_at_its_own_size() {
    for mode in OutputMode::ALL {
        let (core, mut phone, mut port, _clock) = started(mode).await;
        phone.send_frame().await.expect("send");
        timeout(DEADLINE, core.frame_available())
            .await
            .expect("decoded");

        assert!(port.next_frame().is_some());
        let (width, height) = mode.dimensions();
        let frame = port.current_frame().expect("pixels");
        assert_eq!((frame.width(), frame.height()), (width, height));
        core.close().await.expect("close");
        phone.close().await.expect("phone close");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn the_last_frame_completes_the_rate_for_two_seconds_then_the_absence_is_signalled() {
    let (core, mut phone, mut port, clock) = started(OutputMode::Vga).await;
    phone.send_frame().await.expect("send");
    timeout(DEADLINE, core.frame_available())
        .await
        .expect("decoded");

    let first = port.next_frame().expect("fresh frame");
    clock.advance(33);
    let repeated = port.next_frame().expect("the last frame is repeated");
    assert_eq!(
        repeated.sequence,
        first.sequence + 1,
        "a repeat is still a delivered frame"
    );
    clock.advance(2_000);
    assert!(
        port.next_frame().is_none(),
        "frozen video is not presented as live"
    );
    assert!(
        port.current_frame().is_none(),
        "no stale picture is exposed"
    );

    phone.send_frame().await.expect("send again");
    timeout(DEADLINE, async {
        while core.ring().is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("decoded again");
    assert!(
        port.next_frame().is_some(),
        "video resumes with the next frame"
    );
    core.close().await.expect("close");
    phone.close().await.expect("phone close");
}
