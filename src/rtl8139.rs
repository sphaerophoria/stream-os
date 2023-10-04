use core::task::Poll;

use crate::{
    future::wakeup_executor,
    interrupts::{InterruptHandlerData, InterruptHandlerRegisterError, IrqId},
    io::pci::{GeneralPciDevice, InvalidHeaderError, Pci, PciDevice},
    util::{
        async_mutex::Mutex,
        bit_manipulation::{GetBits, SetBits},
    },
};

use alloc::{boxed::Box, vec};

const COMMAND_REGISTER_OFFSET: usize = 0x37;
const RBSTART_OFFSET: usize = 0x30;
const RECEIVE_CONFIG_OFFSET: usize = 0x44;
const INTERRUPT_MASK_OFFSET: usize = 0x3c;
const INTERRUPT_STATUS_OFFSET: usize = 0x3e;
const TRANSMIT_CONFIG_OFFSET: usize = 0x40;
const TRANSMIT_STATUS_OFFSET: usize = 0x10;
const TRANSMIT_DATA_OFFSET: usize = 0x20;
const CAPR_OFFSET: usize = 0x38;
const CBR_OFFSET: usize = 0x3a;

unsafe fn reset_device(base: *mut u8) {
    let command_register = base.add(COMMAND_REGISTER_OFFSET);
    let mut val = command_register.read_volatile();
    val.set_bit(4, true);
    debug!("reset device register value: {:#x}", val);
    command_register.write_volatile(val);
    while command_register.read_volatile().get_bit(4) {}
}

fn generate_receive_buffer() -> Box<[u8]> {
    // Configuration register 0b11 says this is the size
    // We also need 1.5k extra space due to WRAP bit being set
    const DATA_SIZE: usize = 64 * 1024;
    const OVERHEAD: usize = 16;
    const WRAP_PADDING: usize = 1536;
    const BUFFER_SIZE: usize = DATA_SIZE + OVERHEAD + WRAP_PADDING;

    let rx_buffer = vec![0; BUFFER_SIZE];
    rx_buffer.into_boxed_slice()
}

#[derive(Debug)]
#[allow(unused)]
pub struct ValueNotSet<T> {
    set: T,
    retreived: T,
}

unsafe fn write_receive_buffer_address(
    base: *mut u8,
    addr: *mut u8,
) -> Result<(), ValueNotSet<u32>> {
    let rbstart = base.add(RBSTART_OFFSET) as *mut u32;
    rbstart.write_volatile(addr as u32);

    let new_val = rbstart.read_volatile();
    if new_val != addr as u32 {
        Err(ValueNotSet {
            set: addr as u32,
            retreived: new_val,
        })
    } else {
        Ok(())
    }
}

unsafe fn set_receive_buffer_size(base: *mut u8) -> Result<(), ValueNotSet<u32>> {
    let receive_config_reg = base.add(RECEIVE_CONFIG_OFFSET) as *mut u32;
    let mut receive_config_val = receive_config_reg.read_volatile();

    debug!(
        "Previous receive config register value: {:x}",
        receive_config_val
    );

    receive_config_val.set_bits(11, 2, 0b11);
    receive_config_reg.write_volatile(receive_config_val);

    let new_val = receive_config_reg.read_volatile();
    debug!("New receive config register value: {:x}", new_val);
    if new_val != receive_config_val {
        Err(ValueNotSet {
            set: receive_config_val,
            retreived: new_val,
        })
    } else {
        Ok(())
    }
}

#[derive(Debug)]
pub enum InitReceiveBufferError {
    WriteReceiveBuffer(ValueNotSet<u32>),
    SetReceiveBufferSize(ValueNotSet<u32>),
}

unsafe fn init_receive_buffer(base: *mut u8) -> Result<Box<[u8]>, InitReceiveBufferError> {
    let mut rx_buffer = generate_receive_buffer();
    write_receive_buffer_address(base, rx_buffer.as_mut_ptr())
        .map_err(InitReceiveBufferError::WriteReceiveBuffer)?;

    set_receive_buffer_size(base).map_err(InitReceiveBufferError::SetReceiveBufferSize)?;

    Ok(rx_buffer)
}

unsafe fn set_interrupt_mask(base: *mut u8) {
    let interrupt_mask_reg = base.add(INTERRUPT_MASK_OFFSET) as *mut u16;
    // transmit ok bit 2
    // receive ok bit 0
    interrupt_mask_reg.write_volatile(0x5);
}

#[derive(Debug)]
pub struct InvalidIrq;

unsafe fn get_irq_id(
    pci: &mut Pci,
    rtl_device: &mut GeneralPciDevice,
) -> Result<IrqId, InvalidIrq> {
    let irq_num = rtl_device.get_irq_num(pci);
    if irq_num < 8 {
        Ok(IrqId::Pic1(irq_num))
    } else if irq_num < 16 {
        Ok(IrqId::Pic2(irq_num - 8))
    } else {
        Err(InvalidIrq)
    }
}

