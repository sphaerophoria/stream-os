use core::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use alloc::{boxed::Box, sync::Arc};

struct Storage<T> {
    elements: UnsafeCell<Box<[MaybeUninit<T>]>>,
    valid: Box<[AtomicBool]>,
    // Head is where we can pop elements from
    head: AtomicUsize,
    // Tail is the last valid element in the buffer
    tail: AtomicUsize,
    // First valid push position
    reserved: AtomicUsize,
}

#[derive(Clone)]
pub struct Sender<T> {
    storage: Arc<Storage<T>>,
}

impl<T> Sender<T> {
    pub fn push(&self, elem: T) -> Result<(), ()> {
        let mut push_idx;
        let num_elems = self.storage.valid.len();

        let head = self.storage.head.load(Ordering::Acquire);

        loop {
            push_idx = self.storage.reserved.load(Ordering::Acquire);
            if push_idx == num_elems {
                return Err(());
            }

            let mut new_reserved = wrapping_increment(push_idx, num_elems);
            if new_reserved == head {
                new_reserved = num_elems;
            }

            if self
                .storage
                .reserved
                .compare_exchange_weak(push_idx, new_reserved, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                break;
            }
        }

        unsafe {
            (*self.storage.elements.get())[push_idx].write(elem);
        }
        self.storage.valid[push_idx].store(true, Ordering::Release);

        loop {
            let tail = self.storage.tail.load(Ordering::Acquire);
            let mut next_tail = wrapping_increment(tail, num_elems);
            if next_tail == head {
                next_tail = num_elems;
            }

            if tail != num_elems && !self.storage.valid[tail].load(Ordering::Acquire) {
                break;
            }

            // Whether this succeeds or fails, we're just going to keep moving until valid is
            // false
            let _ = self
                .storage
                .tail
                .compare_exchange_weak(tail, next_tail, Ordering::AcqRel, Ordering::Acquire)
                .is_ok();

            if next_tail == num_elems {
                break;
            }
        }

        Ok(())
    }
}

pub struct Receiver<T> {
    storage: Arc<Storage<T>>,
}

impl<T> Receiver<T> {
    #[allow(unused)]
    pub fn size(&self) -> usize {
        unsafe {
            let len = (*self.storage.elements.get()).len();
            let head = self.storage.head.load(Ordering::Acquire);
            let tail = self.storage.tail.load(Ordering::Acquire);
            if self.storage.tail.load(Ordering::Acquire) == len {
                len
            } else if tail >= head {
                tail - head
            } else {
                len - (head - tail)
            }
        }
    }

    pub fn pop(&mut self) -> Option<T> {
        let num_elems = self.storage.valid.len();
        let head = self.storage.head.load(Ordering::Acquire);
        let tail = self.storage.tail.load(Ordering::Acquire);

        if head == tail {
            return None;
        }

        let mut data = MaybeUninit::uninit();
        unsafe {
            core::mem::swap(&mut data, &mut (*self.storage.elements.get())[head]);
        };

        // NOTE head is guaranteed to only be modified by us, a single writer
        self.storage.valid[head].store(false, Ordering::Release);
        self.storage
            .head
            .store(wrapping_increment(head, num_elems), Ordering::Release);

        let tail = self.storage.tail.load(Ordering::Acquire);
        if tail == num_elems {
            let res =
                self.storage
                    .tail
                    .compare_exchange(tail, head, Ordering::AcqRel, Ordering::Acquire);

            // We should be the only modifiers of tail in this case
            assert!(res.is_ok());

            let reserved = self.storage.reserved.load(Ordering::Acquire);
            assert_eq!(reserved, num_elems);
            let res = self.storage.reserved.compare_exchange(
                reserved,
                head,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            // We should be the only modifiers of tail in this case
            assert!(res.is_ok());
        }

        unsafe { Some(data.assume_init()) }
    }
}

pub fn channel<T>(num_elems: usize) -> (Sender<T>, Receiver<T>) {
    let mut elements = alloc::vec::Vec::new();
    let mut valid = alloc::vec::Vec::new();
    for _ in 0..num_elems {
        elements.push(MaybeUninit::uninit());
        valid.push(AtomicBool::new(false));
    }
    let storage = Arc::new(Storage {
        elements: UnsafeCell::new(elements.into_boxed_slice()),
        valid: valid.into_boxed_slice(),
        head: AtomicUsize::new(0),
        tail: AtomicUsize::new(0),
        reserved: AtomicUsize::new(0),
    });

    let tx = Sender {
        storage: Arc::clone(&storage),
    };
    let rx = Receiver { storage };

    (tx, rx)
}

unsafe impl<T: Send> Send for Receiver<T> {}
unsafe impl<T: Send> Sync for Sender<T> {}
unsafe impl<T: Send> Send for Sender<T> {}

fn wrapping_increment(i: usize, size: usize) -> usize {
    (i + 1) % size
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::testing::*;

    create_test!(buffer_full, {
        let (tx, _rx) = channel::<i32>(3);
        test_true!(tx.push(1).is_ok());
        test_true!(tx.push(2).is_ok());
        test_true!(tx.push(3).is_ok());
        test_true!(tx.push(4).is_err());
        Ok(())
    });

    create_test!(pop_empty, {
        let (_tx, mut rx) = channel::<i32>(3);
        test_true!(rx.pop().is_none());
        Ok(())
    });

    create_test!(push_pop_pop, {
        let (tx, mut rx) = channel::<i32>(3);
        test_true!(tx.push(1).is_ok());
        test_eq!(rx.pop(), Some(1));
        test_true!(rx.pop().is_none());
        Ok(())
    });

    create_test!(full_then_not_full, {
        let (tx, mut rx) = channel::<i32>(3);
        test_true!(tx.push(1).is_ok());
        test_true!(tx.push(2).is_ok());
        test_true!(tx.push(3).is_ok());
        test_true!(tx.push(4).is_err());
        test_eq!(rx.pop(), Some(1));
        test_true!(tx.push(4).is_ok());
        test_eq!(rx.pop(), Some(2));
        test_eq!(rx.pop(), Some(3));
        test_eq!(rx.pop(), Some(4));
        Ok(())
    });
}
