use crate::multiboot2::FrameBufferInfo;

#[derive(Copy, Clone)]
pub struct Color(u32);

pub struct FrameBuffer {
    info: FrameBufferInfo,
}

impl FrameBuffer {
    pub fn new(info: FrameBufferInfo) -> FrameBuffer {
        FrameBuffer { info }
    }

    pub fn height(&self) -> u32 {
        self.info.height
    }

    pub fn width(&self) -> u32 {
        self.info.width
    }

    pub fn convert_color(&self, r: f32, g: f32, b: f32) -> Color {
        // R [0, 1] -> [0, 255]
        // G [0, 1] -> [0, 255]
        // B [0, 1] -> [0, 255]
        // -> u32
        // B | G << 8 | R << 16
        // [B, G, R, 0]
        let to_u8 = |val| (val * ((1 << self.info.color_size) - 1) as f32) as u8;

        let color_u32 = (to_u8(b) as u32) << (self.info.blue_offset * 8)
            | (to_u8(g) as u32) << (self.info.green_offset * 8)
            | (to_u8(r) as u32) << (self.info.red_offset * 8);

        Color(color_u32)
    }

    pub fn set_pixel(&mut self, row: u32, col: u32, color: Color) {
        let offset = row as usize * self.info.pitch as usize
            + col as usize * self.info.bytes_per_pix as usize;
        unsafe {
            let addr = self.info.addr.add(offset) as *mut u32;
            addr.write_volatile(color.0);
        }
    }
}

unsafe impl Send for FrameBuffer {}
