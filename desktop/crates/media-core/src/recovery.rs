use std::collections::BTreeMap;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryRequest {
    Nack(Vec<u16>),
    Pli,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecoveryStats {
    pub nack_requests: u64,
    pub nacked_packets: u64,
    pub plis: u64,
}

const NACK_RETRY_INTERVAL: Duration = Duration::from_millis(50);
const NACK_MAX_ATTEMPTS: u8 = 3;
const PLI_MIN_INTERVAL: Duration = Duration::from_millis(500);

struct Outstanding {
    attempts: u8,
    last_sent: Duration,
    given_up: bool,
}

#[derive(Default)]
pub struct Recovery {
    outstanding: BTreeMap<u16, Outstanding>,
    last_pli: Option<Duration>,
    pli_pending: bool,
    stats: RecoveryStats,
}

impl Recovery {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stats(&self) -> RecoveryStats {
        self.stats
    }

    pub fn poll(
        &mut self,
        missing: &[u16],
        reference_lost: bool,
        now: Duration,
    ) -> Vec<RecoveryRequest> {
        self.outstanding
            .retain(|sequence, _| missing.contains(sequence));
        let mut nack = Vec::new();
        let mut keyframe_wanted = reference_lost || self.pli_pending;
        for &sequence in missing {
            let entry = self.outstanding.entry(sequence).or_insert(Outstanding {
                attempts: 0,
                last_sent: now,
                given_up: false,
            });
            if entry.given_up {
                continue;
            }
            if entry.attempts == 0 || now.saturating_sub(entry.last_sent) >= NACK_RETRY_INTERVAL {
                if entry.attempts >= NACK_MAX_ATTEMPTS {
                    entry.given_up = true;
                    keyframe_wanted = true;
                } else {
                    entry.attempts += 1;
                    entry.last_sent = now;
                    nack.push(sequence);
                }
            }
        }
        let mut requests = Vec::new();
        if !nack.is_empty() {
            self.stats.nack_requests += 1;
            self.stats.nacked_packets += nack.len() as u64;
            requests.push(RecoveryRequest::Nack(nack));
        }
        if keyframe_wanted {
            let allowed = self
                .last_pli
                .is_none_or(|last| now.saturating_sub(last) >= PLI_MIN_INTERVAL);
            self.pli_pending = !allowed;
            if allowed {
                self.last_pli = Some(now);
                self.stats.plis += 1;
                requests.push(RecoveryRequest::Pli);
            }
        }
        requests
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn a_new_gap_is_nacked_immediately() {
        let mut recovery = Recovery::new();
        assert_eq!(
            recovery.poll(&[7, 9], false, ms(0)),
            [RecoveryRequest::Nack(vec![7, 9])]
        );
        assert_eq!(
            recovery.stats(),
            RecoveryStats {
                nack_requests: 1,
                nacked_packets: 2,
                plis: 0
            }
        );
    }

    #[test]
    fn a_nack_is_repeated_only_after_the_retry_interval() {
        let mut recovery = Recovery::new();
        recovery.poll(&[7], false, ms(0));
        assert!(recovery.poll(&[7], false, ms(10)).is_empty());
        assert_eq!(
            recovery.poll(&[7], false, ms(60)),
            [RecoveryRequest::Nack(vec![7])]
        );
    }

    #[test]
    fn a_recovered_packet_is_forgotten() {
        let mut recovery = Recovery::new();
        recovery.poll(&[7], false, ms(0));
        assert!(recovery.poll(&[], false, ms(60)).is_empty());
        assert_eq!(
            recovery.poll(&[7], false, ms(61)),
            [RecoveryRequest::Nack(vec![7])],
            "a sequence that went missing again starts a fresh attempt count"
        );
    }

    #[test]
    fn an_unrecoverable_packet_falls_back_to_a_keyframe_request() {
        let mut recovery = Recovery::new();
        let mut requests = Vec::new();
        for attempt in 0..5_u64 {
            requests.extend(recovery.poll(&[7], false, ms(attempt * 60)));
        }
        assert_eq!(
            requests,
            [
                RecoveryRequest::Nack(vec![7]),
                RecoveryRequest::Nack(vec![7]),
                RecoveryRequest::Nack(vec![7]),
                RecoveryRequest::Pli,
            ]
        );
        assert_eq!(recovery.stats().plis, 1);
    }

    #[test]
    fn a_lost_reference_asks_for_a_keyframe_at_a_limited_rate() {
        let mut recovery = Recovery::new();
        assert_eq!(recovery.poll(&[], true, ms(0)), [RecoveryRequest::Pli]);
        assert!(recovery.poll(&[], true, ms(100)).is_empty());
        assert_eq!(recovery.poll(&[], true, ms(600)), [RecoveryRequest::Pli]);
        assert_eq!(recovery.stats().plis, 2);
    }

    #[test]
    fn a_keyframe_request_held_back_by_the_rate_limit_is_not_forgotten() {
        let mut recovery = Recovery::new();
        assert_eq!(recovery.poll(&[], true, ms(0)), [RecoveryRequest::Pli]);
        assert!(recovery.poll(&[], true, ms(100)).is_empty());
        assert_eq!(recovery.poll(&[], false, ms(600)), [RecoveryRequest::Pli]);
        assert!(recovery.poll(&[], false, ms(1_200)).is_empty());
    }
}
