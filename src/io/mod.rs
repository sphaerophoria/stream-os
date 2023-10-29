use alloc::boxed::Box;
use core::{
    cell::UnsafeCell,
    fmt::Write,
    sync::atomic::{AtomicPtr, Ordering},
};
use io_allocator::{IoAllocator, IoOffset, IoRange};

pub mod io_allocator;
pub mod pci;
pub mod ps2;
pub mod rtc;
pub mod serial;

pub struct Printer {
    #[allow(clippy::type_complexity)]
    pub inner: UnsafeCell<Option<Box<dyn Write>>>,
}

impl Printer {
    const fn new() -> Printer {
        Printer {
            inner: UnsafeCell::new(None),
        }
    }
}

// Single threaded
unsafe impl Sync for Printer {}
pub static PRINTER: Printer = Printer::new();

static EXIT_PORT: AtomicPtr<IoRange> = AtomicPtr::new(core::ptr::null_mut());

pub fn init_stdio(writer: Box<dyn Write>) {
    unsafe {
        *PRINTER.inner.get() = Some(writer);
    }
}

pub fn init_late(io_allocator: &mut IoAllocator) {
    const ISA_DEBUG_EXIT_PORT_NUM: u16 = 0xf4;
    let port = Box::new(
        io_allocator
            .request_io_range(ISA_DEBUG_EXIT_PORT_NUM, 1)
            .expect("Failed to get exit port"),
    );

    EXIT_PORT.store(Box::leak(port), Ordering::Release);
}

pub unsafe fn exit(code: u8) {
    let port = EXIT_PORT.load(Ordering::Acquire);
    if port.is_null() {
        return;
    }

    (*port)
        .write_u8(IoOffset::new(0), code)
        .expect("failed to write exit port");
}
