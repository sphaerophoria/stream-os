use crate::{
    io::port_manager::{Port, PortManager},
    util::bit_manipulation::SetBits,
    util::interrupt_guard::InterruptGuarded,
};
use alloc::boxed::Box;
use core::{arch::asm, cell::RefCell};
use hashbrown::HashMap;

static INTERRUPT_TABLE: InterruptTable = InterruptTable::new();
static INTERRUPT_HANDLER_DATA: InterruptHandlerData = InterruptHandlerData::new();
static ISRS: InterruptGuarded<[[u8; 21]; 255]> = InterruptGuarded::new([[0; 21]; 255]);

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

struct InterruptTable {
    inner: RefCell<[GateDescriptor; 256]>,
}

impl InterruptTable {
    const fn new() -> Self {
        Self {
            inner: RefCell::new([GateDescriptor(0); 256]),
        }
    }
}

unsafe impl Sync for InterruptTable {}

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


#[no_mangle]
extern "C" fn generic_interrupt_handler(interrupt_number: u8) {
    let handlers = INTERRUPT_HANDLER_DATA.handlers.lock();
    let f = match handlers
        .as_ref()
        .expect("interrupt handlers not initialized")
        .get(&interrupt_number)
    {
        Some(f) => f,
        None => {
            panic!("no handler for interrupt {}", interrupt_number);
        }
    };
    f();

    const END_OF_INTERRUPT: u8 = 0x20;

    if (PIC2_OFFSET..PIC2_OFFSET + 8).contains(&interrupt_number) {
        let mut ports = INTERRUPT_HANDLER_DATA.ports.lock();
        let ports = ports.as_mut().expect("interrupt handlers not initialized");
        ports.pic2_command.writeb(END_OF_INTERRUPT);
        ports.pic1_command.writeb(END_OF_INTERRUPT);
    }

    if (PIC1_OFFSET..PIC1_OFFSET + 8).contains(&interrupt_number) {
        let mut ports = INTERRUPT_HANDLER_DATA.ports.lock();
        let ports = ports.as_mut().expect("interrupt handlers not initialized");
        ports.pic1_command.writeb(END_OF_INTERRUPT);
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
pub enum IrqId {
    Internal(u8),
    Pic1(u8),
    Pic2(u8),
}

struct PicPorts {
    pic1_command: Port,
    pic1_data: Port,
    pic2_command: Port,
    pic2_data: Port,
}

pub struct InterruptHandlerData {
    #[allow(clippy::type_complexity)]
    handlers: InterruptGuarded<Option<HashMap<u8, Box<dyn Fn()>>>>,
    ports: InterruptGuarded<Option<PicPorts>>,
}

impl InterruptHandlerData {
    pub const fn new() -> InterruptHandlerData {
        InterruptHandlerData {
            handlers: InterruptGuarded::new(None),
            ports: InterruptGuarded::new(None),
        }
    }

    fn init(&self, ports: PicPorts) {
        *self.handlers.lock() = Some(Default::default());
        *self.ports.lock() = Some(ports);
    }

    pub fn register<F: Fn() + 'static>(&self, irq_id: IrqId, f: F) {
        let mut ports = self.ports.lock();
        let ports = ports.as_mut().expect("InterruptHandlers not initialized");

        let interrupt_num = match irq_id {
            IrqId::Internal(i) => i,
            IrqId::Pic1(i) => i + PIC1_OFFSET,
            IrqId::Pic2(i) => i + PIC2_OFFSET,
        };

        {
            let mut handlers = self.handlers.lock();

            let handlers = handlers
                .as_mut()
                .expect("InterruptHandlers not initailized");

            assert!(
                !handlers.contains_key(&interrupt_num),
                "Handlers should only be registered once per interrupt"
            );
            handlers.insert(interrupt_num, Box::new(f));
        }

        match irq_id {
            IrqId::Pic1(i) => {
                let mut mask = ports.pic1_data.readb();
                mask.set_bit(i, false);
                ports.pic1_data.writeb(mask);
            }
            IrqId::Pic2(i) => {
                let mut mask = ports.pic2_data.readb();
                mask.set_bit(i, false);
                ports.pic2_data.writeb(mask);
            }
            _ => (),
        };
    }
}

fn pic_remap(offset1: u8, offset2: u8, ports: &mut PicPorts) {
    // Adapted from https://wiki.osdev.org/8259_PIC
    // https://pdos.csail.mit.edu/6.828/2005/readings/hardware/8259A.pdf
    const ICW1_ICW4: u8 = 0x01;
    const ICW1_INIT: u8 = 0x10;
    const ICW4_8086: u8 = 0x01;

    let a1 = ports.pic1_data.readb();
    let a2 = ports.pic2_data.readb();

    ports.pic1_command.writeb(ICW1_INIT | ICW1_ICW4);
    ports.pic2_command.writeb(ICW1_INIT | ICW1_ICW4);
    ports.pic1_data.writeb(offset1);
    ports.pic2_data.writeb(offset2);
    ports.pic1_data.writeb(4);
    ports.pic2_data.writeb(2);

    ports.pic1_data.writeb(ICW4_8086);
    ports.pic2_data.writeb(ICW4_8086);

    ports.pic1_data.writeb(a1);
    ports.pic2_data.writeb(a2);
}

fn pic_disable_interrupts(ports: &mut PicPorts) {
    // Leave IRQ2 unmasked as it is chained to pic2 which is fully masked. This makes
    // masking/unmasking logic easier downstream
    ports.pic1_data.writeb(0b1111_1011);
    ports.pic2_data.writeb(0xff);
}

pub fn init(port_manager: &mut PortManager) -> &'static InterruptHandlerData {
    let pic1_command = port_manager
        .request_port(0x20)
        .expect("Failed to get pic 1 command");
    let pic1_data = port_manager
        .request_port(0x21)
        .expect("Failed to get pic 1 data");
    let pic2_command = port_manager
        .request_port(0xA0)
        .expect("Failed to get pic 2 command");
    let pic2_data = port_manager
        .request_port(0xA1)
        .expect("Failed to get pic 2 data");

    let mut ports = PicPorts {
        pic1_command,
        pic1_data,
        pic2_command,
        pic2_data,
    };

    pic_remap(PIC1_OFFSET, PIC2_OFFSET, &mut ports);

    pic_disable_interrupts(&mut ports);

    for i in 0..255 {
        ISRS.lock()[i] = generate_interrupt_stub(i as u8);
        let descriptor = GateDescriptor::new(GateDescriptorNewArgs {
            #[allow(clippy::fn_to_numeric_cast)]
            offset: ISRS.lock()[i].as_ptr() as u32,
            segment_selector: 0x08,
            gate_type: 0b1111,
            dpl: 0,
            p: true,
        });

        let mut table = INTERRUPT_TABLE.inner.borrow_mut();
        table[i] = descriptor;
    }

    let table = INTERRUPT_TABLE.inner.borrow_mut();
    let table_ptr: *const GateDescriptor = table.as_ptr();

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
    INTERRUPT_HANDLER_DATA.init(ports);

    &INTERRUPT_HANDLER_DATA
}
