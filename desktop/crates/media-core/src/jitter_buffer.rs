use std::collections::BTreeMap;
use std::time::Duration;

use rtc::rtp::Packet;

const VIDEO_CLOCK_HZ: u64 = 90_000;
const DEPTH_JITTER_FACTOR: u64 = 2;
const MAX_DEPTH: Duration = Duration::from_millis(200);
const RETRANSMISSION_WAIT: Duration = Duration::from_millis(120);
const MAX_MISSING_LISTED: usize = 64;
const JITTER_GAIN: i64 = 16;

struct Entry {
    packet: Packet,
    arrival: Duration,
    playout: Duration,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JitterStats {
    pub lost: u64,
    pub late: u64,
    pub duplicates: u64,
}

#[derive(Default)]
pub struct JitterBuffer {
    stats: JitterStats,
    next: Option<u64>,
    pending: BTreeMap<u64, Entry>,
    origin: Option<(Duration, u32)>,
    highest: Option<u64>,
    jitter_micros: i64,
    last_transit: Option<(u32, i64)>,
}

impl JitterBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    fn extend(&mut self, sequence: u16) -> u64 {
        let extended = match self.highest {
            None => u64::from(sequence) + (1 << 16),
            Some(highest) => {
                let delta = i64::from(sequence.wrapping_sub(highest as u16) as i16);
                highest.saturating_add_signed(delta)
            }
        };
        self.highest = Some(
            self.highest
                .map_or(extended, |highest| highest.max(extended)),
        );
        extended
    }

    fn playout(&mut self, timestamp: u32, now: Duration) -> Duration {
        let (arrival, origin_timestamp) = *self.origin.get_or_insert((now, timestamp));
        let ticks = i64::from(timestamp.wrapping_sub(origin_timestamp) as i32);
        let offset = Duration::from_micros(ticks.unsigned_abs() * 1_000_000 / VIDEO_CLOCK_HZ);
        if ticks >= 0 {
            arrival + offset
        } else {
            arrival.saturating_sub(offset)
        }
    }

    pub fn target_depth(&self) -> Duration {
        let micros = self.jitter_micros.unsigned_abs() * DEPTH_JITTER_FACTOR;
        Duration::from_micros(micros).min(MAX_DEPTH)
    }

    fn measure(&mut self, timestamp: u32, now: Duration) {
        let Some((origin_arrival, origin_timestamp)) = self.origin else {
            return;
        };
        let rtp_micros = i64::from(timestamp.wrapping_sub(origin_timestamp) as i32) * 1_000_000
            / VIDEO_CLOCK_HZ as i64;
        let transit = now.as_micros() as i64 - origin_arrival.as_micros() as i64 - rtp_micros;
        if let Some((last_timestamp, last_transit)) = self.last_transit
            && last_timestamp != timestamp
        {
            let deviation = (transit - last_transit).abs();
            self.jitter_micros += (deviation - self.jitter_micros) / JITTER_GAIN;
        }
        if self.last_transit.is_none_or(|(last, _)| last != timestamp) {
            self.last_transit = Some((timestamp, transit));
        }
    }

    pub fn missing(&self) -> Vec<u16> {
        let Some(highest_pending) = self.pending.last_key_value().map(|(key, _)| *key) else {
            return Vec::new();
        };
        let start = self
            .next
            .or_else(|| self.pending.first_key_value().map(|(key, _)| *key))
            .unwrap_or(highest_pending);
        (start..highest_pending)
            .filter(|sequence| !self.pending.contains_key(sequence))
            .take(MAX_MISSING_LISTED)
            .map(|sequence| sequence as u16)
            .collect()
    }

    pub fn stats(&self) -> JitterStats {
        self.stats
    }

    pub fn push(&mut self, packet: Packet, now: Duration) {
        let extended = self.extend(packet.header.sequence_number);
        if self.next.is_some_and(|next| extended < next) {
            self.stats.late += 1;
            return;
        }
        if self.pending.contains_key(&extended) {
            self.stats.duplicates += 1;
            return;
        }
        let playout = self.playout(packet.header.timestamp, now);
        self.measure(packet.header.timestamp, now);
        let playout = playout + self.target_depth();
        self.pending.insert(
            extended,
            Entry {
                packet,
                arrival: now,
                playout,
            },
        );
    }

