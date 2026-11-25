//! The agents graph corpus held to the stock models at tick 0: every graph file's init, run by henad-graph's host
//! evaluator, equals the stock GPU model's seeded buffers, lane for lane and cell for cell.
//!
//! The corpus is henad-graph's agent starters and the agents ports in `tests/fixtures/graphs`. The random walk has no
//! stock model, and its oracle is the walk's init written out on the agent init stream.

use henad_compute::cpu::agent_engine::agent_init_rng;
use henad_compute::entry::ModelEntry;
use henad_compute::gpu::GpuAgentState;
use henad_core::authoring::model::gpu_agent_model::GpuAgentModel as _;
use henad_core::authoring::primitives::rng::next_float;
use henad_core::explore::fingerprint::schema_hash;
use henad_core::explore::plan::ModelSchema;
use henad_core::params::ParamValue;
use henad_graph::host::{HostEvaluator, HostLanes, HostValue, HostWorld};

use super::support::{agent_corpus, model_entry};
use crate::gpu_ants::GpuAnts;
use crate::gpu_boids::GpuBoids;

/// Stock model a corpus file seeds like.
#[derive(Debug, Clone, Copy)]
enum Oracle {
    Boids,
    Ants,
    AntsWithRng,
    RandomWalk,
}

impl Oracle {
    /// Returns the oracle of the corpus file `name`.
    fn of(name: &str) -> Self {
        match name {
            "boids.toml" | "gpu_boids.toml" => Self::Boids,
            "ants.toml" => Self::Ants,
            "gpu_ants.toml" => Self::AntsWithRng,
            "random_walk.toml" => Self::RandomWalk,
            other => panic!("the agents corpus file {other} has no oracle"),
        }
    }

    /// Returns the file's own parameters at their defaults, or with the init's parameter changed when `varied`.
    fn own_params(self, varied: bool) -> Vec<ParamValue> {
        match self {
            // visual_range, protected_range, separation, alignment, cohesion, max_speed, min_speed.
            Self::Boids => vec![
                ParamValue::F32(50.0),
                ParamValue::F32(8.0),
                ParamValue::F32(0.05),
                ParamValue::F32(0.05),
                ParamValue::F32(0.0005),
                ParamValue::F32(if varied { 22.5 } else { 15.0 }),
                ParamValue::F32(if varied { 1.5 } else { 3.0 }),
            ],
            // update_cutdown, reward, momentum, random_action, evaporation.
            Self::Ants | Self::AntsWithRng => vec![
                ParamValue::F32(0.9),
                ParamValue::F32(if varied { 2.5 } else { 1.0 }),
                ParamValue::F32(0.8),
                ParamValue::F32(0.1),
                ParamValue::F32(0.999),
            ],
            Self::RandomWalk => vec![ParamValue::U32(if varied { 9 } else { 4 }), ParamValue::F32(2.0)],
        }
    }

    /// Returns the expected words of each lane and field layer the stock model seeds, by id.
    fn seed(
        self,
        count: u32,
        width: f32,
        height: f32,
        own: &[ParamValue],
        seed: Option<u64>,
    ) -> Vec<(&'static str, Vec<u32>)> {
        let mut params = vec![ParamValue::U32(count), ParamValue::F32(width), ParamValue::F32(height)];
        params.extend_from_slice(own);
        match self {
            Self::Boids => {
                let geometry = GpuAgentState::<GpuBoids>::geometry_for(&params, &wgpu::Limits::default());
                let buffers = GpuBoids::seed_buffers(&geometry, &params, seed);
                vec![("pos", words(&buffers[0])), ("vel", words(&buffers[1]))]
            }
            Self::Ants | Self::AntsWithRng => {
                let geometry = GpuAgentState::<GpuAnts>::geometry_for(&params, &wgpu::Limits::default());
                let buffers = GpuAnts::seed_buffers(&geometry, &params, seed);
                // `state` packs the last step in its low byte, then the food flag and the reward flag.
                let state = words(&buffers[1]);
                let ParamValue::F32(reward) = own[1] else {
                    unreachable!("the reward is an f32");
                };
                let mut lanes = vec![
                    ("pos", words(&buffers[0])),
                    ("last_step", state.iter().map(|word| word & 0xFF).collect()),
                    ("cargo", state.iter().map(|word| (word >> 8) & 1).collect()),
                    (
                        "reward_held",
                        state
                            .iter()
                            .map(|word| if word & 0x200 != 0 { reward.to_bits() } else { 0 })
                            .collect(),
                    ),
                    ("sites.value", words(&buffers[6])),
                    ("pheromone.to_food", vec![0; geometry.n_cells as usize]),
                    ("pheromone.to_home", vec![0; geometry.n_cells as usize]),
                ];
                if matches!(self, Self::AntsWithRng) {
                    lanes.push(("rng", words(&buffers[3])));
                }
                lanes
            }
            Self::RandomWalk => {
                let mut rng = agent_init_rng(seed);
                let mut pos = Vec::with_capacity(2 * count as usize);
                for _ in 0..count {
                    pos.push(next_float(&mut rng, width).to_bits());
                    pos.push(next_float(&mut rng, height).to_bits());
                }
                vec![("pos", pos), ("last", vec![0; 2 * count as usize])]
            }
        }
    }
}

/// Returns `bytes` as little-endian `u32` words.
fn words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
        .collect()
}

