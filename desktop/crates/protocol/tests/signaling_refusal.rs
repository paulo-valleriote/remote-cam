use remotecam_protocol::signaling::parse_signaling;
use remotecam_protocol::{ReasonCode, SignalingMessage};

const SESSION_HEADER: &str = "v=0\r\no=- 1 1 IN IP4 0.0.0.0\r\ns=-\r\nt=0 0\r\n";

fn frame(message_type: &str, sdp: &str) -> Vec<u8> {
    serde_json::json!({"version": 1, "type": message_type, "epoch": 7, "sdp": sdp})
        .to_string()
        .into_bytes()
}

fn offer(media: &str) -> Vec<u8> {
    frame("offer", &format!("{SESSION_HEADER}{media}"))
}

fn answer(media: &str) -> Vec<u8> {
    frame("answer", &format!("{SESSION_HEADER}{media}"))
}

fn refusal_code(frame: &[u8]) -> ReasonCode {
    parse_signaling(frame)
        .expect_err("the frame is outside the contract")
        .code
}

#[test]
fn an_offer_with_sendonly_audio_and_video_is_accepted() {
    let media = "m=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=sendonly\r\n\
                 m=video 9 UDP/TLS/RTP/SAVPF 96\r\na=sendonly\r\n";

    assert!(matches!(
        parse_signaling(&offer(media)),
        Ok(SignalingMessage::Offer { .. })
    ));
}

#[test]
fn an_offer_with_a_data_channel_is_refused() {
    let media = "m=video 9 UDP/TLS/RTP/SAVPF 96\r\na=sendonly\r\n\
                 m=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\n";

    assert_eq!(refusal_code(&offer(media)), ReasonCode::InvalidMessage);
}

#[test]
fn an_offer_with_sendrecv_is_refused() {
    let media = "m=video 9 UDP/TLS/RTP/SAVPF 96\r\na=sendrecv\r\n";

    assert_eq!(refusal_code(&offer(media)), ReasonCode::InvalidMessage);
}

#[test]
fn an_offer_section_without_a_direction_is_refused_because_sendrecv_is_the_default() {
    let media = "m=video 9 UDP/TLS/RTP/SAVPF 96\r\n";

    assert_eq!(refusal_code(&offer(media)), ReasonCode::InvalidMessage);
}

#[test]
fn an_offer_that_asks_the_desktop_to_send_is_refused() {
    let media = "m=video 9 UDP/TLS/RTP/SAVPF 96\r\na=recvonly\r\n";

    assert_eq!(refusal_code(&offer(media)), ReasonCode::InvalidMessage);
}

#[test]
fn a_session_level_direction_applies_to_sections_without_their_own() {
    let sendrecv_by_default = format!(
        "{SESSION_HEADER}a=sendrecv\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\na=sendonly\r\n\
         m=audio 9 UDP/TLS/RTP/SAVPF 111\r\n"
    );
    assert_eq!(
        refusal_code(&frame("offer", &sendrecv_by_default)),
        ReasonCode::InvalidMessage
    );

    let sendonly_by_default = format!(
        "{SESSION_HEADER}a=sendonly\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\n\
         m=audio 9 UDP/TLS/RTP/SAVPF 111\r\n"
    );
    assert!(parse_signaling(&frame("offer", &sendonly_by_default)).is_ok());
}

#[test]
fn a_section_with_two_directions_is_refused() {
    let media = "m=video 9 UDP/TLS/RTP/SAVPF 96\r\na=sendonly\r\na=sendrecv\r\n";

    assert_eq!(refusal_code(&offer(media)), ReasonCode::InvalidMessage);
}

#[test]
fn an_answer_that_is_recvonly_or_inactive_is_accepted() {
    let media = "m=video 9 UDP/TLS/RTP/SAVPF 96\r\na=recvonly\r\n\
                 m=audio 0 UDP/TLS/RTP/SAVPF 111\r\na=inactive\r\n";

    assert!(matches!(
        parse_signaling(&answer(media)),
        Ok(SignalingMessage::Answer { .. })
    ));
}

#[test]
fn an_answer_that_is_not_recvonly_is_refused() {
    for direction in ["sendrecv", "sendonly"] {
        let media = format!("m=video 9 UDP/TLS/RTP/SAVPF 96\r\na={direction}\r\n");
        assert_eq!(
            refusal_code(&answer(&media)),
            ReasonCode::InvalidMessage,
            "{direction}"
        );
    }
    let without_direction = "m=video 9 UDP/TLS/RTP/SAVPF 96\r\n";
    assert_eq!(
        refusal_code(&answer(without_direction)),
        ReasonCode::InvalidMessage
    );
}

#[test]
fn an_answer_with_a_data_channel_is_refused() {
    let media = "m=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\na=recvonly\r\n";

    assert_eq!(refusal_code(&answer(media)), ReasonCode::InvalidMessage);
}

fn candidate_line(address: &str, candidate_type: &str) -> String {
    format!("a=candidate:1 1 udp 2122260223 {address} 51234 typ {candidate_type} generation 0\r\n")
}

fn offer_with_candidate(line: &str) -> Vec<u8> {
    offer(&format!(
        "m=video 9 UDP/TLS/RTP/SAVPF 96\r\na=sendonly\r\n{line}"
    ))
}

