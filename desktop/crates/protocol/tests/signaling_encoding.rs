//! Emission seam: every validated message encodes to a frame the strict parser
//! accepts, and the canonical bytes are pinned independently of the encoder.

use remotecam_protocol::signaling::{
    Candidate, Candidates, CloseCode, EncodeError, ErrorCode, ErrorText, IceComponent,
    InvalidValue, Limits, Sdp, SignalingMessage, Stats, ThermalStatus, TransportProtocol,
    parse_signaling,
};

fn limits() -> Limits {
    Limits::new()
        .with_max_width(1280)
        .unwrap()
        .with_max_height(720)
        .unwrap()
        .with_max_frame_rate(30)
        .unwrap()
        .with_max_video_bitrate_kbps(2500)
        .unwrap()
        .with_max_audio_bitrate_kbps(48)
        .unwrap()
}

fn every_message() -> Vec<SignalingMessage> {
    vec![
        SignalingMessage::Pair {
            token: [0x11; 32],
            dtls_fingerprint: [0x22; 32],
            limits: limits(),
        },
        SignalingMessage::Paired {
            session_id: [0x33; 16],
            epoch: 7,
            dtls_fingerprint: [0x44; 32],
            accepted_limits: Limits::new().with_max_width(1920).unwrap(),
        },
        SignalingMessage::Offer {
            epoch: 7,
            sdp: Sdp::new("v=0\r\n").unwrap(),
        },
        SignalingMessage::Answer {
            epoch: 7,
            sdp: Sdp::new("v=0\r\n").unwrap(),
        },
        SignalingMessage::Ice {
            epoch: 7,
            candidates: Candidates::new(vec![
                Candidate::new(
                    "192.168.1.42",
                    51234,
                    IceComponent::Rtp,
                    Some(TransportProtocol::Udp),
                )
                .unwrap(),
                Candidate::new("fe80::1", 51235, IceComponent::Rtcp, None).unwrap(),
            ])
            .unwrap(),
        },
        SignalingMessage::Stats(
            Stats::new()
                .with_fps(30.0)
                .unwrap()
                .with_video_bitrate_kbps(2500.0)
                .unwrap()
                .with_rtt_ms(12.4)
                .unwrap()
                .with_loss_pct(0.0)
                .unwrap()
                .with_thermal_status(ThermalStatus::None),
        ),
        SignalingMessage::Close {
            code: CloseCode::EndedByMobile,
        },
        SignalingMessage::Error {
            code: ErrorCode::InvalidMessage,
            message: Some(ErrorText::new("malformed frame").unwrap()),
        },
        SignalingMessage::Ping,
        SignalingMessage::Pong,
    ]
}

#[test]
fn every_message_encodes_to_a_frame_the_strict_parser_accepts() {
    for message in every_message() {
        let frame = message.encode().expect("a valid message must encode");
        assert_eq!(
            parse_signaling(&frame).expect("the encoder must emit a frame the parser accepts"),
            message
        );
    }
}

#[test]
fn the_canonical_frame_pins_the_key_order() {
    let expected = [
        r#"{"version":1,"type":"pair","token":"1111111111111111111111111111111111111111111111111111111111111111","dtls_fingerprint":"2222222222222222222222222222222222222222222222222222222222222222","limits":{"max_width":1280,"max_height":720,"max_frame_rate":30,"max_video_bitrate_kbps":2500,"max_audio_bitrate_kbps":48}}"#,
        r#"{"version":1,"type":"paired","session_id":"33333333333333333333333333333333","epoch":7,"dtls_fingerprint":"4444444444444444444444444444444444444444444444444444444444444444","accepted_limits":{"max_width":1920}}"#,
        r#"{"version":1,"type":"offer","epoch":7,"sdp":"v=0\r\n"}"#,
        r#"{"version":1,"type":"answer","epoch":7,"sdp":"v=0\r\n"}"#,
        r#"{"version":1,"type":"ice","epoch":7,"candidates":[{"address":"192.168.1.42","port":51234,"component":1,"protocol":"udp"},{"address":"fe80::1","port":51235,"component":2}]}"#,
        r#"{"version":1,"type":"stats","fps":30.0,"video_bitrate_kbps":2500.0,"rtt_ms":12.4,"loss_pct":0.0,"thermal_status":"none"}"#,
        r#"{"version":1,"type":"close","code":"ended_by_mobile"}"#,
        r#"{"version":1,"type":"error","code":"invalid_message","message":"malformed frame"}"#,
        r#"{"version":1,"type":"ping"}"#,
        r#"{"version":1,"type":"pong"}"#,
    ];

    for (message, expected) in every_message().into_iter().zip(expected) {
        assert_eq!(
            message.encode().expect("a valid message must encode"),
            expected.as_bytes(),
            "unexpected canonical frame for {message:?}"
        );
    }
}

