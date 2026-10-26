use remotecam_protocol::{MAX_SIGNALING_MESSAGE_BYTES, ReasonCode};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use zeroize::Zeroizing;

use crate::error::PairingError;

const FRAME_TERMINATOR: u8 = b'\n';

/// Reads one newline-terminated signaling frame, without the terminator.
pub async fn read_frame(
    reader: &mut (impl AsyncRead + Unpin),
) -> Result<Zeroizing<Vec<u8>>, PairingError> {
    let mut frame = Zeroizing::new(Vec::new());
    loop {
        let mut byte = [0_u8; 1];
        reader.read_exact(&mut byte).await.map_err(|_| {
            PairingError::new(ReasonCode::InvalidMessage, "the peer ended mid-frame")
        })?;
        if byte[0] == FRAME_TERMINATOR {
            return Ok(frame);
        }
        if frame.len() == MAX_SIGNALING_MESSAGE_BYTES {
            return Err(PairingError::new(
                ReasonCode::InvalidMessage,
                "the frame exceeds the signaling limit",
            ));
        }
        frame.push(byte[0]);
    }
}

/// Writes `frame` followed by the newline terminator and flushes.
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
    let failed = |_| PairingError::new(ReasonCode::InternalError, "the frame could not be written");
    writer.write_all(frame).await.map_err(failed)?;
    writer
        .write_all(&[FRAME_TERMINATOR])
        .await
        .map_err(failed)?;
    writer.flush().await.map_err(failed)
}

#[cfg(test)]
mod tests {
    use super::{read_frame, write_frame};
    use remotecam_protocol::{MAX_SIGNALING_MESSAGE_BYTES, ReasonCode};
    use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};

    #[tokio::test]
    async fn a_written_frame_ends_with_one_newline_and_reads_back_without_it() {
        let (mut left, mut right) = duplex(64 * 1024);

        write_frame(&mut left, b"{\"type\":\"close\"}")
            .await
            .expect("a small frame is written");
        drop(left);

        let mut raw = Vec::new();
        right.read_to_end(&mut raw).await.expect("raw bytes");
        assert_eq!(raw, b"{\"type\":\"close\"}\n");

        let (mut left, mut right) = duplex(64 * 1024);
        left.write_all(&raw).await.expect("send");
        let frame = read_frame(&mut right).await.expect("one frame");
        assert_eq!(&frame[..], b"{\"type\":\"close\"}");
    }

    #[tokio::test]
    async fn frames_are_read_one_at_a_time() {
        let (mut left, mut right) = duplex(1024);
        left.write_all(b"one\ntwo\n").await.expect("send");

        assert_eq!(&read_frame(&mut right).await.expect("first")[..], b"one");
        assert_eq!(&read_frame(&mut right).await.expect("second")[..], b"two");
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

        left.write_all(&over).await.expect("raw send");
        left.write_all(b"\n").await.expect("raw send");
        let error = read_frame(&mut right)
            .await
            .expect_err("over the limit is refused on read");
        assert_eq!(error.code(), ReasonCode::InvalidMessage);
    }

    #[tokio::test]
    async fn a_truncated_frame_is_refused() {
        let (mut left, mut right) = duplex(1024);
        left.write_all(b"no terminator").await.expect("send");
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
