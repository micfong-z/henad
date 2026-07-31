use henad_core::grid::Grid2D;
use henad_core::helpers::xorshift64;

use crate::ants::state::{AGENT_CHUNK, AntsState, FOOD, HOME, LOW_PHEROMONE, NO_STEP, OBSTACLE};

/// One tick.
///
/// The merge comes last so neither agent pass sees this tick's deposits, matching the reference's
/// read and write field split.
pub(crate) fn step(state: &mut AntsState) {
    #[cfg(not(target_arch = "wasm32"))]
    let delivered = {
        deposit_parallel(state);
        advect_parallel(state)
    };
    #[cfg(target_arch = "wasm32")]
    let delivered = {
        deposit_sequential(state);
        advect_sequential(state)
    };

    state.deliveries += u64::from(delivered);
    merge_and_evaporate(state);
    state.quantise_for_display();
    state.tick += 1;
}

// Pass 1, over agents.

struct DepositParams {
    w: i32,
    h: i32,
    cutdown: f32,
    /// Cutdown raised to the diagonal distance, since those neighbours are further away.
    diagonal: f32,
}

impl DepositParams {
    fn new(state: &AntsState) -> Self {
        Self {
            w: state.width as i32,
            h: state.height as i32,
            cutdown: state.update_cutdown,
            diagonal: state.update_cutdown.powf(std::f32::consts::SQRT_2),
        }
    }
}

/// Read side of the deposit pass.
struct DepositRead<'a> {
    pos_x: &'a [f32],
    pos_y: &'a [f32],
    has_food: &'a [u8],
    reward: &'a [f32],
    to_food: &'a [f32],
    to_home: &'a [f32],
}

/// Largest pheromone in the 3x3 neighbourhood, cut down by distance and lifted by the reward.
///
/// Floored at what the cell already holds, which is why `max` downstream reproduces the
/// reference's plain overwrite. A deposit can never come out below the existing value.
#[inline]
fn deposit_value(x: i32, y: i32, reward: f32, field: &[f32], p: &DepositParams) -> f32 {
    let mut best = field[(y * p.w + x) as usize];
    for dx in -1..=1 {
        for dy in -1..=1 {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= p.w || ny >= p.h {
                continue;
            }
            let cut = if dx * dy != 0 { p.diagonal } else { p.cutdown };
            let m = field[(ny * p.w + nx) as usize] * cut + reward;
            if m > best {
                best = m;
            }
        }
    }
    best
}

