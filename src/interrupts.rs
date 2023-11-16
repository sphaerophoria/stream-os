use crate::{
    io::io_allocator::{IoAllocator, IoOffset, IoRange, OffsetOutOfRange},
    multiprocessing::{self, Apic},
    util::bit_manipulation::SetBits,
    util::interrupt_guard::InterruptGuarded,
};
use alloc::{boxed::Box, vec::Vec};
use core::{
    arch::asm,
    sync::atomic::{AtomicPtr, Ordering},
};
use hashbrown::HashMap;

static INTERRUPT_HANDLER_DATA: InterruptHandlerData = InterruptHandlerData::new();
static GATE_DESCRIPTORS: AtomicPtr<GateDescriptor> = AtomicPtr::new(core::ptr::null_mut());

const PIC_COMMAND_OFFSET: IoOffset = IoOffset::new(0);
const PIC_DATA_OFFSET: IoOffset = IoOffset::new(1);

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct GateDescriptor(u64);

struct GateDescriptorNewArgs {
    offset: u32,
    segment_selector: u16,
    gate_type: u8,
    dpl: u8,
    p: bool,
}

impl GateDescriptor {
    fn new(args: GateDescriptorNewArgs) -> GateDescriptor {
        let mut descriptor = 0u64;

        descriptor.set_bits(0, 16, args.offset as u64);
        descriptor.set_bits(48, 16, (args.offset >> 16) as u64);
        descriptor.set_bits(16, 16, args.segment_selector as u64);
        descriptor.set_bits(40, 4, args.gate_type as u64);
        descriptor.set_bits(45, 2, args.dpl as u64);
        descriptor.set_bits(47, 1, args.p as u64);

        GateDescriptor(descriptor)
    }
}

#[repr(C, packed)]
#[derive(Debug)]
struct Idt {
    size: u16,
    offset: u32,
}

#[repr(C)]
#[derive(Debug)]
struct InterruptFrame {
    ip: u32,
    cs: u32,
    flags: u32,
    sp: u32,
    ss: u32,
}

#[allow(unused)]
fn generate_interrupt_stub(num: u8) -> [u8; 21] {
    //10000c:       55                      push   %ebp
    //10000d:       89 e5                   mov    %esp,%ebp
    //10000f:       60                      pusha
    //100010:       6a 5a                   push   $0x5a
    //100012:       b8 00 05 11 00          mov    $0x110500,%eax
    //100017:       ff d0                   call   *%eax
    //100019:       83 c4 04                add    $0x4,%esp
    //10001c:       61                      popa
    //10001d:       89 ec                   mov    %ebp,%esp
    //10001f:       5d                      pop    %ebp
    //100020:       cf                      iret

    const NUM_INDEX: usize = 5;
    const FUNCTION_ADDR_INDEX: usize = 7;
    const TEMPLATE: [u8; 21] = [
        0x55, 0x89, 0xe5, 0x60, 0x6a, 0x5a, 0xb8, 0x00, 0x05, 0x11, 0x00, 0xff, 0xd0, 0x83, 0xc4,
        0x04, 0x61, 0x89, 0xec, 0x5d, 0xcf,
    ];
    let mut ret = TEMPLATE;
    ret[NUM_INDEX] = num;
    #[allow(clippy::fn_to_numeric_cast)]
    let addr = generic_interrupt_handler as u32;
    ret[FUNCTION_ADDR_INDEX] = addr as u8;
    ret[FUNCTION_ADDR_INDEX + 1] = (addr >> 8) as u8;
    ret[FUNCTION_ADDR_INDEX + 2] = (addr >> 16) as u8;
    ret[FUNCTION_ADDR_INDEX + 3] = (addr >> 24) as u8;
    ret
}

#[derive(Debug)]
pub enum InterruptHandlerError {
    NotInitialized,
    Pic1Eoi(OffsetOutOfRange),
    Pic2Eoi(OffsetOutOfRange),
}

#[derive(Debug)]
pub enum InterruptHandlerRegisterError {
    NotInitialized,
    ReadPic1(OffsetOutOfRange),
    ReadPic2(OffsetOutOfRange),
    WritePic1(OffsetOutOfRange),
    WritePic2(OffsetOutOfRange),
}

#[derive(Debug)]
pub struct PicRemapError(OffsetOutOfRange);

#[derive(Debug)]
pub struct DisableInterruptError(OffsetOutOfRange);

#[derive(Debug)]
pub enum InitInterruptError {
    AcquirePic1,
    AcquirePic2,
    RemapPic(PicRemapError),
    DisableInterrupts(DisableInterruptError),
}

