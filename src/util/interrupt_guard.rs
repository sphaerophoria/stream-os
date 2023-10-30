use core::{
    arch::asm,
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::multiprocessing;

static NUM_GUARDS: AtomicUsize = AtomicUsize::new(0);

pub struct InterruptGuard<'a, T> {
    inner: &'a mut T,
}

impl<'a, T> Drop for InterruptGuard<'a, T> {
    fn drop(&mut self) {
        if multiprocessing::cpuid() == multiprocessing::BSP_ID
            && NUM_GUARDS.fetch_sub(1, Ordering::SeqCst) == 1
        {
            unsafe {
                asm!("sti");
            }
        }
    }
}

impl<'a, T> Deref for InterruptGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.inner
    }
}

impl<'a, T> DerefMut for InterruptGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.inner
    }
}

pub struct InterruptGuarded<T> {
    inner: UnsafeCell<T>,
}

impl<T> InterruptGuarded<T> {
    pub const fn new(inner: T) -> InterruptGuarded<T> {
        InterruptGuarded {
            inner: UnsafeCell::new(inner),
        }
    }

    pub fn lock(&self) -> InterruptGuard<'_, T> {
        if multiprocessing::cpuid() == multiprocessing::BSP_ID {
            NUM_GUARDS.fetch_add(1, Ordering::SeqCst);

            unsafe {
                asm!("cli");
            }
        }

        unsafe {
            InterruptGuard {
                inner: &mut *self.inner.get(),
            }
        }
    }
}

// NOTE: Sync implementation assumes single threaded os
unsafe impl<T> Sync for InterruptGuarded<T> {}
