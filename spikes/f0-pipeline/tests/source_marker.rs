//! T074 — the synthetic frame carries a duplicated, correctly placed QR marker.

use f0_pipeline::synthetic_source::{
    MarkerLayout, SyntheticSourceConfig, encode_access_unit, marker_payload, render_frame,
};

#[test]
fn the_payload_is_the_contract_format() {
    assert_eq!(marker_payload(1_700_000_000_123, 77), "1700000000123|77");
}

#[test]
fn the_marker_is_drawn_twice_inside_the_frame() {
    let config = SyntheticSourceConfig::default();
    let rendered = render_frame(&config, 1_700_000_000_000, 5).expect("frame");
    let layout = rendered.layout;
    assert!(layout.top_left.0 > 0 && layout.top_left.1 > 0);
    assert!(
        layout.bottom_right.0 + layout.modules * layout.module_px + 2 * layout.quiet_zone
            <= config.width
    );
    assert!(
        layout.bottom_right.1 + layout.modules * layout.module_px + 2 * layout.quiet_zone
            <= config.height
    );

    let luma = rendered.frame.luma();
    let stride = rendered.frame.stride;
    let top = sample_region(luma, stride, layout.top_left, layout);
    let bottom = sample_region(luma, stride, layout.bottom_right, layout);
    assert!(
        top.iter().any(|value| *value < 0x80),
        "marker must have dark modules"
    );
    assert!(
        top.iter().any(|value| *value > 0x80),
        "marker must have light modules"
    );
    assert_eq!(top, bottom, "both copies must encode the same payload");
}

#[test]
fn a_frame_too_small_for_the_marker_is_rejected() {
    let config = SyntheticSourceConfig {
        width: 64,
        height: 64,
        ..SyntheticSourceConfig::default()
    };
    assert!(render_frame(&config, 0, 0).is_err());
}

#[test]
fn the_encoder_emits_an_annex_b_access_unit() {
    let config = SyntheticSourceConfig::default();
    let rendered = render_frame(&config, 1_700_000_000_000, 1).expect("frame");
    let unit = encode_access_unit(&rendered.frame).expect("encode");
    assert!(!unit.is_empty());
    assert!(
        unit.windows(4).any(|window| window == [0, 0, 0, 1]),
        "an Annex-B access unit starts NALs with 00 00 00 01"
    );
}

fn sample_region(luma: &[u8], stride: usize, origin: (u32, u32), layout: MarkerLayout) -> Vec<u8> {
    let mut values = Vec::new();
    let start_x = origin.0 + layout.quiet_zone;
    let start_y = origin.1 + layout.quiet_zone;
    let size = layout.modules * layout.module_px;
    for y in (start_y..start_y + size).step_by(layout.module_px as usize) {
        for x in (start_x..start_x + size).step_by(layout.module_px as usize) {
            values.push(luma[y as usize * stride + x as usize]);
        }
    }
    values
}
