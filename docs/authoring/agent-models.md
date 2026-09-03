---
title: Agent models
description: Writing a population of agents with the AgentModel trait, its lanes and its four associated types.
icon: material/dots-hexagon
---

# Agent models

_See [Writing a CPU agent model](../guide/first-model/ants.md) for a tutorial._

`AgentModel` is the authoring trait for a population of agents, optionally sitting over a [field](fields.md).
Pick it when your individuals move around and react to each other.
You declare the lanes with `agent_lanes!`, then implement `init`, `run_step_pass` and `stats`.

`boids/` and `ants/` are the two implementations to read alongside this page.
Boids is the smaller of the pair: its agents live in empty space, run a single pass, use no field, and read each other through a `SpatialHash`.
Ants adds a field, a second pass and a tally on top of that.

A CPU agent model is split across three files, with a fourth when it carries a field.

```text
boids/
  lanes.rs   the agent_lanes! declaration
  mod.rs     metadata, params, stats
  step.rs    the kernels
ants/
  field.rs   the pheromone layer
```

## Lanes

```rust
--8<-- "crates/henad-models/src/boids/lanes.rs:lanes"
```

`agent_lanes!` emits one `Vec<T>` per lane, each reachable as a named field, so storage stays struct-of-arrays throughout.
Lanes named `pos_x` and `pos_y` are required, because the engine builds both the neighbour index and the point view from them.

Each lane is declared `dual` or `plain`, one decision per lane rather than one for the whole population.

`dual pos_x / next_pos_x`

:   A double-buffered lane.
    The kernel reads every agent's current value and writes only its own next one, and the engine swaps the two sides after the pass.
    Use it whenever an agent reads a lane that other agents are writing in the same tick.

`plain color: u8 = 0`

:   A lane written in place, starting from the given initial value.
    Only the agent owning the slot ever touches it, and there is nothing to buffer.
    Ants declares every lane `plain`, since its ants never read one another.

Beyond the lanes themselves, the macro generates two view types, named in the declaration.
`BoidRead` holds the current side of every `dual` lane and is readable by every agent, while `BoidChunk` is the slice of each writable lane that one chunk owns.

The `color = <lane>` entry names the lane the renderer reads for per-agent palette indices.
Ants points it at `has_food` and avoids carrying a second lane for the purpose.

## The four associated types

These four types wire your model into the engine.

`Field`

:   The grid underneath the population: `NoField` when your agents live in empty space, `ScalarField<S>` for a scatter-and-decay layer, or `CaField<M>` to put a whole `GridModel` beneath them.
    See [fields](fields.md).

`Index`

:   Use `SpatialHash` when agents read each other.
    `NoIndex` covers a population that ignores its neighbours.
    The hash is a flat counting-sort grid rebuilt every tick from agent positions, and all neighbour queries, toroidal wraparound included, go through `query_radius`.
    `index_cell_size` is read every tick, so a live parameter edit reaches the index.

`Tally`

:   A per-chunk reduction merged in chunk order, or `()` when there is nothing to count.
    `u32` and `u64` already implement it as a sum.
    Chunks merge in the order they were created, whatever order they complete in, and a tally therefore never depends on how rayon schedules the work.
    The accumulated value survives across ticks, which ants relies on to count cumulative deliveries.

`Params`

:   Hot parameters extracted once per tick, leaving the kernel with no enum matching to do.
    See [parameters](parameters.md).

## The step pass

```rust
fn run_step_pass(lanes: &mut Self::Lanes, ctx: &StepCtx<'_, Self>, seed: u64, tick: u64) -> Self::Tally;
```

Normally this method is a single call to the generated `lanes.run_pass(..)` with a per-agent closure, since the chunking and the RNG seeding both happen inside that call.

```rust
lanes.run_pass(BoidsModel::CHUNK, seed, tick, |i, k, read, chunk, _rng| {
    // `i` indexes the whole population, `k` indexes this chunk's slices.
    // `read` is every dual lane's current side, `chunk` is this chunk's writable slices.
});
```

The two indices are not interchangeable.
Use `i` to read a lane through `read`, and `k` to write one through `chunk`.

`StepCtx` carries everything a kernel reads besides its own lanes: the field, the neighbour index, the hot parameters and the world extent.

### CHUNK

`const CHUNK: usize` sets both the RNG seeding granularity and the parallel load balance.
It must be a fixed const per model, never a value derived from the thread count, because the chunk index seeds the generator and results must not depend on the machine.
The value also has to stay small enough that a typical population still splits across every core.
Boids runs on the default of 512, and ants overrides it to 4096.

See [writing fast models](performance.md#chunk) for what happens when the value is set too high.

## A second pass

Some models need a pass over their agents before the step proper, and overriding `run_deposit_pass` provides one.

```rust
fn run_deposit_pass(
    lanes: &Self::Lanes,
    deposits: &mut <Self::Field as FieldLayer>::DepositLanes,
    ctx: &StepCtx<'_, Self>,
);
```

Ants fills its deposit lanes here, before anything reads the field, so every ant writes against the same unchanged field instead of against whatever the ants before it left behind.
The method takes `&Self::Lanes` and not `&mut`, because a deposit pass fills the field's lanes and moves nobody.

## Prepended parameters

Agent count, world width and world height are prepended at indices 0, 1 and 2, from `DEFAULT_AGENTS`, `MAX_AGENTS` and `DEFAULT_EXTENT`.
All three are reload-only.

The extent belongs to the engine rather than to either layer, and an agent layer can never disagree with its field about how big the world is.
A model's own parameters follow the prepended three, and its field's parameters follow those.
Both slice boundaries are computed from descriptor lengths, never from a hard-coded offset.

## Left to the engine

With the trait implemented, the engine handles all of the following:

- Allocates every lane, and swaps the `dual` ones after the pass.
- Splits the population into `CHUNK`-sized chunks across rayon, and seeds a generator per chunk per tick.
- Rebuilds the neighbour index from positions before every step.
- Allocates the field's deposit lanes once and reuses them, then runs the scatter and the decay.
- Merges the tally in chunk order and carries it across ticks.
- Splits the parameter list three ways, and rejects any edit to a reload-only entry.
- Builds the point view, the grid view, the history chart and the snapshots.

## Next

- [Writing an agent model](../guide/first-model/ants.md) builds one from an empty directory.
- [Fields](fields.md) explains the grid slot underneath a population.
- [Determinism and testing](determinism.md) describes the thread-count test every model that draws random numbers should carry.
- [GPU agent models](gpu-agent-models.md) covers the same topology written in compute shaders.
