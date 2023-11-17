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
mod game;
mod gdt;
#[macro_use]
mod interrupts;
mod acpi;
mod cursor;
mod framebuffer;
mod io;
mod libc;
mod mouse;
mod multiboot2;
mod multiprocessing;
mod net;
mod rng;
mod rtl8139;
mod sleep;
mod time;
mod usb;
mod util;

use acpi::MadtEntry;
use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    sync::Arc,
    vec,
    vec::Vec,
};
use multiboot2::Multiboot2;
use multiprocessing::Apic;

use core::{
    arch::global_asm,
    cell::UnsafeCell,
    future::Future,
    mem::MaybeUninit,
    panic::PanicInfo,
    pin::Pin,
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};
use hashbrown::HashMap;

use crate::{
    acpi::AcpiTable,
    cursor::Cursor,
    framebuffer::FrameBuffer,
    future::{Either, Executor},
    interrupts::{InitInterruptError, InterruptHandlerData},
    io::{
        io_allocator::IoAllocator,
        pci::{Pci, PciDevice},
        ps2::Ps2Keyboard,
        rtc::Rtc,
        serial::Serial,
    },
    mouse::Mouse,
    multiprocessing::CpuFnDispatcher,
    net::{
        tcp::Tcp, ArpFrame, ArpFrameParams, ArpOperation, EtherType, EthernetFrameParams,
        ParsedIpv4Frame, ParsedPacket, UnknownArpOperation,
    },
    rng::Rng,
    rtl8139::Rtl8139,
    sleep::{WakeupRequester, WakeupService},
    time::MonotonicTime,
    usb::{uhci::Uhci, Usb, UsbDescriptor},
    util::async_mutex::Mutex,
    util::interrupt_guard::InterruptGuarded,
    util::spinlock::SpinLock,
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
    serial: Arc<Serial>,
    interrupt_handlers: &'static InterruptHandlerData,
}

