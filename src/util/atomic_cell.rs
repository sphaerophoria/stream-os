use core::sync::atomic::{AtomicPtr, Ordering};

use alloc::boxed::Box;

pub struct AtomicCell<T> {
    val: AtomicPtr<T>,
}

impl<T> AtomicCell<T> {
    pub const fn new() -> AtomicCell<T> {
        AtomicCell {
            val: AtomicPtr::new(core::ptr::null_mut()),
        }
    }

    pub fn store(&self, val: T) {
        unsafe {
            let val = Box::new(val);
            let prev = self.val.swap(Box::into_raw(val), Ordering::AcqRel);
            if !prev.is_null() {
                let prev = Box::from_raw(prev);
                drop(prev)
            }
        }
    }

    pub fn get(&self) -> Option<&T> {
        unsafe {
            let val = self.val.load(Ordering::Acquire);
            if !val.is_null() {
                Some(&*val)
            } else {
                None
            }
        }
    }
}
