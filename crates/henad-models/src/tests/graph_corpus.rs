//! The grid graph corpus held to the stock models at tick 0: every graph file's init, run by henad-graph's host
//! evaluator, equals the stock model's seed, and its schema hashes like the stock model's under the stock id. Every
//! file also fits the work budgets at its largest size, and its entry passes the testing kit.
//!
//! The corpus is henad-graph's starters and the exact ports in `tests/fixtures/graphs`. The Vote oracle is a copy of
//! the template's `vote.rs`, since the template sits outside the workspace.

use henad_compute::cpu::grid_engine::{GRID_PARAM_BASE, grid_init_rng};
use henad_compute::cpu::primitives::chunked::{STATS_CHUNK, reduce_chunks};
use henad_compute::gpu::{Demand, HOST_WORK_BUDGET, PassKind, SUBMISSION_WORK_BUDGET};
use henad_core::action::ActionDescriptor;
use henad_core::authoring::model::gpu_grid_model::GpuGridModel as _;
use henad_core::authoring::model::grid_model::GridModel;
use henad_core::authoring::primitives::rng::{below, next_bits};
use henad_core::explore::fingerprint::schema_hash;
use henad_core::explore::plan::ModelSchema;
use henad_core::grid::Grid2D;
use henad_core::helpers::{extract_f32, f32_param, u32_param};
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::topology::NeighborhoodKind;
use henad_core::view::{StatDescriptor, StatValue};
use henad_graph::GraphFile;
use henad_graph::document::{PassStage, World};
use henad_graph::host::{HostEvaluator, HostWorld};

use super::support::{assert_entries_conform, graph_corpus, model_entry, roomy_limits, unpacked_life_cells};
use crate::gpu_game_of_life::GpuGameOfLife;
use crate::gpu_sir::GpuSir;

/// The template's Vote model, copied from `templates/model-project/src/vote.rs`.
pub(super) mod vote {
    use super::{
        Grid2D, GridModel, NeighborhoodKind, ParamDescriptor, ParamValue, STATS_CHUNK, StatDescriptor, StatValue,
        below, extract_f32, f32_param, next_bits, reduce_chunks,
    };

    pub(super) const PALETTE: [[u8; 4]; 2] = [[0x1A, 0x1A, 0x2E, 0xFF], [0xF2, 0xA6, 0x3B, 0xFF]];

    henad_core::params! {
        const DENSITY = f32_param("density", "Initial density", 0.5, 0.0, 1.0, Some(0.01)).on_reload();
    }

    pub(crate) struct Vote;

    impl GridModel for Vote {
        const NAME: &'static str = "Vote";
        const ID: &'static str = "vote";
        const DESCRIPTION: &'static str = "Each cell takes the majority of its neighbourhood";
        const PALETTE: &'static [[u8; 4]] = &PALETTE;
        const NEIGHBORHOOD: NeighborhoodKind = NeighborhoodKind::Moore;
        const STATS: &'static [StatDescriptor] = &[StatDescriptor::new("Ones", PALETTE[1])];

        type Params = ();

        fn param_descriptors() -> Vec<ParamDescriptor> {
            descriptors()
        }

        fn from_params(_params: &[ParamValue]) {}

        fn init(grid: &mut Grid2D<u8>, params: &[ParamValue], rng: &mut u64) {
            let threshold = (extract_f32(params, DENSITY, 0.5) * u32::MAX as f32) as u32;
            for cell in grid.current_mut().iter_mut() {
                *cell = u8::from(below(next_bits(rng), threshold));
            }
        }

        fn step_cell(cell: u8, neighbors: &[u8], _params: &(), _rng: &mut u64) -> u8 {
            u8::from(cell + neighbors.iter().sum::<u8>() >= 5)
        }

        fn stats(grid: &Grid2D<u8>) -> Vec<StatValue> {
            let cells = grid.current();
            let ones = reduce_chunks(
                cells.len(),
                STATS_CHUNK,
                |range| cells[range].iter().map(|&cell| u64::from(cell)).sum::<u64>(),
                |a, b| a + b,
                0,
            );
            vec![StatValue::Scalar(ones as f64)]
        }
    }
}

use vote::Vote;

/// Side of a grid the GPU grid ports accept, the bound of their `grid_width` and `grid_height`.
const GPU_GRID_SIDE: u32 = 16_384;

