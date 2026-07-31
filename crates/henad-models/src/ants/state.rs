use henad_compute::scatter::{Combine, ScatterGrid};
use henad_core::grid::Grid2D;
use henad_core::helpers::{extract_f32, extract_u32, stat};
use henad_core::model::SimState;
use henad_core::params::ParamValue;
use henad_core::view::{GridView, PointView, StatEntry};

/// Below this a trail reads as zero, so it disappears instead of asymptoting.
pub(crate) const LOW_PHEROMONE: f32 = 1e-14;

/// Display window in decades. Trails fall off geometrically, so a linear ramp shows nothing but a
/// bright dot at the nest.
const DISPLAY_DECADES: f32 = 3.0;
const RAMP_STEPS: u8 = 6;

/// Site markers. Static for the whole run, so a bare `Vec<u8>` rather than a [`Grid2D`].
const EMPTY: u8 = 0;
pub(crate) const OBSTACLE: u8 = 1;
pub(crate) const FOOD: u8 = 2;
pub(crate) const HOME: u8 = 3;

/// No step taken yet, so momentum has nothing to continue.
pub(crate) const NO_STEP: u8 = u8::MAX;

/// Agents per rayon chunk. Fixed rather than derived from the thread count, since the RNG is
/// seeded per chunk and results must not depend on the machine.
pub(crate) const AGENT_CHUNK: usize = 4096;

/// Background, two trail ramps, then the site markers. The ramps differ by hue so route home and
/// route to food stay apart at a glance.
pub const CELL_PALETTE: [[u8; 4]; 16] = [
    [0x0E, 0x0E, 0x12, 0xFF], // 0  background
    [0x10, 0x1C, 0x30, 0xFF], // 1  to-home, faintest
    [0x12, 0x2A, 0x4C, 0xFF], // 2
    [0x14, 0x3C, 0x6E, 0xFF], // 3
    [0x16, 0x52, 0x96, 0xFF], // 4
    [0x1A, 0x6B, 0xC0, 0xFF], // 5
    [0x2E, 0x8B, 0xE8, 0xFF], // 6  to-home, strongest
    [0x30, 0x1E, 0x10, 0xFF], // 7  to-food, faintest
    [0x4A, 0x2C, 0x12, 0xFF], // 8
    [0x6C, 0x3E, 0x14, 0xFF], // 9
    [0x94, 0x54, 0x16, 0xFF], // 10
    [0xBE, 0x6E, 0x1A, 0xFF], // 11
    [0xE8, 0x8C, 0x2E, 0xFF], // 12 to-food, strongest
    [0x5A, 0x5A, 0x62, 0xFF], // 13 obstacle
    [0x3D, 0xD5, 0x8C, 0xFF], // 14 food source
    [0xF2, 0xE4, 0x5C, 0xFF], // 15 nest
];

/// Indexed by `has_food` itself, not by a copy of it.
pub const ANT_PALETTE: [[u8; 4]; 2] = [
    [0xE8, 0xE8, 0xF0, 0xFF], // searching
    [0x3D, 0xD5, 0x8C, 0xFF], // carrying food
];

/// Stat series colours.
pub const STAT_PALETTE: [[u8; 4]; 3] = [
    [0x3D, 0xD5, 0x8C, 0xFF], // carrying
    [0xF2, 0xE4, 0x5C, 0xFF], // deliveries
    [0x2E, 0x8B, 0xE8, 0xFF], // total pheromone
];

/// Ant foraging, ported from krABMaga's `antsforaging`.
///
/// Three semantic divergences a comparison has to state. Deposits combine with `max` rather than
/// last-writer-wins, since a parallel scatter needs a commutative combine and ants sharing a cell
/// carry different rewards. The pheromone field is all read old, all write new. The RNG is seeded
/// per chunk per tick rather than drawn per call.
pub struct AntsState {
    // --- agent lanes, SoA ---------------------------------------------------
    // Not double buffered. Ants never read one another, so every lane here is touched only by the
    // ant that owns the slot.
    pub(crate) pos_x: Vec<f32>,
    pub(crate) pos_y: Vec<f32>,
    /// Last direction, encoded `(dx + 1) * 3 + (dy + 1)`, or [`NO_STEP`].
    ///
    /// The delta rather than the previous position, since momentum only uses the difference.
    pub(crate) last_step: Vec<u8>,
    /// `0` searching, `1` carrying. Doubles as the render lane, so there is no colour lane.
    pub(crate) has_food: Vec<u8>,
    pub(crate) reward: Vec<f32>,

    // --- deposit lanes, handed to the scatter grid ---------------------------
    pub(crate) deposit_cell: Vec<u32>,
    /// An ant fills one of these and leaves the other at `0.0`, the identity for [`Combine::Max`].
    pub(crate) deposit_food: Vec<f32>,
    pub(crate) deposit_home: Vec<f32>,