#[test]
fn an_sdp_candidate_that_is_a_private_host_is_accepted() {
    for address in [
        "192.168.1.42",
        "10.0.0.7",
        "172.16.5.1",
        "fe80::1",
        "fd12::7",
    ] {
        let line = candidate_line(address, "host");
        assert!(
            parse_signaling(&offer_with_candidate(&line)).is_ok(),
            "{address}"
        );
    }
}

#[test]
fn an_sdp_candidate_that_is_not_host_is_refused() {
    for candidate_type in ["srflx", "prflx", "relay"] {
        let line = candidate_line("192.168.1.42", candidate_type);
        assert_eq!(
            refusal_code(&offer_with_candidate(&line)),
            ReasonCode::InvalidMessage,
            "{candidate_type}"
        );
    }
}

#[test]
fn an_sdp_candidate_on_a_public_or_apipa_address_is_refused() {
    for address in ["203.0.113.9", "8.8.8.8", "169.254.10.20", "2001:db8::1"] {
        let line = candidate_line(address, "host");
        assert_eq!(
            refusal_code(&offer_with_candidate(&line)),
            ReasonCode::InvalidMessage,
            "{address}"
        );
    }
}

#[test]
fn an_sdp_candidate_over_tcp_or_malformed_is_refused() {
    for line in [
        "a=candidate:1 1 tcp 2122260223 192.168.1.42 51234 typ host\r\n",
        "a=candidate:1 1 udp 2122260223 192.168.1.42 51234\r\n",
        "a=candidate:\r\n",
    ] {
        assert_eq!(
            refusal_code(&offer_with_candidate(line)),
            ReasonCode::InvalidMessage,
            "{line}"
        );
    }
}

fn ice(candidate: &str) -> Vec<u8> {
    format!(r#"{{"version":1,"type":"ice","epoch":7,"candidates":[{candidate}]}}"#).into_bytes()
}

#[test]
fn an_ice_message_accepts_a_private_host_and_refuses_everything_else() {
    assert!(
        parse_signaling(&ice(
            r#"{"address":"192.168.1.42","port":5000,"component":1}"#
        ))
        .is_ok()
    );

    for candidate in [
        r#"{"address":"169.254.10.20","port":5000,"component":1}"#,
        r#"{"address":"203.0.113.9","port":5000,"component":1}"#,
        r#"{"address":"192.168.1.42","port":5000,"component":1,"type":"srflx"}"#,
        r#"{"address":"192.168.1.42","port":5000,"component":1,"type":"relay"}"#,
    ] {
        assert_eq!(
            refusal_code(&ice(candidate)),
            ReasonCode::InvalidMessage,
            "{candidate}"
        );
    }
}

fn pair_with_token(token: &str) -> Vec<u8> {
    format!(
        r#"{{"version":1,"type":"pair","token":"{token}","dtls_fingerprint":"{}","limits":{{}}}}"#,
        "aa".repeat(32)
    )
    .into_bytes()
}

#[test]
fn uppercase_hex_is_refused() {
    assert!(parse_signaling(&pair_with_token(&"ab".repeat(32))).is_ok());
    assert_eq!(
        refusal_code(&pair_with_token(&"AB".repeat(32))),
        ReasonCode::InvalidMessage
    );
}

#[test]
fn a_version_above_one_is_refused_with_its_own_code() {
    assert_eq!(
        refusal_code(br#"{"version":2,"type":"ping"}"#),
        ReasonCode::UnsupportedVersion
    );
}

#[test]
fn an_unknown_field_is_refused_in_every_nesting_level() {
    for frame in [
        &br#"{"version":1,"type":"ping","extra":1}"#[..],
        &br#"{"version":1,"type":"offer","epoch":7,"sdp":"v=0","extra":1}"#[..],
        &br#"{"version":1,"type":"ice","epoch":7,"candidates":[{"address":"10.0.0.1","port":5000,"component":1,"extra":1}]}"#[..],
    ] {
        assert_eq!(refusal_code(frame), ReasonCode::InvalidMessage);
    }
}

#[test]
fn a_refusal_carries_a_reason_code_and_no_free_text() {
    let refusal = parse_signaling(b"{not json").expect_err("malformed JSON");

    assert_eq!(refusal.code.as_str(), "invalid_message");
    assert_eq!(
        format!("{refusal:?}"),
        "ParseError { code: InvalidMessage }"
    );
}

#[test]
fn an_sdp_that_hides_a_line_behind_a_bare_carriage_return_is_refused() {
    for hidden in [
        "a=rtcp-mux\ra=candidate:1 1 udp 1 203.0.113.9 9 typ srflx\r\n",
        "a=rtcp-mux\rm=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\n",
        "a=rtcp-mux\ra=sendrecv\r\n",
        "a=rtcp-mux\u{0}\r\n",
    ] {
        let media = format!("m=video 9 UDP/TLS/RTP/SAVPF 96\r\na=sendonly\r\n{hidden}");
        assert_eq!(
            refusal_code(&offer(&media)),
            ReasonCode::InvalidMessage,
            "{hidden:?}"
        );
    }
}

#[test]
fn an_sdp_with_only_line_feeds_is_still_accepted() {
    let sdp = "v=0\nm=video 9 UDP/TLS/RTP/SAVPF 96\na=sendonly\n";

    assert!(parse_signaling(&frame("offer", sdp)).is_ok());
}
