//! WGSL every agent kernel repeats, held once.

/// Prepended to every pass shader, so a kernel folds without restating how.
///
/// A prefix rather than a splice, so a compile error's reported line is off by a constant.
pub const PRELUDE: &str = r"
const WORKGROUP: u32 = 256u;

// Folds a linear invocation domain onto the 2D workgroup grid `linear_dispatch` picks. 100M
// agents overflow one row of workgroups.
fn linear_index(lid: vec3<u32>, wid: vec3<u32>, groups_x: u32) -> u32 {
    return (wid.y * groups_x + wid.x) * WORKGROUP + lid.x;
}
";

/// The reduction's leaf pass, around a model's own `value`.
///
/// `header` declares the bindings, including `partials` and a `params` uniform carrying at least
/// `lanes` and `groups_x`. `value` is a statement block assigning `value`, with `lane` and the
/// invocation index `i` in scope. [`PRELUDE`] is prepended by the engine, as it is for any pass,
/// so `WORKGROUP` is already in scope here.
#[must_use]
pub fn reduce_leaf(header: &str, value: &str) -> String {
    format!(
        "{header}

var<workgroup> scratch: array<f32, 256>;

@compute
@workgroup_size(256)
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