    // --- fields --------------------------------------------------------------
    pub(crate) to_food: Grid2D<f32>,
    pub(crate) to_home: Grid2D<f32>,
    pub(crate) sites: Vec<u8>,
    pub(crate) scatter: ScatterGrid,

    /// `GridView::cells` is `&[u8]` but pheromone is `f32`, so the model owns the quantisation.
    pub(crate) display_cells: Vec<u8>,

    // --- scalars -------------------------------------------------------------
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) num_ants: u32,
    pub(crate) evaporation: f32,
    pub(crate) update_cutdown: f32,
    pub(crate) reward_value: f32,
    pub(crate) momentum_probability: f32,
    pub(crate) random_action_probability: f32,
    pub(crate) deliveries: u64,
    pub(crate) tick: u64,
    pub(crate) rng_seed: u64,
}

impl AntsState {
    pub fn from_params(params: &[ParamValue]) -> Self {
        let width = extract_u32(params, 0, 200).max(8);
        let height = extract_u32(params, 1, 200).max(8);
        let num_ants = extract_u32(params, 2, 2_000);
        let n_cells = (width as usize) * (height as usize);
        let n = num_ants as usize;

        let mut state = Self {
            pos_x: vec![0.0; n],
            pos_y: vec![0.0; n],
            last_step: vec![NO_STEP; n],
            has_food: vec![0; n],
            reward: vec![0.0; n],
            deposit_cell: vec![0; n],
            deposit_food: vec![0.0; n],
            deposit_home: vec![0.0; n],
            to_food: Grid2D::new(width, height),
            to_home: Grid2D::new(width, height),
            sites: vec![EMPTY; n_cells],
            // Shared by both fields. Same dimensions, same combine, and the calls are sequential.
            scatter: ScatterGrid::new(n_cells, Combine::Max),
            display_cells: vec![0; n_cells],
            width,
            height,
            num_ants,
            evaporation: extract_f32(params, 3, 0.999),
            update_cutdown: extract_f32(params, 4, 0.9),
            reward_value: extract_f32(params, 5, 1.0),
            momentum_probability: extract_f32(params, 6, 0.8),
            random_action_probability: extract_f32(params, 7, 0.1),
            deliveries: 0,
            tick: 0,
            rng_seed: 0xA175_F01A_6ED5_0001,
        };

        state.build_sites();
        state.spawn_at_nest();
        state.quantise_for_display();
        state
    }

    /// Nest, food source and the two obstacle blobs.
    ///
    /// Placed proportionally so the grid stays a parameter. At 200x200 this matches the reference,
    /// which hard-codes them.
    fn build_sites(&mut self) {
        let (w, h) = (self.width as f32, self.height as f32);
        // The reference's ellipse constant is calibrated to a 200 wide field.
        let size = 0.407 * (200.0 / w);
        let blob = |x: f32, y: f32, cx: f32, cy: f32| -> bool {
            let a = (x - cx) * size + (y - cy) * size;
            let b = (x - cx) * size - (y - cy) * size;
            a * a / 36.0 + b * b / 1024.0 <= 1.0
        };

        for j in 0..self.height {
            for i in 0..self.width {
                let (x, y) = (i as f32, j as f32);
                if blob(x, y, 0.500 * w, 0.725 * h) || blob(x, y, 0.450 * w, 0.275 * h) {
                    self.sites[(j * self.width + i) as usize] = OBSTACLE;
                }
            }
        }

        // Placed after the blobs so a site is never buried under an obstacle.
        let (food, nest) = (self.food_cell(), self.nest_cell());
        self.sites[food] = FOOD;
        self.sites[nest] = HOME;
    }

    pub(crate) fn nest_cell(&self) -> usize {
        let x = (0.875 * self.width as f32) as u32;
        let y = (0.875 * self.height as f32) as u32;
        (y * self.width + x) as usize
    }

    pub(crate) fn food_cell(&self) -> usize {
        let x = (0.125 * self.width as f32) as u32;
        let y = (0.125 * self.height as f32) as u32;
        (y * self.width + x) as usize
    }

    /// Ants start holding `reward` so they lay home pheromone immediately and the colony has a
    /// gradient to navigate back along.
    fn spawn_at_nest(&mut self) {
        let nest = self.nest_cell() as u32;
        let (x, y) = ((nest % self.width) as f32, (nest / self.width) as f32);
        for i in 0..self.num_ants as usize {
            self.pos_x[i] = x;
            self.pos_y[i] = y;
            self.reward[i] = self.reward_value;
        }
    }

