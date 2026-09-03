---
title: GPU Agent model
description: Take the ant colony to the GPU as a Henad GPU agent model, where a step becomes a list of compute passes.
icon: material/expansion-card-variant
---

# Writing a GPU agent model

In this tutorial we'll take the Ant Foraging model we built on the CPU to the GPU from scratch, where the colony lives in storage buffers, a step is a list of compute passes, and neither the ants nor the field ever visit the CPU.

This page assumes you have worked through our [CPU Agent model](ants.md) and [GPU Grid model](gpu-game-of-life.md) tutorials first.
The rules of the model, the generated bindings, the `build.rs` step and the way a GPU model is registered all carry over from those two pages, and we won't repeat them here.

## Moving from lanes to buffers

Assuming that you just built both, here's a quick comparison of what changes when an agent model moves to the GPU.

|                     | CPU agent model                          | GPU agent model                                            |
| ------------------- | ---------------------------------------- | ---------------------------------------------------------- |
| State               | one `Vec` per attribute, a **lane**      | one storage buffer per attribute, declared with `buffers!` |
| Transition          | a closure, called once per agent         | a compute pass, one invocation per agent                   |
| Step                | `run_deposit_pass`, then `run_step_pass` | a _list_ of passes, run in declaration order               |
| Field               | a `ScalarField` the engine owns          | three buffers of ours, and a pass that merges them         |
| Many ants, one cell | `ScatterGrid` with `Combine::Max`        | `atomicMax` into an accumulator buffer                     |
| Random numbers      | `chunk_seed` and `xorshift64`            | a per-ant `pcg_hash` state buffer                          |
| Counting            | a per-chunk tally, merged as you go      | a persistent counter the kernel adds into                  |

``` mermaid
flowchart LR
    F["field at tick N"] --> S["<code>step.wgsl</code><br>once per ant"]
    S -->|"atomicMax deposits"| ACC["accum"]
    ACC --> M["<code>merge.wgsl</code><br>once per cell per layer"]
    F --> M
    M --> F1["field at tick N+1"]
```

Let's get started by creating a directory `crates/henad-models/src/gpu_foraging/`, containing `mod.rs`, `state.wgsl`, `step.wgsl`, `merge.wgsl`, `display.wgsl` and `reduce.wgsl`.

## Agent states (buffers)

Let's start by declaring the state again, this time as buffers.

Where the CPU model kept one `Vec` per attribute, the GPU model keeps one storage buffer per attribute, and the `buffers!` macro declares them the way `agent_lanes!` declared lanes.

``` { .rust .annotate title="crates/henad-models/src/gpu_foraging/mod.rs" }
henad_core::buffers! {
    const POS = "pos" drawable; // (1)!
    const STATE = "state"; // (2)!
    const COLOR = "color" drawable; // (3)!
    const RNG = "rng"; // (4)!
    const FIELD = "field"; // (5)!
    const ACCUM = "accum";
    const SITES = "sites";
}
```

