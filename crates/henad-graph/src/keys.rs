//! The stream of random keys that new draws take, for the graph builder and the sheet builder.
//!
//! A *key* is the `u32` a draw mixes into every number it draws, so two draws with different keys draw different
//! numbers. A new draw takes a fresh key from a [`KeyStream`], distinct from every key the file already holds. The
//! stream is an `xorshift64` stream, seeded from entropy in the editor and from a fixed seed in tests.
//!
//! ```
//! use henad_graph::keys::KeyStream;
//!
//! let mut first = KeyStream::seeded(7);
//! let mut second = KeyStream::seeded(7);
//! assert_eq!(first.next_key(), second.next_key());
//! // A key the file already holds is skipped.
//! let taken = second.clone().next_key();
//! assert_ne!(second.next_key_not_in(|key| key == taken), taken);
//! ```

use std::hash::{BuildHasher as _, Hasher as _};
use std::sync::atomic::{AtomicU64, Ordering};

use henad_core::authoring::primitives::rng::{mix_seed, xorshift64};

/// Number of streams seeded from entropy so far, which keeps two streams apart on a target whose hasher keys are fixed.
static STREAMS: AtomicU64 = AtomicU64::new(0);

/// A stream of random draw keys: an `xorshift64` stream seeded from entropy or from a given seed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyStream {
    /// State of the `xorshift64` stream, never 0.
    state: u64,
}

impl KeyStream {
    /// Returns a stream seeded from entropy, different for every call.
    pub fn from_entropy() -> Self {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(STREAMS.fetch_add(1, Ordering::Relaxed));
        Self {
            state: mix_seed(hasher.finish()),
        }
    }

    /// Returns the stream seeded from `seed`. The same seed returns the same keys every run.
    pub fn seeded(seed: u64) -> Self {
        Self { state: mix_seed(seed) }
    }

    /// Returns the next key of the stream: the high 32 bits of the next `xorshift64` state.
    pub fn next_key(&mut self) -> u32 {
        self.state = xorshift64(self.state);
        // The high half of the state. xorshift64's low bits are the weaker half.
        (self.state >> 32) as u32
    }

    /// Returns the next key of the stream for which `is_taken` returns false, skipping the keys before it.
    pub fn next_key_not_in(&mut self, is_taken: impl Fn(u32) -> bool) -> u32 {
        loop {
            let key = self.next_key();
            if !is_taken(key) {
                return key;
            }
        }
    }
}
