//! The ants graphs held to the stock ants models on one device.
//!
//! A probe first reads how the device rounds the deposit's `field * cut + reward`, which the stock `step.wgsl` writes
//! as one expression and the generated `lay` shader as two `let`s. `gpu_ants.toml` is then held to the stock
//! `gpu_ants` in every lane, both field layers, the colours and the three statistics every tick, across a press of
//! Reset colony. The starter `ants.toml` draws keyed words, and is held to the stock models where no draw decides:
//! at tick 0, under the cross-engine gate of `tests/consistency_ants.rs`, in the decay of every cell no ant stands on,
//! and in the other checks of that file.

use std::sync::Arc;

use henad_compute::cpu::agent_engine::AgentModelState;
use henad_compute::entry::ModelEntry;
use henad_compute::gpu::sim_thread::GpuSimState;
use henad_compute::gpu::{AgentHooks, AgentPassId, AgentRun, GpuContext, RerunUploads, wgpu};
use henad_core::authoring::model::field::Extent;
use henad_core::authoring::model::gpu_agent_model::{Geometry, PassCtx, PassId};
use henad_core::authoring::primitives::rng::pcg_hash;
use henad_core::model::SimState as _;
use henad_core::params::ParamValue;
use henad_core::view::{StatEntry, StatValue};
use henad_graph::GraphFile;
use henad_graph::wgsl::{AgentProgram, LaneBuffer};

use super::lanes::GraphLanes;
use super::support::{
    baseline_device, buffer_index, buffer_words, build_gpu, declarations_like, fixture_text, graph_buffer, model_entry,
    params_for, press, snapshot, starter_entry, stat, stat_values, step,
};
use crate::ants::AntsModel;
use crate::ants::field::{LOW_PHEROMONE, OBSTACLE, TO_FOOD, TO_HOME};

#[path = "../../tests/gates/ants.rs"]
mod gate;

use gate::{GATE_AGENTS, GATE_H, GATE_TICKS, GATE_W, GateAnt, GateResult, gate_field, gate_params};

// Buffers of the stock `gpu_ants`, in its `buffers!` order.
const STOCK_POS: usize = 0;
const STOCK_STATE: usize = 1;
const STOCK_COLOR: usize = 2;
const STOCK_RNG: usize = 3;
const STOCK_FIELD: usize = 4;
const STOCK_SITES: usize = 6;

// Bits of the stock `state` word, as `gpu_ants/state.wgsl` packs them.
const LAST_STEP_MASK: u32 = 0xFF;
const HAS_FOOD_BIT: u32 = 0x100;
const HAS_REWARD_BIT: u32 = 0x200;

/// Number of inputs the contraction probe runs.
const PROBE_INPUTS: u32 = 1 << 16;

/// The probe's shader: the stock deposit's expression in one statement, and the generated shader's two `let`s, each
/// under the stock's `max`.
const PROBE_WGSL: &str = r"
@group(0) @binding(0) var<storage, read> inputs: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> outputs: array<vec2<u32>>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if (i >= arrayLength(&inputs)) {
        return;
    }
    let v = inputs[i];
    let stock = max(v.w, v.x * v.y + v.z);
    let n43_out = v.x * v.y;
    let n44_out = n43_out + v.z;
    let graph = max(v.w, n44_out);
    outputs[i] = vec2<u32>(bitcast<u32>(stock), bitcast<u32>(graph));
}
";

/// Returns the probe's input `i`: a field value, a cut, a reward and the best value so far, in the ranges a run of
/// the ants meets, with the stock cuts and rewards among them.
fn probe_input(i: u32) -> [f32; 4] {
    let word = |salt: u32| pcg_hash(i.wrapping_mul(4).wrapping_add(salt));
    let unit = |salt: u32| (word(salt) >> 8) as f32 / (1u32 << 24) as f32;
    let field = unit(0) * 20.0;
    let cut = match word(1) % 3 {
        0 => 0.9,
        1 => 0.9f32.powf(std::f32::consts::SQRT_2),
        _ => 0.5 + unit(5) * 0.5,
    };
    let reward = match word(2) % 4 {
        0 => 0.0,
        1 => 1.0,
        2 => 2.5,
        _ => 0.1 + unit(6) * 9.9,
    };
    let best = if word(3) % 2 == 0 { 0.0 } else { unit(7) * 4.0 };
    [field, cut, reward, best]
}