unsafe fn clear_interrupt(base: *mut u8) {
    let reg = base.add(INTERRUPT_STATUS_OFFSET) as *mut u16;
    // According to the OSDev wiki, we need to both read _and_ write the register to clear the
    // interrupt
    reg.read_volatile();
    reg.write_volatile(0x05);
}

#[derive(Debug)]
pub enum InitInterruptError {
    InvalidIrq(InvalidIrq),
    Register(InterruptHandlerRegisterError),
}

unsafe fn init_interrupts(
    base: *mut u8,
    pci: &mut Pci,
    rtl_device: &mut GeneralPciDevice,
    interrupt_handlers: &InterruptHandlerData,
) -> Result<(), InitInterruptError> {
    let irq_id = get_irq_id(pci, rtl_device).map_err(InitInterruptError::InvalidIrq)?;

    interrupt_handlers
        .register(irq_id, move || unsafe {
            clear_interrupt(base);
            wakeup_executor();
        })
        .map_err(InitInterruptError::Register)?;

    set_interrupt_mask(base);
    Ok(())
}

unsafe fn enable_transmit_receive(base: *mut u8) -> Result<(), ValueNotSet<u8>> {
    let reg = base.add(COMMAND_REGISTER_OFFSET);
    let mut val = reg.read_volatile();
    val.set_bits(2, 2, 0b11);
    reg.write_volatile(val);
    let read_back = reg.read_volatile();
    if read_back != val {
        Err(ValueNotSet {
            set: val,
            retreived: read_back,
        })
    } else {
        Ok(())
    }
}

#[derive(Debug)]
pub enum SetTransmitConfigError {
    EnableLoopback(ValueNotSet<u32>),
    EnableAppendCrc(ValueNotSet<u32>),
}

unsafe fn set_transmit_config(
    base: *mut u8,
    with_loopback: bool,
) -> Result<(), SetTransmitConfigError> {
    if with_loopback {
        enable_loopback(base).map_err(SetTransmitConfigError::EnableLoopback)?;
    }

    enable_append_crc(base).map_err(SetTransmitConfigError::EnableAppendCrc)?;

    Ok(())
}

unsafe fn enable_loopback(base: *mut u8) -> Result<(), ValueNotSet<u32>> {
    let transmit_config_reg = base.add(TRANSMIT_CONFIG_OFFSET) as *mut u32;
    let mut config = transmit_config_reg.read_volatile();
    debug!("initial transmission config: {:#x}", config);

    config.set_bits(17, 2, 0b11);
    debug!("written transmission config: {:b}", config);

    transmit_config_reg.write_volatile(config);
    let new_val = transmit_config_reg.read_volatile();
    debug!("read back transmission config: {:b}", config);

    if new_val != config {
        Err(ValueNotSet {
            set: config,
            retreived: new_val,
        })
    } else {
        Ok(())
    }
}

unsafe fn enable_append_crc(base: *mut u8) -> Result<(), ValueNotSet<u32>> {
    let transmit_config_reg = base.add(TRANSMIT_CONFIG_OFFSET) as *mut u32;
    let mut config = transmit_config_reg.read_volatile();
    debug!("initial transmission config: {:#x}", config);

    config.set_bit(16, true);
    transmit_config_reg.write_volatile(config);
    let new_val = transmit_config_reg.read_volatile();
    debug!("read back transmission config: {:#x}", config);

    if new_val != config {
        Err(ValueNotSet {
            set: config,
            retreived: new_val,
        })
    } else {
        Ok(())
    }
}

unsafe fn init_receive_configuration(base: *mut u8) -> Result<(), ValueNotSet<u32>> {
    // Setup global receiver configuration
    let global_receive_config_reg = base.add(RECEIVE_CONFIG_OFFSET) as *mut u32;
    let mut global_receive_config = global_receive_config_reg.read_volatile();
    // Disable receive
    global_receive_config.set_bits(0, 6, 0x00);
    // Physical match
    global_receive_config.set_bit(1, true);
    // Multicast
    global_receive_config.set_bit(3, true);
    // Overwrite start of buffer when too much data (WRAP)
    global_receive_config.set_bit(7, true);
    global_receive_config_reg.write_volatile(global_receive_config);
    let retreived = global_receive_config_reg.read_volatile();
    if retreived != global_receive_config {
        Err(ValueNotSet {
            set: global_receive_config,
            retreived,
        })
    } else {
        Ok(())
    }
}

