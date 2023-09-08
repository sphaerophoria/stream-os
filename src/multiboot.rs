#[repr(C, packed)]
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
}

impl MultibootInfo {
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

pub unsafe fn print_mmap_sections(info: *const MultibootInfo) {
    let num_mmap_addrs = (*info).mmap_length / core::mem::size_of::<MultibootMmapEntry>() as u32;
    let boot_loader_name_slice = core::slice::from_raw_parts((*info).boot_loader_name, 4);
    let boot_loader_str = core::str::from_utf8_unchecked(boot_loader_name_slice);
    println!("{}", boot_loader_str);
    println!("Available memory segments...");
    println!("num_mmap_addrs: {num_mmap_addrs}");
    let mut total_length = 0;
    for entry in (*info).get_mmap_addrs() {
        let len = entry.len as f32 / 1024.0;
        let size = entry.size;
        if size == 0 {
            continue;
        }
        let addr = entry.addr;
        total_length += entry.len;
        println!("size: {size}, len: {len}K, addr: {addr:#04X}");
    }
    println!("total length: {}M", total_length as f32 / 1024.0 / 1024.0);
}
