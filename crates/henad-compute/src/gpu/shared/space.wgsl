#define_import_path shared::space

// World geometry, the twin of `henad_core::authoring::primitives::space`.
//
// Integer results are bit-equal to the Rust side. Float results agree to a tolerance instead, since
// WGSL's float `%` is defined through a division while Rust's is an exact fmod.

const TORUS: u32 = 0u;
const BOUNDED: u32 = 1u;

// Neighbour tables, matching the Rust consts of the same names.
const MOORE_ROW_MAJOR: u32 = 0u;
const MOORE_COLUMN_MAJOR: u32 = 1u;
const VON_NEUMANN: u32 = 2u;

// Wraps `v` into `0..m`. Undefined for `m == 0`, where the Rust side panics.
fn wrap_index(v: i32, m: i32) -> i32 {
    return ((v % m) + m) % m;
}

// Wraps `v` into `0.0..world`.
fn wrap_coord(v: f32, world: f32) -> f32 {
    let r = v % world;
    return select(r, r + abs(world), r < 0.0);
}

fn cell_index(x: u32, y: u32, w: u32) -> u32 {
    return y * w + x;
}

// The cell `(dx, dy)` away from `(x, y)`. `.z` is non-zero when that cell exists, since WGSL has no
// `Option`. `dy` runs south.
fn offset_cell(x: u32, y: u32, dx: i32, dy: i32, w: u32, h: u32, boundary: u32) -> vec3<i32> {
    let nx = i32(x) + dx;
    let ny = i32(y) + dy;
    if (boundary == TORUS) {
        return vec3<i32>(wrap_index(nx, i32(w)), wrap_index(ny, i32(h)), 1);
    }
    if (nx < 0 || ny < 0 || nx >= i32(w) || ny >= i32(h)) {
        return vec3<i32>(0, 0, 0);
    }
    return vec3<i32>(nx, ny, 1);
}

// Shortest signed delta from `a` to `b`, in `[-world/2, world/2)` under `TORUS`.
fn axis_delta(a: f32, b: f32, world: f32, boundary: u32) -> f32 {
    let d = b - a;
    if (boundary == BOUNDED) {
        return d;
    }
    let half = world * 0.5;
    return wrap_coord(d + half, world) - half;
}

// Squared distance, taking points as `vec2` since that is how the agent buffers are packed. The
// Rust side takes four scalars instead, since its lanes are struct-of-arrays.
fn dist_sq(a: vec2<f32>, b: vec2<f32>, world: vec2<f32>, boundary: u32) -> f32 {
    let d = vec2<f32>(
        axis_delta(a.x, b.x, world.x, boundary),
        axis_delta(a.y, b.y, world.y, boundary),
    );
    return dot(d, d);
}

fn neighbor_count(table: u32) -> u32 {
    return select(8u, 4u, table == VON_NEUMANN);
}

// Neighbour `n` of a table, in the same order as the Rust const of that name.
//
// The Moore tables are the 3x3 block with the centre skipped, so the index steps over 4.
fn neighbor_offset(table: u32, n: u32) -> vec2<i32> {
    if (table == VON_NEUMANN) {
        if (n == 0u) { return vec2<i32>(0, -1); }
        if (n == 1u) { return vec2<i32>(-1, 0); }
        if (n == 2u) { return vec2<i32>(1, 0); }
        return vec2<i32>(0, 1);
    }
    let k = n + select(0u, 1u, n >= 4u);
    let lo = i32(k % 3u) - 1;
    let hi = i32(k / 3u) - 1;
    if (table == MOORE_COLUMN_MAJOR) {
        return vec2<i32>(hi, lo);
    }
    return vec2<i32>(lo, hi);
}

// Heading as one of eight octants, clockwise from east because the display's y axis points down.
fn heading_octant(vx: f32, vy: f32) -> u32 {
    let east = vx >= 0.0;
    let south = vy >= 0.0;
    let steep = abs(vy) > abs(vx);
    if (east && south && !steep) { return 0u; }
    if (east && south && steep) { return 1u; }
    if (!east && south && steep) { return 2u; }
    if (!east && south && !steep) { return 3u; }
    if (!east && !south && !steep) { return 4u; }
    if (!east && !south && steep) { return 5u; }
    if (east && !south && steep) { return 6u; }
    return 7u;
}