/// Runs the probe on `ctx` and returns the stock form's and the generated form's bits for each input.
fn run_probe(ctx: &GpuContext, inputs: &[[f32; 4]]) -> Vec<[u32; 2]> {
    let device = &ctx.device;
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("contraction_probe"),
        source: wgpu::ShaderSource::Wgsl(PROBE_WGSL.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("contraction_probe"),
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });
    let input_bytes: Vec<u8> = inputs.iter().flatten().flat_map(|value| value.to_le_bytes()).collect();
    let input = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("contraction_probe_inputs"),
        size: input_bytes.len() as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    ctx.queue.write_buffer(&input, 0, &input_bytes);
    let size = (inputs.len() * 8) as u64;
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("contraction_probe_outputs"),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("contraction_probe_staging"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("contraction_probe"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: input.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: output.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups((inputs.len() as u32).div_ceil(64), 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &staging, 0, size);
    ctx.queue.submit(Some(encoder.finish()));
    let slice = staging.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("the device polls");
    let bytes = slice.get_mapped_range().expect("the mapped range").to_vec();
    staging.unmap();
    bytes
        .chunks_exact(8)
        .map(|pair| {
            [
                u32::from_le_bytes([pair[0], pair[1], pair[2], pair[3]]),
                u32::from_le_bytes([pair[4], pair[5], pair[6], pair[7]]),
            ]
        })
        .collect()
}

/// The device rounds the deposit's `max(best, field * cut + reward)` alike in the stock's one expression and in the
/// generated shader's two `let`s, bit for bit over [`PROBE_INPUTS`] inputs. The test prints how many results equal a
/// rounded product and sum, and how many a fused multiply-add, over the inputs where those two differ.
///
/// Note that the oracle of `gpu_ants.toml` rests on the first equality alone. Which of the other two the device
/// returns says whether it contracts, and is information for the record.
#[test]
#[expect(clippy::print_stderr, reason = "the probe's counts are information for the record")]
fn the_device_rounds_the_deposit_alike_in_both_forms() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    let inputs: Vec<[f32; 4]> = (0..PROBE_INPUTS).map(probe_input).collect();
    let found = run_probe(&ctx, &inputs);
    let (mut apart, mut unfused, mut fused) = (0u32, 0u32, 0u32);
    for (i, ([field, cut, reward, best], [stock, graph])) in inputs.iter().zip(&found).enumerate() {
        assert_eq!(
            stock,
            graph,
            "input {i} ({field}, {cut}, {reward}, {best}): the stock form gives {} and the generated form {}",
            f32::from_bits(*stock),
            f32::from_bits(*graph)
        );
        let separate = best.max(field * cut + reward).to_bits();
        let contracted = best.max(field.mul_add(*cut, *reward)).to_bits();
        if separate != contracted {
            apart += 1;
            unfused += u32::from(*stock == separate);
            fused += u32::from(*stock == contracted);
        }
    }
    assert!(apart > 0, "no input tells a rounded product from a fused one");
    eprintln!(
        "contraction probe: {PROBE_INPUTS} inputs, both forms equal; of {apart} inputs where a fused multiply-add \
         differs from a rounded product and sum, the device returned the rounded result {unfused} times and the fused \
         one {fused} times"
    );
}

/// Returns the parameters of an ants model of `agents` ants in a square world of side `world`.
fn ants_params(entry: &ModelEntry, agents: u32, world: f32) -> Vec<ParamValue> {
    params_for(
        entry,
        &[
            ("num_agents", ParamValue::U32(agents)),
            ("world_width", ParamValue::F32(world)),
            ("world_height", ParamValue::F32(world)),
        ],
    )
}

