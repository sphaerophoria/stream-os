use core::arch::global_asm;
use elf::endian::EndianParse;

use alloc::vec;

global_asm!(
    "
        .section .bss
        saved_stack_ptr:
         .long 0

        .section .text
        .global c_exit
        c_exit:
          pop %eax
          mov saved_stack_ptr,%esp
          popal
          ret

        .global load_process

        load_process:
          pushal
          mov %esp,saved_stack_ptr
          mov 40(%esp),%eax
          push %eax
          mov 40(%esp),%eax
          call *%eax
          pop %eax
          ret",
    options(att_syntax)
);

extern "C" {
    fn load_process(buf: *const u8, vtable: *const vtable) -> core::ffi::c_void;
    fn c_exit(code: i32);
}

unsafe extern "C" fn c_print(data: *const i8) {
    let s = core::ffi::CStr::from_ptr(data);
    print!("{}", s.to_str().unwrap());
}

unsafe extern "C" fn c_panic(data: *const i8) {
    let s = core::ffi::CStr::from_ptr(data);
    panic!("{}", s.to_str().unwrap());
}

#[repr(C)]
struct vtable {
    print: unsafe extern "C" fn(data: *const i8),
    exit: unsafe extern "C" fn(code: core::ffi::c_int),
    panic: unsafe extern "C" fn(data: *const i8),
}

fn get_min_max_mapped_address<E: EndianParse>(data: &elf::ElfBytes<E>) -> (u64, u64) {
    let segments = data.segments().expect("Failed to get segments");
    let mut smallest_address = u64::MAX;
    let mut largest_address = u64::MIN;
    for segment in segments.iter().filter(|s| s.p_type == 1) {
        smallest_address = smallest_address.min(segment.p_vaddr);
        largest_address = largest_address.max(segment.p_vaddr + segment.p_memsz);
    }
    println!("address range: {}-{}", smallest_address, largest_address);
    (smallest_address, largest_address)
}

pub fn run_process(elf_file: &[u8]) {
    let data = elf::ElfBytes::<elf::endian::LittleEndian>::minimal_parse(elf_file)
        .expect("Failed to parse elf");
    let (min_addr, max_addr) = get_min_max_mapped_address(&data);

    let mut mapped_process = vec![
        0u8;
        (max_addr - min_addr)
            .try_into()
            .expect("process space > usize")
    ];

    let segments = data.segments().expect("Failed to get segments");
    for segment in segments.iter().filter(|s| s.p_type == 1) {
        let desired_v_addr = segment.p_vaddr;
        let mapped_addr = (desired_v_addr - min_addr)
            .try_into()
            .expect("Mapped address does not fit in usize");
        let source_start = segment.p_offset as usize;
        let source_end = source_start + segment.p_filesz as usize;
        let dest_end = mapped_addr + segment.p_filesz as usize;
        mapped_process[mapped_addr..dest_end].copy_from_slice(&elf_file[source_start..source_end]);
    }

    let vtable = vtable {
        print: c_print,
        exit: c_exit,
        panic: c_panic,
    };

    println!(
        "Offset for start: {}",
        (data.ehdr.e_entry - min_addr) as usize
    );

    println!(
        "Arg 1: {:?}",
        mapped_process
            .as_ptr()
            .add((data.ehdr.e_entry - min_addr) as usize)
    );
    println!("Arg 2: {:?}", &vtable as *const vtable);

    unsafe {
        load_process(
            mapped_process
                .as_ptr()
                .add((data.ehdr.e_entry - min_addr) as usize),
            &vtable as *const vtable,
        );
    }
}
