use super::state::{INFECTED, RECOVERED, SUSCEPTIBLE, SirState, xorshift64};

pub(crate) fn step(state: &mut SirState) {
    #[cfg(not(target_arch = "wasm32"))]
    step_parallel(state);
    #[cfg(target_arch = "wasm32")]
    step_sequential(state);

    state.grid.swap();
    state.tick += 1;
    state.record_history();
}

struct RowParams {
    y: u32,
    w: u32,
    h: u32,
    infection_rate: f32,
    recovery_rate: f32,
}

/// Returns `(count_s, count_i, count_r, rng)`.
#[inline]
fn process_row(
    current: &[u8],
    next_row: &mut [u8],
    p: &RowParams,
    mut rng: u64,
) -> (u64, u64, u64, u64) {
    let mut count_s: u64 = 0;
    let mut count_i: u64 = 0;
    let mut count_r: u64 = 0;

    // * Assume toroidal grid, so neighbors wrap around edges

    let ws = p.w as usize;
    let ym = ((p.y + p.h - 1) % p.h) as usize;
    let yc = p.y as usize;
    let yp = ((p.y + 1) % p.h) as usize;

    for x in 0..p.w {
        let xs = x as usize;
        let xm = ((x + p.w - 1) % p.w) as usize;
        let xp = ((x + 1) % p.w) as usize;

        let cell = current[yc * ws + xs];

        match cell {
            SUSCEPTIBLE => {
                let infected_count = u32::from(current[ym * ws + xm] == INFECTED)
                    + u32::from(current[ym * ws + xs] == INFECTED)
                    + u32::from(current[ym * ws + xp] == INFECTED)
                    + u32::from(current[yc * ws + xm] == INFECTED)
                    + u32::from(current[yc * ws + xp] == INFECTED)
                    + u32::from(current[yp * ws + xm] == INFECTED)
                    + u32::from(current[yp * ws + xs] == INFECTED)
                    + u32::from(current[yp * ws + xp] == INFECTED);

                if infected_count > 0 {
                    let prob_safe = (1.0 - p.infection_rate).powi(infected_count as i32);
                    rng = xorshift64(rng);
                    let rand_val = (rng >> 33) as f32 / (u32::MAX >> 1) as f32;
                    if rand_val > prob_safe {
                        next_row[xs] = INFECTED;
                        count_i += 1;
                    } else {
                        next_row[xs] = SUSCEPTIBLE;
                        count_s += 1;
                    }
                } else {
                    next_row[xs] = SUSCEPTIBLE;
                    count_s += 1;
                }
            }
            INFECTED => {
                rng = xorshift64(rng);
                let rand_val = (rng >> 33) as f32 / (u32::MAX >> 1) as f32;
                if rand_val < p.recovery_rate {
                    next_row[xs] = RECOVERED;
                    count_r += 1;
                } else {
                    next_row[xs] = INFECTED;
                    count_i += 1;
                }
            }
            _ => {
                next_row[xs] = cell;
                count_r += 1;
            }
        }
    }

    (count_s, count_i, count_r, rng)
}

#[cfg(not(target_arch = "wasm32"))]
fn step_parallel(state: &mut SirState) {
    use rayon::prelude::*;

    let w = state.grid.width();
    let h = state.grid.height();
    let ws = w as usize;
    let infection_rate = state.infection_rate;
    let recovery_rate = state.recovery_rate;
    let global_seed = state.rng_state;
    let tick = state.tick;

    let (current, next) = state.grid.current_and_next_mut();

    let (total_s, total_i, total_r) = next
        .par_chunks_mut(ws)
        .enumerate()
        .map(|(y, next_row)| {
            let row_seed = global_seed ^ tick ^ (y as u64);
            let rng = xorshift64(row_seed.max(1));
            let p = RowParams {
                y: y as u32,
                w,
                h,
                infection_rate,
                recovery_rate,
            };
            let (s, i, r, _rng) = process_row(current, next_row, &p, rng);
            (s, i, r)
        })
        .reduce(|| (0, 0, 0), |a, b| (a.0 + b.0, a.1 + b.1, a.2 + b.2));

    state.rng_state = xorshift64(global_seed ^ tick);
    state.count_s = total_s;
    state.count_i = total_i;
    state.count_r = total_r;
}

#[cfg(target_arch = "wasm32")]
fn step_sequential(state: &mut SirState) {
    let w = state.grid.width();
    let h = state.grid.height();
    let ws = w as usize;
    let infection_rate = state.infection_rate;
    let recovery_rate = state.recovery_rate;
    let mut rng = state.rng_state;

    let mut total_s: u64 = 0;
    let mut total_i: u64 = 0;
    let mut total_r: u64 = 0;

    let (current, next) = state.grid.current_and_next_mut();

    for (y, next_row) in next.chunks_mut(ws).enumerate() {
        let p = RowParams {
            y: y as u32,
            w,
            h,
            infection_rate,
            recovery_rate,
        };
        let (s, i, r, new_rng) = process_row(current, next_row, &p, rng);
        total_s += s;
        total_i += i;
        total_r += r;
        rng = new_rng;
    }

    state.rng_state = rng;
    state.count_s = total_s;
    state.count_i = total_i;
    state.count_r = total_r;
}