/// Asserts that the ants graph `graph`, built from `entry` with its lanes placed as `lanes`, holds the lanes and field
/// layers of the stock `gpu_ants` state `stock`: positions, the three fields the stock packs in `state`, the sites and,
/// when `with_rng`, the `rng` lane. `reward` is the reward parameter, the value the stock's reward bit stands for.
fn assert_lanes_equal(
    entry: &ModelEntry,
    lanes: &GraphLanes,
    graph: &dyn GpuSimState,
    stock: &dyn GpuSimState,
    reward: f32,
    with_rng: bool,
    what: &str,
) {
    let packed = buffer_words(stock, STOCK_STATE);
    let expected: [(&str, Vec<u32>); 6] = [
        ("pos", buffer_words(stock, STOCK_POS)),
        ("last_step", packed.iter().map(|word| word & LAST_STEP_MASK).collect()),
        (
            "cargo",
            packed.iter().map(|word| u32::from(word & HAS_FOOD_BIT != 0)).collect(),
        ),
        (
            "reward_held",
            packed
                .iter()
                .map(|word| {
                    if word & HAS_REWARD_BIT == 0 {
                        0
                    } else {
                        reward.to_bits()
                    }
                })
                .collect(),
        ),
        ("field_pheromone_cells", buffer_words(stock, STOCK_FIELD)),
        ("field_sites_cells", buffer_words(stock, STOCK_SITES)),
    ];
    for (label, words) in expected {
        // A field has a buffer of its own, and a lane may share one.
        let found = if label.starts_with("field_") {
            graph_buffer(entry, graph, label)
        } else {
            lanes.read(graph, label)
        };
        if let Some(first) = found.iter().zip(&words).position(|(a, b)| a != b) {
            panic!(
                "{what}: {label} word {first} is {:#x} and gpu_ants holds {:#x} ({} words differ)",
                found[first],
                words[first],
                found.iter().zip(&words).filter(|(a, b)| a != b).count()
            );
        }
        assert_eq!(found.len(), words.len(), "{what}: {label}");
    }
    if with_rng {
        assert!(
            lanes.read(graph, "rng") == buffer_words(stock, STOCK_RNG),
            "{what}: the rng lane differs from gpu_ants"
        );
    }
}

/// `gpu_ants.toml` equals the stock `gpu_ants` on one device every tick: every lane, both field layers and the sites,
/// the colours of the ants and the three statistics, Carrying Food, Deliveries and Total Pheromone, bit for bit, before
/// and after a press of Reset colony, at two worlds and three seeds. The longest run reaches the colony's first
/// deliveries.
#[test]
fn the_exact_ants_graph_equals_gpu_ants_every_tick() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    let models = crate::example_models();
    let stock_entry = models.get("gpu_ants").expect("the stock model");
    let entry = model_entry(&fixture_text("graphs/gpu_ants.toml"));
    let lanes = GraphLanes::of_text(&fixture_text("graphs/gpu_ants.toml"));
    let colors = buffer_index(&entry, "agent_colors");
    for (agents, world, seed, ticks, press_at) in [
        (2_000, 200.0, None, 1_540, Some(1_500)),
        (2_000, 200.0, Some(7), 160, None),
        (20_000, 300.0, Some(11), 60, Some(25)),
    ] {
        let params = ants_params(&entry, agents, world);
        let stock_params = ants_params(stock_entry, agents, world);
        assert_eq!(params, stock_params, "the two schemas order their parameters alike");
        let ParamValue::F32(reward) = params[entry.param_index("reward").expect("a reward parameter")] else {
            unreachable!("the reward is an f32");
        };
        let mut graph = build_gpu(&entry, &params, seed, &ctx);
        let mut stock = build_gpu(stock_entry, &params, seed, &ctx);
        for tick in 0..=ticks {
            if tick > 0 {
                step(&ctx, &mut *graph, 1);
                step(&ctx, &mut *stock, 1);
            }
            if press_at == Some(tick) {
                press(&ctx, &mut *graph, 0);
                press(&ctx, &mut *stock, 0);
            }
            let what = format!("{agents} ants in {world}, seed {seed:?}, tick {tick}");
            assert_lanes_equal(&entry, &lanes, &*graph, &*stock, reward, true, &what);
            let (found, expected) = (stat_values(&ctx, &mut *graph), stat_values(&ctx, &mut *stock));
            assert_eq!(found, expected, "{what}: the statistics");
            assert!(
                graph.read_buffer(colors) == stock.read_buffer(STOCK_COLOR),
                "{what}: the colours of the ants"
            );
        }
        if ticks >= 1_500 {
            assert!(
                stat(&stock.stats(), "Deliveries") > 0.0,
                "the colony delivered nothing, so the comparison never reached the nest"
            );
        }
    }
}

