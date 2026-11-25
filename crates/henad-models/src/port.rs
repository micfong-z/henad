//! Helpers that the GPU ports share: the seeding of a `pcg_hash` state buffer, and the packing of positions and
//! palettes into the layouts the shaders read.

use henad_core::authoring::primitives::rng::{mix_seed, pcg_hash};

/// Domain separator of a port's `rng` buffer seed. Otherwise the buffer and the stream that seeds the cells or agents
/// would start correlated.
const RNG_BUFFER_SALT: u64 = 0x5EED_5EED_5EED_5EED;

/// Returns the seed of a port's `rng` buffer, from the run's `seed` and `init_seed`, the default seed of the CPU
/// engine's init stream.
pub(crate) fn rng_buffer_seed(seed: Option<u64>, init_seed: u64) -> u64 {
    let domain = init_seed ^ RNG_BUFFER_SALT;
    seed.map_or(domain, |seed| mix_seed(seed ^ domain))
}

/// Returns `count` initial `pcg_hash` states, each hashed from `seed` and its index.
pub(crate) fn pcg_states(count: usize, seed: u64) -> Vec<u32> {
    let seed32 = (seed ^ (seed >> 32)) as u32;
    (0..count).map(|index| pcg_hash(seed32 ^ index as u32)).collect()
}

/// Interleaves `xs` and `ys` into the `vec2` layout the shaders read.
pub(crate) fn interleave(xs: &[f32], ys: &[f32]) -> Vec<f32> {
    xs.iter().zip(ys).flat_map(|(&x, &y)| [x, y]).collect()
}

/// Packs `palette` into a uniform of `ROWS` rows of four colours, indexed as `palette[i >> 2][i & 3]`.
///
/// # Panics
///
/// Panics if `palette` holds more than `ROWS * 4` colours.
pub(crate) fn pack_palette<const ROWS: usize>(palette: &[[u8; 4]]) -> [[u32; 4]; ROWS] {
    let mut packed = [[0u32; 4]; ROWS];
    for (index, rgba) in palette.iter().enumerate() {
        packed[index / 4][index % 4] = u32::from_le_bytes(*rgba);
    }
    packed
}

/// Returns entry `index` of `palette` packed into a `u32` as the CPU agent upload packs it, or entry 0 past the end.
///
/// # Panics
///
/// Panics if `palette` is empty.
pub(crate) fn packed_color(palette: &[[u8; 4]], index: u8) -> u32 {
    let rgba = palette.get(index as usize).copied().unwrap_or(palette[0]);
    u32::from_le_bytes(rgba)
}