1. Positions, as one `vec2<f32>` per ant. The `drawable` flag marks a buffer the renderer reads directly as a vertex stream, so the ants are drawn straight out of it with no copy.
2. One word per ant, packing what the CPU kept in three lanes. More on this [below](#packing-an-ant).
3. Packed RGBA per ant, also drawable. The CPU lane held a palette index, this one holds the colour itself, since a GPU model draws its agents directly.
4. One random number generator state per ant. We come back to it under [Testing](#testing).
5. The field, in three buffers: the two pheromone layers, two matching accumulator layers, and one terrain word per cell.

`buffers!` works like the `params!` macro we have used twice already.
Each entry gets a `const` holding its index, derived from declaration order, and the string is the label a shader's binding names refer to.
The macro also emits `SPECS`, the list the trait reads.

### Buffer flags

There are two flags, `double_buffered` and `drawable`, and both default to off.

A buffer marked `double_buffered` is, well... double-buffered, and is used for a pass that reads the previous values while writing this tick's, exactly like a `dual` lane.
The shipped GPU boids marks all three of its buffers so, because a boid reads its neighbours' current positions while writing its own next one.

Ants marks nothing double buffered, because no ant ever reads another ant's slots, and deposits land in `accum` rather than in the field the step is reading.
The engine only builds a second side for buffers that ask for one, so this model pays nothing for the ping-pong machinery the grid model needed.

### Packing an ant

The CPU model kept `last_step`, `has_food` and `reward` in three separate lanes.
The GPU model packs all three into the one `state` word:

``` { .rust .annotate title="crates/henad-models/src/gpu_foraging/mod.rs" }
/// `state` packs what the CPU model keeps in three lanes. Mirrored in `state.wgsl`.
const HAS_FOOD_BIT: u32 = 0b01_00000000; // 0x100
const HAS_REWARD_BIT: u32 = 0b10_00000000; // 0x200

/// The three per-ant scalars the CPU keeps in separate lanes, as `step.wgsl` reads them.
fn pack_state(lanes: &AntLanes, i: usize) -> u32 {
    let mut packed = u32::from(lanes.last_step[i]); // (1)!
    if lanes.has_food[i] != 0 {
        packed |= HAS_FOOD_BIT;
    }
    if lanes.reward[i] != 0.0 { // (2)!
        packed |= HAS_REWARD_BIT;
    }
    packed
}
```

1. The direction code fits a byte, so it takes the low eight bits, with `NO_STEP` at 255 still clear of every real encoding.
2. `reward` was an `f32` on the CPU and survives here as a single bit. That works because of a property of the CPU model: its reward lane only ever holds `0.0` or the reward parameter, nothing in between, so "has a reward" plus the parameter reconstructs the value exactly.

Why pack at all?
Count the step pass's bindings [below](#pass-1-stepping): 7 buffers plus the engine's counters is 8 storage bindings, which is precisely the WebGPU baseline limit per shader stage.
Packing allows our model to stay under that limit, and hence run on nearly all WebGPU devices!

The WGSL side of the packing lives in a small file both the step and the reduce shader import:

``` { .wgsl .annotate title="crates/henad-models/src/gpu_foraging/state.wgsl" }
#define_import_path gpu_foraging::state // (1)!

// `state` packs the three per-ant scalars the CPU keeps in separate lanes. `reward` is a bit
// rather than an f32 because the CPU model only ever stores 0.0 or the reward param in it.
const LAST_STEP_MASK: u32 = 0xFFu;
const HAS_FOOD_BIT: u32 = 0x100u;
const HAS_REWARD_BIT: u32 = 0x200u;
```

1. A file with an import path is a module other shaders can `#import` from, through the same mechanism that gives us `shared::rng`. It is not an entry point, so it does not go in `build.rs`.

## Fields

Apart from the ants, we still need the pheromone trails and the terrain.
On the CPU we handed those to a `ScalarField` and wrote a `ScalarFieldSpec` describing them.
On the GPU the field is the three buffers we just declared, and the spec's four jobs land in three places:

| `ScalarFieldSpec` on the CPU | On the GPU                                                   |
| ---------------------------- | ------------------------------------------------------------ |
| `COMBINE = Combine::Max`     | `atomicMax` into `accum`, in the step shader                 |
| `decay`                      | `merge.wgsl`, one invocation per cell per layer              |
| `build_sites`                | the `sites` buffer, filled from the CPU spec's `build_sites` |
| `quantize`                   | `display.wgsl`, one invocation per texel                     |

The scatter comes with the step shader further down, and the terrain arrives with seeding.
Let's write the other two now.

### Merging and decay `merge.wgsl`

After every ant has run, a second pass folds the accumulated deposits into the field and decays it.
This is `ScalarField::update` in miniature:

``` { .wgsl .annotate title="crates/henad-models/src/gpu_foraging/merge.wgsl" }
#import shared::prelude::linear_index // (1)!

struct Params { // (2)!
    n: u32,
    groups_x: u32,
    evaporation: f32,
    low: f32,
}

@group(0) @binding(0) var<storage, read_write> field: array<f32>;
// `atomic<u32>` in the step shader. Only one invocation touches each entry here.
@group(0) @binding(1) var<storage, read_write> accum: array<u32>; // (3)!
@group(0) @binding(2) var<uniform> params: Params;

@compute
@workgroup_size(256) // (4)!
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let i = linear_index(lid, wid, params.groups_x);
    if (i >= params.n) {
        return;
    }

    let deposited = bitcast<f32>(accum[i]); // (5)!
    accum[i] = 0u;

    // Decay after the merge, so a fresh deposit is already one step old when read.
    var v = max(field[i], deposited) * params.evaporation;
    if (v < params.low) {
        // Without the floor a trail never disappears, it just asymptotes.
        v = 0.0;
    }
    field[i] = v;
}
```

1. An agent pass folds its linear invocation domain onto a 2D grid of workgroups, because a large domain overflows one row of them. `linear_index` from the shared prelude does the fold, and needs the fold width `groups_x` from the uniform.
2. A uniform block is a struct we design, and `build.rs` generates its Rust twin, which we fill in under [Uniforms](#uniforms).
3. The step shader writes `accum` through atomics, and here it is bound as a plain `u32` array. Only one invocation touches each entry in this pass, so it can reset the slot for the next tick with an ordinary store.
4. Every agent pass declares 256, and every display pass declares a square, exactly as the grid model's shaders all declared 16 by 16.
5. Deposits are stored as the bit pattern of an `f32`, for a reason the step shader makes clear.

The order matches the CPU too, decay after the merge, so a fresh deposit is already one step old when the next tick reads it.

### Display `display.wgsl`

Displaying the field is `quantize` moved into WGSL, one invocation per texel, choosing the stronger trail and looking the result up in the same log-scaled ramp:

``` { .wgsl .annotate title="crates/henad-models/src/gpu_foraging/display.wgsl" }
// Quantizes the field into the display texture, mirroring `PheromoneField::quantize`.

struct Params {
    width: u32,
    height: u32,
    n_cells: u32,
    _pad: u32, // (1)!
    // Under the cell grid on a large world.
    tex: vec2<u32>,
    _pad2: vec2<u32>,
    // `ants::field::CELL_PALETTE`, packed so the colours cannot drift from the CPU model's.
    palette: array<vec4<u32>, 4>, // (2)!
}

@group(0) @binding(0) var<storage, read> field: array<f32>;
@group(0) @binding(1) var<storage, read> sites: array<u32>;
@group(0) @binding(2) var output: texture_storage_2d<rgba8unorm, write>; // (3)!
@group(0) @binding(3) var<uniform> params: Params;

const OBSTACLE: u32 = 1u;
const FOOD: u32 = 2u;
const HOME: u32 = 3u;

const LOW_PHEROMONE: f32 = 1e-14;
const DISPLAY_DECADES: f32 = 3.0;
const RAMP_STEPS: f32 = 6.0;
const INV_LOG2_10: f32 = 0.30103; // (4)!

fn ramp_step(v: f32) -> u32 {
    if (v <= LOW_PHEROMONE) {
        return 0u;
    }
    let decades = log2(v) * INV_LOG2_10 / DISPLAY_DECADES + 1.0;
    if (decades <= 0.0) {
        return 0u;
    }
    return clamp(u32(clamp(decades * RAMP_STEPS, 0.0, RAMP_STEPS)), 1u, u32(RAMP_STEPS));
}

@compute
@workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    if (global_id.x >= params.tex.x || global_id.y >= params.tex.y) {
        return;
    }

    // One invocation per texel, which is one cell until the world outgrows the texture cap.
    let x = global_id.x * params.width / params.tex.x;
    let y = global_id.y * params.height / params.tex.y;

    let c = y * params.width + x;
    let site = sites[c];

    var index = 0u;
    if (site == OBSTACLE) {
        index = 13u;
    } else if (site == FOOD) {
        index = 14u;
    } else if (site == HOME) {
        index = 15u;
    } else {
        // Stronger route wins the cell, so overlapping trails stay legible.
        let to_food = field[c];
        let to_home = field[params.n_cells + c]; // (5)!
        var v = to_home;
        var base = 0u;
        if (to_food > to_home) {
            v = to_food;
            base = 6u;
        }
        let step = ramp_step(v);
        if (step != 0u) {
            index = base + step;
        }
    }

    let color = unpack4x8unorm(params.palette[index >> 2u][index & 3u]);
    textureStore(output, vec2<i32>(global_id.xy), color);
}
```

1. Uniform layout rules align a `vec2` to 8 bytes and a `vec4` to 16, and the padding fields make that alignment explicit rather than leaving it to the compiler. The generated Rust struct carries the same fields, so the two sides cannot be laid out differently.
2. Sixteen colours, packed four to a `vec4<u32>`. Unlike the grid model's display shader, the palette is not baked into the WGSL. It arrives through the uniform, packed on the Rust side from the CPU model's `CELL_PALETTE`, so the sixteen colours cannot drift between the backends. With sixteen entries that is worth the plumbing, where two baked constants were fine.
3. `output` is one of the engine's reserved binding names, along with `params`, `dims`, `counters` and `partials`. Anything else names one of our buffers by its label.
4. WGSL has `log2` and no `log10`, hence the constant.
5. The two layers sit end to end in one buffer, to-food first, so the second layer starts `n_cells` in.

## Implementing `GpuAgentModel`

This follows the same shape as [Implementing `GpuGridModel`](gpu-game-of-life.md#implementing-gpugridmodel).

``` rust title="crates/henad-models/src/gpu_foraging/mod.rs"
use henad_core::authoring::model::gpu_agent_model::GpuAgentModel;

pub struct GpuForagingModel;

impl GpuAgentModel for GpuForagingModel {}
```

``` text title="cargo check -p henad-models"
error[E0046]: not all trait items implemented, missing: `NAME`, `ID`, `DESCRIPTION`, `STATS`, `BUFFERS`,
              `POS_BUFFER`, `COLOR_BUFFER`, `STEP_PASSES`, `REDUCE`, `param_descriptors`, `dims`,
              `buffer_lens`, `seed_buffers`, `pass_params_bytes`, `stats`
 --> crates/henad-models/src/gpu_foraging/mod.rs:5:1
  |
5 | impl GpuAgentModel for GpuForagingModel {}
  | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ missing 15 items in implementation
```

Three more items have defaults and so are missing from the list: `INDEX`, `COUNTERS` and `DISPLAY`.
Ants needs two of the three.

### Identity and Metadata

``` { .rust .annotate title="crates/henad-models/src/gpu_foraging/mod.rs" }
impl GpuAgentModel for GpuForagingModel {
    const NAME: &'static str = "Ant Foraging (GPU)";
    const ID: &'static str = "gpu_foraging"; // (1)!
    const DESCRIPTION: &'static str =
        "Ants lay and follow pheromone trails between a nest and a food source, stepped entirely on the GPU";
    const STATS: &'static [StatDescriptor] = ForagingModel::STATS; // (2)!

    const BUFFERS: &'static [BufferSpec] = SPECS; // (3)!
    const POS_BUFFER: usize = POS; // (4)!
    const COLOR_BUFFER: usize = COLOR;

    const COUNTERS: usize = 1; // (5)!
}
```

1. The shipped port already uses the id `gpu_ants`, and ids have to be unique across the registry.
2. Reused wholesale from the CPU model, so both backends chart the same three series in the same colours.
3. The list `buffers!` emitted.
4. Which two of the drawable buffers the renderer reads.
5. Persistent `u32` counters a kernel accumulates into, and nothing ever clears. One, for deliveries. A reduction is recomputed from current state each time, while deliveries are events that already happened, so they accumulate here instead, exactly as the `Tally` did on the CPU.

`INDEX` keeps its default of `false`.
Our ants read the field and never each other, so the engine builds no neighbour index, just as `NoIndex` told it on the CPU.

Here are the imports that `impl` relies on, the whole of the trait's vocabulary included, since we meet the rest of it further down:

``` rust title="crates/henad-models/src/gpu_foraging/mod.rs"
use henad_core::authoring::model::gpu_agent_model::{
    BufferSpec, DisplaySpec, Domain, Geometry, GpuAgentModel, PassCtx, PassId, PassSpec, ReduceSpec,
};
use henad_core::view::{StatDescriptor, StatValue};

use crate::foraging::{ANT_PALETTE, AntLanes, ForagingModel};
```

### Parameters

The model declares no parameters of its own.
Unlike a CPU agent model, nothing is prepended to a GPU model's list, so we hand back the CPU model's composed list verbatim:

``` rust title="crates/henad-models/src/gpu_foraging/mod.rs"
    fn param_descriptors() -> Vec<ParamDescriptor> {
        agent_model_param_descriptors::<ForagingModel>()
    }
```

`agent_model_param_descriptors` builds exactly the list the CPU engine composed for `ForagingModel`, the engine's three followed by the model's four and the field's one.
The shipped port declares the same list, so its output shows us what to expect:

``` text title="cargo run -p henad-cli -- gpu_ants --params" hl_lines="2 3 4"
parameters for gpu_ants (Ant Foraging (GPU)):
  index=0 id=num_agents kind=u32 default=2000 min=1 max=5000000 apply=reload label="Number of Agents"
  index=1 id=world_width kind=f32 default=200 min=1 max=10000 apply=reload label="World Width"
  index=2 id=world_height kind=f32 default=200 min=1 max=10000 apply=reload label="World Height"
  index=3 id=update_cutdown kind=f32 default=0.9 min=0.5 max=1 apply=reload label="Trail Falloff"
  index=4 id=reward kind=f32 default=1 min=0.1 max=10 apply=reload label="Site Reward"
  index=5 id=momentum kind=f32 default=0.8 min=0 max=1 apply=reload label="Momentum Probability"
  index=6 id=random_action kind=f32 default=0.1 min=0 max=1 apply=reload label="Random Action Probability"
  index=7 id=evaporation kind=f32 default=0.999 min=0.9 max=1 apply=reload label="Evaporation"
```

Both backends therefore take the same parameter vector, and a slider in the app means the same thing on either one.
Every entry says `apply=reload`, the four the CPU model applies live included.
A GPU state rejects a live edit, and the engine marks the whole list accordingly.

### Sizes

``` { .rust .annotate title="crates/henad-models/src/gpu_foraging/mod.rs" }
    fn dims(params: &[ParamValue]) -> (u32, Extent) { // (1)!
        (
            extract_u32(params, NUM_AGENTS, ForagingModel::DEFAULT_AGENTS),
            Extent {
                w: extract_f32(params, WORLD_WIDTH, ForagingModel::DEFAULT_EXTENT.w),
                h: extract_f32(params, WORLD_HEIGHT, ForagingModel::DEFAULT_EXTENT.h),
            },
        )
    }

    fn buffer_lens(geom: &Geometry) -> Vec<usize> { // (2)!
        let n = geom.num_agents as usize;
        let cells = geom.n_cells as usize;
        vec![n * 2, n, n, n, cells * 2, cells * 2, cells]
    }
```

1. The engine's own three parameters, read back by the indices `cpu::agent_engine` gives them, with the defaults coming from the CPU model too.
2. One length per buffer, in `u32`-sized elements and in declaration order. Reading it against the `buffers!` block gives the full layout: two floats per ant for `pos`, one word each for `state`, `color` and `rng`, two layers of cells for `field` and `accum`, and one word per cell for `sites`.

From `dims` the engine resolves a `Geometry` once at construction, carrying the population, the extent, the cell grid, the number of cells and the display texture size, and hands it to everything below.

``` rust title="crates/henad-models/src/gpu_foraging/mod.rs"
use henad_compute::cpu::agent_engine::{
    AGENT_INIT_SEED, NUM_AGENTS, WORLD_HEIGHT, WORLD_WIDTH, agent_model_param_descriptors, split_params,
};
use henad_core::authoring::model::field::Extent;
use henad_core::helpers::{extract_f32, extract_u32};
use henad_core::params::{ParamDescriptor, ParamValue};
```

### Initialisation

On the CPU, `init` filled the lanes before tick 0.
Here we fill every buffer ourselves, on the CPU, and the engine uploads them once at construction.
How the colony _starts_ comes, as always, from the CPU model:

``` { .rust .annotate title="crates/henad-models/src/gpu_foraging/mod.rs" }
    fn seed_buffers(geom: &Geometry, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u8>> {
        let n = geom.num_agents as usize;
        let n_cells = geom.n_cells as usize;

        let mut lanes = AntLanes::alloc(n); // (1)!
        let mut rng_state = seed.map_or(AGENT_INIT_SEED, mix_seed);
        ForagingModel::init(
            &mut lanes,
            geom.extent,
            split_params::<ForagingModel>(params).0, // (2)!
            &mut rng_state,
        );

        let positions: Vec<f32> = lanes // (3)!
            .pos_x
            .iter()
            .zip(&lanes.pos_y)
            .flat_map(|(&x, &y)| [x, y])
            .collect();
        let packed: Vec<u32> = (0..n).map(|i| pack_state(&lanes, i)).collect();
        let colors: Vec<u32> = lanes.has_food.iter().map(|&f| packed_ant_color(f)).collect(); // (4)!
        let rng_seed = seed.map_or(RNG_INIT_SEED, |s| mix_seed(s ^ RNG_INIT_SEED)); // (5)!

        let mut site_bytes = vec![EMPTY; n_cells];
        PheromoneField::build_sites(geom.width, geom.height, &mut site_bytes); // (6)!
        let site_words: Vec<u32> = site_bytes.iter().map(|&s| u32::from(s)).collect();

        vec![
            bytemuck::cast_slice(&positions).to_vec(), // (7)!
            bytemuck::cast_slice(&packed).to_vec(),
            bytemuck::cast_slice(&colors).to_vec(),
            bytemuck::cast_slice(&seed_rng_states(n, rng_seed)).to_vec(),
            Vec::new(), // (8)!
            Vec::new(),
            bytemuck::cast_slice(&site_words).to_vec(),
        ]
    }
```

1. We allocate the CPU model's lanes and run its `init` on them. That one call makes tick 0 bit identical between the backends, same positions and same rewards, and a port that reimplemented `init` would be free to drift. Keep it confined to this function.
2. `split_params` divides the composed list into the model's slice and the field's slice, so `init` sees the same slice it saw on the CPU.
3. The rest of the function is packing, turning lanes into the layouts the shaders read. Positions interleave into `vec2<f32>`.
4. The CPU's palette-index colour lane becomes packed RGBA, since this model draws its colours directly.
5. The RNG buffer gets a seed deliberately separated from the ant-position stream, so the two do not start correlated.
6. Through the CPU field spec, so the two backends cannot place the nest, the food or the obstacles differently.
7. Raw bytes rather than `u32`, because agent buffers hold mixed types. `bytemuck::cast_slice` reinterprets a slice of floats or words as bytes without copying.
8. An empty vector leaves a buffer cleared, which is exactly right for a field that starts with no pheromone and an accumulator the merge resets anyway.

The helpers this leans on are small:

``` { .rust .annotate title="crates/henad-models/src/gpu_foraging/mod.rs" }
/// Domain separated from the ant seeding stream, so the two do not start correlated.
const RNG_INIT_SEED: u64 = AGENT_INIT_SEED ^ 0x5EED_5EED_5EED_5EED;

/// Matches `pcg_hash` in `shared::rng` bit for bit, since `u32` arithmetic wraps the same on both sides.
fn pcg_hash(input: u32) -> u32 { // (1)!
    let state = input.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28).wrapping_add(4))) ^ state).wrapping_mul(277_803_737);
    (word >> 22) ^ word
}

fn seed_rng_states(n: usize, seed: u64) -> Vec<u32> {
    let seed32 = (seed ^ (seed >> 32)) as u32;
    (0..n).map(|i| pcg_hash(seed32 ^ i as u32)).collect()
}

fn packed_ant_palette() -> [u32; 2] { // (2)!
    [u32::from_le_bytes(ANT_PALETTE[0]), u32::from_le_bytes(ANT_PALETTE[1])]
}

fn packed_ant_color(index: u8) -> u32 {
    let rgba = ANT_PALETTE.get(index as usize).copied().unwrap_or(ANT_PALETTE[0]);
    u32::from_le_bytes(rgba)
}

/// Indexed as `palette[i >> 2][i & 3]` in `display.wgsl`.
fn packed_cell_palette() -> [[u32; 4]; 4] {
    let mut packed = [[0u32; 4]; 4];
    for (i, rgba) in CELL_PALETTE.iter().enumerate() {
        packed[i / 4][i % 4] = u32::from_le_bytes(*rgba);
    }
    packed
}
```

1. WGSL has no 64-bit integers, so the GPU cannot run `xorshift64`. Its generator is `pcg_hash` over `u32`, from `shared::rng`, and this is the same hash in Rust so the buffer can be seeded to a known first state.
2. Both palettes are packed from the CPU model's constants rather than retyped, one for the step uniform and one for the display uniform.

``` rust title="crates/henad-models/src/gpu_foraging/mod.rs"
use henad_compute::cpu::field::scalar::ScalarFieldSpec as _;
use henad_core::authoring::model::agent_model::{AgentLanes as _, AgentModel as _};
use henad_core::authoring::primitives::rng::mix_seed;

use crate::foraging::field::{CELL_PALETTE, EMPTY, LOW_PHEROMONE, PheromoneField};
```

The two `as _` imports bring `alloc`, `init` and `build_sites` into scope without naming the traits, since nothing here calls them by name.

### Tick lifecycle

Here is the lifecycle of a tick for a GPU agent model.
We only need to write the passes, and the rest is handled by the engine.

|     | Stage                                                         | Implementation                |
| --- | ------------------------------------------------------------- | ----------------------------- |
| 1   | Every ant deposits and moves, one invocation each             | **Pending**, in `step.wgsl`   |
| 2   | Deposits merge into the field, then the field decays          | `merge.wgsl`, written above   |
| 3   | Double buffered buffers swap                                  | Engine, nothing to swap here  |
|     | _On publish rather than every tick_                           |                               |
| 4   | The field is drawn into the display texture                   | `display.wgsl`, written above |
| 5   | The reduce leaf runs, the tree folds it, and values read back | **Pending**, in `reduce.wgsl` |

The uniform block of every pass is filled once, when the model is built, and a neighbour index would be rebuilt before stage 1 if `INDEX` asked for one.

The passes are declared as data, in the order they run:

``` { .rust .annotate title="crates/henad-models/src/gpu_foraging/mod.rs" }
    const STEP_PASSES: &'static [PassSpec] = &[
        PassSpec {
            label: "step",
            shader: crate::shader_bindings::gpu_foraging::step::SHADER_STRING,
            bindings: crate::binding_decls::bindings::GPU_FORAGING_STEP,
            domain: Domain::Agents, // (1)!
        },
        PassSpec {
            label: "merge",
            shader: crate::shader_bindings::gpu_foraging::merge::SHADER_STRING,
            bindings: crate::binding_decls::bindings::GPU_FORAGING_MERGE,
            domain: Domain::Cells(2), // (2)!
        },
    ];

    const DISPLAY: Option<DisplaySpec> = Some(DisplaySpec { // (3)!
        shader: crate::shader_bindings::gpu_foraging::display::SHADER_STRING,
        bindings: crate::binding_decls::bindings::GPU_FORAGING_DISPLAY,
        workgroup: 16,
    });

    const REDUCE: ReduceSpec = ReduceSpec { // (4)!
        shader: crate::shader_bindings::gpu_foraging::reduce::SHADER_STRING,
        bindings: crate::binding_decls::bindings::GPU_FORAGING_REDUCE,
        lanes: 2,
        domain: Domain::AgentsOrCells, // (5)!
    };
```

1. One invocation per agent.
2. `n` invocations per cell, for a field with `n` layers. Two layers, so `merge` runs over every cell of both.
3. Declared because this model draws a grid underneath its agents. A model with agents in empty space, like the shipped GPU boids, leaves it `None`.
4. A two-lane reduction, one lane counting carrying ants and one summing the field. We write only the leaf, and the engine owns every level of the reduction tree above it.
5. The two lanes live on different domains, one per ant and one per cell, so the pass dispatches over whichever is longer.

As on the grid page, the shaders have to be listed in the crate's `build.rs` before `shader_bindings` knows about them.
Four entries this time, since `state.wgsl` is a module rather than an entry point:

``` rust title="crates/henad-models/build.rs"
    "gpu_foraging/step.wgsl",
    "gpu_foraging/merge.wgsl",
    "gpu_foraging/display.wgsl",
    "gpu_foraging/reduce.wgsl",
```

#### Pass 1: stepping

Now for the heart of the model.
One invocation is one ant, and the kernel mirrors `advect_agent` and `deposit_value` from the CPU page closely enough to read side by side.
It opens with its imports, its uniform and its bindings:

``` { .wgsl .annotate title="crates/henad-models/src/gpu_foraging/step.wgsl" }
#import shared::prelude::linear_index
#import shared::rng::{choice3, next_bits, next_float, reservoir_accept} // (1)!
#import gpu_foraging::state::{LAST_STEP_MASK, HAS_FOOD_BIT, HAS_REWARD_BIT}

struct Params {
    num_agents: u32,
    groups_x: u32,
    grid_w: u32,
    grid_h: u32,

    n_cells: u32,
    cutdown: f32,
    diagonal: f32,
    reward: f32,

    momentum: f32,
    random_action: f32,
    // Searching and carrying, in the uniform to keep a storage binding free.
    palette: vec2<u32>, // (2)!
}

@group(0) @binding(0) var<storage, read_write> pos: array<vec2<f32>>; // (3)!
@group(0) @binding(1) var<storage, read_write> state: array<u32>;
@group(0) @binding(2) var<storage, read_write> color: array<u32>;
@group(0) @binding(3) var<storage, read_write> rng: array<u32>;
@group(0) @binding(4) var<storage, read>       field: array<f32>;
@group(0) @binding(5) var<storage, read_write> accum: array<atomic<u32>>; // (4)!
@group(0) @binding(6) var<storage, read>       sites: array<u32>;
@group(0) @binding(7) var<storage, read_write> counters: array<atomic<u32>>; // (5)!
@group(0) @binding(8) var<uniform>             params: Params;
```

1. The same four draws we used on the CPU, as their WGSL twins. Each pair is pinned to the other by a parity test.
2. The hot parameters, the same ones `from_params` derived on the CPU, `diagonal` included. The two ant colours ride along in the uniform to keep a storage binding free.
3. For an agent model the engine resolves each binding _by name_. `pos` binds the buffer labelled "pos", and the slot indices come from the shader itself at build time, so Rust and WGSL cannot disagree about the layout. `field` and `sites` are read-only here, and that is the whole guarantee the fused pass rests on.
4. `accum` is declared atomic, since many ants write it.
5. `counters` is one of the engine's reserved names, holding the persistent counters `COUNTERS` asked for. Eight storage bindings in all, the baseline limit.

A few constants and helpers follow, mirroring their CPU namesakes:

``` { .wgsl .annotate title="crates/henad-models/src/gpu_foraging/step.wgsl" }
// Matches `ants::field`.
const OBSTACLE: u32 = 1u;
const FOOD: u32 = 2u;
const HOME: u32 = 3u;
const TO_FOOD: u32 = 0u;
const TO_HOME: u32 = 1u;

// Matches `ants::lanes::NO_STEP`.
const NO_STEP: u32 = 255u;

const DELIVERIES: u32 = 0u; // (1)!

fn cell_of(x: i32, y: i32) -> u32 {
    return u32(y * i32(params.grid_w) + x);
}

fn in_field(x: i32, y: i32) -> bool {
    return x >= 0 && y >= 0 && x < i32(params.grid_w) && y < i32(params.grid_h);
}

// This model is bounded, not toroidal like the others.
fn passable(x: i32, y: i32) -> bool { // (2)!
    return in_field(x, y) && sites[cell_of(x, y)] != OBSTACLE;
}
```

1. The index of our one counter.
2. Every candidate cell funnels through here, including both fallback paths below, exactly as on the CPU. Forgetting the bounds check in one of the fallbacks is still the easiest bug this page can produce.

Rule 2, how much pheromone to lay, is `deposit_value` ported line for line:

``` { .wgsl .annotate title="crates/henad-models/src/gpu_foraging/step.wgsl" }
// Mirrors `ants::step::deposit_value`. Floored at what the cell already holds, which is why
// `atomicMax` downstream reproduces the reference's plain overwrite.
fn deposit_value(x: i32, y: i32, reward: f32, base: u32) -> f32 {
    var best = field[base + cell_of(x, y)]; // (1)!
    for (var dx = -1; dx <= 1; dx = dx + 1) {
        for (var dy = -1; dy <= 1; dy = dy + 1) {
            let nx = x + dx;
            let ny = y + dy;
            if (!in_field(nx, ny)) {
                continue;
            }
            var cut = params.cutdown;
            if (dx * dy != 0) {
                cut = params.diagonal;
            }
            best = max(best, field[base + cell_of(nx, ny)] * cut + reward);
        }
    }
    return best;
}
```

1. `base` selects the layer inside the one `field` buffer. The loop visits the centre cell too, at `dx == 0` and `dy == 0`, which covers the `here * cutdown + reward` term the CPU wrote out separately.

The entry point recovers the ant index and unpacks the state word:

``` { .wgsl .annotate title="crates/henad-models/src/gpu_foraging/step.wgsl" }
@compute
@workgroup_size(256)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let i = linear_index(lid, wid, params.groups_x); // (1)!
    if (i >= params.num_agents) {
        return;
    }

    let p = pos[i];
    let x = i32(p.x);
    let y = i32(p.y);
    let packed = state[i]; // (2)!
    let last_step = packed & LAST_STEP_MASK;
    let has_food = (packed & HAS_FOOD_BIT) != 0u;
    var reward = 0.0;
    if ((packed & HAS_REWARD_BIT) != 0u) {
        reward = params.reward;
    }
```

1. The fold from the merge shader again. A big population overflows one row of workgroups, so agent passes always dispatch as a rectangle and carry `groups_x` in their uniform.
2. Unpacking the state word gives back the three CPU lanes, with the reward reconstructed from its bit and the parameter.

Then the ant deposits, which on the CPU was the whole first pass:

``` { .wgsl .annotate title="crates/henad-models/src/gpu_foraging/step.wgsl" }
    // An ant lays the trail for the trip it just made and follows the one it is making, so
    // carrying food lays to-food and follows to-home.
    var lay = TO_HOME;
    var follow = TO_FOOD;
    if (has_food) {
        lay = TO_FOOD;
        follow = TO_HOME;
    }
    let lay_base = lay * params.n_cells;
    let value = deposit_value(x, y, reward, lay_base);
    // Non-negative f32 compares the same as its bit pattern, so an integer max is a float max.
    atomicMax(&accum[lay_base + cell_of(x, y)], bitcast<u32>(value)); // (1)!
```

1. This line is the GPU's answer to `ScatterGrid`, and it is doing something slightly sneaky. WGSL has no atomic max for floats, but a non-negative `f32` compares in the same order as its bit pattern does as an integer, so `bitcast<u32>` turns a float max into an integer one. Many ants writing one cell are resolved by hardware atomics, and it is only correct because the model combines deposits with `max`, which no ordering can change.

Choosing where to move is the same three-way logic as the CPU kernel, trail first, then momentum, then a random kick:

``` { .wgsl .annotate title="crates/henad-models/src/gpu_foraging/step.wgsl" }
    var r = rng[i]; // (1)!
    let trail_base = follow * params.n_cells;

    // An impossible pheromone, so the first passable neighbour always wins.
    var best = -1.0;
    var bx = x;
    var by = y;
    // 2 not 1 is the reference's off-by-one, giving the first neighbour visited 2/(k+1) against
    // 1/(k+1) for the rest, which drifts ants up-left. Kept deliberately, see the gap report.
    var count = 2u; // (2)!

    // The `dx` outer, `dy` inner order is load-bearing. Ties are broken by a reservoir draw, so
    // the visit order changes the outcome.
    for (var dx = -1; dx <= 1; dx = dx + 1) { // (3)!
        for (var dy = -1; dy <= 1; dy = dy + 1) {
            if (dx == 0 && dy == 0) {
                continue;
            }
            let nx = x + dx;
            let ny = y + dy;
            if (!passable(nx, ny)) {
                continue;
            }
            let m = field[trail_base + cell_of(nx, ny)];
            if (m > best) {
                count = 2u;
            }
            if (m > best || (m == best && reservoir_accept(next_bits(&r), count))) { // (4)!
                best = m;
                bx = nx;
                by = ny;
            }
            count = count + 1u;
        }
    }

    if (best == 0.0 && last_step != NO_STEP) {
        // No pheromone nearby, so probably keep going the way we were.
        if (next_float(&r, 1.0) < params.momentum) { // (5)!
            let mx = x + i32(last_step / 3u) - 1;
            let my = y + i32(last_step % 3u) - 1;
            if (passable(mx, my)) {
                bx = mx;
                by = my;
            }
        }
    } else if (next_float(&r, 1.0) < params.random_action) { // (6)!
        let dx = choice3(next_bits(&r));
        let dy = choice3(next_bits(&r)); // (7)!
        let mx = x + dx;
        let my = y + dy;
        if (!(dx == 0 && dy == 0) && passable(mx, my)) {
            bx = mx;
            by = my;
        }
    }
```

1. The ant's own generator state, loaded into a local and written back at the end. On the CPU a chunk's generator came from `chunk_seed`, and here every ant carries its own, since a shader has no chunk.
2. The same deliberate quirk as the CPU page, giving the first neighbour visited twice the odds of every other.
3. `dx` on the outside and `dy` on the inside, spelled out as two loops where the CPU walked `MOORE_COLUMN_MAJOR`. The order is the same, and it has to be, because ties are broken by a draw.
4. The reservoir draw, from `shared::rng`. The same call as on the CPU, over a 32-bit word.
5. The lost-ant branch, repeating the last step with probability `momentum`. The direction decodes inline where the CPU had `decode_step`.
6. Otherwise the small chance of ignoring the trail altogether.
7. Two draws, two separate `next_bits` calls, for the same reason as on the CPU.

Arrival closes the tick for this ant:

``` { .wgsl .annotate title="crates/henad-models/src/gpu_foraging/step.wgsl" }
    // The deposit above spent whatever the ant was carrying. Only a site grants more.
    var out_food = has_food;
    var out_reward = false;
    let site = sites[cell_of(bx, by)];
    if (site == HOME && has_food) { // (1)!
        out_food = false;
        out_reward = true;
        atomicAdd(&counters[DELIVERIES], 1u); // (2)!
    } else if (site == FOOD && !has_food) {
        out_food = true;
        out_reward = true;
    }

    var out_state = u32((bx - x + 1) * 3 + (by - y + 1)); // (3)!
    if (out_food) {
        out_state = out_state | HAS_FOOD_BIT;
    }
    if (out_reward) {
        out_state = out_state | HAS_REWARD_BIT;
    }

    pos[i] = vec2<f32>(f32(bx), f32(by)); // (4)!
    state[i] = out_state;
    rng[i] = r;
    color[i] = select(params.palette.x, params.palette.y, out_food);
}
```

1. Rule 4. Reaching a site flips what the ant carries and grants a fresh reward.
2. A delivery bumps the persistent counter. `atomicAdd` because any number of ants can arrive home in the same tick.
3. `encode_step` inline, then the two flags.
4. The last four lines write the ant's own slots back, including the colour the renderer draws it in next frame. Nothing here touches another ant's slot, and that is what lets every buffer stay single-sided.

#### Pass 2: merging

We wrote `merge.wgsl` under [Fields](#merging-and-decay-mergewgsl), and the second `PassSpec` above runs it after every ant has stepped, one invocation per cell per layer.
Nothing else is needed.
wgpu orders the two passes, so the merge sees every deposit of the tick.

### Uniforms

Each pass declared a `struct Params` in WGSL, and `build.rs` generated a `#[repr(C)]` Rust twin of each.
`pass_params_bytes` fills the right one:

``` { .rust .annotate title="crates/henad-models/src/gpu_foraging/mod.rs" }
    fn pass_params_bytes(pass: PassId, ctx: PassCtx<'_>, params: &[ParamValue]) -> Vec<u8> { // (1)!
        let geom = ctx.geom;
        match pass {
            PassId::Step(0) => {
                let hot = ForagingModel::from_params(split_params::<ForagingModel>(params).0, Self::dims(params).1); // (2)!
                bytemuck::bytes_of(&StepParams {
                    num_agents: geom.num_agents,
                    groups_x: ctx.groups_x, // (3)!
                    grid_w: geom.width,
                    grid_h: geom.height,
                    n_cells: geom.n_cells,
                    cutdown: hot.cutdown,
                    diagonal: hot.diagonal,
                    reward: hot.reward,
                    momentum: hot.momentum,
                    random_action: hot.random_action,
                    palette: packed_ant_palette(),
                })
                .to_vec()
            }
            PassId::Step(_) => bytemuck::bytes_of(&MergeParams {
                n: ctx.invocations,
                groups_x: ctx.groups_x,
                evaporation: PheromoneField::from_params(params).evaporation, // (4)!
                low: LOW_PHEROMONE,
            })
            .to_vec(),
            PassId::Display => bytemuck::bytes_of(&DisplayParams {
                width: geom.width,
                height: geom.height,
                n_cells: geom.n_cells,
                _pad: 0,
                tex: [geom.display.0, geom.display.1],
                _pad2: [0; 2],
                palette: packed_cell_palette(),
            })
            .to_vec(),
            PassId::Reduce => bytemuck::bytes_of(&ReduceParams {
                n: ctx.invocations,
                lanes: Self::REDUCE.lanes as u32,
                groups_x: ctx.groups_x,
                num_agents: geom.num_agents,
                n_cells: geom.n_cells,
                ..bytemuck::Zeroable::zeroed() // (5)!
            })
            .to_vec(),
        }
    }
```

1. `PassId` says which block is being asked for, step passes by their index in `STEP_PASSES`, and `PassCtx` carries the geometry plus the two numbers only the engine knows, the invocation count and the fold width.
2. The step's arm runs the _CPU model's_ `from_params`, so the hot-parameter derivations we wrote on the ants page, `diagonal` being `cutdown` raised to √2 for instance, are computed in exactly one place.
3. `linear_index` in the shader needs the fold width the engine picked.
4. The field's one parameter, through the CPU field spec's `from_params`.
5. The reduce block has padding fields to fill, and zeroing the rest is simpler than naming them.

The generated structs come from the same place as the shader strings:

``` rust title="crates/henad-models/src/gpu_foraging/mod.rs"
use crate::shader_bindings::gpu_foraging::display::Params as DisplayParams;
use crate::shader_bindings::gpu_foraging::merge::Params as MergeParams;
use crate::shader_bindings::gpu_foraging::reduce::Params as ReduceParams;
use crate::shader_bindings::gpu_foraging::step::Params as StepParams;
```

Add a field to a WGSL `Params` without touching this function and the crate stops compiling, since the generated struct gained a field this initialiser does not name.
That is the whole point of generating the twin.

### Statistics

We have the same three statistics to report as on the CPU.
Two come from a reduction and one from the counter.
The reduce shader is only the _leaf_, computing one value per lane per workgroup, and the engine's reduction tree does the rest:

``` { .wgsl .annotate title="crates/henad-models/src/gpu_foraging/reduce.wgsl" }
// Leaf of the stat reduction. One workgroup folds its slice down to one value per lane, and
// `GpuLaneReduce` owns every level above this.

#import shared::prelude::WORKGROUP
#import shared::reduce_tree::block_sum // (1)!
#import gpu_foraging::state::HAS_FOOD_BIT

struct Params {
    n: u32,
    lanes: u32,
    groups_x: u32,
    num_agents: u32,
    n_cells: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var<storage, read> state: array<u32>;
@group(0) @binding(1) var<storage, read> field: array<f32>;
@group(0) @binding(2) var<storage, read_write> partials: array<f32>; // (2)!
@group(0) @binding(3) var<uniform> params: Params;

@compute
@workgroup_size(256)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let block = wid.y * params.groups_x + wid.x;
    let i = block * WORKGROUP + lid.x;

    for (var lane: u32 = 0u; lane < params.lanes; lane = lane + 1u) {
        var value: f32 = 0.0;
        // One lane is per ant and the other per cell, so each bounds-checks its own domain.
        if (lane == 0u) { // (3)!
            if (i < params.num_agents) {
                value = f32((state[i] & HAS_FOOD_BIT) != 0u);
            }
        } else {
            if (i < params.n_cells) {
                value = field[i] + field[params.n_cells + i];
            }
        }

        let total = block_sum(lid.x, value); // (4)!
        if (lid.x == 0u) {
            partials[block * params.lanes + lane] = total;
        }
    }
}
```

1. `block_sum` is the workgroup fold, summing a value across the 256 invocations in a fixed pairwise order so the result replays. Every invocation has to reach it, since it barriers, and that is why the bounds checks below guard the value rather than returning early.
2. `partials` is another reserved name, the leaf's output. One value per lane per workgroup, and the engine's tree folds those down to one value per lane.
3. Lane 0 counts carrying ants and lane 1 sums both pheromone layers, the same two figures `stats` computed on the CPU. Deliveries come from the counter instead.
4. Invocation 0 gets the total and writes it. The others still had to take part in the fold.

`stats` then turns what was read back into the declared series:

``` { .rust .annotate title="crates/henad-models/src/gpu_foraging/mod.rs" }
    fn stats(sums: &[f32], counters: &[u32], _geom: &Geometry) -> Vec<StatValue> {
        vec![
            StatValue::Scalar(f64::from(sums[0])), // (1)!
            StatValue::Scalar(f64::from(counters[0])), // (2)!
            StatValue::Scalar(f64::from(sums[1])),
        ]
    }
```

1. The reduced lanes, in lane order. Both arrive through an asynchronous readback, so a reported stat is a few milliseconds stale, and reads zero until the first readback lands.
2. The persistent counter, cumulative across every tick since the model was built, exactly as the tally was.

## Running it

That finishes the model.
Declare the module and register it, and then we can run it.

``` rust title="crates/henad-models/src/lib.rs"
pub mod gpu_foraging;
```

``` rust title="crates/henad-models/src/registry.rs"
entries.push(register_gpu_agent_model::<crate::gpu_foraging::GpuForagingModel>(&ctx));
```

=== "Desktop app"

    ``` bash
    cargo run --release --bin henad-app
    ```

    Pick the second Ant Foraging (GPU), press Build, and set it playing.
    Give it a few hundred ticks before expecting a trail, as on the CPU.
    See [App tour](../app.md) for a quick overview of the UI.

=== "Headless"

    ``` bash
    cargo run --release -p henad-cli -- gpu_foraging --steps 1000 --reps 3
    ```

    To scale up, keep the world area proportional to the agent count so that density stays constant, and give the GPU a warm-up before anything is timed:

    ``` bash
    cargo run --release -p henad-cli -- gpu_foraging \
      --set num_agents=1000000 --set world_width=4472 --set world_height=4472 \
      --global-warmup 500 --steps 2000
    ```

=== "Browser"

    ``` bash
    ./scripts/build_web.sh serve --release
    ```

    Then open `http://localhost:8080`.
    The GPU models appear in the browser too, as long as it exposes WebGPU with compute support.

## Testing

The determinism story needs more care than the grid model's, and it is worth being precise about what holds.

**Tick 0 is bit identical** to the CPU model, by the seeding above.

**The backends then diverge**, because the generators differ.
The CPU draws from `xorshift64` over `u64`, and because WGSL has no 64-bit integers, the GPU draws from `pcg_hash` over `u32`.
The two are counterparts in role and produce different streams, so from tick 1 the colonies make different random choices, and a cell-by-cell comparison against the CPU model is off the table.

**A GPU run still replays bit identically.**
Deposits combine with `max`, which no scheduling order can change, and no ant reads another ant's slots, so however the GPU schedules the invocations, the same seed gives the same run.
This is the property the shipped GPU boids cannot have, since its neighbour sums add floats in whatever order its index happens to produce.

That last property is the one to test, and the test is a plain replay:

``` { .rust .annotate title="crates/henad-models/src/gpu_foraging/mod.rs" }
#[cfg(test)]
mod tests {
    use super::*;
    use henad_compute::gpu::GpuAgentState;

    #[test]
    fn a_run_replays_bit_identically() {
        let Some(ctx) = crate::tests::support::headless_context("gpu_foraging_test_device", wgpu::Features::empty())
        else {
            log::warn!("skipping a_run_replays_bit_identically: no wgpu adapter available");
            return;
        };

        let mut params: Vec<ParamValue> = GpuForagingModel::param_descriptors() // (1)!
            .iter()
            .map(|d| d.kind.default_value())
            .collect();
        params[NUM_AGENTS] = ParamValue::U32(4_000);

        let run = || {
            let mut state = GpuAgentState::<GpuForagingModel>::new(&ctx, &params);
            state.run_batched(300); // (2)!
            (
                state.read_buffer(POS), // (3)!
                state.read_buffer(STATE),
                state.read_buffer(FIELD),
            )
        };

        let (pos_a, state_a, field_a) = run();
        let (pos_b, state_b, field_b) = run();
        assert_eq!(pos_a, pos_b, "ant positions are not reproducible");
        assert_eq!(state_a, state_b, "packed ant state is not reproducible");
        assert_eq!(field_a, field_b, "the pheromone field is not reproducible");
    }
}
```

1. Every parameter at its default, then the population raised so the run spans many workgroups.
2. `run_batched` steps in submission-sized batches, exactly as the real runner does.
3. `read_buffer` copies a buffer back to the CPU as raw words, so the comparison is on bits rather than on floats.

With equality against the CPU gone, the rest of the port is checked the way the [determinism page](../../authoring/determinism.md) recommends, by invariants.
The shipped port's tests are a good list to copy: ants stay inside the bounded world, never stand inside an obstacle, the colony lays trail and delivers food within a bounded number of ticks, and the reduced total pheromone agrees with the field it summed.
None of those is as sharp as bit equality, but together they cover the places a port slips, the momentum and random-action fallbacks especially.

## The finished files

For reference, here is everything we wrote on this page.

??? example "`gpu_foraging/mod.rs` completed"

    ``` rust
    --8<-- "crates/henad-models/src/tests/tutorial/gpu_foraging.rs"
    ```

??? example "`gpu_foraging/state.wgsl` completed"

    ``` wgsl
    --8<-- "crates/henad-models/src/gpu_ants/state.wgsl"
    ```

??? example "`gpu_foraging/step.wgsl` completed"

    ``` wgsl
    --8<-- "crates/henad-models/src/gpu_ants/step.wgsl"
    ```

??? example "`gpu_foraging/merge.wgsl` completed"

    ``` wgsl
    --8<-- "crates/henad-models/src/gpu_ants/merge.wgsl"
    ```

??? example "`gpu_foraging/display.wgsl` completed"

    ``` wgsl
    --8<-- "crates/henad-models/src/gpu_ants/display.wgsl"
    ```

??? example "`gpu_foraging/reduce.wgsl` completed"

    ``` wgsl
    --8<-- "crates/henad-models/src/gpu_ants/reduce.wgsl"
    ```

The Rust listing is stored in the repository at [`crates/henad-models/src/tests/tutorial/gpu_foraging.rs`](https://github.com/micfong-z/henad/blob/master/crates/henad-models/src/tests/tutorial/gpu_foraging.rs), where it binds the shipped shaders under the paths its own directory gives them.
The five shaders are the shipped port's own, at [`crates/henad-models/src/gpu_ants/`](https://github.com/micfong-z/henad/tree/master/crates/henad-models/src/gpu_ants), and differ from what we wrote only in the import path of `state.wgsl`, which follows the directory name.

The actual default model is at [`crates/henad-models/src/gpu_ants/mod.rs`](https://github.com/micfong-z/henad/blob/master/crates/henad-models/src/gpu_ants/mod.rs).

On top of everything the grid page listed, batching, capacity, error handling and the display cap, the agent engine handled these on our behalf:

- Binding resolution by name, so no pass ever states a slot index.
- The reduction tree above our leaf shader, folding partials in a fixed order.
- The persistent counters, allocated, bound and read back alongside the reduction.
- The dispatch fold that turns a million-agent domain into a legal workgroup rectangle.
- The instanced renderer that draws `pos` and `color` in place, with no copy off the GPU.

## Next

That is both tutorials done, on both backends.
From here:

- [Choosing a trait](../../authoring/index.md) is the map of the four authoring paths you have now seen all of.
- [Porting a model to the GPU](../../authoring/porting.md) condenses this page and the last into a checklist for your own port.
- `gpu_boids/` is the other shipped `GpuAgentModel`: one pass, three double buffered lanes and a neighbour index, the structural opposite of ants.
