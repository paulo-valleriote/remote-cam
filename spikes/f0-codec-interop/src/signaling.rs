//! Signaling module for the desktop interop receiver.

use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::protocol::Message;
use futures::{SinkExt, StreamExt};
use webrtc::peer_connection::{PeerConnection, RTCIceCandidateInit, RTCSessionDescription};
use webrtc::error::Error;

use crate::metrics::InteropMetrics;

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(tag = "type")]
pub enum SignalingMessage {
    Offer { sdp: String },
    Answer { sdp: String },
    Ice { candidate: String, sdp_mid: String, sdp_mline_index: u16 },
    Close { reason: String },
    Error { code: String, message: String },
}

pub async fn signaling_server(
    addr: &str,
    peer_connection: Arc<dyn PeerConnection>,
) -> Result<(), Error> {
    let listener = TcpListener::bind(addr).await.map_err(Error::from_std)?;
    println!("Signaling server listening on {}", addr);

    loop {
        let (stream, _) = listener.accept().await.map_err(Error::from_std)?;
        let pc = peer_connection.clone();

        tokio::spawn(async move {
            let ws_stream = match accept_async(stream).await {
                Ok(ws) => ws,
                Err(e) => {
                    eprintln!("WebSocket handshake failed: {}", e);
                    return;
                }
            };

            let (mut ws_sender, mut ws_receiver) = ws_stream.split();

            while let Some(msg) = ws_receiver.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        if let Ok(sig_msg) = serde_json::from_str::<SignalingMessage>(&text) {
                            handle_signaling_message(pc.clone(), sig_msg, &mut ws_sender).await;
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Err(e) => {
                        eprintln!("WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
        });
    }
}

async fn handle_signaling_message(
    pc: Arc<dyn PeerConnection>,
    msg: SignalingMessage,
    ws_sender: &mut futures::stream::SplitSink<tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>, Message>,
) {
    match msg {
        SignalingMessage::Offer { sdp } => {
            println!("Received offer");
            let offer = match RTCSessionDescription::offer(sdp) {
                Ok(o) => o,
                Err(e) => {
                    eprintln!("Failed to create offer: {}", e);
                    return;
                }
            };

            if let Err(e) = pc.set_remote_description(offer).await {
                eprintln!("Failed to set remote offer: {}", e);
                return;
            }

            // Create answer
            let answer = match pc.create_answer(None).await {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("Failed to create answer: {}", e);
                    return;
                }
            };

            if let Err(e) = pc.set_local_description(answer.clone()).await {
                eprintln!("Failed to set local answer: {}", e);
                return;
            }

            // Send answer
            let response = SignalingMessage::Answer { sdp: answer.sdp };
            if let Ok(json) = serde_json::to_string(&response) {
                let _ = ws_sender.send(Message::text(json)).await;
            }

            InteropMetrics::log_answer_created();
        }
        SignalingMessage::Ice { candidate, sdp_mid, sdp_mline_index } => {
            let ice_candidate = RTCIceCandidateInit {
                candidate,
                sdp_mid: Some(sdp_mid),
                sdp_mline_index: Some(sdp_mline_index),
                ..Default::default()
            };
            if let Err(e) = pc.add_ice_candidate(ice_candidate).await {
                eprintln!("Failed to add ICE candidate: {}", e);
            }
        }
        SignalingMessage::Close { reason } => {
            println!("Received close: {}", reason);
            let _ = pc.close().await;
        }
        SignalingMessage::Error { code, message } => {
            eprintln!("Received error: {} - {}", code, message);
        }
        _ => {}
    }
}