use crate::util::bit_manipulation::{GetBits, SetBits};
use core::arch::asm;

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct GdtSegment(u64);

impl GdtSegment {
    fn new(base: u32, limit: u32, access: u8, flags: u8) -> GdtSegment {
        let mut descriptor = 0u64;
        descriptor.set_bits(0, 16, limit as u64);
        descriptor.set_bits(48, 4, (limit >> 16) as u64);

        descriptor.set_bits(16, 24, base.into());
        descriptor.set_bits(56, 8, (base >> 24).into());

        descriptor.set_bits(40, 8, access.into());
        descriptor.set_bits(52, 4, flags.into());

        GdtSegment(descriptor)
    }

    fn base(&self) -> u32 {
        // Prevent unaligned access
        let data = self.0;
        let mut base = data.get_bits(16, 24);
        let upper = data.get_bits(56, 8);
        base |= upper << 24;
        base as u32
    }

    fn limit(&self) -> u32 {
        // Prevent unaligned access
        let data = self.0;
        let mut limit = data.get_bits(0, 16);
        let upper = data.get_bits(48, 4);
        limit |= upper << 16;
        limit as u32
    }

    fn access(&self) -> u8 {
        // Prevent unaligned access
        let data = self.0;
        data.get_bits(40, 8) as u8
    }

    fn flags(&self) -> u8 {
        // Prevent unaligned access
        let data = self.0;

        data.get_bits(52, 4) as u8
    }
}

#[repr(C, packed)]
struct Gdt {
    limit: u16,
    base: u32,
}

struct AccessByteParams {
    p: bool,
    dpl: u8,
    s: bool,
    e: bool,
    dc: bool,
    rw: bool,
    a: bool,
}

fn gen_access_byte(params: AccessByteParams) -> u8 {
    let mut access_byte = 0u8;
    access_byte.set_bit(7, params.p);
    access_byte.set_bits(5, 2, params.dpl);
    access_byte.set_bit(4, params.s);
    access_byte.set_bit(3, params.e);
    access_byte.set_bit(2, params.dc);
    access_byte.set_bit(1, params.rw);
    access_byte.set_bit(0, params.a);

    access_byte
}

fn get_gdt_vals() -> [GdtSegment; 3] {
    let access_byte = gen_access_byte(AccessByteParams {
        p: true,
        dpl: 0,
        s: true,
        e: true,
        dc: false,
        rw: false,
        a: true,
    });

    let code = GdtSegment::new(0, 0xffffffff, access_byte, 0b1100);

    let access_byte = gen_access_byte(AccessByteParams {
        p: true,
        dpl: 0,
        s: true,
        e: false,
        dc: false,
        rw: true,
        a: true,
    });

    let data = GdtSegment::new(0, 0xffffffff, access_byte, 0b1100);

    [GdtSegment(0), code, data]
}

fn read_gdtr() -> Gdt {
    let mut ret = core::mem::MaybeUninit::uninit();
    unsafe {
        asm!(r#"
             sgdt ({})
             "#,
             in (reg) ret.as_mut_ptr(), options(att_syntax, nostack, preserves_flags));
        ret.assume_init()
    }
}

pub unsafe fn debug_print_gdt() {
    let gdt = read_gdtr();
    let limit = gdt.limit;
    let base = gdt.base as *const GdtSegment;
    let limit = limit + 1;
    debug!("base: {base:?}, limit: {limit:#x}");
    for i in 0..(limit / 8) {
        debug!("Segment {i}");
        let segment = *base.add(i.into());
        debug!(
            "base: {:#x}, limit: {:#x}, access: {:#x}, flags: {:#x}",
            segment.base(),
            segment.limit(),
            segment.access(),
            segment.flags()
        );
    }
}

// Caller responsible for setting interrupt flags
pub unsafe fn init() {
    debug!("Initial gdt");
    debug_print_gdt();

    let entries = get_gdt_vals().to_vec().leak() as &[GdtSegment];
    let entry_ptr = entries.as_ptr();

    let limit = core::mem::size_of_val(entries) - 1;
    let gdt = Gdt {
        limit: limit as u16,
        base: entry_ptr as u32,
    };

    unsafe {
        let cpu_flags: i32;
        asm!(r#"
             pushf
             pop {cpu_flags}
             push {cpu_flags}
             popf
             "#,
             cpu_flags = out (reg) cpu_flags);

        assert_eq!(
            (cpu_flags >> 9) & 0x1,
            0,
            "Caller is responsible for disable/enabling interrupts"
        );

        asm!(r#"
             lgdt ({gdt})
             jmp $0x08, $1f
             1:
             mov $0x10, {reload_reg}
             mov {reload_reg}, %ds
             mov {reload_reg}, %es
             mov {reload_reg}, %fs
             mov {reload_reg}, %gs
             mov {reload_reg}, %ss
             "#,
             gdt = in (reg) &gdt,
             reload_reg = out (reg) _,
             options(att_syntax));
    }

    debug!("Updated gdt");
    debug_print_gdt();
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::testing::*;

    create_test!(tests_base_extraction, {
        const TEST_SEGMENT: GdtSegment = GdtSegment(0x120D00345678BEEF);
        test_eq!(TEST_SEGMENT.base(), 0x12345678);
        test_eq!(TEST_SEGMENT.limit(), 0xdbeef);
        Ok(())
    });

    create_test!(test_back_and_forth, {
        const BASE: u32 = 0x12345678;
        const LIMIT: u32 = 0xdbeef;
        let segment = GdtSegment::new(BASE, LIMIT, 0x65, 0x4);
        test_eq!(segment.base(), 0x12345678);
        test_eq!(segment.limit(), 0xdbeef);
        test_eq!(segment.access(), 0x65);
        test_eq!(segment.flags(), 0x4);
        Ok(())
    });

    create_test!(test_access_byte_code_segment, {
        let access_byte = gen_access_byte(AccessByteParams {
            p: true,
            dpl: 0,
            s: true,
            e: true,
            dc: false,
            rw: false,
            a: true,
        });
        test_eq!(access_byte, 0b10011001);
        Ok(())
    });
}
