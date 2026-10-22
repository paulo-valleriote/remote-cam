//! Structured logging shared by the desktop and mobile applications.

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use remotecam_protocol::ReasonCode;
use serde::Serialize;

/// Log severity. The wire spelling is lowercase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Debug,
    Info,
    Warning,
    Error,
}

impl Severity {
    /// Stable wire spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// Operation outcome, distinct from the severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Started,
    Success,
    Rejected,
    Failed,
    Stopped,
}

impl Outcome {
    /// Stable wire spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Success => "success",
            Self::Rejected => "rejected",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
        }
    }
}

/// One structured log record. Additional fields are deliberately impossible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogRecord {
    pub timestamp: String,
    pub severity: &'static str,
    pub operation: String,
    pub correlation_id: String,
    pub outcome: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Reason a record cannot be emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogViolation {
    /// The operation is not a stable lowercase identifier.
    InvalidOperation,
    /// The correlation id is not 32 lowercase hex characters.
    InvalidCorrelationId,
    /// The detail still contains a secret or an absolute path.
    UnsafeDetail,
}

impl LogRecord {
    /// Builds a record with the current UTC timestamp.
    pub fn now(
        severity: Severity,
        operation: impl Into<String>,
        correlation_id: impl Into<String>,
        outcome: Outcome,
    ) -> Result<Self, LogViolation> {
        Self::at(
            format_rfc3339(SystemTime::now()),
            severity,
            operation,
            correlation_id,
            outcome,
        )
    }

    /// Builds a record with an explicit timestamp, for deterministic tests.
    pub fn at(
        timestamp: impl Into<String>,
        severity: Severity,
        operation: impl Into<String>,
        correlation_id: impl Into<String>,
        outcome: Outcome,
    ) -> Result<Self, LogViolation> {
        let operation = operation.into();
        let correlation_id = correlation_id.into();
        validate_operation(&operation)?;
        validate_correlation_id(&correlation_id)?;
        Ok(Self {
            timestamp: timestamp.into(),
            severity: severity.as_str(),
            operation,
            correlation_id,
            outcome: outcome.as_str(),
            reason_code: None,
            detail: None,
        })
    }

    /// Attaches a closed reason code.
    pub fn with_reason_code(mut self, code: ReasonCode) -> Self {
        self.reason_code = Some(code.as_str());
        self
    }

    /// Attaches a detail after redaction. Rejects a detail that still looks
    /// like a secret or an absolute path.
    pub fn with_detail(mut self, detail: &str) -> Result<Self, LogViolation> {
        let redacted = redact(detail);
        if contains_unsafe_material(&redacted) {
            return Err(LogViolation::UnsafeDetail);
        }
        self.detail = Some(truncate_chars(&redacted, 200));
        Ok(self)
    }

    /// Serializes the record as one JSON line.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a log record is always serializable")
    }
}

/// Writes one JSON line and flushes it so a crash cannot lose the last record.
pub fn write_line(writer: &mut impl Write, record: &LogRecord) -> std::io::Result<()> {
    writer.write_all(record.to_json().as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}

/// Replaces token- and fingerprint-shaped runs and absolute user paths.
pub fn redact(input: &str) -> String {
    let sanitized = redact_paths(&redact_hex_runs(input));
    sanitized
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

fn redact_hex_runs(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut run_start: Option<usize> = None;
    for (index, character) in input.char_indices() {
        if character.is_ascii_hexdigit() {
            if run_start.is_none() {
                run_start = Some(index);
            }
        } else if let Some(start) = run_start.take() {
            push_redacted_run(&mut output, &input[start..index]);
            output.push(character);
        } else {
            output.push(character);
        }
    }
    if let Some(start) = run_start {
        push_redacted_run(&mut output, &input[start..]);
    }
    output
}

fn push_redacted_run(output: &mut String, run: &str) {
    if run.len() >= 32 {
        output.push_str("[redacted]");
    } else {
        output.push_str(run);
    }
}

fn redact_paths(input: &str) -> String {
    const PREFIXES: [&str; 4] = ["/home/", "/Users/", "C:\\Users\\", "c:\\Users\\"];
    let mut result = String::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        let rest = &input[index..];
        if PREFIXES.iter().any(|prefix| rest.starts_with(prefix)) {
            result.push_str("<path>");
            let path_length = rest.find(char::is_whitespace).unwrap_or(rest.len());
            index += path_length;
        } else {
            let character = rest.chars().next().expect("non-empty remainder");
            result.push(character);
            index += character.len_utf8();
        }
    }
    result
}

fn contains_unsafe_material(input: &str) -> bool {
    let mut hex_run = 0_usize;
    for character in input.chars() {
        if character.is_ascii_hexdigit() {
            hex_run += 1;
            if hex_run >= 32 {
                return true;
            }
        } else {
            hex_run = 0;
        }
    }
    ["/home/", "/Users/", "C:\\Users\\", "c:\\Users\\"]
        .iter()
        .any(|prefix| input.contains(prefix))
}

fn validate_operation(operation: &str) -> Result<(), LogViolation> {
    let mut characters = operation.chars();
    let Some(first) = characters.next() else {
        return Err(LogViolation::InvalidOperation);
    };
    if !first.is_ascii_lowercase() {
        return Err(LogViolation::InvalidOperation);
    }
    if operation.len() > 64
        || !characters.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '_'
                || character == '.'
        })
    {
        return Err(LogViolation::InvalidOperation);
    }
    Ok(())
}