/// `ants.toml` built on the device starts as the stock `gpu_ants` does: positions, the packed fields, both layers and
/// the sites, at two worlds and two seeds.
#[test]
fn the_ants_starter_starts_as_gpu_ants_on_the_device() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    let models = crate::example_models();
    let stock_entry = models.get("gpu_ants").expect("the stock model");
    let entry = starter_entry("ants.toml");
    let lanes = GraphLanes::of_starter("ants.toml");
    for (agents, world) in [(2_000, 200.0), (777, 96.0)] {
        for seed in [None, Some(7)] {
            let params = ants_params(&entry, agents, world);
            let graph = build_gpu(&entry, &params, seed, &ctx);
            let stock = build_gpu(stock_entry, &params, seed, &ctx);
            let ParamValue::F32(reward) = params[entry.param_index("reward").expect("a reward parameter")] else {
                unreachable!("the reward is an f32");
            };
            let what = format!("{agents} ants in {world}, seed {seed:?}");
            assert_lanes_equal(&entry, &lanes, &*graph, &*stock, reward, false, &what);
        }
    }
}

/// Runs the gate on the stock CPU ants, as `consistency_ants.rs` does.
fn cpu_gate(seed: Option<u64>) -> GateResult {
    let mut state = AgentModelState::<AntsModel>::from_agents_and_field(
        &gate_params(),
        seed,
        |lanes, _extent| {
            for (i, &(x, y, last_step, has_food, reward)) in GATE_AGENTS.iter().enumerate() {
                lanes.pos_x[i] = x;
                lanes.pos_y[i] = y;
                lanes.last_step[i] = last_step;
                lanes.has_food[i] = has_food;
                lanes.reward[i] = reward;
            }
        },
        |field, _extent| {
            assert!(field.seed_field(TO_FOOD, &gate_field(TO_FOOD)), "to-food layer");
            assert!(field.seed_field(TO_HOME, &gate_field(TO_HOME)), "to-home layer");
        },
    );
    for _ in 0..GATE_TICKS {
        state.step();
    }
    let lanes = state.lanes();
    let agents = (0..GATE_AGENTS.len())
        .map(|i| {
            (
                lanes.pos_x[i],
                lanes.pos_y[i],
                lanes.last_step[i],
                lanes.has_food[i],
                lanes.reward[i],
            )
        })
        .collect();
    (
        agents,
        state.field().field(TO_FOOD).current().to_vec(),
        state.field().field(TO_HOME).current().to_vec(),
    )
}

/// Hooks of a graph whose build uploads planted words in place of some of its seeded buffers. Every other hook
/// forwards to the graph's own.
struct PlantedHooks {
    inner: Box<dyn AgentHooks>,
    /// Buffers to replace, each as its position, the lane's segment when it plants one lane of the buffer, and its
    /// words.
    planted: Vec<(usize, Option<LaneBuffer>, Vec<u32>)>,
}

impl AgentHooks for PlantedHooks {
    fn dims(&self, params: &[ParamValue]) -> (u32, Extent) {
        self.inner.dims(params)
    }

    fn buffer_lens(&self, geom: &Geometry) -> Vec<usize> {
        self.inner.buffer_lens(geom)
    }

