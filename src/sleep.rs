use crate::{
    future::wakeup_executor, time::MonotonicTime, util::interrupt_guard::InterruptGuarded,
};

use core::{future::Future, task::Poll};

use hashbrown::HashSet;

pub struct WakeupList {
    wakeup_times: InterruptGuarded<HashSet<usize>>,
}

impl WakeupList {
    pub fn new() -> WakeupList {
        WakeupList {
            wakeup_times: InterruptGuarded::new(HashSet::new()),
        }
    }

    pub fn register_wakeup_time(&self, tick: usize) {
        let mut wakeup_times = self.wakeup_times.lock();
        wakeup_times.insert(tick);
    }

    pub fn wakeup_if_neccessary(&self, time: usize) {
        let wakeup_times = self.wakeup_times.lock();
        if wakeup_times.contains(&time) {
            wakeup_executor();
        }
    }
}

pub struct SleepFuture<'a> {
    end_tick: usize,
    monotonic_time: &'a MonotonicTime,
}

impl SleepFuture<'_> {
    fn new<'a>(
        time_s: f32,
        monotonic_time: &'a MonotonicTime,
        wakeup_list: &WakeupList,
    ) -> SleepFuture<'a> {
        let start = monotonic_time.get();
        let end = start + (time_s * monotonic_time.tick_freq()) as usize;
        wakeup_list.register_wakeup_time(end);

        SleepFuture {
            end_tick: end,
            monotonic_time,
        }
    }
}

impl<'a> Future for SleepFuture<'a> {
    type Output = ();

    fn poll(
        self: core::pin::Pin<&mut Self>,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        if self.monotonic_time.get() < self.end_tick {
            return Poll::Pending;
        }
        Poll::Ready(())
    }
}

pub async fn sleep(time_s: f32, monotonic_time: &MonotonicTime, wakeup_list: &WakeupList) {
    SleepFuture::new(time_s, monotonic_time, wakeup_list).await
}
