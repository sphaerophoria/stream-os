use alloc::vec::Vec;
use core::mem::MaybeUninit;

#[no_mangle]
pub unsafe extern "C" fn memset(s: *mut u8, c: i32, n: usize) -> *mut u8 {
    let c = c as u8;
    for i in 0..n {
        *s.add(i) = c;
    }
    s
}

#[no_mangle]
pub unsafe extern "C" fn memcpy(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    for i in 0..n {
        *d.add(i) = *s.add(i);
    }
    d
}

#[no_mangle]
pub unsafe extern "C" fn memcmp(s1: *const u8, s2: *const u8, n: usize) -> i32 {
    for i in 0..n {
        let a = *s1.add(i) as i32;
        let b = *s2.add(i) as i32;
        if a != b {
            return a - b;
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn memmove(dest: *mut u8, src: *const u8, n: usize) {
    let mut copy: Vec<MaybeUninit<u8>> = alloc::vec::Vec::with_capacity(n);
    unsafe {
        let copy_ptr = copy.as_mut_ptr();
        for i in 0..n {
            (*copy_ptr.add(i)).write(*src.add(i));
        }

        copy.set_len(n);
    }
    memcpy(dest, copy.as_ptr() as *mut u8, n);
}
