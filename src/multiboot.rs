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

#[repr(C, packed)]
pub struct MultibootMmapEntry {
    size: u32,
    addr_low: u32,
    addr_high: u32,
    len_low: u32,
    len_high: u32,
    typ: u32,
}

pub unsafe fn print_mmap_sections(info: *const MultibootInfo) {
    let mmap_length = (*info).mmap_length;
    println!("Available memory segments...");
    println!("mmap_length: {mmap_length}");
    for i in 0..(*info).mmap_length {
        let p = ((*info).mmap_addr + core::mem::size_of::<MultibootMmapEntry>() as u32 * i)
            as *const MultibootMmapEntry;
        let len = (*p).len_low;
        let size = (*p).size;
        if size == 0 {
            break;
        }
        let addr = (*p).addr_low;
        println!("size: {size}, len: {len}, addr: {addr}");
    }
}
