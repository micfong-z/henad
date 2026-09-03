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
  grid_engine.rs    GridModelState<M>    GridModel  -> SimState
  agent_engine.rs   AgentModelState<A>   AgentModel -> SimState
  field/
    ca.rs           CaField<M>           a GridModel as a field layer
    scalar.rs       ScalarField<S>       scatter-and-decay f32 layers
  primitives/
    lanes_macro.rs  agent_lanes!
    chunked.rs      chunk drivers and RNG seeding
    scatter.rs      the many-agents-one-cell write path
  sim_thread.rs     the runner
```

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
6  field.update        scatter, then decay, then swap
7  swap the dual lanes
8  advance the tick seed
```

Steps 3 and 4 each build their own `StepCtx`, because the deposit pass takes the lanes by shared reference and the step pass by mutable one, and the borrow checker wants the two apart.
The index is rebuilt before the deposit pass, which leaves both passes looking at the same neighbourhood.

Parameter splitting happens in `split_params`, dividing the composed list into the engine's own part, the model's part and the field's part.
The split is computed from the descriptor lengths, never from a hard-coded offset.

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

## `for_each_chunk_mut!` is a macro

Rewriting it as a generic function is not an option.
Written as a generic taking `F: Fn(..)`, the extra closure layer stopped the kernel inlining through it and cost 48% on SIR, and `#[inline]` did not recover the loss.
Any new hot-loop driver faces the same constraint.

The macro comes in two forms: one over a single mutable slice, and one stepping three together for a pass that writes more than one output lane.
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
Its two arms and the budget picking between them are covered in [fields](../authoring/fields.md#the-scatter).

The property that matters inside this crate is the choice of arm, which comes from the worker count.
Both arms must therefore produce identical bits.
Any divergence would make a model's results depend on the machine they ran on.
A test pins either arm explicitly and compares both against a reference written the obvious way.

Read the module docs before changing this file.
The strategy choice rests on measurement (`benches/scatter.rs`), and atomics are not an option under this contention pattern.

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

The public API is identical either way, and nothing in `henad-app` needs to know which driver is active.
rayon still parallelises the kernels in both cases, and no kernel has a sequential twin.
If you find a `#[cfg(target_arch = "wasm32")]` around a hot loop, someone rebuilt a twin.

Publishing goes through `build_snapshot`, which calls `prepare_view` first and refills the buffers of a snapshot handed back by the UI.
A publish is then a copy rather than a fresh multi-megabyte allocation.
Both views are consulted, and a composite model publishes its field and its agents together.

## Faults

The run loop is wrapped in a panic catch once, at thread start and outside the loop, which keeps the catch off the per-tick path.

rayon catches a worker's panic itself and re-raises it on the caller with `resume_unwind`, and that re-raise does not run the panic hook a second time.
`fault.rs` keeps a global fallback of recent panic sites alongside its thread-local record for exactly this case.
Without the fallback, every `step_cell` panic would lose its `file:line`.

## Next

- [The GPU backend](gpu-backend.md) describes the sibling directory.
- [Architecture](architecture.md) covers how the two halves sit against each other.
- [Writing fast models](../authoring/performance.md) is the model-facing half of this page.
