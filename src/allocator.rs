use crate::{
    multiboot2::Multiboot2,
    util::{interrupt_guard::InterruptGuarded, spinlock::SpinLock},
};

use core::{
    alloc::{GlobalAlloc, Layout},
    sync::atomic::{AtomicPtr, Ordering},
};

#[global_allocator]
pub static ALLOC: Allocator = Allocator::new();

#[repr(C, packed)]
#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub struct FreeSegment {
    size: usize,
    next_segment: *mut FreeSegment,
}

impl FreeSegment {
    fn get_start(&self) -> *mut u8 {
        unsafe { (self as *const FreeSegment).add(1) as *mut u8 }
    }

    fn get_end(&self) -> *mut u8 {
        unsafe { self.get_start().add(self.size) }
    }

    unsafe fn set_end(&mut self, end: *mut u8) {
        self.size = end
            .offset_from(self.get_start())
            .try_into()
            .expect("Expected end > start");
    }
}

#[repr(C, packed)]
#[derive(Debug)]
struct UsedSegment {
    size: usize,
    padding: [u8; 4],
}

impl UsedSegment {
    fn get_start(&self) -> *mut u8 {
        unsafe { (self as *const UsedSegment).add(1) as *mut u8 }
    }

    fn set_end(&mut self, end: *mut u8) {
        unsafe {
            self.size = end
                .offset_from(self.get_start())
                .try_into()
                .expect("Expected end > start");
        }
    }
}

pub struct Allocator {
    pub first_free: AtomicPtr<FreeSegment>,
    lock: SpinLock<()>,
}

impl Allocator {
    pub const fn new() -> Allocator {
        Allocator {
            first_free: AtomicPtr::new(core::ptr::null_mut()),
            lock: SpinLock::new(()),
        }
    }
}

pub unsafe fn init(info: &Multiboot2) {
    assert_eq!(
        core::mem::size_of::<UsedSegment>(),
        core::mem::size_of::<FreeSegment>()
    );

    let big_block = info
        .get_mmap_addrs()
        .iter()
        .find(|entry| entry.addr == (&crate::KERNEL_START) as *const u32 as u64);

    let big_block = big_block.expect("Failed to find big block of ram");
    let kernel_end_addr = (&crate::KERNEL_END as *const u32) as u64;
    let kernel_start_addr = (&crate::KERNEL_START as *const u32) as u64;
    let reserved_memory_length = (kernel_end_addr - kernel_start_addr) as usize;

    let segment_size =
        big_block.len as usize - reserved_memory_length - core::mem::size_of::<FreeSegment>();

    let segment = &crate::KERNEL_END as *const u32;
    let segment = segment as *mut FreeSegment;
    *segment = FreeSegment {
        size: segment_size,
        next_segment: core::ptr::null_mut(),
    };

    ALLOC.first_free.store(segment, Ordering::Relaxed);
}

unsafe fn find_header_for_allocation(segment: &FreeSegment, layout: &Layout) -> Option<*mut u8> {
    let segment_start: *mut u8 = segment.get_start();
    let segment_end: *mut u8 = segment.get_end();

    let mut ptr: *mut u8 = segment_end.sub(layout.size());
    ptr = ptr.sub((ptr as usize) % layout.align());
    ptr = ptr.sub(core::mem::size_of::<UsedSegment>());

    if ptr < segment_start {
        debug!(
            "Segment size too small, segment: {:?}, layout: {:?}",
            segment, layout
        );
        return None;
    }

    Some(ptr)
}

unsafe fn get_header_ptr_from_allocated(ptr: *mut u8) -> *mut UsedSegment {
    ptr.sub(core::mem::size_of::<UsedSegment>()) as *mut UsedSegment
}

unsafe fn merge_if_adjacent(a: *mut FreeSegment, b: *mut FreeSegment) {
    if (*a).get_end() == b as *mut u8 {
        (*a).set_end((*b).get_end());
        (*a).next_segment = (*b).next_segment;
    }
}

unsafe fn insert_segment_after(item: *mut FreeSegment, new_segment: *mut FreeSegment) {
    let next = (*item).next_segment;
    (*item).next_segment = new_segment;
    (*new_segment).next_segment = next;

    merge_if_adjacent(new_segment, (*new_segment).next_segment);
    merge_if_adjacent(item, new_segment);
}

unsafe fn insert_segment_into_list(list_head: *mut FreeSegment, new_segment: *mut FreeSegment) {
    let mut it = list_head;
    while !it.is_null() {
        assert!(it < new_segment);

        let should_insert = (*it).next_segment.is_null() || (*it).next_segment > new_segment;
        if should_insert {
            insert_segment_after(it, new_segment);
            return;
        }

        it = (*it).next_segment;
    }
    panic!("Failed to insert segment into list");
}

