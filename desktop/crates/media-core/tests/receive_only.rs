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

// ADR-0015 refuses these offers before they reach the Media Core (protocol and Session
// Core). These tests are the second line of defence: even a hostile offer that got through
// can never make the desktop send.

struct Section {
    kind: String,
    port: u16,
    attributes: String,
}

fn sections(sdp: &str) -> Vec<Section> {
    sdp.split("\nm=")
        .skip(1)
        .map(|text| {
            let mut fields = text.lines().next().unwrap_or_default().split(' ');
            Section {
                kind: fields.next().unwrap_or_default().to_owned(),
                port: fields
                    .next()
                    .and_then(|port| port.parse().ok())
                    .unwrap_or(u16::MAX),
                attributes: text.to_owned(),
            }
        })
        .collect()
}

fn assert_accepted_sections_are_recvonly(answer: &str) {
    for section in sections(answer).iter().filter(|section| section.port != 0) {
        assert_eq!(
            section.kind, "video",
            "only video may be accepted:\n{answer}"
        );
        assert!(
            section.attributes.contains("a=recvonly"),
            "answer:\n{answer}"
        );
    }
}

async fn answer_to(offer: &str) -> (MediaCore, String) {
    let core = MediaCore::new(LOOPBACK).await.expect("media core");
    let answer = core.answer(offer).await.expect("answer");
    (core, answer)
}

async fn phone_offer() -> String {
    let phone = SyntheticPhone::new(LOOPBACK).await.expect("phone");
    let offer = phone.offer().await.expect("offer");
    phone.close().await.expect("phone close");
    assert!(offer.contains("a=sendonly\r\n"), "offer:\n{offer}");
    offer
}

#[tokio::test(flavor = "current_thread")]
async fn a_sendrecv_offer_is_still_answered_recvonly() {
    let offer = phone_offer().await.replace("a=sendonly", "a=sendrecv");

    let (core, answer) = answer_to(&offer).await;

    assert_eq!(sections(&answer).len(), 1, "answer:\n{answer}");
    assert_accepted_sections_are_recvonly(&answer);
    assert!(answer.contains("a=recvonly"), "answer:\n{answer}");
    assert_eq!(core.send_transceiver_count().await, 0);
    core.close().await.expect("close");
}

#[tokio::test(flavor = "current_thread")]
async fn an_offer_without_direction_negotiates_no_media() {
    let offer = phone_offer().await.replace("a=sendonly\r\n", "");
    assert!(!offer.contains("a=sendonly"));

    let (core, answer) = answer_to(&offer).await;

    assert!(
        sections(&answer).iter().all(|section| section.port == 0),
        "no media section may be accepted:\n{answer}"
    );
    assert_eq!(core.send_transceiver_count().await, 0);
    core.close().await.expect("close");
}

#[tokio::test(flavor = "current_thread")]
async fn a_data_channel_section_is_rejected() {
    let mut offer = phone_offer().await;
    offer.push_str(
        "m=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\nc=IN IP4 0.0.0.0\r\na=mid:data\r\na=sctp-port:5000\r\n",
    );

    let (core, answer) = answer_to(&offer).await;

    let data = sections(&answer)
        .into_iter()
        .find(|section| section.kind == "application")
        .expect("the answer keeps the m-line slot, rejected");
    assert_eq!(data.port, 0, "answer:\n{answer}");
    assert_accepted_sections_are_recvonly(&answer);
    assert_eq!(core.send_transceiver_count().await, 0);
    core.close().await.expect("close");
}

#[tokio::test(flavor = "current_thread")]
async fn a_second_video_section_is_rejected_and_sends_nothing() {
    let offer = phone_offer().await;
    let start = offer.find("m=video").expect("video section");
    assert!(offer.contains("a=mid:0"));
    let doubled = format!(
        "{offer}{}",
        offer[start..].replace("a=mid:0", "a=mid:extra")
    );

    let (core, answer) = answer_to(&doubled).await;

    let accepted = sections(&answer)
        .iter()
        .filter(|section| section.port != 0)
        .count();
    assert_eq!(accepted, 1, "answer:\n{answer}");
    assert_accepted_sections_are_recvonly(&answer);
    assert_eq!(core.send_transceiver_count().await, 0);
    core.close().await.expect("close");
}

#[tokio::test(flavor = "current_thread")]
async fn a_refused_offer_leaves_no_sending_transceiver() {
    let core = MediaCore::new(LOOPBACK).await.expect("media core");

    assert!(core.answer("not an sdp").await.is_err());

    assert_eq!(core.send_transceiver_count().await, 0);
    core.close().await.expect("close");
}
