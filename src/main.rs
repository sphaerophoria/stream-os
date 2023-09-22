#![allow(clippy::missing_safety_doc)]
#![feature(panic_info_message)]
#![feature(concat_idents)]
#![feature(abi_x86_interrupt)]
#![feature(maybe_uninit_uninit_array)]
#![feature(const_maybe_uninit_uninit_array)]
#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(testing::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

#[macro_use]
mod print;
#[macro_use]
mod logger;
#[macro_use]
#[cfg(test)]
mod testing;
mod allocator;
mod future;
mod gdt;
#[macro_use]
mod interrupts;
mod io;
mod libc;
mod multiboot;
mod rtl8139;
mod sleep;
mod time;
mod util;

use alloc::{boxed::Box, rc::Rc, vec};

use core::{arch::global_asm, cell::RefCell, fmt::Write, panic::PanicInfo};

use crate::{
    future::execute_fut,
    interrupts::{InitInterruptError, InterruptHandlerData},
    io::{
        io_allocator::IoAllocator, pci::Pci, rtc::Rtc, serial::Serial, vga::TerminalWriter,
        PrinterFunction,
    },
    multiboot::MultibootInfo,
    rtl8139::Rtl8139,
    sleep::WakeupList,
    time::MonotonicTime,
    util::interrupt_guard::InterruptGuarded,
};

// Include boot.s which defines _start as inline assembly in main. This allows us to do more fine
// grained setup than if we used a naked _start function in rust. Theoretically we could use a
// naked function + some inline asm, but this seems much more straight forward.
global_asm!(include_str!("boot.s"), options(att_syntax));

extern "C" {
    static KERNEL_START: u32;
    static KERNEL_END: u32;
}

struct EarlyInitHandles {
    io_allocator: IoAllocator,
    terminal_writer: Rc<RefCell<TerminalWriter>>,
    serial: Rc<RefCell<Serial>>,
    interrupt_handlers: &'static InterruptHandlerData,
}

unsafe fn interrupt_guarded_init(
    info: *const MultibootInfo,
) -> Result<EarlyInitHandles, InitInterruptError> {
    let _guard = InterruptGuarded::new(());
    let _guard = _guard.lock();

    allocator::init(&*info);
    logger::init(Default::default());

    let mut io_allocator = io::io_allocator::IoAllocator::new();
    let terminal_writer = Rc::new(RefCell::new(TerminalWriter::new()));
    let serial = Rc::new(RefCell::new(
        Serial::new(&mut io_allocator).expect("Failed to initialize serial"),
    ));

    io::init_stdio(gen_printers(
        Rc::clone(&serial),
        Rc::clone(&terminal_writer),
    ));
    gdt::init();

    let interrupt_handlers = interrupts::init(&mut io_allocator)?;

    Ok(EarlyInitHandles {
        io_allocator,
        terminal_writer,
        serial,
        interrupt_handlers,
    })
}

#[allow(clippy::await_holding_refcell_ref)]
fn gen_printers(
    serial: Rc<RefCell<Serial>>,
    terminal_writer: Rc<RefCell<TerminalWriter>>,
) -> Box<PrinterFunction> {
    Box::new(move |s| {
        let serial = Rc::clone(&serial);
        let terminal_writer = Rc::clone(&terminal_writer);
        terminal_writer
            .borrow_mut()
            .write_str(s)
            .expect("Failed to write to terminal");
        serial.borrow_mut().write_str(s);
    })
}

#[allow(unused)]
struct Kernel {
    io_allocator: IoAllocator,
    interrupt_handlers: &'static InterruptHandlerData,
    rtc: Rtc,
    pci: Pci,
    rtl8139: Rtl8139,
    serial: Rc<RefCell<Serial>>,
    terminal_writer: Rc<RefCell<TerminalWriter>>,
    monotonic_time: Rc<MonotonicTime>,
    wakeup_list: Rc<WakeupList>,
}

impl Kernel {
    unsafe fn init(info: *const MultibootInfo) -> Result<Kernel, InitInterruptError> {
        let EarlyInitHandles {
            mut io_allocator,
            terminal_writer,
            serial,
            interrupt_handlers,
        } = interrupt_guarded_init(info)?;

        let monotonic_time = Rc::new(MonotonicTime::new(Rtc::tick_freq()));
        let wakeup_list = Rc::new(WakeupList::new());
        io::init_late(&mut io_allocator);

        let on_tick = {
            let monotonic_time = Rc::clone(&monotonic_time);
            let wakeup_list = Rc::clone(&wakeup_list);

            move || {
                let tick = monotonic_time.increment();
                wakeup_list.wakeup_if_neccessary(tick);
            }
        };

        let rtc = io::rtc::Rtc::new(&mut io_allocator, interrupt_handlers, on_tick)
            .expect("Failed to construct rtc");

        let mut pci = Pci::new(&mut io_allocator).expect("Failed to initialize pci");

        let rtl8139 = Rtl8139::new(&mut pci).expect("Failed to initialize rtl8139");

        Ok(Kernel {
            interrupt_handlers,
            io_allocator,
            rtc,
            pci,
            rtl8139,
            serial,
            terminal_writer,
            monotonic_time,
            wakeup_list,
        })
    }

    async unsafe fn demo(&mut self) {
        info!("A vector: {:?}", vec![1, 2, 3, 4, 5]);
        let a_map: hashbrown::HashMap<&'static str, i32> =
            [("test", 1), ("test2", 2)].into_iter().collect();
        info!("A map: {:?}", a_map);

        let mut date = self.rtc.read().expect("failed to read date");
        info!("Current date: {:?}", date);
        date.hours -= 1;
        self.rtc.write(&date).expect("failed to write rtc date");

        let date = self.rtc.read().expect("failed to read date");
        info!("Current date modified in cmos: {:?}", date);

        self.rtl8139.log_mac();

        info!("Sleep for 3 seconds");

        sleep::sleep(3.0, &self.monotonic_time, &self.wakeup_list).await;

        info!("And now we exit/halt");
    }
}

async unsafe fn async_main(mut kernel: Kernel) {
    let sleep = {
        let monotonic_time = Rc::clone(&kernel.monotonic_time);
        let wakeup_list = Rc::clone(&kernel.wakeup_list);
        move |t| {
            let monotonic_time = Rc::clone(&monotonic_time);
            let wakeup_list = Rc::clone(&wakeup_list);
            Box::pin(async move { sleep::sleep(t, &monotonic_time, &wakeup_list).await })
        }
    };

    let demo_fut = async {
        #[cfg(test)]
        {
            test_main();
            // FIXME: Sleep for a little longer to give the logger time to print the last message
            sleep(0.1).await;
            io::exit(0);
        }

        kernel.demo().await;
        // FIXME: Sleep for a little longer to give the logger time to print the last message
        sleep(0.1).await;
    };

    futures::future::select(Box::pin(logger::service(&sleep)), Box::pin(demo_fut)).await;
}

#[no_mangle]
pub unsafe extern "C" fn kernel_main(_multiboot_magic: u32, info: *const MultibootInfo) -> i32 {
    let kernel = Kernel::init(info).expect("Failed to initialize kernel");

    execute_fut(async_main(kernel));

    io::exit(0);
    0
}

/// This function is called on panic.
#[panic_handler]
fn panic(panic_info: &PanicInfo) -> ! {
    if let Some(args) = panic_info.message() {
        println!("{}", args);
    } else {
        println!("Paniced!");
    }

    unsafe {
        io::exit(1);
    }

    loop {}
}
