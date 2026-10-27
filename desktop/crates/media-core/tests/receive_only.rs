#![cfg(not(windows))]

//! Receive-only Invariant at the Media Core boundary.
//!
//! ADR-0015 refuses `sendrecv`, direction-less offers and data channels before an offer
//! reaches the Media Core (protocol and Session Core). These tests are the second line of
//! defence: even a hostile offer that got through can never make the desktop send.

use remotecam_media_core::MediaCore;
use remotecam_media_core::testing::SyntheticPhone;

const LOOPBACK: &str = "127.0.0.1:0";

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

fn accepted(answer: &str) -> Vec<Section> {
    sections(answer)
        .into_iter()
        .filter(|section| section.port != 0)
        .collect()
}

fn assert_accepted_sections_are_recvonly(answer: &str) {
    for section in accepted(answer) {
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

async fn assert_no_sending_transceiver(core: &MediaCore) {
    assert!(!core.has_sending_transceiver().await);
}

async fn phone_offer() -> String {
    let phone = SyntheticPhone::new(LOOPBACK).await.expect("phone");
    let offer = phone.offer().await.expect("offer");
    phone.close().await.expect("phone close");
    assert!(offer.contains("a=sendonly\r\n"), "offer:\n{offer}");
    offer
}

async fn answer_to(offer: &str) -> (MediaCore, String) {
    let core = MediaCore::new(LOOPBACK).await.expect("media core");
    let answer = core.answer(offer).await.expect("answer");
    (core, answer)
}

#[tokio::test(flavor = "current_thread")]
async fn a_fresh_core_has_no_sending_transceiver() {
    let core = MediaCore::new(LOOPBACK).await.expect("media core");

    assert_no_sending_transceiver(&core).await;
    assert!(core.is_receive_only().await);
    core.close().await.expect("close");
}

#[tokio::test(flavor = "current_thread")]
async fn answering_a_sendonly_offer_stays_receive_only() {
    let (core, answer) = answer_to(&phone_offer().await).await;

    assert_eq!(accepted(&answer).len(), 1, "answer:\n{answer}");
    assert_accepted_sections_are_recvonly(&answer);
    assert_no_sending_transceiver(&core).await;
    assert!(core.is_receive_only().await);
    core.close().await.expect("close");
}

#[tokio::test(flavor = "current_thread")]
async fn a_closed_core_has_no_sending_transceiver() {
    let (core, _answer) = answer_to(&phone_offer().await).await;

    core.close().await.expect("close");

    assert_no_sending_transceiver(&core).await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_refused_offer_leaves_no_sending_transceiver() {
    let core = MediaCore::new(LOOPBACK).await.expect("media core");

    assert!(core.answer("not an sdp").await.is_err());

    assert_no_sending_transceiver(&core).await;
    core.close().await.expect("close");
}

#[tokio::test(flavor = "current_thread")]
async fn a_sendrecv_offer_is_still_answered_recvonly() {
    let offer = phone_offer().await.replace("a=sendonly", "a=sendrecv");

    let (core, answer) = answer_to(&offer).await;

    assert_eq!(accepted(&answer).len(), 1, "answer:\n{answer}");
    assert_accepted_sections_are_recvonly(&answer);
    assert_no_sending_transceiver(&core).await;
    core.close().await.expect("close");
}

#[tokio::test(flavor = "current_thread")]
async fn an_offer_without_direction_negotiates_no_media() {
    let offer = phone_offer().await.replace("a=sendonly\r\n", "");
    assert!(!offer.contains("a=sendonly"));

    let (core, answer) = answer_to(&offer).await;

    assert!(
        accepted(&answer).is_empty(),
        "no media section may be accepted:\n{answer}"
    );
    assert_no_sending_transceiver(&core).await;
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
    assert_no_sending_transceiver(&core).await;
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

    assert_eq!(accepted(&answer).len(), 1, "answer:\n{answer}");
    assert_accepted_sections_are_recvonly(&answer);
    assert_no_sending_transceiver(&core).await;
    core.close().await.expect("close");
}
