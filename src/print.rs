macro_rules! print {
    ($($arg:tt)*) => {
        #[allow(unused_unsafe)]
        unsafe {
            let printer = &mut *crate::io::PRINTER.inner.get();
            if let Some(printer) = printer.as_mut() {
                if let Some(s) = format_args!($($arg)*).as_str() {
                    printer(s).await;
                } else {
                    printer(&alloc::format!($($arg)*)).await;
                }
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