#[test]
fn absent_optional_fields_are_omitted() {
    let empty_limits = SignalingMessage::Pair {
        token: [0; 32],
        dtls_fingerprint: [0; 32],
        limits: Limits::new(),
    };
    assert_eq!(
        empty_limits.encode().unwrap(),
        br#"{"version":1,"type":"pair","token":"0000000000000000000000000000000000000000000000000000000000000000","dtls_fingerprint":"0000000000000000000000000000000000000000000000000000000000000000","limits":{}}"#
    );

    assert_eq!(
        SignalingMessage::Stats(Stats::new()).encode().unwrap(),
        br#"{"version":1,"type":"stats"}"#
    );

    assert_eq!(
        SignalingMessage::Error {
            code: ErrorCode::InternalError,
            message: None,
        }
        .encode()
        .unwrap(),
        br#"{"version":1,"type":"error","code":"internal_error"}"#
    );

    let candidate_without_protocol = Candidates::new(vec![
        Candidate::new("10.0.0.7", 5000, IceComponent::Rtp, None).unwrap(),
    ])
    .unwrap();
    assert_eq!(
        SignalingMessage::Ice {
            epoch: 0,
            candidates: candidate_without_protocol,
        }
        .encode()
        .unwrap(),
        br#"{"version":1,"type":"ice","epoch":0,"candidates":[{"address":"10.0.0.7","port":5000,"component":1}]}"#
    );
}

#[test]
fn limits_refuse_values_outside_the_declared_ranges() {
    assert_eq!(Limits::new().with_max_width(159), Err(InvalidValue));
    assert_eq!(Limits::new().with_max_width(3841), Err(InvalidValue));
    assert_eq!(Limits::new().with_max_height(119), Err(InvalidValue));
    assert_eq!(Limits::new().with_max_height(2161), Err(InvalidValue));
    assert_eq!(Limits::new().with_max_frame_rate(0), Err(InvalidValue));
    assert_eq!(Limits::new().with_max_frame_rate(61), Err(InvalidValue));
    assert_eq!(
        Limits::new().with_max_video_bitrate_kbps(199),
        Err(InvalidValue)
    );
    assert_eq!(
        Limits::new().with_max_video_bitrate_kbps(8001),
        Err(InvalidValue)
    );
    assert_eq!(
        Limits::new().with_max_audio_bitrate_kbps(7),
        Err(InvalidValue)
    );
    assert_eq!(
        Limits::new().with_max_audio_bitrate_kbps(129),
        Err(InvalidValue)
    );

    assert!(
        Limits::new()
            .with_max_width(160)
            .unwrap()
            .with_max_height(120)
            .unwrap()
            .with_max_frame_rate(1)
            .unwrap()
            .with_max_video_bitrate_kbps(200)
            .unwrap()
            .with_max_audio_bitrate_kbps(8)
            .is_ok()
    );
    assert!(
        Limits::new()
            .with_max_width(3840)
            .unwrap()
            .with_max_height(2160)
            .unwrap()
            .with_max_frame_rate(60)
            .unwrap()
            .with_max_video_bitrate_kbps(8000)
            .unwrap()
            .with_max_audio_bitrate_kbps(128)
            .is_ok()
    );
}