    fn seed_buffers(&self, geom: &Geometry, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u8>> {
        let mut buffers = self.inner.seed_buffers(geom, params, seed);
        for (index, segment, words) in &self.planted {
            let bytes: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
            match segment {
                Some(segment) => {
                    let range = segment.segment_range(buffers[*index].len() / 4);
                    buffers[*index][range.start * 4..range.end * 4].copy_from_slice(&bytes);
                }
                None => buffers[*index] = bytes,
            }
        }
        buffers
    }

    fn index_cell_size(&self, params: &[ParamValue]) -> f32 {
        self.inner.index_cell_size(params)
    }

    fn pass_params_bytes(&self, pass: PassId, ctx: PassCtx<'_>, params: &[ParamValue]) -> Vec<u8> {
        self.inner.pass_params_bytes(pass, ctx, params)
    }

    fn stats(&self, sums: &[f32], counters: &[u32], geom: &Geometry) -> Vec<StatValue> {
        self.inner.stats(sums, counters, geom)
    }

    fn run_params_bytes(
        &self,
        pass: AgentPassId,
        ctx: PassCtx<'_>,
        run: AgentRun,
        params: &[ParamValue],
    ) -> Result<Vec<u8>, String> {
        self.inner.run_params_bytes(pass, ctx, run, params)
    }

    fn run_stats(
        &self,
        sums: &[f32],
        counters: &[u32],
        geom: &Geometry,
        run: AgentRun,
        params: &[ParamValue],
    ) -> Vec<StatValue> {
        self.inner.run_stats(sums, counters, geom, run, params)
    }

    fn rerun(
        &self,
        action: usize,
        geom: &Geometry,
        run: AgentRun,
        params: &[ParamValue],
        stream: &mut u64,
    ) -> Result<RerunUploads, String> {
        self.inner.rerun(action, geom, run, params, stream)
    }

    fn fault(&self, counters: &[u32]) -> Option<String> {
        self.inner.fault(counters)
    }

    fn reports_faults(&self) -> bool {
        self.inner.reports_faults()
    }

    fn step_cost(&self, step: usize, geom: &Geometry, params: &[ParamValue]) -> u64 {
        self.inner.step_cost(step, geom, params)
    }
}

/// Returns the entry of `file`, a graph of agents, whose build uploads the words of `planted` in place of the seeded
/// lanes they name by id, or of the seeded buffers they name by label.
pub(super) fn planted_entry(file: &GraphFile, planted: Vec<(&str, Vec<u32>)>) -> ModelEntry {
    let program = AgentProgram::compile(file).unwrap_or_else(|problems| panic!("{problems:#?}"));
    let plain = program.entry();
    let mut spec = program.spec();
    let planted = planted
        .into_iter()
        .map(|(name, words)| {
            if let Some(segment) = program.lane_buffer(name) {
                return (segment.buffer(), Some(segment), words);
            }
            let position = spec
                .buffers
                .iter()
                .position(|buffer| buffer.label == name)
                .unwrap_or_else(|| panic!("no lane or buffer {name}"));
            (position, None, words)
        })
        .collect();
    // A second spec of the same program lends its hooks, which share the program's compiled parts.
    spec.hooks = Box::new(PlantedHooks {
        inner: program.spec().hooks,
        planted,
    });
    let declarations = declarations_like(&plain, spec.topology_hint(), spec.structure());
    ModelEntry::from_gpu_agent_spec(declarations, plain.source().clone(), Arc::new(spec))
}

/// Returns the entry of the starter `ants.toml` whose build plants the gate's ants and seeded field.
fn planted_gate_entry() -> ModelEntry {
    let starter = henad_graph::starters::starter("ants.toml").expect("the ants starter");
    let file = GraphFile::parse(starter.text).expect("the ants starter reads");
    let lane = |column: fn(&GateAnt) -> Vec<u32>| GATE_AGENTS.iter().flat_map(column).collect();
    planted_entry(
        &file,
        vec![
            ("pos", lane(|agent| vec![agent.0.to_bits(), agent.1.to_bits()])),
            ("last_step", lane(|agent| vec![u32::from(agent.2)])),
            ("cargo", lane(|agent| vec![u32::from(agent.3)])),
            ("reward_held", lane(|agent| vec![agent.4.to_bits()])),
            (
                "field_pheromone_cells",
                gate_field(TO_FOOD)
                    .iter()
                    .chain(&gate_field(TO_HOME))
                    .map(|value| value.to_bits())
                    .collect(),
            ),
        ],
    )
}

/// Runs the gate on the device through `ants.toml` with the seed `seed`.
fn graph_gate(ctx: &GpuContext, entry: &ModelEntry, seed: Option<u64>) -> GateResult {
    let params = params_for(
        entry,
        &[
            ("num_agents", ParamValue::U32(GATE_AGENTS.len() as u32)),
            ("world_width", ParamValue::F32(GATE_W as f32)),
            ("world_height", ParamValue::F32(GATE_H as f32)),
            ("momentum", ParamValue::F32(0.0)),
            ("random_action", ParamValue::F32(0.0)),
        ],
    );
    assert_eq!(
        params,
        gate_params(),
        "the graph's gate parameters are the stock gate's"
    );
    let mut state = build_gpu(entry, &params, seed, ctx);
    step(ctx, &mut *state, GATE_TICKS);
    let lanes = GraphLanes::of_starter("ants.toml");
    let read = |id: &str| lanes.read(&*state, id);
    let (pos, last_step, cargo, reward) = (read("pos"), read("last_step"), read("cargo"), read("reward_held"));
    let agents = (0..GATE_AGENTS.len())
        .map(|i| {
            (
                f32::from_bits(pos[2 * i]),
                f32::from_bits(pos[2 * i + 1]),
                u8::try_from(last_step[i]).expect("a step code fits a byte"),
                u8::try_from(cargo[i]).expect("a cargo variant fits a byte"),
                f32::from_bits(reward[i]),
            )
        })
        .collect();
    let field: Vec<f32> = graph_buffer(entry, &*state, "field_pheromone_cells")
        .into_iter()
        .map(f32::from_bits)
        .collect();
    let cells = (GATE_W * GATE_H) as usize;
    (agents, field[..cells].to_vec(), field[cells..].to_vec())
}

/// `ants.toml` on the device equals the stock CPU ants under the cross-engine gate of `consistency_ants.rs`: with
/// momentum and random action off and a tie-free seeded field, every ant's cell, last step, load and reward, and every
/// cell of both layers, bit for bit after the gate's four ticks, at four seeds.
#[test]
fn the_ants_starter_equals_the_stock_ants_under_the_gate() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    let entry = planted_gate_entry();
    let expected = cpu_gate(Some(1));
    for seed in [None, Some(1), Some(2), Some(99)] {
        let (agents, to_food, to_home) = graph_gate(&ctx, &entry, seed);
        for (i, (found, want)) in agents.iter().zip(&expected.0).enumerate() {
            assert_eq!(
                (found.0, found.1, found.2, found.3, found.4.to_bits()),
                (want.0, want.1, want.2, want.3, want.4.to_bits()),
                "ant {i} at seed {seed:?}"
            );
        }
        for (layer, found, want) in [("to_food", &to_food, &expected.1), ("to_home", &to_home, &expected.2)] {
            for (cell, (found, want)) in found.iter().zip(want).enumerate() {
                assert_eq!(
                    found.to_bits(),
                    want.to_bits(),
                    "{layer} cell {cell} at seed {seed:?}: {found} on the device, {want} on the CPU"
                );
            }
        }
    }
}

