pub trait GetBits {
    fn get_bits(&self, shift: Self, length: Self) -> Self;
    fn get_bit(&self, shift: Self) -> bool;
}

pub trait SetBits {
    fn set_bits(&mut self, shift: Self, length: Self, val: Self);
    fn set_bit(&mut self, shift: Self, enable: bool);
}

macro_rules! impl_set_bits {
    ($t:ty) => {
        impl SetBits for $t {
            fn set_bits(&mut self, shift: Self, length: Self, val: Self) {
                let mut mask = (1 << length) - 1;
                mask <<= shift;

                *self &= !mask;
                *self |= (val << shift) & mask;
            }

            fn set_bit(&mut self, shift: Self, enable: bool) {
                self.set_bits(shift, 1, enable as Self);
            }
        }
    };
}

impl_set_bits!(u8);
impl_set_bits!(u16);
impl_set_bits!(u32);
impl_set_bits!(u64);

macro_rules! impl_get_bits {
    ($t:ty) => {
        impl GetBits for $t {
            fn get_bits(&self, shift: Self, length: Self) -> Self {
                let mask = (1 << length) - 1;
                (*self >> shift) & mask
            }

            fn get_bit(&self, shift: Self) -> bool {
                self.get_bits(shift, 1) == 1
            }
        }
    };
}

impl_get_bits!(u8);
impl_get_bits!(u16);
impl_get_bits!(u32);
impl_get_bits!(u64);

#[cfg(test)]
mod test {
    use super::*;
    use crate::testing::*;

    create_test!(test_clear_bit, {
        let mut val = 0xffu8;
        val.set_bit(3, false);
        test_eq!(val, 0xf7);
        Ok(())
    });

    create_test!(test_set_bit, {
        let mut val = 0x00u8;
        val.set_bit(3, true);
        test_eq!(val, 0x08);
        Ok(())
    });

    create_test!(test_set_bits, {
        let mut val = 0x00u8;
        val.set_bits(4, 2, 3);
        test_eq!(val, 0x30);
        Ok(())
    });

    create_test!(test_clear_bits, {
        let mut val = 0xffu8;
        val.set_bits(4, 2, 0);
        test_eq!(val, 0xcf);
        Ok(())
    });

    create_test!(test_get_bits, {
        let val = 0x12345678u32;
        test_eq!(val.get_bit(3), true);
        test_eq!(val.get_bit(2), false);
        test_eq!(val.get_bit(1), false);
        test_eq!(val.get_bit(0), false);

        test_eq!(val.get_bit(31), false);
        test_eq!(val.get_bit(30), false);
        test_eq!(val.get_bit(29), false);
        test_eq!(val.get_bit(28), true);

        test_eq!(val.get_bits(28, 4), 0x1);
        test_eq!(val.get_bits(24, 4), 0x2);
        Ok(())
    });
}
