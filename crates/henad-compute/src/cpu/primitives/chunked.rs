//! Chunk-parallel drivers. The only place the rayon/wasm split lives.

use std::ops::Range;

use henad_core::helpers::xorshift64;

#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;

#[cfg(not(target_arch = "wasm32"))]
#[doc(hidden)]
pub use rayon as __rayon;

/// Cells or agents per chunk in a stats reduction.
pub const STATS_CHUNK: usize = 8192;

/// Runs a body over each chunk of a mutable slice, in parallel on native.
///
/// A macro rather than a function taking a closure. The extra closure layer a generic driver needs
/// stops a hot kernel inlining through it, and `#[inline]` does not rescue it.
///
/// ```ignore
/// for_each_chunk_mut!(next, row_width, |y, _base, next_row| { .. });
/// ```
#[macro_export]
macro_rules! for_each_chunk_mut {
    ($items:expr, $chunk:expr, |$c:ident, $base:ident, $slice:ident| $body:block) => {{
        let chunk = ($chunk).max(1);

        #[cfg(not(target_arch = "wasm32"))]
        {
            use $crate::cpu::primitives::chunked::__rayon::prelude::*;
            $items.par_chunks_mut(chunk).enumerate().for_each(|($c, $slice)| {
                let $base = $c * chunk;
                $body
            });
        }

        #[cfg(target_arch = "wasm32")]
        {
            for ($c, $slice) in $items.chunks_mut(chunk).enumerate() {
                let $base = $c * chunk;
                $body
            }
        }
    }};

    // Three lanes stepped together, for a pass that writes more than one output lane.
    ($a:expr, $b:expr, $d:expr, $chunk:expr, |$c:ident, $base:ident, $sa:ident, $sb:ident, $sd:ident| $body:block) => {{
        let chunk = ($chunk).max(1);

        #[cfg(not(target_arch = "wasm32"))]
        {
            use $crate::cpu::primitives::chunked::__rayon::prelude::*;
            $a.par_chunks_mut(chunk)
                .zip($b.par_chunks_mut(chunk))
                .zip($d.par_chunks_mut(chunk))
                .enumerate()
                .for_each(|($c, (($sa, $sb), $sd))| {
                    let $base = $c * chunk;
                    $body
                });
        }

        #[cfg(target_arch = "wasm32")]
        {
            for ($c, (($sa, $sb), $sd)) in $a
                .chunks_mut(chunk)
                .zip($b.chunks_mut(chunk))
                .zip($d.chunks_mut(chunk))
                .enumerate()
            {
                let $base = $c * chunk;
                $body
            }
        }
    }};
}

/// Half-open index range of chunk `c`.
#[inline]
fn chunk_range(c: usize, chunk: usize, len: usize) -> Range<usize> {
    let start = c * chunk;
    start..(start + chunk).min(len)
}

/// Maps each chunk of `0..len` then folds the results in chunk order.
///
/// Takes a length rather than a slice so a caller can read several lanes per chunk. Folding in
/// index order is what keeps a float reduction from depending on how rayon schedules the work.
pub fn reduce_chunks<A, M, F>(len: usize, chunk: usize, map: M, fold: F, init: A) -> A
where
    A: Send,
    M: Fn(Range<usize>) -> A + Send + Sync,
    F: Fn(A, A) -> A,
{
    let chunk = chunk.max(1);
    let n = len.div_ceil(chunk);

    #[cfg(not(target_arch = "wasm32"))]
    let partials: Vec<A> = (0..n)
        .into_par_iter()
        .map(|c| map(chunk_range(c, chunk, len)))
        .collect();

    #[cfg(target_arch = "wasm32")]
    let partials: Vec<A> = (0..n).map(|c| map(chunk_range(c, chunk, len))).collect();

    partials.into_iter().fold(init, fold)
}

/// Seed for chunk `c` of tick `tick`.
///
/// Derived from the chunk index rather than from anything a worker mutates, so the stream a given
/// agent sees does not depend on how rayon schedules the work. `base` comes from [`advance_tick_seed`].
#[inline]
pub fn chunk_seed(base: u64, tick: u64, c: usize) -> u64 {
    let mixed = base ^ tick.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (c as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    // xorshift64's state may never be zero.
    xorshift64(mixed | 1)
}

/// Advances the per tick base handed to [`chunk_seed`].
///
/// Called once per tick on the sequential path, never by a worker. Folding the tick in here rather
/// than relying on `chunk_seed` alone is measurably faster, for reasons not tracked down.
#[inline]
pub fn advance_tick_seed(seed: u64, tick: u64) -> u64 {
    xorshift64(seed ^ tick)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_index_is_visited_exactly_once() {
        for len in [0usize, 1, 7, 4096, 4097, 8193] {
            let mut items = vec![0u32; len];
            for_each_chunk_mut!(items, 100, |_c, base, slice| {
                for (k, v) in slice.iter_mut().enumerate() {
                    *v = (base + k) as u32 + 1;
                }
            });
            let expected: Vec<u32> = (1..=len as u32).collect();
            assert_eq!(items, expected, "len {len}");
        }
    }

    /// A ragged final chunk is the easy one to drop or double count.
    #[test]
    fn reduction_covers_a_ragged_tail() {
        for len in [0usize, 1, 99, 100, 101] {
            let total = reduce_chunks(len, 10, |r| r.sum::<usize>(), |a, b| a + b, 0);
            assert_eq!(total, (0..len).sum::<usize>(), "len {len}");
        }
    }

    /// Folding in completion order instead of chunk order would show up here.
    #[test]
    fn reduction_folds_in_chunk_order() {
        let order = reduce_chunks(
            50,
            10,
            |r| vec![r.start],
            |mut a, b| {
                a.extend(b);
                a
            },
            Vec::new(),
        );
        assert_eq!(order, vec![0, 10, 20, 30, 40]);
    }

    #[test]
    fn chunk_seeds_differ_by_chunk_and_tick() {
        let a = chunk_seed(1, 0, 0);
        assert_ne!(a, chunk_seed(1, 0, 1), "chunk index must change the stream");
        assert_ne!(a, chunk_seed(1, 1, 0), "tick must change the stream");
        assert_eq!(a, chunk_seed(1, 0, 0), "seeding must be reproducible");
    }

    /// A zero state would make xorshift64 emit zero forever.
    #[test]
    fn chunk_seed_is_never_zero() {
        for tick in 0..64u64 {
            for c in 0..64 {
                assert_ne!(chunk_seed(0, tick, c), 0, "tick {tick} chunk {c}");
            }
        }
    }
}
