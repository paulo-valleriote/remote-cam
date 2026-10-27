use opus::{Application, Channels, Encoder};
use remotecam_media_core::audio::{AudioError, FRAME_SAMPLES, OpusDecoder, SAMPLE_RATE_HZ};

fn tone(frame: usize) -> Vec<i16> {
    (0..FRAME_SAMPLES)
        .map(|index| {
            let phase = (frame * FRAME_SAMPLES + index) as f32 * 440.0 * std::f32::consts::TAU
                / SAMPLE_RATE_HZ as f32;
            (phase.sin() * 8_000.0) as i16
        })
        .collect()
}

fn encode(frame: usize) -> Vec<u8> {
    let mut encoder =
        Encoder::new(SAMPLE_RATE_HZ, Channels::Mono, Application::Voip).expect("encoder");
    let mut packet = vec![0_u8; 400];
    let length = encoder.encode(&tone(frame), &mut packet).expect("encode");
    packet.truncate(length);
    packet
}

fn energy(samples: &[i16]) -> i64 {
    samples.iter().map(|sample| i64::from(*sample).abs()).sum()
}

#[test]
fn decodes_a_20_ms_packet_to_960_signed_16_bit_samples() {
    let mut decoder = OpusDecoder::new().expect("decoder");
    let pcm = decoder.decode(&encode(0)).expect("decode");
    assert_eq!(pcm.len(), FRAME_SAMPLES);
    assert!(energy(&pcm) > 0, "a tone must not decode to silence");
}

#[test]
fn conceals_a_lost_packet_with_a_full_frame_after_audio() {
    let mut encoder =
        Encoder::new(SAMPLE_RATE_HZ, Channels::Mono, Application::Voip).expect("encoder");
    let mut decoder = OpusDecoder::new().expect("decoder");
    for frame in 0..5 {
        let mut packet = vec![0_u8; 400];
        let length = encoder.encode(&tone(frame), &mut packet).expect("encode");
        decoder.decode(&packet[..length]).expect("decode");
    }
    let concealed = decoder.conceal().expect("plc");
    assert_eq!(concealed.len(), FRAME_SAMPLES);
    assert!(
        energy(&concealed) > 0,
        "PLC extrapolates the previous audio"
    );
}

#[test]
fn conceals_before_any_packet_as_silence() {
    let mut decoder = OpusDecoder::new().expect("decoder");
    assert_eq!(decoder.conceal().expect("plc"), vec![0; FRAME_SAMPLES]);
}

#[test]
fn rejects_a_malformed_packet() {
    let mut decoder = OpusDecoder::new().expect("decoder");
    assert!(matches!(decoder.decode(&[]), Err(AudioError::EmptyPacket)));
    let error = decoder.decode(&[0xff, 0xff, 0xff]).expect_err("garbage");
    assert!(matches!(error, AudioError::Decoder(_)));
    assert!(error.to_string().contains("opus decoder error"));
}
