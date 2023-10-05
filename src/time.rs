use core::sync::atomic::{AtomicUsize, Ordering};

pub struct MonotonicTime {
    tick: AtomicUsize,
    tick_freq: f32,
}

impl MonotonicTime {
    pub fn new(tick_freq: f32) -> MonotonicTime {
        MonotonicTime {
            tick: AtomicUsize::new(0),
            tick_freq,
        }
    }

    pub fn increment(&self) -> usize {
        let last = self.tick.fetch_add(1, Ordering::AcqRel);
        last + 1
    }

    #[cfg(test)]
    pub fn set_tick(&self, val: usize) {
        self.tick.store(val, Ordering::Release);
    }

    pub fn get(&self) -> usize {
        self.tick.load(Ordering::Acquire)
    }

    pub fn tick_freq(&self) -> f32 {
        self.tick_freq
    }
}