unsafe fn init_capr(base: *mut u8) -> Result<(), ValueNotSet<u16>> {
    // Setup global receiver configuration
    let capr = base.add(CAPR_OFFSET) as *mut u16;
    // According to qemu source, this has to be offset by 16 bits
    const INITIAL_VAL: u16 = 0xfff0;
    capr.write_volatile(INITIAL_VAL);
    let set_val = capr.read_volatile();
    if set_val != INITIAL_VAL {
        Err(ValueNotSet {
            set: INITIAL_VAL,
            retreived: set_val,
        })
    } else {
        Ok(())
    }
}

struct TranmissionWaiter {
    transmit_status_reg: *mut u32,
}

impl core::future::Future for TranmissionWaiter {
    type Output = ();

    fn poll(
        self: core::pin::Pin<&mut Self>,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        unsafe {
            let status = self.transmit_status_reg.read_volatile();
            let own = status.get_bit(13);

            if own {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }
    }
}

async unsafe fn transmit_data_and_wait(
    transmit_data_ptr: *mut u32,
    transmit_status_reg: *mut u32,
    data: &[u8],
) {
    transmit_data_ptr.write_volatile(data.as_ptr() as u32);

    let mut status = transmit_status_reg.read_volatile();
    // Lowest 12 bits contain the pointer
    status.set_bits(0, 12, data.len() as u32);
    // Set own bit to false to trigger a write
    status.set_bit(13, false);
    transmit_status_reg.write_volatile(status);

    let f = TranmissionWaiter {
        transmit_status_reg,
    };
    f.await;
}

struct ReceiverWaiter {
    capr_reg: *mut u16,
    cbr_reg: *mut u16,
}

impl core::future::Future for ReceiverWaiter {
    type Output = ();

    fn poll(
        self: core::pin::Pin<&mut Self>,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        unsafe {
            if self.capr_reg.read_volatile().wrapping_add(16) == self.cbr_reg.read_volatile() {
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        }
    }
}

async unsafe fn get_packet(base: *mut u8, receive_buf: &[u8]) -> &[u8] {
    // header 16 bit
    // length 16 bit
    // packet
    let capr = (base.add(CAPR_OFFSET) as *mut u16).read_volatile();
    let cbr = (base.add(CBR_OFFSET) as *mut u16).read_volatile();
    let start_offset = capr.wrapping_add(16);
    // Qemu source says this is offset by 16 bits
    let header = receive_buf.as_ptr().add(start_offset as usize) as *const u16;
    let length = header.wrapping_add(1).read_volatile();

    debug!("Received header: {:x}", header.read_volatile());
    debug!("Received buf length: {}", length);

    debug!(
        "header: {:?}, length: {}, capr: {:#x}, cbr: {}, total_buffer_lenght: {:#x}",
        header.read_volatile(),
        length,
        capr,
        cbr,
        receive_buf.len()
    );

    // NOTE: WRAP bit is important here
    core::slice::from_raw_parts(header.add(2) as *mut u8, length as usize)
}

unsafe fn increment_capr(base: *mut u8, receive_buf: &[u8]) {
    let capr_reg = base.add(CAPR_OFFSET) as *mut u16;
    let mut capr = capr_reg.read_volatile();
    let start_offset = capr.wrapping_add(16);
    // Qemu source says this is offset by 16 bits
    let header = receive_buf.as_ptr().add(start_offset as usize) as *const u16;
    let length = header.wrapping_add(1).read_volatile();

    // Add 4 for the header and packet length, then dword align with + 3 and mask
    // FIXME: fn align_16 ...
    capr = capr.wrapping_add(length + 4 + 3) & !0b11;
    capr_reg.write_volatile(capr);
}

#[derive(Debug)]
pub enum Rtl8139InitError {
    PciProbeFailed(InvalidHeaderError),
    DeviceNotFound,
    PciHeaderTypeIncorrect(PciDevice),
    MmapRangeNotFound,
    MmapRangeUnexpected(usize),
    InitReceiveBuffer(InitReceiveBufferError),
    InitInterrupts(InitInterruptError),
    EnableTransmitReceive(ValueNotSet<u8>),
    SetTransmitConfig(SetTransmitConfigError),
    InitReceiveConfig(ValueNotSet<u32>),
    InitCapr(ValueNotSet<u16>),
}

#[derive(Debug)]
pub struct PacketTooShort;

struct Inner {
    base: *mut u8,
    transmit_idx: u8,
    receive_buf: Box<[u8]>,
}

impl Inner {
    pub fn new(
        pci: &mut Pci,
        interrupt_handlers: &InterruptHandlerData,
        with_loopback: bool,
    ) -> Result<Inner, Rtl8139InitError> {
        let device = pci
            .find_device(0x10ec, 0x8139)
            .map_err(Rtl8139InitError::PciProbeFailed)?
            .ok_or(Rtl8139InitError::DeviceNotFound)?;

        let mut rtl_device = match device {
            PciDevice::General(v) => v,
            _ => {
                return Err(Rtl8139InitError::PciHeaderTypeIncorrect(device));
            }
        };

        let mmap_range = rtl_device
            .find_mmap_range(pci)
            .ok_or(Rtl8139InitError::MmapRangeNotFound)?;

        if mmap_range.length != 256 {
            return Err(Rtl8139InitError::MmapRangeUnexpected(mmap_range.length));
        }

        // Required for the card to write to memory
        rtl_device.enable_bus_mastering(pci);
        unsafe {
            reset_device(mmap_range.start);
            let receive_buf = init_receive_buffer(mmap_range.start)
                .map_err(Rtl8139InitError::InitReceiveBuffer)?;
            init_interrupts(mmap_range.start, pci, &mut rtl_device, interrupt_handlers)
                .map_err(Rtl8139InitError::InitInterrupts)?;
            enable_transmit_receive(mmap_range.start)
                .map_err(Rtl8139InitError::EnableTransmitReceive)?;

            set_transmit_config(mmap_range.start, with_loopback)
                .map_err(Rtl8139InitError::SetTransmitConfig)?;

            init_receive_configuration(mmap_range.start)
                .map_err(Rtl8139InitError::InitReceiveConfig)?;

            init_capr(mmap_range.start).map_err(Rtl8139InitError::InitCapr)?;

            Ok(Inner {
                base: mmap_range.start,
                transmit_idx: 0,
                receive_buf,
            })
        }
    }

