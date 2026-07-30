// Instanced agent sprites, one quad per agent expanded in clip space from a world-space centre.
//
// Positions arrive as two vertex buffers rather than one interleaved one, so the sim's SoA lanes
// upload with no repacking.

struct Uniforms {
    world: vec2<f32>,
    // Half the sprite size in clip units. Precomputed, since it depends on the target rect.
    half_size: vec2<f32>,
}

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VertexInput {
    @location(0) pos_x: f32,
    @location(1) pos_y: f32,
    @location(2) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    // -1..1 across the sprite, so the fragment stage can carve a disc out of the quad.
    @location(1) offset: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32, in: VertexInput) -> VertexOutput {
    // Triangle-strip corner order: (0,0) (1,0) (0,1) (1,1), remapped to -1..1.
    let corner = vec2<f32>(
        f32(vertex_index & 1u),
        f32((vertex_index >> 1u) & 1u),
    ) * 2.0 - 1.0;

    // World to clip. Y is flipped, model row 0 is the top and so is clip +1.
    let centre = vec2<f32>(
        in.pos_x / u.world.x * 2.0 - 1.0,
        1.0 - in.pos_y / u.world.y * 2.0,
    );

    var out: VertexOutput;
    out.clip_position = vec4<f32>(centre + corner * u.half_size, 0.0, 1.0);
    out.color = in.color;
    out.offset = corner;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Round agents read better than squares once sprites overlap. Subpixel quads never reach
    // here, so this costs nothing at scale.
    if dot(in.offset, in.offset) > 1.0 {
        discard;
    }
    return in.color;
}
