use std::time::Duration;

use remotecam_protocol::ReasonCode;
use remotecam_protocol::signaling::{CloseCode, SignalingMessage};
use remotecam_session_core::Effect;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::time::{Instant, timeout_at};

use crate::error::PairingError;
use crate::frame::write_frame;

/// How long the desktop waits to tell the replaced peer it was superseded.
pub const SUPERSEDE_BUDGET: Duration = Duration::from_millis(250);

/// Owns the channel of the peer that holds the active session.
#[derive(Debug)]
pub struct ActiveSession<C> {
    channel: Option<C>,
}

impl<C: AsyncRead + AsyncWrite + Unpin> ActiveSession<C> {
    pub const fn new() -> Self {
        Self { channel: None }
    }

    /// Whether a peer currently holds the session.
    pub const fn is_occupied(&self) -> bool {
        self.channel.is_some()
    }

    /// Performs the effects the Session Core emitted for an authenticated peer.
    ///
    /// The predecessor is told it was superseded and dropped before the successor
    /// is paired. Telling the predecessor is best effort within [`SUPERSEDE_BUDGET`]:
    /// a peer that is gone or not reading never holds the successor back.
    pub async fn apply(&mut self, effects: Vec<Effect>, successor: C) -> Result<(), PairingError> {
        let deadline = Instant::now() + SUPERSEDE_BUDGET;
        let mut successor = Some(successor);
        for effect in effects {
            match effect {
                Effect::SendClose { reason } => {
                    if let Some(channel) = self.channel.as_mut() {
                        let _ = timeout_at(deadline, send_close(channel, reason)).await;
                    }
                }
                Effect::End => {
                    if let Some(mut channel) = self.channel.take() {
                        let _ = timeout_at(deadline, channel.shutdown()).await;
                    }
                }
                Effect::SendPaired {
                    session_id,
                    epoch,
                    dtls_fingerprint,
                    accepted_limits,
                } => {
                    let mut channel = successor.take().ok_or_else(unowned)?;
                    let frame = SignalingMessage::Paired {
                        session_id: *session_id.as_bytes(),
                        epoch: u64::from(epoch),
                        dtls_fingerprint,
                        accepted_limits,
                    }
                    .encode()
                    .map_err(|_| unowned())?;
                    write_frame(&mut channel, &frame).await?;
                    self.channel = Some(channel);
                }
                _ => return Err(unowned()),
            }
        }
        Ok(())
    }
}

pub(crate) async fn send_close(
    channel: &mut (impl AsyncWrite + Unpin),
    code: CloseCode,
) -> Result<(), PairingError> {
    let frame = SignalingMessage::Close { code }
        .encode()
        .map_err(|_| unowned())?;
    write_frame(channel, &frame).await
}

fn unowned() -> PairingError {
    PairingError::new(
        ReasonCode::InternalError,
        "the effect is not part of replacing the active session",
    )
}

impl<C: AsyncRead + AsyncWrite + Unpin> Default for ActiveSession<C> {
    fn default() -> Self {
        Self::new()
    }
}