/// Stock model a corpus file is held to.
#[derive(Debug, Clone, Copy)]
enum Oracle {
    /// `gpu_game_of_life`, whose `state` buffer packs 32 cells to a word.
    GameOfLife,
    /// The template's `gpu_vote`, whose seed is the CPU `Vote`'s init.
    Vote,
    /// `gpu_sir`'s `state` buffer alone, for a file whose draws after tick 0 are its own.
    SirCells,
    /// `gpu_sir`'s `state` and `rng` buffers, for the exact port.
    SirExact,
    /// No stock model: `repeat_trials.toml` exercises `region.repeat`, and its cells start at their declared initial
    /// value. `graph_repeat.rs` holds its rate to the closed form.
    Trials,
}

impl Oracle {
    /// Returns the oracle of the corpus file `name`.
    ///
    /// # Panics
    ///
    /// Panics on a file with no oracle, so a new grid file cannot pass unchecked.
    fn of(name: &str) -> Self {
        match name {
            "game_of_life.toml" => Self::GameOfLife,
            "vote.toml" => Self::Vote,
            "sir.toml" => Self::SirCells,
            "gpu_sir_exact.toml" => Self::SirExact,
            "repeat_trials.toml" => Self::Trials,
            other => panic!("the grid corpus file {other} has no oracle in this test"),
        }
    }

    /// Id of the stock model, `None` for a file with no stock model.
    fn stock_id(self) -> Option<&'static str> {
        match self {
            Self::GameOfLife => Some("gpu_game_of_life"),
            Self::Vote => Some("gpu_vote"),
            Self::SirCells | Self::SirExact => Some("gpu_sir"),
            Self::Trials => None,
        }
    }

    /// Returns the stock model's own parameters at their defaults, with `value` in place of the parameter `id`.
    fn own_params(self, id: &str, value: f32) -> Vec<ParamValue> {
        let descriptors = match self {
            Self::GameOfLife => GpuGameOfLife::param_descriptors()[GRID_PARAM_BASE..].to_vec(),
            Self::Vote => Vote::param_descriptors(),
            Self::SirCells | Self::SirExact => GpuSir::param_descriptors()[GRID_PARAM_BASE..].to_vec(),
            Self::Trials => vec![
                u32_param("trials", "Trials", 4, 0, 4),
                f32_param("p", "Chance of a hit", 0.1, 0.0, 1.0, Some(0.01)),
            ],
        };
        descriptors
            .iter()
            .map(|descriptor| {
                if descriptor.id() == id {
                    ParamValue::F32(value)
                } else {
                    descriptor.kind().default_value()
                }
            })
            .collect()
    }

    /// Returns the stock seed of `width` by `height` cells, as lanes of one word per cell by lane id.
    fn seed(self, width: u32, height: u32, own: &[ParamValue], seed: Option<u64>) -> Vec<(&'static str, Vec<u32>)> {
        let mut params = vec![ParamValue::U32(width), ParamValue::U32(height)];
        params.extend_from_slice(own);
        match self {
            Self::GameOfLife => {
                let words = GpuGameOfLife::seed_buffers(width, height, &params, seed).remove(0);
                vec![("state", unpacked_life_cells(&words, width, height))]
            }
            Self::Vote => {
                let mut grid = Grid2D::new(width, height);
                Vote::init(&mut grid, own, &mut grid_init_rng(seed));
                vec![("vote", grid.current().iter().map(|&cell| u32::from(cell)).collect())]
            }
            Self::SirCells => {
                let cells = GpuSir::seed_buffers(width, height, &params, seed).remove(0);
                vec![("health", cells)]
            }
            Self::SirExact => {
                let mut buffers = GpuSir::seed_buffers(width, height, &params, seed).into_iter();
                let cells = buffers.next().expect("the state buffer");
                let rng = buffers.next().expect("the rng buffer");
                vec![("health", cells), ("rng", rng)]
            }
            Self::Trials => vec![("outcome", vec![0; (width as usize) * (height as usize)])],
        }
    }

    /// Returns the stock model's schema, `None` for a file with no stock model.
    fn schema(self) -> Option<(Vec<ParamDescriptor>, Vec<StatDescriptor>, Vec<ActionDescriptor>)> {
        Some(match self {
            Self::GameOfLife => (
                GpuGameOfLife::param_descriptors(),
                GpuGameOfLife::STATS.to_vec(),
                GpuGameOfLife::ACTIONS
                    .iter()
                    .map(|action| action.desc.clone())
                    .collect(),
            ),
            // `gpu_vote` declares the GPU ports' grid parameters, then the CPU model's own, and no action.
            Self::Vote => {
                let mut params = vec![
                    u32_param("grid_width", "Grid width", 1024, 1, GPU_GRID_SIDE),
                    u32_param("grid_height", "Grid height", 1024, 1, GPU_GRID_SIDE),
                ];
                params.extend(Vote::param_descriptors());
                (params, Vote::STATS.to_vec(), Vec::new())
            }
            Self::SirCells | Self::SirExact => (
                GpuSir::param_descriptors(),
                GpuSir::STATS.to_vec(),
                GpuSir::ACTIONS.iter().map(|action| action.desc.clone()).collect(),
            ),
            Self::Trials => return None,
        })
    }
}

