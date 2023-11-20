use crate::{
    interrupts::InterruptHandlerData,
    io::{
        io_allocator::{IoAllocator, IoOffset, IoRange},
        pci::{GeneralPciDevice, Pci},
    },
    sleep::WakeupRequester,
    time::MonotonicTime,
    util::{
        bit_manipulation::{GetBits, SetBits},
        lock_free_queue::{self, Sender},
    },
};

use super::{Pid, UsbPacket};

use alloc::{boxed::Box, collections::BTreeMap, sync::Arc, vec::Vec};
use core::{
    future::Future,
    task::{Context, Poll, Waker},
};

const USB_CMD_OFFSET: IoOffset = IoOffset::new(0);
const USB_STATUS_OFFSET: IoOffset = IoOffset::new(0x02);
const FRAME_NUMBER_OFFSET: IoOffset = IoOffset::new(0x06);
const FRAME_LIST_OFFSET: IoOffset = IoOffset::new(0x08);

struct UsbCmdReg {
    max_packet: bool,
    configure: bool,
    software_debug: bool,
    global_resume: bool,
    global_suspend: bool,
    global_reset: bool,
    host_controller_reset: bool,
    run: bool,
}

impl UsbCmdReg {
    fn to_u16(&self) -> u16 {
        let mut ret = 0u16;

        if self.max_packet {
            ret.set_bit(7, true);
        }

        if self.configure {
            ret.set_bit(6, true);
        }

        if self.software_debug {
            ret.set_bit(5, true);
        }

        if self.global_resume {
            ret.set_bit(4, true);
        }

        if self.global_suspend {
            ret.set_bit(3, true);
        }

        if self.global_reset {
            ret.set_bit(2, true);
        }

        if self.host_controller_reset {
            ret.set_bit(1, true);
        }

        if self.run {
            ret.set_bit(0, true);
        }

        ret
    }
}

struct UsbPortStatus(u16);

#[allow(unused)]
impl UsbPortStatus {
    fn suspend(&self) -> bool {
        self.0.get_bit(12)
    }

    fn set_suspend(&mut self, val: bool) {
        self.0.set_bit(12, val);
    }

    fn reset(&self) -> bool {
        self.0.get_bit(9)
    }

    fn set_reset(&mut self, val: bool) {
        self.0.set_bit(9, val)
    }

    fn low_speed(&self) -> bool {
        self.0.get_bit(8)
    }

    fn set_low_speed(&mut self, val: bool) {
        self.0.set_bit(8, val)
    }

    fn resume_detected(&self) -> bool {
        self.0.get_bit(6)
    }

    fn set_resume_detected(&mut self, val: bool) {
        self.0.set_bit(6, val)
    }

    fn line_status(&self) -> u8 {
        self.0.get_bits(4, 2) as u8
    }

    fn set_line_status(&mut self, val: u8) {
        assert!(val <= 0x3);
        self.0.set_bits(4, 2, val as u16)
    }

    fn port_enable_changed(&self) -> bool {
        self.0.get_bit(3)
    }

    fn set_port_enable_changed(&mut self, val: bool) {
        self.0.set_bit(3, val)
    }

    fn port_enabled(&self) -> bool {
        self.0.get_bit(2)
    }

    fn set_port_enabled(&mut self, val: bool) {
        self.0.set_bit(2, val)
    }

    fn connected_changed(&self) -> bool {
        self.0.get_bit(1)
    }

    fn set_connected_changed(&mut self, val: bool) {
        self.0.set_bit(1, val)
    }

    fn connected(&self) -> bool {
        self.0.get_bit(0)
    }

    fn set_connected(&mut self, val: bool) {
        self.0.set_bit(0, val)
    }
}

pub struct UhciFuture<'a> {
    buffers: &'a mut BTreeMap<u64, Box<TransferDescriptorStorage>>,
    waker_tx: &'a Sender<Waker>,
    ids: Vec<u64>,
}

