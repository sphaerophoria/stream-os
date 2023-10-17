use core::{
    future::Future,
    sync::atomic::{AtomicBool, Ordering},
};

use alloc::{sync::Arc, task::Wake};

struct KernelWaker {
    should_poll: AtomicBool,
}

impl Wake for KernelWaker {
    fn wake(self: Arc<Self>) {
        self.should_poll.store(true, Ordering::Release);
    }
}

pub fn execute_fut<F: Future>(mut fut: F) {
    let mut fut = unsafe { core::pin::Pin::new_unchecked(&mut fut) };

    let waker = Arc::new(KernelWaker {
        should_poll: AtomicBool::new(true),
    });

    let context_waker = Arc::clone(&waker).into();
    let mut context = core::task::Context::from_waker(&context_waker);

    loop {
        waker.should_poll.store(false, Ordering::Release);

        if fut.as_mut().poll(&mut context).is_ready() {
            break;
        }

        if waker.should_poll.load(Ordering::Acquire) {
            continue;
        }

        while !waker.should_poll.load(Ordering::Acquire) {
            unsafe {
                core::arch::asm!("hlt");
            }
        }
    }
}
