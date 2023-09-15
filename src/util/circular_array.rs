use core::mem::MaybeUninit;

pub struct PushError;

fn wrapping_increment(i: &mut usize, container_size: usize) {
    *i = (*i + 1) % container_size
}

pub struct CircularArray<T, const N: usize> {
    array: [MaybeUninit<T>; N],
    head: usize,
    tail: usize,
}

impl<T, const N: usize> CircularArray<T, N> {
    pub const fn new() -> Self {
        Self {
            array: MaybeUninit::uninit_array(),
            head: 0,
            tail: 0,
        }
    }

    pub fn pop_front(&mut self) -> Option<T> {
        let index = self.head;

        if self.head == self.tail {
            return None;
        }

        wrapping_increment(&mut self.head, N);

        if self.tail == N + 1 {
            self.tail = index;
        }

        let mut ret = MaybeUninit::uninit();
        core::mem::swap(&mut ret, &mut self.array[index]);
        unsafe { Some(ret.assume_init()) }
    }

    pub fn push_back(&mut self, item: T) -> Result<(), PushError> {
        let insertion_index = self.tail;
        match self.increment_tail() {
            Some(tail) => self.tail = tail,
            None => return Err(PushError),
        }

        self.array[insertion_index] = MaybeUninit::new(item);

        Ok(())
    }

    fn increment_tail(&mut self) -> Option<usize> {
        if self.tail == N + 1 {
            return None;
        }

        wrapping_increment(&mut self.tail, N);
        if self.tail == self.head {
            self.tail = N + 1;
        }

        Some(self.tail)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::testing::*;

    create_test!(buffer_full, {
        let mut buffer: CircularArray<i32, 3> = CircularArray::new();
        test_true!(buffer.push_back(1).is_ok());
        test_true!(buffer.push_back(2).is_ok());
        test_true!(buffer.push_back(3).is_ok());
        test_true!(buffer.push_back(4).is_err());
        Ok(())
    });

    create_test!(pop_empty, {
        let mut buffer: CircularArray<i32, 3> = CircularArray::new();
        test_true!(buffer.pop_front().is_none());
        Ok(())
    });

    create_test!(push_pop_pop, {
        let mut buffer: CircularArray<i32, 3> = CircularArray::new();
        test_true!(buffer.push_back(1).is_ok());
        test_eq!(buffer.pop_front(), Some(1));
        test_true!(buffer.pop_front().is_none());
        Ok(())
    });

    create_test!(full_then_not_full, {
        let mut buffer: CircularArray<i32, 3> = CircularArray::new();
        test_true!(buffer.push_back(1).is_ok());
        test_true!(buffer.push_back(2).is_ok());
        test_true!(buffer.push_back(3).is_ok());
        test_true!(buffer.push_back(4).is_err());
        test_eq!(buffer.pop_front(), Some(1));
        test_true!(buffer.push_back(4).is_ok());
        test_eq!(buffer.pop_front(), Some(2));
        test_eq!(buffer.pop_front(), Some(3));
        test_eq!(buffer.pop_front(), Some(4));
        Ok(())
    });
}