    pub async fn write(&mut self, packet: &[u8]) -> Result<(), PacketTooShort> {
        debug!("Writing packet with length: {}", packet.len());

        if packet.len() < 60 {
            return Err(PacketTooShort);
        }

        unsafe {
            let extra_offset = self.transmit_idx as usize * core::mem::size_of::<u32>();
            let data_offset = TRANSMIT_DATA_OFFSET + extra_offset;
            let status_offset = TRANSMIT_STATUS_OFFSET + extra_offset;
            let data_ptr = self.base.add(data_offset) as *mut u32;
            let status_ptr = self.base.add(status_offset) as *mut u32;
            transmit_data_and_wait(data_ptr, status_ptr, packet).await;
            self.transmit_idx = (self.transmit_idx + 1) % 4;
        }

        Ok(())
    }

    pub async fn read<F, Fut>(&mut self, on_read: F) -> Fut
    where
        F: Fn(&[u8]) -> Fut,
        Fut: core::future::Future<Output = ()>,
    {
        let data = unsafe { get_packet(self.base, &self.receive_buf).await };
        let fut = on_read(data);
        unsafe {
            increment_capr(self.base, &self.receive_buf);
        }

        fut
    }

    pub fn log_mac(&mut self) {
        let mut mac = [0; 6];
        for (i, v) in mac.iter_mut().enumerate() {
            unsafe { *v = self.base.add(i).read_volatile() }
        }

        info!("Mac address: {:x?}", mac);
    }

    pub fn get_mac(&mut self) -> [u8; 6] {
        let mut mac = [0; 6];
        for (i, v) in mac.iter_mut().enumerate() {
            unsafe { *v = self.base.add(i).read_volatile() }
        }
        mac
    }
}

pub struct Rtl8139 {
    inner: Mutex<Inner>,
}

impl Rtl8139 {
    pub fn new(
        pci: &mut Pci,
        interrupt_handlers: &InterruptHandlerData,
        with_loopback: bool,
    ) -> Result<Rtl8139, Rtl8139InitError> {
        let inner = Mutex::new(Inner::new(pci, interrupt_handlers, with_loopback)?);
        Ok(Rtl8139 { inner })
    }

    pub async fn write(&self, packet: &[u8]) -> Result<(), PacketTooShort> {
        let mut inner = self.inner.lock().await;
        inner.write(packet).await
    }

    pub async fn read<F, Fut>(&self, on_read: F)
    where
        F: Fn(&[u8]) -> Fut,
        Fut: core::future::Future<Output = ()>,
    {
        unsafe {
            let base = self.inner.lock().await.base;
            let capr_reg = base.add(CAPR_OFFSET) as *mut u16;
            let cbr_reg = base.add(CBR_OFFSET) as *mut u16;

            let fut = loop {
                ReceiverWaiter { capr_reg, cbr_reg }.await;
                //sleep(1.0).await;
                if let Some(mut v) = self.inner.try_lock() {
                    let fut = v.read(on_read).await;
                    break fut;
                };
            };
            fut.await;
        }
    }

    pub async fn log_mac(&self) {
        self.inner.lock().await.log_mac();
    }

    pub fn get_mac(&self) -> [u8; 6] {
        self.inner.try_lock().unwrap().get_mac()
    }
}
