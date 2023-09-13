use crate::{io::port_manager::PortManager, util::bit_manipulation::SetBits};
use core::{arch::asm, cell::RefCell};

static INTERRUPT_TABLE: InterruptTable = InterruptTable::new();

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

extern "x86-interrupt" fn general_fault_handler() {
    println!("Hi");
}

extern "x86-interrupt" fn double_fault_handler() {
    panic!("Double fault");
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

macro_rules! interrupt {
    ($num: expr) => {
        core::arch::asm!(concat!("int ", stringify!($num)));
    };
}

pub fn init(port_manager: &mut PortManager) {
    let mut pic1_data = port_manager
        .request_port(0x21)
        .expect("Failed to get pic 1 data");
    let mut pic2_data = port_manager
        .request_port(0xA1)
        .expect("Failed to get pic 2 data");

    // Disable external interrupts
    pic1_data.writeb(0xff);
    pic2_data.writeb(0xff);

    #[allow(clippy::fn_to_numeric_cast)]
    let general_fault_descriptor = GateDescriptor::new(GateDescriptorNewArgs {
        offset: general_fault_handler as u32,
        segment_selector: 0x08,
        gate_type: 0b1111,
        dpl: 0,
        p: true,
    });

    #[allow(clippy::fn_to_numeric_cast)]
    let double_fault_descriptor = GateDescriptor::new(GateDescriptorNewArgs {
        offset: double_fault_handler as u32,
        segment_selector: 0x08,
        gate_type: 0b1111,
        dpl: 0,
        p: true,
    });

    let mut table = INTERRUPT_TABLE.inner.borrow_mut();
    table[11] = general_fault_descriptor;
    table[8] = double_fault_descriptor;
    let table_ptr: *const GateDescriptor = table.as_ptr();

    let idt = Idt {
        size: 256 * 8 - 1,
        offset: table_ptr as u32,
    };

    println!("{:?}", read_idtr());
    unsafe {
        asm!(r#"
             lidt ({idt})
             sti
             "#,
             idt = in (reg) &idt,
             options(att_syntax));
    }

    println!("{:?}", read_idtr());
}
