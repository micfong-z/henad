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

// The 64-bit product `a * b` as `(low, high)` words, built from 16-bit halves.
fn mul_wide(a: u32, b: u32) -> vec2<u32> {
    let low_low = (a & 0xFFFFu) * (b & 0xFFFFu);
    let high_low = (a >> 16u) * (b & 0xFFFFu);
    let low_high = (a & 0xFFFFu) * (b >> 16u);
    let high_high = (a >> 16u) * (b >> 16u);
    // At most 2^32 - 1, so this sum never wraps.
    let middle = (low_low >> 16u) + (high_low & 0xFFFFu) + low_high;
    return vec2<u32>((middle << 16u) | (low_low & 0xFFFFu), high_high + (high_low >> 16u) + (middle >> 16u));
}

struct IndexDraw {
    index: u32,
    accepted: bool,
}

// The index in `[0, n)` that `bits` maps to, rejected when the word would favour some indices over others.
fn index_from_bits(bits: u32, n: u32) -> IndexDraw {
    let wide = mul_wide(bits, n);
    let rejected = wide.x < n && wide.x < (0u - n) % n;
    return IndexDraw(wide.y, !rejected);
}

// A uniform integer in `[0, n)`, redrawing any rejected word. Returns 0 when `n` is 0.
fn next_index(r: ptr<function, u32>, n: u32) -> u32 {
    var draw = index_from_bits(next_bits(r), n);
    while !draw.accepted {
        draw = index_from_bits(next_bits(r), n);
    }
    return draw.index;
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
