use std::time::Duration;

use remotecam_media_core::Nv12Frame;
use remotecam_media_core::audio_engine::{AudioEngine, AudioSink, FrameOrigin};
use remotecam_media_core::av_sync::{AudioClock, SyncMode, VideoSync};
use rtc::rtp::Packet;
use rtc::rtp::header::Header;

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

/// A frame whose media time is `millis` after the first frame of the stream.
fn frame_at(millis: u64) -> Nv12Frame {
    let ticks = u32::try_from(millis * 90).expect("timestamp");
    Nv12Frame::new(2, 2, ticks, vec![0; 6]).expect("frame")
}

fn live_clock(position: Duration, at: Duration) -> AudioClock {
    let clock = AudioClock::new();
    clock.publish(position, at);
    clock
}

#[test]
fn without_audio_the_video_runs_free_and_the_mode_is_reported() {
    let mut sync = VideoSync::new(AudioClock::new());
    sync.offer(frame_at(0));

    let tick = sync.tick(ms(0));

    assert_eq!(tick.mode, SyncMode::VideoFree);
    assert!(
        tick.frame.is_some(),
        "video must not wait for a missing clock"
    );
    assert_eq!(tick.drift, None, "there is no master to drift from");
}

#[test]
fn a_frame_is_presented_when_the_audio_clock_reaches_its_media_time() {
    let clock = live_clock(ms(0), ms(0));
    let mut sync = VideoSync::new(clock.clone());
    sync.offer(frame_at(0));
    sync.offer(frame_at(100));

    let first = sync.tick(ms(0));
    assert_eq!(first.mode, SyncMode::AudioMaster);
    assert_eq!(first.frame.expect("first").rtp_timestamp(), 0);
    assert_eq!(first.drift, Some(Duration::ZERO));

    let held = sync.tick(ms(50));
    assert!(
        held.frame.is_none(),
        "the 100 ms frame is still in the future"
    );
    assert_eq!(held.drift, None);

    clock.publish(ms(100), ms(100));
    let second = sync.tick(ms(100));
    assert_eq!(second.frame.expect("second").rtp_timestamp(), 9_000);
}

#[test]
fn the_audio_position_advances_between_publications() {
    let clock = live_clock(ms(40), ms(1_000));
    assert_eq!(clock.read(ms(1_000)), Some(ms(40)));
    assert_eq!(clock.read(ms(1_015)), Some(ms(55)));
}

#[test]
fn a_late_frame_reports_how_far_behind_the_audio_it_is() {
    let clock = live_clock(ms(150), ms(150));
    let mut sync = VideoSync::new(clock);
    sync.offer(frame_at(0));

    let tick = sync.tick(ms(150));

    assert_eq!(tick.drift, Some(ms(150)));
    assert!(tick.frame.is_some(), "the late frame is still shown");
}

#[test]
fn a_frame_slightly_ahead_of_the_audio_is_presented_within_half_a_frame() {
    let clock = live_clock(ms(84), ms(84));
    let mut sync = VideoSync::new(clock);
    sync.offer(frame_at(0));
    sync.offer(frame_at(100));

    let tick = sync.tick(ms(84));

    assert_eq!(tick.frame.expect("frame").rtp_timestamp(), 9_000);
    assert_eq!(tick.drift, Some(ms(16)));
}

#[test]
fn the_newest_due_frame_wins_and_older_ones_are_skipped() {
    let clock = live_clock(ms(100), ms(100));
    let mut sync = VideoSync::new(clock);
    for millis in [0, 33, 66, 100] {
        sync.offer(frame_at(millis));
    }

    let tick = sync.tick(ms(100));

    assert_eq!(tick.frame.expect("frame").rtp_timestamp(), 9_000);
    assert_eq!(sync.stats().skipped, 3);
    assert_eq!(sync.queued(), 0);
}

#[test]
fn video_far_ahead_of_the_audio_is_reported_while_it_is_held() {
    let clock = live_clock(ms(0), ms(0));
    let mut sync = VideoSync::new(clock);
    sync.offer(frame_at(0));
    sync.tick(ms(0));
    sync.offer(frame_at(300));

    let tick = sync.tick(ms(0));

    assert!(tick.frame.is_none());
    assert_eq!(tick.drift, Some(ms(300)));
}

