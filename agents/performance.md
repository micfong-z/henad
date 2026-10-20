# Performance-critical paths

Read before touching a kernel, a chunk driver, the spatial hash or the scatter grid.

- **A parallel pass costs a wake-up per worker, and stepping from outside the pool costs an inject
  on top.** `runner/thread.rs` pumps inside `rayon::scope` (or inside `pool.install` for a runner
  from `SimThread::new_in`) and `henad-cli` steps inside one, so the passes a kernel runs are
  injected from a worker rather than from a thread that has to be parked and woken for each.
  `ca.rs::rows_per_leaf` is the other half: one row per rayon leaf handed 48 workers a 64 by 64
  grid as 64 jobs of 64 cells, and the floor turns that into one job. Measured together at 13x on
  the smallest grid rungs.
- `henad-compute/src/cpu/field/ca.rs::step_row_moore`/`step_row_vn` and
  `henad-models/src/*/step.rs` (the agent and node kernels) are the hot inner loops. The x-wrap is
  peeled off both row loops so the interior runs without a per-cell modulo; keep that shape,
  including the `enumerate()` interior loop.
- **rayon runs on every target, web included, and no kernel has a sequential twin.** There are no
  paired `_parallel`/`_sequential` functions to keep in step, and reintroducing one is a regression.
  A `#[cfg(target_arch = "wasm32")]` around a hot loop means someone rebuilt a twin.
- **`for_each_chunk_mut!` is a macro, not a function, and must stay one.** As a generic fn taking
  `F: Fn(..)` the extra closure layer stopped the kernel inlining through it and cost 48% on SIR;
  `#[inline]` did not help. Same trap applies to any new hot-loop driver.
- Determinism: a chunk's RNG comes from `chunk_seed(base, tick, chunk_index)`, never from anything
  a worker mutates, so results don't depend on how rayon schedules chunks. This now has to hold on
  the web too, where the pool width is whatever `navigator.hardwareConcurrency` reported. `base` is advanced once
  per tick on the sequential path by `advance_tick_seed` — folding the tick in only through
  `chunk_seed` measured 14% slower on SIR with identical content, and that was never explained.
  Boids, ants, Virus on a Network and Team Assembly each have a
  `results_do_not_depend_on_the_thread_count` test, as do the foraging tutorial (in its `tests/`),
  `cpu/grid_engine.rs`, `cpu/layout.rs` and `cpu/primitives/components.rs`. Keep them. The kit's `ThreadCount` compares
  every CPU model of a set the same way, at 14 jobs. `a_build_that_reads_the_pool_width_fails_the_thread_count_check`
  pins it deterministically. `a_shared_accumulator_fails_the_thread_count_check` retries up to five
  times, since a pool can take a shared lock in the single thread's order by chance.
- `AgentModel::CHUNK` is per-model on purpose. It sets both the RNG seeding granularity and the
  parallel load balance, so it must be a fixed const (not derived from the thread count) but still
  small enough to split across every core — 4096 gave only 13 chunks for 50k boids and cost 20%.
  512 is the default, boids overrides to 64 (its kernel draws nothing, and 512 gave a thousand
  agents two chunks) and ants to 4096. Changing ants' changes its results, since a chunk is its
  seeding unit; changing boids' does not.
- `SpatialHash` (`henad-core/src/spatial_hash.rs`) is a flat counting-sort grid, rebuilt every
  tick from agent positions — this replaced a naive neighbor search and was the biggest lever in
  getting boids to scale. All neighbor queries (including toroidal wraparound) go through
  `query_radius` or `for_each_within`, which walk the same cells in the same order; the second
  hands a kernel the deltas the range test already computed, so it does not work them out twice.
  Don't reintroduce O(n²) neighbor search. `HashGrid::new` is the one place the cell geometry is
  decided, for the CPU sort and the GPU one alike, and it caps the grid at `MAX_INDEX_CELLS`.
- `henad-compute/src/cpu/primitives/scatter.rs` (`ScatterGrid`) handles the one write pattern the rest of the
  engine can't: many agents depositing into the same cell. Read its module docs before changing
  it — the strategy choice is measured (`benches/scatter.rs`), not assumed, and **atomics are not
  an option**: `fetch_max` scales negatively under contention (7.1 ms at one thread, 99.2 ms at
  four). Its three arms must stay bit-identical, because the arm is picked from the worker count
  and the deposit count, so any divergence would make a model's results depend on the machine.
  Re-run the bench rather than reasoning about it. The sparse arm (`Banded`) is what a field layer
  takes: measured 2.3x to 12.3x over the dense ones wherever there are more cells than deposits,
  and behind them at one deposit per cell even on one worker.
- Data layout is Struct-of-Arrays throughout (`pos_x: Vec<f32>`, `pos_y: Vec<f32>`, ... rather
  than `Vec<Agent>`) specifically for cache locality and rayon-friendliness — preserve this when
  adding fields to a model's state. `agent_lanes!` emits one `Vec<T>` per lane with named field
  access for exactly this reason.
- Benchmarking: this machine drifts up to 40% under sustained load, so only interleaved old-vs-new
  runs on a cooled machine mean anything. Game of Life is the cleanest signal, since it has no step
  RNG and its output is bit-identical across refactors. When reporting a number, say whether it is
  release-mode and what is actually being measured — a flat-out `step()` counter, not a frame rate.
  A surprisingly favourable result gets flagged as surprising rather than presented as a finding: a
  "300x GPU speedup" here was once a debug-build, framerate-capped artifact hiding a real 10x.
