use std::time::Duration;

use remotecam_media_core::av_scenario::{AvConfig, run_av_profile, run_av_profile_observed};
use remotecam_media_core::av_sync::{DRIFT_TARGET, SyncMode};
use remotecam_media_core::testing::{LossProfile, ProfileLabel};

#[test]
fn the_nominal_profile_keeps_the_drift_inside_the_target() {
    let report = run_av_profile(AvConfig::nominal(120));

    assert_eq!(report.label, ProfileLabel::Nominal);
    assert_eq!(report.mode, SyncMode::AudioMaster);
    assert!(report.presented > 1_000, "video must actually be presented");
    assert!(
        report.max_drift <= DRIFT_TARGET,
        "max drift {:?} exceeds the target",
        report.max_drift
    );
    assert_eq!(report.over_target, 0);
    assert_eq!(report.stopped_at, None);
}

#[test]
fn every_measured_drift_reaches_the_observer_in_time_order() {
    let mut seen: Vec<(Duration, Duration)> = Vec::new();

    let report = run_av_profile_observed(AvConfig::nominal(10), &mut |at, drift| {
        seen.push((at, drift));
        true
    });

    assert_eq!(seen.len() as u64, report.measured);
    assert!(seen.windows(2).all(|pair| pair[0].0 < pair[1].0));
}

#[test]
fn an_observer_that_stops_the_run_ends_it_at_that_instant() {
    let config = AvConfig {
        video_delay: Duration::from_millis(150),
        ..AvConfig::nominal(30)
    };

    let report =
        run_av_profile_observed(config, &mut |_, drift| drift <= Duration::from_millis(100));

    let stopped = report.stopped_at.expect("the run was stopped");
    assert!(stopped < Duration::from_secs(5), "stopped at {stopped:?}");
    assert!(report.max_drift > Duration::from_millis(100));
}

#[test]
fn without_audio_the_video_keeps_playing_and_the_report_marks_the_mode() {
    let config = AvConfig {
        audio: false,
        ..AvConfig::nominal(10)
    };

    let report = run_av_profile(config);

    assert_eq!(report.mode, SyncMode::VideoFree);
    assert!(report.presented > 100);
    assert_eq!(
        report.max_drift,
        Duration::ZERO,
        "no master, nothing to drift from"
    );
}

#[test]
fn a_profile_other_than_nominal_is_labelled() {
    let config = AvConfig {
        profile: LossProfile {
            loss_per_mille: 50,
            ..LossProfile::NOMINAL
        },
        ..AvConfig::nominal(5)
    };

    assert_eq!(run_av_profile(config).label, ProfileLabel::OutOfProfile);
}

#[test]
fn the_clock_is_read_in_order_and_never_torn_while_the_audio_chain_publishes() {
    use remotecam_media_core::av_sync::AudioClock;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    const OFFSET: Duration = Duration::from_secs(7);
    let clock = AudioClock::new();
    let done = Arc::new(AtomicBool::new(false));
    let writer = {
        let (clock, done) = (clock.clone(), Arc::clone(&done));
        std::thread::spawn(move || {
            for step in 0..50_000_u64 {
                let position = Duration::from_millis(step * 20);
                clock.publish(position, position + OFFSET);
            }
            done.store(true, Ordering::Release);
        })
    };
    let (mut previous, mut reads) = (Duration::ZERO, 0_u64);
    while !done.load(Ordering::Acquire) {
        if let Some((position, at)) = clock.latest() {
            assert_eq!(
                at - position,
                OFFSET,
                "position and time come from one publication"
            );
            assert!(
                position >= previous,
                "the audio position never goes backwards"
            );
            previous = position;
            reads += 1;
        }
    }
    writer.join().expect("writer");
    assert!(reads > 0, "the reader overlapped the writer");
    let (last, _) = clock.latest().expect("published");
    assert_eq!(last, Duration::from_millis(49_999 * 20));
}

struct NullSink;

impl remotecam_media_core::audio_engine::AudioSink for NullSink {
    fn write(&mut self, _: &[i16], _: remotecam_media_core::audio_engine::FrameOrigin) {}
}

#[test]
fn the_video_chain_never_sees_the_audio_clock_behind_the_instant_the_audio_chain_announced() {
    use remotecam_media_core::Nv12Frame;
    use remotecam_media_core::audio_engine::AudioEngine;
    use remotecam_media_core::av_sync::{AudioClock, VideoSync};
    use rtc::rtp::Packet;
    use rtc::rtp::header::Header;
    use std::sync::mpsc::sync_channel;

    let clock = AudioClock::new();
    let (announce, announced) = sync_channel::<Duration>(0);
    let audio_clock = clock.clone();
    let audio_chain = std::thread::spawn(move || {
        let mut engine = AudioEngine::new(Box::new(NullSink))
            .expect("engine")
            .with_clock(audio_clock);
        for step in 0..1_500_u32 {
            let now = Duration::from_millis(u64::from(step));
            if step % 20 == 0 {
                let packet = Packet {
                    header: Header {
                        timestamp: step / 20 * 960,
                        ..Default::default()
                    },
                    payload: vec![0xF8, 0xFF, 0xFE].into(),
                };
                engine.ingest(packet, now);
            }
            engine.advance(now);
            announce.send(now).expect("video chain is listening");
        }
    });

    let mut sync = VideoSync::new(clock.clone());
    let (mut last_presented, mut presented) = (None, 0_u32);
    let mut next_frame = 0_u32;
    for now in announced {
        if now >= Duration::from_micros(u64::from(next_frame) * 33_333) {
            let frame = Nv12Frame::new(2, 2, next_frame * 3_000, vec![0; 6]).expect("frame");
            sync.offer(frame);
            next_frame += 1;
        }
        // The audio chain is blocked on the rendezvous, so its publication for `now`
        // happened before this read: the video must see a live, current clock.
        if now >= Duration::from_millis(60) {
            let position = clock.read(now).expect("the audio clock is live");
            assert!(position <= now, "audio cannot be ahead of real time");
            assert!(
                position + Duration::from_millis(61) >= now,
                "audio lags by its depth only"
            );
        }
        if let Some(frame) = sync.tick(now).frame {
            assert!(
                last_presented.is_none_or(|previous| frame.rtp_timestamp() > previous),
                "frames are presented in media order"
            );
            last_presented = Some(frame.rtp_timestamp());
            presented += 1;
        }
    }
    audio_chain.join().expect("audio chain");
    assert!(presented > 30);
}
