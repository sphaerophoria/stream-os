use core::{
    future::Future,
    sync::atomic::{AtomicBool, Ordering},
    task::{RawWaker, RawWakerVTable},
};

static SHOULD_POLL: AtomicBool = AtomicBool::new(false);

const WAKER_VTABLE: RawWakerVTable = create_raw_waker_vtable();

const fn create_raw_waker_vtable() -> RawWakerVTable {
    RawWakerVTable::new(waker_clone, waker_wake, waker_wake, waker_drop)
}

unsafe fn waker_clone(_: *const ()) -> RawWaker {
    RawWaker::new(core::ptr::null(), &WAKER_VTABLE)
}

unsafe fn waker_wake(_: *const ()) {
    SHOULD_POLL.store(true, Ordering::Release);
}

unsafe fn waker_drop(_: *const ()) {}

pub fn execute_fut<F: Future>(mut fut: F) {
    let mut fut = unsafe { core::pin::Pin::new_unchecked(&mut fut) };

    let waker = unsafe {
        let waker = waker_clone(core::ptr::null());
        core::task::Waker::from_raw(waker)
    };

    let mut context = core::task::Context::from_waker(&waker);

    while fut.as_mut().poll(&mut context).is_pending() {
        SHOULD_POLL.store(false, Ordering::Release);

        while !SHOULD_POLL.load(Ordering::Acquire) {
            unsafe {
                core::arch::asm!("hlt");
            }
        }
    }
}

pub fn wakeup_executor() {
    SHOULD_POLL.store(true, Ordering::Release);
}
