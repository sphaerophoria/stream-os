#![allow(clippy::missing_safety_doc)]
#![feature(panic_info_message)]
#![feature(concat_idents)]
#![feature(abi_x86_interrupt)]
#![feature(maybe_uninit_uninit_array)]
#![feature(const_maybe_uninit_uninit_array)]
#![feature(core_intrinsics)]
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
mod rng;
mod rtl8139;
mod sleep;
mod time;
mod util;

use alloc::{boxed::Box, rc::Rc, vec, vec::Vec};
use futures::future::Either;

use core::{
    arch::global_asm,
    cell::RefCell,
    fmt::Write,
    panic::PanicInfo,
    pin::Pin,
    task::{Context, Poll},
};
use hashbrown::HashMap;

use crate::{
    future::execute_fut,
    interrupts::{InitInterruptError, InterruptHandlerData},
    io::{
        io_allocator::IoAllocator, pci::Pci, rtc::Rtc, serial::Serial, vga::TerminalWriter,
        PrinterFunction,
    },
    multiboot::MultibootInfo,
    net::{
        ArpFrame, ArpFrameParams, ArpOperation, EtherType, EthernetFrameParams, ParsedIpv4Frame,
        ParsedPacket, UnknownArpOperation,
    },
    rng::Rng,
    rtl8139::Rtl8139,
    sleep::WakeupList,
    time::MonotonicTime,
    util::async_mutex::Mutex,
    util::interrupt_guard::InterruptGuarded,
};

// Include boot.s which defines _start as inline assembly in main. This allows us to do more fine
// grained setup than if we used a naked _start function in rust. Theoretically we could use a
// naked function + some inline asm, but this seems much more straight forward.
global_asm!(include_str!("boot.s"), options(att_syntax));

const STATIC_IP: [u8; 4] = [192, 168, 2, 2];

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

// FIXME: Ip address should be strong typed
type IpAddr = [u8; 4];
type MacAddr = [u8; 6];

struct ArpReadyFuture<'a> {
    ip: &'a IpAddr,
    table: &'a Mutex<HashMap<IpAddr, MacAddr>>,
}

impl<'a> core::future::Future for ArpReadyFuture<'a> {
    type Output = MacAddr;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let guard = core::pin::pin!(self.table.lock());
        let guard = match guard.poll(cx) {
            Poll::Ready(v) => v,
            Poll::Pending => {
                return Poll::Pending;
            }
        };

        match guard.get(self.ip) {
            Some(v) => Poll::Ready(*v),
            None => Poll::Pending,
        }
    }
}

struct ArpTable {
    table: Mutex<HashMap<IpAddr, MacAddr>>,
}

impl ArpTable {
    fn new() -> ArpTable {
        let table = Mutex::new(HashMap::new());
        ArpTable { table }
    }

    async fn write_mac(&self, ip: &IpAddr, mac: &MacAddr) {
        let mut table = self.table.lock().await;
        table.insert(*ip, *mac);
    }

    async fn wait_for(&self, ip: &[u8; 4]) -> [u8; 6] {
        ArpReadyFuture {
            ip,
            table: &self.table,
        }
        .await
    }
}

#[allow(unused)]
struct Kernel {
    io_allocator: IoAllocator,
    interrupt_handlers: &'static InterruptHandlerData,
    rng: Mutex<Rng>,
    rtc: Rtc,
    pci: Pci,
    rtl8139: Rtl8139,
    arp_table: ArpTable,
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

        let arp_table = ArpTable::new();
        let rng = Mutex::new(Rng::new());

