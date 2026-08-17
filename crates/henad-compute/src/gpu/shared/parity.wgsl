#define_import_path shared::parity

// Runs each `shared::space` primitive over caller-supplied cases, so a test can compare the result
// against the Rust twin. One invocation per case.
//
// Every op writes into the same `Out`. The field carrying the answer is fixed per op, and the
// test knows it.

#import shared::space::{wrap_index, wrap_coord, cell_index, offset_cell, axis_delta, dist_sq}
#import shared::space::{neighbor_count, neighbor_offset, heading_octant}

const OP_WRAP_INDEX: u32 = 0u;
const OP_WRAP_COORD: u32 = 1u;
const OP_CELL_INDEX: u32 = 2u;
const OP_OFFSET_CELL: u32 = 3u;
const OP_AXIS_DELTA: u32 = 4u;
const OP_DIST_SQ: u32 = 5u;
const OP_NEIGHBOR_COUNT: u32 = 6u;
const OP_NEIGHBOR_OFFSET: u32 = 7u;
const OP_HEADING_OCTANT: u32 = 8u;

struct Case {
    op: u32,
    boundary: u32,
    table: u32,
    n: u32,
    i: vec4<i32>,
    u: vec4<u32>,
    f: vec4<f32>,
    g: vec4<f32>,
}

struct Out {
    i: vec4<i32>,
    f: vec4<f32>,
}

@group(0) @binding(0) var<storage, read> cases: array<Case>;
@group(0) @binding(1) var<storage, read_write> results: array<Out>;

@compute
@workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if (idx >= arrayLength(&cases)) {
        return;
    }
    let c = cases[idx];

    var out: Out;
    out.i = vec4<i32>(0, 0, 0, 0);
    out.f = vec4<f32>(0.0, 0.0, 0.0, 0.0);

    switch c.op {
        case 0u: {
            out.i.x = wrap_index(c.i.x, c.i.y);
        }
        case 1u: {
            out.f.x = wrap_coord(c.f.x, c.f.y);
        }
        case 2u: {
            out.i.x = i32(cell_index(c.u.x, c.u.y, c.u.z));
        }
        case 3u: {
            out.i = vec4<i32>(offset_cell(c.u.x, c.u.y, c.i.x, c.i.y, c.u.z, c.u.w, c.boundary), 0);
        }
        case 4u: {
            out.f.x = axis_delta(c.f.x, c.f.y, c.f.z, c.boundary);
        }
        case 5u: {
            out.f.x = dist_sq(c.f.xy, c.f.zw, c.g.xy, c.boundary);
        }
        case 6u: {
            out.i.x = i32(neighbor_count(c.table));
        }
        case 7u: {
            out.i = vec4<i32>(neighbor_offset(c.table, c.n), 0, 0);
        }
        case 8u: {
            out.i.x = i32(heading_octant(c.f.x, c.f.y));
        }
        default: {}
    }

    results[idx] = out;
}
