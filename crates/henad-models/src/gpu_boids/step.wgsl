// One boid per invocation, mirroring `boids/step.rs::step_agent`.
//
// Neighbours come from the index rebuilt earlier this step, walked exactly as
// `SpatialHash::query_radius` does. That query's `<= r^2` filter is folded away here, since the
// kernel's own `< visual_sq` and `< protected_sq` tests are strictly narrower.

struct Params {
    num_agents: u32,
    groups_x: u32,
    grid_w: u32,
    grid_h: u32,

    cell_w: f32,
    cell_h: f32,
    cell_w_inv: f32,
    cell_h_inv: f32,

    world_w: f32,
    world_h: f32,
    half_w: f32,
    half_h: f32,

    visual_range: f32,
    visual_sq: f32,
    protected_sq: f32,
    separation: f32,

    alignment: f32,
    cohesion: f32,
    max_speed: f32,
    min_speed: f32,

    // Heading colours, in the uniform to keep a storage binding free. Indexed as
    // `palette[o >> 2u][o & 3u]`.
    palette: array<vec4<u32>, 2>,
}

@group(0) @binding(0) var<storage, read>       pos_in: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read>       vel_in: array<vec2<f32>>;
@group(0) @binding(2) var<storage, read_write> pos_out: array<vec2<f32>>;
@group(0) @binding(3) var<storage, read_write> vel_out: array<vec2<f32>>;
@group(0) @binding(4) var<storage, read_write> color_out: array<u32>;
@group(0) @binding(5) var<storage, read>       cell_start: array<u32>;
@group(0) @binding(6) var<storage, read>       sorted: array<u32>;
@group(0) @binding(7) var<uniform>             params: Params;

const WORKGROUP: u32 = 256u;

fn wrap(v: i32, m: i32) -> i32 {
    return ((v % m) + m) % m;
}

// Mirrors `boids::step::heading_octant`. Half-open spans running clockwise from east, since the
// display's y axis points down.
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

@compute
@workgroup_size(256)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let i = (wid.y * params.groups_x + wid.x) * WORKGROUP + lid.x;
    if (i >= params.num_agents) {
        return;
    }

    let p = pos_in[i];
    let v = vel_in[i];

    let grid_w = i32(params.grid_w);
    let grid_h = i32(params.grid_h);
    let cell_radius_x = i32(ceil(params.visual_range / params.cell_w));
    let cell_radius_y = i32(ceil(params.visual_range / params.cell_h));
    let cell_x = wrap(i32(floor(p.x * params.cell_w_inv)), grid_w);
    let cell_y = wrap(i32(floor(p.y * params.cell_h_inv)), grid_h);

    // A radius wider than the world would walk the same cell twice. Same guard as the CPU query.
    var x_lo = cell_x - cell_radius_x;
    var x_hi = cell_x + cell_radius_x;
    if (2 * cell_radius_x + 1 > grid_w) {
        x_lo = 0;
        x_hi = grid_w - 1;
    }
    var y_lo = cell_y - cell_radius_y;
    var y_hi = cell_y + cell_radius_y;
    if (2 * cell_radius_y + 1 > grid_h) {
        y_lo = 0;
        y_hi = grid_h - 1;
    }

    var close = vec2<f32>(0.0, 0.0);
    var avg_vel = vec2<f32>(0.0, 0.0);
    var avg_pos = vec2<f32>(0.0, 0.0);
    var count = 0u;

    for (var gy = y_lo; gy <= y_hi; gy = gy + 1) {
        let wy = u32(wrap(gy, grid_h));
        for (var gx = x_lo; gx <= x_hi; gx = gx + 1) {
            let wx = u32(wrap(gx, grid_w));
            let cell = wy * params.grid_w + wx;
            let start = cell_start[cell];
            let end = cell_start[cell + 1u];

            for (var s = start; s < end; s = s + 1u) {
                let j = sorted[s];
                if (j == i) {
                    continue;
                }

                var d = pos_in[j] - p;
                if (d.x > params.half_w) {
                    d.x = d.x - params.world_w;
                } else if (d.x < -params.half_w) {
                    d.x = d.x + params.world_w;
                }
                if (d.y > params.half_h) {
                    d.y = d.y - params.world_h;
                } else if (d.y < -params.half_h) {
                    d.y = d.y + params.world_h;
                }

                let dist_sq = dot(d, d);
                if (dist_sq < params.protected_sq) {
                    close = close - d;
                }
                if (dist_sq < params.visual_sq) {
                    avg_vel = avg_vel + vel_in[j];
                    avg_pos = avg_pos + p + d;
                    count = count + 1u;
                }
            }
        }
    }

    var new_v = v + close * params.separation;

    if (count > 0u) {
        let count_inv = 1.0 / f32(count);
        new_v = new_v
            + (avg_vel * count_inv - v) * params.alignment
            + (avg_pos * count_inv - p) * params.cohesion;
    }

    let speed_sq = dot(new_v, new_v);
    if (speed_sq > 0.0) {
        let speed = sqrt(speed_sq);
        if (speed > params.max_speed) {
            new_v = new_v / speed * params.max_speed;
        } else if (speed < params.min_speed) {
            new_v = new_v / speed * params.min_speed;
        }
    } else {
        new_v = vec2<f32>(params.min_speed, 0.0);
    }

    // `rem_euclid`, so a boid leaving one edge re-enters at the other rather than clamping.
    let world = vec2<f32>(params.world_w, params.world_h);
    let moved = p + new_v;
    pos_out[i] = moved - floor(moved / world) * world;
    vel_out[i] = new_v;

    let octant = heading_octant(new_v.x, new_v.y);
    color_out[i] = params.palette[octant >> 2u][octant & 3u];
}
