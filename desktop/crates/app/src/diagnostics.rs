use std::net::IpAddr;

use remotecam_pairing::DiagnosticEntry;

use crate::logging::{LogRecord, LogViolation, Outcome, Severity};

const OPERATION: &str = "pairing.listener";

pub fn log_record(
    entry: &DiagnosticEntry,
    correlation_id: &str,
) -> Result<LogRecord, LogViolation> {
    let severity = if entry.is_security_event() {
        Severity::Error
    } else {
        Severity::Warning
    };
    let mut detail = format!("origin={}", masked_origin(entry.origin));
    if let Some(blocked) = entry.blocked_for_millis {
        detail.push_str(&format!(" blocked_for_ms={blocked}"));
    }
    LogRecord::now(severity, OPERATION, correlation_id, Outcome::Rejected)?
        .with_reason_code(entry.reason)
        .with_detail(&detail)
}

fn masked_origin(origin: Option<IpAddr>) -> String {
    match origin {
        None => "none".to_owned(),
        Some(IpAddr::V4(address)) => {
            let [a, b, c, _] = address.octets();
            format!("{a}.{b}.{c}.x")
        }
        Some(IpAddr::V6(address)) => {
            let [a, b, c, ..] = address.segments();
            format!("{a:x}:{b:x}:{c:x}:x")
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use remotecam_pairing::DiagnosticEntry;
    use remotecam_protocol::ReasonCode;

    use super::log_record;

    const CORRELATION: &str = "0123456789abcdef0123456789abcdef";

    fn entry(reason: ReasonCode, origin: Option<IpAddr>, blocked: Option<u64>) -> DiagnosticEntry {
        DiagnosticEntry {
            reason,
            origin,
            blocked_for_millis: blocked,
        }
    }

    #[test]
    fn a_throttled_origin_is_a_warning_with_a_masked_address() {
        let origin = IpAddr::V4(Ipv4Addr::new(192, 168, 0, 42));

        let record = log_record(
            &entry(ReasonCode::RateLimited, Some(origin), Some(60_000)),
            CORRELATION,
        )
        .expect("record");

        assert_eq!(record.severity, "warning");
        assert_eq!(record.operation, "pairing.listener");
        assert_eq!(record.outcome, "rejected");
        assert_eq!(record.reason_code, Some("rate_limited"));
        assert_eq!(
            record.detail.as_deref(),
            Some("origin=192.168.0.x blocked_for_ms=60000")
        );
    }

    #[test]
    fn a_security_event_is_an_error_and_may_have_no_origin() {
        let record = log_record(&entry(ReasonCode::ListenerClosed, None, None), CORRELATION)
            .expect("record");

        assert_eq!(record.severity, "error");
        assert_eq!(record.reason_code, Some("listener_closed"));
        assert_eq!(record.detail.as_deref(), Some("origin=none"));
    }

    #[test]
    fn an_ipv6_origin_keeps_only_its_first_three_groups() {
        let origin = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0xabcd, 1, 2, 3, 4, 5));

        let record = log_record(
            &entry(ReasonCode::CertificatePinMismatch, Some(origin), None),
            CORRELATION,
        )
        .expect("record");

        assert_eq!(record.detail.as_deref(), Some("origin=2001:db8:abcd:x"));
    }

    #[test]
    fn a_malformed_correlation_id_is_refused() {
        assert!(log_record(&entry(ReasonCode::RateLimited, None, None), "nope").is_err());
    }
}
