use crate::multiboot::FrameBufferInfo;

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

    pub fn set_pixel(&mut self, row: u32, col: u32, r: f32, g: f32, b: f32) {
        let r = self.convert_color(r);
        let g = self.convert_color(g);
        let b = self.convert_color(b);

        let offset = row as usize * self.info.pitch as usize
            + col as usize * self.info.bytes_per_pix as usize;
        unsafe {
            let addr = self.info.addr.add(offset);
            addr.add(self.info.red_offset as usize).write_volatile(r);
            addr.add(self.info.green_offset as usize).write_volatile(g);
            addr.add(self.info.blue_offset as usize).write_volatile(b);
        }
    }

    fn convert_color(&self, val: f32) -> u8 {
        (val * ((1 << self.info.color_size) - 1) as f32) as u8
    }
}