impl Future for UhciFuture<'_> {
    type Output = Vec<Vec<u8>>;

    fn poll(mut self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        for id in &self.ids {
            let item = self.buffers.get(id).expect("Invalid ID for USB storage");
            unsafe {
                let status_word = (&item.descriptor.0[1] as *const u32).read_volatile();
                // If bit 23 is set, the usb hardware hasn't flagged this descriptor as serviced
                // yet
                if status_word.get_bit(23) {
                    self.waker_tx
                        .push(cx.waker().clone())
                        .expect("USB waker queue too short");

                    return Poll::Pending;
                }
            }
        }

        let self_unpin = &mut *self;
        let mut ret = Vec::new();
        for id in &self_unpin.ids {
            let mut buf = self_unpin.buffers.remove(id).expect("Failed to remove id");
            buf.hw_sync();
            ret.push(buf.buf);
        }

        Poll::Ready(ret)
    }
}

#[derive(Debug)]
pub struct InvalidPacketErr;

#[derive(Debug, Hash)]
struct TransferDescriptorID(usize);

pub struct Uhci {
    frame_list: Vec<u32>,
    io_range: IoRange,
    master_queue: Box<QueueStorage>,
    last_id: u64,
    time: Arc<MonotonicTime>,
    wakeup_requester: WakeupRequester,
    waker_tx: Sender<Waker>,
}

impl Uhci {
    pub fn new(
        mut device: GeneralPciDevice,
        io_allocator: &mut IoAllocator,
        pci: &mut Pci,
        time: Arc<MonotonicTime>,
        wakeup_requester: WakeupRequester,
        interrupt_handlers: &InterruptHandlerData,
    ) -> Uhci {
        // By default set the terminate bit on each frame, we will adjust them later maybe
        let mut frame_list = unsafe {
            let layout =
                alloc::alloc::Layout::from_size_align(1024 * 4, 4096).expect("Invalid layout");
            let frame_list = alloc::alloc::alloc(layout);
            Vec::from_raw_parts(frame_list as *mut u32, 1024, 1024)
        };

        let io_base = device
            .find_io_base(pci)
            .expect("Failed to find io_base for uhci") as u16;

        let io_range = io_allocator
            .request_io_range(io_base, 20)
            .expect("Failed to allocate IO range");

        let mut master_queue_head = QueueHead([0; 2]);
        master_queue_head.set_head_link(&LinkPointer::None);
        master_queue_head.set_element_link(&LinkPointer::None);

        let master_queue = Box::new(QueueStorage {
            queue: master_queue_head,
            bufs: BTreeMap::new(),
        });

        assert_eq!(frame_list.as_ptr() as u32 & 0xfff, 0);
        for elem in &mut frame_list {
            set_link_pointer(
                elem,
                &LinkPointer::QH(&master_queue.queue as *const QueueHead),
            );
        }

        let irq_num = device.get_irq_num(pci).unwrap();
        info!("Interrupt number for uhci card: {:?}", irq_num);
        info!("uhci io range: {:?}", io_range);

        let (waker_tx, mut waker_rx) = lock_free_queue::channel::<Waker>(50);

        interrupt_handlers
            .register(irq_num, move || {
                // NOTE: Not using the io_range abstraction because piping of single mutable writer is
                // too difficult, this status write should be atomic anyways
                unsafe {
                    let mut val: u16;
                    core::arch::asm!("
                                 in %dx, %ax
                                 ",
                                 in ("dx") io_base + 2,
                                 out ("ax") val,
                                 options(att_syntax));

                    if !val.get_bit(0) {
                        return;
                    }
                }

                while let Some(waker) = waker_rx.pop() {
                    waker.wake();
                }

                unsafe {
                    core::arch::asm!("
                                 out %ax, %dx
                                 ",
                                 in ("dx") io_base + 2,
                                 in ("ax") 1,
                                 options(att_syntax));
                }
            })
            .expect("Failed to register interrupt handler");

        Uhci {
            frame_list,
            io_range,
            master_queue,
            last_id: 0,
            time,
            wakeup_requester,
            waker_tx,
        }
    }

    // NOTE: Vec<Box> looks odd, however we need to ensure that TransferDescriptorStorage does not
    // move in memory
    #[allow(clippy::vec_box)]
    pub fn append_work(&mut self, work: Vec<UsbPacket>) -> UhciFuture<'_> {
        let mut work: Vec<_> = work
            .into_iter()
            .map(|item| generate_td(item).expect("Invalid packet"))
            .collect();
        chain_tds(&mut work);

