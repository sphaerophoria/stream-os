use crate::util::spinlock::SpinLock;

use hashbrown::HashMap;

use core::task::Waker;

struct WakeupListInner {
    id: u64,
    wakers: HashMap<u64, Waker>,
}

pub struct WakerList {
    inner: SpinLock<WakeupListInner>,
}

impl WakerList {
    pub fn new() -> WakerList {
        let inner = WakeupListInner {
            id: 0,
            wakers: Default::default(),
        };

        let inner = SpinLock::new(inner);

        WakerList { inner }
    }

    pub fn notify_one(&self) {
        let inner = self.inner.lock();
        if let Some((_, waker)) = inner.wakers.iter().next() {
            waker.wake_by_ref();
        }
    }

    pub fn handle(&self) -> WakerListHandle<'_> {
        let mut inner = self.inner.lock();
        let id = inner.id;
        inner.id += 1;
        WakerListHandle {
            id,
            inner: &self.inner,
        }
    }
}

pub struct WakerListHandle<'a> {
    id: u64,
    inner: &'a SpinLock<WakeupListInner>,
}

impl WakerListHandle<'_> {
    pub fn register(&mut self, waker: Waker) {
        let mut inner = self.inner.lock();
        inner.wakers.insert(self.id, waker);
    }
}

impl Drop for WakerListHandle<'_> {
    fn drop(&mut self) {
        let mut inner = self.inner.lock();
        inner.wakers.remove(&self.id);
    }
}
