---
title: Statistics
description: The stat series a model declares, and the parallel reduction that fills them.
icon: material/chart-line
---

# Statistics

Every curve on the charts tab starts as a declaration on the model.
You declare the series once as a const and then return bare values in the same order, which keeps the labels and the values from drifting apart.

```rust
const STATS: &'static [StatDescriptor] = &[
    StatDescriptor::new("Susceptible", PALETTE[0]),
    StatDescriptor::new("Infected", PALETTE[1]),
    StatDescriptor::new("Recovered", PALETTE[2]),
];
```

A descriptor is just a label and a colour.
Take the colour from the model's own palette, and each chart line then keeps the same colour as the thing it counts.

```rust
fn stats(grid: &Grid2D<u8>) -> Vec<StatValue> {
    let (s, i, r) = count_sir(grid.current());
    vec![
        StatValue::Scalar(s as f64),
        StatValue::Scalar(i as f64),
        StatValue::Scalar(r as f64),
    ]
}
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
The history chart plots one number per series, so a `Vector2D` is charted as its magnitude and a `Histogram` as its total count.
Boids reports average velocity as a `Vector2D`, which reads as a direction in the panel and doubles as a measure of flock coherence on the chart.

## When it runs

`stats` runs when a snapshot is published rather than on every tick.
Snapshots go out on a fixed cadence while the simulation runs as fast as it can, so a reduction over ten million agents happens a few times a second instead of thousands of times.

If a value is needed for the readout but not by the step itself, compute it here.

## Reducing in parallel

To fold up a whole grid or population, run the reduction in chunks through `reduce_chunks`.

```rust
fn count_alive(cells: &[u8]) -> u64 {
    reduce_chunks(
        cells.len(),
        STATS_CHUNK,
        |r| cells[r].iter().filter(|&&c| c == ALIVE).count() as u64,
        |a, b| a + b,
        0,
    )
}
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
