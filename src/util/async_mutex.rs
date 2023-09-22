use core::{
    cell::UnsafeCell,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    task::Poll,
};

#[derive(Debug, Eq, PartialEq)]
pub struct MutexGuard<'a, T> {
    inner: &'a mut T,
    count: &'a mut usize,
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
        *self.count -= 1;
        assert_eq!(*self.count, 0);
    }
}

pub struct Mutex<T> {
    inner: UnsafeCell<T>,
    count: UnsafeCell<usize>,
    _not_send: PhantomData<*const ()>,
}

impl<T> Mutex<T> {
    pub fn new(inner: T) -> Mutex<T> {
        Mutex {
            inner: inner.into(),
            count: 0.into(),
            _not_send: PhantomData,
        }
    }

    fn acquire(&self) -> MutexGuard<'_, T> {
        unsafe {
            *self.count.get() += 1;
            assert_eq!(*self.count.get(), 1);

            MutexGuard {
                inner: &mut *self.inner.get(),
                count: &mut *self.count.get(),
                _not_send: PhantomData,
            }
        }
    }

    pub async fn lock(&self) -> MutexGuard<'_, T> {
        unsafe {
            MutexLocker {
                count: &*self.count.get(),
            }
            .await;
            self.acquire()
        }
    }

    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        unsafe {
            if is_lock_free(*self.count.get()) {
                Some(self.acquire())
            } else {
                None
            }
        }
    }
}

fn is_lock_free(count: usize) -> bool {
    count == 0
}

struct MutexLocker<'a> {
    count: &'a usize,
}

impl core::future::Future for MutexLocker<'_> {
    type Output = ();

    fn poll(
        self: core::pin::Pin<&mut Self>,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        if is_lock_free(*self.count) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

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
