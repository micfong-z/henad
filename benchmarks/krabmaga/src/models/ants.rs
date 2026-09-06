//! Ant foraging on a bounded lattice over two pheromone layers.
//!
//! Ants lay a trail for the trip they are making and follow the trail for the trip they are not, so
//! a colony that finds food leaves a path back to it. Written from Henad's declaration, which
//! differs from the krABMaga `antsforaging` example it descends from. Deposits combine with `max`
//! rather than last-writer-wins, and the whole field is read before any of it is written.
//!
//! Shape follows `antsforaging`: one agent per ant, depositing and then stepping, over
//! `DenseNumberGrid2D` layers. Deposits land in each layer's write buffer, combined with `max`
//! against whatever is already staged there, and `State::after_step` folds them into the read
//! values and decays the result once every ant has run.

use core::fmt;
use std::any::Any;
use std::sync::atomic::{AtomicU64, Ordering};

use krabmaga::engine::agent::Agent;
use krabmaga::engine::fields::dense_number_grid_2d::DenseNumberGrid2D;
use krabmaga::engine::fields::field::Field;
use krabmaga::engine::location::Int2D;
use krabmaga::engine::schedule::Schedule;
use krabmaga::engine::state::State;
use krabmaga::rand::{Rng, SeedableRng};
use krabmaga::rand_pcg::Pcg64;

const EMPTY: u8 = 0;
const OBSTACLE: u8 = 1;
const FOOD: u8 = 2;
const HOME: u8 = 3;

/// Below this a trail reads as zero, so it disappears instead of asymptoting.
const LOW_PHEROMONE: f32 = 1e-14;
pub const NO_STEP: u8 = 255;

/// `dx` outer, `dy` inner. A tie between two equally good neighbours is broken from the visit
/// order, so this cannot be reordered without changing where the ants go.
const MOORE_COLUMN_MAJOR: [(i32, i32); 8] = [(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)];

fn encode_step(dx: i32, dy: i32) -> u8 {
    ((dx + 1) * 3 + (dy + 1)) as u8
}

fn decode_step(code: u8) -> (i32, i32) {
    (i32::from(code) / 3 - 1, i32::from(code) % 3 - 1)
}

#[derive(Clone)]
pub struct Ant {
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub has_food: u8,
    pub reward: f32,
    pub last_step: u8,
    rng: Pcg64,
}

impl fmt::Display for Ant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {} {}", self.id, self.x, self.y)
    }
}

impl Ant {
    /// The value this ant lays, largest over its own cell and the eight around it.
    ///
    /// Floored at what the cell already holds, which is why combining with `max` reproduces the
    /// plain overwrite of the model this descends from.
    fn deposit(&self, model: &Ants) {
        let field = model.trail(self.has_food != 0);
        let here = Int2D { x: self.x, y: self.y };
        let value = field.get_value(&here).unwrap_or(0.0);
        let mut best = value.max(value * model.cutdown + self.reward);

        for (dx, dy) in MOORE_COLUMN_MAJOR {
            let (nx, ny) = (self.x + dx, self.y + dy);
            if !model.inside(nx, ny) {
                continue;
            }
            let cut = if dx != 0 && dy != 0 {
                model.diagonal
            } else {
                model.cutdown
            };
            let neighbor = field.get_value(&Int2D { x: nx, y: ny }).unwrap_or(0.0);
            best = best.max(neighbor * cut + self.reward);
        }

        // The write buffer is the deposit staging area, so combining is a read of what is already
        // staged. Every cell is rewritten in `after_step` before the buffers swap.
        let staged = field.get_value_unbuffered(&here).unwrap_or(0.0);
        if best > staged {
            field.set_value_location(best, &here);
        }
    }

