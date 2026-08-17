#define_import_path shared::rng

// Random draws, the twin of `henad_core::authoring::primitives::rng`.
//
// Only the draws over a raw word are twins. The generator is not: WGSL has no 64-bit integers, so
// this side advances with `pcg_hash` over `u32` where the Rust side runs `xorshift64` over `u64`.

// Mirrored in Rust by each model that seeds a buffer with it, bit for bit.
fn pcg_hash(input: u32) -> u32 {
    var state = input * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

fn next_bits(r: ptr<function, u32>) -> u32 {
    *r = pcg_hash(*r);
    return *r;
}

// A uniform float in `[0, max)`. The top 24 bits over a power of two, so nothing rounds and the
// range stays half-open.
fn random_float(bits: u32, max: f32) -> f32 {
    return f32(bits >> 8u) / 16777216.0 * max;
}

fn next_float(r: ptr<function, u32>, max: f32) -> f32 {
    return random_float(next_bits(r), max);
}

// A Bernoulli trial, true for `threshold` of the 2^32 possible words.
fn below(bits: u32, threshold: u32) -> bool {
    return bits < threshold;
}

// One of -1, 0 or +1.
fn choice3(bits: u32) -> i32 {
    return i32(bits % 3u) - 1;
}

// Accepts the `count`-th of a run of equally good candidates, with probability `1 / count`.
fn reservoir_accept(bits: u32, count: u32) -> bool {
    return random_float(bits, 1.0) < 1.0 / f32(count);
}
