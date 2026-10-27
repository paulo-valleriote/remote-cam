#![cfg(not(windows))]

use remotecam_media_core::MediaCore;
use remotecam_media_core::testing::SyntheticPhone;

const LOOPBACK: &str = "127.0.0.1:0";

#[tokio::test(flavor = "current_thread")]
async fn a_fresh_core_has_no_sending_transceiver() {
    let core = MediaCore::new(LOOPBACK).await.expect("media core");
    assert_eq!(core.send_transceiver_count().await, 0);
    core.close().await.expect("close");
}

#[tokio::test(flavor = "current_thread")]
async fn answering_a_sendonly_offer_adds_no_sending_transceiver() {
    let core = MediaCore::new(LOOPBACK).await.expect("media core");
    let phone = SyntheticPhone::new(LOOPBACK).await.expect("phone");
    let offer = phone.offer().await.expect("offer");

    let answer = core.answer(&offer).await.expect("answer");

    assert!(answer.contains("a=recvonly"), "answer:\n{answer}");
    assert!(!answer.contains("a=sendrecv"), "answer:\n{answer}");
    assert!(!answer.contains("a=sendonly"), "answer:\n{answer}");
    assert_eq!(core.send_transceiver_count().await, 0);
    assert!(core.is_receive_only().await);
    core.close().await.expect("close");
    phone.close().await.expect("phone close");
}

#[tokio::test(flavor = "current_thread")]
async fn a_closed_core_has_no_sending_transceiver() {
    let core = MediaCore::new(LOOPBACK).await.expect("media core");
    let phone = SyntheticPhone::new(LOOPBACK).await.expect("phone");
    let offer = phone.offer().await.expect("offer");
    core.answer(&offer).await.expect("answer");

    core.close().await.expect("close");

    assert_eq!(core.send_transceiver_count().await, 0);
    phone.close().await.expect("phone close");
}

async fn answer_to(
    offer: &str,
) -> (
    MediaCore,
    Result<String, remotecam_media_core::MediaCoreError>,
) {
    let core = MediaCore::new(LOOPBACK).await.expect("media core");
    let answer = core.answer(offer).await;
    (core, answer)
}

async fn phone_offer() -> String {
    let phone = SyntheticPhone::new(LOOPBACK).await.expect("phone");
    let offer = phone.offer().await.expect("offer");
    phone.close().await.expect("phone close");
    offer
}

#[tokio::test(flavor = "current_thread")]
async fn a_sendrecv_offer_is_still_answered_recvonly() {
    let offer = phone_offer().await.replace("a=sendonly", "a=sendrecv");

    let (core, answer) = answer_to(&offer).await;

    let answer = answer.expect("answer");
    assert!(answer.contains("a=recvonly"), "answer:\n{answer}");
    assert!(!answer.contains("a=sendrecv"), "answer:\n{answer}");
    assert_eq!(core.send_transceiver_count().await, 0);
    core.close().await.expect("close");
}

#[tokio::test(flavor = "current_thread")]
async fn an_offer_without_direction_never_yields_a_sending_answer() {
    let offer = phone_offer().await.replace("a=sendonly\r\n", "");
    assert!(!offer.contains("a=sendonly"));

    let (core, answer) = answer_to(&offer).await;

    let answer = answer.expect("answer");
    assert!(!answer.contains("a=sendrecv"), "answer:\n{answer}");
    assert!(!answer.contains("a=sendonly"), "answer:\n{answer}");
    assert_eq!(core.send_transceiver_count().await, 0);
    core.close().await.expect("close");
}

#[tokio::test(flavor = "current_thread")]
async fn a_data_channel_section_is_never_accepted() {
    let mut offer = phone_offer().await;
    offer.push_str(
        "m=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\nc=IN IP4 0.0.0.0\r\na=mid:data\r\na=sctp-port:5000\r\n",
    );

    let (core, answer) = answer_to(&offer).await;

    if let Ok(answer) = answer {
        let section = answer
            .split("m=application")
            .nth(1)
            .map(|rest| rest.lines().next().unwrap_or_default().to_owned());
        if let Some(line) = section {
            assert!(
                line.contains(" 0 "),
                "data channel must be rejected: m=application{line}"
            );
        }
    }
    assert_eq!(core.send_transceiver_count().await, 0);
    core.close().await.expect("close");
}

#[tokio::test(flavor = "current_thread")]
async fn a_second_video_section_adds_no_sending_transceiver() {
    let offer = phone_offer().await;
    let start = offer.find("m=video").expect("video section");
    let section = offer[start..].to_owned();
    let doubled = format!(
        "{offer}{}",
        section
            .replace("a=mid:0", "a=mid:extra")
            .replace("a=msid:", "a=msid:x")
    );

    let (core, answer) = answer_to(&doubled).await;

    if let Ok(answer) = answer {
        assert!(!answer.contains("a=sendrecv"), "answer:\n{answer}");
        assert!(!answer.contains("a=sendonly"), "answer:\n{answer}");
    }
    assert_eq!(core.send_transceiver_count().await, 0);
    core.close().await.expect("close");
}
