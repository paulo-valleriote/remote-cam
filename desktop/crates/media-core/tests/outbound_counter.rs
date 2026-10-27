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