        // FIXME: Stop the card from running while we push
        if let Some(td) = self.master_queue.bufs.last_entry() {
            let td = td.into_mut();
            if (td.descriptor.status() & 0x80) == 0 {
                self.master_queue.queue.set_element_link(&LinkPointer::TD(
                    &work[0].descriptor as *const TransferDescriptor,
                ));
            } else {
                td.descriptor.set_link_pointer(&LinkPointer::TD(
                    &work[0].descriptor as *const TransferDescriptor,
                ))
            }
        } else {
            self.master_queue.queue.set_element_link(&LinkPointer::TD(
                &work[0].descriptor as *const TransferDescriptor,
            ));
        }

        let ids: Vec<_> = (0..work.len()).map(|v| v as u64 + self.last_id).collect();
        self.last_id += work.len() as u64;

        let iter = ids.clone().into_iter().zip(work);

        self.master_queue.bufs.extend(iter);

        UhciFuture {
            buffers: &mut self.master_queue.bufs,
            waker_tx: &self.waker_tx,
            ids,
        }
    }

    async fn reset(&mut self) {
        let reset_cmd = UsbCmdReg {
            max_packet: false,
            configure: false,
            software_debug: false,
            global_resume: false,
            global_suspend: false,
            global_reset: true,
            host_controller_reset: false,
            run: false,
        }
        .to_u16();

        debug!("Writing usb reset");
        self.io_range
            .write_16(USB_CMD_OFFSET, reset_cmd)
            .expect("Invalid offset for usb cmd");

        crate::sleep::sleep(0.01, &self.time, &self.wakeup_requester).await;

        let unreset_cmd = UsbCmdReg {
            max_packet: false,
            configure: false,
            software_debug: false,
            global_resume: false,
            global_suspend: false,
            global_reset: false,
            host_controller_reset: false,
            run: false,
        }
        .to_u16();

        debug!("Disabling usb reset");
        self.io_range
            .write_16(USB_CMD_OFFSET, unreset_cmd)
            .expect("Invalid offset for usb cmd");

        crate::sleep::sleep(0.05, &self.time, &self.wakeup_requester).await;

        let hostreset_cmd = UsbCmdReg {
            max_packet: false,
            configure: false,
            software_debug: false,
            global_resume: false,
            global_suspend: false,
            global_reset: false,
            host_controller_reset: true,
            run: false,
        }
        .to_u16();

        debug!("Resetting host");
        self.io_range
            .write_16(USB_CMD_OFFSET, hostreset_cmd)
            .expect("Invalid offset for usb cmd");
        crate::sleep::sleep(0.01, &self.time, &self.wakeup_requester).await;
    }

    fn set_frame_list_offset(&mut self) {
        debug!(
            "Writing frame list offset as {:?}",
            self.frame_list.as_ptr()
        );
        self.io_range
            .write_32(FRAME_LIST_OFFSET, self.frame_list.as_ptr() as u32)
            .expect("Failed to write frame list offset");
    }

    fn set_frame_number(&mut self, val: u16) {
        self.io_range
            .write_16(FRAME_NUMBER_OFFSET, val)
            .expect("Failed to write frame number offset");
    }

    fn clear_usb_status(&mut self) {
        self.io_range
            .write_16(USB_STATUS_OFFSET, 0x1f)
            .expect("Failed to clear status register");
    }

    fn enable_uhci_card(&mut self) {
        let cmd = UsbCmdReg {
            max_packet: true,
            configure: true,
            software_debug: false,
            global_resume: false,
            global_suspend: false,
            global_reset: false,
            host_controller_reset: false,
            run: true,
        }
        .to_u16();

        debug!("Enabling card");
        self.io_range
            .write_16(USB_CMD_OFFSET, cmd)
            .expect("Invalid offset for usb cmd");
    }

    pub async fn reset_port(&mut self, port_offset: IoOffset) -> bool {
        let mut val = UsbPortStatus(
            self.io_range
                .read_16(port_offset)
                .expect("Failed to read port status"),
        );
        val.set_reset(true);

        self.io_range
            .write_16(port_offset, val.0)
            .expect("Failed to write port status");
        crate::sleep::sleep(0.05, &self.time, &self.wakeup_requester).await;

        let mut val = UsbPortStatus(
            self.io_range
                .read_16(port_offset)
                .expect("Failed to read port status"),
        );
        // Avoid clearing connection change bit
        // https://github.com/fysnet/FYSOS/blob/9fea9ca93a2600afdac3060e8c45b4678998abe8/main/usb/utils/gdevdesc/gd_uhci.c#L291
        val.set_connected_changed(false);
        val.set_port_enabled(false);
        val.set_port_enable_changed(false);
        val.set_resume_detected(false);
        val.set_low_speed(false);
        val.set_reset(false);
        self.io_range
            .write_16(port_offset, val.0)
            .expect("Failed to write port status");

        crate::sleep::sleep(0.005, &self.time, &self.wakeup_requester).await;

        let mut val = UsbPortStatus(
            self.io_range
                .read_16(port_offset)
                .expect("Failed to read port status"),
        );
        val.set_connected_changed(true);
        self.io_range
            .write_16(port_offset, val.0)
            .expect("Failed to write port status");

        val.set_port_enabled(true);
        self.io_range
            .write_16(port_offset, val.0)
            .expect("Failed to write port status");

        crate::sleep::sleep(0.005, &self.time, &self.wakeup_requester).await;

        let val = UsbPortStatus(
            self.io_range
                .read_16(port_offset)
                .expect("Failed to read port status"),
        );
        val.port_enabled() && val.connected()
    }

    fn enable_interrupts(&mut self) {
        self.io_range.write_16(IoOffset::new(4), 1 << 2).unwrap();
    }

    pub async fn init(&mut self) {
        self.reset().await;
        self.set_frame_list_offset();
        self.set_frame_number(0);
        self.clear_usb_status();
        self.enable_uhci_card();
        self.enable_interrupts();
    }
}