/// Fills one chunk's `(cell, food, home)` deposit lanes.
///
/// An ant deposits into one field, so the other lane gets the `Combine::Max` identity and both
/// stay dense.
#[inline]
fn deposit_chunk(
    base: usize,
    cells: &mut [u32],
    food: &mut [f32],
    home: &mut [f32],
    r: &DepositRead<'_>,
    p: &DepositParams,
) {
    for k in 0..cells.len() {
        let i = base + k;
        let x = r.pos_x[i] as i32;
        let y = r.pos_y[i] as i32;
        cells[k] = (y * p.w + x) as u32;

        if r.has_food[i] != 0 {
            food[k] = deposit_value(x, y, r.reward[i], r.to_food, p);
            home[k] = 0.0;
        } else {
            food[k] = 0.0;
            home[k] = deposit_value(x, y, r.reward[i], r.to_home, p);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn deposit_parallel(state: &mut AntsState) {
    use rayon::prelude::*;

    let p = DepositParams::new(state);
    let AntsState {
        pos_x,
        pos_y,
        has_food,
        reward,
        to_food,
        to_home,
        deposit_cell,
        deposit_food,
        deposit_home,
        ..
    } = state;
    let r = DepositRead {
        pos_x,
        pos_y,
        has_food,
        reward,
        to_food: to_food.current(),
        to_home: to_home.current(),
    };

    deposit_cell
        .par_chunks_mut(AGENT_CHUNK)
        .zip(deposit_food.par_chunks_mut(AGENT_CHUNK))
        .zip(deposit_home.par_chunks_mut(AGENT_CHUNK))
        .enumerate()
        .for_each(|(ci, ((cells, food), home))| deposit_chunk(ci * AGENT_CHUNK, cells, food, home, &r, &p));
}

#[cfg(target_arch = "wasm32")]
fn deposit_sequential(state: &mut AntsState) {
    let p = DepositParams::new(state);
    let AntsState {
        pos_x,
        pos_y,
        has_food,
        reward,
        to_food,
        to_home,
        deposit_cell,
        deposit_food,
        deposit_home,
        ..
    } = state;
    let r = DepositRead {
        pos_x,
        pos_y,
        has_food,
        reward,
        to_food: to_food.current(),
        to_home: to_home.current(),
    };

    for (ci, ((cells, food), home)) in deposit_cell
        .chunks_mut(AGENT_CHUNK)
        .zip(deposit_food.chunks_mut(AGENT_CHUNK))
        .zip(deposit_home.chunks_mut(AGENT_CHUNK))
        .enumerate()
    {
        deposit_chunk(ci * AGENT_CHUNK, cells, food, home, &r, &p);
    }
}

// Pass 2, over agents.

struct MoveParams {
    w: i32,
    h: i32,
    momentum: f32,
    random_action: f32,
    reward: f32,
}

impl MoveParams {
    fn new(state: &AntsState) -> Self {
        Self {
            w: state.width as i32,
            h: state.height as i32,
            momentum: state.momentum_probability,
            random_action: state.random_action_probability,
            reward: state.reward_value,
        }
    }
}

struct MoveRead<'a> {
    to_food: &'a [f32],
    to_home: &'a [f32],
    sites: &'a [u8],
}

#[inline]
fn encode_step(dx: i32, dy: i32) -> u8 {
    ((dx + 1) * 3 + (dy + 1)) as u8
}

#[inline]
fn decode_step(s: u8) -> (i32, i32) {
    let s = i32::from(s);
    (s / 3 - 1, s % 3 - 1)
}

/// Inside the field and not an obstacle. This model is bounded, not toroidal like the others.
#[inline]
fn passable(x: i32, y: i32, sites: &[u8], p: &MoveParams) -> bool {
    x >= 0 && y >= 0 && x < p.w && y < p.h && sites[(y * p.w + x) as usize] != OBSTACLE
}

#[inline]
fn next_unit(rng: &mut u64) -> f32 {
    *rng = xorshift64(*rng);
    ((*rng >> 40) as f32) / 16_777_216.0
}

#[inline]
fn next_delta(rng: &mut u64) -> i32 {
    *rng = xorshift64(*rng);
    ((*rng >> 32) % 3) as i32 - 1
}

/// Derived from the chunk index rather than a shared running state, so the stream an ant sees does
/// not depend on how rayon schedules the work. Same rule `grid_engine` uses per row.
#[inline]
fn chunk_seed(base: u64, tick: u64, chunk: usize) -> u64 {
    let mixed = base ^ tick.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (chunk as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    // xorshift64's state may never be zero.
    xorshift64(mixed | 1)
}

struct AntMove {
    x: i32,
    y: i32,
    last_step: u8,
    has_food: u8,
    reward: f32,
    delivered: bool,
}

/// One ant's move, following the reference's `Ant::act`.
///
/// The `dx` outer, `dy` inner order is load-bearing. Ties between equal pheromone are broken by a
/// reservoir draw, so the visit order changes the outcome.
fn advect_agent(
    x: i32,
    y: i32,
    last_step: u8,
    has_food: u8,
    r: &MoveRead<'_>,
    p: &MoveParams,
    rng: &mut u64,
) -> AntMove {
    // Ants follow the trip they are not currently making, so carrying food reads the home field.
    let field = if has_food != 0 { r.to_home } else { r.to_food };

    // An impossible pheromone, so the first passable neighbour always wins.
    let mut best = -1.0f32;
    let (mut bx, mut by) = (x, y);
    let mut count = 2u32;

    for dx in -1..=1 {
        for dy in -1..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let (nx, ny) = (x + dx, y + dy);
            if !passable(nx, ny, r.sites, p) {
                continue;
            }
            let m = field[(ny * p.w + nx) as usize];
            if m > best {
                count = 2;
            }
            if m > best || (m == best && next_unit(rng) < 1.0 / count as f32) {
                best = m;
                bx = nx;
                by = ny;
            }
            count += 1;
        }
    }

    if best == 0.0 && last_step != NO_STEP {
        // No pheromone nearby, so probably keep going the way we were.
        if next_unit(rng) < p.momentum {
            let (dx, dy) = decode_step(last_step);
            let (mx, my) = (x + dx, y + dy);
            if passable(mx, my, r.sites, p) {
                bx = mx;
                by = my;
            }
        }
    } else if next_unit(rng) < p.random_action {
        let (dx, dy) = (next_delta(rng), next_delta(rng));
        let (mx, my) = (x + dx, y + dy);
        if !(dx == 0 && dy == 0) && passable(mx, my, r.sites, p) {
            bx = mx;
            by = my;
        }
    }

    let mut out = AntMove {
        x: bx,
        y: by,
        last_step: encode_step(bx - x, by - y),
        has_food,
        // The deposit pass spent whatever the ant was carrying. Only a site grants more.
        reward: 0.0,
        delivered: false,
    };

    match r.sites[(by * p.w + bx) as usize] {
        HOME if has_food != 0 => {
            out.reward = p.reward;
            out.has_food = 0;
            out.delivered = true;
        }
        FOOD if has_food == 0 => {
            out.reward = p.reward;
            out.has_food = 1;
        }
        _ => {}
    }
    out
}

/// The lanes one movement chunk owns, in the order the rayon zip yields them.
type MoveLanes<'a> = (&'a mut [f32], &'a mut [f32], &'a mut [u8], &'a mut [u8], &'a mut [f32]);

/// Moves one chunk of ants in place, returning how many delivered food home.
///
/// In place because ants never read one another, so no lane needs a double buffer.
#[inline]
fn advect_chunk(lanes: MoveLanes<'_>, r: &MoveRead<'_>, p: &MoveParams, seed: u64) -> u32 {
    let (pos_x, pos_y, last_step, has_food, reward) = lanes;
    let mut rng = seed;
    let mut delivered = 0;

    for k in 0..pos_x.len() {
        let out = advect_agent(
            pos_x[k] as i32,
            pos_y[k] as i32,
            last_step[k],
            has_food[k],
            r,
            p,
            &mut rng,
        );
        pos_x[k] = out.x as f32;
        pos_y[k] = out.y as f32;
        last_step[k] = out.last_step;
        has_food[k] = out.has_food;
        reward[k] = out.reward;
        delivered += u32::from(out.delivered);
    }
    delivered
}

#[cfg(not(target_arch = "wasm32"))]
fn advect_parallel(state: &mut AntsState) -> u32 {
    use rayon::prelude::*;

    let p = MoveParams::new(state);
    let (seed, tick) = (state.rng_seed, state.tick);
    let AntsState {
        pos_x,
        pos_y,
        last_step,
        has_food,
        reward,
        to_food,
        to_home,
        sites,
        ..
    } = state;
    let r = MoveRead {
        to_food: to_food.current(),
        to_home: to_home.current(),
        sites,
    };

    let per_chunk: Vec<u32> = pos_x
        .par_chunks_mut(AGENT_CHUNK)
        .zip(pos_y.par_chunks_mut(AGENT_CHUNK))
        .zip(last_step.par_chunks_mut(AGENT_CHUNK))
        .zip(has_food.par_chunks_mut(AGENT_CHUNK))
        .zip(reward.par_chunks_mut(AGENT_CHUNK))
        .enumerate()
        .map(|(ci, ((((px, py), ls), hf), rw))| advect_chunk((px, py, ls, hf, rw), &r, &p, chunk_seed(seed, tick, ci)))
        .collect();

    // Folded in chunk order, not completion order.
    per_chunk.iter().sum()
}

#[cfg(target_arch = "wasm32")]
fn advect_sequential(state: &mut AntsState) -> u32 {
    let p = MoveParams::new(state);
    let (seed, tick) = (state.rng_seed, state.tick);
    let AntsState {
        pos_x,
        pos_y,
        last_step,
        has_food,
        reward,
        to_food,
        to_home,
        sites,
        ..
    } = state;
    let r = MoveRead {
        to_food: to_food.current(),
        to_home: to_home.current(),
        sites,
    };

    let mut total = 0;
    for (ci, ((((px, py), ls), hf), rw)) in pos_x
        .chunks_mut(AGENT_CHUNK)
        .zip(pos_y.chunks_mut(AGENT_CHUNK))
        .zip(last_step.chunks_mut(AGENT_CHUNK))
        .zip(has_food.chunks_mut(AGENT_CHUNK))
        .zip(reward.chunks_mut(AGENT_CHUNK))
        .enumerate()
    {
        total += advect_chunk((px, py, ls, hf, rw), &r, &p, chunk_seed(seed, tick, ci));
    }
    total
}

// Pass 3, over cells.

/// Lands this tick's deposits into each field, decays every cell, and swaps.
///
/// Decay after the merge, matching the reference, so a fresh deposit is already one evaporation
/// step old by the time anything reads it.
fn merge_and_evaporate(state: &mut AntsState) {
    let evaporation = state.evaporation;

    {
        let (current, next) = state.to_food.current_and_next_mut();
        state
            .scatter
            .scatter(&state.deposit_cell, &state.deposit_food, current, next);
    }
    evaporate(state.to_food.next_mut(), evaporation);
    state.to_food.swap();

    {
        let (current, next) = state.to_home.current_and_next_mut();
        state
            .scatter
            .scatter(&state.deposit_cell, &state.deposit_home, current, next);
    }
    evaporate(state.to_home.next_mut(), evaporation);
    state.to_home.swap();
}

#[inline]
fn decay(v: f32, evaporation: f32) -> f32 {
    let d = v * evaporation;
    // Without the floor a trail never disappears, it just asymptotes.
    if d < LOW_PHEROMONE { 0.0 } else { d }
}

fn evaporate(cells: &mut [f32], evaporation: f32) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        cells.par_iter_mut().for_each(|v| *v = decay(*v, evaporation));
    }
    #[cfg(target_arch = "wasm32")]
    for v in cells.iter_mut() {
        *v = decay(*v, evaporation);
    }
}