unsafe fn interrupt_guarded_init(
    info: &Multiboot2,
) -> Result<EarlyInitHandles, InitInterruptError> {
    let _guard = InterruptGuarded::new(());
    let _guard = _guard.lock();

    allocator::init(info);
    logger::init(Default::default());
    let mut io_allocator = io::io_allocator::IoAllocator::new();
    let serial = Arc::new(Serial::new(&mut io_allocator).expect("Failed to initialize serial"));

    struct SerialWriter {
        serial: Arc<Serial>,
    }

    impl core::fmt::Write for SerialWriter {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            self.serial.write_str(s);
            Ok(())
        }
    }

    io::init_stdio(Box::new(SerialWriter {
        serial: Arc::clone(&serial),
    }));

    gdt::init();

    let interrupt_handlers = interrupts::init(&mut io_allocator)?;

    Ok(EarlyInitHandles {
        io_allocator,
        serial,
        interrupt_handlers,
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
    cpu_dispatcher: CpuFnDispatcher,
    io_allocator: IoAllocator,
    interrupt_handlers: &'static InterruptHandlerData,
    rng: Mutex<Rng>,
    rtc: Rtc,
    pci: Pci,
    ps2: SpinLock<Ps2Keyboard>,
    rtl8139: Rtl8139,
    usb: Usb,
    arp_table: ArpTable,
    serial: Arc<Serial>,
    framebuffer: FrameBuffer,
    cursor: Cursor,
    tcp: Tcp,
    monotonic_time: Arc<MonotonicTime>,
    wakeup_requester: WakeupRequester,
    wakeup_service: WakeupService,
}

impl Kernel {
    unsafe fn init(multiboot_magic: u32, info: *const u8) -> Result<Kernel, InitInterruptError> {
        let info = Multiboot2::new(multiboot_magic, info);

        let EarlyInitHandles {
            mut io_allocator,
            serial,
            interrupt_handlers,
        } = interrupt_guarded_init(&info)?;

        let monotonic_time = Arc::new(MonotonicTime::new(Rtc::tick_freq()));
        let (wakeup_requester, wakeup_service, mut interrupt_wakeups) =
            sleep::construct_wakeup_handlers();
        io::init_late(&mut io_allocator);

        let on_tick = {
            let monotonic_time = Arc::clone(&monotonic_time);

            move || {
                let tick = monotonic_time.increment();
                interrupt_wakeups.wakeup_if_neccessary(tick);
            }
        };

        interrupt_handlers
            .register(
                interrupts::IrqId::Internal(multiprocessing::WAKEUP_IRQ_ID),
                || {},
            )
            .expect("Failed to register empty interrupt handler");

        let mut rtc = io::rtc::Rtc::new(&mut io_allocator, interrupt_handlers, on_tick)
            .expect("Failed to construct rtc");

        let mut pci = Pci::new(&mut io_allocator).expect("Failed to initialize pci");

        let pci_devices: Vec<_> = pci
            .devices()
            .filter_map(|device| match device {
                Ok(PciDevice::General(device)) => Some(device),
                _ => None,
            })
            .collect();

        let mut rtl8139 = None;
        let mut uhci = None;

        for mut device in pci_devices {
            let id = device.id(&mut pci);
            let interface_id = device.interface_id(&mut pci);

            debug!(
                "PCI device: {:?} with id {:#x}, {:#x} and interface: {:?}",
                device, id.0, id.1, interface_id
            );

            if id == Rtl8139::PCI_ID {
                rtl8139 = Some(
                    Rtl8139::new(device, &mut pci, interrupt_handlers, false)
                        .expect("Failed to initialize rtl8139"),
                );
            } else if interface_id.class == 0x0c
                && interface_id.subclass == 0x03
                && interface_id.interface == 0x00
            {
                uhci = Some(Uhci::new(
                    device,
                    &mut io_allocator,
                    &mut pci,
                    Arc::clone(&monotonic_time),
                    wakeup_requester.clone(),
                    interrupt_handlers,
                ));
            }
        }

        let rtl8139 = rtl8139.expect("Failed to find pci device id for rtl8139");
        let uhci = uhci.expect("Failed to find uhci controller");

        let usb = Usb::new(uhci);

        let arp_table = ArpTable::new();
        let rng = Mutex::new(Rng::new(rtc.read().unwrap().seconds as u64));
        let tcp = Tcp::new(Arc::clone(&monotonic_time), wakeup_requester.clone());

        let framebuffer_info = info
            .get_framebuffer_info()
            .expect("Failed to get framebuffer info");

        let framebuffer = FrameBuffer::new(framebuffer_info);

        let ps2 = SpinLock::new(Ps2Keyboard::new(&mut io_allocator, interrupt_handlers));

        let rsdp = info.get_rsdp().expect("Failed to get rsdp");
        if !rsdp.validate_checksum() {
            panic!("Invalid rdsp");
        }

        let rsdt = rsdp.rsdt();

        let madt = rsdt
            .iter()
            .find_map(|item| match item.upgrade() {
                AcpiTable::Madt(madt) => Some(madt),
                _ => None,
            })
            .expect("Failed to find madt");

        let mut apic = Apic::new(madt.local_apic_addr());
        multiprocessing::boot_all_cpus(
            &mut apic,
            madt.entries().map(|x| {
                let MadtEntry::LocalApic { apic_id, .. } = x;
                apic_id
            }),
            &monotonic_time,
        );

        let cpu_dispatcher =
            CpuFnDispatcher::new(apic).expect("Cpu dispatcher construction failed");

        let cursor = Cursor::new();
        Ok(Kernel {
            cpu_dispatcher,
            interrupt_handlers,
            io_allocator,
            rtc,
            rng,
            pci,
            ps2,
            arp_table,
            rtl8139,
            cursor,
            usb,
            serial,
            tcp,
            framebuffer,
            monotonic_time,
            wakeup_service,
            wakeup_requester,
        })
    }

    unsafe fn demo(&mut self) {
        let init_demo = async {
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
        };

        let send_udp = async {
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

            let sleep_fut = sleep::sleep(1.0, &self.monotonic_time, &self.wakeup_requester);
            let sleep_fut = core::pin::pin!(sleep_fut);
            let arp_lookup = self.arp_table.wait_for(&REMOTE_IP);
            let arp_lookup = core::pin::pin!(arp_lookup);

            let mac = match crate::future::select(arp_lookup, sleep_fut).await {
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
        };

        let echo_tcp = async {
            let listener = self.tcp.listen(STATIC_IP, 80).await;
            loop {
                let connection = listener.connection().await;
                let data = connection.read().await;

                info!(
                    "Received TCP data: \"{}\" on cpu {}",
                    core::str::from_utf8_unchecked(&data),
                    multiprocessing::cpuid()
                );

                match handle_http_request(&data) {
                    Ok(response) => {
                        connection.write(response.to_string().into_bytes()).await;
                    }
                    Err(_) => {
                        connection
                            .write(
                                "HTTP/1.1 500 Internal servrer error\r\n\
                            Content-Length: 0
                            \r\n\
                            \r\n"
                                    .to_string()
                                    .into_bytes(),
                            )
                            .await;
                    }
                }
            }
        };

        let tcp_service = async {
            loop {
                let outgoing_data = self.tcp.service().await;
                let ipv4_frame = net::generate_ipv4_frame(
                    &outgoing_data.payload,
                    net::Ipv4Protocol::Tcp,
                    &outgoing_data.local_ip,
                    &outgoing_data.remote_ip,
                );

                // FIXME: Generate arp request if needed?
                let ethernet_frame = net::generate_ethernet_frame(&EthernetFrameParams {
                    dest_mac: self.arp_table.wait_for(&outgoing_data.remote_ip).await,
                    source_mac: self.rtl8139.get_mac(),
                    ether_type: EtherType::Ipv4,
                    payload: &ipv4_frame,
                });

                self.rtl8139.write(&ethernet_frame).await.unwrap();
            }
        };

        let recv = async {
            recv_loop(&self.rtl8139, &self.arp_table, &self.tcp, &self.rng).await;
        };

        //let mouse_move_tx = self.cursor.get_movement_writer();
        let (mouse_move_tx, mouse_move_rx) = util::async_channel::channel();

        let doom = async {
            doomgeneric_Create(0, core::ptr::null());
            let tick_freq = 1.0 / self.monotonic_time.tick_freq();

            loop {
                let start = self.monotonic_time.get();

                let mut movement = cursor::Movement { x: 0.0, y: 0.0 };
                loop {
                    let this_movement = mouse_move_rx.try_recv().await;
                    let this_movement: cursor::Movement = match this_movement {
                        Some(v) => v,
                        None => break,
                    };
                    movement.x += this_movement.x;
                    movement.y += this_movement.y;
                }
                doomgeneric_PushMouse(movement.x, movement.y);
                doomgeneric_Tick();
                let end = self.monotonic_time.get();
                crate::sleep::sleep(
                    0.033 - ((end - start) as f32 * tick_freq),
                    &self.monotonic_time,
                    &self.wakeup_requester,
                )
                .await;
            }
        };

        //let mut game = game::Game::new(
        //    &mut self.framebuffer,
        //    &mut self.ps2,
        //    &self.monotonic_time,
        //    &self.wakeup_requester,
        //    self.cursor.get_pos_reader(),
        //);

        let device_rx = self.usb.device_channel();
        let usb_handle = self.usb.handle();
        let usb_driver_dispatch = async {
            loop {
                let device = device_rx.recv().await;
                let descriptors = usb_handle
                    .get_configuration_descriptors(device.address)
                    .await;
                for descriptor in &descriptors {
                    if let UsbDescriptor::Interface(intf) = descriptor {
                        if intf.interface_class() == 3
                            && intf.interface_subclass() == 1
                            && intf.interface_protocol() == 2
                        {
                            info!("Found HID mouse with boot protocol support");
                            // Check if device is a mouse??
                            let mut mouse =
                                Mouse::new(device, usb_handle.clone(), mouse_move_tx.clone());
                            mouse.service().await;
                            break;
                        }
                    }
                }
            }
        };

        let mut executor = Executor::new(Some(&self.cpu_dispatcher));
        executor.spawn(logger::service());
        executor.spawn(init_demo);
        executor.spawn(recv);
        executor.spawn(echo_tcp);
        executor.spawn(tcp_service);
        executor.spawn(send_udp);
        //executor.spawn(game.run());
        executor.spawn(self.wakeup_service.service());
        executor.spawn(self.rtl8139.service());
        executor.spawn(self.cpu_dispatcher.service());
        executor.spawn(self.usb.service());
        executor.spawn(usb_driver_dispatch);
        executor.spawn(self.cursor.service());
        executor.spawn(doom);
        executor.run();

        info!("And now we exit/halt");
    }
}

struct IncompleteHttpRequest;

struct HttpResponse {
    headers: HashMap<String, String>,
    body: String,
}

impl HttpResponse {
    fn new(body: String, content_type: String) -> HttpResponse {
        let mut headers = HashMap::new();
        headers.insert("Content-Length".to_string(), body.len().to_string());
        headers.insert("Content-Type".to_string(), content_type);
        headers.insert("Connection".to_string(), "close".into());

        HttpResponse { headers, body }
    }
}

impl core::fmt::Display for HttpResponse {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "HTTP/1.1 200 OK\r\n")?;
        for (key, value) in &self.headers {
            write!(f, "{}: {}\r\n", key, value)?;
        }
        write!(f, "\r\n{}", self.body)?;
        Ok(())
    }
}