#[repr(align(16))]
struct QueueHead([u32; 2]);

#[allow(unused)]
impl QueueHead {
    fn set_head_link(&mut self, val: &LinkPointer) {
        set_link_pointer(&mut self.0[0], val)
    }

    fn head_link(&self) -> LinkPointer {
        get_link_pointer(self.0[0])
    }

    fn set_element_link(&mut self, val: &LinkPointer) {
        set_link_pointer(&mut self.0[1], val)
    }

    fn element_link(&self) -> LinkPointer {
        get_link_pointer(self.0[1])
    }
}

#[derive(Debug, Eq, PartialEq)]
enum LinkPointer {
    TD(*const TransferDescriptor),
    QH(*const QueueHead),
    None,
}

struct TransferDescriptorStorage {
    descriptor: TransferDescriptor,
    buf: Vec<u8>,
}

impl TransferDescriptorStorage {
    fn hw_sync(&mut self) {
        for item in &mut self.descriptor.0 {
            unsafe {
                *item = (item as *mut u32).read_volatile();
            }
        }

        for item in &mut self.buf {
            unsafe {
                *item = (item as *mut u8).read_volatile();
            }
        }
    }
}

struct QueueStorage {
    queue: QueueHead,
    bufs: BTreeMap<u64, Box<TransferDescriptorStorage>>,
}

#[repr(align(16))]
struct TransferDescriptor([u32; 8]);

#[allow(unused)]
impl TransferDescriptor {
    fn link_pointer(&self) -> LinkPointer {
        get_link_pointer(self.0[0])
    }

    fn set_link_pointer(&mut self, ptr: &LinkPointer) {
        set_link_pointer(&mut self.0[0], ptr)
    }

    fn spd(&self) -> bool {
        self.0[1].get_bit(29)
    }

    fn set_spd(&mut self, val: bool) {
        self.0[1].set_bit(29, val);
    }

    fn err_counter(&self) -> u8 {
        self.0[1].get_bits(27, 2) as u8
    }

    fn set_err_counter(&mut self, val: u8) {
        assert!(val <= 3);
        self.0[1].set_bits(27, 2, val as u32);
    }

    fn low_speed(&self) -> bool {
        self.0[1].get_bit(26)
    }

    fn set_low_speed(&mut self, val: bool) {
        self.0[1].set_bit(26, val);
    }

    fn isochronus_select(&self) -> bool {
        self.0[1].get_bit(25)
    }

    fn set_isochronus_select(&mut self, val: bool) {
        self.0[1].set_bit(25, val);
    }

    fn interrupt_on_complete(&self) -> bool {
        self.0[1].get_bit(24)
    }

    fn set_interrupt_on_complete(&mut self, val: bool) {
        self.0[1].set_bit(24, val);
    }

    fn status(&self) -> u8 {
        self.0[1].get_bits(16, 8) as u8
    }

