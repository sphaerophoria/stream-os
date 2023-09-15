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
mod gdt;
#[macro_use]
mod interrupts;
mod io;
mod libc;
mod multiboot;
mod util;

use alloc::vec;
use io::rtc::Rtc;
use multiboot::MultibootInfo;

use core::{arch::global_asm, panic::PanicInfo};

use crate::util::interrupt_guard::InterruptGuarded;

// Include boot.s which defines _start as inline assembly in main. This allows us to do more fine
// grained setup than if we used a naked _start function in rust. Theoretically we could use a
// naked function + some inline asm, but this seems much more straight forward.
global_asm!(include_str!("boot.s"), options(att_syntax));

extern "C" {
    static KERNEL_START: u32;
    static KERNEL_END: u32;
}

fn sleep(time_s: f32, rtc: &Rtc) {
    // 256hz clock
    let start = rtc.get_tick();
    let end = start + (time_s * rtc.tick_freq()) as usize;
    while rtc.get_tick() < end {
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn kernel_main(_multiboot_magic: u32, info: *const MultibootInfo) -> i32 {
    // Disable interrupts until interrupts are initialized
    let interrupt_guard = InterruptGuarded::new(());
    let interrupt_guard = interrupt_guard.lock();

    allocator::init(&*info);
    logger::init(Default::default());

    let mut port_manager = io::port_manager::PortManager::new();
    io::init_stdio(&mut port_manager);
    io::init_late(&mut port_manager);

    #[cfg(test)]
    {
        test_main();
        io::exit(0);
    }

    gdt::init();

    let interrupt_handlers = interrupts::init(&mut port_manager);
    drop(interrupt_guard);

    info!("A vector: {:?}", vec![1, 2, 3, 4, 5]);
    let a_map: hashbrown::HashMap<&'static str, i32> =
        [("test", 1), ("test2", 2)].into_iter().collect();
    info!("A map: {:?}", a_map);

    let mut rtc =
        io::rtc::Rtc::new(&mut port_manager, interrupt_handlers).expect("Failed to construct rtc");
    let mut date = rtc.read();
    info!("Current date: {:?}", date);
    date.hours -= 1;
    rtc.write(&date);
    let date = rtc.read();
    info!("Current date modified in cmos: {:?}", date);
    info!("Sleep for 3 seconds");
    logger::service();

    sleep(3.0, &rtc);

    info!("And now we exit/halt");

    logger::service();

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
