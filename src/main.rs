#![allow(bad_asm_style, clippy::missing_safety_doc)]
#![feature(panic_info_message)]
#![feature(concat_idents)]
#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(testing::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

#[macro_use]
mod io;
#[cfg(test)]
#[macro_use]
mod testing;
mod allocator;
mod libc;
mod multiboot;

use alloc::vec;
use multiboot::MultibootInfo;

use core::{arch::global_asm, panic::PanicInfo};

// Include boot.s which defines _start as inline assembly in main. This allows us to do more fine
// grained setup than if we used a naked _start function in rust. Theoretically we could use a
// naked function + some inline asm, but this seems much more straight forward.
global_asm!(include_str!("boot.s"));

extern "C" {
    static KERNEL_START: u32;
    static KERNEL_END: u32;
}

#[no_mangle]
pub unsafe extern "C" fn kernel_main(_multiboot_magic: u32, info: *const MultibootInfo) -> i32 {
    io::vga::init();
    io::serial::init().expect("Failed to initialize serial");
    allocator::init(&*info);

    #[cfg(test)]
    {
        test_main();
        io::exit(0);
    }

    println!(
        "Kernel start {:?}, Kernel end {:?}",
        &KERNEL_START as *const u32, &KERNEL_END as *const u32
    );

    let v = vec![1, 2, 3, 4, 5];
    println!("{:?}", v);

    unsafe {
        multiboot::print_mmap_sections(info);
    }

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
