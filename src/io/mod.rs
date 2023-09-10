use core::arch::asm;

macro_rules! print {
    ($($arg:tt)*) => {
        #[allow(unused_unsafe)]
        unsafe {
            use $crate::io::vga::TerminalWriter;
            use core::fmt::Write as FmtWrite;
            let writer = &$crate::io::vga::TERMINAL_WRITER as *const TerminalWriter;
            // write_fmt needs writer as &mut, but we only access it as *const. Cast to fulfil the
            // API requirements
            let writer = writer as *mut TerminalWriter;
            write!(&mut *(writer), $($arg)*).expect("Failed to print to vga");

            let writer = &$crate::io::serial::SERIAL as *const $crate::io::serial::Serial;
            let writer = writer as *mut $crate::io::serial::Serial;
            write!(&mut *(writer), $($arg)*).expect("Failed to print to serial");
        }
    }
}

macro_rules! println {
    ($($arg:tt)*) => {
        print!($($arg)*);
        print!("\n");
    }
}

#[macro_use]
pub mod vga;
pub mod serial;

unsafe fn inb(addr: u16) -> u8 {
    let mut ret;
    asm!(r#"
        .att_syntax
        in %dx, %al
        "#,
        in("dx") addr,
        out("al") ret);
    ret
}

unsafe fn outb(addr: u16, val: u8) {
    asm!(r#"
        .att_syntax
        out %al, %dx
        "#,
        in("dx") addr,
        in("al") val);
}

pub unsafe fn exit(code: u8) {
    const ISA_DEBUG_EXIT_PORT: u16 = 0xf4;
    outb(ISA_DEBUG_EXIT_PORT, code);
}
