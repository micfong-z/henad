// One ant per invocation, mirroring `ants/step.rs`.
//
// The CPU's deposit and advect passes are fused here. Both read the field as it stands before
// this tick's merge, and an ant only ever touches its own lanes, so one invocation doing both in
// order is the same computation.

#import shared::prelude::linear_index
#import shared::rng::pcg_hash
#import gpu_ants::state::{LAST_STEP_MASK, HAS_FOOD_BIT, HAS_REWARD_BIT}

struct Params {
    num_agents: u32,
    groups_x: u32,
    grid_w: u32,
    grid_h: u32,

    n_cells: u32,
    cutdown: f32,
    diagonal: f32,
    reward: f32,

    momentum: f32,
    random_action: f32,
    // Searching and carrying, in the uniform to keep a storage binding free.
    palette: vec2<u32>,
}

@group(0) @binding(0) var<storage, read_write> pos: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read_write> state: array<u32>;
@group(0) @binding(2) var<storage, read_write> color: array<u32>;
@group(0) @binding(3) var<storage, read_write> rng: array<u32>;
@group(0) @binding(4) var<storage, read>       field: array<f32>;
@group(0) @binding(5) var<storage, read_write> accum: array<atomic<u32>>;
@group(0) @binding(6) var<storage, read>       sites: array<u32>;
@group(0) @binding(7) var<storage, read_write> counters: array<atomic<u32>>;
@group(0) @binding(8) var<uniform>             params: Params;


// Matches `ants::field`.
const OBSTACLE: u32 = 1u;
const FOOD: u32 = 2u;
const HOME: u32 = 3u;
const TO_FOOD: u32 = 0u;
const TO_HOME: u32 = 1u;

// Matches `ants::lanes::NO_STEP`.
const NO_STEP: u32 = 255u;

const DELIVERIES: u32 = 0u;

fn next_unit(r: ptr<function, u32>) -> f32 {
    *r = pcg_hash(*r);
    return f32(*r) / 4294967295.0;
}

fn next_delta(r: ptr<function, u32>) -> i32 {
    *r = pcg_hash(*r);
    return i32(*r % 3u) - 1;
}

fn cell_of(x: i32, y: i32) -> u32 {
    return u32(y * i32(params.grid_w) + x);
}

fn in_field(x: i32, y: i32) -> bool {
    return x >= 0 && y >= 0 && x < i32(params.grid_w) && y < i32(params.grid_h);
}

// This model is bounded, not toroidal like the others.
fn passable(x: i32, y: i32) -> bool {
    return in_field(x, y) && sites[cell_of(x, y)] != OBSTACLE;
}

// Mirrors `ants::step::deposit_value`. Floored at what the cell already holds, which is why
// `atomicMax` downstream reproduces the reference's plain overwrite.
fn deposit_value(x: i32, y: i32, reward: f32, base: u32) -> f32 {
    var best = field[base + cell_of(x, y)];
    for (var dx = -1; dx <= 1; dx = dx + 1) {
        for (var dy = -1; dy <= 1; dy = dy + 1) {
            let nx = x + dx;
            let ny = y + dy;
            if (!in_field(nx, ny)) {
                continue;
            }
            var cut = params.cutdown;
            if (dx * dy != 0) {
                cut = params.diagonal;
            }
            best = max(best, field[base + cell_of(nx, ny)] * cut + reward);
        }
    }
    return best;
}

@compute
@workgroup_size(256)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let i = linear_index(lid, wid, params.groups_x);
    if (i >= params.num_agents) {
        return;
    }

    let p = pos[i];
    let x = i32(p.x);
    let y = i32(p.y);
    let packed = state[i];
    let last_step = packed & LAST_STEP_MASK;
    let has_food = (packed & HAS_FOOD_BIT) != 0u;
    var reward = 0.0;
    if ((packed & HAS_REWARD_BIT) != 0u) {
        reward = params.reward;
    }

    // An ant lays the trail for the trip it just made and follows the one it is making, so
    // carrying food lays to-food and follows to-home.
    var lay = TO_HOME;
    var follow = TO_FOOD;
    if (has_food) {
        lay = TO_FOOD;
        follow = TO_HOME;
    }
    let lay_base = lay * params.n_cells;
    let value = deposit_value(x, y, reward, lay_base);
    // Non-negative f32 compares the same as its bit pattern, so an integer max is a float max.
    atomicMax(&accum[lay_base + cell_of(x, y)], bitcast<u32>(value));

    var r = rng[i];
    let trail_base = follow * params.n_cells;

    // An impossible pheromone, so the first passable neighbour always wins.
    var best = -1.0;
    var bx = x;
    var by = y;
    // 2 not 1 is the reference's off-by-one, giving the first neighbour visited 2/(k+1) against
    // 1/(k+1) for the rest, which drifts ants up-left. Kept deliberately, see the gap report.
    var count = 2u;

    // The `dx` outer, `dy` inner order is load-bearing. Ties are broken by a reservoir draw, so
    // the visit order changes the outcome.
    for (var dx = -1; dx <= 1; dx = dx + 1) {
        for (var dy = -1; dy <= 1; dy = dy + 1) {
            if (dx == 0 && dy == 0) {
                continue;
            }
            let nx = x + dx;
            let ny = y + dy;
            if (!passable(nx, ny)) {
                continue;
            }
            let m = field[trail_base + cell_of(nx, ny)];
            if (m > best) {
                count = 2u;
            }
            if (m > best || (m == best && next_unit(&r) < 1.0 / f32(count))) {
                best = m;
                bx = nx;
                by = ny;
            }
            count = count + 1u;
        }
    }

    if (best == 0.0 && last_step != NO_STEP) {
        // No pheromone nearby, so probably keep going the way we were.
        if (next_unit(&r) < params.momentum) {
            let mx = x + i32(last_step / 3u) - 1;
            let my = y + i32(last_step % 3u) - 1;
            if (passable(mx, my)) {
                bx = mx;
                by = my;
            }
        }
    } else if (next_unit(&r) < params.random_action) {
        let dx = next_delta(&r);
        let dy = next_delta(&r);
        let mx = x + dx;
        let my = y + dy;
        if (!(dx == 0 && dy == 0) && passable(mx, my)) {
            bx = mx;
            by = my;
        }
    }

    // The deposit above spent whatever the ant was carrying. Only a site grants more.
    var out_food = has_food;
    var out_reward = false;
    let site = sites[cell_of(bx, by)];
    if (site == HOME && has_food) {
        out_food = false;
        out_reward = true;
        atomicAdd(&counters[DELIVERIES], 1u);
    } else if (site == FOOD && !has_food) {
        out_food = true;
        out_reward = true;
    }

    var out_state = u32((bx - x + 1) * 3 + (by - y + 1));
    if (out_food) {
        out_state = out_state | HAS_FOOD_BIT;
    }
    if (out_reward) {
        out_state = out_state | HAS_REWARD_BIT;
    }

    pos[i] = vec2<f32>(f32(bx), f32(by));
    state[i] = out_state;
    rng[i] = r;
    color[i] = select(params.palette.x, params.palette.y, out_food);
}
