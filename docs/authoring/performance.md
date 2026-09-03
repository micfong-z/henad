---
title: Writing fast models
description: The layout and scheduling constraints a model has to respect to reach large agent counts.
icon: material/rocket-launch-outline
---

# Writing fast models

Henad exists to run models at agent counts other frameworks cannot reach.
Reaching them depends on cache-friendliness and real parallelism, so treat both as design constraints from the start.
The engine does most of the work already, and this page covers the parts where your model has to cooperate with it.

## Keep the lanes flat

Storage is struct-of-arrays throughout, so a population is `pos_x: Vec<f32>`, `pos_y: Vec<f32>` and so on.
An array-of-structs `Vec<Agent>` would gather each agent's fields together and undo that layout.

With lanes, a kernel's inner loop streams contiguous memory, and rayon can split a lane without splitting an agent.
`agent_lanes!` emits one `Vec<T>` per lane for exactly this reason.

A few shapes break the layout:

- A lane holding a struct, because touching one field then pulls the rest into cache with it.
- A `Vec<Vec<T>>` per agent, which turns every access into a pointer chase.
- Several lanes indexed through a value recomputed each iteration, leaving nothing to prefetch.

Prefer the narrowest type that works.
A grid cell is a `u8`, and widening it to `u32` doubles the memory traffic of every step.

## CHUNK

`AgentModel::CHUNK` sets both the RNG seeding granularity and the parallel load balance.
It has to be a fixed const rather than something derived from the thread count, because the chunk index seeds the generator and results cannot be allowed to depend on the machine.

It also has to be small enough that a typical population still splits across every core.
At 4096, 50k boids produced only 13 chunks and cost 20%.
Boids therefore runs on the default of 512, while ants overrides to 4096, its per-agent kernel being cheap enough that per-chunk overhead dominates.

Neither of those numbers is a rule, and if your model has an unusual per-agent cost you should measure both ends.

## Never scan every agent

`SpatialHash` is a flat counting-sort grid, rebuilt every tick from the agent positions.
Declaring `type Index = SpatialHash` and querying through `query_radius` is the single biggest lever for getting an agent model to scale, and boids only scaled in the first place once its naive neighbour search was replaced with this hash.

```rust
hash.query_radius(pos_x[i], pos_y[i], radius, pos_x, pos_y, buf);
```

The result buffer is caller-provided, and a query therefore does not allocate.
Boids keeps one buffer in a `thread_local!` and reuses it across the whole pass.

Toroidal wraparound is handled inside the query.
Do not reintroduce an O(n²) neighbour loop, and do not filter the whole population by distance.

`index_cell_size` is read every tick, which lets a live edit to whatever parameter sets the radius reach the index too.

## Hot parameters

Extract your parameters once per tick into `Self::Params`, and never match a `ParamValue` enum inside a kernel.
Precompute anything the kernel would otherwise redo on every invocation, such as squared radii, reciprocals and half extents.
Boids does all three, which leaves its neighbour loop with no setup per neighbour.

See [parameters](parameters.md#hot-parameters).

## The scatter write path

Dedicated machinery sits behind one write pattern only, many agents depositing into the same cell, and you reach it through [`ScalarField`](fields.md#the-scatter).

The cost is real, either one private grid per worker in the shadow arm or a counting sort over the whole population in the sorted arm.
A model that only needs each agent to write its own slot should use a `plain` lane instead and pay nothing.

Atomics do not help with this pattern.
Under contention `fetch_max` scales negatively, taking 7.1 ms at one thread and 99.2 ms at four.
The choice of strategy rests on measurement, so if you want to change it, re-run the benchmark rather than reasoning it out.

## Move work to publish

`stats` and `prepare_view` run on publish, at a snapshot cadence of a few times a second, while `step` runs thousands of times a second.

Anything a readout or a picture needs, but a step does not, belongs in `prepare_view`.
Ants quantises its whole pheromone field there, which would be unaffordable per tick but is nearly free per snapshot.

## Measuring it

!!! warning "A debug build tells you nothing"

    The unoptimised build is slow enough to change which end is the bottleneck.
    A "300x GPU speedup" in this repository was once a debug-build, framerate-capped artifact hiding a real 10x.
    Always measure with `--release`, and read the flat-out tick counter instead of a frame rate.

```bash
cargo run --release -p henad-cli -- boids --steps 1000 --reps 3
```

`henad-cli` steps a state in a bare loop with no rendering, no runner and no pacing, so a measurement times nothing but `step()`.
`--export-stats` writes out the time series.
See [the command line](../reference/cli.md).

To sweep every model across the configuration matrix:

```bash
python3 scripts/bench_matrix.py
```

Grid models scale over grid size, and agent models scale over agent count at constant density.
`--dry-run` prints the matrix without running anything.

Keep the world area proportional to the agent count to hold the density constant.
Otherwise two runs at different scales are not comparable.

### Rules for a number to mean anything

- **Interleave old and new.** A machine drifts by up to 40% under sustained load, so two runs an hour apart are not comparable.
- **Say what was measured.** State that the build was release mode and that the number is a `step()` counter rather than a frame rate.
- **Prefer Game of Life for a refactor.** It draws no random numbers during a step and its output is bit-identical across refactors, which makes it the cleanest signal available.
- **Flag a surprising result as surprising.** A favourable number with no mechanism behind it usually turns out to be a measurement bug.

## On the GPU

Two things dominate GPU performance, and the engine owns both of them.

**Batching.** Steps go out many to a submission.
One dispatch per step would spend its time on submission overhead instead of on the work.
The runner does the batching, and it sizes the batch adaptively against a wall-clock target to keep the UI responsive.

**Memory traffic.** A GPU grid step is usually bandwidth-bound, and it wins through latency hiding instead of faster arithmetic.
Bit-packing pays off in that regime.
GPU Game of Life packs 32 cells per `u32` and evaluates the rule SWAR-style, which cuts the traffic by 32 for the same result.

## Next

- [Determinism and testing](determinism.md) covers the constraints the same parallelism imposes on results.
- [The architecture](../developing/architecture.md) explains where a tick actually runs.
