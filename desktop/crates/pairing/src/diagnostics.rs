use std::collections::VecDeque;
use std::net::IpAddr;

use remotecam_protocol::ReasonCode;

const DIAGNOSTIC_CAPACITY: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticEntry {
    pub reason: ReasonCode,
    pub origin: Option<IpAddr>,
    pub blocked_for_millis: Option<u64>,
}

impl DiagnosticEntry {
    pub(crate) fn refusal(reason: ReasonCode, origin: Option<IpAddr>) -> Self {
        Self {
            reason,
            origin,
            blocked_for_millis: None,
        }
    }

    pub(crate) fn throttled(origin: IpAddr, blocked_for_millis: u64) -> Self {
        Self {
            reason: ReasonCode::RateLimited,
            origin: Some(origin),
            blocked_for_millis: Some(blocked_for_millis),
        }
    }

    pub fn is_security_event(&self) -> bool {
        self.reason.is_security_event()
    }
}

#[derive(Debug, Default)]
pub(crate) struct DiagnosticLog {
    entries: VecDeque<DiagnosticEntry>,
    dropped: u64,
}

impl DiagnosticLog {
    pub(crate) fn push(&mut self, entry: DiagnosticEntry) {
        if self.entries.len() >= DIAGNOSTIC_CAPACITY {
            self.entries.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.entries.push_back(entry);
    }

    pub(crate) fn drain(&mut self) -> Vec<DiagnosticEntry> {
        self.entries.drain(..).collect()
    }

    pub(crate) const fn dropped(&self) -> u64 {
        self.dropped
    }
}
