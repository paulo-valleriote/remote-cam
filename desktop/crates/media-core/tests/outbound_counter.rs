use remotecam_media_core::{OutboundKind, OutboundRtpCounter};
use remotecam_protocol::ReasonCode;

#[test]
fn control_rtcp_is_allowed_and_counted_apart_from_media() {
    let counter = OutboundRtpCounter::new();

    for _ in 0..3 {
        assert_eq!(counter.record(OutboundKind::ControlRtcp), Ok(()));
    }

    assert_eq!(counter.control_rtcp(), 3);
    assert_eq!(counter.media_rtp(), 0);
    assert_eq!(counter.violation(), None);
}

#[test]
fn any_media_rtp_fails_with_security_violation_not_just_a_metric() {
    let counter = OutboundRtpCounter::new();

    assert_eq!(
        counter.record(OutboundKind::MediaRtp),
        Err(ReasonCode::SecurityViolation)
    );

    assert_eq!(counter.media_rtp(), 1);
    assert_eq!(counter.violation(), Some(ReasonCode::SecurityViolation));
}

#[test]
fn violation_is_latched_and_control_rtcp_afterwards_does_not_clear_it() {
    let counter = OutboundRtpCounter::new();
    assert_eq!(counter.record(OutboundKind::ControlRtcp), Ok(()));
    assert_eq!(counter.violation(), None);

    assert!(counter.record(OutboundKind::MediaRtp).is_err());
    assert_eq!(counter.record(OutboundKind::ControlRtcp), Ok(()));

    assert_eq!(counter.violation(), Some(ReasonCode::SecurityViolation));
    assert_eq!(counter.control_rtcp(), 2);
}

#[test]
fn a_clone_observes_a_violation_recorded_through_another_handle() {
    let counter = OutboundRtpCounter::new();
    let tap = counter.clone();

    assert!(tap.record(OutboundKind::MediaRtp).is_err());

    assert_eq!(counter.violation(), Some(ReasonCode::SecurityViolation));
}

#[test]
fn concurrent_recorders_lose_no_increment_and_every_media_call_fails() {
    const THREADS: u64 = 8;
    const PER_THREAD: u64 = 1_000;
    let counter = OutboundRtpCounter::new();

    let workers: Vec<_> = (0..THREADS)
        .map(|worker| {
            let counter = counter.clone();
            std::thread::spawn(move || {
                let mut media_failures = 0;
                for _ in 0..PER_THREAD {
                    assert_eq!(counter.record(OutboundKind::ControlRtcp), Ok(()));
                    // Only odd workers send media: even workers prove RTCP is never blamed.
                    if worker % 2 == 1
                        && counter.record(OutboundKind::MediaRtp)
                            == Err(ReasonCode::SecurityViolation)
                    {
                        media_failures += 1;
                    }
                }
                media_failures
            })
        })
        .collect();
    let failures: u64 = workers.into_iter().map(|w| w.join().unwrap()).sum();

    assert_eq!(counter.control_rtcp(), THREADS * PER_THREAD);
    assert_eq!(counter.media_rtp(), THREADS / 2 * PER_THREAD);
    assert_eq!(failures, THREADS / 2 * PER_THREAD);
}

#[test]
fn once_a_recorder_saw_its_failure_every_observer_sees_the_violation() {
    let counter = OutboundRtpCounter::new();
    let (failed_tx, failed_rx) = std::sync::mpsc::channel();
    let observer = {
        let counter = counter.clone();
        std::thread::spawn(move || {
            failed_rx.recv().unwrap();
            counter.violation()
        })
    };

    assert!(counter.record(OutboundKind::MediaRtp).is_err());
    failed_tx.send(()).unwrap();

    assert_eq!(
        observer.join().unwrap(),
        Some(ReasonCode::SecurityViolation)
    );
}

#[test]
fn an_unclassified_packet_is_a_violation_counted_apart_from_media() {
    let counter = OutboundRtpCounter::new();

    assert_eq!(
        counter.record(OutboundKind::Unclassified),
        Err(ReasonCode::SecurityViolation)
    );

    assert_eq!(counter.unclassified(), 1);
    assert_eq!(counter.media_rtp(), 0);
    assert_eq!(counter.violation(), Some(ReasonCode::SecurityViolation));
}

#[tokio::test(flavor = "current_thread")]
async fn violated_stays_pending_until_the_first_forbidden_packet() {
    let counter = OutboundRtpCounter::new();
    assert_eq!(counter.record(OutboundKind::ControlRtcp), Ok(()));

    let early =
        tokio::time::timeout(std::time::Duration::from_millis(50), counter.violated()).await;
    assert!(early.is_err(), "control RTCP never wakes the watcher");

    let waiter = tokio::spawn({
        let counter = counter.clone();
        async move { counter.violated().await }
    });
    tokio::task::yield_now().await;
    assert!(counter.record(OutboundKind::MediaRtp).is_err());

    assert_eq!(waiter.await.unwrap(), ReasonCode::SecurityViolation);
}

#[tokio::test(flavor = "current_thread")]
async fn violated_resolves_at_once_for_a_violation_that_already_happened() {
    let counter = OutboundRtpCounter::new();
    assert!(counter.record(OutboundKind::MediaRtp).is_err());

    let reason = tokio::time::timeout(std::time::Duration::from_secs(1), counter.violated())
        .await
        .expect("no wakeup is lost for an earlier violation");

    assert_eq!(reason, ReasonCode::SecurityViolation);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_concurrent_watcher_is_woken_by_one_violation() {
    let counter = OutboundRtpCounter::new();
    let watchers: Vec<_> = (0..16)
        .map(|_| {
            let counter = counter.clone();
            tokio::spawn(async move { counter.violated().await })
        })
        .collect();
    tokio::task::yield_now().await;

    assert!(counter.record(OutboundKind::MediaRtp).is_err());

    for watcher in watchers {
        let reason = tokio::time::timeout(std::time::Duration::from_secs(5), watcher)
            .await
            .expect("watcher woke")
            .unwrap();
        assert_eq!(reason, ReasonCode::SecurityViolation);
    }
}
