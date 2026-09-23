---
title: The CPU backend
description: The engines, drivers and hot loops behind a CPU model in henad-compute.
icon: material/memory
---

# The CPU backend

`henad-compute/src/cpu/` turns a CPU authoring impl into something runnable.
It is the sibling of [the GPU backend](gpu-backend.md) rather than a base for it, and the two mirror each other file by file.
Reading one backend against the other is a quick way to learn both.

```text
cpu/
  grid_engine.rs     GridModelState<M>     GridModel    -> SimState
  agent_engine.rs    AgentModelState<A>    AgentModel   -> SimState
  network_engine.rs  NetworkModelState<N>  NetworkModel -> SimState
  layout.rs          the spring layout behind a network's positions
  field/
    ca.rs            CaField<M>            a GridModel as a field layer
    scalar.rs        ScalarField<S>        scatter-and-decay f32 layers
  primitives/
    lanes_macro.rs   agent_lanes!
    chunked.rs       chunk drivers and RNG seeding
    scatter.rs       the many-agents-one-cell write path
    components.rs    connected components over a network's rows
  sim_thread.rs      the runner
```

The network engine, its layout and its components are the exception to the mirroring.
Network models run on the CPU only, and none of the three has a counterpart in `gpu/`.

## The engines

Each engine implements the whole of `SimState` for its trait.
A model therefore implements neither `SimState` nor `Model` itself, which keeps the runner interface out of the authoring surface.

`GridModelState<M>` owns the `Grid2D<u8>`, the parameter store and the tick counter.
Its step dispatches on `M::NEIGHBORHOOD` once, outside the row loop, and no per-cell work goes on the choice.

`AgentModelState<A>` owns rather more: the lanes, the field, the neighbour index, the deposit lanes, the tally and the seed.
One tick runs through a fixed sequence.

```text
1  extract hot params, for the model and for the field
2  rebuild the neighbour index from positions
3  run_deposit_pass    fills the field's deposit lanes
4  run_step_pass       moves the agents, returns a tally
5  merge the tally
6  field.update        scatter with the decay folded in, then swap
7  swap the dual lanes
8  advance the tick seed
```

Steps 3 and 4 each build their own `StepCtx`, because the deposit pass takes the lanes by shared reference and the step pass by mutable one, and the borrow checker wants the two apart.
The index is rebuilt before the deposit pass, which leaves both passes looking at the same neighbourhood.

Parameter splitting happens in `split_params`, dividing the composed list into the engine's own part, the model's part and the field's part.
The split is computed from the descriptor lengths, never from a hard-coded offset.

`NetworkModelState<N>` owns the lanes, the `Network`, the model's `Aux`, the parameter store, and separate seeds for the node pass, the global pass, actions and the layout.
Its tick is shorter.

```text
1  extract hot params
2  set_directed        rebuilds the rows if the direction switched
3  run_global_pass     sequential, and free to change the graph
4  run_node_pass       parallel over every slot, retired ones included
5  swap the dual lanes
6  repack the rows     only when should_repack says so
7  advance the tick seed
```

Of the two passes, only the global pass can change the graph.
The node pass gets it by shared reference, and every worker reads it at once.
Both passes are handed the tick before the increment, while `prepare_view` runs after it and is handed the count of completed ticks.

`stats` sees the aux by shared reference only.
`population` counts live nodes, and `heap_bytes` adds whatever `aux_heap_bytes` reports for the aux.

The engine prepends `num_agents`, `world_width` and `world_height` to the model's parameters.
The node count keeps the id `num_agents`.
The benchmark scripts look it up by that id.
Building a state runs the model's `init` and then repacks once.
The repack reclaims the space that relocations left behind and lays the rows out in node order.

## The row loop

`CaField::step_grid` is the hot inner loop behind every grid model, and small changes to its shape show up in every model's step time.

Three row slices are taken per row, wrapped vertically, and sliced to exactly one row wide.
Each slice being exactly a row, a neighbour access comes out as a single index instead of a `row * stride + x` multiply-add.