    fn set_status(&mut self, val: u8) {
        self.0[1].set_bits(16, 8, val as u32)
    }

    fn actlen(&self) -> u16 {
        self.0[1].get_bits(0, 11) as u16
    }

    fn set_actlen(&mut self, val: u16) {
        assert!(val < 1 << 11);
        self.0[1].set_bits(0, 11, val as u32)
    }

    fn set_maxlen(&mut self, mut len: u16) {
        assert!(len <= 1280);

        if len == 0 {
            len = 0x7ff;
        } else {
            len -= 1
        }

        self.0[2].set_bits(21, 11, len as u32)
    }

    fn maxlen(&self) -> u16 {
        let mut ret = self.0[2].get_bits(21, 11) as u16;
        if ret == 0x7ff {
            ret = 0
        } else {
            ret += 1;
        }

        ret
    }

    fn set_data_toggle(&mut self, val: bool) {
        self.0[2].set_bit(19, val);
    }

    fn data_toggle(&self) -> bool {
        self.0[2].get_bit(19)
    }

    fn set_endpoint(&mut self, val: u8) {
        assert!(val < 16);
        self.0[2].set_bits(15, 4, val as u32);
    }

    fn endpoint(&self) -> u8 {
        self.0[2].get_bits(15, 4) as u8
    }

    fn set_address(&mut self, val: u8) {
        assert!(val <= 0x7e);
        self.0[2].set_bits(8, 7, val as u32);
    }

    fn address(&self) -> u8 {
        self.0[2].get_bits(8, 7) as u8
    }

    fn set_pid(&mut self, val: u8) {
        self.0[2].set_bits(0, 8, val as u32);
    }

    fn pid(&self) -> u8 {
        self.0[2].get_bits(0, 8) as u8
    }

    fn set_data(&mut self, data: *mut u8) {
        self.0[3] = data as u32;
    }

    fn data(&self) -> *mut u8 {
        self.0[3] as *mut u8
    }
}

fn set_link_pointer(dest: &mut u32, val: &LinkPointer) {
    unsafe {
        let mut source = (dest as *mut u32).read_volatile();
        match val {
            LinkPointer::None => {
                source.set_bit(0, true);
            }
            LinkPointer::TD(ptr_val) => {
                source.set_bit(0, false);
                source.set_bit(1, false);
                // Pointers are guaranteed to be 32 bit aligned
                assert_eq!((*ptr_val as u32).get_bits(0, 4), 0);
                source.set_bits(4, 28, (*ptr_val as u32) >> 4);
            }
            LinkPointer::QH(ptr_val) => {
                source.set_bit(0, false);
                source.set_bit(1, true);
                // Pointers are guaranteed to be 32 bit aligned
                assert_eq!((*ptr_val as u32).get_bits(0, 4), 0);
                source.set_bits(4, 28, (*ptr_val as u32) >> 4);
            }
        }

        (dest as *mut u32).write_volatile(source)
    }
}

fn get_link_pointer(source: u32) -> LinkPointer {
    if source.get_bit(0) {
        return LinkPointer::None;
    }

    let ptr = source & !0xf;
    if source.get_bit(1) {
        LinkPointer::QH(ptr as *const QueueHead)
    } else {
        LinkPointer::TD(ptr as *const TransferDescriptor)
    }
}

fn chain_tds(tds: &mut [Box<TransferDescriptorStorage>]) {
    for i in 1..tds.len() {
        let second_ptr = &tds[i].descriptor as *const TransferDescriptor;
        let first = &mut tds[i - 1];

        first
            .descriptor
            .set_link_pointer(&LinkPointer::TD(second_ptr));
    }

    tds.last_mut()
        .unwrap()
        .descriptor
        .set_interrupt_on_complete(true);
}