/// Cells per rayon chunk in the stats reduction. See `crate::boids::state::STATS_CHUNK`.
const STATS_CHUNK: usize = 8192;

fn chunk_sum(cells: &[f32]) -> f64 {
    let mut total = 0.0f64;
    for &v in cells {
        total += f64::from(v);
    }
    total
}

/// Summed chunk by chunk in index order, so rayon's scheduling cannot change the total.
pub(crate) fn total_pheromone(to_food: &Grid2D<f32>, to_home: &Grid2D<f32>) -> f64 {
    field_sum(to_food.current()) + field_sum(to_home.current())
}

fn field_sum(cells: &[f32]) -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    let partials: Vec<f64> = {
        use rayon::prelude::*;
        cells.par_chunks(STATS_CHUNK).map(chunk_sum).collect()
    };
    #[cfg(target_arch = "wasm32")]
    let partials: Vec<f64> = cells.chunks(STATS_CHUNK).map(chunk_sum).collect();

    partials.iter().sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_encoding_round_trips_every_direction() {
        for dx in -1..=1 {
            for dy in -1..=1 {
                assert_eq!(decode_step(encode_step(dx, dy)), (dx, dy));
            }
        }
    }

    /// A collision would give an ant that has never moved somebody else's momentum.
    #[test]
    fn no_step_is_outside_the_encoding() {
        let encodable: Vec<u8> = (-1..=1)
            .flat_map(|dx| (-1..=1).map(move |dy| encode_step(dx, dy)))
            .collect();
        assert!(!encodable.contains(&NO_STEP), "NO_STEP overlaps a real direction");
    }
}
