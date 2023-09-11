use core::fmt::Write;

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
    terminal_pos: usize,
    terminal_color: u8,
    terminal_buffer: &'static mut [u16],
}

impl TerminalWriter {
    pub fn new() -> TerminalWriter {
        let terminal_pos = 0;
        let terminal_color = vga_entry_color(VgaColor::LightGrey, VgaColor::Black);
        let terminal_buffer =
            unsafe { core::slice::from_raw_parts_mut(0xB8000 as *mut u16, VGA_WIDTH * VGA_HEIGHT) };

        for y in 0..VGA_HEIGHT {
            for x in 0..VGA_WIDTH {
                let index = y * VGA_WIDTH + x;
                terminal_buffer[index] = vga_entry(b' ', terminal_color);
            }
        }

        TerminalWriter {
            terminal_pos,
            terminal_color,
            terminal_buffer,
        }
    }

    #[allow(dead_code)]
    pub fn set_color(&mut self, color: u8) {
        self.terminal_color = color;
    }

    fn putchar(&mut self, c: u8) {
        if c == b'\n' {
            self.terminal_pos += VGA_WIDTH - (self.terminal_pos % VGA_WIDTH);
            return;
        }

        self.terminal_buffer[self.terminal_pos] = vga_entry(c, self.terminal_color);
        self.terminal_pos += 1;
        self.terminal_pos %= VGA_WIDTH * VGA_HEIGHT;
    }

    fn write(&mut self, data: &[u8]) {
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
