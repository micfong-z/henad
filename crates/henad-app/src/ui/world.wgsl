#define_import_path world

// Clip position beyond the far plane. A primitive with every vertex here is clipped away entirely.
const HIDDEN: vec4<f32> = vec4<f32>(0.0, 0.0, 2.0, 1.0);

// Returns whether a position is finite. A retired node sits at NaN.
// This tests the bits, since a shader may assume every float is finite and optimise a float comparison away.
fn is_placed(p: vec2<f32>) -> bool {
    let bits = bitcast<vec2<u32>>(p) & vec2<u32>(0x7f800000u);
    return all(bits != vec2<u32>(0x7f800000u));
}

// Converts a world position to clip space. Y is flipped, since model row 0 is at the top, as is clip +1.
fn to_clip(p: vec2<f32>, world: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(p.x / world.x * 2.0 - 1.0, 1.0 - p.y / world.y * 2.0);
}
