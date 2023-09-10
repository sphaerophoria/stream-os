use core::arch::asm;

macro_rules! print {
    ($($arg:tt)*) => {
        #[allow(unused_unsafe)]
        unsafe {
            use core::fmt::Write as FmtWrite;
            let mut writer = $crate::io::vga::TERMINAL_WRITER.borrow_mut();
            write!(writer, $($arg)*).expect("Failed to print to vga");

            let mut writer = $crate::io::serial::SerialWriter::new();
            write!(writer, $($arg)*).expect("Failed to print to serial");
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
