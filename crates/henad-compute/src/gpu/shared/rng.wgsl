#define_import_path shared::rng

// Mirrored in Rust by each model that seeds a buffer with it, bit for bit.
fn pcg_hash(input: u32) -> u32 {
    var state = input * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}
