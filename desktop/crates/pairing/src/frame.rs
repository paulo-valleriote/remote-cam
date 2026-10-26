use remotecam_protocol::{MAX_SIGNALING_MESSAGE_BYTES, ReasonCode};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use zeroize::Zeroizing;

use crate::error::PairingError;

const LENGTH_PREFIX_BYTES: usize = 4;

pub async fn read_frame(
    reader: &mut (impl AsyncRead + Unpin),
) -> Result<Zeroizing<Vec<u8>>, PairingError> {
    let mut prefix = [0_u8; LENGTH_PREFIX_BYTES];
    reader
        .read_exact(&mut prefix)
        .await
        .map_err(|_| PairingError::new(ReasonCode::InvalidMessage, "the peer ended mid-frame"))?;
    let length = u32::from_be_bytes(prefix) as usize;
    if length > MAX_SIGNALING_MESSAGE_BYTES {
        return Err(PairingError::new(
            ReasonCode::SecurityViolation,
            "the frame exceeds the signaling limit",
        ));
    }
    let mut frame = Zeroizing::new(vec![0_u8; length]);
    reader
        .read_exact(&mut frame[..])
        .await
        .map_err(|_| PairingError::new(ReasonCode::InvalidMessage, "the peer ended mid-frame"))?;
    Ok(frame)
}

pub async fn write_frame(
    writer: &mut (impl AsyncWrite + Unpin),
    frame: &[u8],
) -> Result<(), PairingError> {
    if frame.len() > MAX_SIGNALING_MESSAGE_BYTES {
        return Err(PairingError::new(
            ReasonCode::InvalidMessage,
            "the frame exceeds the signaling limit",
        ));
    }
    let mut wire = Vec::with_capacity(LENGTH_PREFIX_BYTES + frame.len());
    wire.extend_from_slice(&(frame.len() as u32).to_be_bytes());
    wire.extend_from_slice(frame);
    let failed = |_| PairingError::new(ReasonCode::InternalError, "the frame could not be written");
    writer.write_all(&wire).await.map_err(failed)?;
    writer.flush().await.map_err(failed)
}

#[cfg(test)]
mod tests {
    use super::{read_frame, write_frame};
    use remotecam_protocol::{MAX_SIGNALING_MESSAGE_BYTES, ReasonCode};
    use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};

    #[tokio::test]
    async fn a_written_frame_is_prefixed_with_its_big_endian_length() {
        let (mut left, mut right) = duplex(64 * 1024);

        write_frame(&mut left, b"{\"type\":\"close\"}")
            .await
            .expect("a small frame is written");
        drop(left);

        let mut raw = Vec::new();
        right.read_to_end(&mut raw).await.expect("raw bytes");
        assert_eq!(raw, b"\x00\x00\x00\x10{\"type\":\"close\"}");

        let (mut left, mut right) = duplex(64 * 1024);
        left.write_all(&raw).await.expect("send");
        let frame = read_frame(&mut right).await.expect("one frame");
        assert_eq!(&frame[..], b"{\"type\":\"close\"}");
    }

    #[tokio::test]
    async fn frames_are_read_one_at_a_time() {
        let (mut left, mut right) = duplex(1024);
        left.write_all(b"\x00\x00\x00\x03one\x00\x00\x00\x03two")
            .await
            .expect("send");

        assert_eq!(&read_frame(&mut right).await.expect("first")[..], b"one");
        assert_eq!(&read_frame(&mut right).await.expect("second")[..], b"two");
    }

    #[tokio::test]
    async fn an_empty_frame_is_a_zero_length_prefix() {
        let (mut left, mut right) = duplex(1024);
        write_frame(&mut left, b"").await.expect("empty is allowed");
        drop(left);

        let mut raw = Vec::new();
        right.read_to_end(&mut raw).await.expect("raw bytes");
        assert_eq!(raw, b"\x00\x00\x00\x00");
    }

    #[tokio::test]
    async fn a_frame_at_the_limit_is_accepted_and_one_byte_more_is_refused() {
        let at_limit = vec![b'a'; MAX_SIGNALING_MESSAGE_BYTES];
        let (mut left, mut right) = duplex(64 * 1024);
        write_frame(&mut left, &at_limit)
            .await
            .expect("the limit is allowed");
        assert_eq!(
            read_frame(&mut right).await.expect("at the limit").len(),
            MAX_SIGNALING_MESSAGE_BYTES
        );

        let over = vec![b'a'; MAX_SIGNALING_MESSAGE_BYTES + 1];
        let error = write_frame(&mut left, &over)
            .await
            .expect_err("over the limit is refused on write");
        assert_eq!(error.code(), ReasonCode::InvalidMessage);

        let length = u32::try_from(over.len()).expect("the test length fits");
        left.write_all(&length.to_be_bytes())
            .await
            .expect("raw prefix");
        let error = read_frame(&mut right)
            .await
            .expect_err("over the limit is refused on read");
        assert_eq!(error.code(), ReasonCode::SecurityViolation);
    }

    #[tokio::test]
    async fn a_truncated_frame_is_refused() {
        let (mut left, mut right) = duplex(1024);
        left.write_all(b"\x00\x00\x00\x0aabc").await.expect("send");
        drop(left);

        let error = read_frame(&mut right)
            .await
            .expect_err("the peer closed mid-frame");
        assert_eq!(error.code(), ReasonCode::InvalidMessage);
    }

    #[tokio::test]
    async fn a_closed_writer_is_reported_as_an_internal_error() {
        let (mut left, right) = duplex(1024);
        drop(right);

        let error = write_frame(&mut left, b"x")
            .await
            .expect_err("nobody reads");
        assert_eq!(error.code(), ReasonCode::InternalError);
    }
}
