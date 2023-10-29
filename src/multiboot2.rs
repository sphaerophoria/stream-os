use crate::acpi::Rsdp;

use core::marker::PhantomData;

#[repr(C)]
struct BootInfoHeader {
    total_size: u32,
    reserved: u32,
}

#[repr(C)]
#[derive(Debug)]
struct ImageLoad {
    typ: u32,
    size: u32,
    load_base_addr: u32,
}

#[repr(C)]
#[derive(Debug)]
pub struct MemoryMapEntry {
    pub addr: u64,
    pub len: u64,
    pub typ: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Debug)]
struct MemoryMap {
    typ: u32,
    size: u32,
    entry_size: u32,
    entry_version: u32,
    entries: (),
}

impl MemoryMap {
    fn entries(&self) -> &[MemoryMapEntry] {
        let entries = &self.entries as *const ();
        let entries = entries as *const MemoryMapEntry;
        unsafe {
            core::slice::from_raw_parts(
                entries,
                (self.size - 16) as usize / core::mem::size_of::<MemoryMapEntry>(),
            )
        }
    }
}

#[derive(Debug)]
#[repr(C, packed)]
struct FrameBufferInfoPriv {
    typ: u32,
    size: u32,
    framebuffer_addr: u64,
    framebuffer_pitch: u32,
    framebuffer_width: u32,
    framebuffer_height: u32,
    framebuffer_bpp: u8,
    framebuffer_type: u8,
    // NOTE: Spec says reserved should be 0, however when I parse the color info, these values make
    // sense and are correct, and with reserved of u8, it's inconsistent with itself and incorrect
    reserved: u16,
    framebuffer_red_field_position: u8,
    framebuffer_red_mask_size: u8,
    framebuffer_green_field_position: u8,
    framebuffer_green_mask_size: u8,
    framebuffer_blue_field_position: u8,
    framebuffer_blue_mask_size: u8,
}

#[derive(Debug)]
#[repr(C)]
struct RdspTag {
    typ: u32,
    size: u32,
    descriptor: Rsdp,
}

#[derive(Debug)]
enum Tag<'a> {
    MemoryMap(&'a MemoryMap),
    FrameBufferInfo(&'a FrameBufferInfoPriv),
    Rsdp(&'a RdspTag),
    ImageLoad(&'a ImageLoad),
}

struct TagIterator<'a> {
    loc: *const u8,
    end: *const u8,
    _phantom: PhantomData<&'a ()>,
}

impl<'a> Iterator for TagIterator<'a> {
    type Item = Tag<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            loop {
                if self.loc == self.end {
                    return None;
                }

                let typ = *(self.loc as *const u32);
                let mut size = *(self.loc.add(4) as *const u32);
                size = (size + 7) & !7;
                let item = self.loc;
                self.loc = self.loc.add(size as usize);
                match typ {
                    0 => return None,
                    6 => {
                        let map = &*(item as *const MemoryMap);
                        return Some(Tag::MemoryMap(map));
                    }
                    8 => return Some(Tag::FrameBufferInfo(&*(item as *const FrameBufferInfoPriv))),
                    14 => return Some(Tag::Rsdp(&*(item as *const RdspTag))),
                    21 => return Some(Tag::ImageLoad(&*(item as *const ImageLoad))),
                    _ => (),
                }
            }
        }
    }
}

impl BootInfoHeader {
    fn tags(&self) -> impl Iterator<Item = Tag<'_>> {
        unsafe {
            let header_addr = self as *const BootInfoHeader;
            let loc = header_addr.add(1) as *const u8;
            let end = (header_addr as *const u8).add((*header_addr).total_size as usize);

            TagIterator {
                loc,
                end,
                _phantom: PhantomData,
            }
        }
    }
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

pub struct Multiboot2 {
    info: *const BootInfoHeader,
}

impl Multiboot2 {
    pub fn new(magic: u32, info: *const u8) -> Multiboot2 {
        assert_eq!(magic, 0x36d76289);
        let info = info as *const BootInfoHeader;

        Multiboot2 { info }
    }

    pub unsafe fn get_rsdp(&self) -> Option<&'_ Rsdp> {
        for tag in (*self.info).tags() {
            if let Tag::Rsdp(p) = tag {
                return Some(&p.descriptor);
            }
        }

        None
    }

    pub unsafe fn get_framebuffer_info(&self) -> Option<FrameBufferInfo> {
        for tag in (*self.info).tags() {
            if let Tag::FrameBufferInfo(i) = tag {
                assert_eq!(i.framebuffer_red_mask_size, i.framebuffer_green_mask_size);
                assert_eq!(i.framebuffer_red_mask_size, i.framebuffer_blue_mask_size);
                assert!(i.framebuffer_red_field_position % 8 == 0);
                assert!(i.framebuffer_green_field_position % 8 == 0);
                assert!(i.framebuffer_blue_field_position % 8 == 0);
                assert!(i.framebuffer_bpp % 8 == 0);

                return Some(FrameBufferInfo {
                    color_size: i.framebuffer_red_mask_size,
                    red_offset: i.framebuffer_red_field_position / 8,
                    blue_offset: i.framebuffer_blue_field_position / 8,
                    green_offset: i.framebuffer_green_field_position / 8,
                    bytes_per_pix: i.framebuffer_bpp / 8,
                    width: i.framebuffer_width,
                    pitch: i.framebuffer_pitch,
                    height: i.framebuffer_height,
                    addr: i.framebuffer_addr as *mut u8,
                });
            }
        }

        None
    }

    pub unsafe fn get_mmap_addrs(&self) -> &[MemoryMapEntry] {
        for tag in (*self.info).tags() {
            if let Tag::MemoryMap(m) = tag {
                return m.entries();
            }
        }
        panic!("Failed to find mmap entries");
    }
}
