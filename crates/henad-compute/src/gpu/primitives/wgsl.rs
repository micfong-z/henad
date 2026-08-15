//! WGSL every agent kernel repeats, held once.

use crate::gpu::primitives::dispatch::WORKGROUP;

/// The reduction's leaf pass, around a model's own `value`.
///
/// `header` declares the bindings, including `partials` and a `params` uniform carrying at least
/// `lanes` and `groups_x`. `value` is a statement block assigning `value`, with `lane` and the
/// invocation index `i` in scope. The one shader still assembled at runtime, so unlike a declared
/// pass it cannot import `shared::prelude` and declares its own workgroup width.
pub fn reduce_leaf(header: &str, value: &str) -> String {
    format!(
        "{header}

const WORKGROUP: u32 = {WORKGROUP}u;

var<workgroup> scratch: array<f32, {WORKGROUP}>;

@compute
@workgroup_size({WORKGROUP})
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {{
    let block = wid.y * params.groups_x + wid.x;
    let i = block * WORKGROUP + lid.x;

    for (var lane: u32 = 0u; lane < params.lanes; lane = lane + 1u) {{
        var value: f32 = 0.0;
{value}
        scratch[lid.x] = value;
        workgroupBarrier();

        for (var stride: u32 = WORKGROUP / 2u; stride > 0u; stride = stride >> 1u) {{
            var acc = scratch[lid.x];
            if (lid.x < stride) {{
                acc = acc + scratch[lid.x + stride];
            }}
            workgroupBarrier();
            scratch[lid.x] = acc;
            workgroupBarrier();
        }}

        if (lid.x == 0u) {{
            partials[block * params.lanes + lane] = scratch[0];
        }}
        workgroupBarrier();
    }}
}}
"
    )
}
