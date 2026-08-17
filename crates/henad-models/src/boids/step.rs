use std::cell::RefCell;

use henad_core::authoring::model::agent_model::{AgentModel as _, StepCtx};
use henad_core::authoring::primitives::space::{Boundary, axis_delta, heading_octant, wrap_coord};

use crate::boids::lanes::{BoidChunk, BoidLanes, BoidRead};
use crate::boids::{BoidParams, BoidsModel};

thread_local! {
    /// Neighbour ids from the last `query_radius`, reused so a query does not allocate.
    static BUF: RefCell<Vec<u32>> = RefCell::new(Vec::with_capacity(64));
}

pub(crate) fn run(lanes: &mut BoidLanes, ctx: &StepCtx<'_, BoidsModel>, seed: u64, tick: u64) {
    let hash = ctx.index;
    let params = ctx.params;
    lanes.run_pass(BoidsModel::CHUNK, seed, tick, |i, k, read, chunk, _rng| {
        BUF.with(|cell| {
            let mut buf = cell.borrow_mut();
            step_agent(i, k, read, chunk, hash, params, &mut buf);
        });
    });
}

#[inline]
fn step_agent(
    i: usize,
    k: usize,
    read: BoidRead<'_>,
    out: &mut BoidChunk<'_>,
    hash: &henad_core::spatial_hash::SpatialHash,
    params: &BoidParams,
    buf: &mut Vec<u32>,
) {
    let (pos_x, pos_y, vel_x, vel_y) = (read.pos_x, read.pos_y, read.vel_x, read.vel_y);
    hash.query_radius(pos_x[i], pos_y[i], params.visual_range, pos_x, pos_y, buf);

    let mut close_dx = 0.0;
    let mut close_dy = 0.0;
    let mut avg_vx = 0.0;
    let mut avg_vy = 0.0;
    let mut avg_px = 0.0;
    let mut avg_py = 0.0;
    let mut count = 0u32;

    for &j in buf.iter() {
        if j == i as u32 {
            continue;
        }
        // Not `space::dist_sq` below, which would have to recompute both deltas, and the
        // separation and cohesion sums still need them.
        let dx = axis_delta(pos_x[i], pos_x[j as usize], params.world_w, Boundary::Torus);
        let dy = axis_delta(pos_y[i], pos_y[j as usize], params.world_h, Boundary::Torus);

        let dist_sq = dx * dx + dy * dy;

        if dist_sq < params.protected_sq {
            close_dx -= dx;
            close_dy -= dy;
        }

        if dist_sq < params.visual_sq {
            avg_vx += vel_x[j as usize];
            avg_vy += vel_y[j as usize];
            avg_px += pos_x[i] + dx;
            avg_py += pos_y[i] + dy;
            count += 1;
        }
    }

    let mut new_vx = vel_x[i] + close_dx * params.separation_factor;
    let mut new_vy = vel_y[i] + close_dy * params.separation_factor;

    if count > 0 {
        let count_inv = 1.0 / count as f32;
        new_vx += (avg_vx * count_inv - vel_x[i]) * params.alignment_factor
            + (avg_px * count_inv - pos_x[i]) * params.cohesion_factor;
        new_vy += (avg_vy * count_inv - vel_y[i]) * params.alignment_factor
            + (avg_py * count_inv - pos_y[i]) * params.cohesion_factor;
    }

    let speed_sq = new_vx * new_vx + new_vy * new_vy;
    if speed_sq > 0.0 {
        let speed = speed_sq.sqrt();
        if speed > params.max_speed {
            new_vx = new_vx / speed * params.max_speed;
            new_vy = new_vy / speed * params.max_speed;
        } else if speed < params.min_speed {
            new_vx = new_vx / speed * params.min_speed;
            new_vy = new_vy / speed * params.min_speed;
        }
    } else {
        new_vx = params.min_speed;
        new_vy = 0.0;
    }

    out.pos_x[k] = wrap_coord(pos_x[i] + new_vx, params.world_w);
    out.pos_y[k] = wrap_coord(pos_y[i] + new_vy, params.world_h);
    out.vel_x[k] = new_vx;
    out.vel_y[k] = new_vy;
    out.color[k] = heading_octant(new_vx, new_vy);
}

#[cfg(test)]
mod tests {
    use henad_core::authoring::primitives::space::heading_octant;

    use crate::boids::HEADING_PALETTE;

    /// Every octant must land inside the palette, or the renderer silently falls back to entry 0.
    #[test]
    fn every_heading_indexes_the_palette() {
        for step in 0..64u8 {
            let angle = f32::from(step) * std::f32::consts::TAU / 64.0;
            let octant = heading_octant(angle.cos(), angle.sin());
            assert!(
                (octant as usize) < HEADING_PALETTE.len(),
                "angle {angle} gave octant {octant}"
            );
        }
    }
}