    fn advect(&mut self, model: &Ants) {
        // Ants follow the trip they are not currently making.
        let trail = model.trail(self.has_food == 0);
        let (x, y) = (self.x, self.y);

        let mut best = -1.0f32;
        let mut target = (x, y);
        // 2 rather than 1 is the reference's off-by-one, which gives the first neighbour visited
        // twice the chance of the rest. Reproduced so the ports stay the same simulation.
        let mut count = 2u32;
        for (dx, dy) in MOORE_COLUMN_MAJOR {
            let (nx, ny) = (x + dx, y + dy);
            if !model.passable(nx, ny) {
                continue;
            }
            let value = trail.get_value(&Int2D { x: nx, y: ny }).unwrap_or(0.0);
            if value > best {
                count = 2;
            }
            if value > best || (value == best && self.rng.random::<f64>() < 1.0 / f64::from(count)) {
                best = value;
                target = (nx, ny);
            }
            count += 1;
        }

        if best == 0.0 && self.last_step != NO_STEP {
            if self.rng.random::<f64>() < model.momentum {
                let (dx, dy) = decode_step(self.last_step);
                if model.passable(x + dx, y + dy) {
                    target = (x + dx, y + dy);
                }
            }
        } else if self.rng.random::<f64>() < model.random_action {
            let dx = self.rng.random_range(-1..=1);
            let dy = self.rng.random_range(-1..=1);
            if (dx != 0 || dy != 0) && model.passable(x + dx, y + dy) {
                target = (x + dx, y + dy);
            }
        }

        self.last_step = encode_step(target.0 - x, target.1 - y);
        // The deposit pass spent whatever the ant was carrying, only a site grants more.
        self.reward = 0.0;
        match model.site(target.0, target.1) {
            HOME if self.has_food != 0 => {
                self.reward = model.reward;
                self.has_food = 0;
                model.deliveries.fetch_add(1, Ordering::Relaxed);
            }
            FOOD if self.has_food == 0 => {
                self.reward = model.reward;
                self.has_food = 1;
            }
            _ => {}
        }
        self.x = target.0;
        self.y = target.1;
    }
}

impl Agent for Ant {
    /// Deposit then move.
    ///
    /// Both passes read the published field and never each other's writes, so running them ant by
    /// ant reaches the same state as running each pass over the whole colony.
    fn step(&mut self, state: &mut dyn State) {
        let model = state.as_any().downcast_ref::<Ants>().expect("ants state");
        self.deposit(model);
        self.advect(model);
    }
}

pub struct Ants {
    pub width: i32,
    pub height: i32,
    to_food: DenseNumberGrid2D<f32>,
    to_home: DenseNumberGrid2D<f32>,
    sites: Vec<u8>,
    cutdown: f32,
    /// Cutdown raised to the diagonal distance, since those neighbours are further away.
    diagonal: f32,
    reward: f32,
    momentum: f64,
    random_action: f64,
    evaporation: f32,
    num_agents: u32,
    /// An exact `(x, y, last_step, has_food, reward)` list for a gate scenario.
    agents: Option<Vec<(i32, i32, u8, u8, f32)>>,
    /// Seeds both layers from the gate's formula instead of leaving them empty.
    seeded_field: bool,
    seed: u64,
    /// Counted for the same reason Henad and the other four ports count it, so the timed step does
    /// the same work everywhere. Nothing reads it.
    deliveries: AtomicU64,
}

#[allow(clippy::too_many_arguments)]
impl Ants {
    pub fn new(
        num_agents: u32,
        world_w: f32,
        world_h: f32,
        cutdown: f32,
        reward: f32,
        momentum: f64,
        random_action: f64,
        evaporation: f32,
        seed: u64,
    ) -> Self {
        let width = (world_w as i32).max(1);
        let height = (world_h as i32).max(1);
        Ants {
            width,
            height,
            to_food: DenseNumberGrid2D::new(width, height),
            to_home: DenseNumberGrid2D::new(width, height),
            sites: build_sites(width, height),
            cutdown,
            diagonal: cutdown.powf(std::f32::consts::SQRT_2),
            reward,
            momentum,
            random_action,
            evaporation,
            num_agents,
            agents: None,
            seeded_field: false,
            deliveries: AtomicU64::new(0),
            seed,
        }
    }

    pub fn with_gate_start(mut self, agents: &[(i32, i32, u8, u8, f32)]) -> Self {
        self.num_agents = agents.len() as u32;
        self.agents = Some(agents.to_vec());
        self.seeded_field = true;
        self
    }

    pub fn population(&self) -> u64 {
        u64::from(self.num_agents)
    }

    fn trail(&self, to_food: bool) -> &DenseNumberGrid2D<f32> {
        if to_food {
            &self.to_food
        } else {
            &self.to_home
        }
    }

    fn inside(&self, x: i32, y: i32) -> bool {
        x >= 0 && x < self.width && y >= 0 && y < self.height
    }

    fn passable(&self, x: i32, y: i32) -> bool {
        self.inside(x, y) && self.site(x, y) != OBSTACLE
    }

    fn site(&self, x: i32, y: i32) -> u8 {
        self.sites[(x * self.height + y) as usize]
    }

    /// One layer as the fixture wants it, ascending in x within a row and rows ascending in y.
    ///
    /// Before the first step the layers still sit in the write buffer, since `Schedule::step` is
    /// what publishes them.
    pub fn layer(&self, to_food: bool) -> Vec<Vec<f32>> {
        let field = self.trail(to_food);
        (0..self.height)
            .map(|y| {
                (0..self.width)
                    .map(|x| {
                        let loc = Int2D { x, y };
                        field
                            .get_value(&loc)
                            .or_else(|| field.get_value_unbuffered(&loc))
                            .unwrap_or(0.0)
                    })
                    .collect()
            })
            .collect()
    }
}