The x wrap is peeled off both row loops.
Only the first and last column actually wrap, so both are handled separately and the interior of the loop runs without a per-cell modulo.
The interior also uses `enumerate()` rather than the `zip()` that clippy suggests, because the `zip()` form measures worse.

```rust
next_row[0] = moore_cell::<M>(rows, last, 0, last.min(1), hot, rng);
if let Some(interior) = next_row.get_mut(1..last) {
    for (i, out) in interior.iter_mut().enumerate() {
        let x = i + 1;
        *out = moore_cell::<M>(rows, x - 1, x, x + 1, hot, rng);
    }
}
if last > 0 {
    next_row[last] = moore_cell::<M>(rows, last - 1, last, 0, hot, rng);
}
```

The odd-looking `last.min(1)` covers a one-column grid, where both wraps land on x 0.

A model indexes the neighbour slice by position, which makes the gather order published API.
A test drives a probe model whose cells encode their own offsets and asserts the order inside `step_cell`.

The rows are handed out one per closure call, but not one per rayon leaf.
`rows_per_leaf` puts a floor under how many rows a leaf takes, from `MIN_LEAF_CELLS` and the grid's width, since a 64 by 64 grid split one row at a time hands 48 workers 64 jobs of 64 cells and costs more to distribute than to run.
The floor is a scheduling choice only.
Each row keeps its own index and its own `chunk_seed`, so the grid a tick produces does not depend on how the rows were grouped.
Only a floor, too: a grid with rows to spare still splits below it, which is what lets rayon balance a model whose rows differ in cost.

## `for_each_chunk_mut!` is a macro

Rewriting it as a generic function is not an option.
Written as a generic taking `F: Fn(..)`, the extra closure layer stopped the kernel inlining through it and cost 48% on SIR, and `#[inline]` did not recover the loss.
Any new hot-loop driver faces the same constraint.

The macro comes in three forms: one over a single mutable slice, one stepping three together for a pass that writes more than one output lane, and one taking a `min_leaf` floor on how many chunks a rayon leaf takes.
Ants uses the three-lane form for its deposit pass.

## Seeding

```rust
pub fn chunk_seed(base: u64, tick: u64, c: usize) -> u64;
pub fn advance_tick_seed(seed: u64, tick: u64) -> u64;
```

`chunk_seed` derives a chunk's generator from the chunk index alone, never from anything a worker mutates, which makes a run independent of the thread count.
The `base` itself is advanced once per tick, on the sequential path, by `advance_tick_seed`.

The tick could in principle be folded in through `chunk_seed` alone, but doing so measured 14% slower on SIR with identical content.
That result has never been explained, and both functions stay until someone explains it.

## The scatter

