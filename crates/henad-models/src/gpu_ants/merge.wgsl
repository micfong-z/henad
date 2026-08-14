// Folds this tick's deposits into the field and decays it, one invocation per cell per layer.
//
// The CPU equivalent is `ScalarField::update`: scatter with `Combine::Max` into the next buffer,
// then decay it. Resetting `accum` here rather than clearing the buffer each tick costs nothing,
// since this pass already owns the cell.

struct Params {
    n: u32,
    groups_x: u32,
    evaporation: f32,
    low: f32,
}

@group(0) @binding(0) var<storage, read_write> field: array<f32>;
// `atomic<u32>` in the step shader. Only one invocation touches each entry here.
@group(0) @binding(1) var<storage, read_write> accum: array<u32>;
@group(0) @binding(2) var<uniform> params: Params;


@compute
@workgroup_size(256)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let i = linear_index(lid, wid, params.groups_x);
    if (i >= params.n) {
        return;
    }

    let deposited = bitcast<f32>(accum[i]);
    accum[i] = 0u;

    // Decay after the merge, so a fresh deposit is already one step old when read.
    var v = max(field[i], deposited) * params.evaporation;
    if (v < params.low) {
        // Without the floor a trail never disappears, it just asymptotes.
        v = 0.0;
    }
    field[i] = v;
}