/// Returns the cells of a square world of side `side` that some ant of `pos`, an interleaved position lane, stands on.
fn occupied(pos: &[u32], side: u32) -> Vec<bool> {
    let mut mask = vec![false; (side * side) as usize];
    for xy in pos.chunks_exact(2) {
        let (x, y) = (f32::from_bits(xy[0]) as usize, f32::from_bits(xy[1]) as usize);
        mask[y * side as usize + x] = true;
    }
    mask
}

/// Side of the world of the decay check and the consistency checks, as `consistency_ants.rs` sets it.
const CHECK_SIDE: u32 = 100;
/// Evaporation factor of those checks, low enough that decay shows within a short run.
const CHECK_EVAPORATION: f32 = 0.99;

/// Builds `ants.toml` on the device as `consistency_ants.rs` builds the stock ants.
fn check_state(ctx: &GpuContext, entry: &ModelEntry) -> Box<dyn GpuSimState> {
    let params = params_for(
        entry,
        &[
            ("num_agents", ParamValue::U32(400)),
            ("world_width", ParamValue::F32(CHECK_SIDE as f32)),
            ("world_height", ParamValue::F32(CHECK_SIDE as f32)),
            ("evaporation", ParamValue::F32(CHECK_EVAPORATION)),
        ],
    );
    build_gpu(entry, &params, Some(20_260_806), ctx)
}