#[no_mangle]
extern "C" fn generic_interrupt_handler(interrupt_number: u8) {
    let ret = (|| -> Result<(), InterruptHandlerError> {
        let mut handlers = INTERRUPT_HANDLER_DATA.handlers.lock();
        let fns = match handlers
            .as_mut()
            .ok_or(InterruptHandlerError::NotInitialized)?
            .get_mut(&interrupt_number)
        {
            Some(fns) => fns,
            None => {
                panic!(
                    "no handler for interrupt {} on cpu {}",
                    interrupt_number,
                    multiprocessing::cpuid()
                );
            }
        };
        for f in fns {
            f();
        }

        // Secondary processors use APIC not PIC
        if multiprocessing::cpuid() != multiprocessing::BSP_ID {
            let apic = Apic::new(multiprocessing::APIC_ADDR);
            unsafe {
                apic.write_eoi();
            }
            return Ok(());
        }

        const END_OF_INTERRUPT: u8 = 0x20;

        if (PIC2_OFFSET..PIC2_OFFSET + 8).contains(&interrupt_number) {
            let mut pic_io = INTERRUPT_HANDLER_DATA.pic_io.lock();
            let pic_io = pic_io
                .as_mut()
                .ok_or(InterruptHandlerError::NotInitialized)?;
            pic_io
                .pic2_io
                .write_u8(PIC_COMMAND_OFFSET, END_OF_INTERRUPT)
                .map_err(InterruptHandlerError::Pic2Eoi)?;
            pic_io
                .pic1_io
                .write_u8(PIC_COMMAND_OFFSET, END_OF_INTERRUPT)
                .map_err(InterruptHandlerError::Pic1Eoi)?;
        }

        if (PIC1_OFFSET..PIC1_OFFSET + 8).contains(&interrupt_number) {
            let mut pic_io = INTERRUPT_HANDLER_DATA.pic_io.lock();
            let pic_io = pic_io
                .as_mut()
                .ok_or(InterruptHandlerError::NotInitialized)?;
            pic_io
                .pic1_io
                .write_u8(PIC_COMMAND_OFFSET, END_OF_INTERRUPT)
                .map_err(InterruptHandlerError::Pic1Eoi)?;
        }

        Ok(())
    })();

    if let Err(e) = ret {
        error!("Interrupt handler error: {:?}", e);
    }
}

fn read_idtr() -> Idt {
    let mut ret = core::mem::MaybeUninit::uninit();
    unsafe {
        asm!(r#"
             sidt ({})
             "#,
             in (reg) ret.as_mut_ptr(), options(att_syntax, nostack, preserves_flags));
        ret.assume_init()
    }
}

const PIC1_OFFSET: u8 = 0x40;
const PIC2_OFFSET: u8 = 0x48;

#[allow(unused)]
#[derive(Debug)]
pub enum IrqId {
    Internal(u8),
    Pic1(u8),
    Pic2(u8),
}

struct PicIo {
    pic1_io: IoRange,
    pic2_io: IoRange,
}

pub struct InterruptHandlerData {
    #[allow(clippy::type_complexity)]
    handlers: InterruptGuarded<Option<HashMap<u8, Vec<Box<dyn FnMut()>>>>>,
    pic_io: InterruptGuarded<Option<PicIo>>,
}

impl InterruptHandlerData {
    pub const fn new() -> InterruptHandlerData {
        InterruptHandlerData {
            handlers: InterruptGuarded::new(None),
            pic_io: InterruptGuarded::new(None),
        }
    }

    fn init(&self, pic_io: PicIo) {
        *self.handlers.lock() = Some(Default::default());
        *self.pic_io.lock() = Some(pic_io);
    }

    pub fn register<F: FnMut() + 'static>(
        &self,
        irq_id: IrqId,
        f: F,
    ) -> Result<(), InterruptHandlerRegisterError> {
        let mut pic_io = self.pic_io.lock();
        let pic_io = pic_io
            .as_mut()
            .ok_or(InterruptHandlerRegisterError::NotInitialized)?;

        let interrupt_num = match irq_id {
            IrqId::Internal(i) => i,
            IrqId::Pic1(i) => i + PIC1_OFFSET,
            IrqId::Pic2(i) => i + PIC2_OFFSET,
        };

        {
            let mut handlers = self.handlers.lock();

            let handlers = handlers
                .as_mut()
                .ok_or(InterruptHandlerRegisterError::NotInitialized)?;

            let id_handlers = handlers.entry(interrupt_num).or_default();
            id_handlers.push(Box::new(f));
        }

        match irq_id {
            IrqId::Pic1(i) => {
                let mut mask = pic_io
                    .pic1_io
                    .read_u8(PIC_DATA_OFFSET)
                    .map_err(InterruptHandlerRegisterError::ReadPic1)?;
                mask.set_bit(i, false);
                pic_io
                    .pic1_io
                    .write_u8(PIC_DATA_OFFSET, mask)
                    .map_err(InterruptHandlerRegisterError::WritePic1)?;
            }
            IrqId::Pic2(i) => {
                let mut mask = pic_io
                    .pic2_io
                    .read_u8(PIC_DATA_OFFSET)
                    .map_err(InterruptHandlerRegisterError::ReadPic2)?;
                mask.set_bit(i, false);
                pic_io
                    .pic2_io
                    .write_u8(PIC_DATA_OFFSET, mask)
                    .map_err(InterruptHandlerRegisterError::WritePic2)?;
            }
            _ => (),
        };

        Ok(())
    }
}

