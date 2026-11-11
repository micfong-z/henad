//! Fowler-Noll-Vo (FNV-1a) 64-bit hasher, whose hashes never change between Rust releases or platforms.

const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a hasher over 64 bits.
///
/// Integers are written little-endian, and text is written after its length, so two fields never run together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fnv1a64 {
    state: u64,
}

impl Fnv1a64 {
    /// Returns a hasher with nothing written to it.
    pub const fn new() -> Self {
        Self { state: OFFSET_BASIS }
    }

    /// Writes `bytes` as they are, with no length before them.
    pub const fn write(&mut self, bytes: &[u8]) {
        let mut index = 0;
        while index < bytes.len() {
            self.state ^= bytes[index] as u64;
            self.state = self.state.wrapping_mul(PRIME);
            index += 1;
        }
    }

    /// Writes `value` as eight little-endian bytes.
    pub const fn write_u64(&mut self, value: u64) {
        self.write(&value.to_le_bytes());
    }

    /// Writes the byte length of `text` as a `u64`, then its bytes.
    pub const fn write_str(&mut self, text: &str) {
        self.write_u64(text.len() as u64);
        self.write(text.as_bytes());
    }

    /// Writes the bits of `value`, so two values hash alike only when they are bit for bit the same.
    pub fn write_f64(&mut self, value: f64) {
        self.write_u64(value.to_bits());
    }

    /// Returns the hash of everything written so far.
    pub const fn finish(&self) -> u64 {
        self.state
    }
}

impl Default for Fnv1a64 {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns the FNV-1a hash of `bytes`.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hasher = Fnv1a64::new();
    hasher.write(bytes);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::fnv1a64;

    #[test]
    fn fnv_is_stable() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }
}
