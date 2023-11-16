use alloc::vec::Vec;
use core::{arch::asm, ops::RangeInclusive};

fn ranges_overlap(range: &RangeInclusive<u16>, new_range: &RangeInclusive<u16>) -> bool {
    (range.start() >= new_range.start() && range.start() <= new_range.end())
        || (range.end() >= new_range.start() && range.end() <= new_range.end())
}

fn range_already_allocated(
    allocated_ranges: &[RangeInclusive<u16>],
    new_range: &RangeInclusive<u16>,
) -> bool {
    for range in allocated_ranges {
        // Check that range is not used
        if ranges_overlap(range, new_range) {
            return true;
        }
    }

    false
}

pub struct IoAllocator {
    allocated_ranges: Vec<RangeInclusive<u16>>,
}

impl IoAllocator {
    pub fn new() -> IoAllocator {
        IoAllocator {
            allocated_ranges: Default::default(),
        }
    }

    pub fn request_io_range(&mut self, addr: u16, length: u16) -> Option<IoRange> {
        let new_range = addr..=(addr + length - 1);
        if range_already_allocated(&self.allocated_ranges, &new_range) {
            return None;
        }

        self.allocated_ranges.push(new_range);
        Some(IoRange { addr, length })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct IoOffset(u16);

impl IoOffset {
    pub const fn new(val: u16) -> IoOffset {
        IoOffset(val)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct OffsetOutOfRange {
    value: u16,
    num_bytes: u16,
    length: u16,
}

#[derive(Debug)]
pub struct IoRange {
    addr: u16,
    length: u16,
}

impl IoRange {
    fn verify_offset(&self, offset: IoOffset, num_bytes: u16) -> Result<(), OffsetOutOfRange> {
        if offset.0 + num_bytes > self.length {
            return Err(OffsetOutOfRange {
                value: offset.0,
                num_bytes,
                length: self.length,
            });
        }
        Ok(())
    }

    pub fn write_u8(&mut self, offset: IoOffset, val: u8) -> Result<(), OffsetOutOfRange> {
        self.verify_offset(offset, 1)?;
        unsafe {
            asm!(r#"
                out %al, %dx
                "#,
                in("dx") self.addr + offset.0,
                in("al") val,
                options(att_syntax));
        }
        Ok(())
    }

    pub fn read_u8(&mut self, offset: IoOffset) -> Result<u8, OffsetOutOfRange> {
        self.verify_offset(offset, 1)?;
        unsafe {
            let mut ret;
            asm!(r#"
                in %dx, %al
                "#,
                in("dx") self.addr + offset.0,
                out("al") ret,
                options(att_syntax));
            Ok(ret)
        }
    }

    pub fn write_16(&mut self, offset: IoOffset, val: u16) -> Result<(), OffsetOutOfRange> {
        self.verify_offset(offset, 2)?;
        unsafe {
            asm!(r#"
                out %ax, %dx
                "#,
                in("dx") self.addr + offset.0,
                in("ax") val,
                options(att_syntax));
        }
        Ok(())
    }

    pub fn read_16(&mut self, offset: IoOffset) -> Result<u16, OffsetOutOfRange> {
        self.verify_offset(offset, 2)?;
        unsafe {
            let mut ret;
            asm!(r#"
                in %dx, %ax
                "#,
                in("dx") self.addr + offset.0,
                out("ax") ret,
                options(att_syntax));
            Ok(ret)
        }
    }

    pub fn write_32(&mut self, offset: IoOffset, val: u32) -> Result<(), OffsetOutOfRange> {
        self.verify_offset(offset, 4)?;
        unsafe {
            asm!(r#"
                out %eax, %dx
                "#,
                in("dx") self.addr + offset.0,
                in("eax") val,
                options(att_syntax));
        }
        Ok(())
    }

    pub fn read_32(&mut self, offset: IoOffset) -> Result<u32, OffsetOutOfRange> {
        self.verify_offset(offset, 4)?;
        unsafe {
            let mut ret;
            asm!(r#"
                in %dx, %eax
                "#,
                in("dx") self.addr + offset.0,
                out("eax") ret,
                options(att_syntax));
            Ok(ret)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::testing::*;

    create_test!(test_overlapping_ranges, {
        test_true!(ranges_overlap(&(0..=5), &(5..=10)));
        test_true!(ranges_overlap(&(0..=5), &(4..=5)));
        test_false!(ranges_overlap(&(0..=5), &(6..=10)));
        test_false!(ranges_overlap(&(6..=10), &(0..=5)));
        Ok(())
    });

    create_test!(test_verify_offset, {
        let range = IoRange {
            addr: 50,
            length: 24,
        };

        test_ok!(range.verify_offset(IoOffset(23), 1));
        test_err!(range.verify_offset(IoOffset(24), 1));

        test_err!(range.verify_offset(IoOffset(23), 2));
        test_err!(range.verify_offset(IoOffset(21), 4));
        test_ok!(range.verify_offset(IoOffset(20), 4));
        Ok(())
    });
}
