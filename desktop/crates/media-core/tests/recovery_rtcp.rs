#![cfg(not(windows))]

use remotecam_media_core::MediaCore;
use remotecam_media_core::testing::SyntheticPhone;

const LOOPBACK: &str = "127.0.0.1:0";

#[tokio::test(flavor = "current_thread")]
async fn the_answer_advertises_nack_and_pli_feedback_for_video() {
    let core = MediaCore::new(LOOPBACK).await.expect("media core");
    let phone = SyntheticPhone::new(LOOPBACK).await.expect("phone");
    let answer = core
        .answer(&phone.offer().await.expect("offer"))
        .await
        .expect("answer");
    assert!(
        answer.contains("a=rtcp-fb:") && answer.contains(" nack\r\n"),
        "{answer}"
    );
    assert!(answer.contains(" nack pli"), "{answer}");
    core.close().await.expect("close");
    phone.close().await.expect("close phone");
}
