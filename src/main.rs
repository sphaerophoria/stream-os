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
mod net;
mod rtl8139;
mod sleep;
mod time;
mod util;

use alloc::{boxed::Box, rc::Rc, vec, vec::Vec};

use core::{arch::global_asm, cell::RefCell, fmt::Write, panic::PanicInfo};

use crate::{
    future::execute_fut,
    interrupts::{InitInterruptError, InterruptHandlerData},
    io::{
        io_allocator::IoAllocator, pci::Pci, rtc::Rtc, serial::Serial, vga::TerminalWriter,
        PrinterFunction,
    },
    multiboot::MultibootInfo,
    net::{
        ArpFrame, ArpFrameParams, ArpOperation, EthernetFrameParams, ParsedIpv4Frame, ParsedPacket,
        UnknownArpOperation,
    },
    rtl8139::Rtl8139,
    sleep::WakeupList,
    time::MonotonicTime,
    util::interrupt_guard::InterruptGuarded,
};

// Include boot.s which defines _start as inline assembly in main. This allows us to do more fine
// grained setup than if we used a naked _start function in rust. Theoretically we could use a
// naked function + some inline asm, but this seems much more straight forward.
global_asm!(include_str!("boot.s"), options(att_syntax));

const STATIC_IP: [u8; 4] = [192, 168, 122, 55];

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

        let rtl8139 = Rtl8139::new(&mut pci, interrupt_handlers, false)
            .expect("Failed to initialize rtl8139");

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

        self.rtl8139.log_mac().await;

        info!("Waiting for UDP message for 3 seconds...");
        let recv = async {
            recv_loop(&self.rtl8139).await;
        };
        let recv = core::pin::pin!(recv);

        let sleep_fut = sleep::sleep(3.0, &self.monotonic_time, &self.wakeup_list);
        let sleep_fut = core::pin::pin!(sleep_fut);
        info!("Current date modified in cmos: {:?}", date);
        futures::future::select(recv, sleep_fut).await;

        info!("And now we exit/halt");
    }
}

async fn handle_arp_frame(arp_frame: &ArpFrame<'_>, rtl8139: &Rtl8139, mac: &[u8; 6]) {
    debug!("Received arp frame: {:?}", arp_frame);

    match arp_frame.operation() {
        Ok(ArpOperation::Request) => (),
        Ok(ArpOperation::Reply) => {
            debug!("Received arp reply, ignoring");
            return;
        }
        Err(UnknownArpOperation(v)) => {
            debug!("Received unknown arp operation, {}", v);
        }
    }

    if arp_frame.operation() != Ok(ArpOperation::Request) {
        return;
    }

    if arp_frame.target_hardware_address() != mac
        && arp_frame.target_protocol_address() != STATIC_IP
    {
        return;
    }

    let mut params =
        ArpFrameParams::try_from(arp_frame).expect("Arp frame should be validated above");

    core::mem::swap(
        &mut params.target_protocol_address,
        &mut params.sender_protocol_address,
    );
    core::mem::swap(
        &mut params.target_hardware_address,
        &mut params.sender_hardware_address,
    );
    params.operation = ArpOperation::Reply;
    params.sender_hardware_address = *mac;
    params.sender_protocol_address = STATIC_IP;

    let response = net::generate_arp_frame(&params);

    let response_frame = net::generate_ethernet_frame(&EthernetFrameParams {
        dest_mac: arp_frame
            .sender_hardware_address()
            .try_into()
            .expect("Invalid length for dest mac"),
        source_mac: *mac,
        // FIXME: enum with to_int() or something
        ether_type: 0x0806,
        payload: &response,
    });

    rtl8139.write(&response_frame).await.unwrap();
}

// FIXME: Where does this belong?
async fn handle_packet(packet: Vec<u8>, rtl8139: &Rtl8139, mac: &[u8; 6]) {
    let packet = net::parse_packet(&packet);

    let packet = match packet {
        Ok(v) => v,
        Err(e) => {
            debug!("Received invalid packet: {:?}", e);
            return;
        }
    };

    match packet {
        ParsedPacket::Arp(arp_frame) => {
            handle_arp_frame(&arp_frame, rtl8139, mac).await;
        }
        ParsedPacket::Ipv4(ipv4_frame) => {
            let frame = net::parse_ipv4(&ipv4_frame);
            match frame {
                Ok(ParsedIpv4Frame::Udp(udp_frame)) => {
                    unsafe {
                        info!(
                            "Received UDP message: {}",
                            core::str::from_utf8_unchecked(udp_frame.data())
                        );
                    }
                    if udp_frame.data() == b"exit\n" {
                        unsafe {
                            io::exit(0);
                        }
                    }
                }
                Ok(ParsedIpv4Frame::Unknown(p)) => {
                    debug!("Unknown ipv4 protocol {:?}", p);
                }
                Err(e) => {
                    debug!("Invalid ipv4 packet: {:?}", e);
                }
            }
        }
        ParsedPacket::Unknown(t) => {
            debug!("Found unknown packet type: {:#06x}", t);
        }
    }
}

async fn recv_loop(rtl8139: &Rtl8139) {
    let mac = rtl8139.get_mac();

    loop {
        info!("Waiting for a packet");
        rtl8139
            .read(|packet| {
                // FIXME: Avoid copying but types are hard
                handle_packet(packet.to_vec(), rtl8139, &mac)
            })
            .await;
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
