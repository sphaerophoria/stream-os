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
mod sleep;
mod time;
mod util;

use alloc::{boxed::Box, rc::Rc, vec};
use interrupts::InterruptHandlerData;
use io::{port_manager::PortManager, vga::TerminalWriter};
use multiboot::MultibootInfo;

use core::{arch::global_asm, cell::RefCell, fmt::Write, panic::PanicInfo};

use crate::{
    future::execute_fut,
    io::{rtc::Rtc, serial::Serial, PrinterFunction},
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

async unsafe fn async_main(
    port_manager: &mut PortManager,
    interrupt_handlers: &InterruptHandlerData,
) {
    let serial = Rc::new(RefCell::new(
        Serial::new(port_manager, interrupt_handlers).expect("Failed to initialize serial"),
    ));

    let terminal_writer = Rc::new(RefCell::new(TerminalWriter::new()));

    let printer_function: Box<PrinterFunction> = Box::new(move |s| {
        let serial = Rc::clone(&serial);
        let terminal_writer = Rc::clone(&terminal_writer);
        alloc::boxed::Box::pin(async move {
            terminal_writer
                .borrow_mut()
                .write_str(s)
                .expect("Failed to write to terminal");
            serial.borrow_mut().write_str(s).await;
        })
    });

    io::init_stdio(printer_function);
    io::init_late(port_manager);

    #[cfg(test)]
    {
        test_main();
        io::exit(0);
    }

    let monotonic_time = Rc::new(MonotonicTime::new(Rtc::tick_freq()));
    let wakeup_list = Rc::new(WakeupList::new());
    let on_tick = {
        let monotonic_time = Rc::clone(&monotonic_time);
        let wakeup_list = Rc::clone(&wakeup_list);

        move || {
            let tick = monotonic_time.increment();
            wakeup_list.wakeup_if_neccessary(tick);
        }
    };

    let sleep = {
        let monotonic_time = Rc::clone(&monotonic_time);
        let wakeup_list = Rc::clone(&wakeup_list);
        move |t| {
            let monotonic_time = Rc::clone(&monotonic_time);
            let wakeup_list = Rc::clone(&wakeup_list);
            Box::pin(async move { sleep::sleep(t, &monotonic_time, &wakeup_list).await })
        }
    };

    let demo = async move {
        let mut rtc = io::rtc::Rtc::new(port_manager, interrupt_handlers, on_tick)
            .expect("Failed to construct rtc");

        info!("A vector: {:?}", vec![1, 2, 3, 4, 5]);
        let a_map: hashbrown::HashMap<&'static str, i32> =
            [("test", 1), ("test2", 2)].into_iter().collect();
        info!("A map: {:?}", a_map);

        let mut date = rtc.read();
        info!("Current date: {:?}", date);
        date.hours -= 1;
        rtc.write(&date);
        let date = rtc.read();
        info!("Current date modified in cmos: {:?}", date);
        info!("Sleep for 3 seconds");

        sleep::sleep(3.0, &monotonic_time, &wakeup_list).await;

        info!("And now we exit/halt");

        // FIXME: Sleep for a little longer to give the logger time to print the last message
        sleep::sleep(0.1, &monotonic_time, &wakeup_list).await;
    };

    futures::future::select(Box::pin(logger::service(sleep)), Box::pin(demo)).await;
}

#[no_mangle]
pub unsafe extern "C" fn kernel_main(_multiboot_magic: u32, info: *const MultibootInfo) -> i32 {
    // Disable interrupts until interrupts are initialized
    let interrupt_guard = InterruptGuarded::new(());
    let interrupt_guard = interrupt_guard.lock();

    allocator::init(&*info);
    logger::init(Default::default());

    gdt::init();

    let mut port_manager = io::port_manager::PortManager::new();
    let interrupt_handlers = interrupts::init(&mut port_manager);
    drop(interrupt_guard);

    execute_fut(async_main(&mut port_manager, interrupt_handlers));

    io::exit(0);
    0
}

/// This function is called on panic.
#[panic_handler]
fn panic(panic_info: &PanicInfo) -> ! {
    if let Some(args) = panic_info.message() {
        execute_fut(async {
            println!("{}", args);
        });
    } else {
        execute_fut(async {
            println!("Paniced!");
        });
    }

    unsafe {
        io::exit(1);
    }

    loop {}
}
