#![allow(unused)]

use ahash::AHasher;
use core::hash::{BuildHasher, Hasher};

pub struct Rng {
    state: u64,
    hasher: ahash::AHasher,
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        let hash_builder = core::hash::BuildHasherDefault::<AHasher>::default();
        let hasher = hash_builder.build_hasher();
        Rng {
            state: seed,
            hasher,
        }
    }

    pub fn u64(&mut self) -> u64 {
        self.hasher.write_u64(self.state);
        let ret = self.hasher.finish();
        self.state += 1;
        ret
    }

    pub fn normalized(&mut self) -> f32 {
        let val = self.u64();
        val as f32 / u64::MAX as f32
    }
}