fn validate_correlation_id(correlation_id: &str) -> Result<(), LogViolation> {
    if correlation_id.len() == 32
        && correlation_id
            .chars()
            .all(|character| character.is_ascii_digit() || ('a'..='f').contains(&character))
    {
        Ok(())
    } else {
        Err(LogViolation::InvalidCorrelationId)
    }
}

fn truncate_chars(input: &str, limit: usize) -> String {
    if input.chars().count() <= limit {
        return input.to_owned();
    }
    input.chars().take(limit).collect()
}

/// Formats a `SystemTime` as seconds-precision RFC 3339 UTC.
pub fn format_rfc3339(time: SystemTime) -> String {
    let duration = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let seconds = duration.as_secs() as i64;
    let days = seconds.div_euclid(86_400);
    let second_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = second_of_day / 3_600;
    let minute = (second_of_day % 3_600) / 60;
    let second = second_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = month_index + if month_index < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn record() -> LogRecord {
        LogRecord::at(
            "2026-11-03T09:14:22Z",
            Severity::Info,
            "pairing.attempt",
            "00112233445566778899aabbccddeeff",
            Outcome::Rejected,
        )
        .unwrap()
    }

    #[test]
    fn emits_one_json_line_with_the_contract_fields() {
        let record = record()
            .with_reason_code(ReasonCode::CertificatePinMismatch)
            .with_detail("pinned certificate mismatch")
            .unwrap();
        let json = record.to_json();
        assert!(json.starts_with('{'));
        assert!(json.contains("\"severity\":\"info\""));
        assert!(json.contains("\"operation\":\"pairing.attempt\""));
        assert!(json.contains("\"outcome\":\"rejected\""));
        assert!(json.contains("\"reason_code\":\"certificate_pin_mismatch\""));
        assert!(!json.contains('\n'));

        let mut output = Vec::new();
        write_line(&mut output, &record).unwrap();
        assert!(output.ends_with(b"\n"));
    }

    #[test]
    fn omits_optional_fields_when_absent() {
        let json = record().to_json();
        assert!(!json.contains("reason_code"));
        assert!(!json.contains("detail"));
    }

    #[test]
    fn rejects_invalid_operations_and_correlation_ids() {
        assert_eq!(
            LogRecord::at(
                "2026-11-03T09:14:22Z",
                Severity::Info,
                "Pairing attempt",
                "00112233445566778899aabbccddeeff",
                Outcome::Started
            ),
            Err(LogViolation::InvalidOperation)
        );
        assert_eq!(
            LogRecord::at(
                "2026-11-03T09:14:22Z",
                Severity::Info,
                "pairing.attempt",
                "XYZ",
                Outcome::Started
            ),
            Err(LogViolation::InvalidCorrelationId)
        );
        assert_eq!(
            LogRecord::at(
                "2026-11-03T09:14:22Z",
                Severity::Info,
                "a".repeat(65),
                "00112233445566778899aabbccddeeff",
                Outcome::Started
            ),
            Err(LogViolation::InvalidOperation)
        );
    }

    #[test]
    fn redacts_tokens_and_fingerprints() {
        let token = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let redacted = redact(&format!("token={token}"));
        assert_eq!(redacted, "token=[redacted]");
        assert!(!redacted.contains(token));
    }

    #[test]
    fn redacts_absolute_user_paths() {
        assert_eq!(redact("read /home/alice/secret.wav"), "read <path>");
        assert_eq!(redact("read C:\\Users\\alice\\secret.wav"), "read <path>");
        assert_eq!(redact("read /Users/alice/secret.wav"), "read <path>");
    }

    #[test]
    fn removes_control_characters() {
        assert_eq!(redact("line\nbreak"), "line break");
    }

    #[test]
    fn a_detail_is_redacted_before_it_is_stored() {
        let detail = format!("fingerprint {}", "ab".repeat(32));
        let record = record().with_detail(&detail).unwrap();
        assert!(record.detail.unwrap().contains("[redacted]"));
        assert_eq!(
            redact("keep 0123456789abcdef0123456789abcdef"),
            "keep [redacted]"
        );
        assert_eq!(redact("path=/home/alice/x.wav"), "path=<path>");
    }

    #[test]
    fn formats_rfc3339_for_known_instants() {
        assert_eq!(format_rfc3339(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(
            format_rfc3339(UNIX_EPOCH + Duration::from_secs(1_893_456_000)),
            "2030-01-01T00:00:00Z"
        );
        assert_eq!(
            format_rfc3339(UNIX_EPOCH + Duration::from_secs(1_782_000_000)),
            "2026-06-21T00:00:00Z"
        );
    }
}
