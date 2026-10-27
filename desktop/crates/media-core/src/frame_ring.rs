use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::{AtomicU64, Ordering};

pub const FRAME_RING_SLOTS: usize = 3;

#[derive(Debug)]
pub struct FrameRing<T> {
    slots: Mutex<VecDeque<T>>,
    dropped: AtomicU64,
}

impl<T> FrameRing<T> {
    pub fn new() -> Self {
        Self {
            slots: Mutex::new(VecDeque::with_capacity(FRAME_RING_SLOTS)),
            dropped: AtomicU64::new(0),
        }
    }

    pub fn push(&self, frame: T) -> Option<T> {
        let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        let evicted = if slots.len() == FRAME_RING_SLOTS {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            slots.pop_front()
        } else {
            None
        };
        slots.push_back(frame);
        evicted
    }

    pub fn pop(&self) -> Option<T> {
        self.slots
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
    }

    pub fn len(&self) -> usize {
        self.slots
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn clear(&self) {
        self.slots
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }
}

impl<T> Default for FrameRing<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn frames_come_out_in_arrival_order() {
        let ring = FrameRing::new();
        assert!(ring.push(1).is_none());
        assert!(ring.push(2).is_none());
        assert_eq!(ring.len(), 2);
        assert_eq!(ring.pop(), Some(1));
        assert_eq!(ring.pop(), Some(2));
        assert_eq!(ring.pop(), None);
        assert!(ring.is_empty());
    }

    #[test]
    fn a_full_ring_drops_the_oldest_frame() {
        let ring = FrameRing::new();
        for frame in 0..FRAME_RING_SLOTS as u32 {
            assert!(ring.push(frame).is_none());
        }
        assert_eq!(ring.push(10), Some(0));
        assert_eq!(ring.push(11), Some(1));
        assert_eq!(ring.dropped(), 2);
        assert_eq!(ring.len(), FRAME_RING_SLOTS);
        assert_eq!(ring.pop(), Some(2));
        assert_eq!(ring.pop(), Some(10));
        assert_eq!(ring.pop(), Some(11));
    }

    #[test]
    fn clear_discards_without_counting_a_lag_drop() {
        let ring = FrameRing::default();
        ring.push(1);
        ring.clear();
        assert!(ring.is_empty());
        assert_eq!(ring.dropped(), 0);
    }

    #[test]
    fn a_poisoned_lock_does_not_stop_the_network_thread() {
        let ring = Arc::new(FrameRing::new());
        let poisoner = Arc::clone(&ring);
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.slots.lock().expect("lock");
            panic!("poison the ring");
        })
        .join();
        assert!(ring.push(7).is_none());
        assert_eq!(ring.pop(), Some(7));
        assert_eq!(ring.len(), 0);
    }

    #[test]
    fn a_slow_consumer_never_blocks_the_producer() {
        let ring = Arc::new(FrameRing::new());
        let producer = Arc::clone(&ring);
        std::thread::spawn(move || {
            for frame in 0..1_000_u32 {
                producer.push(frame);
            }
        })
        .join()
        .expect("producer finishes without a consumer");
        assert_eq!(ring.len(), FRAME_RING_SLOTS);
        assert_eq!(ring.dropped(), 1_000 - FRAME_RING_SLOTS as u64);
        assert_eq!(ring.pop(), Some(997));
    }
}