impl State for Ants {
    fn init(&mut self, schedule: &mut Schedule) {
        for x in 0..self.width {
            for y in 0..self.height {
                let loc = Int2D { x, y };
                let (food, home) = if self.seeded_field {
                    (
                        crate::scenarios::ants_field(x, y, true),
                        crate::scenarios::ants_field(x, y, false),
                    )
                } else {
                    (0.0, 0.0)
                };
                self.to_food.set_value_location(food, &loc);
                self.to_home.set_value_location(home, &loc);
            }
        }

        let nest = ((0.875 * self.width as f32) as i32, (0.875 * self.height as f32) as i32);
        for id in 0..self.num_agents {
            // Every ant starts on the nest holding one reward, so the colony lays home pheromone
            // from the first tick.
            let (x, y, last_step, has_food, reward) = match &self.agents {
                Some(agents) => agents[id as usize],
                None => (nest.0, nest.1, NO_STEP, 0, self.reward),
            };
            schedule.schedule_repeating(
                Box::new(Ant {
                    id,
                    x,
                    y,
                    has_food,
                    reward,
                    last_step,
                    rng: Pcg64::seed_from_u64(self.seed ^ (u64::from(id) << 32)),
                }),
                0.0,
                0,
            );
        }
    }

    /// Fold the staged deposits into the published field, then decay.
    fn after_step(&mut self, _schedule: &mut Schedule) {
        for field in [&self.to_food, &self.to_home] {
            for x in 0..self.width {
                for y in 0..self.height {
                    let loc = Int2D { x, y };
                    let held = field.get_value(&loc).unwrap_or(0.0);
                    let deposit = field.get_value_unbuffered(&loc).unwrap_or(0.0);
                    let mut value = held.max(deposit) * self.evaporation;
                    if value < LOW_PHEROMONE {
                        value = 0.0;
                    }
                    field.set_value_location(value, &loc);
                }
            }
        }
    }

    fn update(&mut self, _step: u64) {
        self.to_food.lazy_update();
        self.to_home.lazy_update();
    }

    fn reset(&mut self) {
        self.to_food = DenseNumberGrid2D::new(self.width, self.height);
        self.to_home = DenseNumberGrid2D::new(self.width, self.height);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn as_state(&self) -> &dyn State {
        self
    }

    fn as_state_mut(&mut self) -> &mut dyn State {
        self
    }
}

/// Nest, food source and the two obstacle blobs, placed proportionally.
///
/// At 200 by 200 this is where the model Henad descends from hard-codes them.
fn build_sites(width: i32, height: i32) -> Vec<u8> {
    let mut sites = vec![EMPTY; (width * height) as usize];
    let (w, h) = (f64::from(width), f64::from(height));
    // The reference's ellipse constant is calibrated to a 200 wide field.
    let size = 0.407 * (200.0 / w);
    // Double precision and this grouping, as the declaration writes it.
    let blob = |x: f64, y: f64, cx: f64, cy: f64| {
        let a = ((x - cx) + (y - cy)) * size;
        let b = ((x - cx) - (y - cy)) * size;
        a * a / 36.0 + b * b / 1024.0 <= 1.0
    };

    for x in 0..width {
        for y in 0..height {
            let (fx, fy) = (f64::from(x), f64::from(y));
            if blob(fx, fy, 0.500 * w, 0.725 * h) || blob(fx, fy, 0.450 * w, 0.275 * h) {
                sites[(x * height + y) as usize] = OBSTACLE;
            }
        }
    }

    // After the blobs, so neither site is buried.
    let food = ((0.125 * w) as i32, (0.125 * h) as i32);
    let nest = ((0.875 * w) as i32, (0.875 * h) as i32);
    sites[(food.0 * height + food.1) as usize] = FOOD;
    sites[(nest.0 * height + nest.1) as usize] = HOME;
    sites
}

/// `x y last_step has_food reward` per agent, in creation order.
///
/// The schedule hands its agents back in hash order, hence the sort.
pub fn rows(schedule: &Schedule) -> Vec<(i32, i32, u8, u8, f32)> {
    let mut ants: Vec<Ant> = schedule
        .get_all_events()
        .iter()
        .filter_map(|agent| agent.downcast_ref::<Ant>().cloned())
        .collect();
    ants.sort_by_key(|ant| ant.id);
    ants.iter()
        .map(|ant| (ant.x, ant.y, ant.last_step, ant.has_food, ant.reward))
        .collect()
}