    /// Rebuilds `display_cells` from the two f32 fields.
    ///
    /// Every tick rather than every snapshot, since `SimState` has no pre-snapshot hook and
    /// `grid_view` only gets `&self`. Wasted work once `ticks_per_snapshot` is above one.
    pub(crate) fn quantise_for_display(&mut self) {
        let Self {
            to_food,
            to_home,
            sites,
            display_cells,
            ..
        } = self;
        let (to_food, to_home) = (to_food.current(), to_home.current());

        let quantise = |c: usize, out: &mut u8| {
            *out = match sites[c] {
                OBSTACLE => 13,
                FOOD => 14,
                HOME => 15,
                _ => {
                    // Stronger route wins the cell, so overlapping trails stay legible.
                    let (food, home) = (to_food[c], to_home[c]);
                    let (v, base) = if food > home { (food, 6) } else { (home, 0) };
                    match ramp_step(v) {
                        0 => 0,
                        step => base + step,
                    }
                }
            };
        };

        #[cfg(not(target_arch = "wasm32"))]
        {
            use rayon::prelude::*;
            display_cells
                .par_iter_mut()
                .enumerate()
                .for_each(|(c, out)| quantise(c, out));
        }
        #[cfg(target_arch = "wasm32")]
        for (c, out) in display_cells.iter_mut().enumerate() {
            quantise(c, out);
        }
    }
}

/// Log scaled strength in `0..=RAMP_STEPS`, where 0 means not worth drawing.
fn ramp_step(v: f32) -> u8 {
    if v <= LOW_PHEROMONE {
        return 0;
    }
    // Peak pheromone sits at roughly `reward`, so the window is the decades below 1.0.
    let decades = v.log10() / DISPLAY_DECADES + 1.0;
    if decades <= 0.0 {
        return 0;
    }
    ((decades * f32::from(RAMP_STEPS)) as u8).clamp(1, RAMP_STEPS)
}

impl SimState for AntsState {
    fn step(&mut self) {
        super::step::step(self);
    }

    fn tick(&self) -> u64 {
        self.tick
    }

