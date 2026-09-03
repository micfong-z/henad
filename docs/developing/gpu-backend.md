---
title: The GPU backend
description: The engines, batching runner and device handling behind a GPU model in henad-compute.
icon: material/expansion-card
---

# The GPU backend

`henad-compute/src/gpu/` is the sibling of [the CPU backend](cpu-backend.md), and it serves the models whose state lives in GPU buffers and never round-trips to the CPU.

```text
gpu/
  grid_engine.rs    GpuGridState<M>      GpuGridModel  -> SimState + GpuSimState
  agent_engine.rs   GpuAgentState<M>     GpuAgentModel -> SimState + GpuSimState
  primitives/       spatial_hash, prefix_scan, reduce, readback, dispatch, pipeline
  shared/           WGSL reached by #import: prelude, space, rng, dims, reduce_tree
  view/             display.rs (a texture layer), agents.rs (lane buffers drawn in place)
  limits.rs         what raises the device past the WebGPU baseline
  capacity.rs       whether a model fits, asked before anything is allocated
  fault.rs          error scopes around engine work
  sim_thread.rs     the batching runner
  timing.rs         its adaptive-batch controller
```

Nothing in this directory creates a `wgpu::Device`.
The device is injected through `GpuContext`, cloned from whoever owns acquisition, which keeps the crate free of any dependency on egui or eframe.
Models still live in `henad-models`, where they contribute shaders, seed data and metadata, and every wgpu object is built here.

## The engines

`GpuGridState<M>` and `GpuAgentState<M>` mirror their `cpu/` namesakes.
Each derives every buffer, layout, pipeline and bind group from what its model declares, and each implements both `SimState` and `GpuSimState`.

`GpuSimState` is the extra interface a GPU model needs on top of `SimState`, and `GpuSimThread` drives the state through it.

| Method | Role |
|---|---|
| `encode_steps` | Records `count` steps into an encoder, advancing the tick counter |
| `encode_snapshot_passes` | Records the display and reduce passes, at snapshot cadence |
| `begin_stats_readback` | Starts the async readback, right after the submission |
| `poll_stats_readback` | Completes one without waiting on the GPU |

Ping-ponged buffers are handled through a parity index plus two pre-built bind groups per side, flipped per tick, so no bind group is rebuilt while stepping.
A buffer written in place gets one side, and `sides()` hands back that same buffer twice.

## The primitives

The files here are the GPU counterparts of the data structures in `henad-core`.

`spatial_hash.rs`

:   The twin of `SpatialHash`, rebuilt every tick with the same layout, so a kernel walks cell `c` as `sorted[cell_start[c]..cell_start[c + 1]]`.
    Unlike the CPU sort it is **not stable**.
    Membership matches, but a cell's slice comes out in whatever order the atomics resolve, and a kernel summing floats over one will not replay.

`prefix_scan.rs`

:   A multi-level exclusive prefix sum, standing in for the counting sort's serial running total.
    Each workgroup scans `WORKGROUP` elements, the level above scans their totals, and the results are added back down the chain.

`reduce.rs`

:   A multi-level float sum over a population, for stats an `atomic<u32>` cannot hold.
    The model supplies only the leaf, and every level above it belongs to this file.
    Levels pair in fixed order, which keeps the sum reproducible.

`readback.rs`

:   Async readback of a handful of `u32` counters.
    Blocking on `map_async` right after submission would stall the sim thread at the display cadence and cap throughput at roughly one in-flight batch per frame.
    The map instead starts right after submission and completes on some later loop iteration, which leaves a reported stat a few milliseconds stale, the same staleness the display already accepts.

`dispatch.rs`

:   Folds a linear invocation domain onto wgpu's 2D workgroup grid.
    Past 65535 workgroups on one axis a dispatch has to be a rectangle, so kernels take `groups_x` and recover their flat index through `linear_index`.
    `WORKGROUP` is read from the WGSL declaring it, and the group limit is hardcoded instead of read from the adapter, which keeps the fold identical on every machine.

## Batching

Steps go out `batch_size` at a time, split across submissions of at most 64 steps each.
Every step is still its own compute pass, because wgpu only synchronises between passes and the ping-pong needs exactly that synchronisation.

Display and reduce run once `SNAPSHOT_INTERVAL` has elapsed, and nothing else gates them.
The number of steps per snapshot is therefore emergent, and stays independent of batch size.

One batch is outstanding at a time.
Left unbounded, egui's own submissions queue up behind a dozen batches of sim work, and then every frame pays for all of them.

