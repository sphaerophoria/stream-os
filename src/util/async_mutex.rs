use crate::util::waker_list::{WakerList, WakerListHandle};

use core::{
    cell::UnsafeCell,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicUsize, Ordering},
    task::Poll,
};

pub struct MutexGuard<'a, T> {
    inner: &'a mut T,
    count: &'a AtomicUsize,
    wakeup_list: &'a WakerList,
    _not_send: PhantomData<*const ()>,
}

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.inner
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner
    }
}

impl<T> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        self.count.store(0, Ordering::Release);
        self.wakeup_list.notify_one();
    }
}

unsafe impl<T: Send> Send for MutexGuard<'_, T> {}

pub struct Mutex<T> {
    inner: UnsafeCell<T>,
    count: AtomicUsize,
    wakeup_list: WakerList,
    _not_send: PhantomData<*const ()>,
}

impl<T> Mutex<T> {
    pub fn new(inner: T) -> Mutex<T> {
        Mutex {
            inner: inner.into(),
            count: 0.into(),
            wakeup_list: WakerList::new(),
            _not_send: PhantomData,
        }
    }

    fn acquire(&self) -> Option<MutexGuard<'_, T>> {
        unsafe {
            let count = self.count.load(Ordering::Acquire);
            if count != 0 {
                return None;
            }

            if self
                .count
                .compare_exchange_weak(count, count + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                return None;
            };

            // Should be guaranteed that only 1 thread made it through

            Some(MutexGuard {
                inner: &mut *self.inner.get(),
                count: &self.count,
                wakeup_list: &self.wakeup_list,
                _not_send: PhantomData,
            })
        }
    }

    pub async fn lock(&self) -> MutexGuard<'_, T> {
        loop {
            let wakeup_handle = self.wakeup_list.handle();
            MutexLocker {
                count: &self.count,
                wakeup_handle,
            }
            .await;

            if let Some(guard) = self.acquire() {
                return guard;
            }
        }
    }

    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        self.acquire()
    }
}

fn is_lock_free(count: usize) -> bool {
    count == 0
}

struct MutexLocker<'a> {
    count: &'a AtomicUsize,
    wakeup_handle: WakerListHandle<'a>,
}

impl core::future::Future for MutexLocker<'_> {
    type Output = ();

    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        let count = self.count.load(Ordering::Acquire);
        if is_lock_free(count) {
            Poll::Ready(())
        } else {
            self.wakeup_handle.register(cx.waker().clone());
            Poll::Pending
        }
    }
}

unsafe impl<T: Send> Sync for Mutex<T> {}
unsafe impl<T: Send> Send for Mutex<T> {}

#[cfg(test)]
mod test {
    use super::*;
    use crate::testing::*;

    create_test!(test_try_async_lock, {
        let x = Mutex::new(5);
        let guard1 = x.lock().await;
        let guard2 = x.try_lock();
        test_true!(guard2.is_none());
        drop(guard1);
        let guard2 = x.try_lock();
        let guard2 = match guard2 {
            Some(v) => v,
            None => {
                return Err("Guard should be available".into());
            }
        };
        test_eq!(*guard2, 5);
        Ok(())
    });
}