    pub fn poll(&mut self, now: Duration) -> Vec<Packet> {
        self.release(Some(now))
    }

    pub fn flush(&mut self) -> Vec<Packet> {
        self.release(None)
    }

    fn release(&mut self, due_at: Option<Duration>) -> Vec<Packet> {
        let mut released = Vec::new();
        while let Some(entry) = self.pending.first_entry() {
            if due_at.is_some_and(|now| entry.get().playout > now) {
                break;
            }
            let sequence = *entry.key();
            if let Some(next) = self.next.filter(|next| sequence > *next) {
                if let Some(now) = due_at
                    && now.saturating_sub(entry.get().arrival) < RETRANSMISSION_WAIT
                {
                    break;
                }
                self.stats.lost += sequence - next;
            }
            self.next = Some(sequence + 1);
            released.push(entry.remove().packet);
        }
        released
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtc::rtp::header::Header;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    fn packet(sequence: u16, timestamp: u32) -> Packet {
        Packet {
            header: Header {
                sequence_number: sequence,
                timestamp,
                ..Default::default()
            },
            payload: vec![0].into(),
        }
    }

    fn sequences(packets: &[Packet]) -> Vec<u16> {
        packets.iter().map(|p| p.header.sequence_number).collect()
    }

    #[test]
    fn reordered_packets_are_released_in_sequence_order() {
        let mut buffer = JitterBuffer::new();
        buffer.push(packet(10, 0), ms(0));
        buffer.push(packet(12, 6_000), ms(66));
        buffer.push(packet(11, 3_000), ms(70));
        assert_eq!(sequences(&buffer.poll(ms(200))), [10, 11, 12]);
    }

    #[test]
    fn a_skipped_gap_counts_as_lost_and_a_straggler_is_discarded_as_late() {
        let mut buffer = JitterBuffer::new();
        buffer.push(packet(10, 0), ms(0));
        assert_eq!(sequences(&buffer.poll(ms(0))), [10]);
        buffer.push(packet(12, 6_000), ms(66));
        assert_eq!(sequences(&buffer.poll(ms(200))), [12]);
        assert_eq!(buffer.stats().lost, 1);
        buffer.push(packet(11, 3_000), ms(210));
        assert!(buffer.poll(ms(300)).is_empty());
        assert_eq!(buffer.stats().late, 1);
    }

    #[test]
    fn a_duplicate_is_ignored() {
        let mut buffer = JitterBuffer::new();
        buffer.push(packet(5, 0), ms(0));
        buffer.push(packet(5, 0), ms(1));
        assert_eq!(sequences(&buffer.poll(ms(100))), [5]);
        assert_eq!(buffer.stats().duplicates, 1);
    }

    #[test]
    fn sequence_numbers_wrap_around_in_order() {
        let mut buffer = JitterBuffer::new();
        buffer.push(packet(65_535, 0), ms(0));
        buffer.push(packet(1, 6_000), ms(66));
        buffer.push(packet(0, 3_000), ms(60));
        assert_eq!(sequences(&buffer.poll(ms(200))), [65_535, 0, 1]);
        assert_eq!(buffer.stats().lost, 0);
    }

    #[test]
    fn gaps_seen_together_wait_for_their_retransmission_together() {
        let mut buffer = JitterBuffer::new();
        buffer.push(packet(0, 0), ms(0));
        assert_eq!(sequences(&buffer.poll(ms(0))), [0]);
        buffer.push(packet(2, 3_000), ms(100));
        buffer.push(packet(4, 6_000), ms(100));
        assert_eq!(sequences(&buffer.poll(ms(230))), [2, 4]);
        assert_eq!(buffer.stats().lost, 2);
    }

    const FRAME_SPACING_MS: u64 = 33;
    const FRAME_TICKS: u32 = 3_000;

    fn feed_frames(buffer: &mut JitterBuffer, frames: u32, jitter_ms: impl Fn(u32) -> i64) {
        for index in 0..frames {
            let arrival = (u64::from(index) * FRAME_SPACING_MS) as i64 + jitter_ms(index);
            let sequence = index as u16;
            buffer.push(
                packet(sequence, index * FRAME_TICKS),
                ms(arrival.max(0) as u64 + 1_000),
            );
        }
    }

    #[test]
    fn a_network_without_jitter_stays_within_the_twenty_millisecond_budget() {
        let mut buffer = JitterBuffer::new();
        feed_frames(&mut buffer, 300, |_| 0);
        assert!(
            buffer.target_depth() <= ms(20),
            "{:?}",
            buffer.target_depth()
        );
    }

    #[test]
    fn the_depth_grows_to_absorb_the_measured_jitter_and_stays_bounded() {
        let mut buffer = JitterBuffer::new();
        feed_frames(
            &mut buffer,
            300,
            |index| if index % 2 == 0 { 10 } else { -10 },
        );
        let depth = buffer.target_depth();
        assert!(
            depth >= ms(20),
            "{depth:?} must cover the 20 ms peak-to-peak jitter"
        );
        assert!(depth <= ms(100), "{depth:?} must not add needless latency");
    }

    #[test]
    fn the_depth_shrinks_back_when_the_network_calms_down() {
        let mut buffer = JitterBuffer::new();
        feed_frames(
            &mut buffer,
            300,
            |index| if index % 2 == 0 { 10 } else { -10 },
        );
        for index in 300..900_u32 {
            buffer.push(
                packet(index as u16, index * FRAME_TICKS),
                ms(u64::from(index) * FRAME_SPACING_MS + 1_000),
            );
        }
        assert!(buffer.target_depth() <= ms(20));
    }

    #[test]
    fn missing_lists_the_gaps_below_the_highest_packet_received() {
        let mut buffer = JitterBuffer::new();
        buffer.push(packet(10, 0), ms(0));
        buffer.push(packet(13, 9_000), ms(100));
        buffer.push(packet(11, 3_000), ms(101));
        assert_eq!(buffer.missing(), [12]);
        buffer.push(packet(12, 6_000), ms(102));
        assert!(buffer.missing().is_empty());
    }

    #[test]
    fn missing_survives_the_sequence_wraparound() {
        let mut buffer = JitterBuffer::new();
        buffer.push(packet(65_534, 0), ms(0));
        buffer.push(packet(1, 9_000), ms(100));
        assert_eq!(buffer.missing(), [65_535, 0]);
    }

    #[test]
    fn flush_releases_everything_held_in_order_ignoring_the_schedule() {
        let mut buffer = JitterBuffer::new();
        buffer.push(packet(1, 0), ms(0));
        buffer.push(packet(3, 90_000), ms(1));
        buffer.push(packet(2, 45_000), ms(2));
        assert_eq!(sequences(&buffer.flush()), [1, 2, 3]);
        assert!(buffer.flush().is_empty());
    }

    #[test]
    fn a_gap_is_held_for_the_retransmission_and_filled_in_time() {
        let mut buffer = JitterBuffer::new();
        buffer.push(packet(10, 0), ms(0));
        assert_eq!(sequences(&buffer.poll(ms(0))), [10]);
        buffer.push(packet(12, 6_000), ms(66));
        assert!(buffer.poll(ms(70)).is_empty(), "held for packet 11");
        buffer.push(packet(11, 3_000), ms(110));
        assert_eq!(sequences(&buffer.poll(ms(110))), [11, 12]);
        assert_eq!(buffer.stats(), JitterStats::default());
    }

    #[test]
    fn a_gap_is_given_up_once_the_retransmission_wait_runs_out() {
        let mut buffer = JitterBuffer::new();
        buffer.push(packet(10, 0), ms(0));
        buffer.poll(ms(0));
        buffer.push(packet(12, 6_000), ms(66));
        assert!(buffer.poll(ms(66 + 119)).is_empty());
        assert_eq!(sequences(&buffer.poll(ms(66 + 120))), [12]);
        assert_eq!(buffer.stats().lost, 1);
    }
}