#[test]
fn sdp_and_error_text_are_bounded_in_characters() {
    assert_eq!(Sdp::new(""), Err(InvalidValue));
    assert!(Sdp::new(&"a".repeat(8192)).is_ok());
    assert_eq!(Sdp::new(&"a".repeat(8193)), Err(InvalidValue));
    assert!(Sdp::new(&"é".repeat(8192)).is_ok());

    assert!(ErrorText::new("").is_ok());
    assert!(ErrorText::new(&"x".repeat(200)).is_ok());
    assert_eq!(ErrorText::new(&"x".repeat(201)), Err(InvalidValue));
}

#[test]
fn candidates_are_bounded_and_private() {
    assert!(Candidate::new("192.168.1.42", 1, IceComponent::Rtp, None).is_ok());
    assert!(Candidate::new("192.168.1.42", 65535, IceComponent::Rtcp, None).is_ok());
    assert_eq!(
        Candidate::new("192.168.1.42", 0, IceComponent::Rtp, None),
        Err(InvalidValue)
    );
    assert_eq!(
        Candidate::new("8.8.8.8", 5000, IceComponent::Rtp, None),
        Err(InvalidValue)
    );

    let candidate = |port| Candidate::new("10.0.0.1", port, IceComponent::Rtp, None).unwrap();
    let sixty_four: Vec<Candidate> = (0..64).map(|index| candidate(5000 + index)).collect();
    assert!(Candidates::new(sixty_four).is_ok());
    let sixty_five: Vec<Candidate> = (0..65).map(|index| candidate(5000 + index)).collect();
    assert_eq!(Candidates::new(sixty_five), Err(InvalidValue));
}

#[test]
fn stats_refuse_non_finite_and_out_of_range_values() {
    assert_eq!(Stats::new().with_fps(-1.0), Err(InvalidValue));
    assert_eq!(Stats::new().with_fps(120.1), Err(InvalidValue));
    assert_eq!(Stats::new().with_fps(f64::NAN), Err(InvalidValue));
    assert!(Stats::new().with_fps(0.0).is_ok());
    assert!(Stats::new().with_fps(120.0).is_ok());

    assert_eq!(Stats::new().with_rtt_ms(-0.1), Err(InvalidValue));
    assert_eq!(Stats::new().with_rtt_ms(f64::INFINITY), Err(InvalidValue));
    assert_eq!(
        Stats::new().with_video_bitrate_kbps(f64::NAN),
        Err(InvalidValue)
    );
    assert_eq!(Stats::new().with_loss_pct(100.1), Err(InvalidValue));
    assert!(Stats::new().with_loss_pct(100.0).is_ok());
}

#[test]
fn close_and_error_codes_are_closed_and_disjoint() {
    for code in [
        "ended_by_mobile",
        "ended_by_desktop",
        "superseded",
        "timeout",
        "media_failure",
        "security_violation",
        "internal_error",
    ] {
        assert_eq!(CloseCode::parse(code).unwrap().as_str(), code);
    }
    for code in [
        "invalid_message",
        "unsupported_version",
        "rate_limited",
        "internal_error",
    ] {
        assert_eq!(ErrorCode::parse(code).unwrap().as_str(), code);
    }
    assert_eq!(CloseCode::parse("rate_limited"), None);
    assert_eq!(ErrorCode::parse("timeout"), None);
    assert_eq!(CloseCode::parse("bye"), None);
}

#[test]
fn a_frame_above_the_size_cap_is_refused() {
    let escaping_sdp = Sdp::new(&"\"".repeat(8192)).unwrap();
    assert_eq!(
        SignalingMessage::Offer {
            epoch: 0,
            sdp: escaping_sdp,
        }
        .encode(),
        Err(EncodeError::FrameTooLarge)
    );

    let fitting_sdp = Sdp::new(&"a".repeat(8192)).unwrap();
    let frame = SignalingMessage::Answer {
        epoch: 0,
        sdp: fitting_sdp,
    }
    .encode()
    .expect("an 8192-character SDP fits the frame");
    assert!(frame.len() < 16 * 1024);
}