fn pic_remap(offset1: u8, offset2: u8, pic_io: &mut PicIo) -> Result<(), PicRemapError> {
    // Adapted from https://wiki.osdev.org/8259_PIC
    // https://pdos.csail.mit.edu/6.828/2005/readings/hardware/8259A.pdf
    const ICW1_ICW4: u8 = 0x01;
    const ICW1_INIT: u8 = 0x10;
    const ICW4_8086: u8 = 0x01;

    (|| {
        let a1 = pic_io.pic1_io.read_u8(PIC_DATA_OFFSET)?;
        let a2 = pic_io.pic2_io.read_u8(PIC_DATA_OFFSET)?;

        pic_io
            .pic1_io
            .write_u8(PIC_COMMAND_OFFSET, ICW1_INIT | ICW1_ICW4)?;
        pic_io
            .pic2_io
            .write_u8(PIC_COMMAND_OFFSET, ICW1_INIT | ICW1_ICW4)?;
        pic_io.pic1_io.write_u8(PIC_DATA_OFFSET, offset1)?;
        pic_io.pic2_io.write_u8(PIC_DATA_OFFSET, offset2)?;
        pic_io.pic1_io.write_u8(PIC_DATA_OFFSET, 4)?;
        pic_io.pic2_io.write_u8(PIC_DATA_OFFSET, 2)?;

        pic_io.pic1_io.write_u8(PIC_DATA_OFFSET, ICW4_8086)?;
        pic_io.pic2_io.write_u8(PIC_DATA_OFFSET, ICW4_8086)?;

        pic_io.pic1_io.write_u8(PIC_DATA_OFFSET, a1)?;
        pic_io.pic2_io.write_u8(PIC_DATA_OFFSET, a2)?;
        Ok(())
    })()
    .map_err(PicRemapError)?;

    Ok(())
}

fn pic_disable_interrupts(pic_io: &mut PicIo) -> Result<(), DisableInterruptError> {
    // Leave IRQ2 unmasked as it is chained to pic2 which is fully masked. This makes
    // masking/unmasking logic easier downstream
    pic_io
        .pic1_io
        .write_u8(PIC_DATA_OFFSET, 0b1111_1011)
        .map_err(DisableInterruptError)?;
    pic_io
        .pic2_io
        .write_u8(PIC_DATA_OFFSET, 0xff)
        .map_err(DisableInterruptError)?;
    Ok(())
}

pub fn load_idt() {
    if GATE_DESCRIPTORS.load(Ordering::Acquire).is_null() {
        let mut table = Vec::with_capacity(255);
        let mut isrs = Vec::with_capacity(255);
        for i in 0..255 {
            isrs.push(generate_interrupt_stub(i as u8));

            let descriptor = GateDescriptor::new(GateDescriptorNewArgs {
                #[allow(clippy::fn_to_numeric_cast)]
                offset: isrs[i].as_ptr() as u32,
                segment_selector: 0x08,
                gate_type: 0b1111,
                dpl: 0,
                p: true,
            });

            table.push(descriptor);
        }

        // FIXME: only generate one time
        let table = table.leak();
        let isrs = isrs.leak();
        if GATE_DESCRIPTORS
            .compare_exchange(
                core::ptr::null_mut(),
                table.as_mut_ptr(),
                Ordering::Release,
                Ordering::Relaxed,
            )
            .is_err()
        {
            unsafe {
                let _ = Box::from_raw(table);
                let _ = Box::from_raw(isrs);
            }
        }
    }
    let table_ptr: *const GateDescriptor = GATE_DESCRIPTORS.load(Ordering::Acquire);

    let idt = Idt {
        size: 256 * 8 - 1,
        offset: table_ptr as u32,
    };

    debug!("{:?}", read_idtr());
    unsafe {
        asm!(r#"
             lidt ({idt})
             "#,
             idt = in (reg) &idt,
             options(att_syntax));
    }

    debug!("{:?}", read_idtr());
}

pub fn init(
    io_allocator: &mut IoAllocator,
) -> Result<&'static InterruptHandlerData, InitInterruptError> {
    let pic1_io = io_allocator
        .request_io_range(0x20, 2)
        .ok_or(InitInterruptError::AcquirePic1)?;
    let pic2_io = io_allocator
        .request_io_range(0xA0, 2)
        .ok_or(InitInterruptError::AcquirePic2)?;

    let mut pic_io = PicIo { pic1_io, pic2_io };

    pic_remap(PIC1_OFFSET, PIC2_OFFSET, &mut pic_io).map_err(InitInterruptError::RemapPic)?;

    pic_disable_interrupts(&mut pic_io).map_err(InitInterruptError::DisableInterrupts)?;

    load_idt();

    INTERRUPT_HANDLER_DATA.init(pic_io);

    Ok(&INTERRUPT_HANDLER_DATA)
}
