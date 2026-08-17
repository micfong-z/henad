use henad_compute::cpu::field::scalar::{Deposits, ScalarRead};
use henad_compute::for_each_chunk_mut;
use henad_core::authoring::model::agent_model::{AgentModel as _, StepCtx};
use henad_core::authoring::primitives::rng::{choice3, next_bits, next_float, reservoir_accept};
use henad_core::authoring::primitives::space::{Boundary, MOORE_COLUMN_MAJOR, cell_index, offset_cell};

use crate::ants::field::{FOOD, HOME, OBSTACLE, TO_FOOD, TO_HOME};
use crate::ants::lanes::{AntChunk, AntLanes, NO_STEP};
use crate::ants::{AntParams, AntsModel};

// Pass 1, over agents.

/// Largest pheromone in the 3x3 neighbourhood, cut down by distance and lifted by the reward.
///
/// Floored at what the cell already holds, which is why `max` downstream reproduces the
/// reference's plain overwrite. A deposit can never come out below the existing value.
#[inline]
fn deposit_value(x: i32, y: i32, reward: f32, field: &[f32], p: &AntParams) -> f32 {
    let here = field[cell_index(x as u32, y as u32, p.w as u32) as usize];
    // The centre counts as an orthogonal neighbour, and a max reduction does not care about order.
    let mut best = here.max(here * p.cutdown + reward);
    for &(dx, dy) in &MOORE_COLUMN_MAJOR {
        let Some((nx, ny)) = offset_cell(x as u32, y as u32, dx, dy, p.w as u32, p.h as u32, Boundary::Bounded) else {
            continue;
        };
        let cut = if dx * dy != 0 { p.diagonal } else { p.cutdown };
        let m = field[cell_index(nx, ny, p.w as u32) as usize] * cut + reward;
        if m > best {
            best = m;
        }
    }
    best
}

/// Fills the `(cell, to_food, to_home)` deposit lanes.
///
/// An ant deposits into one field, so the other lane gets the `Combine::Max` identity and both
/// stay dense.
pub(crate) fn deposit(lanes: &AntLanes, deposits: &mut Deposits, ctx: &StepCtx<'_, AntsModel>) {
    let p = ctx.params;
    let (to_food, to_home) = (ctx.field.field(TO_FOOD), ctx.field.field(TO_HOME));
    let (pos_x, pos_y) = (&lanes.pos_x, &lanes.pos_y);
    let (has_food, reward) = (&lanes.has_food, &lanes.reward);

    let Deposits { cell, values } = deposits;
    let (head, tail) = values.split_at_mut(1);
    let (food_lane, home_lane) = (&mut head[0], &mut tail[0]);

    for_each_chunk_mut!(
        cell,
        food_lane,
        home_lane,
        AntsModel::CHUNK,
        |_c, base, cells, food, home| {
            for k in 0..cells.len() {
                let i = base + k;
                let x = pos_x[i] as i32;
                let y = pos_y[i] as i32;
                cells[k] = (y * p.w + x) as u32;

                if has_food[i] != 0 {
                    food[k] = deposit_value(x, y, reward[i], to_food, p);
                    home[k] = 0.0;
                } else {
                    food[k] = 0.0;
                    home[k] = deposit_value(x, y, reward[i], to_home, p);
                }
            }
        }
    );
}

// Pass 2, over agents.

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
fn passable(x: i32, y: i32, sites: &[u8], p: &AntParams) -> bool {
    x >= 0 && y >= 0 && x < p.w && y < p.h && sites[(y * p.w + x) as usize] != OBSTACLE
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
    field: ScalarRead<'_>,
    p: &AntParams,
    rng: &mut u64,
) -> AntMove {
    let sites = field.sites;
    // Ants follow the trip they are not currently making, so carrying food reads the home field.
    let trail = if has_food != 0 {
        field.field(TO_HOME)
    } else {
        field.field(TO_FOOD)
    };

    // An impossible pheromone, so the first passable neighbour always wins.
    let mut best = -1.0f32;
    let (mut bx, mut by) = (x, y);
    // 2 not 1 is the reference's off-by-one, giving the first neighbour visited 2/(k+1) against
    // 1/(k+1) for the rest, which drifts ants up-left. Kept deliberately, see the gap report.
    let mut count = 2u32;

    for &(dx, dy) in &MOORE_COLUMN_MAJOR {
        let (nx, ny) = (x + dx, y + dy);
        if !passable(nx, ny, sites, p) {
            continue;
        }
        let m = trail[(ny * p.w + nx) as usize];
        if m > best {
            count = 2;
        }
        if m > best || (m == best && reservoir_accept(next_bits(rng), count)) {
            best = m;
            bx = nx;
            by = ny;
        }
        count += 1;
    }

    if best == 0.0 && last_step != NO_STEP {
        // No pheromone nearby, so probably keep going the way we were.
        if next_float(rng, 1.0) < p.momentum {
            let (dx, dy) = decode_step(last_step);
            let (mx, my) = (x + dx, y + dy);
            if passable(mx, my, sites, p) {
                bx = mx;
                by = my;
            }
        }
    } else if next_float(rng, 1.0) < p.random_action {
        let (dx, dy) = (choice3(next_bits(rng)), choice3(next_bits(rng)));
        let (mx, my) = (x + dx, y + dy);
        if !(dx == 0 && dy == 0) && passable(mx, my, sites, p) {
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

    match sites[(by * p.w + bx) as usize] {
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

/// Moves every ant, returning how many delivered food home.
pub(crate) fn advect(lanes: &mut AntLanes, ctx: &StepCtx<'_, AntsModel>, seed: u64, tick: u64) -> u64 {
    let p = ctx.params;
    let field = ctx.field;
    lanes.run_pass(
        AntsModel::CHUNK,
        seed,
        tick,
        |_i, k, _read, c: &mut AntChunk<'_>, rng| {
            let out = advect_agent(
                c.pos_x[k] as i32,
                c.pos_y[k] as i32,
                c.last_step[k],
                c.has_food[k],
                field,
                p,
                rng,
            );
            c.pos_x[k] = out.x as f32;
            c.pos_y[k] = out.y as f32;
            c.last_step[k] = out.last_step;
            c.has_food[k] = out.has_food;
            c.reward[k] = out.reward;
            u64::from(out.delivered)
        },
    )
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
