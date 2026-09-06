---
title: CPU Agent model
description: Write an agent model in Henad, building an ant colony that forages over a pheromone field.
icon: material/bug-outline
---

# Writing a CPU agent model

In this tutorial we'll build the Ant Foraging model on the CPU from scratch, which automatically uses all available cores.
This is classified as an **agent model** (as opposed from a **grid model**).

This page assumes you have worked through our [CPU Grid Model](game-of-life.md) tutorial first.

## What is Ant Foraging?

Before we write any code, let's be clear about what we are building.
Ant foraging is a behavior where ants search for food sources and return to the nest, laying down [pheromone](https://en.wikipedia.org/wiki/Pheromone) trails to guide other ants.
Roughly, the rules are:

1. Every ant starts out on the nest, with some reward.
2. Each tick, an ant lays pheromone (using reward) on the cell it stands on. Carrying food lays the `to-food` trail, searching for food lays the `to-home` trail.
3. It then steps to the neighbouring cell with the strongest pheromone of the _other_ trail.
   For example, a food-carrying ant follows the `to-home` trail, because, well, it is heading home.
   If no neighbour has any pheromone, it steps in a random direction.
4. If it reaches the food source, it picks food up, and if it reaches the nest, it drops food off.
   When a site is reached, the ants will leave the strongest pheromone possible and weaker amounts will be left in the neighbour cells by picking up additional reward.
5. Both trails evaporate a little every tick.

There are also obstacles in the world that ants cannot cross, and the nest and food source are fixed locations.
In a certain sense, the ants are finding the shortest path between the nest and the food source.

!!! info

    This model is a ported version of krABMga's [Ant Foraging](https://krabmaga.github.io/antsforaging/) model.

## Moving from cells to agents

Assuming that you just built a [CPU Grid Model](game-of-life.md), here's a quick comparison of the two types of models.

|                   | Grid model                           | Agent model                                    |
| ----------------- | ------------------------------------ | ---------------------------------------------- |
| State             | one `u8` per cell of a fixed lattice | one `Vec` per attribute, called a **lane**     |
| Transition        | `step_cell`, called once per cell    | a closure, called once per agent               |
| Neighbours        | Moore or von Neumann                 | optional `SpatialHash::query_radius`           |
| Additional Layers | —                                    | an optional [field](../../authoring/fields.md) |
| Counting          | a pass over the grid in `stats`      | a per-chunk tally, merged as you go            |

Let's set up.
Make a directory called `crates/henad-models/src/foraging/`, containing `mod.rs` and `field.rs`.

## Agent states (lanes)

Let's start by declaring the states (or attributes) that each ant needs to remember.

Agent states are stored in struct-of-arrays, i.e. one `Vec` per attribute, rather than one `Vec` of structs.
This layout means that the engine can process one attribute at a time, which is cache-friendly and allows the engine to parallelise over chunks of agents.
Each array is called a **lane** in Henad.

To make lives a bit easier, there's a `agent_lanes!` macro that generates it from a declaration, along with some configuration flags.

So what does an ant need to remember?
It needs its position, the direction it last moved in, whether it is carrying food, and how much reward it has left to deposit.
We can declare this in lane form as follows:

``` { .rust .annotate title="crates/henad-models/src/foraging/mod.rs" }
use henad_compute::agent_lanes;

/// No step taken yet, so momentum has nothing to continue.
pub const NO_STEP: u8 = u8::MAX;

agent_lanes! {
    pub struct AntLanes {
        read AntRead; // (1)!
        chunk AntChunk; // (2)!
        plain pos_x: f32 = 0.0, // (3)!
        plain pos_y: f32 = 0.0,
        /// Last direction, encoded `(dx + 1) * 3 + (dy + 1)`, or [`NO_STEP`].
        plain last_step: u8 = NO_STEP, // (4)!
        /// `0` searching, `1` carrying. Doubles as the render lane.
        plain has_food: u8 = 0,
        plain reward: f32 = 0.0,
    }
    color = has_food; // (5)!
}
```

1. `read` gives an identifier to the read-only type of any double buffered lane. Ants declares no such lane (i.e., no `dual` lanes), so the type stays empty. We still need to give it a name for the macro.
2. `chunk` gives an identifier to the writable slice one chunk owns. Our kernel will receive one of these.
3. `pos_x` and `pos_y` are required lanes, because the engine builds both the neighbour index and the point view from them.
4. All nine directions can be encoded into a single byte, as an example of compression.
5. `color` declares one palette index per agent, and it uses a lane we already have.

### Lane types

There are two types of lane, `plain` and `dual`.

A lane marked `plain` is written in place.
Marking it `dual` instead makes the lane double buffered under two names, which suits models whose agents read each other's current values while writing their own next ones.
See the Boids model for an example of a `dual` lane in action.

Ants declares every lane `plain`, because no ant ever reads another ant's slot.
Buffering is decided per lane rather than per model, so a model that writes everywhere in place pays nothing for a feature it never touches.

We have designated `color` to be `has_food`, which means that the engine will render the ants in the colour specified by the following palette, indexed by the `has_food` lane.

``` rust title="crates/henad-models/src/foraging/mod.rs"
pub const ANT_PALETTE: [[u8; 4]; 2] = [
    [0xE8, 0xE8, 0xF0, 0xFF], // searching
    [0x3D, 0xD5, 0x8C, 0xFF], // carrying food
];
```

## Fields

Apart from the ants, we also need a way to store the pheromone trails they leave behind, and the terrain itself.
These are represented by fields, and in particular, using the `ScalarField` struct.
We will write code about fields in `field.rs`.

A `ScalarField` is a set of `f32` grids that agents deposit into and that decay away every tick.
Beside them sits one static terrain layer that never changes at all.
For this model we want two pheromone grids, plus four marker values describing what occupies a cell of the terrain:

``` rust title="crates/henad-models/src/foraging/field.rs"
pub const EMPTY: u8 = 0;
pub const OBSTACLE: u8 = 1;
pub const FOOD: u8 = 2;
pub const HOME: u8 = 3;

pub const TO_FOOD: usize = 0;
pub const TO_HOME: usize = 1;
```

!!! warning "Separate module must be used"

    `henad_core::params!` generates a function named `descriptors()` into the module it expands in, which means a module can hold only one parameter list.
    A field layer declares parameters of its own, separately from the model above it, so it needs to be in a separate module.

The field itself carries a single parameter, which controls how fast a trail fades:

``` rust title="crates/henad-models/src/foraging/field.rs"
use henad_core::helpers::{extract_f32, f32_param};

henad_core::params! {
    const EVAPORATION = f32_param("evaporation", "Evaporation", 0.999, 0.9, 1.0, Some(0.001));
}
```

### Specification

The `ScalarFieldSpec` trait specifies information about the field to the engine.

``` { .rust .annotate title="crates/henad-models/src/foraging/field.rs" }
use henad_compute::cpu::field::scalar::ScalarFieldSpec;
use henad_compute::cpu::primitives::scatter::Combine;
use henad_core::params::{ParamDescriptor, ParamValue};

pub struct PheromoneField;

pub struct FieldParams {
    pub evaporation: f32,
}

impl ScalarFieldSpec for PheromoneField {
    const FIELDS: usize = 2; // (1)!
    const COMBINE: Combine = Combine::Max; // (2)!
    const PALETTE: &'static [[u8; 4]] = &CELL_PALETTE;

    type Params = FieldParams; // (3)!

    fn param_descriptors() -> Vec<ParamDescriptor> {
        descriptors()
    }

    fn from_params(params: &[ParamValue]) -> FieldParams {
        FieldParams {
            evaporation: extract_f32(params, EVAPORATION, 0.999),
        }
    }
}
```

1. We need 2 grids, `TO_FOOD` and `TO_HOME`.
2. The rule for combining the deposits of two ants that write into the same cell on the same tick.
3. Hot parameters for the layer, extracted once per tick exactly as a model's are.

Let's look at `COMBINE` more closely.
All combiners have to be commutative and associative, because otherwise the result would depend on which core arrived at a cell first.
Such a race condition should generally be avoided.

Henad provides 2 combiners, `Max` and `SumFixed`.

### Terrain

The terrain is specified by a single `u8` grid, which is static and never changes in the `ScalarField`.
This is built by the `build_sites` function once at construction.
We will build two elliptical walls of obstacles, and place the nest and food source in the two corners of the world.

``` { .rust .annotate title="crates/henad-models/src/foraging/field.rs" }
    fn build_sites(width: u32, height: u32, sites: &mut [u8]) {
        let (w, h) = (f64::from(width), f64::from(height));
        let size = 0.407 * (200.0 / w); // (1)!
        let blob = |x: f64, y: f64, cx: f64, cy: f64| -> bool {
            let a = ((x - cx) + (y - cy)) * size;
            let b = ((x - cx) - (y - cy)) * size;
            a * a / 36.0 + b * b / 1024.0 <= 1.0 // (2)!
        };

        for j in 0..height {
            for i in 0..width {
                let (x, y) = (f64::from(i), f64::from(j));
                if blob(x, y, 0.500 * w, 0.725 * h) || blob(x, y, 0.450 * w, 0.275 * h) {
                    sites[(j * width + i) as usize] = OBSTACLE;
                }
            }
        }

        // Placed after the blobs so a site is never buried under an obstacle.
        sites[food_cell(width, height)] = FOOD;
        sites[nest_cell(width, height)] = HOME;
    }
```

1. Every quantity here is a fraction of the world size rather than a pixel count, so that the grid size can stay a parameter and the same layout survives a resize.
2. Each blob is a long, thin ellipse tilted 45 degrees, and the two of them form a pair of walls the colony has to find its way around.

``` rust title="crates/henad-models/src/foraging/field.rs"
pub fn nest_cell(width: u32, height: u32) -> usize {
    let x = (0.875 * width as f32) as u32;
    let y = (0.875 * height as f32) as u32;
    (y * width + x) as usize
}

pub fn food_cell(width: u32, height: u32) -> usize {
    let x = (0.125 * width as f32) as u32;
    let y = (0.125 * height as f32) as u32;
    (y * width + x) as usize
}
```

### Decay

Once the tick's deposits have merged in, `decay` deals with decay.

``` { .rust .annotate title="crates/henad-models/src/foraging/field.rs" }
pub const LOW_PHEROMONE: f32 = 1e-14;

    fn decay(v: f32, p: &FieldParams) -> f32 {
        let d = v * p.evaporation;
        // Without the floor a trail never disappears.
        if d < LOW_PHEROMONE { 0.0 } else { d } // (1)!
    }
```

1. This is mostly just for the sake of a peaceful mind. :material-robot-happy-outline:

### Display

Displaying the field is controlled by `quantize`, which maps the `f32` pheromone values to a palette index.

``` { .rust .annotate title="crates/henad-models/src/foraging/field.rs" }
    fn quantize(site: u8, values: &[f32], out: &mut u8) {
        *out = match site {
            OBSTACLE => 13,
            FOOD => 14,
            HOME => 15,
            _ => {
                // Stronger route wins the cell, so overlapping trails stay legible.
                let (food, home) = (values[TO_FOOD], values[TO_HOME]);
                let (v, base) = if food > home { (food, 6) } else { (home, 0) }; // (1)!
                match ramp_step(v) {
                    0 => 0,
                    step => base + step,
                }
            }
        };
    }
```

1. Two six-step ramps share the palette, like a heatmap.

The ramp itself is logarithmic because trails decay geometrically.

``` rust title="crates/henad-models/src/foraging/field.rs"
const DISPLAY_DECADES: f32 = 3.0;
const RAMP_STEPS: u8 = 6;

/// Log scaled strength in `0..=RAMP_STEPS`, where 0 means not worth drawing.
fn ramp_step(v: f32) -> u8 {
    if v <= LOW_PHEROMONE {
        return 0;
    }
    let decades = v.log10() / DISPLAY_DECADES + 1.0;
    if decades <= 0.0 {
        return 0;
    }
    ((decades * f32::from(RAMP_STEPS)) as u8).clamp(1, RAMP_STEPS)
}
```

The palette holds sixteen entries, including a background colour, 6 blues for to-home, 6 oranges for to-food, and the three terrain colours at the end.

??? example "`CELL_PALETTE`"

    ``` rust title="crates/henad-models/src/foraging/field.rs"
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
    ```

That completes `field.rs`, and we can head back to `mod.rs`.

## Implementing `AgentModel`

This is very similar to [Implementing `GridModel`](game-of-life.md#implementing-gridmodel).

``` rust title="crates/henad-models/src/foraging/mod.rs"
use henad_core::authoring::model::agent_model::AgentModel;

pub struct ForagingModel;

impl AgentModel for ForagingModel {}
```

### Identity and Metadata

``` { .rust .annotate title="crates/henad-models/src/foraging/mod.rs" }
impl AgentModel for ForagingModel {
    const NAME: &'static str = "Ant Foraging";
    const ID: &'static str = "foraging"; // (1)!
    const DESCRIPTION: &'static str =
        "Ants lay and follow pheromone trails between a nest and a food source, around obstacles";
    const PALETTE: &'static [[u8; 4]] = &ANT_PALETTE;
    const STATS: &'static [StatDescriptor] = &[
        StatDescriptor::new("Carrying Food", STAT_PALETTE[0]),
        StatDescriptor::new("Deliveries", STAT_PALETTE[1]),
        StatDescriptor::new("Total Pheromone", STAT_PALETTE[2]),
    ];
    const CHUNK: usize = 4096; // (2)!
    const DEFAULT_AGENTS: u32 = 2_000; // (3)!
    const MAX_AGENTS: u32 = 5_000_000;
    const DEFAULT_EXTENT: Extent = Extent { w: 200.0, h: 200.0 };

    type Lanes = AntLanes;
    type Field = ScalarField<PheromoneField>; // (4)!
    type Index = NoIndex; // (5)!
    type Params = AntParams; // (6)!
    type Tally = u64; // (7)!
}
```

1. The shipped model already uses the id `ants`, and ids have to be unique across the registry.
2. The number of agents per chunk. There is more to say about this value [below](#deciding-on-chunk).
3. The engine prepends agent count, world width and world height to the parameter list, and these three consts supply their defaults and the upper bound.
4. `NoField` places agents in empty space, `ScalarField<S>` places the kind of field we just wrote, and `CaField<M>` (**C**ellular **a**utomata **Field**) places a whole grid model underneath a population as the underlying field.
5. Choose `SpatialHash` when agents read each other, and `NoIndex` when they do not. Our ants read the field and never each other, so the engine skips building an index entirely.
6. The hot parameters, extracted once per tick.
7. A per-chunk reduction, merged in chunk order and accumulated across ticks. Use `()` for a model with nothing to count.

Here are the imports that `impl` relies on:

``` rust title="crates/henad-models/src/foraging/mod.rs"
use henad_compute::cpu::field::scalar::ScalarField;
use henad_core::authoring::model::agent_model::{AgentModel, NoIndex, StepCtx};
use henad_core::authoring::model::field::Extent;
use henad_core::view::{StatDescriptor, StatValue};

use self::field::{FOOD, HOME, OBSTACLE, PheromoneField, TO_FOOD, TO_HOME, nest_cell};
```

We also need stat colours to match the three descriptors:

``` rust title="crates/henad-models/src/foraging/mod.rs"
pub const STAT_PALETTE: [[u8; 4]; 3] = [
    [0x3D, 0xD5, 0x8C, 0xFF], // carrying
    [0xF2, 0xE4, 0x5C, 0xFF], // deliveries
    [0x2E, 0x8B, 0xE8, 0xFF], // total pheromone
];
```

#### Deciding on `CHUNK`

We set `CHUNK` to 4096 here against a default of 512, and before changing a value like this it helps to understand that the const does two jobs at once.
It sets the granularity of random number seeding, and it is also the unit of parallel work.

If the value is too large, there are not enough chunks to fill every core: at 4096, fifty thousand boids produced just 13 chunks and paid 20% for it.
If the value is too small, the seeding overhead begins to show.

Ants gets away with 4096 because each ant does far more work per step than a boid, which means a bigger chunk still keeps a worker busy.

### Parameters

The model declares 4 parameters, and all 4 apply live to a running simulation.

``` rust title="crates/henad-models/src/foraging/mod.rs"
use henad_core::helpers::{extract_f32, f32_param};
use henad_core::params::{ParamDescriptor, ParamValue};

henad_core::params! {
    const UPDATE_CUTDOWN = f32_param("update_cutdown", "Trail Falloff", 0.9, 0.5, 1.0, Some(0.01));
    const REWARD = f32_param("reward", "Site Reward", 1.0, 0.1, 10.0, Some(0.1));
    const MOMENTUM = f32_param("momentum", "Momentum Probability", 0.8, 0.0, 1.0, Some(0.01));
    const RANDOM_ACTION = f32_param("random_action", "Random Action Probability", 0.1, 0.0, 1.0, Some(0.01));
}
```

The list an operator actually sees joins all three sources end to end.
The shipped ants declares the same 8 parameters, so its output shows us what to expect:

``` text title="cargo run -p henad-cli -- ants --params" hl_lines="2 3 4 9"
parameters for ants (Ant Foraging):
  index=0 id=num_agents kind=u32 default=2000 min=1 max=5000000 apply=reload label="Number of Agents"
  index=1 id=world_width kind=f32 default=200 min=1 max=10000 apply=reload label="World Width"
  index=2 id=world_height kind=f32 default=200 min=1 max=10000 apply=reload label="World Height"
  index=3 id=update_cutdown kind=f32 default=0.9 min=0.5 max=1 apply=live label="Trail Falloff"
  index=4 id=reward kind=f32 default=1 min=0.1 max=10 apply=live label="Site Reward"
  index=5 id=momentum kind=f32 default=0.8 min=0 max=1 apply=live label="Momentum Probability"
  index=6 id=random_action kind=f32 default=0.1 min=0 max=1 apply=live label="Random Action Probability"
  index=7 id=evaporation kind=f32 default=0.999 min=0.9 max=1 apply=live label="Evaporation"
```

The highlighted lines are the ones automatically generated by the model and its field.
Each layer receives its own slice of the list, numbered from zero within that slice, which is why `REWARD` is parameter 1 inside the model and parameter 4 to the outside world.

#### Hot parameters

`from_params` runs once per tick, so it is the place to put anything a kernel would otherwise recompute for every agent.

``` { .rust .annotate title="crates/henad-models/src/foraging/mod.rs" }
pub struct AntParams {
    pub w: i32, // (1)!
    pub h: i32,
    pub cutdown: f32,
    /// Cutdown raised to the diagonal distance, since those neighbours are further away.
    pub diagonal: f32, // (2)!
    pub reward: f32,
    pub momentum: f32,
    pub random_action: f32,
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
```

1. The world size arrives here pre-cast to `i32`, ready for the kernel's cell indexing, because casting on every access would tax each lookup.
2. This is used to save `powf` calls.

Forwarding the descriptors follows the usual pattern:

``` rust title="crates/henad-models/src/foraging/mod.rs"
    fn param_descriptors() -> Vec<ParamDescriptor> {
        descriptors()
    }
```

### Initialisation

For the first tick, `init` is called to fill the lanes.

``` { .rust .annotate title="crates/henad-models/src/foraging/mod.rs" }
    fn init(lanes: &mut AntLanes, extent: Extent, params: &[ParamValue], _rng: &mut u64) {
        let (width, height) = extent.cells(); // (1)!
        let nest = nest_cell(width, height) as u32;
        let (x, y) = ((nest % width) as f32, (nest / width) as f32);
        let reward = extract_f32(params, REWARD, 1.0);
        for i in 0..lanes.pos_x.len() {
            lanes.pos_x[i] = x;
            lanes.pos_y[i] = y;
            lanes.reward[i] = reward; // (2)!
        }
    }
```

1. The whole model shares one extent. `cells()` expresses that extent at one cell per unit.
2. The ants need some initial reward to start depositing pheromone.

Ants do not need random numbers during setup, which is why `_rng` remain unused here.

### Tick lifecycle

Here is the lifecycle of a tick for an agent model.
We only need to implement the two passes, and the rest is handled by the engine.

|     | Stage                                                        | Implementation                     |
| --- | ------------------------------------------------------------ | ---------------------------------- |
| 1   | Hot parameters are extracted, yours and the field's          | Engine, calling `from_params`      |
| 2   | The neighbour index is rebuilt from agent positions          | Engine, skipped for `NoIndex`      |
| 3   | Deposit lanes are filled. Nothing moves                      | **Pending**, in `run_deposit_pass` |
| 4   | Every agent steps, returning a per-chunk tally               | **Pending**, in `run_step_pass`    |
| 5   | Deposits are scattered into the field, then the field decays | Field layer                        |
| 6   | Double buffered lanes swap                                   | Engine                             |
| 7   | The tick seed advances                                       | Engine                             |

Both of our passes read the same field, because it changes only at step 5.

Every ant reads the old field and writes fresh values, so no ant gains an edge from being stepped ahead of another.

We'll write the second pass first, because the deposit pass makes more sense once we have an idea of what the ants are doing.

#### Pass 2: moving

Now for the heart of the model, which takes the role `step_cell` played on the grid page.
One ant makes one decision, with no knowledge that any other ant exists.

``` { .rust .annotate title="crates/henad-models/src/foraging/mod.rs" }
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
    let trail = if has_food != 0 { // (1)!
        field.field(TO_HOME)
    } else {
        field.field(TO_FOOD)
    };

    // An impossible pheromone, so the first passable neighbour always wins.
    let mut best = -1.0f32;
    let (mut bx, mut by) = (x, y);
    let mut count = 2u32; // (2)!

    for &(dx, dy) in &MOORE_COLUMN_MAJOR { // (3)!
        let (nx, ny) = (x + dx, y + dy);
        if !passable(nx, ny, sites, p) {
            continue;
        }
        let m = trail[(ny * p.w + nx) as usize];
        if m > best {
            count = 2;
        }
        if m > best || (m == best && reservoir_accept(next_bits(rng), count)) { // (4)!
            best = m;
            bx = nx;
            by = ny;
        }
        count += 1;
    }

    if best == 0.0 && last_step != NO_STEP {
        // No pheromone nearby, so probably keep going the way we were.
        if next_float(rng, 1.0) < p.momentum { // (5)!
            let (dx, dy) = decode_step(last_step);
            let (mx, my) = (x + dx, y + dy);
            if passable(mx, my, sites, p) {
                bx = mx;
                by = my;
            }
        }
    } else if next_float(rng, 1.0) < p.random_action { // (6)!
        let (dx, dy) = (choice3(next_bits(rng)), choice3(next_bits(rng))); // (7)!
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

    match sites[(by * p.w + bx) as usize] { // (8)!
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
```

1. See rule 3.
2. The count starts at 2, which gives the first neighbour visited twice the odds of every other. This reproduces an off-by-one quirk in the reference implementation (from krABMga) on purpose.
3. `dx` iterates on the outside and `dy` on the inside. Because ties are broken by a draw, the visit order shapes the result, and this ordering will be a feature of our model.
4. A strictly better neighbour wins outright, and equal strengths go to a reservoir draw. `reservoir_accept(bits, k)` accepts the `k`-th candidate of a run with probability `1/k`, spreading the choice evenly across however many neighbours tied. This matters because early in a run the whole grid reads zero, every direction ties, and a positional tie-break there would march the entire colony off together.
5. This branch handles a lost ant. With no pheromone anywhere in reach, the ant most likely repeats its last step.
6. Otherwise there is a small chance of ignoring the trail altogether. Without that escape hatch a colony settles into the first route it finds and stops looking for better ones.
7. Two draws need two separate `next_bits` calls, because feeding one word to both would correlate the axes.
8. See rule 4.

The function relies on three small helpers, together with the `AntMove` struct it returns:

``` { .rust .annotate title="crates/henad-models/src/foraging/mod.rs" }
#[inline]
fn encode_step(dx: i32, dy: i32) -> u8 {
    ((dx + 1) * 3 + (dy + 1)) as u8 // (1)!
}

#[inline]
fn decode_step(s: u8) -> (i32, i32) {
    let s = i32::from(s);
    (s / 3 - 1, s % 3 - 1)
}

/// Inside the field and not an obstacle. This model is bounded, not toroidal.
#[inline]
fn passable(x: i32, y: i32, sites: &[u8], p: &AntParams) -> bool { // (2)!
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
```

1. The nine directions map onto `0..9`, which leaves `NO_STEP` at 255 well clear of every real encoding.
2. Every candidate cell funnels through here, including both fallback paths above. Unlike every other model in the repository, ants plays on a bounded world, and forgetting the bounds check in one of the fallbacks is the easiest bug this page can produce.

##### Running it over the population

The `run_pass` method, generated for us by `agent_lanes!`, handles most nuances of parallelism and random number seeding.

``` { .rust .annotate title="crates/henad-models/src/foraging/mod.rs" }
fn advect(lanes: &mut AntLanes, ctx: &StepCtx<'_, ForagingModel>, seed: u64, tick: u64) -> u64 {
    let p = ctx.params;
    let field = ctx.field;
    lanes.run_pass(
        ForagingModel::CHUNK,
        seed,
        tick,
        |_i, k, _read, c: &mut AntChunk<'_>, rng| { // (1)!
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
            u64::from(out.delivered) // (2)!
        },
    )
}
```

1. The closure takes five arguments: the global agent index, the index within the chunk, the read-only half of any `dual` lanes, this chunk's writable slice, and a random generator. Ants uses two of the five.
2. The closure returns this agent's contribution to the tally, and `run_pass` folds contributions within a chunk, then across chunks in chunk order.

Under the hood, `run_pass` splits the lanes into chunks, hands each chunk a generator derived from `chunk_seed(seed, tick, chunk_index)`, and feeds our closure one agent at a time.
A chunk's seed derives from its index alone, so which agent meets which random stream is deterministic and reproducible.

That leaves `ctx`, which bundles everything an agent kernel reads beyond its own lanes: the field, the neighbour index, the hot parameters and the extent.

#### Pass 1: depositing

Now that movement is settled, we can circle back to the pass that actually runs first.
An ant lays pheromone where it currently stands, before anything has moved, and every other ant has to see the same field while that happens.
Running the deposits as a separate pass keeps that guarantee intact.

Rule 2 governs how much pheromone to lay, and it is subtler than it first sounds.

``` { .rust .annotate title="crates/henad-models/src/foraging/mod.rs" }
/// Largest pheromone in the 3x3 neighbourhood, cut down by distance and lifted by the reward.
#[inline]
fn deposit_value(x: i32, y: i32, reward: f32, field: &[f32], p: &AntParams) -> f32 {
    let here = field[cell_index(x as u32, y as u32, p.w as u32) as usize];
    let mut best = here.max(here * p.cutdown + reward); // (1)!
    for &(dx, dy) in &MOORE_COLUMN_MAJOR {
        let Some((nx, ny)) = offset_cell(x as u32, y as u32, dx, dy, p.w as u32, p.h as u32, Boundary::Bounded) else {
            continue; // (2)!
        };
        let cut = if dx * dy != 0 { p.diagonal } else { p.cutdown }; // (3)!
        let m = field[cell_index(nx, ny, p.w as u32) as usize] * cut + reward;
        if m > best {
            best = m;
        }
    }
    best
}
```

1. The result is floored at whatever the cell already holds, so a deposit can never come out weaker than the standing value. Because of that floor, `Combine::Max` can stand in for a plain overwrite.
2. At an edge, `Boundary::Bounded` returns `None` instead of wrapping. If we swapped in `Boundary::Torus`, the same call would wrap around the world.
3. Diagonal neighbours sit further away, so they take the steeper cut. Recall that `p.diagonal` holds `cutdown` raised to √2, computed once per tick back in `from_params`.

In other words, an ant never lays its reward flat.
It gathers the strongest pheromone within reach, cuts that down by distance, adds its own reward, and deposits the total.

The pass itself fills three lanes, in which each agent identifies one cell and stores one value per field.

``` { .rust .annotate title="crates/henad-models/src/foraging/mod.rs" }
fn deposit(lanes: &AntLanes, deposits: &mut Deposits, ctx: &StepCtx<'_, ForagingModel>) {
    let p = ctx.params;
    let (to_food, to_home) = (ctx.field.field(TO_FOOD), ctx.field.field(TO_HOME));
    let (pos_x, pos_y) = (&lanes.pos_x, &lanes.pos_y);
    let (has_food, reward) = (&lanes.has_food, &lanes.reward);

    let Deposits { cell, values } = deposits;
    let (head, tail) = values.split_at_mut(1); // (1)!
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
                cells[k] = (y * p.w + x) as u32; // (2)!

                if has_food[i] != 0 {
                    food[k] = deposit_value(x, y, reward[i], to_food, p);
                    home[k] = 0.0; // (3)!
                } else {
                    food[k] = 0.0;
                    home[k] = deposit_value(x, y, reward[i], to_home, p);
                }
            }
        }
    );
}
```

1. This makes two mutable borrows out of one `Vec<Vec<f32>>`. Nothing clever is going on, only the split that keeps the borrow checker content.
2. This is where the ant's deposit will land. Each agent identifies one cell, and any number of agents can identify the same one.
3. Since everything passes through `Max`, `0.0` acts as the identity.

Lastly, wire both passes into the trait:

``` rust title="crates/henad-models/src/foraging/mod.rs"
    fn run_deposit_pass(lanes: &AntLanes, deposits: &mut Deposits, ctx: &StepCtx<'_, Self>) {
        deposit(lanes, deposits, ctx);
    }

    fn run_step_pass(lanes: &mut AntLanes, ctx: &StepCtx<'_, Self>, seed: u64, tick: u64) -> u64 {
        advect(lanes, ctx, seed, tick)
    }
```

A single-pass model can omit `run_deposit_pass` entirely.

These imports should now all be in place:

``` rust title="crates/henad-models/src/foraging/mod.rs"
use henad_compute::cpu::field::scalar::{Deposits, ScalarField, ScalarRead};
use henad_compute::for_each_chunk_mut;
use henad_core::authoring::primitives::rng::{choice3, next_bits, next_float, reservoir_accept};
use henad_core::authoring::primitives::space::{Boundary, MOORE_COLUMN_MAJOR, cell_index, offset_cell};
```

### Statistics

We have three statistics to report:

``` { .rust .annotate title="crates/henad-models/src/foraging/mod.rs" }
    fn stats(lanes: &AntLanes, field: &ScalarField<PheromoneField>, tally: &u64) -> Vec<StatValue> {
        let carrying = lanes.has_food.iter().filter(|&&f| f != 0).count(); // (1)!
        vec![
            StatValue::Scalar(carrying as f64),
            StatValue::Scalar(*tally as f64), // (2)!
            StatValue::Scalar(total_pheromone(field.field(TO_FOOD), field.field(TO_HOME))),
        ]
    }
```

1. A plain count over one lane. It runs at publish time rather than every tick, so it stays off the hot path.
2. The tally is a running total accumulated across every tick since the model was built, so this is a cumulative figure rather than a per-tick one.

Summing the field needs a bit of care as floats are involved.

``` { .rust .annotate title="crates/henad-models/src/foraging/mod.rs" }
fn total_pheromone(to_food: &Grid2D<f32>, to_home: &Grid2D<f32>) -> f64 {
    field_sum(to_food.current()) + field_sum(to_home.current())
}

fn field_sum(cells: &[f32]) -> f64 {
    reduce_chunks(
        cells.len(),
        STATS_CHUNK,
        |r| cells[r].iter().map(|&v| f64::from(v)).sum::<f64>(), // (1)!
        |a, b| a + b,
        0.0,
    )
}
```

1. Values widen to `f64` before summing, and the chunks fold in index order. Float addition is not associative, so a sum folded in whatever order chunks happened to finish would differ from machine to machine.

``` rust title="crates/henad-models/src/foraging/mod.rs"
use henad_compute::cpu::primitives::chunked::{STATS_CHUNK, reduce_chunks};
use henad_core::grid::Grid2D;
```

## Running it

That finishes the model. Declare the module and register it, and then we can run it.

``` rust title="crates/henad-models/src/lib.rs"
pub mod foraging;
```

``` rust title="crates/henad-models/src/registry.rs"
register_agent_model::<crate::foraging::ForagingModel>(),
```

=== "Desktop app"

    ``` bash
    cargo run --release --bin henad-app
    ```

    Pick Ant Foraging, press Build, and set it playing.
    Give it a few hundred ticks: nothing looks like anything until the first ant stumbles onto the food, and a trail appears within a few dozen ticks after that.
    See [App tour](../app.md) for a quick overview of the UI.

=== "Headless"

    ``` bash
    cargo run --release -p henad-cli -- foraging --steps 1000 --reps 3
    ```

    To scale up, it is likely a good idea to keep the world area proportional to the agent count, so that density stays constant:

    ``` bash
    cargo run --release -p henad-cli -- foraging \
      --set num_agents=1000000 --set world_width=4472 --set world_height=4472 \
      --steps 1000
    ```

=== "Browser"

    ``` bash
    ./scripts/build_web.sh serve --release
    ```

    Then open `http://localhost:8080`.
    See [App tour](../app.md) for a quick overview of the UI.

## The finished files

For reference, here is everything we wrote on this page.

??? example "`foraging/mod.rs` completed"

    ``` rust
    --8<-- "crates/henad-models/src/tests/tutorial/foraging/mod.rs"
    ```

??? example "`foraging/field.rs` completed"

    ``` rust
    --8<-- "crates/henad-models/src/tests/tutorial/foraging/field.rs"
    ```

The listing above is stored in the repository at [`crates/henad-models/src/tests/tutorial/foraging/`](https://github.com/micfong-z/henad/tree/master/crates/henad-models/src/tests/tutorial/foraging/).

The actual default model is at [`crates/henad-models/src/ants/`](https://github.com/micfong-z/henad/tree/master/crates/henad-models/src/ants).

## Next

- [Writing a GPU grid model](gpu-game-of-life.md) and [writing a GPU agent model](gpu-ants.md) take the two CPU models into GPU models.
- [Choosing a trait](../../authoring/index.md) introduces the two GPU traits, for carrying a model like this one into compute shaders.
- [Authoring primitives](../../reference/primitives.md) is a reference of authoring primitives.
