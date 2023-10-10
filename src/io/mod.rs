use alloc::boxed::Box;
use core::cell::{RefCell, UnsafeCell};
use io_allocator::{IoAllocator, IoOffset, IoRange};

#[macro_use]
pub mod vga;
pub mod io_allocator;
pub mod pci;
pub mod ps2;
pub mod rtc;
pub mod serial;

pub type PrinterFunction = dyn FnMut(&'_ str);

pub struct Printer {
    #[allow(clippy::type_complexity)]
    pub inner: UnsafeCell<Option<Box<PrinterFunction>>>,
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

struct ExitPort {
    port: RefCell<Option<IoRange>>,
}

impl ExitPort {
    const fn new() -> ExitPort {
        ExitPort {
            port: RefCell::new(None),
        }
    }
}
// RefCell is not sync, but we only have one thread...
unsafe impl Sync for ExitPort {}

static EXIT_PORT: ExitPort = ExitPort::new();

pub fn init_stdio(print_fn: Box<PrinterFunction>) {
    unsafe {
        *PRINTER.inner.get() = Some(print_fn);
    }
}

pub fn init_late(io_allocator: &mut IoAllocator) {
    const ISA_DEBUG_EXIT_PORT_NUM: u16 = 0xf4;
    let mut port = EXIT_PORT.port.borrow_mut();
    *port = Some(
        io_allocator
            .request_io_range(ISA_DEBUG_EXIT_PORT_NUM, 1)
            .expect("Failed to get exit port"),
    )
}

pub unsafe fn exit(code: u8) {
    let mut port = EXIT_PORT.port.borrow_mut();
    port.as_mut()
        .expect("exit port not initialized")
        .write_u8(IoOffset::new(0), code)
        .expect("failed to write exit port");
}
