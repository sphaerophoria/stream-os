use core::{
    fmt::Write,
    sync::atomic::{AtomicU8, AtomicUsize, Ordering},
};

// NOTE: This should be safe, TerminalWriter is synbc, and AtomicPtr satisfies the constraint on a
// global. We initialize this at the start of main, so everyone else should be able to access the
// pointer without crashing
pub static TERMINAL_WRITER: TerminalWriter = TerminalWriter::new();

macro_rules! print {
    ($($arg:tt)*) => {
        #[allow(unused_unsafe)]
        unsafe {
            use $crate::vga::TerminalWriter;
            use core::fmt::Write as FmtWrite;
            let writer = &$crate::vga::TERMINAL_WRITER as *const TerminalWriter;
            // write_fmt needs writer as &mut, but we only access it as *const. Cast to fulfil the
            // API requirements
            let writer = writer as *mut TerminalWriter;
            write!(&mut *(writer), $($arg)*).expect("Failed to print")
        }
    }
}

macro_rules! println {
    ($($arg:tt)*) => {
        print!($($arg)*);
        print!("\n");
    }
}

/* Hardware text mode color constants. */
#[allow(dead_code)]
enum VgaColor {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGrey = 7,
    DarkGrey = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    LightMagenta = 13,
    LightBrown = 14,
    White = 15,
}

const fn vga_entry_color(fg: VgaColor, bg: VgaColor) -> u8 {
    fg as u8 | (bg as u8) << 4
}

const fn vga_entry(uc: u8, color: u8) -> u16 {
    uc as u16 | (color as u16) << 8
}

const VGA_WIDTH: usize = 80;
const VGA_HEIGHT: usize = 25;

pub struct TerminalWriter {
    terminal_pos: AtomicUsize,
    terminal_color: AtomicU8,
    terminal_buffer: *mut u16,
}

impl TerminalWriter {
    const fn new() -> TerminalWriter {
        let terminal_pos = AtomicUsize::new(0);
        let terminal_color = vga_entry_color(VgaColor::LightGrey, VgaColor::Black);
        let terminal_buffer = 0xB8000 as *mut u16;

        TerminalWriter {
            terminal_pos,
            terminal_color: AtomicU8::new(terminal_color),
            terminal_buffer,
        }
    }

    pub fn init() -> &'static TerminalWriter {
        let color = TERMINAL_WRITER.terminal_color.load(Ordering::Relaxed);
        for y in 0..VGA_HEIGHT {
            for x in 0..VGA_WIDTH {
                let index = y * VGA_WIDTH + x;
                unsafe {
                    *TERMINAL_WRITER.terminal_buffer.add(index) = vga_entry(b' ', color);
                }
            }
        }
        &TERMINAL_WRITER
    }

    #[allow(dead_code)]
    pub fn set_color(&self, color: u8) {
        self.terminal_color.store(color, Ordering::Relaxed);
    }

    fn putchar(&self, c: u8) {
        if c == b'\n' {
            let mut pos = self.terminal_pos.load(Ordering::Relaxed);
            pos += VGA_WIDTH - (pos % VGA_WIDTH);
            self.terminal_pos.store(pos, Ordering::Relaxed);
            return;
        }

        let color = self.terminal_color.load(Ordering::Relaxed);
        // Increment col as we always try to advance the cursor after we write
        let pos = self.terminal_pos.fetch_add(1, Ordering::Relaxed);
        unsafe {
            *self.terminal_buffer.add(pos) = vga_entry(c, color);
        }
    }

    fn write(&self, data: &[u8]) {
        for c in data {
            self.putchar(*c);
        }
    }
}

impl Write for TerminalWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.write(s.as_bytes());
        Ok(())
    }
}

unsafe impl Sync for TerminalWriter {}