/// Every cell of `ants.toml` that no ant stands on decays by exactly the evaporation factor each tick, floored at
/// [`LOW_PHEROMONE`], as `consistency_ants.rs` holds the stock ants.
#[test]
fn cells_without_an_ant_decay_by_exactly_the_evaporation_rate_on_the_device() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    let entry = starter_entry("ants.toml");
    let lanes = GraphLanes::of_starter("ants.toml");
    let mut state = check_state(&ctx, &entry);
    step(&ctx, &mut *state, 10);
    let mut trail = 0usize;
    for tick in 0..40 {
        let before = graph_buffer(&entry, &*state, "field_pheromone_cells");
        let ants = occupied(&lanes.read(&*state, "pos"), CHECK_SIDE);
        step(&ctx, &mut *state, 1);
        let after = graph_buffer(&entry, &*state, "field_pheromone_cells");
        let mut checked = 0usize;
        for (i, (&held, &now)) in before.iter().zip(&after).enumerate() {
            if ants[i % ants.len()] {
                continue;
            }
            let decayed = f32::from_bits(held) * CHECK_EVAPORATION;
            let expected = if decayed < LOW_PHEROMONE { 0.0 } else { decayed };
            assert_eq!(
                now,
                expected.to_bits(),
                "layer {} cell {} held {} then {} at tick {tick}, and {expected} was expected",
                i / ants.len(),
                i % ants.len(),
                f32::from_bits(held),
                f32::from_bits(now)
            );
            checked += 1;
            trail += usize::from(held != 0);
        }
        assert!(checked > 0, "no unoccupied cells to check at tick {tick}");
    }
    assert!(trail > 0, "no cell held a trail, so nothing decayed");
}

/// `ants.toml` on the device meets the other checks of `consistency_ants.rs`: every ant stays on the integer lattice,
/// inside the world and off every obstacle, its load is 0 or 1, and Deliveries never decreases.
#[test]
fn the_ants_starter_meets_the_consistency_checks_on_the_device() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    let entry = starter_entry("ants.toml");
    let lanes = GraphLanes::of_starter("ants.toml");
    let mut state = check_state(&ctx, &entry);
    let sites = graph_buffer(&entry, &*state, "field_sites_cells");
    let obstacle = u32::from(OBSTACLE);
    assert!(sites.contains(&obstacle), "the world holds an obstacle");
    let deliveries = |stats: &[StatEntry]| stat(stats, "Deliveries");
    snapshot(&ctx, &mut *state);
    let mut previous = deliveries(&state.stats());
    for tick in 0..40 {
        step(&ctx, &mut *state, 1);
        let pos = lanes.read(&*state, "pos");
        let cargo = lanes.read(&*state, "cargo");
        for (i, xy) in pos.chunks_exact(2).enumerate() {
            let (x, y) = (f32::from_bits(xy[0]), f32::from_bits(xy[1]));
            let side = CHECK_SIDE as f32;
            assert!(
                (0.0..side).contains(&x) && (0.0..side).contains(&y),
                "ant {i} at ({x}, {y}) left the world at tick {tick}"
            );
            assert!(
                x.fract() == 0.0 && y.fract() == 0.0,
                "ant {i} at ({x}, {y}) came off the lattice at tick {tick}"
            );
            let cell = y as usize * CHECK_SIDE as usize + x as usize;
            assert_ne!(sites[cell], obstacle, "ant {i} stood on an obstacle at tick {tick}");
            assert!(cargo[i] <= 1, "ant {i} holds load {} at tick {tick}", cargo[i]);
        }
        snapshot(&ctx, &mut *state);
        let now = deliveries(&state.stats());
        assert!(
            now >= previous,
            "deliveries fell from {previous} to {now} at tick {tick}"
        );
        previous = now;
    }
}