/// Returns the words of the lane or field layer `id` of `lanes`.
fn column_words<'a>(lanes: &'a HostLanes, name: &str, id: &str) -> &'a [u32] {
    lanes
        .column(id)
        .unwrap_or_else(|| panic!("{name} declares the lane or layer {id}"))
        .words()
}

#[test]
fn tick_0_lanes_of_every_agent_corpus_file_equal_the_stock_init() {
    let corpus = agent_corpus();
    let names: Vec<&str> = corpus.iter().map(|(name, _)| name.as_str()).collect();
    for expected in [
        "boids.toml",
        "ants.toml",
        "random_walk.toml",
        "gpu_boids.toml",
        "gpu_ants.toml",
    ] {
        assert!(names.contains(&expected), "{expected} is missing from {names:?}");
    }
    for (name, file) in &corpus {
        let oracle = Oracle::of(name);
        let evaluator = HostEvaluator::new(file).unwrap_or_else(|error| panic!("{name}: {error}"));
        // A world of fractional size has a field grid of whole cells, and a narrow world holds the field grid of
        // one column.
        for (count, width, height) in [(37, 200.0, 200.0), (500, 123.5, 77.25), (64, 1.0, 40.0)] {
            for varied in [false, true] {
                let own = oracle.own_params(varied);
                for seed in [None, Some(0), Some(7), Some(u64::MAX)] {
                    let world = HostWorld::Agents { count, width, height };
                    let lanes = evaluator
                        .init(world, &own, seed)
                        .unwrap_or_else(|error| panic!("{name}: {error}"));
                    for (id, expected) in oracle.seed(count, width, height, &own, seed) {
                        assert_eq!(
                            column_words(&lanes, name, id),
                            expected.as_slice(),
                            "{name}: {id} of {count} agents in {width}x{height}, varied {varied}, seed {seed:?}"
                        );
                    }
                }
            }
        }
    }
}

/// The ants' reset reruns the init pass `colony` on the host, which restores the four lanes of every ant and leaves the
/// static sites, as the stock `reset_colony` does.
#[test]
fn the_ants_reset_reruns_the_colony_on_the_host() {
    for (name, file) in agent_corpus() {
        if !matches!(Oracle::of(&name), Oracle::Ants | Oracle::AntsWithRng) {
            continue;
        }
        let evaluator = HostEvaluator::new(&file).unwrap_or_else(|error| panic!("{name}: {error}"));
        let world = HostWorld::Agents {
            count: 50,
            width: 60.0,
            height: 40.0,
        };
        let own = Oracle::Ants.own_params(true);
        let start = evaluator.init(world, &own, Some(9)).expect("the init runs");
        let mut lanes = start.clone();
        // Every ant moved, stepped, picked up food and spent its reward.
        for (id, value) in [
            ("pos", HostValue::Vec2F([1.0, 2.0])),
            ("last_step", HostValue::U32(3)),
            ("cargo", HostValue::Category(1)),
            ("reward_held", HostValue::F32(0.0)),
        ] {
            let column = lanes.column_mut(id).expect("a lane");
            for index in 0..column.len() {
                assert!(column.set(index, value), "{name}: {id} takes {value:?}");
            }
            assert_ne!(
                column_words(&lanes, &name, id),
                column_words(&start, &name, id),
                "{name}: {id} moved"
            );
        }
        let mut stream = henad_core::action::action_seed(Some(9));
        let written = evaluator
            .rerun("reset_colony", 1, &mut lanes, &own, Some(9), &mut stream)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert!(!written.is_empty(), "{name}: the rerun writes lanes");
        for id in ["pos", "last_step", "cargo", "reward_held", "sites.value"] {
            assert_eq!(
                column_words(&lanes, &name, id),
                column_words(&start, &name, id),
                "{name}: {id}"
            );
        }
    }
}

/// Returns the schema hash of the schema of `entry` under the model id `id`.
fn hash_under(entry: &ModelEntry, id: &str) -> u64 {
    let schema = entry.schema();
    schema_hash(&ModelSchema {
        id,
        params: schema.params,
        stats: schema.stats,
        actions: schema.actions,
    })
}

/// [`schema_hash`] reads the model id, so a graph's `node_<model>` id hashes apart from the stock model. Every other
/// part of the schema is held equal here: the entry each agents corpus file builds hashes as the stock GPU port and the
/// stock CPU model of its oracle under the stock id. The random walk has no stock model.
#[test]
fn every_agent_corpus_schema_hashes_like_the_stock_models_under_their_ids() {
    let models = crate::example_models();
    let mut checked = 0;
    for (name, file) in agent_corpus() {
        let stock_ids: &[&str] = match Oracle::of(&name) {
            Oracle::Boids => &["gpu_boids", "boids"],
            Oracle::Ants | Oracle::AntsWithRng => &["gpu_ants", "ants"],
            Oracle::RandomWalk => continue,
        };
        let entry = model_entry(&file.to_toml());
        assert_eq!(entry.id(), file.document().target.id, "{name}");
        for &stock_id in stock_ids {
            let stock = models.get(stock_id).expect("a stock model");
            assert_eq!(
                hash_under(&entry, stock_id),
                schema_hash(&stock.schema()),
                "{name}: the schema differs from {stock_id}"
            );
            assert_ne!(
                schema_hash(&entry.schema()),
                schema_hash(&stock.schema()),
                "{name}: the hash reads the model id"
            );
            checked += 1;
        }
    }
    assert_eq!(
        checked, 8,
        "every agents corpus file with a stock model, against both stock backends"
    );
}
