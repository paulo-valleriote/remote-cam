use std::collections::BTreeMap;
use std::time::Duration;

use rtc::rtp::Packet;

use crate::audio::{AudioError, FRAME_SAMPLES, OpusDecoder, SAMPLE_RATE_HZ};

const FRAME_TICKS: u64 = FRAME_SAMPLES as u64;
const PLAYOUT_DEPTH: Duration = Duration::from_millis(60);
const MAX_CONCEALED_RUN: u32 = 5;
const RESYNC_TICKS: u64 = 2 * SAMPLE_RATE_HZ as u64;

/// Where decoded PCM goes. The Media Core does not know what is behind it.
pub trait AudioSink: Send {
    fn write(&mut self, pcm: &[i16], origin: FrameOrigin);
}

/// Whether a frame is audio the phone sent or something the engine made up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameOrigin {
    Decoded,
    Concealed,
    Silence,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AudioStats {
    pub decoded: u64,
    pub concealed: u64,
    pub silent: u64,
    pub late: u64,
    pub duplicates: u64,
    pub resyncs: u64,
    pub decode_errors: u64,
}

struct Schedule {
    anchor: Duration,
    origin: u64,
}

/// Synchronous audio core: RTP timestamps in, one 20 ms PCM frame per slot out.
/// Time enters only as an injected `Duration`.
pub struct AudioEngine {
    decoder: OpusDecoder,
    sink: Box<dyn AudioSink>,
    pending: BTreeMap<u64, Vec<u8>>,
    schedule: Option<Schedule>,
    cursor: u64,
    concealed_run: u32,
    stats: AudioStats,
}

impl AudioEngine {
    pub fn new(sink: Box<dyn AudioSink>) -> Result<Self, AudioError> {
        Ok(Self {
            decoder: OpusDecoder::new()?,
            sink,
            pending: BTreeMap::new(),
            schedule: None,
            cursor: 0,
            concealed_run: 0,
            stats: AudioStats::default(),
        })
    }

    pub fn stats(&self) -> AudioStats {
        self.stats
    }

    /// Lifts a 32-bit RTP timestamp onto the 64-bit schedule. Truncating the cursor to
    /// `u32` is deliberate: the wrapping signed difference is the distance across a wrap.
    fn extend(&self, timestamp: u32) -> u64 {
        let delta = i64::from(timestamp.wrapping_sub(self.cursor as u32) as i32);
        self.cursor.saturating_add_signed(delta)
    }

    fn anchor_at(&mut self, extended: u64, now: Duration) {
        self.schedule = Some(Schedule {
            anchor: now,
            origin: extended,
        });
        self.cursor = extended;
    }

    pub fn ingest(&mut self, packet: Packet, now: Duration) {
        if self.schedule.is_none() {
            let first = u64::from(packet.header.timestamp) | (1 << 32);
            self.anchor_at(first, now);
        }
        let extended = self.extend(packet.header.timestamp);
        if extended.abs_diff(self.cursor) > RESYNC_TICKS {
            self.pending.clear();
            self.anchor_at(extended, now);
            self.stats.resyncs += 1;
        } else if extended < self.cursor {
            self.stats.late += 1;
            return;
        }
        if self.pending.contains_key(&extended) {
            self.stats.duplicates += 1;
            return;
        }
        self.pending.insert(extended, packet.payload.to_vec());
    }

    fn due(&self, schedule: &Schedule) -> Duration {
        let ticks = self.cursor - schedule.origin;
        schedule.anchor
            + Duration::from_micros(ticks * 1_000_000 / u64::from(SAMPLE_RATE_HZ))
            + PLAYOUT_DEPTH
    }

    pub fn advance(&mut self, now: Duration) {
        while let Some(schedule) = &self.schedule {
            if self.due(schedule) > now {
                break;
            }
            while let Some(stale) = self.pending.first_entry()
                && *stale.key() < self.cursor
            {
                stale.remove();
                self.stats.late += 1;
            }
            let slot_end = self.cursor + FRAME_TICKS;
            let payload = self
                .pending
                .first_entry()
                .filter(|entry| *entry.key() < slot_end)
                .map(|entry| entry.remove());
            let (pcm, origin) = self.frame(payload);
            self.sink.write(&pcm, origin);
            self.cursor = slot_end;
        }
    }

    fn frame(&mut self, payload: Option<Vec<u8>>) -> (Vec<i16>, FrameOrigin) {
        if let Some(payload) = payload {
            match self.decoder.decode(&payload) {
                Ok(pcm) if pcm.len() == FRAME_SAMPLES => {
                    self.concealed_run = 0;
                    self.stats.decoded += 1;
                    return (pcm, FrameOrigin::Decoded);
                }
                _ => self.stats.decode_errors += 1,
            }
        }
        if self.concealed_run < MAX_CONCEALED_RUN
            && let Ok(pcm) = self.decoder.conceal()
        {
            self.concealed_run += 1;
            self.stats.concealed += 1;
            return (pcm, FrameOrigin::Concealed);
        }
        self.stats.silent += 1;
        (vec![0; FRAME_SAMPLES], FrameOrigin::Silence)
    }
}