unsafe fn convert_used_to_free_segment(list_head: *mut FreeSegment, header_ptr: *mut UsedSegment) {
    let size = (*header_ptr).size;
    let free_segment_ptr = header_ptr as *mut FreeSegment;
    (*free_segment_ptr).size = size;
    (*free_segment_ptr).next_segment = core::ptr::null_mut();
    insert_segment_into_list(list_head, free_segment_ptr);
}

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _guard1 = InterruptGuarded::new(());
        let _guard1 = _guard1.lock();
        let _guard2 = self.lock.lock();

        let mut free_block_it = self.first_free.load(Ordering::Relaxed);

        while !free_block_it.is_null() {
            let header_ptr = find_header_for_allocation(&*free_block_it, &layout);
            let header_ptr = match header_ptr {
                Some(v) => v,
                None => {
                    free_block_it = (*free_block_it).next_segment;
                    continue;
                }
            };

            // Grab this before updating our size so we don't lose the end of the block
            let used_end = (*free_block_it).get_end();

            (*free_block_it).set_end(header_ptr);

            let header_ptr = header_ptr as *mut UsedSegment;
            (*header_ptr).set_end(used_end);
            return (*header_ptr).get_start();
        }
        panic!("Failed to allocate");
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        let _guard1 = InterruptGuarded::new(());
        let _guard1 = _guard1.lock();
        let _guard2 = self.lock.lock();

        let header_ptr = get_header_ptr_from_allocated(ptr);
        convert_used_to_free_segment(self.first_free.load(Ordering::Relaxed), header_ptr);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::testing::*;
    use alloc::{boxed::Box, vec::Vec};

    // We cannot return a vector here as that alters the alloc state, so we just say that we only
    // support capturing up to N segments, increase as necessary
    fn capture_alloc_state() -> [FreeSegment; 100] {
        unsafe {
            let mut ret = [FreeSegment {
                size: 0,
                next_segment: core::ptr::null_mut(),
            }; 100];
            let mut ret_it = 0;
            let mut list_it = ALLOC.first_free.load(Ordering::Relaxed);

            while !list_it.is_null() {
                ret[ret_it] = *list_it;

                ret_it += 1;
                list_it = (*list_it).next_segment;
            }

            ret
        }
    }

    create_test!(test_simple_alloc, {
        unsafe {
            let initial_state = capture_alloc_state();
            let p = Box::new(4);

            test_ne!(initial_state, capture_alloc_state());

            // At this point, from the initial state we should have one of the blocks decrease in
            // size by 4 bytes, and that should be the _only_ change

            let alloc_state = capture_alloc_state();
            let num_diff = initial_state
                .iter()
                .zip(alloc_state.iter())
                .filter(|(a, b)| a != b)
                .count();
            test_eq!(num_diff, 1);

            let diff_item = initial_state
                .iter()
                .zip(alloc_state.iter())
                .find(|(a, b)| a != b)
                .expect("could not find a != b");

            let before = core::ptr::addr_of!(diff_item.0.size);
            let after = core::ptr::addr_of!(diff_item.1.size);
            // We can only test that at least the given memory has been allocated because we do not
            // know the state of alignment before the allocation
            test_ge!(
                before.read_unaligned(),
                after.read_unaligned() + 4 + core::mem::size_of::<UsedSegment>()
            );

            drop(p);

            test_eq!(initial_state, capture_alloc_state());
        }

        Ok(())
    });

    create_test!(test_nested_vector_alloc, {
        let initial_state = capture_alloc_state();
        {
            let mut v = Vec::new();
            const NUM_ALLOCATIONS: usize = 10;
            // Allocating a bunch of shit
            for i in 1..NUM_ALLOCATIONS {
                let mut v2 = Vec::new();
                for j in 0..i {
                    v2.push(j);
                }
                v.push(v2);
            }

            // Creating holes in allocations
            for i in (0..NUM_ALLOCATIONS - 1).filter(|x| (x % 2) == 0).rev() {
                let len = v.len() - 1;
                v.swap(len, i);
                v.pop();
            }

            // alloc and dealloc again
            {
                let mut v = Vec::new();
                for i in 1..NUM_ALLOCATIONS {
                    let mut v2 = Vec::new();
                    for j in 0..i {
                        v2.push(j);
                    }
                    v.push(v2);
                }
            }

            // Checking for memory corruption
            for elem in v {
                for (i, item) in elem.into_iter().enumerate() {
                    test_eq!(i, item);
                }
            }
        }

        test_eq!(initial_state, capture_alloc_state());
        Ok(())
    });
}
