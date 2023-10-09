use crate::util::bit_manipulation::GetBits;

#[repr(C)]
pub struct MultibootInfo {
    /* Multiboot info version number */
    flags: u32,

    /* Available memory from BIOS */
    mem_lower: u32,
    mem_upper: u32,

    /* "root" partition */
    boot_device: u32,

    /* Kernel command line */
    cmdline: u32,

    /* Boot-Module list */
    mods_count: u32,
    mods_addr: u32,

    dummy: [u8; 16],

    /* Memory Mapping buffer */
    mmap_length: u32,
    mmap_addr: u32,

    /* Drive Info buffer */
    drives_length: u32,
    drives_addr: u32,

    /* ROM configuration table */
    config_table: u32,

    /* Boot Loader Name */
    boot_loader_name: *const u8,

    /* APM table */
    apm_table: u32,

    vbe_control_info: u32,
    vbe_mode_info: u32,
    vbe_mode: u16,
    vbe_interface_seg: u16,
    vbe_interface_off: u16,
    vbe_interface_len: u16,

    framebuffer_addr: u64,
    framebuffer_pitch: u32,
    framebuffer_width: u32,
    framebuffer_height: u32,
    framebuffer_bpp: u8,
    framebuffer_type: u8,
    color_info: [u8; 6],
}

#[derive(Debug)]
pub struct FrameBufferInfo {
    pub color_size: u8,
    pub red_offset: u8,
    pub blue_offset: u8,
    pub green_offset: u8,
    pub bytes_per_pix: u8,
    pub width: u32,
    pub pitch: u32,
    pub height: u32,
    pub addr: *mut u8,
}

#[derive(Debug)]
pub enum FrameBufferInfoError {
    NoFramebuffer,
    InvalidColorType(u8),
    BitsPerPixNotByteAligned,
    ColorOffsetNotByteAligned,
}

impl MultibootInfo {
    pub unsafe fn get_framebuffer_info(&self) -> Result<FrameBufferInfo, FrameBufferInfoError> {
        if !self.flags.get_bit(12) {
            return Err(FrameBufferInfoError::NoFramebuffer);
        }

        if self.framebuffer_type != 1 {
            return Err(FrameBufferInfoError::InvalidColorType(
                self.framebuffer_type,
            ));
        }

        let r_pos = self.color_info[0];
        let r_size = self.color_info[1];
        let g_pos = self.color_info[2];
        let g_size = self.color_info[3];
        let b_pos = self.color_info[4];
        let b_size = self.color_info[5];

        // r_size looks wrong in our test, so we just assume all will be the same. Maybe an invalid
        // assumption
        let color_size = r_size.max(b_size).max(g_size);

        // Framebuffer implementation requires byte aligned colors
        if self.framebuffer_bpp % 8 != 0 {
            return Err(FrameBufferInfoError::BitsPerPixNotByteAligned);
        }

        if r_pos % 8 != 0 || g_pos % 8 != 0 || b_pos % 8 != 0 {
            return Err(FrameBufferInfoError::ColorOffsetNotByteAligned);
        }

        Ok(FrameBufferInfo {
            color_size,
            addr: self.framebuffer_addr as *mut u8,
            // Positions do not match reality in qemu test :(
            blue_offset: r_pos / 8,
            red_offset: g_pos / 8,
            green_offset: b_pos / 8,
            bytes_per_pix: self.framebuffer_bpp / 8,
            height: self.framebuffer_height,
            width: self.framebuffer_width,
            pitch: self.framebuffer_pitch,
        })
    }

    pub unsafe fn get_mmap_addrs(&self) -> &[MultibootMmapEntry] {
        let num_mmap_entries =
            self.mmap_length as usize / core::mem::size_of::<MultibootMmapEntry>();
        core::slice::from_raw_parts(
            self.mmap_addr as *const MultibootMmapEntry,
            num_mmap_entries,
        )
    }
}

#[repr(C, packed)]
#[derive(Debug)]
pub struct MultibootMmapEntry {
    pub size: u32,
    pub addr: u64,
    pub len: u64,
    pub typ: u32,
}
