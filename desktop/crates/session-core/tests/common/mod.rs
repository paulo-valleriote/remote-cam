use remotecam_protocol::signaling::{Limits, SignalingMessage};

pub const PEER_FINGERPRINT: [u8; 32] = [0x55; 32];

/// The first `pair` frame a peer sends with `token` from the default peer fingerprint.
pub fn pair_frame(token: [u8; 32]) -> Vec<u8> {
    pair_frame_from_peer(token, PEER_FINGERPRINT)
}

pub fn pair_frame_from_peer(token: [u8; 32], dtls_fingerprint: [u8; 32]) -> Vec<u8> {
    SignalingMessage::Pair {
        token,
        dtls_fingerprint,
        limits: Limits::new(),
    }
    .encode()
    .expect("the pair message is valid")
}