#[test]
fn a_stale_audio_clock_releases_the_video_and_a_live_one_takes_it_back() {
    let clock = live_clock(ms(0), ms(0));
    let mut sync = VideoSync::new(clock.clone());
    sync.offer(frame_at(0));
    assert!(sync.tick(ms(0)).frame.is_some());
    sync.offer(frame_at(500));
    assert!(sync.tick(ms(10)).frame.is_none(), "held by the master");

    let stale = sync.tick(ms(1_000));
    assert_eq!(stale.mode, SyncMode::VideoFree);
    assert!(stale.frame.is_some(), "no master, no waiting");

    clock.publish(ms(1_000), ms(1_000));
    sync.offer(frame_at(1_000));
    let back = sync.tick(ms(1_000));
    assert_eq!(back.mode, SyncMode::AudioMaster);
    assert_eq!(back.drift, Some(Duration::ZERO));
    assert!(sync.stats().free_presented >= 1);
    assert!(sync.stats().master_presented >= 1);
}

struct NullSink;

impl AudioSink for NullSink {
    fn write(&mut self, _: &[i16], _: FrameOrigin) {}
}

fn audio_packet(index: u32) -> Packet {
    Packet {
        header: Header {
            timestamp: index * 960,
            ..Default::default()
        },
        payload: vec![0xF8, 0xFF, 0xFE].into(),
    }
}

#[test]
fn audio_and_video_never_share_a_buffer() {
    let clock = AudioClock::new();
    let mut audio = AudioEngine::new(Box::new(NullSink))
        .expect("engine")
        .with_clock(clock.clone());
    let mut video = VideoSync::new(clock);

    for index in 0_u32..5 {
        video.offer(frame_at(u64::from(index) * 33));
    }
    assert_eq!(video.queued(), 5);
    assert_eq!(
        audio.pending_len(),
        0,
        "video must not land in the audio buffer"
    );

    for index in 0..3 {
        audio.ingest(audio_packet(index), ms(0));
    }
    assert_eq!(audio.pending_len(), 3);
    assert_eq!(video.queued(), 5, "audio must not land in the video queue");
}

#[test]
fn the_audio_engine_publishes_its_presented_position_on_the_shared_clock() {
    let clock = AudioClock::new();
    let mut audio = AudioEngine::new(Box::new(NullSink))
        .expect("engine")
        .with_clock(clock.clone());
    assert_eq!(clock.read(ms(0)), None, "nothing is playing yet");

    for index in 0..10 {
        audio.ingest(audio_packet(index), ms(u64::from(index) * 20));
    }
    // The first packet anchors the schedule at 0; the 60 ms playout depth puts the
    // first slot at 60 ms and the fourth at 120 ms.
    audio.advance(ms(120));

    assert_eq!(clock.read(ms(120)), Some(ms(60)));
}

/// Presents `count` frames 33 ms apart, each against an audio clock `lateness(i)` ahead
/// of its media time, and returns the drift reported for each.
fn present_stream(
    sync: &mut VideoSync,
    clock: &AudioClock,
    range: std::ops::Range<u64>,
    lateness: impl Fn(u64) -> u64,
) -> Vec<Duration> {
    range
        .map(|index| {
            let media = ms(index * 33);
            clock.publish(media + ms(lateness(index)), media + ms(lateness(index)));
            sync.offer(frame_at(index * 33));
            sync.tick(media + ms(lateness(index)))
                .drift
                .expect("presented")
        })
        .collect()
}

#[test]
fn one_late_frame_is_jitter_and_does_not_count_as_drift() {
    let clock = AudioClock::new();
    let mut sync = VideoSync::new(clock.clone());
    present_stream(&mut sync, &clock, 0..20, |_| 0);

    let spike = present_stream(&mut sync, &clock, 20..21, |_| 150);

    assert_eq!(
        sync.stats().max_frame_offset,
        ms(150),
        "the raw per-frame offset still shows the spike"
    );
    assert!(
        spike[0] <= ms(20),
        "a single outlier must not move the drift: {:?}",
        spike[0]
    );
}

#[test]
fn a_sustained_offset_is_drift_and_reaches_the_limit_within_a_second() {
    let clock = AudioClock::new();
    let mut sync = VideoSync::new(clock.clone());
    present_stream(&mut sync, &clock, 0..20, |_| 0);

    let drifts = present_stream(&mut sync, &clock, 20..50, |_| 150);

    assert!(drifts.iter().any(|drift| *drift > ms(100)));
    assert_eq!(*drifts.last().expect("drifts"), ms(150));
    assert!(sync.stats().max_drift >= ms(150));
}
