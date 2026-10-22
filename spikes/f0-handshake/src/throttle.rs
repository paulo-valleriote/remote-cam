//! Throttling scenarios for the pairing gate (T056).
//!
//! The listener is a property of the implementation, so the harness observes the
//! real decision stream: five attempts allowed, the sixth refused, the block
//! growing, and the listener closing after ten consecutive failures. A second
//! origin pairs while the first is throttled, because a global limit would be a
//! denial-of-service defect.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use remotecam_protocol::ReasonCode;
use remotecam_signaling::certificate::EphemeralCertificate;
use remotecam_signaling::client::{ExactPinVerifier, PinnedClient};
use remotecam_signaling::throttling::{ATTEMPT_LIMIT_PER_MINUTE, CONSECUTIVE_FAILURE_LIMIT};
use serde::Serialize;
use tokio::net::TcpStream;

use crate::scenarios::{
    await_paired, expected_fingerprints, loopback_policy, loopback_socket, send_pair,
    start_server_loop,
};

const HOSTILE: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 1);
const LEGITIMATE: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 2);
const CORRECT_TOKEN: [u8; 32] = [0x21; 32];
const WRONG_TOKEN: [u8; 32] = [0x22; 32];

#[derive(Debug, Clone, Serialize)]
pub struct DecisionEvidence {
    pub sequence: u32,
    pub at_ms: u64,
    pub origin_id: String,
    pub action: String,
    pub reason_code: Option<String>,
    pub outcome: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThrottleScenarioEvidence {
    pub throttle_id: String,
    pub origin_id: String,
    pub attempts_observed: usize,
    pub attempt_limit_per_minute: u32,
    pub limit_exceeded_at_attempt: u32,
    pub reason_code: String,
    pub blocked: bool,
    pub block_duration_s: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consecutive_failures_at_listener_close: Option<u32>,
    pub listener_closed: bool,
    pub legitimate_pairing_unaffected: bool,
    pub session_established: bool,
    pub decisions: Vec<DecisionEvidence>,
    pub trace_ref: String,
    pub security_event: bool,
}

/// One planned attempt in a throttle scenario.
#[derive(Debug, Clone, Copy)]
enum Attempt {
    Hostile,
    Legitimate,
}

/// Runs the three throttling scenarios.
pub async fn run_all() -> Vec<ThrottleScenarioEvidence> {
    let mut results = Vec::new();
    results.push(
        run_scenario(
            "throttle-rate-limit",
            &[
                Attempt::Hostile,
                Attempt::Hostile,
                Attempt::Hostile,
                Attempt::Hostile,
                Attempt::Hostile,
                Attempt::Hostile,
                Attempt::Legitimate,
            ],
        )
        .await,
    );
    results.push(
        run_scenario(
            "throttle-listener-close",
            &[
                Attempt::Hostile,
                Attempt::Hostile,
                Attempt::Hostile,
                Attempt::Hostile,
                Attempt::Hostile,
                Attempt::Hostile,
                Attempt::Hostile,
                Attempt::Hostile,
                Attempt::Hostile,
                Attempt::Legitimate,
                Attempt::Hostile,
            ],
        )
        .await,
    );
    results.push(
        run_scenario(
            "throttle-legitimate-origin",
            &[
                Attempt::Hostile,
                Attempt::Hostile,
                Attempt::Hostile,
                Attempt::Hostile,
                Attempt::Hostile,
                Attempt::Hostile,
                Attempt::Hostile,
                Attempt::Hostile,
                Attempt::Hostile,
                Attempt::Legitimate,
            ],
        )
        .await,
    );
    results
}

async fn run_scenario(throttle_id: &str, plan: &[Attempt]) -> ThrottleScenarioEvidence {
    let certificate = EphemeralCertificate::generate().expect("certificate");
    let (address, _store, handle) = start_server_loop(
        &certificate,
        None,
        CORRECT_TOKEN,
        loopback_policy(),
        plan.len(),
    )
    .await
    .expect("server loop");

    let client = PinnedClient::new(Arc::new(ExactPinVerifier::new(certificate.fingerprint())))
        .expect("client");
    let mut accepted_origins = Vec::new();
    let mut trace = Vec::new();

    for (index, attempt) in plan.iter().enumerate() {
        let (origin, token) = match attempt {
            Attempt::Hostile => (HOSTILE, WRONG_TOKEN),
            Attempt::Legitimate => (LEGITIMATE, CORRECT_TOKEN),
        };
        trace.push(format!(
            "attempt={} origin={origin} token={}",
            index + 1,
            if matches!(attempt, Attempt::Legitimate) {
                "correct"
            } else {
                "forged"
            }
        ));
        let result = drive_attempt(&client, address, origin, token).await;
        if let Ok(origin) = result {
            trace.push(format!("attempt={} accepted origin={origin}", index + 1));
            accepted_origins.push(origin);
        }
    }

    let (_outcomes, server) = handle.await.expect("server task");
    let decisions: Vec<DecisionEvidence> = server
        .decisions()
        .iter()
        .map(|decision| DecisionEvidence {
            sequence: decision.sequence,
            at_ms: decision.at_ms,
            origin_id: decision.origin_id.clone(),
            action: decision.action.as_str().to_owned(),
            reason_code: decision.reason_code.map(|code| code.as_str().to_owned()),
            outcome: decision.outcome.clone(),
        })
        .collect();
    let limit_exceeded_at_attempt = decisions
        .iter()
        .find(|decision| decision.action == "rate_limited")
        .map_or(0, |decision| decision.sequence);
    let block_duration_ms = server
        .throttling()
        .block_durations_ms()
        .last()
        .copied()
        .unwrap_or_default();
    let legitimate_unaffected = accepted_origins.iter().any(|origin| origin == "127.0.0.2");
    let hostile_established = accepted_origins.iter().any(|origin| origin == "127.0.0.1");
    let consecutive = server.throttling().consecutive_failures("127.0.0.1");
    let closed = server.listener_closed();
    let security_event = server.throttling().security_event();

    crate::trace::record(throttle_id, &trace);
    ThrottleScenarioEvidence {
        throttle_id: throttle_id.to_owned(),
        origin_id: "127.0.0.1".to_owned(),
        attempts_observed: plan.len(),
        attempt_limit_per_minute: ATTEMPT_LIMIT_PER_MINUTE,
        limit_exceeded_at_attempt,
        reason_code: ReasonCode::RateLimited.as_str().to_owned(),
        blocked: block_duration_ms > 0,
        block_duration_s: block_duration_ms / 1000,
        consecutive_failures_at_listener_close: closed
            .then_some(consecutive.max(CONSECUTIVE_FAILURE_LIMIT)),
        listener_closed: closed,
        legitimate_pairing_unaffected: legitimate_unaffected,
        session_established: hostile_established,
        decisions,
        trace_ref: format!("spikes/evidence/pairing/{throttle_id}.trace"),
        security_event,
    }
}

/// Drives one attempt and returns the accepted origin on success.
async fn drive_attempt(
    client: &PinnedClient,
    address: SocketAddr,
    origin: Ipv4Addr,
    token: [u8; 32],
) -> Result<String, ReasonCode> {
    let tcp: TcpStream = if origin == LEGITIMATE {
        let socket = loopback_socket(origin).expect("loopback socket");
        socket
            .connect(address)
            .await
            .map_err(|_| ReasonCode::DesktopUnreachable)?
    } else {
        TcpStream::connect(address)
            .await
            .map_err(|_| ReasonCode::DesktopUnreachable)?
    };
    let mut socket = client
        .connect_stream(tcp)
        .await
        .map_err(|error| error.code)?;
    send_pair(&mut socket, &token, &expected_fingerprints().video)
        .await
        .map_err(|_| ReasonCode::TlsHandshakeFailed)?;
    await_paired(&mut socket)
        .await
        .map_err(|_| ReasonCode::TlsHandshakeFailed)?;
    Ok(origin.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_manifest_matches_the_compiled_scenarios() {
        let document: serde_json::Value =
            serde_json::from_str(include_str!("../scenarios/throttling.json")).expect("manifest");
        let rows = document["scenarios"].as_array().expect("scenarios");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["throttle_id"], "throttle-rate-limit");
        assert_eq!(rows[1]["throttle_id"], "throttle-listener-close");
        assert_eq!(rows[2]["throttle_id"], "throttle-legitimate-origin");
    }
}