fn generate_td(packet: UsbPacket) -> Result<Box<TransferDescriptorStorage>, InvalidPacketErr> {
    const USB_MAX_PACKET_LEN: usize = 1024;
    if packet.data.len() > USB_MAX_PACKET_LEN {
        return Err(InvalidPacketErr);
    }

    let mut ret = Box::new(TransferDescriptorStorage {
        buf: packet.data,
        descriptor: TransferDescriptor([0; 8]),
    });
    ret.descriptor.set_link_pointer(&LinkPointer::None);
    ret.descriptor.set_low_speed(true);
    ret.descriptor.set_status(0x80);
    ret.descriptor
        .set_maxlen(ret.buf.len().try_into().map_err(|_| InvalidPacketErr)?);
    ret.descriptor.set_address(packet.address);
    ret.descriptor.set_endpoint(packet.endpoint);
    let pid = match packet.pid {
        Pid::Setup => 0b0010_1101,
        Pid::Out => 0b1110_0001,
        Pid::In => 0b0110_1001,
    };
    ret.descriptor.set_pid(pid);
    ret.descriptor.set_data(ret.buf.as_mut_ptr());
    ret.descriptor.set_data_toggle(packet.data_toggle);

    Ok(ret)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::testing::*;

    create_test!(test_td_link_pointer_td, {
        let val = LinkPointer::TD(0xdeadbe00 as *const TransferDescriptor);

        let mut td = TransferDescriptor([0; 8]);
        td.set_link_pointer(&val);
        test_eq!(td.0[0], 0xdeadbe00u32);
        test_eq!(td.link_pointer(), val);

        Ok(())
    });

    create_test!(test_td_link_pointer_qh, {
        let val = LinkPointer::QH(0xdeadbe00 as *const QueueHead);

        let mut td = TransferDescriptor([0; 8]);
        td.set_link_pointer(&val);
        test_eq!(td.0[0], 0xdeadbe02u32);
        test_eq!(td.link_pointer(), val);

        Ok(())
    });

    create_test!(test_td_link_pointer_none, {
        let val = LinkPointer::None;

        let mut td = TransferDescriptor([0; 8]);
        td.set_link_pointer(&val);
        test_eq!(td.0[0], 0x1u32);
        test_eq!(td.link_pointer(), val);

        Ok(())
    });

    create_test!(test_td_spd, {
        let mut td = TransferDescriptor([0; 8]);
        td.set_spd(true);
        test_eq!(td.0[1], 1 << 29);
        test_eq!(td.spd(), true);

        Ok(())
    });

    create_test!(test_td_err_counter, {
        let mut td = TransferDescriptor([0; 8]);
        td.set_err_counter(3);
        test_eq!(td.0[1], 3 << 27);
        test_eq!(td.err_counter(), 3);

        Ok(())
    });

    create_test!(test_td_low_speed, {
        let mut td = TransferDescriptor([0; 8]);
        td.set_low_speed(true);
        test_eq!(td.0[1], 1 << 26);
        test_eq!(td.low_speed(), true);

        Ok(())
    });

    create_test!(test_td_isochronus_select, {
        let mut td = TransferDescriptor([0; 8]);
        td.set_isochronus_select(true);
        test_eq!(td.0[1], 1 << 25);
        test_eq!(td.isochronus_select(), true);

        Ok(())
    });

    create_test!(test_td_interrupt_on_complete, {
        let mut td = TransferDescriptor([0; 8]);
        td.set_interrupt_on_complete(true);
        test_eq!(td.0[1], 1 << 24);
        test_eq!(td.interrupt_on_complete(), true);

        Ok(())
    });

    create_test!(test_td_status, {
        let mut td = TransferDescriptor([0; 8]);
        td.set_status(0xef);
        test_eq!(td.0[1], 0xef << 16);
        test_eq!(td.status(), 0xef);

        Ok(())
    });

    create_test!(test_td_actlen, {
        let mut td = TransferDescriptor([0; 8]);
        td.set_actlen(0x5ff);
        test_eq!(td.0[1], 0x5ff);
        test_eq!(td.actlen(), 0x5ff);

        Ok(())
    });

    create_test!(test_td_maxlen, {
        let test_vals = [(1280, 0x4ff), (0, 0x7ff), (1, 0x00), (300, 299)];

        let mut td = TransferDescriptor([0; 8]);

        for (val, in_mem) in test_vals {
            td.set_maxlen(val);
            test_eq!(val, td.maxlen());
            test_eq!(td.0[2].get_bits(21, 11), in_mem);
        }

        Ok(())
    });

    create_test!(test_td_data_toggle, {
        let mut td = TransferDescriptor([0; 8]);

        td.set_data_toggle(true);
        test_eq!(td.data_toggle(), true);

        td.set_data_toggle(false);
        test_eq!(td.data_toggle(), false);

        Ok(())
    });

    create_test!(test_td_endpoint, {
        let mut td = TransferDescriptor([0; 8]);

        td.set_endpoint(0xd);
        test_eq!(td.endpoint(), 0xd);

        Ok(())
    });

    create_test!(test_td_address, {
        let mut td = TransferDescriptor([0; 8]);

        td.set_address(0x6d);
        test_eq!(td.address(), 0x6d);

        Ok(())
    });

    create_test!(test_td_pid, {
        let mut td = TransferDescriptor([0; 8]);

        td.set_pid(0xfd);
        test_eq!(td.pid(), 0xfd);

        Ok(())
    });

    create_test!(test_td_data, {
        let mut td = TransferDescriptor([0; 8]);

        td.set_data(0xdeadbeef as *mut u8);
        test_eq!(td.data(), 0xdeadbeef as *mut u8);

        Ok(())
    });

    create_test!(test_td_full_descriptor, {
        // Stolen and adapted from https://github.com/fysnet/FYSOS/blob/9fea9ca93a2600afdac3060e8c45b4678998abe8/main/usb/utils/gdevdesc/gd_uhci.c
        let mut descriptor = [0u32; 8];
        descriptor[0] = 0xdeadbeef & !0xF;
        descriptor[1] = (1 << 26) | (3 << 27) | (0x80 << 16);
        descriptor[2] = (7 << 21) | ((0x23 & 0x7F) << 8) | 0x2D;
        descriptor[3] = 4096;

        let td = TransferDescriptor(descriptor);

        test_eq!(
            td.link_pointer(),
            LinkPointer::TD(0xdeadbee0 as *const TransferDescriptor)
        );
        test_eq!(td.maxlen(), 8);
        test_eq!(td.data_toggle(), false);
        test_eq!(td.spd(), false);
        test_eq!(td.err_counter(), 3);
        test_eq!(td.low_speed(), true);
        test_eq!(td.isochronus_select(), false);
        test_eq!(td.interrupt_on_complete(), false);
        test_eq!(td.status(), 0x80);
        test_eq!(td.actlen(), 0);
        test_eq!(td.endpoint(), 0);
        test_eq!(td.address(), 0x23);
        test_eq!(td.pid(), 0x2d);
        test_eq!(td.data(), 4096 as *mut u8);
        Ok(())
    });
    create_test!(test_queue_head_link_pointer_td, {
        let val = LinkPointer::TD(0xdeadbe00 as *const TransferDescriptor);

        let mut qh = QueueHead([0; 2]);
        qh.set_head_link(&val);
        test_eq!(qh.0[0], 0xdeadbe00u32);
        test_eq!(qh.head_link(), val);

        Ok(())
    });

    create_test!(test_queue_head_link_pointer_qh, {
        let val = LinkPointer::QH(0xdeadbe00 as *const QueueHead);

        let mut qh = QueueHead([0; 2]);
        qh.set_head_link(&val);
        test_eq!(qh.0[0], 0xdeadbe02u32);
        test_eq!(qh.head_link(), val);

        Ok(())
    });

    create_test!(test_queue_head_link_pointer_none, {
        let val = LinkPointer::None;

        let mut qh = QueueHead([0; 2]);
        qh.set_head_link(&val);
        test_eq!(qh.0[0], 0x1u32);
        test_eq!(qh.head_link(), val);

        Ok(())
    });

    create_test!(test_queue_element_link_pointer_td, {
        let val = LinkPointer::TD(0xdeadbe00 as *const TransferDescriptor);

        let mut qh = QueueHead([0; 2]);
        qh.set_element_link(&val);
        test_eq!(qh.0[1], 0xdeadbe00u32);
        test_eq!(qh.element_link(), val);

        Ok(())
    });

    create_test!(test_queue_element_link_pointer_qh, {
        let val = LinkPointer::QH(0xdeadbe00 as *const QueueHead);

        let mut qh = QueueHead([0; 2]);
        qh.set_element_link(&val);
        test_eq!(qh.0[1], 0xdeadbe02u32);
        test_eq!(qh.element_link(), val);

        Ok(())
    });

    create_test!(test_queue_element_link_pointer_none, {
        let val = LinkPointer::None;

        let mut qh = QueueHead([0; 2]);
        qh.set_element_link(&val);
        test_eq!(qh.0[1], 0x1u32);
        test_eq!(qh.element_link(), val);

        Ok(())
    });
}