        Ok(Kernel {
            interrupt_handlers,
            io_allocator,
            rtc,
            rng,
            pci,
            arp_table,
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
        date.hours = (date.hours + 1) % 24;
        self.rtc.write(&date).expect("failed to write rtc date");

        let date = self.rtc.read().expect("failed to read date");
        info!("Current date modified in cmos: {:?}", date);

        self.rtl8139.log_mac().await;

        let outgoing = async {
            let mac = self.rtl8139.get_mac();
            const REMOTE_IP: [u8; 4] = [192, 168, 2, 1];
            let arp_frame: Vec<u8> = net::generate_arp_request(&REMOTE_IP, &STATIC_IP, &mac);
            let ethernet_frame = net::generate_ethernet_frame(&EthernetFrameParams {
                dest_mac: [0xff; 6],
                source_mac: mac,
                ether_type: EtherType::Arp,
                payload: &arp_frame,
            });
            self.rtl8139.write(&ethernet_frame).await.unwrap();

            let sleep_fut = sleep::sleep(1.0, &self.monotonic_time, &self.wakeup_list);
            let sleep_fut = core::pin::pin!(sleep_fut);
            let arp_lookup = self.arp_table.wait_for(&REMOTE_IP);
            let arp_lookup = core::pin::pin!(arp_lookup);

            let mac = match futures::future::select(arp_lookup, sleep_fut).await {
                Either::Left((mac, _)) => mac,
                Either::Right(_) => {
                    warn!("ARP lookup for {:?} failed", REMOTE_IP);
                    return;
                }
            };

            info!("Resolved mac address!: {:?}", mac);

            let udp_frame = net::generate_udp_frame(6000, b"hello from inside the os\n");
            let ipv4_frame = net::generate_ipv4_frame(
                &udp_frame,
                net::Ipv4Protocol::Udp,
                &STATIC_IP,
                &REMOTE_IP,
            );
            let ethernet_frame = net::generate_ethernet_frame(&EthernetFrameParams {
                dest_mac: mac,
                source_mac: self.rtl8139.get_mac(),
                ether_type: EtherType::Ipv4,
                payload: &ipv4_frame,
            });

            self.rtl8139.write(&ethernet_frame).await.unwrap();

            info!("Sleeping for 5 seconds to wait for incoming connections");
            sleep::sleep(5.0, &self.monotonic_time, &self.wakeup_list).await;
        };

        let recv = async {
            recv_loop(&self.rtl8139, &self.arp_table).await;
        };
        let recv = core::pin::pin!(recv);

        let outgoing = core::pin::pin!(outgoing);
        futures::future::select(recv, outgoing).await;

        info!("And now we exit/halt");
    }
}

async fn handle_arp_frame(
    arp_frame: &ArpFrame<'_>,
    rtl8139: &Rtl8139,
    mac: &[u8; 6],
    arp_table: &ArpTable,
) {
    debug!("Received arp frame: {:?}", arp_frame);

    match arp_frame.operation() {
        Ok(ArpOperation::Request) => (),
        Ok(ArpOperation::Reply) => {
            let mac = arp_frame
                .sender_hardware_address()
                .try_into()
                .expect("Arp mac address not the right size");
            let ip = arp_frame
                .sender_protocol_address()
                .try_into()
                .expect("Arp ip address not the right size");
            arp_table.write_mac(&ip, &mac).await;
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
        ether_type: EtherType::Arp,
        payload: &response,
    });

    rtl8139.write(&response_frame).await.unwrap();
}

// FIXME: Where does this belong?
async fn handle_packet(packet: Vec<u8>, rtl8139: &Rtl8139, mac: &[u8; 6], arp_table: &ArpTable) {
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
            handle_arp_frame(&arp_frame, rtl8139, mac, arp_table).await;
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

async fn recv_loop(rtl8139: &Rtl8139, arp_table: &ArpTable) {
    let mac = rtl8139.get_mac();

    loop {
        info!("Waiting for a packet");
        rtl8139
            .read(|packet| {
                // FIXME: Avoid copying but types are hard
                handle_packet(packet.to_vec(), rtl8139, &mac, arp_table)
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

    futures::future::select(Box::pin(logger::service()), Box::pin(demo_fut)).await;
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
