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

macro_rules! no_err_interrupt_handler {
    ($num: expr) => {
        paste::paste! {
            extern "x86-interrupt" fn [<interrupt_handler_ $num>](_frame: InterruptFrame) {
                generic_interrupt_handler($num);
            }
        }
    };
}

macro_rules! err_interrupt_handler {
    ($num: expr) => {
        paste::paste! {
            extern "x86-interrupt" fn [<interrupt_handler_ $num>](_frame: InterruptFrame, _error: u32) {
                generic_interrupt_handler($num);
            }
        }
    }
}

macro_rules! insert_gate_descriptor {
    ($num: expr) => {
        paste::paste! {
            #[allow(clippy::fn_to_numeric_cast)]
            let descriptor = GateDescriptor::new(GateDescriptorNewArgs {
                offset: [<interrupt_handler_ $num>] as u32,
                segment_selector: 0x08,
                gate_type: 0b1111,
                dpl: 0,
                p: true,
            });

            let mut table = INTERRUPT_TABLE.inner.borrow_mut();
            table[$num] = descriptor;
            drop(table);
        }
    };
}

no_err_interrupt_handler!(0);
no_err_interrupt_handler!(1);
no_err_interrupt_handler!(2);
no_err_interrupt_handler!(3);
no_err_interrupt_handler!(4);
no_err_interrupt_handler!(5);
no_err_interrupt_handler!(6);
no_err_interrupt_handler!(7);
err_interrupt_handler!(8);
no_err_interrupt_handler!(9);
err_interrupt_handler!(10);
err_interrupt_handler!(11);
err_interrupt_handler!(12);
err_interrupt_handler!(13);
err_interrupt_handler!(14);
no_err_interrupt_handler!(15);
no_err_interrupt_handler!(16);
err_interrupt_handler!(17);
no_err_interrupt_handler!(18);
no_err_interrupt_handler!(19);
no_err_interrupt_handler!(20);
err_interrupt_handler!(21);
no_err_interrupt_handler!(22);
no_err_interrupt_handler!(23);
no_err_interrupt_handler!(24);
no_err_interrupt_handler!(25);
no_err_interrupt_handler!(26);
no_err_interrupt_handler!(27);
no_err_interrupt_handler!(28);
err_interrupt_handler!(29);
err_interrupt_handler!(30);
no_err_interrupt_handler!(31);
no_err_interrupt_handler!(32);
no_err_interrupt_handler!(33);
no_err_interrupt_handler!(34);
no_err_interrupt_handler!(35);
no_err_interrupt_handler!(36);
no_err_interrupt_handler!(37);
no_err_interrupt_handler!(38);
no_err_interrupt_handler!(39);
no_err_interrupt_handler!(40);
no_err_interrupt_handler!(41);
no_err_interrupt_handler!(42);
no_err_interrupt_handler!(43);
no_err_interrupt_handler!(44);
no_err_interrupt_handler!(45);
no_err_interrupt_handler!(46);
no_err_interrupt_handler!(47);
no_err_interrupt_handler!(48);
no_err_interrupt_handler!(49);
no_err_interrupt_handler!(50);
no_err_interrupt_handler!(51);
no_err_interrupt_handler!(52);
no_err_interrupt_handler!(53);
no_err_interrupt_handler!(54);
no_err_interrupt_handler!(55);
no_err_interrupt_handler!(56);
no_err_interrupt_handler!(57);
no_err_interrupt_handler!(58);
no_err_interrupt_handler!(59);
no_err_interrupt_handler!(60);
no_err_interrupt_handler!(61);
no_err_interrupt_handler!(62);
no_err_interrupt_handler!(63);
no_err_interrupt_handler!(64);
no_err_interrupt_handler!(65);
no_err_interrupt_handler!(66);
no_err_interrupt_handler!(67);
no_err_interrupt_handler!(68);
no_err_interrupt_handler!(69);
no_err_interrupt_handler!(70);
no_err_interrupt_handler!(71);
no_err_interrupt_handler!(72);
no_err_interrupt_handler!(73);
no_err_interrupt_handler!(74);
no_err_interrupt_handler!(75);
no_err_interrupt_handler!(76);
no_err_interrupt_handler!(77);
no_err_interrupt_handler!(78);
no_err_interrupt_handler!(79);
no_err_interrupt_handler!(80);
no_err_interrupt_handler!(81);
no_err_interrupt_handler!(82);
no_err_interrupt_handler!(83);
no_err_interrupt_handler!(84);
no_err_interrupt_handler!(85);
no_err_interrupt_handler!(86);
no_err_interrupt_handler!(87);
no_err_interrupt_handler!(88);
no_err_interrupt_handler!(89);
no_err_interrupt_handler!(90);
no_err_interrupt_handler!(91);
no_err_interrupt_handler!(92);
no_err_interrupt_handler!(93);
no_err_interrupt_handler!(94);
no_err_interrupt_handler!(95);
no_err_interrupt_handler!(96);
no_err_interrupt_handler!(97);
no_err_interrupt_handler!(98);
no_err_interrupt_handler!(99);
no_err_interrupt_handler!(100);
no_err_interrupt_handler!(101);
no_err_interrupt_handler!(102);
no_err_interrupt_handler!(103);
no_err_interrupt_handler!(104);
no_err_interrupt_handler!(105);
no_err_interrupt_handler!(106);
no_err_interrupt_handler!(107);
no_err_interrupt_handler!(108);
no_err_interrupt_handler!(109);
no_err_interrupt_handler!(110);
no_err_interrupt_handler!(111);
no_err_interrupt_handler!(112);
no_err_interrupt_handler!(113);
no_err_interrupt_handler!(114);
no_err_interrupt_handler!(115);
no_err_interrupt_handler!(116);
no_err_interrupt_handler!(117);
no_err_interrupt_handler!(118);
no_err_interrupt_handler!(119);
no_err_interrupt_handler!(120);
no_err_interrupt_handler!(121);
no_err_interrupt_handler!(122);
no_err_interrupt_handler!(123);
no_err_interrupt_handler!(124);
no_err_interrupt_handler!(125);
no_err_interrupt_handler!(126);
no_err_interrupt_handler!(127);
no_err_interrupt_handler!(128);
no_err_interrupt_handler!(129);
no_err_interrupt_handler!(130);
no_err_interrupt_handler!(131);
no_err_interrupt_handler!(132);
no_err_interrupt_handler!(133);
no_err_interrupt_handler!(134);
no_err_interrupt_handler!(135);
no_err_interrupt_handler!(136);
no_err_interrupt_handler!(137);
no_err_interrupt_handler!(138);
no_err_interrupt_handler!(139);
no_err_interrupt_handler!(140);
no_err_interrupt_handler!(141);
no_err_interrupt_handler!(142);
no_err_interrupt_handler!(143);
no_err_interrupt_handler!(144);
no_err_interrupt_handler!(145);
no_err_interrupt_handler!(146);
no_err_interrupt_handler!(147);
no_err_interrupt_handler!(148);
no_err_interrupt_handler!(149);
no_err_interrupt_handler!(150);
no_err_interrupt_handler!(151);
no_err_interrupt_handler!(152);
no_err_interrupt_handler!(153);
no_err_interrupt_handler!(154);
no_err_interrupt_handler!(155);
no_err_interrupt_handler!(156);
no_err_interrupt_handler!(157);
no_err_interrupt_handler!(158);
no_err_interrupt_handler!(159);
no_err_interrupt_handler!(160);
no_err_interrupt_handler!(161);
no_err_interrupt_handler!(162);
no_err_interrupt_handler!(163);
no_err_interrupt_handler!(164);
no_err_interrupt_handler!(165);
no_err_interrupt_handler!(166);
no_err_interrupt_handler!(167);
no_err_interrupt_handler!(168);
no_err_interrupt_handler!(169);
no_err_interrupt_handler!(170);
no_err_interrupt_handler!(171);
no_err_interrupt_handler!(172);
no_err_interrupt_handler!(173);
no_err_interrupt_handler!(174);
no_err_interrupt_handler!(175);
no_err_interrupt_handler!(176);
no_err_interrupt_handler!(177);
no_err_interrupt_handler!(178);
no_err_interrupt_handler!(179);
no_err_interrupt_handler!(180);
no_err_interrupt_handler!(181);
no_err_interrupt_handler!(182);
no_err_interrupt_handler!(183);
no_err_interrupt_handler!(184);
no_err_interrupt_handler!(185);
no_err_interrupt_handler!(186);
no_err_interrupt_handler!(187);
no_err_interrupt_handler!(188);
no_err_interrupt_handler!(189);
no_err_interrupt_handler!(190);
no_err_interrupt_handler!(191);
no_err_interrupt_handler!(192);
no_err_interrupt_handler!(193);
no_err_interrupt_handler!(194);
no_err_interrupt_handler!(195);
no_err_interrupt_handler!(196);
no_err_interrupt_handler!(197);
no_err_interrupt_handler!(198);
no_err_interrupt_handler!(199);
no_err_interrupt_handler!(200);
no_err_interrupt_handler!(201);
no_err_interrupt_handler!(202);
no_err_interrupt_handler!(203);
no_err_interrupt_handler!(204);
no_err_interrupt_handler!(205);
no_err_interrupt_handler!(206);
no_err_interrupt_handler!(207);
no_err_interrupt_handler!(208);
no_err_interrupt_handler!(209);
no_err_interrupt_handler!(210);
no_err_interrupt_handler!(211);
no_err_interrupt_handler!(212);
no_err_interrupt_handler!(213);
no_err_interrupt_handler!(214);
no_err_interrupt_handler!(215);
no_err_interrupt_handler!(216);
no_err_interrupt_handler!(217);
no_err_interrupt_handler!(218);
no_err_interrupt_handler!(219);
no_err_interrupt_handler!(220);
no_err_interrupt_handler!(221);
no_err_interrupt_handler!(222);
no_err_interrupt_handler!(223);
no_err_interrupt_handler!(224);
no_err_interrupt_handler!(225);
no_err_interrupt_handler!(226);
no_err_interrupt_handler!(227);
no_err_interrupt_handler!(228);
no_err_interrupt_handler!(229);
no_err_interrupt_handler!(230);
no_err_interrupt_handler!(231);
no_err_interrupt_handler!(232);
no_err_interrupt_handler!(233);
no_err_interrupt_handler!(234);
no_err_interrupt_handler!(235);
no_err_interrupt_handler!(236);
no_err_interrupt_handler!(237);
no_err_interrupt_handler!(238);
no_err_interrupt_handler!(239);
no_err_interrupt_handler!(240);
no_err_interrupt_handler!(241);
no_err_interrupt_handler!(242);
no_err_interrupt_handler!(243);
no_err_interrupt_handler!(244);
no_err_interrupt_handler!(245);
no_err_interrupt_handler!(246);
no_err_interrupt_handler!(247);
no_err_interrupt_handler!(248);
no_err_interrupt_handler!(249);
no_err_interrupt_handler!(250);
no_err_interrupt_handler!(251);
no_err_interrupt_handler!(252);
no_err_interrupt_handler!(253);
no_err_interrupt_handler!(254);
no_err_interrupt_handler!(255);