/// Returns every grid file of the corpus, starters first.
fn grid_corpus() -> Vec<(String, GraphFile)> {
    graph_corpus(|world| matches!(world, World::Grid(_)))
}

/// Parameter each corpus file's init reads, with a value away from its default.
fn varied_param(oracle: Oracle) -> (&'static str, f32) {
    match oracle {
        Oracle::GameOfLife => ("density", 0.42),
        Oracle::Vote => ("density", 0.37),
        Oracle::SirCells | Oracle::SirExact => ("initial_infected_pct", 0.2),
        Oracle::Trials => ("p", 0.3),
    }
}

#[test]
fn tick_0_cells_of_every_grid_corpus_file_equal_the_stock_init() {
    let corpus = grid_corpus();
    let names: Vec<&str> = corpus.iter().map(|(name, _)| name.as_str()).collect();
    for expected in [
        "game_of_life.toml",
        "vote.toml",
        "sir.toml",
        "gpu_sir_exact.toml",
        "repeat_trials.toml",
    ] {
        assert!(names.contains(&expected), "{expected} is missing from {names:?}");
    }
    for (name, file) in &corpus {
        let oracle = Oracle::of(name);
        let evaluator = HostEvaluator::new(file).unwrap_or_else(|error| panic!("{name}: {error}"));
        let World::Grid(grid) = file.document().world else {
            unreachable!("the corpus holds grid files alone");
        };
        let (varied, value) = varied_param(oracle);
        let defaults = oracle.own_params("", 0.0);
        let changed = oracle.own_params(varied, value);
        // Odd sides leave a partly filled word in each packed Life row, and a 64-wide grid fills whole words.
        for (width, height) in [(37, 23), (64, 48)] {
            let world = HostWorld::Grid {
                width,
                height,
                boundary: grid.boundary,
            };
            for own in [&defaults, &changed] {
                for seed in [None, Some(0), Some(7), Some(u64::MAX)] {
                    let lanes = evaluator
                        .init(world, own, seed)
                        .unwrap_or_else(|error| panic!("{name}: {error}"));
                    for (lane, expected) in oracle.seed(width, height, own, seed) {
                        let column = lanes
                            .column(lane)
                            .unwrap_or_else(|| panic!("{name} declares the lane {lane}"));
                        assert_eq!(
                            column.words(),
                            expected.as_slice(),
                            "{name}: lane {lane} at {width}x{height}, {own:?}, seed {seed:?}"
                        );
                    }
                }
            }
        }
    }
}

/// Returns the schema a GPU grid entry of `file` declares: `grid_width` and `grid_height` up to [`GPU_GRID_SIDE`] at
/// the world's defaults, then the file's parameters, its statistics and its actions.
fn graph_schema(file: &GraphFile) -> (Vec<ParamDescriptor>, Vec<StatDescriptor>, Vec<ActionDescriptor>) {
    let document = file.document();
    let World::Grid(grid) = document.world else {
        unreachable!("the corpus holds grid files alone");
    };
    let mut params = vec![
        u32_param("grid_width", "Grid Width", grid.width, 1, GPU_GRID_SIDE),
        u32_param("grid_height", "Grid Height", grid.height, 1, GPU_GRID_SIDE),
    ];
    params.extend(document.params.iter().cloned());
    let stats = document
        .stats
        .iter()
        .map(|stat| StatDescriptor::owned(stat.label.clone(), stat.color))
        .collect();
    let actions = document
        .actions
        .iter()
        .map(|action| ActionDescriptor::owned(action.id.clone(), action.label.clone()))
        .collect();
    (params, stats, actions)
}

/// Returns the schema hash of `schema` under the model id `id`.
fn hash_of(
    id: &str,
    (params, stats, actions): &(Vec<ParamDescriptor>, Vec<StatDescriptor>, Vec<ActionDescriptor>),
) -> u64 {
    schema_hash(&ModelSchema {
        id,
        params,
        stats,
        actions,
    })
}

