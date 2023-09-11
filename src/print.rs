macro_rules! print {
    ($($arg:tt)*) => {
        #[allow(unused_unsafe)]
        unsafe {
            let mut sinks = crate::io::STDOUT_SINKS.borrow_mut();
            use core::fmt::Write as FmtWrite;
            if let Some(vga) = &mut sinks.vga {
                write!(vga, $($arg)*).expect("Failed to print to vga");
            }

            if let Some(serial) = &mut sinks.serial {
                write!(serial, $($arg)*).expect("Failed to print to serial");
            }
        }
    }
}

macro_rules! println {
    ($($arg:tt)*) => {
        print!($($arg)*);
        print!("\n");
    }
}
