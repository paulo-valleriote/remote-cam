#![cfg(not(windows))]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use remotecam_media_core::testing::{
    PICTURE_HEIGHT, PICTURE_WIDTH, SyntheticPhone, synthetic_picture,
};
use remotecam_media_core::{
    DecodeError, FRAME_RING_SLOTS, MediaCore, Nv12Frame, RingMediaPort, VideoDecoder,
};
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

struct ReleaseProbe {
    released: Arc<AtomicBool>,
}

impl VideoDecoder for ReleaseProbe {
    fn decode(&mut self, _: &[u8], _: u32) -> Result<Option<Nv12Frame>, DecodeError> {
        Ok(None)
    }
}

impl Drop for ReleaseProbe {
    fn drop(&mut self) {
        self.released.store(true, Ordering::SeqCst);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn closing_the_media_core_releases_the_decoder_and_the_ring() {
    let released = Arc::new(AtomicBool::new(false));
    let probe = ReleaseProbe {
        released: Arc::clone(&released),
    };
    let core = MediaCore::with_decoder(LOOPBACK, Box::new(probe))
        .await
        .expect("media core");
    assert!(!released.load(Ordering::SeqCst));

    core.close().await.expect("close");

    assert!(
        released.load(Ordering::SeqCst),
        "the decode thread is joined and drops its decoder"
    );
    assert!(core.ring().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_consumer_racing_the_decode_thread_sees_ordered_frames_and_consistent_counts() {
    let (core, mut phone) = negotiated().await;
    let sent = 12_u64;
    let consumer_core = Arc::clone(&core);
    let consumer = tokio::spawn(async move {
        let mut port = RingMediaPort::new(
            consumer_core.clone(),
            ManualClock::new(Timestamp::from_millis(0)),
        );
        port.start();
        let mut sequences = Vec::new();
        let mut rtp_timestamps = Vec::new();
        timeout(DEADLINE, async {
            while consumer_core.stats().decoded_frames() < sent || !consumer_core.ring().is_empty()
            {
                if let Some(media_frame) = port.next_frame() {
                    sequences.push(media_frame.sequence);
                    rtp_timestamps.push(port.current_frame().expect("pixels").rtp_timestamp());
                } else {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }
        })
        .await
        .expect("the consumer drains the ring");
        (sequences, rtp_timestamps)
    });
    for _ in 0..sent {
        phone.send_frame().await.expect("send");
    }
    let (sequences, rtp_timestamps) = consumer.await.expect("consumer");

    assert!(sequences.windows(2).all(|pair| pair[1] == pair[0] + 1));
    assert!(rtp_timestamps.windows(2).all(|pair| pair[1] > pair[0]));
    assert_eq!(
        sequences.len() as u64 + core.ring().dropped(),
        core.stats().decoded_frames()
    );
    core.close().await.expect("close");
    phone.close().await.expect("phone close");
}