/// [`schema_hash`] reads the model id, so a graph's `node_<model>` id hashes apart from the stock model. Every other
/// part of the schema the hash reads is held equal here, under the stock id.
#[test]
fn every_grid_corpus_schema_hashes_like_the_stock_model_under_its_id() {
    let mut checked = 0;
    for (name, file) in grid_corpus() {
        let oracle = Oracle::of(&name);
        let (Some(stock_id), Some(stock)) = (oracle.stock_id(), oracle.schema()) else {
            continue;
        };
        checked += 1;
        let graph = graph_schema(&file);
        assert_eq!(
            hash_of(stock_id, &graph),
            hash_of(stock_id, &stock),
            "{name}: the schema differs from {stock_id}"
        );
        assert_ne!(
            hash_of(&file.document().target.id, &graph),
            hash_of(stock_id, &stock),
            "{name}: the hash reads the model id"
        );
        // The entry the model file builds declares the schema this test builds.
        let entry = model_entry(&file.to_toml());
        let schema = entry.schema();
        let declared = (schema.params.to_vec(), schema.stats.to_vec(), schema.actions.to_vec());
        assert_eq!(schema.id, file.document().target.id, "{name}");
        assert_eq!(
            hash_of(stock_id, &declared),
            hash_of(stock_id, &stock),
            "{name}: the entry's schema differs from {stock_id}"
        );
    }
    assert_eq!(checked, 4, "every grid corpus file with a stock model");
    // The registered stock entries hash as the schemas this test builds.
    let models = crate::example_models();
    for oracle in [Oracle::GameOfLife, Oracle::SirExact] {
        let id = oracle.stock_id().expect("a stock model");
        let entry = models.get(id).expect("a stock model");
        let schema = oracle.schema().expect("a stock schema");
        assert_eq!(schema_hash(&entry.schema()), hash_of(id, &schema));
    }
}

/// Returns the demand's work for `file` at `GPU_GRID_SIDE` by `GPU_GRID_SIDE` cells: one pass per GPU pass of the
/// file, with the checker's cost per element, and the init passes as one run on the host.
fn largest_work(file: &GraphFile) -> Demand {
    let report = file.check();
    let document = file.document();
    let cells = u64::from(GPU_GRID_SIDE) * u64::from(GPU_GRID_SIDE);
    let limits = wgpu::Limits::default();
    let mut demand = Demand::default();
    demand.set_display(GPU_GRID_SIDE, GPU_GRID_SIDE, &limits);
    let texels = demand
        .texture
        .map_or(0, |(width, height)| u64::from(width) * u64::from(height));
    let mut init = 0u64;
    for (pass, cost) in document.passes.iter().zip(report.passes.iter().map(|pass| pass.cost)) {
        let (kind, elements) = match pass.stage {
            PassStage::Init => {
                init = init.saturating_add(cost.saturating_mul(cells));
                continue;
            }
            PassStage::Tick => (PassKind::Step, cells),
            PassStage::View => (PassKind::Display, texels),
            PassStage::Stats => (PassKind::Reduce, cells),
            PassStage::Action => {
                let action = pass.action.as_deref().expect("an action pass names its action");
                let index = document
                    .actions
                    .iter()
                    .position(|declared| declared.id == action)
                    .expect("the action is declared");
                (PassKind::Action(index), cells)
            }
            other => panic!("pass {} of stage {other:?} on a grid", pass.id),
        };
        demand.push_pass(pass.id.clone(), kind, 0, cost.saturating_mul(elements));
    }
    demand.push_host("init".to_owned(), init);
    demand
}

/// Every grid corpus file fits the submission and host budgets at the largest grid the GPU grid ports accept, so the
/// budgets never refuse a corpus model at a size its stock model runs at.
#[test]
fn every_grid_corpus_file_at_its_largest_size_fits_the_work_budgets() {
    for (name, file) in grid_corpus() {
        let demand = largest_work(&file);
        let submission = demand.submission_work();
        assert!(
            submission > 0 && submission <= SUBMISSION_WORK_BUDGET,
            "{name}: {submission} units in one submission"
        );
        for run in &demand.host {
            assert!(run.work <= HOST_WORK_BUDGET, "{name}: {} units on the host", run.work);
        }
        let found = demand.shortfalls(&wgpu::Limits::default());
        assert!(found.is_empty(), "{name}: {found:?}");
        // The entry the compiler builds reports the same work, on a device that holds its buffers.
        let entry = model_entry(&file.to_toml());
        let limits = roomy_limits();
        let side = [ParamValue::U32(GPU_GRID_SIDE), ParamValue::U32(GPU_GRID_SIDE)];
        let found = entry.shortfalls(&side, &limits);
        assert!(found.is_empty(), "{name}: {found:?}");
    }
}

/// The entry of every grid corpus file passes every check of the testing kit, its GPU checks on a baseline device
/// included.
#[test]
fn every_grid_corpus_entry_conforms() {
    assert_entries_conform(
        grid_corpus()
            .into_iter()
            .map(|(name, file)| (name, model_entry(&file.to_toml()))),
    );
}
