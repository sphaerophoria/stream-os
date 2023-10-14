use crate::{
    future::wakeup_executor, time::MonotonicTime, util::async_mutex::Mutex,
    util::interrupt_guard::InterruptGuarded,
};

use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use alloc::{
    collections::{BTreeSet, VecDeque},
    sync::Arc,
};

// Multi-thread way to request wakeups
#[derive(Clone)]
pub struct WakeupRequester {
    posted_wakeup_times: Arc<Mutex<VecDeque<usize>>>,
}

impl WakeupRequester {
    pub async fn register_wakeup_time(&self, tick: usize) {
        let mut wakeup_times = self.posted_wakeup_times.lock().await;
        wakeup_times.push_back(tick);
    }
}

struct TimeWaiter<'a> {
    posted_wakeup_times: &'a Arc<Mutex<VecDeque<usize>>>,
}

impl core::future::Future for TimeWaiter<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let pinned = core::pin::pin!(self.posted_wakeup_times.lock());
        let times = match pinned.poll(cx) {
            Poll::Ready(v) => v,
            Poll::Pending => return Poll::Pending,
        };

        if times.is_empty() {
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
}

// Registers wakeup requests with interrupt handler
pub struct WakeupService {
    posted_wakeup_times: Arc<Mutex<VecDeque<usize>>>,
    interrupt_visible_wakeup_times: Arc<InterruptGuarded<BTreeSet<usize>>>,
}

impl WakeupService {
    pub async fn service(&mut self) {
        loop {
            TimeWaiter {
                posted_wakeup_times: &self.posted_wakeup_times,
            }
            .await;
            let mut wakeup_times = self.posted_wakeup_times.lock().await;
            let mut interrupt_times = self.interrupt_visible_wakeup_times.lock();
            let len = wakeup_times.len();
            for time in wakeup_times.drain(..len) {
                interrupt_times.insert(time);
            }
        }
    }
}

// Checks wakeups in interrupt handler
pub struct InterruptWakeupList {
    wakeup_times: Arc<InterruptGuarded<BTreeSet<usize>>>,
}

impl InterruptWakeupList {
    pub fn wakeup_if_neccessary(&self, time: usize) {
        let mut wakeup_times = self.wakeup_times.lock();

        let mut last_idx = 0;
        for (i, item) in wakeup_times.iter().enumerate() {
            if *item > time {
                break;
            }
            last_idx = i + 1;
        }

        for _ in 0..last_idx {
            wakeup_times.pop_first();
        }

        if last_idx > 0 {
            wakeup_executor();
        }
    }
}

pub fn construct_wakeup_handlers() -> (WakeupRequester, WakeupService, InterruptWakeupList) {
    let posted_wakeup_times = Arc::new(Mutex::new(VecDeque::new()));
    let interrupt_visible_wakeup_itimes = Arc::new(InterruptGuarded::new(BTreeSet::new()));

    let requester = WakeupRequester {
        posted_wakeup_times: Arc::clone(&posted_wakeup_times),
    };

    let handler = WakeupService {
        posted_wakeup_times,
        interrupt_visible_wakeup_times: Arc::clone(&interrupt_visible_wakeup_itimes),
    };

    let interrupt_handler = InterruptWakeupList {
        wakeup_times: interrupt_visible_wakeup_itimes,
    };

    (requester, handler, interrupt_handler)
}

struct SleepFuture<'a> {
    end_tick: usize,
    monotonic_time: &'a MonotonicTime,
}

impl SleepFuture<'_> {
    async fn new<'a>(
        time_s: f32,
        monotonic_time: &'a MonotonicTime,
        wakeup_list: &WakeupRequester,
    ) -> SleepFuture<'a> {
        let start = monotonic_time.get();
        let end = start + (time_s * monotonic_time.tick_freq()) as usize;
        wakeup_list.register_wakeup_time(end).await;

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

pub async fn sleep(time_s: f32, monotonic_time: &MonotonicTime, wakeup_list: &WakeupRequester) {
    SleepFuture::new(time_s, monotonic_time, wakeup_list)
        .await
        .await
}
