//! The deterministic random numbers behind the scene's fixed shapes (stars, ridges, trees, rain, snow).
//!
//! The picture was first drawn by a Python program that seeded `random.Random(seed)`. To keep every shape
//! identical, this reproduces the numbers Python's `random` module gives for a seed: the Mersenne Twister
//! itself comes from the `rand_mt` crate (seeded the way CPython does, `init_by_array`), and the few
//! conversions the scene uses (a float in [0, 1), a bounded integer, `uniform`, `choice`) follow CPython's
//! algorithms. The `rand` crate cannot be used directly: its seeding and its float and range sampling give
//! different numbers for the same seed, which would redraw every cloud and tree.

use rand_mt::Mt;

pub struct SceneRng {
    mt: Mt,
}

impl SceneRng {
    /// The generator Python's `random.Random(seed)` starts as, for a non-negative integer seed.
    pub fn new(seed: u64) -> Self {
        let mut key = vec![(seed & 0xFFFF_FFFF) as u32];
        if seed > 0xFFFF_FFFF {
            key.push((seed >> 32) as u32);
        }
        SceneRng { mt: Mt::new_with_key(key) }
    }

    fn next_u32(&mut self) -> u32 {
        self.mt.next_u32()
    }

    /// `random.random()`: a float in [0, 1) with 53 bits.
    pub fn random(&mut self) -> f64 {
        let a = (self.next_u32() >> 5) as f64;
        let b = (self.next_u32() >> 6) as f64;
        (a * 67_108_864.0 + b) / 9_007_199_254_740_992.0
    }

    fn getrandbits(&mut self, k: u32) -> u32 {
        if k == 0 {
            0
        } else {
            self.next_u32() >> (32 - k)
        }
    }

    fn randbelow(&mut self, n: u32) -> u32 {
        let k = 32 - n.leading_zeros();
        loop {
            let r = self.getrandbits(k);
            if r < n {
                return r;
            }
        }
    }

    /// `random.randrange(start, stop)`.
    pub fn randrange(&mut self, start: i32, stop: i32) -> i32 {
        start + self.randbelow((stop - start) as u32) as i32
    }

    /// `random.randint(a, b)` (inclusive).
    pub fn randint(&mut self, a: i32, b: i32) -> i32 {
        self.randrange(a, b + 1)
    }

    /// `random.uniform(a, b)`.
    pub fn uniform(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.random()
    }

    /// `random.choice(items)`.
    pub fn choice<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.randbelow(items.len() as u32) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_python_sequences() {
        let mut r = SceneRng::new(1717);
        assert_eq!(r.random(), 0.7793757332294945);
        assert_eq!(r.random(), 0.2287662770050798);
        let ints: Vec<i32> = (0..5).map(|_| r.randrange(0, 64)).collect();
        assert_eq!(ints, [27, 14, 5, 28, 18]);
        let ints: Vec<i32> = (0..5).map(|_| r.randint(9, 12)).collect();
        assert_eq!(ints, [11, 12, 12, 11, 10]);
        assert_eq!(r.uniform(0.5, 1.0), 0.9495812518611326);
        assert_eq!(r.choice(&[2, 3, 3]), 3);
        assert_eq!(r.choice(&[1, 1, -1]), 1);
        let mut r = SceneRng::new(64281);
        assert_eq!(r.randint(0, 1), 0);
        assert_eq!(r.random(), 0.5697818689332441);
    }
}
