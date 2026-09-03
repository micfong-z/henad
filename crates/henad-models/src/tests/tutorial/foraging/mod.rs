//! Ant foraging as `docs/guide/first-model/ants.md` builds it.
//!
//! The id is `foraging` rather than `ants`, since the shipped model already holds that one and the
//! page tells a reader the same thing.

pub mod field;

use henad_compute::agent_lanes;
use henad_compute::cpu::field::scalar::{Deposits, ScalarField, ScalarRead};
use henad_compute::cpu::primitives::chunked::{STATS_CHUNK, reduce_chunks};
use henad_compute::for_each_chunk_mut;
use henad_core::authoring::model::agent_model::{AgentModel, NoIndex, StepCtx};
use henad_core::authoring::model::field::Extent;
use henad_core::authoring::primitives::rng::{choice3, next_bits, next_float, reservoir_accept};
use henad_core::authoring::primitives::space::{Boundary, MOORE_COLUMN_MAJOR, cell_index, offset_cell};
use henad_core::grid::Grid2D;
use henad_core::helpers::{extract_f32, f32_param};
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::view::{StatDescriptor, StatValue};

use self::field::{FOOD, HOME, OBSTACLE, PheromoneField, TO_FOOD, TO_HOME, nest_cell};

/// No step taken yet, so momentum has nothing to continue.
pub const NO_STEP: u8 = u8::MAX;

agent_lanes! {
    pub struct AntLanes {
        read AntRead;
        chunk AntChunk;
        plain pos_x: f32 = 0.0,
        plain pos_y: f32 = 0.0,
        /// Last direction, encoded `(dx + 1) * 3 + (dy + 1)`, or [`NO_STEP`].
        plain last_step: u8 = NO_STEP,
        /// `0` searching, `1` carrying. Doubles as the render lane.
        plain has_food: u8 = 0,
        plain reward: f32 = 0.0,
    }
    color = has_food;
}

pub const ANT_PALETTE: [[u8; 4]; 2] = [
    [0xE8, 0xE8, 0xF0, 0xFF], // searching
    [0x3D, 0xD5, 0x8C, 0xFF], // carrying food
];

pub const STAT_PALETTE: [[u8; 4]; 3] = [
    [0x3D, 0xD5, 0x8C, 0xFF], // carrying
    [0xF2, 0xE4, 0x5C, 0xFF], // deliveries
    [0x2E, 0x8B, 0xE8, 0xFF], // total pheromone
];

henad_core::params! {
    const UPDATE_CUTDOWN = f32_param("update_cutdown", "Trail Falloff", 0.9, 0.5, 1.0, Some(0.01));
    const REWARD = f32_param("reward", "Site Reward", 1.0, 0.1, 10.0, Some(0.1));
    const MOMENTUM = f32_param("momentum", "Momentum Probability", 0.8, 0.0, 1.0, Some(0.01));
    const RANDOM_ACTION = f32_param("random_action", "Random Action Probability", 0.1, 0.0, 1.0, Some(0.01));
}

pub struct ForagingModel;

pub struct AntParams {
    pub w: i32,
    pub h: i32,
    pub cutdown: f32,
    /// Cutdown raised to the diagonal distance, since those neighbours are further away.
    pub diagonal: f32,
    pub reward: f32,
    pub momentum: f32,
    pub random_action: f32,
}