    fn grid_view(&self) -> Option<GridView<'_>> {
        Some(GridView {
            width: self.width,
            height: self.height,
            cells: &self.display_cells,
            palette: &CELL_PALETTE,
        })
    }

    fn point_view(&self) -> Option<PointView<'_>> {
        Some(PointView {
            pos_x: &self.pos_x,
            pos_y: &self.pos_y,
            // Both layers stretch to the same rect, and nothing checks that across the crate
            // boundary, so the agent world has to be the grid extent in cells.
            world_w: self.width as f32,
            world_h: self.height as f32,
            // Already a 0/1 palette index, so carrying vs searching costs no extra lane.
            color: Some(&self.has_food),
            palette: &ANT_PALETTE,
        })
    }

    fn stats(&self) -> Vec<StatEntry> {
        let carrying = self.has_food.iter().filter(|&&f| f != 0).count();
        let pheromone = super::step::total_pheromone(&self.to_food, &self.to_home);
        vec![
            stat("Carrying Food", carrying as f64, STAT_PALETTE[0]),
            stat("Deliveries", self.deliveries as f64, STAT_PALETTE[1]),
            stat("Total Pheromone", pheromone, STAT_PALETTE[2]),
        ]
    }

    fn set_param(&mut self, index: usize, value: &ParamValue) -> bool {
        match (index, value) {
            (3, ParamValue::F32(v)) => {
                self.evaporation = *v;
                true
            }
            (4, ParamValue::F32(v)) => {
                self.update_cutdown = *v;
                true
            }
            (5, ParamValue::F32(v)) => {
                self.reward_value = *v;
                true
            }
            (6, ParamValue::F32(v)) => {
                self.momentum_probability = *v;
                true
            }
            (7, ParamValue::F32(v)) => {
                self.random_action_probability = *v;
                true
            }
            _ => false,
        }
    }

    fn population(&self) -> u64 {
        u64::from(self.num_ants)
    }

    fn heap_bytes(&self) -> usize {
        let agents = self.pos_x.capacity() * 4 * 4 // pos_x, pos_y, reward, deposit_cell
            + self.pos_x.capacity() * 2 // last_step, has_food
            + self.deposit_food.capacity() * 8; // deposit_food, deposit_home
        agents
            + self.to_food.heap_bytes()
            + self.to_home.heap_bytes()
            + self.sites.capacity()
            + self.display_cells.capacity()
            + self.scatter.heap_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_state() -> AntsState {
        AntsState::from_params(&[ParamValue::U32(200), ParamValue::U32(200), ParamValue::U32(500)])
    }

    /// Proportional placement has to land exactly where the reference hard-codes these.
    #[test]
    fn sites_match_the_reference_layout_at_200_squared() {
        let state = default_state();
        assert_eq!(state.nest_cell(), 175 * 200 + 175, "nest");
        assert_eq!(state.food_cell(), 25 * 200 + 25, "food");
        assert_eq!(state.sites[state.nest_cell()], HOME);
        assert_eq!(state.sites[state.food_cell()], FOOD);
        assert!(
            state.sites.contains(&OBSTACLE),
            "the two obstacle blobs should cover some cells"
        );
    }

    #[test]
    fn every_ant_starts_on_the_nest_holding_a_reward() {
        let state = default_state();
        let nest = state.nest_cell() as u32;
        let (nx, ny) = ((nest % state.width) as f32, (nest / state.width) as f32);
        for i in 0..state.num_ants as usize {
            assert_eq!((state.pos_x[i], state.pos_y[i]), (nx, ny), "ant {i} is not on the nest");
            assert_eq!(state.reward[i], state.reward_value, "ant {i} has no reward to spend");
        }
    }

    /// Every quantised value must land inside the palette, or the renderer silently draws entry 0.
    #[test]
    fn every_display_cell_indexes_the_palette() {
        let mut state = default_state();
        for _ in 0..50 {
            state.step();
        }
        state.quantise_for_display();
        for (c, &cell) in state.display_cells.iter().enumerate() {
            assert!(
                (cell as usize) < CELL_PALETTE.len(),
                "cell {c} quantised to {cell}, past the palette"
            );
        }
    }

    /// Forgetting the refresh in `step` leaves the grid layer frozen at construction, with sites
    /// still rendering and pheromone never appearing. It was wrong that way first.
    #[test]
    fn the_grid_layer_shows_pheromone_laid_since_construction() {
        let mut state = default_state();
        assert!(
            !state.display_cells.iter().any(|&c| (1..=12).contains(&c)),
            "no trail should exist before the first tick"
        );

        for _ in 0..100 {
            state.step();
        }

        let trail = state.display_cells.iter().filter(|&&c| (1..=12).contains(&c)).count();
        assert!(
            trail > 0,
            "ants have been depositing for 100 ticks but the grid layer shows no trail at all"
        );
    }

    /// Handed to the renderer directly, so it may only ever hold a valid palette index.
    #[test]
    fn has_food_stays_a_valid_palette_index() {
        let mut state = default_state();
        for _ in 0..100 {
            state.step();
        }
        assert!(
            state.has_food.iter().all(|&f| (f as usize) < ANT_PALETTE.len()),
            "has_food is doubling as the render lane, so it may only hold 0 or 1"
        );
    }

    #[test]
    fn ants_stay_inside_the_bounded_field() {
        let mut state = default_state();
        for tick in 0..200 {
            state.step();
            for i in 0..state.num_ants as usize {
                let (x, y) = (state.pos_x[i], state.pos_y[i]);
                assert!(
                    x >= 0.0 && x < state.width as f32 && y >= 0.0 && y < state.height as f32,
                    "ant {i} left the field at ({x}, {y}) on tick {tick}; the reference is bounded, not toroidal"
                );
            }
        }
    }

    /// Three things could leak scheduling into the result. The scatter arm comes from the worker
    /// count, the movement RNG is seeded per chunk, and deliveries are a parallel reduction.
    ///
    /// One worker also stands in for wasm, where the shadow arm reduces through a single grid.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn results_do_not_depend_on_the_thread_count() {
        /// Ant cells, deliveries, and both pheromone fields as raw bits.
        fn run(threads: usize) -> (Vec<u32>, u64, Vec<u32>) {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .expect("rayon pool");
            pool.install(|| {
                let mut state = default_state();
                for _ in 0..200 {
                    state.step();
                }
                let cells = state
                    .pos_x
                    .iter()
                    .zip(&state.pos_y)
                    .map(|(&x, &y)| y as u32 * state.width + x as u32)
                    .collect();
                let field = state
                    .to_home
                    .current()
                    .iter()
                    .chain(state.to_food.current())
                    .map(|v| v.to_bits())
                    .collect();
                (cells, state.deliveries, field)
            })
        }

        let (cells_1, deliveries_1, field_1) = run(1);
        let (cells_n, deliveries_n, field_n) = run(7);
        assert_eq!(cells_1, cells_n, "ant positions depend on the thread count");
        assert_eq!(deliveries_1, deliveries_n, "delivery count depends on the thread count");
        assert_eq!(
            field_1, field_n,
            "pheromone field is not bit-identical across thread counts"
        );
    }

    /// The momentum and random action fallbacks are the easy ones to forget an obstacle check in.
    #[test]
    fn ants_never_enter_an_obstacle() {
        let mut state = default_state();
        for tick in 0..200 {
            state.step();
            for i in 0..state.num_ants as usize {
                let c = (state.pos_y[i] as u32 * state.width + state.pos_x[i] as u32) as usize;
                assert_ne!(state.sites[c], OBSTACLE, "ant {i} is inside an obstacle on tick {tick}");
            }
        }
    }
}