`batch_size` is either fixed by the UI or chosen by adaptive control.
Adaptive mode keeps an EMA of `time_per_step` and picks a size so that `batch_size * time_per_step` tracks a user-set target, 8 ms by default, half a 60fps frame.
It runs off wall-clock time rather than GPU timestamps, and `TimestampQuery` stays diagnostic-only.
`MAX_BATCH_SIZE` bounds the output, because by the time a slowdown needs reacting to, an oversized batch is already committed.

Submissions go on the same queue egui renders on, using handles cloned from egui's render state.
wgpu serialises submissions to a queue and treats each as atomic from the GPU's point of view, so egui's render pass samples either the fully-written previous display texture or the fully-written next one.
A torn frame cannot occur.

## Limits

`limits.rs::raise` treats the two kinds of limit differently.

**Size limits follow the adapter.**
`max_storage_buffer_binding_size`, `max_buffer_size` and `max_texture_dimension_2d` are all raised to whatever the hardware offers.
The baseline caps a storage binding at 128 MiB and a texture side at 8192, where an M4 Pro offers 4 GiB and 16384.
The size a run can reach is a property of the hardware, and a fixed baseline would only get in the way.

**Binding counts come from the models.**
`max_storage_buffers_per_shader_stage` sits at 8 in the baseline, and `raise` asks for precisely the number `registry::gpu_storage_bindings_needed()` derives by walking every model's declared passes.
Today that comes to 8, from `gpu_ants`'s step pass.
wgpu's own advice is to request only what you need, and a constant would end up either short of a future model or carrying dead headroom.

`raise` takes the count as an argument instead of computing it itself.
`henad-compute` sits below `henad-models` and cannot see the models, and a host needs the number before it has a device.

!!! warning "Metal shares one argument table"

    wgpu on Metal shares one argument table across storage, uniform and vertex bindings, so a check counting only storage buffers can pass locally and fail there.

## Failure handling

Three layers protect the process.

**Capacity, before allocation.**
`capacity.rs` computes buffer sizes, texture dimensions and per-pass storage-binding counts from what a model already declares, and checks them before anything is built.
The app disables Build and both engines assert with a readable message.
This layer covers only sizes and binding counts.
Everything else the device rejects falls to the scopes below.

**Error scopes, around the build.**
Left alone, wgpu's default handler panics on any error no scope claims, and a bad uniform layout or an allocation the device has no memory for then ends the process.
`gpu::fault::catching_on` wraps model construction in scopes for all three `ErrorFilter`s, and `GpuContext::new` installs `on_uncaptured_error` as the floor under every path no scope covers, egui's own rendering included.

Error scopes are **thread-local**, and a scope pushed on the UI thread never sees what a sim thread does.
The uncaptured-error sink covers that asymmetry.

**The panic catch, around the run loop.**
The run loop is wrapped once at thread start, outside the loop, which keeps the catch off the per-tick path.
See [the CPU backend](cpu-backend.md#faults) for the global fallback that sits alongside the thread-local record.

## Traps already handled

Each of these failed silently or misleadingly once.
The notes record why the current shape is load-bearing.

**A timestamp stamped on an empty compute pass is never written.**
The symptom is a `start` of 0 and an absurd elapsed time, an absolute GPU tick around 4e14 ns.
`agent_engine.rs` now puts the opening stamp on the index rebuild's counting pass when there is an index, and on the first declared pass when there is not.

**One oversized submission silently returns zeros.**
Enough passes in a single command buffer trips the OS GPU watchdog, which raises no error and no panic and leaves every later readback reading zero.
The 64-step cap exists to stay under the watchdog, and `run_batched` respects it.
The problem first surfaced as a flaky test.

**Two clocks that must be reset together.**
`sim_thread.rs` gates its stats refresh on `last_stats_publish` but divides by `tps_timer`.
Resetting one without the other reports a whole batch over a near-zero window as a plausible-looking TPS.
Go through `reset_tps_window`.

**The display texture is a sampled view, never a mirror.**
One texel per cell would cap the grid at `max_texture_dimension_2d` and cost four bytes per cell, which at 16384² comes to 1.07 GB of RGBA for something drawn into a panel a thousand pixels wide.
`display_scale.rs` caps each axis, a display pass dispatches per texel and reads the cell at `texel * grid / tex`, and the CPU upload path samples the same way.
Both are the identity below the cap.

## Next

- [The CPU backend](cpu-backend.md) describes the sibling directory.
- [Shaders and bindings](../authoring/shaders.md) covers the generated half from the model's side.
- [Environment variables](../reference/environment.md) covers `HENAD_DUMP_WGSL` and `HENAD_REQUIRE_GPU`.