fn generic_interrupt_handler(interrupt_number: u8) {
    let handlers = INTERRUPT_HANDLER_DATA.handlers.lock();
    let f = match handlers
        .as_ref()
        .expect("interrupt handlers not initialized")
        .get(&interrupt_number)
    {
        Some(f) => f,
        None => {
            panic!("no handler for interrupt {}", interrupt_number)
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

    pub fn register<F: Fn() + 'static>(&self, interrupt_num: IrqId, f: F) {
        let mut ports = self.ports.lock();
        let ports = ports.as_mut().expect("InterruptHandlers not initialized");

        let interrupt_num = match interrupt_num {
            IrqId::Internal(i) => i,
            IrqId::Pic1(i) => {
                let mut mask = ports.pic1_data.readb();
                mask.set_bit(i, false);
                ports.pic1_data.writeb(mask);
                i + PIC1_OFFSET
            }
            IrqId::Pic2(i) => {
                let mut mask = ports.pic2_data.readb();
                mask.set_bit(i, false);
                ports.pic2_data.writeb(mask);
                i + PIC2_OFFSET
            }
        };

        self.handlers
            .lock()
            .as_mut()
            .expect("InterruptHandlers not initailized")
            .insert(interrupt_num, Box::new(f));
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

    insert_gate_descriptor!(0);
    insert_gate_descriptor!(1);
    insert_gate_descriptor!(2);
    insert_gate_descriptor!(3);
    insert_gate_descriptor!(4);
    insert_gate_descriptor!(5);
    insert_gate_descriptor!(6);
    insert_gate_descriptor!(7);
    insert_gate_descriptor!(8);
    insert_gate_descriptor!(9);
    insert_gate_descriptor!(10);
    insert_gate_descriptor!(11);
    insert_gate_descriptor!(12);
    insert_gate_descriptor!(13);
    insert_gate_descriptor!(14);
    insert_gate_descriptor!(15);
    insert_gate_descriptor!(16);
    insert_gate_descriptor!(17);
    insert_gate_descriptor!(18);
    insert_gate_descriptor!(19);
    insert_gate_descriptor!(20);
    insert_gate_descriptor!(21);
    insert_gate_descriptor!(22);
    insert_gate_descriptor!(23);
    insert_gate_descriptor!(24);
    insert_gate_descriptor!(25);
    insert_gate_descriptor!(26);
    insert_gate_descriptor!(27);
    insert_gate_descriptor!(28);
    insert_gate_descriptor!(29);
    insert_gate_descriptor!(30);
    insert_gate_descriptor!(31);
    insert_gate_descriptor!(32);
    insert_gate_descriptor!(33);
    insert_gate_descriptor!(34);
    insert_gate_descriptor!(35);
    insert_gate_descriptor!(36);
    insert_gate_descriptor!(37);
    insert_gate_descriptor!(38);
    insert_gate_descriptor!(39);
    insert_gate_descriptor!(40);
    insert_gate_descriptor!(41);
    insert_gate_descriptor!(42);
    insert_gate_descriptor!(43);
    insert_gate_descriptor!(44);
    insert_gate_descriptor!(45);
    insert_gate_descriptor!(46);
    insert_gate_descriptor!(47);
    insert_gate_descriptor!(48);
    insert_gate_descriptor!(49);
    insert_gate_descriptor!(50);
    insert_gate_descriptor!(51);
    insert_gate_descriptor!(52);
    insert_gate_descriptor!(53);
    insert_gate_descriptor!(54);
    insert_gate_descriptor!(55);
    insert_gate_descriptor!(56);
    insert_gate_descriptor!(57);
    insert_gate_descriptor!(58);
    insert_gate_descriptor!(59);
    insert_gate_descriptor!(60);
    insert_gate_descriptor!(61);
    insert_gate_descriptor!(62);
    insert_gate_descriptor!(63);
    insert_gate_descriptor!(64);
    insert_gate_descriptor!(65);
    insert_gate_descriptor!(66);
    insert_gate_descriptor!(67);
    insert_gate_descriptor!(68);
    insert_gate_descriptor!(69);
    insert_gate_descriptor!(70);
    insert_gate_descriptor!(71);
    insert_gate_descriptor!(72);
    insert_gate_descriptor!(73);
    insert_gate_descriptor!(74);
    insert_gate_descriptor!(75);
    insert_gate_descriptor!(76);
    insert_gate_descriptor!(77);
    insert_gate_descriptor!(78);
    insert_gate_descriptor!(79);
    insert_gate_descriptor!(80);
    insert_gate_descriptor!(81);
    insert_gate_descriptor!(82);
    insert_gate_descriptor!(83);
    insert_gate_descriptor!(84);
    insert_gate_descriptor!(85);
    insert_gate_descriptor!(86);
    insert_gate_descriptor!(87);
    insert_gate_descriptor!(88);
    insert_gate_descriptor!(89);
    insert_gate_descriptor!(90);
    insert_gate_descriptor!(91);
    insert_gate_descriptor!(92);
    insert_gate_descriptor!(93);
    insert_gate_descriptor!(94);
    insert_gate_descriptor!(95);
    insert_gate_descriptor!(96);
    insert_gate_descriptor!(97);
    insert_gate_descriptor!(98);
    insert_gate_descriptor!(99);
    insert_gate_descriptor!(100);
    insert_gate_descriptor!(101);
    insert_gate_descriptor!(102);
    insert_gate_descriptor!(103);
    insert_gate_descriptor!(104);
    insert_gate_descriptor!(105);
    insert_gate_descriptor!(106);
    insert_gate_descriptor!(107);
    insert_gate_descriptor!(108);
    insert_gate_descriptor!(109);
    insert_gate_descriptor!(110);
    insert_gate_descriptor!(111);
    insert_gate_descriptor!(112);
    insert_gate_descriptor!(113);
    insert_gate_descriptor!(114);
    insert_gate_descriptor!(115);
    insert_gate_descriptor!(116);
    insert_gate_descriptor!(117);
    insert_gate_descriptor!(118);
    insert_gate_descriptor!(119);
    insert_gate_descriptor!(120);
    insert_gate_descriptor!(121);
    insert_gate_descriptor!(122);
    insert_gate_descriptor!(123);
    insert_gate_descriptor!(124);
    insert_gate_descriptor!(125);
    insert_gate_descriptor!(126);
    insert_gate_descriptor!(127);
    insert_gate_descriptor!(128);
    insert_gate_descriptor!(129);
    insert_gate_descriptor!(130);
    insert_gate_descriptor!(131);
    insert_gate_descriptor!(132);
    insert_gate_descriptor!(133);
    insert_gate_descriptor!(134);
    insert_gate_descriptor!(135);
    insert_gate_descriptor!(136);
    insert_gate_descriptor!(137);
    insert_gate_descriptor!(138);
    insert_gate_descriptor!(139);
    insert_gate_descriptor!(140);
    insert_gate_descriptor!(141);
    insert_gate_descriptor!(142);
    insert_gate_descriptor!(143);
    insert_gate_descriptor!(144);
    insert_gate_descriptor!(145);
    insert_gate_descriptor!(146);
    insert_gate_descriptor!(147);
    insert_gate_descriptor!(148);
    insert_gate_descriptor!(149);
    insert_gate_descriptor!(150);
    insert_gate_descriptor!(151);
    insert_gate_descriptor!(152);
    insert_gate_descriptor!(153);
    insert_gate_descriptor!(154);
    insert_gate_descriptor!(155);
    insert_gate_descriptor!(156);
    insert_gate_descriptor!(157);
    insert_gate_descriptor!(158);
    insert_gate_descriptor!(159);
    insert_gate_descriptor!(160);
    insert_gate_descriptor!(161);
    insert_gate_descriptor!(162);
    insert_gate_descriptor!(163);
    insert_gate_descriptor!(164);
    insert_gate_descriptor!(165);
    insert_gate_descriptor!(166);
    insert_gate_descriptor!(167);
    insert_gate_descriptor!(168);
    insert_gate_descriptor!(169);
    insert_gate_descriptor!(170);
    insert_gate_descriptor!(171);
    insert_gate_descriptor!(172);
    insert_gate_descriptor!(173);
    insert_gate_descriptor!(174);
    insert_gate_descriptor!(175);
    insert_gate_descriptor!(176);
    insert_gate_descriptor!(177);
    insert_gate_descriptor!(178);
    insert_gate_descriptor!(179);
    insert_gate_descriptor!(180);
    insert_gate_descriptor!(181);
    insert_gate_descriptor!(182);
    insert_gate_descriptor!(183);
    insert_gate_descriptor!(184);
    insert_gate_descriptor!(185);
    insert_gate_descriptor!(186);
    insert_gate_descriptor!(187);
    insert_gate_descriptor!(188);
    insert_gate_descriptor!(189);
    insert_gate_descriptor!(190);
    insert_gate_descriptor!(191);
    insert_gate_descriptor!(192);
    insert_gate_descriptor!(193);
    insert_gate_descriptor!(194);
    insert_gate_descriptor!(195);
    insert_gate_descriptor!(196);
    insert_gate_descriptor!(197);
    insert_gate_descriptor!(198);
    insert_gate_descriptor!(199);
    insert_gate_descriptor!(200);
    insert_gate_descriptor!(201);
    insert_gate_descriptor!(202);
    insert_gate_descriptor!(203);
    insert_gate_descriptor!(204);
    insert_gate_descriptor!(205);
    insert_gate_descriptor!(206);
    insert_gate_descriptor!(207);
    insert_gate_descriptor!(208);
    insert_gate_descriptor!(209);
    insert_gate_descriptor!(210);
    insert_gate_descriptor!(211);
    insert_gate_descriptor!(212);
    insert_gate_descriptor!(213);
    insert_gate_descriptor!(214);
    insert_gate_descriptor!(215);
    insert_gate_descriptor!(216);
    insert_gate_descriptor!(217);
    insert_gate_descriptor!(218);
    insert_gate_descriptor!(219);
    insert_gate_descriptor!(220);
    insert_gate_descriptor!(221);
    insert_gate_descriptor!(222);
    insert_gate_descriptor!(223);
    insert_gate_descriptor!(224);
    insert_gate_descriptor!(225);
    insert_gate_descriptor!(226);
    insert_gate_descriptor!(227);
    insert_gate_descriptor!(228);
    insert_gate_descriptor!(229);
    insert_gate_descriptor!(230);
    insert_gate_descriptor!(231);
    insert_gate_descriptor!(232);
    insert_gate_descriptor!(233);
    insert_gate_descriptor!(234);
    insert_gate_descriptor!(235);
    insert_gate_descriptor!(236);
    insert_gate_descriptor!(237);
    insert_gate_descriptor!(238);
    insert_gate_descriptor!(239);
    insert_gate_descriptor!(240);
    insert_gate_descriptor!(241);
    insert_gate_descriptor!(242);
    insert_gate_descriptor!(243);
    insert_gate_descriptor!(244);
    insert_gate_descriptor!(245);
    insert_gate_descriptor!(246);
    insert_gate_descriptor!(247);
    insert_gate_descriptor!(248);
    insert_gate_descriptor!(249);
    insert_gate_descriptor!(250);
    insert_gate_descriptor!(251);
    insert_gate_descriptor!(252);
    insert_gate_descriptor!(253);
    insert_gate_descriptor!(254);
    insert_gate_descriptor!(255);

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
