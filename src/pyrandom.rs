//! A Mersenne Twister that draws the same numbers as Python's `random.Random(seed)`.
//!
//! The scene's shapes (stars, ridges, trees, raindrops) are seeded, so the picture only stays
//! identical to the original Python display if the same seed gives the same sequence. Only the
//! calls the scene uses are implemented, with CPython's algorithms.

const N: usize = 624;
const M: usize = 397;

pub struct PyRandom {
    mt: [u32; N],
    index: usize,
}

impl PyRandom {
    /// `random.Random(seed)` for a non-negative integer seed.
    pub fn new(seed: u64) -> Self {
        let mut rng = PyRandom { mt: [0; N], index: N };
        rng.init_genrand(19_650_218);
        let mut key = vec![(seed & 0xFFFF_FFFF) as u32];
        if seed > 0xFFFF_FFFF {
            key.push((seed >> 32) as u32);
        }
        rng.init_by_array(&key);
        rng
    }

    fn init_genrand(&mut self, s: u32) {
        self.mt[0] = s;
        for i in 1..N {
            let prev = self.mt[i - 1];
            self.mt[i] = 1_812_433_253u32.wrapping_mul(prev ^ (prev >> 30)).wrapping_add(i as u32);
        }
        self.index = N;
    }

    fn init_by_array(&mut self, key: &[u32]) {
        let mut i = 1usize;
        let mut j = 0usize;
        let mut k = N.max(key.len());
        while k > 0 {
            let prev = self.mt[i - 1];
            self.mt[i] = (self.mt[i] ^ (prev ^ (prev >> 30)).wrapping_mul(1_664_525))
                .wrapping_add(key[j])
                .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= N {
                self.mt[0] = self.mt[N - 1];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
            k -= 1;
        }
        k = N - 1;
        while k > 0 {
            let prev = self.mt[i - 1];
            self.mt[i] = (self.mt[i] ^ (prev ^ (prev >> 30)).wrapping_mul(1_566_083_941))
                .wrapping_sub(i as u32);
            i += 1;
            if i >= N {
                self.mt[0] = self.mt[N - 1];
                i = 1;
            }
            k -= 1;
        }
        self.mt[0] = 0x8000_0000;
        self.index = N;
    }

    fn next_u32(&mut self) -> u32 {
        if self.index >= N {
            for kk in 0..N {
                let y = (self.mt[kk] & 0x8000_0000) | (self.mt[(kk + 1) % N] & 0x7FFF_FFFF);
                let mut v = self.mt[(kk + M) % N] ^ (y >> 1);
                if y & 1 != 0 {
                    v ^= 0x9908_B0DF;
                }
                self.mt[kk] = v;
            }
            self.index = 0;
        }
        let mut y = self.mt[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9D2C_5680;
        y ^= (y << 15) & 0xEFC6_0000;
        y ^= y >> 18;
        y
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
    fn matches_cpython_sequences() {
        let mut r = PyRandom::new(1717);
        assert_eq!(r.random(), 0.7793757332294945);
        assert_eq!(r.random(), 0.2287662770050798);
        let ints: Vec<i32> = (0..5).map(|_| r.randrange(0, 64)).collect();
        assert_eq!(ints, [27, 14, 5, 28, 18]);
        let ints: Vec<i32> = (0..5).map(|_| r.randint(9, 12)).collect();
        assert_eq!(ints, [11, 12, 12, 11, 10]);
        assert_eq!(r.uniform(0.5, 1.0), 0.9495812518611326);
        assert_eq!(r.choice(&[2, 3, 3]), 3);
        assert_eq!(r.choice(&[1, 1, -1]), 1);
        let mut r = PyRandom::new(64281);
        assert_eq!(r.randint(0, 1), 0);
        assert_eq!(r.random(), 0.5697818689332441);
    }
}
