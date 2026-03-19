use henad_core::spatial_hash::SpatialHash;

use crate::boids::state::BoidsState;

pub(crate) fn step(state: &mut BoidsState) {
    state.hash.build(&state.pos_x, &state.pos_y);

    #[cfg(not(target_arch = "wasm32"))]
    step_parallel(state);
    #[cfg(target_arch = "wasm32")]
    step_sequential(state);

    state.swap_buffers();
    state.avg_speed = state
        .vel_x
        .iter()
        .zip(state.vel_y.iter())
        .map(|(vx, vy)| vx.hypot(*vy))
        .sum::<f32>()
        * state.num_boids_inv;
    state.avg_vel_x = state.vel_x.iter().sum::<f32>() * state.num_boids_inv;
    state.avg_vel_y = state.vel_y.iter().sum::<f32>() * state.num_boids_inv;
    state.tick += 1;
}

struct BoidParams {
    visual_sq: f32,
    visual_range: f32,
    protected_sq: f32,
    separation_factor: f32,
    alignment_factor: f32,
    cohesion_factor: f32,
    max_speed: f32,
    min_speed: f32,
    half_w: f32,
    half_h: f32,
    world_w: f32,
    world_h: f32,
}

#[inline]
#[expect(
    clippy::too_many_arguments,
    reason = "Boid behavior depends on many parameters"
)]
fn process_agent(
    i: usize,
    pos_x: &[f32],
    pos_y: &[f32],
    vel_x: &[f32],
    vel_y: &[f32],
    hash: &SpatialHash,
    params: &BoidParams,
    buf: &mut Vec<u32>,
) -> (f32, f32, f32, f32) {
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
        let mut dx = pos_x[j as usize] - pos_x[i];
        let mut dy = pos_y[j as usize] - pos_y[i];

        // Handle toroidal wraparound
        if dx > params.half_w {
            dx -= params.world_w;
        } else if dx < -params.half_w {
            dx += params.world_w;
        }
        if dy > params.half_h {
            dy -= params.world_h;
        } else if dy < -params.half_h {
            dy += params.world_h;
        }

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

    (
        (pos_x[i] + new_vx).rem_euclid(params.world_w),
        (pos_y[i] + new_vy).rem_euclid(params.world_h),
        new_vx,
        new_vy,
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn step_parallel(state: &mut BoidsState) {
    use std::cell::RefCell;

    use rayon::prelude::*;

    let params = BoidParams {
        visual_sq: state.visual_range * state.visual_range,
        visual_range: state.visual_range,
        protected_sq: state.protected_range * state.protected_range,
        separation_factor: state.separation_factor,
        alignment_factor: state.alignment_factor,
        cohesion_factor: state.cohesion_factor,
        max_speed: state.max_speed,
        min_speed: state.min_speed,
        half_w: 0.5 * state.world_w,
        half_h: 0.5 * state.world_h,
        world_w: state.world_w,
        world_h: state.world_h,
    };

    let pos_x = &state.pos_x;
    let pos_y = &state.pos_y;
    let vel_x = &state.vel_x;
    let vel_y = &state.vel_y;
    let hash = &state.hash;

    thread_local! {
        static BUF: RefCell<Vec<u32>> = RefCell::new(Vec::with_capacity(64));
    }

    state
        .next_pos_x
        .par_iter_mut()
        .zip(state.next_pos_y.par_iter_mut())
        .zip(state.next_vel_x.par_iter_mut())
        .zip(state.next_vel_y.par_iter_mut())
        .enumerate()
        .for_each(|(i, (((new_px, new_py), new_vx), new_vy))| {
            BUF.with(|buf_cell| {
                let mut buf = buf_cell.borrow_mut();
                let (px, py, vx, vy) =
                    process_agent(i, pos_x, pos_y, vel_x, vel_y, hash, &params, &mut buf);
                *new_px = px;
                *new_py = py;
                *new_vx = vx;
                *new_vy = vy;
            });
        });
}

#[cfg(target_arch = "wasm32")]
fn step_sequential(state: &mut BoidsState) {
    let params = BoidParams {
        visual_sq: state.visual_range * state.visual_range,
        visual_range: state.visual_range,
        protected_sq: state.protected_range * state.protected_range,
        separation_factor: state.separation_factor,
        alignment_factor: state.alignment_factor,
        cohesion_factor: state.cohesion_factor,
        max_speed: state.max_speed,
        min_speed: state.min_speed,
        half_w: 0.5 * state.world_w,
        half_h: 0.5 * state.world_h,
        world_w: state.world_w,
        world_h: state.world_h,
    };

    let pos_x = &state.pos_x;
    let pos_y = &state.pos_y;
    let vel_x = &state.vel_x;
    let vel_y = &state.vel_y;
    let hash = &state.hash;
    let mut buf = Vec::with_capacity(64);

    for i in 0..state.num_boids as usize {
        let (new_px, new_py, new_vx, new_vy) =
            process_agent(i, pos_x, pos_y, vel_x, vel_y, hash, &params, &mut buf);
        state.next_pos_x[i] = new_px;
        state.next_pos_y[i] = new_py;
        state.next_vel_x[i] = new_vx;
        state.next_vel_y[i] = new_vy;
    }
}
