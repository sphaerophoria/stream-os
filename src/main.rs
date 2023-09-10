#![allow(bad_asm_style, clippy::missing_safety_doc)]
#![feature(panic_info_message)]
#![no_std] // don't link the Rust standard library
#![no_main] // disable all Rust-level entry points

extern crate alloc;

#[macro_use]
mod io;
mod allocator;
mod libc;
mod multiboot;

use alloc::{vec, vec::Vec};
use core::sync::atomic::Ordering;
use io::vga::TerminalWriter;
use multiboot::MultibootInfo;

use core::{arch::global_asm, panic::PanicInfo};

use crate::io::serial::Serial;

#[global_allocator]
static ALLOC: allocator::Allocator = allocator::Allocator::new();

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
    TerminalWriter::init();
    Serial::init().expect("Failed to initialize serial");

    println!(
        "Kernel start {:?}, Kernel end {:?}",
        &KERNEL_START as *const u32, &KERNEL_END as *const u32
    );

    ALLOC.init(&*info);

    let initial_state = *ALLOC.first_free.load(Ordering::Relaxed);

    {
        let mut v = Vec::new();
        const NUM_ALLOCATIONS: usize = 10;
        // Allocating a bunch of shit
        for i in 1..NUM_ALLOCATIONS {
            let mut v2 = Vec::new();
            for j in 0..i {
                v2.push(j);
            }
            v.push(v2);
        }

        // Creating holes in allocations
        for i in (0..NUM_ALLOCATIONS - 1).filter(|x| (x % 2) == 0).rev() {
            let len = v.len() - 1;
            v.swap(len, i);
            v.pop();
        }

        // alloc and dealloc again
        {
            let mut v = Vec::new();
            for i in 1..NUM_ALLOCATIONS {
                let mut v2 = Vec::new();
                for j in 0..i {
                    v2.push(j);
                }
                v.push(v2);
            }
        }

        println!("Pre-dealloc");
        allocator::print_all_free_segments(ALLOC.first_free.load(Ordering::Relaxed));

        // Checking for memory corruption
        for elem in v {
            for (i, item) in elem.into_iter().enumerate() {
                assert_eq!(i, item);
            }
        }
    }

    println!("Post-dealloc");
    allocator::print_all_free_segments(ALLOC.first_free.load(Ordering::Relaxed));
    assert_eq!(*ALLOC.first_free.load(Ordering::Relaxed), initial_state);

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
