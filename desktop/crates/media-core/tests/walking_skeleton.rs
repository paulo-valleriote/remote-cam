#![cfg(not(windows))]

//! REC-50: synthetic H.264 RTP arrives as a real NV12 `MediaFrame` through the
//! Media Port, over a loopback WebRTC connection.

use std::sync::Arc;
use std::time::Duration;

use remotecam_media_core::testing::{
    PICTURE_HEIGHT, PICTURE_WIDTH, SyntheticPhone, synthetic_picture,
};
use remotecam_media_core::{FRAME_RING_SLOTS, MediaCore, RingMediaPort};
use remotecam_session_core::testing::ManualClock;
use remotecam_session_core::{MediaPort, Timestamp};
use tokio::time::timeout;

const LOOPBACK: &str = "127.0.0.1:0";
const DEADLINE: Duration = Duration::from_secs(15);

async fn negotiated() -> (Arc<MediaCore>, SyntheticPhone) {
    let core = Arc::new(MediaCore::new(LOOPBACK).await.expect("media core"));
    let phone = SyntheticPhone::new(LOOPBACK).await.expect("phone");
    let offer = phone.offer().await.expect("offer");
    assert!(
        offer.contains("a=sendonly"),
        "the phone offers sendonly:\n{offer}"
    );
    let answer = core.answer(&offer).await.expect("answer");
    assert!(
        answer.contains("a=recvonly"),
        "the desktop answers recvonly:\n{answer}"
    );
    phone.accept_answer(&answer).await.expect("accept answer");
    phone.connected().await.expect("connected");
    (core, phone)
}

#[tokio::test(flavor = "current_thread")]
async fn the_first_synthetic_frame_reaches_the_media_port_as_nv12() {
    let (core, mut phone) = negotiated().await;
    assert!(core.is_receive_only().await);

    let clock = ManualClock::new(Timestamp::from_millis(100));
    let mut port = RingMediaPort::new(Arc::clone(&core), clock.clone());
    assert!(
        port.next_frame().is_none(),
        "a stopped port delivers nothing"
    );
    port.start();

    phone.send_frame().await.expect("send");
    timeout(DEADLINE, core.frame_available())
        .await
        .expect("a decoded frame arrives");

    let media_frame = port.next_frame().expect("frame");
    assert_eq!(media_frame.sequence, 0);
    assert_eq!(media_frame.timestamp, Timestamp::from_millis(100));

    let frame = port.current_frame().expect("pixels");
    assert_eq!(
        (frame.width() as usize, frame.height() as usize),
        (PICTURE_WIDTH, PICTURE_HEIGHT)
    );
    assert_eq!(frame.data().len(), PICTURE_WIDTH * PICTURE_HEIGHT * 3 / 2);
    let expected = synthetic_picture(0);
    let error: u64 = frame
        .luma()
        .iter()
        .zip(&expected)
        .map(|(a, b)| u64::from(a.abs_diff(*b)))
        .sum();
    assert!(
        error / (frame.luma().len() as u64) < 12,
        "decoded luma follows the encoded ramp"
    );

    port.stop();
    assert!(port.current_frame().is_none());
    core.close().await.expect("close");
    phone.close().await.expect("phone close");
    assert!(core.stats().packets() > 0);
    assert_eq!(core.stats().decode_errors(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn a_lagging_consumer_loses_the_oldest_frames_not_the_stream() {
    let (core, mut phone) = negotiated().await;
    let sent = FRAME_RING_SLOTS as u64 + 4;
    for _ in 0..sent {
        phone.send_frame().await.expect("send");
    }
    timeout(DEADLINE, async {
        while core.stats().decoded_frames() < sent {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("every frame is decoded");

    assert_eq!(core.ring().len(), FRAME_RING_SLOTS);
    assert_eq!(core.ring().dropped(), sent - FRAME_RING_SLOTS as u64);
    core.close().await.expect("close");
    phone.close().await.expect("phone close");
}
