use core::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, Ordering},
};

pub struct SpinLockGuard<'a, T> {
    inner: &'a mut T,
    available: &'a AtomicBool,
}

impl<'a, T> Drop for SpinLockGuard<'a, T> {
    fn drop(&mut self) {
        self.available.store(true, Ordering::SeqCst);
    }
}

impl<'a, T> Deref for SpinLockGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.inner
    }
}

impl<'a, T> DerefMut for SpinLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.inner
    }
}

pub struct SpinLock<T> {
    inner: UnsafeCell<T>,
    available: AtomicBool,
}

impl<T> SpinLock<T> {
    pub const fn new(inner: T) -> SpinLock<T> {
        SpinLock {
            inner: UnsafeCell::new(inner),
            available: AtomicBool::new(true),
        }
    }

    pub fn lock(&self) -> SpinLockGuard<'_, T> {
        while self
            .available
            .compare_exchange_weak(true, false, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {}

        assert!(!self.available.load(Ordering::Acquire));

        unsafe {
            SpinLockGuard {
                inner: &mut *self.inner.get(),
                available: &self.available,
            }
        }
    }
}

unsafe impl<T> Sync for SpinLock<T> {}
unsafe impl<T: Send> Send for SpinLock<T> {}