impl AgentModel for ForagingModel {
    const NAME: &'static str = "Ant Foraging";
    const ID: &'static str = "foraging";
    const DESCRIPTION: &'static str =
        "Ants lay and follow pheromone trails between a nest and a food source, around obstacles";
    const PALETTE: &'static [[u8; 4]] = &ANT_PALETTE;
    const STATS: &'static [StatDescriptor] = &[
        StatDescriptor::new("Carrying Food", STAT_PALETTE[0]),
        StatDescriptor::new("Deliveries", STAT_PALETTE[1]),
        StatDescriptor::new("Total Pheromone", STAT_PALETTE[2]),
    ];
    const CHUNK: usize = 4096;
    const DEFAULT_AGENTS: u32 = 2_000;
    const MAX_AGENTS: u32 = 5_000_000;
    const DEFAULT_EXTENT: Extent = Extent { w: 200.0, h: 200.0 };

    type Lanes = AntLanes;
    type Field = ScalarField<PheromoneField>;
    type Index = NoIndex;
    type Params = AntParams;
    type Tally = u64;

    fn param_descriptors() -> Vec<ParamDescriptor> {
        descriptors()
    }

    fn from_params(params: &[ParamValue], extent: Extent) -> AntParams {
        let cutdown = extract_f32(params, UPDATE_CUTDOWN, 0.9);
        AntParams {
            w: extent.w as i32,
            h: extent.h as i32,
            cutdown,
            diagonal: cutdown.powf(std::f32::consts::SQRT_2),
            reward: extract_f32(params, REWARD, 1.0),
            momentum: extract_f32(params, MOMENTUM, 0.8),
            random_action: extract_f32(params, RANDOM_ACTION, 0.1),
        }
    }

    fn init(lanes: &mut AntLanes, extent: Extent, params: &[ParamValue], _rng: &mut u64) {
        let (width, height) = extent.cells();
        let nest = nest_cell(width, height) as u32;
        let (x, y) = ((nest % width) as f32, (nest / width) as f32);
        let reward = extract_f32(params, REWARD, 1.0);
        for i in 0..lanes.pos_x.len() {
            lanes.pos_x[i] = x;
            lanes.pos_y[i] = y;
            lanes.reward[i] = reward;
        }
    }

    fn run_deposit_pass(lanes: &AntLanes, deposits: &mut Deposits, ctx: &StepCtx<'_, Self>) {
        deposit(lanes, deposits, ctx);
    }

    fn run_step_pass(lanes: &mut AntLanes, ctx: &StepCtx<'_, Self>, seed: u64, tick: u64) -> u64 {
        advect(lanes, ctx, seed, tick)
    }

    fn stats(lanes: &AntLanes, field: &ScalarField<PheromoneField>, tally: &u64) -> Vec<StatValue> {
        let carrying = lanes.has_food.iter().filter(|&&f| f != 0).count();
        vec![
            StatValue::Scalar(carrying as f64),
            StatValue::Scalar(*tally as f64),
            StatValue::Scalar(total_pheromone(field.field(TO_FOOD), field.field(TO_HOME))),
        ]
    }
}

// --- Pass one, deposit ---

/// Largest pheromone in the 3x3 neighbourhood, cut down by distance and lifted by the reward.
#[inline]
fn deposit_value(x: i32, y: i32, reward: f32, field: &[f32], p: &AntParams) -> f32 {
    let here = field[cell_index(x as u32, y as u32, p.w as u32) as usize];
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

fn deposit(lanes: &AntLanes, deposits: &mut Deposits, ctx: &StepCtx<'_, ForagingModel>) {
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
        ForagingModel::CHUNK,
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

// --- Pass two, move ---

#[inline]
fn encode_step(dx: i32, dy: i32) -> u8 {
    ((dx + 1) * 3 + (dy + 1)) as u8
}

#[inline]
fn decode_step(s: u8) -> (i32, i32) {
    let s = i32::from(s);
    (s / 3 - 1, s % 3 - 1)
}

/// Inside the field and not an obstacle. This model is bounded, not toroidal.
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
    // 2 not 1 reproduces the reference's off-by-one, giving the first neighbour visited 2/(k+1)
    // against 1/(k+1) for the rest.
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
fn advect(lanes: &mut AntLanes, ctx: &StepCtx<'_, ForagingModel>, seed: u64, tick: u64) -> u64 {
    let p = ctx.params;
    let field = ctx.field;
    lanes.run_pass(
        ForagingModel::CHUNK,
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

// --- Statistics ---

fn total_pheromone(to_food: &Grid2D<f32>, to_home: &Grid2D<f32>) -> f64 {
    field_sum(to_food.current()) + field_sum(to_home.current())
}

fn field_sum(cells: &[f32]) -> f64 {
    reduce_chunks(
        cells.len(),
        STATS_CHUNK,
        |r| cells[r].iter().map(|&v| f64::from(v)).sum::<f64>(),
        |a, b| a + b,
        0.0,
    )
}

#[test]
fn results_do_not_depend_on_the_thread_count() {
    use henad_compute::cpu::agent_engine::AgentModelState;
    use henad_core::model::SimState as _;

    fn run(threads: usize) -> Vec<u32> {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .expect("rayon pool");
        pool.install(|| {
            let mut state = AgentModelState::<ForagingModel>::from_params(&[
                ParamValue::U32(500),
                ParamValue::F32(200.0),
                ParamValue::F32(200.0),
            ]);
            for _ in 0..200 {
                state.step();
            }
            let lanes = state.lanes();
            lanes
                .pos_x
                .iter()
                .zip(&lanes.pos_y)
                .map(|(&x, &y)| y as u32 * 200 + x as u32)
                .collect()
        })
    }

    assert_eq!(run(1), run(7), "ant positions depend on the thread count");
}
