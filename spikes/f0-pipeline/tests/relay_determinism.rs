//! T065 — relay determinism, media-only impairment and realised loss.

use f0_pipeline::relay::{
    Action, ImpairmentConfig, LossDistribution, PROFILE_NAME, RelayCore, classify, parse_relay_log,
    relay_log_jsonl,
};

fn rtp_packet(sequence: u16, ssrc: u32) -> Vec<u8> {
    let mut packet = vec![0x80, 96, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    packet[2..4].copy_from_slice(&sequence.to_be_bytes());
    packet[8..12].copy_from_slice(&ssrc.to_be_bytes());
    packet.extend_from_slice(&[0xAA; 32]);
    packet
}

fn run(seed: u64, count: u16) -> Vec<f0_pipeline::relay::RelayDecision> {
    let config = ImpairmentConfig {
        seed,
        ..ImpairmentConfig::pinned(None, 0.0)
    };
    let mut core = RelayCore::new(config);
    (0..count)
        .filter_map(|sequence| {
            core.decide(sequence as f64 * 33.0, &rtp_packet(sequence, 0xDEAD_BEEF))
        })
        .collect()
}

#[test]
fn the_same_seed_produces_an_identical_decision_log() {
    let first = run(42, 500);
    let second = run(42, 500);
    assert_eq!(relay_log_jsonl(&first), relay_log_jsonl(&second));
}

#[test]
fn a_different_seed_produces_a_different_loss_pattern() {
    let first = run(42, 500);
    let second = run(7, 500);
    assert_ne!(relay_log_jsonl(&first), relay_log_jsonl(&second));
}

#[test]
fn control_traffic_is_never_impaired_or_logged() {
    let mut core = RelayCore::new(ImpairmentConfig::pinned(None, 0.0));
    // RTCP receiver report: version 2, packet type 201.
    let rtcp = [0x80, 201, 0, 1, 0, 0, 0, 0];
    // DTLS handshake record and a STUN binding request are not version 2 RTP.
    let dtls = [0x16, 0xFE, 0xFD, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let stun = [0x00, 0x01, 0x00, 0x08, 0x21, 0x12, 0xA4, 0x42, 0, 0, 0, 0];
    let rtp = rtp_packet(1, 1);

    assert!(core.decide(0.0, &rtcp).is_none());
    assert!(core.decide(1.0, &dtls).is_none());
    assert!(core.decide(2.0, &stun).is_none());
    assert!(core.decide(3.0, &rtp).is_some());

    assert_eq!(core.media_seen(), 1);
    assert!(matches!(
        classify(&rtcp),
        f0_pipeline::relay::Traffic::Control
    ));
}

#[test]
fn realised_loss_is_computed_from_the_log_not_the_configuration() {
    let decisions = run(42, 20_000);
    let dropped = decisions
        .iter()
        .filter(|decision| decision.action == Action::Drop)
        .count();
    let realised = 100.0 * dropped as f64 / decisions.len() as f64;
    assert!(
        (realised - 3.0).abs() < 0.5,
        "realised loss was {realised} % over {} packets",
        decisions.len()
    );

    let parsed = parse_relay_log(&relay_log_jsonl(&decisions));
    assert_eq!(parsed.len(), decisions.len());
}

#[test]
fn delay_stays_inside_the_pinned_band() {
    let config = ImpairmentConfig::pinned(None, 0.0);
    let decisions = run(42, 2_000);
    for decision in decisions {
        if decision.action == Action::Delay {
            assert!(
                (20.0..=40.0).contains(&decision.delay_ms),
                "delay {} ms is outside 30 ± 10 ms",
                decision.delay_ms
            );
        }
    }
    assert_eq!(config.added_latency_ms, 30.0);
    assert_eq!(config.jitter_ms, 10.0);
}

#[test]
fn a_stall_holds_then_releases_media() {
    let config = ImpairmentConfig {
        stall_at_ms: Some(1_000.0),
        stall_ms: 5_000.0,
        ..ImpairmentConfig::pinned(None, 0.0)
    };
    let mut core = RelayCore::new(config);
    assert_eq!(
        core.decide(1_100.0, &rtp_packet(1, 1)).map(|d| d.action),
        Some(Action::Hold)
    );
    assert_eq!(
        core.decide(2_000.0, &rtp_packet(2, 1)).map(|d| d.action),
        Some(Action::Hold)
    );
    let release = core.decide(6_500.0, &rtp_packet(3, 1)).expect("release");
    assert_eq!(release.action, Action::StallRelease);
    assert_eq!(release.delay_ms, 5_000.0);
}

#[test]
fn the_pinned_profile_is_named_and_a_deviation_is_labelled() {
    let pinned = ImpairmentConfig::pinned(None, 0.0);
    assert_eq!(PROFILE_NAME, "nominal-3pct-30ms-10ms");
    assert!(pinned.is_pinned_profile());
    assert_eq!(pinned.loss_distribution, LossDistribution::Uniform);

    let deviation = ImpairmentConfig {
        loss_distribution: LossDistribution::Burst,
        ..pinned
    };
    assert!(!deviation.is_pinned_profile());
    assert_eq!(
        deviation.profile_document("log")["deviation_label"],
        "profile-deviation"
    );
}
