---
title: Statistics
description: The stat series a model declares, and the parallel reduction that fills them.
icon: material/chart-line
---

# Statistics

Every curve on the charts tab starts as a declaration on the model.
You declare the series once as a const and then return bare values in the same order, which keeps the labels and the values from drifting apart.

```rust
--8<-- "crates/henad-models/src/sir.rs:stat_descriptors"
```

A descriptor is just a label and a colour.
Take the colour from the model's own palette, and each chart line then keeps the same colour as the thing it counts.

```rust
--8<-- "crates/henad-models/src/sir.rs:stats"
```

When a snapshot is published, the engine zips the two lists together.
If `values` comes back short, the trailing series are left out instead of mislabelled, and a registry test asserts that every declared series gets a value.

## Values

```rust
pub enum StatValue {
    Scalar(f64),
    Vector2D { x: f64, y: f64 },
    Histogram { edges: Vec<f64>, counts: Vec<u64> },
}
```

The stats panel shows each variant in full.
A whole `Scalar` shows without decimals, and a fractional one with up to three.
The history chart plots one number per series, so a `Vector2D` is charted as its magnitude and a `Histogram` as its total count.
Boids reports average velocity as a `Vector2D`, which reads as a direction in the panel and doubles as a measure of flock coherence on the chart.

## When it runs

`stats` runs when a snapshot is published rather than on every tick.
Snapshots go out at most once every 16 ms, about 60 a second, however fast the simulation runs.
A model ticking faster than that runs its reduction on only some of its ticks.

If a value is needed for the readout but not by the step itself, compute it here.

A publish calls [`prepare_view`](views.md#prepare_view) before `stats`, and a value cached there is current when `stats` reads it.
`henad-cli --export-stats` samples a CPU model the same way and calls `prepare_view` before each row.
An exported series matches what the app shows.

## Reducing in parallel

To fold up a whole grid or population, run the reduction in chunks through `reduce_chunks`.

```rust
--8<-- "crates/henad-models/src/game_of_life.rs:count_alive"
```

`reduce_chunks` takes a length rather than a slice, so a single closure can read several lanes per chunk.
It folds the partials **in chunk order rather than completion order**.
A float reduction folded in arrival order would depend on how rayon happened to schedule the work.
The chunk size, `STATS_CHUNK`, is 8192.

Boids sums three totals in one pass this way, folding them into a struct instead of running three separate reductions.

## Tallies

Some quantities cannot be recomputed from the current state at all, because they count things that already happened.
An `AgentModel` declares a `Tally` for those.

```rust
type Tally = u64;
```

Each step pass returns one tally per chunk.
The engine merges them in chunk order, accumulates the merged value across ticks, and hands the total to `stats` alongside the lanes and the field.
Ants counts deliveries this way, since a delivered item leaves no trace in the population itself.

The default is `()`, meaning there is nothing to count.
`u32` and `u64` already implement the merge as a sum.

## Network models

A [`NetworkModel`](network-models.md) has no tally.
Its `stats` receives the lanes, the graph and the model's `Aux`, the state it keeps outside the lanes and the graph.

```rust
fn stats(lanes: &Self::Lanes, graph: &Network, aux: &Self::Aux) -> Vec<StatValue>;
```

`stats` borrows `aux` immutably and cannot store anything in it.
A stat that needs a walk of the graph, such as a count of connected components, is computed in `prepare_view` and kept in `aux` for `stats` to read.
Team Assembly keys its cached components by the graph's version and node count, and labels them again only when either has changed.

```rust
--8<-- "crates/henad-models/src/team_assembly/mod.rs:components"
```

`label_components`, from `henad_compute::cpu::primitives::components`, labels the components in parallel and returns their count and the size of the largest.
It reads a directed graph as undirected.

`stats` can be called before any `prepare_view` has run, and it still has to return a value for every series.
The registry test that counts the series calls it on a freshly built state.
Team Assembly reports both component stats as zero until the first labelling.

## On the GPU

On the GPU the state never leaves the device, and a stat comes back through a reduction pass followed by an asynchronous readback.
`SimState::stats()` reports whatever the last completed readback produced, which is a few milliseconds stale, and it reads all zero until the first readback lands.

=== "GPU grid models"

    Your reduce shader accumulates into an `atomic<u32>` array whose length must equal `STATS.len()`.
    `stats(counts: &[u32])` then turns those counters into the published values.

=== "GPU agent models"

    Your model writes only the leaf of the reduction tree, one value per `ReduceSpec::lanes`, and the engine owns every level above it.
    `stats(sums: &[f32], counters: &[u32], geom: &Geometry)` receives the reduction results and the persistent counters together, because a cumulative count is not a reduction and is never cleared.

## History

`StatsHistory` is a ring buffer with one column per series, pushed once per snapshot along with the tick it came from.
The chart's window, and the size of the buffer behind it, are both UI settings.
See the [app tour](../guide/app.md#charts-tab).

## Next

- [Parameters](parameters.md) covers the parameter declarations, which follow the same declare-once pattern.
- [Palettes and views](views.md) explains the palette the stat colours come from.