fn handle_http_request(data: &[u8]) -> Result<HttpResponse, IncompleteHttpRequest> {
    let first_line_end = data
        .windows(2)
        .position(|d| d == b"\r\n")
        .ok_or(IncompleteHttpRequest)?;

    let request_line = &data[..first_line_end];

    let uri = request_line
        .split(|b| *b == b' ')
        .nth(1)
        .ok_or(IncompleteHttpRequest)?;

    let mut uri_items = uri.splitn(2, |b| *b == b'?');

    let path = uri_items
        .next()
        .expect("Should always get at least 1 element");
    let params = uri_items.next();

    let ret = match path {
        b"/" => HttpResponse::new(
            include_str!("../res/index.html").to_string(),
            "text/html".into(),
        ),
        b"/form" => unsafe {
            HttpResponse::new(
                format!(
                    "Got form request with params: {}",
                    core::str::from_utf8_unchecked(params.unwrap())
                ),
                "text/html".into(),
            )
        },
        _ => HttpResponse::new("Uh oh".to_string(), "text/html".into()),
    };

    Ok(ret)
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
async fn handle_packet(
    packet: Vec<u8>,
    rtl8139: &Rtl8139,
    mac: &[u8; 6],
    arp_table: &ArpTable,
    tcp: &Tcp,
    rng: &Mutex<Rng>,
) {
    let packet = net::parse_packet(&packet);

    let packet = match packet {
        Ok(v) => v,
        Err(e) => {
            debug!("Received invalid packet: {:?}", e);
            return;
        }
    };

    match packet.inner {
        ParsedPacket::Arp(arp_frame) => {
            handle_arp_frame(&arp_frame, rtl8139, mac, arp_table).await;
        }
        ParsedPacket::Ipv4(ipv4_frame) => {
            debug!("Received IPV4 frame");
            let frame = net::parse_ipv4(&ipv4_frame);
            match frame {
                Ok(ParsedIpv4Frame::Udp(udp_frame)) => {
                    unsafe {
                        debug!(
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
                Ok(ParsedIpv4Frame::Tcp(tcp_frame)) => {
                    //if rng.lock().await.normalized() < 0.1 {
                    //    info!("Dropping packet");
                    //    return
                    //}
                    let response_tcp_frame = tcp
                        .handle_frame(&tcp_frame, &ipv4_frame.source_ip(), &STATIC_IP, rng)
                        .await;
                    if let Some(response_tcp_frame) = response_tcp_frame {
                        let response_ipv4_frame = net::generate_ipv4_frame(
                            &response_tcp_frame,
                            net::Ipv4Protocol::Tcp,
                            &STATIC_IP,
                            &ipv4_frame.source_ip(),
                        );

                        let response_ethernet_frame =
                            net::generate_ethernet_frame(&EthernetFrameParams {
                                dest_mac: packet
                                    .ethernet
                                    .source_mac()
                                    .try_into()
                                    .expect("invalid source mac length"),
                                source_mac: rtl8139.get_mac(),
                                ether_type: EtherType::Ipv4,
                                payload: &response_ipv4_frame,
                            });

                        rtl8139.write(&response_ethernet_frame).await.unwrap();
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

async fn recv_loop(rtl8139: &Rtl8139, arp_table: &ArpTable, tcp: &Tcp, rng: &Mutex<Rng>) {
    let mac = rtl8139.get_mac();

    loop {
        debug!("Waiting for a packet");
        rtl8139
            .read(|packet| {
                // FIXME: Avoid copying but types are hard
                handle_packet(packet.to_vec(), rtl8139, &mac, arp_table, tcp, rng)
            })
            .await;
    }
}

#[cfg(test)]
async unsafe fn test_and_wait(monotonic_time: Arc<MonotonicTime>) {
    test_main();

    let start = monotonic_time.get();
    // t * t/s
    let end = (start as f32 + 0.1 / monotonic_time.tick_freq()) as usize;

    struct BusyWait {
        monotonic_time: Arc<MonotonicTime>,
        end: usize,
    }

    impl core::future::Future for BusyWait {
        type Output = ();

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            if self.monotonic_time.get() < self.end {
                cx.waker().wake_by_ref();
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        }
    }

    BusyWait {
        monotonic_time: Arc::clone(&monotonic_time),
        end,
    }
    .await;

    io::exit(0);
}

extern "C" {
    static DG_ScreenBuffer: *mut u32;
    fn doomgeneric_Create(argc: i32, argv: *const *const u8);
    fn doomgeneric_PushMouse(x: f32, y: f32);
    fn doomgeneric_Tick();
    fn test_printf();
    fn print_long_size();
}

pub struct StaticKernel(UnsafeCell<MaybeUninit<Kernel>>);
unsafe impl Sync for StaticKernel {}

impl StaticKernel {
    fn get<'a, 'b>(&'a self) -> &'b mut Kernel {
        unsafe {
            let maybe_unint_kernel = &mut *self.0.get();
            maybe_unint_kernel.assume_init_mut()
        }
    }
}

static KERNEL: StaticKernel = StaticKernel(UnsafeCell::new(MaybeUninit::uninit()));

#[no_mangle]
pub unsafe extern "C" fn DG_GetTicksMs() -> u32 {
    let kernel = KERNEL.get();
    let tick = kernel.monotonic_time.get();
    let tick_freq = kernel.monotonic_time.tick_freq();
    let ticks_ms_f = (1.0 / tick_freq) /* s / tick */ * tick as f32 /* tick */ * 1000.0 /* s -> ms */;
    ticks_ms_f as u32
}

#[no_mangle]
pub unsafe extern "C" fn DG_DrawFrame() {
    let kernel = KERNEL.get();

    let res_x = 640;
    let res_y = 400;

    let mut line_start = kernel.framebuffer.info.addr;
    let mut from_ptr = DG_ScreenBuffer as *mut u8;
    for _ in 0..res_y {
        line_start.copy_from(from_ptr, res_x * 4);

        from_ptr = from_ptr.add(res_x * 4);
        line_start = line_start.add(kernel.framebuffer.info.pitch as usize)
    }
}

const RAW_WAKER_VTABLE: RawWakerVTable =
    RawWakerVTable::new(clone, null_future_fn, null_future_fn, null_future_fn);

unsafe fn clone(_: *const ()) -> RawWaker {
    RawWaker::new(core::ptr::null(), &RAW_WAKER_VTABLE)
}

unsafe fn null_future_fn(data: *const ()) {}

#[no_mangle]
pub unsafe extern "C" fn DG_GetKey(pressed: *mut i32, doom_key: *mut u8) -> i32 {
    let kernel = KERNEL.get();

    let mut ps2 = kernel.ps2.lock();
    let read_fut = ps2.read();
    let mut read_fut = core::pin::pin!(read_fut);

    let waker = RawWaker::new(core::ptr::null(), &RAW_WAKER_VTABLE);
    let waker = Waker::from_raw(waker);
    let mut context = Context::from_waker(&waker);
    let scancode: u8 = match read_fut.poll(&mut context) {
        Poll::Ready(v) => v,
        Poll::Pending => {
            return 0;
        }
    };

    println!("Got scancode: {}", scancode);

    const DOOM_KEY_RIGHTARROW: u8 = 0xae;
    const DOOM_KEY_LEFTARROW: u8 = 0xac;
    const DOOM_KEY_UPARROW: u8 = 0xad;
    const DOOM_KEY_DOWNARROW: u8 = 0xaf;
    const DOOM_KEY_STRAFE_L: u8 = 0xa0;
    const DOOM_KEY_STRAFE_R: u8 = 0xa1;
    const DOOM_KEY_USE: u8 = 0xa2;
    const DOOM_KEY_FIRE: u8 = 0xa3;
    const DOOM_KEY_ESCAPE: u8 = 27;
    const DOOM_KEY_ENTER: u8 = 13;

    const W_DOWN: u8 = 0x11;
    const A_DOWN: u8 = 0x1e;
    const S_DOWN: u8 = 0x1f;
    const D_DOWN: u8 = 0x20;
    const E_DOWN: u8 = 0x12;
    const CTRL_DOWN: u8 = 0x1d;
    const ENTER_DOWN: u8 = 0x1c;
    const ESC_DOWN: u8 = 0x01;

    *doom_key = match scancode & 0x7f {
        W_DOWN => DOOM_KEY_UPARROW,
        A_DOWN => DOOM_KEY_STRAFE_L,
        S_DOWN => DOOM_KEY_DOWNARROW,
        D_DOWN => DOOM_KEY_STRAFE_R,
        E_DOWN => DOOM_KEY_USE,
        CTRL_DOWN => DOOM_KEY_FIRE,
        ENTER_DOWN => DOOM_KEY_ENTER,
        ESC_DOWN => DOOM_KEY_ESCAPE,
        _ => 0,
    };

    *pressed = 1 - (scancode >> 7) as i32;
    println!("{:#x}, {}", scancode, *pressed);

    return 1;
}

#[no_mangle]
pub unsafe extern "C" fn kernel_main(multiboot_magic: u32, info: *const u8) -> i32 {
    (*KERNEL.0.get()) =
        MaybeUninit::new(Kernel::init(multiboot_magic, info).expect("Failed to initialize kernel"));

    //test_printf();
    //
    //print_long_size();
    //#[cfg(test)]
    //{
    //    let mut executor = Executor::new(None);
    //    executor.spawn(logger::service());
    //    executor.spawn(test_and_wait(Arc::clone(&kernel.monotonic_time)));
    //    executor.run();
    //}

    KERNEL.get().demo();

    io::exit(0);
    0
}

#[no_mangle]
pub unsafe extern "C" fn panic_c(s: *const i8) {
    let s = core::ffi::CStr::from_ptr(s);
    panic!("{}", s.to_str().unwrap());
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
