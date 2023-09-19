use alloc::{boxed::Box};
use core::{
    cell::{RefCell, UnsafeCell},
    future::Future,
    pin::Pin,
};
use port_manager::{Port, PortManager};





#[macro_use]
pub mod vga;
pub mod port_manager;
pub mod rtc;
pub mod serial;

pub type PrinterFunction = dyn FnMut(&'_ str) -> Pin<Box<dyn '_ + Future<Output = ()>>>;

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
    port: RefCell<Option<Port>>,
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

pub fn init_late(port_manager: &mut PortManager) {
    const ISA_DEBUG_EXIT_PORT_NUM: u16 = 0xf4;
    let mut port = EXIT_PORT.port.borrow_mut();
    *port = Some(
        port_manager
            .request_port(ISA_DEBUG_EXIT_PORT_NUM)
            .expect("Failed to get exit port"),
    )
}

pub unsafe fn exit(code: u8) {
    let mut port = EXIT_PORT.port.borrow_mut();
    port.as_mut()
        .expect("exit port not initialized")
        .writeb(code);
}
