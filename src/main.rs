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
use io::io_allocator::IoOffset;

use core::{arch::global_asm, cell::RefCell, fmt::Write, panic::PanicInfo};

use crate::{
    future::execute_fut,
    interrupts::{InitInterruptError, InterruptHandlerData},
    io::{
        io_allocator::IoAllocator,
        pci::{Pci, PciDevice},
        rtc::Rtc,
        serial::Serial,
        vga::TerminalWriter,
        PrinterFunction,
    },
    multiboot::MultibootInfo,
    sleep::WakeupList,
    time::MonotonicTime,
    util::{bit_manipulation::GetBits, interrupt_guard::InterruptGuarded},
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
    interrupt_handlers: &'static InterruptHandlerData,
    monotonic_time: Rc<MonotonicTime>,
    wakeup_list: Rc<WakeupList>,
}

unsafe fn early_init(info: *const MultibootInfo) -> Result<EarlyInitHandles, InitInterruptError> {
    allocator::init(&*info);
    logger::init(Default::default());

    gdt::init();

    let mut io_allocator = io::io_allocator::IoAllocator::new();
    let interrupt_handlers = interrupts::init(&mut io_allocator)?;

    let monotonic_time = Rc::new(MonotonicTime::new(Rtc::tick_freq()));
    let wakeup_list = Rc::new(WakeupList::new());
    Ok(EarlyInitHandles {
        io_allocator,
        interrupt_handlers,
        monotonic_time,
        wakeup_list,
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
        alloc::boxed::Box::pin(async move {
            terminal_writer
                .borrow_mut()
                .write_str(s)
                .expect("Failed to write to terminal");
            serial
                .borrow_mut()
                .write_str(s)
                .await
                .expect("failed to write to terminal");
        })
    })
}

#[allow(unused)]
struct Kernel {
    io_allocator: IoAllocator,
    interrupt_handlers: &'static InterruptHandlerData,
    rtc: Rtc,
    pci: Pci,
    serial: Rc<RefCell<Serial>>,
    terminal_writer: Rc<RefCell<TerminalWriter>>,
    monotonic_time: Rc<MonotonicTime>,
    wakeup_list: Rc<WakeupList>,
}

impl Kernel {
    async unsafe fn init(early_handles: EarlyInitHandles) -> Kernel {
        let mut io_allocator = early_handles.io_allocator;
        let interrupt_handlers = early_handles.interrupt_handlers;
        let wakeup_list = early_handles.wakeup_list;
        let monotonic_time = early_handles.monotonic_time;

        let serial = Rc::new(RefCell::new(
            Serial::new(&mut io_allocator, interrupt_handlers)
                .expect("Failed to initialize serial"),
        ));

        let terminal_writer = Rc::new(RefCell::new(TerminalWriter::new()));

        let printer_function = gen_printers(Rc::clone(&serial), Rc::clone(&terminal_writer));

        io::init_stdio(printer_function);
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

        let pci = Pci::new(&mut io_allocator).expect("Failed to initialize pci");

        Kernel {
            interrupt_handlers,
            io_allocator,
            rtc,
            pci,
            serial,
            terminal_writer,
            monotonic_time,
            wakeup_list,
        }
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

        let rtl_device = match self.pci.find_device(0x10ec, 0x8139).unwrap().unwrap() {
            PciDevice::General(v) => v,
            _ => panic!("RTL device not general as expected"),
        };
        let io_base = rtl_device.find_io_base(&mut self.pci).unwrap();
        assert!(io_base <= u16::MAX as u32);
        let mut rtl_io = self
            .io_allocator
            .request_io_range(io_base as u16, 8)
            .unwrap();
        let mut mac = alloc::vec::Vec::new();
        let first_4 = rtl_io.read_32(IoOffset::new(0)).unwrap();
        mac.push(first_4.get_bits(0, 8));
        mac.push(first_4.get_bits(8, 8));
        mac.push(first_4.get_bits(16, 8));
        mac.push(first_4.get_bits(24, 8));
        let second_4 = rtl_io.read_32(IoOffset::new(4)).unwrap();
        mac.push(second_4.get_bits(0, 8));
        mac.push(second_4.get_bits(8, 8));
        println!("Mac address received through io space: {:x?}", mac);

        let memory_base = rtl_device.find_mmap_base(&mut self.pci).unwrap() as *const u8;
        let mac = core::slice::from_raw_parts(memory_base, 6);
        println!("Mac address received through memory space: {:x?}", mac);

        info!("Sleep for 3 seconds");

        sleep::sleep(3.0, &self.monotonic_time, &self.wakeup_list).await;

        info!("And now we exit/halt");
    }
}

async unsafe fn async_main(early_handles: EarlyInitHandles) {
    let sleep = {
        let monotonic_time = Rc::clone(&early_handles.monotonic_time);
        let wakeup_list = Rc::clone(&early_handles.wakeup_list);
        move |t| {
            let monotonic_time = Rc::clone(&monotonic_time);
            let wakeup_list = Rc::clone(&wakeup_list);
            Box::pin(async move { sleep::sleep(t, &monotonic_time, &wakeup_list).await })
        }
    };

    let kernel_fut = async {
        let mut kernel = Kernel::init(early_handles).await;
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

    futures::future::select(Box::pin(logger::service(&sleep)), Box::pin(kernel_fut)).await;
}

#[no_mangle]
pub unsafe extern "C" fn kernel_main(_multiboot_magic: u32, info: *const MultibootInfo) -> i32 {
    let early_handles = {
        // Disable interrupts until interrupts are initialized
        let interrupt_guard = InterruptGuarded::new(());
        #[allow(unused)]
        let interrupt_guard = interrupt_guard.lock();

        early_init(info)
    };

    execute_fut(async_main(early_handles.unwrap()));

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
