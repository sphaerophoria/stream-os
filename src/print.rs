macro_rules! print {
    ($($arg:tt)*) => {
        #[allow(unused_unsafe)]
        unsafe {
            let printer = &mut *crate::io::PRINTER.inner.get();
            if let Some(printer) = printer.as_mut() {
                let _ = core::fmt::write(&mut **printer, format_args!($($arg)*));
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