`ScatterGrid` handles the one write pattern the rest of the engine cannot express directly: many agents depositing into the same cell.
Its three arms, and what picks between them, are covered in [fields](../authoring/fields.md#the-scatter).

The property that matters inside this crate is the choice of arm, which comes from the worker count and the deposit count.
All three must therefore produce identical bits.
Any divergence would make a model's results depend on the machine they ran on.
A test pins each arm explicitly and compares them against a reference written the obvious way, in the dense regime and the sparse one.

`scatter_then` carries a closure applied to every combined cell before it is written, which is how a decaying field avoids a second pass over its grid.
The result is the same because decay is monotone on non-negative values, so decaying a merged cell and merging decayed ones agree bit for bit.

Read the module docs before changing this file.
The strategy choice rests on measurement (`benches/scatter.rs`), and atomics are not an option under this contention pattern.

## The graph

`Network` lives in `henad-core/src/network.rs` and keeps the graph in two forms.
The renderer draws from an edge list, with `src`, `dst` and a colour byte per edge.
Kernels walk rows of neighbours in compressed sparse row (CSR) layout, where each entry carries a neighbour and the index of its edge in the list.
An undirected graph keeps both directions in one set of rows, and a directed one keeps an in-row and an out-row per node.

The rows are updated in place as edges come and go.
Each row has slack after its last entry, so adding an edge writes into that space.
A full row relocates to the end of the arrays with twice its capacity, and at least four entries, leaving its old space stale.
Removing an edge swaps the last entry of each endpoint's row into its place and swap-removes it from the edge list, renumbering the edge that moved.
Retiring a node removes its edges and leaves its whole row stale.
`spawn` hands out the most recently retired slot first.

Stale space is reclaimed by `repack`.
After each tick the engine asks `should_repack`.
It holds once stale entries make up more than half of the row storage and number more than four, and the engine then repacks.
`repack` is a compaction.
It copies each row as it stands into fresh arrays in node order, gives it a quarter of its length as slack and at least four entries, and keeps the order of entries within every row.
A compaction is cheaper than rebuilding the rows from the edge list.

`rebuild` sorts the edge list into rows from scratch.
It runs only when the model flips between directed and undirected, because the flip changes which rows exist and what they hold.
The engine calls `set_directed` at the top of every tick, and the call does nothing unless the value changed.

`version` goes up whenever the edge list or an edge colour changes, and when the direction switches.
`update_colors` bumps it only when its closure reports a change.
A recolour that changes nothing then costs the view no copy.

## The layout

`layout.rs` places a network's nodes for drawing.
It follows NetLogo's `layout-spring` with three changes: repulsion is cut off at a radius, the pull along an edge saturates, and movement is damped.

Each iteration gathers the force on every live node before any node moves.
Every edge pulls its two ends towards a rest length, and every pair of nodes within the cutoff pushes apart with a force that falls off as the square of their distance.
Both forces are divided by the mean degree of the pair, as in NetLogo.
The pairs within the cutoff come from a walk of a `SpatialHash`, rebuilt each iteration over the live nodes.
The hash wraps at the world's edges and the layout does not.
Each delta is recomputed without the wrap, and positions are clamped into the world.

An iteration leaves out a live node without a finite position, such as a spawn into a reused slot that the model has not placed yet.
That node does not move, and no other node feels a force from it.

Every `SpringParams` constant is in units of the mean spacing, `sqrt(W * H / n)` for `n` live nodes.
The default cutoff of 2.5 spacings then covers about the same number of neighbours at any population and world size.

The pull along an edge saturates.
It is `spring * S * tanh((d - rest) / S)`, with `S` the `saturation` constant, and a long edge pulls no harder than `spring * S`.
A few long edges, such as rewired shortcuts across the world, cannot fold the rest of the layout.

Movement is damped as in ForceAtlas2.
A node's swing is how much its force changed since the last iteration, and its traction is how much of the force held.
A global speed tracks the ratio of total traction to total swing, each weighted by degree plus one and scaled by a tolerance of 0.2.
The speed rises by at most half its value per iteration.
Each node moves at that speed, cut down further the more its own force swings.
A node that oscillates settles, and a node under a steady pull keeps its pace.
A step is also clamped to NetLogo's limit, a fiftieth of the world's width plus height along each axis.

The gather runs in parallel chunks of 512 nodes, each chunk writing the forces of its own nodes only.
Swing and traction are summed through `reduce_chunks` in chunk order.
Neither result depends on how rayon splits the work, and nothing is written through atomics.
Two nodes on the same point are pushed apart along an angle drawn from the layout's seed, the iteration and the node index.

`relax_layout` runs iterations until the time budget is spent, and always at least one.
The budget is 4 ms by default and is set from the app's [Pacing tab](../guide/app.md#pacing-tab).
[The runner](#the-runner) decides which publishes call it.

## Components

`primitives/components.rs` labels every node with the lowest node index in its component, using min-label propagation with pointer jumping.
Each round sets every node's label to the smallest of its own and its neighbours' labels.
Two pointer jumps follow every round that changed a label, each replacing a label with its label's label, to shorten the chains that propagation leaves behind.
The labelling stops after a round that changes nothing, and returns the number of components and the size of the largest.

Every pass reads one label buffer and writes the other, in parallel chunks of 4096 nodes and with no atomics.
A node's new label depends only on the labels of the round before, and the lowest index in a component is the same whatever order the work ran in.
The result is identical on any number of threads.
The labelling ignores direction and walks a directed graph's in-rows and out-rows alike.
A retired slot keeps its own label and is not counted.

Team Assembly calls it from `prepare_view`, at most once per publish, and caches the result in its aux against the graph's version and node count.

## The runner

`SimThread` exists so stepping never blocks rendering.
It owns the state, steps it, and publishes a `Snapshot` on a fixed cadence into a slot the UI takes from.
The work and the way it is driven are split across two types.

`SimLoop`

:   Decides what work is due now and when it next wants calling, through a `Pace` of `Idle`, `Now` or `After(duration)`.
    Everything about blocking, waiting and frame budgets lives outside it.

`Driver`

:   Decides how to wait.
    On native it spawns an OS thread and blocks on the command channel.
    On the web it runs the loop inline from the host's frame loop and hands the frame back once `PUMP_BUDGET_MS` has been spent, since `wasm32-unknown-unknown` cannot spawn a thread even with atomics.

The native driver pumps inside a `rayon::scope`, so a kernel's parallel passes are injected from a worker rather than from a thread rayon has to park and wake for each one.
Only the pump moves inside it.
The waits either side stay outside, since a worker blocked on a command channel is a worker the pool cannot use, and on a one-worker pool it would never come back.

The public API is identical either way, and nothing in `henad-app` needs to know which driver is active.
rayon still parallelises the kernels in both cases, and no kernel has a sequential twin.
If you find a `#[cfg(target_arch = "wasm32")]` around a hot loop, someone rebuilt a twin.

Publishing goes through `build_snapshot`, which calls `prepare_view` first and refills the buffers of a snapshot handed back by the UI.
A publish is then a copy rather than a fresh multi-megabyte allocation.
Every view is consulted, and a composite model publishes its field and its agents together.
A network model's edge list travels in an `EdgeSnapshot`, recycled with the rest.
The list is copied only when the graph's `version` or the list's length differs from the recycled copy.

Each snapshot carries a `serial` that counts publishes, and `view_ms`, the time `prepare_view` and the layout took together.
The viewport uploads its layers again on a new serial instead of a new tick, since a layout relaxing while paused moves nodes without advancing the tick.
The Performance tab shows `view_ms` as Prepare view.

`build_snapshot` relaxes a network's layout after `prepare_view` and before it reads the stats, at publish cadence and never inside `step()`.
How far the layout gets in one publish depends on wall-clock time.
Node positions are not a function of the tick, and they are for drawing only.
`henad-cli` never publishes and never lays a network out.

Whether a publish relaxes is up to the loop.
A publish that follows a tick relaxes the layout.
A publish while paused relaxes it only if `while_paused` was set, and a paused loop then keeps publishing every `PUBLISH_INTERVAL` instead of going idle.
An action or a change to the layout settings also publishes, and on a paused network that publish moves no node unless `while_paused` is set.

`SimCommand::SetLayout` carries `on`, `budget_ms` and `while_paused`.
The budget is capped at `MAX_VIEW_BUDGET_MS`.
On the web that cap is `PUMP_BUDGET_MS`, because a publish there runs inside the frame pump.
A state with no layout returns false from `set_layout`, and the loop then never asks it to relax.

`SimCommand::Act` runs one of the model's declared actions between ticks and publishes at once.
The tick has not moved, and nothing else would publish what the action did.
An action draws from [a stream of its own](../authoring/parameters.md#actions), seeded by `action_seed` and shared with no tick.

## Faults

The run loop is wrapped in a panic catch once, at thread start and outside the loop, which keeps the catch off the per-tick path.

rayon catches a worker's panic itself and re-raises it on the caller with `resume_unwind`, and that re-raise does not run the panic hook a second time.
`fault.rs` keeps a global fallback of recent panic sites alongside its thread-local record for exactly this case.
Without the fallback, every `step_cell` panic would lose its `file:line`.

## Next

- [The GPU backend](gpu-backend.md) describes the sibling directory.
- [Architecture](architecture.md) covers how the two halves sit against each other.
- [Writing fast models](../authoring/performance.md) is the model-facing half of this page.
- [Network models](../authoring/network-models.md) covers the trait the network engine runs.
