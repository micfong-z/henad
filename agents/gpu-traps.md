# GPU traps that already cost real debugging

Each of these failed silently or misleadingly once. The engine now handles all of them, so the note
is about not undoing them.

- **A timestamp stamped on an empty compute pass is never written.** The symptom is a `start` of 0
  and an absurd elapsed time (an absolute GPU tick, ~4e14 ns). `gpu/agent_engine.rs` puts the
  opening stamp on the index rebuild's counting pass when there is an index, and on the first
  declared pass when there is not.
- **One oversized submission silently returns zeros.** Enough passes in a single command buffer
  trips the OS GPU watchdog — no error, no panic, and every later readback reads zero. Batch at 64
  steps per submission, as `GpuAgentState::run_batched` and the real runner do. This first showed up
  as a flaky test. The kit's `FullSubmission` compares one full submission of every GPU model at its
  defaults with single steps run before it. The sweep's GPU tracks
  (`henad-explore/src/exec/gpu.rs`) keep the bound per run. Each command buffer holds the steps of
  one run, and two runs' buffers are never merged. N runs of 64 steps in one buffer would trip the
  watchdog again.
- **`max_storage_buffers_per_shader_stage` is 8** in `wgpu::Limits::default()` and in the WebGPU
  baseline. `limits.rs::raise` asks for exactly what the models need, the `GpuNeeds` a host reads
  from its set through `ModelSet::gpu_needs()` before any device exists. Each GPU entry declares its
  own from its pass list — no constant, because wgpu's own advice is to request only what you need
  and a constant would be either short of a future model or dead headroom. For the example models
  it comes to 8, since `gpu_ants`'s step pass sits at exactly 8. `raise` takes the needs rather
  than knowing them: henad-compute cannot see which models a host offers.
  The kit's `BaselineBuild`, run by the registry tests on a `Limits::default()` device from
  `headless_test_device`, holds the line, and asserts in the same breath that `capacity.rs` agrees.
  Build and declared demand pin each other, and an over-reported pass count fails there.
  `every_gpu_entry_needs_the_bindings_its_widest_pass_binds` pins each entry's `GpuNeeds` to its
  demand. Note wgpu on Metal shares one argument table across storage + uniform + vertex, so a
  check counting only storage buffers can pass locally and fail there.
- **`Limits::default()` is not the hardware, and its _size_ limits are what bound a run.** The
  baseline caps one storage binding at 128 MiB, one buffer at 256 MiB and a texture side at 8192,
  where an M4 Pro offers 4 GiB, 14.3 GB and 16384. `limits.rs::raise` takes all three to whatever
  the adapter reports. Unlike the buffer count, these are deliberately machine-dependent: how big a
  run can be is a property of the hardware however we ask.
- **The display texture is a sampled view, never a mirror of the grid.** One texel per cell caps
  the grid at `max_texture_dimension_2d` and costs 4 bytes per cell, which at 16384² is 1.07 GB of
  RGBA for something drawn into a ~1000 px panel. `display_scale.rs` caps each axis at
  `MAX_DISPLAY_DIM`, a display pass dispatches per _texel_ and reads the cell at
  `texel * grid / tex`, and `viewport.rs` samples the CPU grid the same way on upload. Both are
  identity below the cap.
- **A model over the device's limit is refused before it is built.** `gpu/capacity.rs` computes a
  model's buffer sizes, texture dimensions and per-pass storage-binding counts from what it already
  declares, and checks them first. The app disables Build and both engines assert with a readable
  message. That covers sizes and binding counts. The cheap half.
- **Everything else the device rejects reaches the UI instead of the process.** wgpu's default handler panics on any
  error no scope claims. `gpu::fault::catching_on` wraps model construction in error scopes for all
  three `ErrorFilter`s, and `GpuContext::new` installs `on_uncaptured_error` as the floor under
  every path no scope covers, egui's own rendering included. Contracts `capacity.rs` cannot see
  (uniform layout, an allocation the device has no memory for) now reach the UI as a modal, and
  `contracts.rs` turns a workgroup size or a buffer label the device would accept into a panic
  at construction. Do not undo either half — remove the handler and the next validation error ends the
  process. Note error scopes are **thread-local**, so a scope pushed on the UI thread never sees
  what a sim thread does. The sink exists to cover that asymmetry.
- **A panicking kernel is caught too, and its location needs help.** Both sim threads wrap
  `run()` once at thread start, outside the loop, so the catch costs nothing per tick. Rayon
  catches a worker's panic and re-raises it on the caller with `resume_unwind`, which does not run
  the panic hook again, so `fault.rs` keeps a global fallback alongside its thread-local record.
  Drop the fallback and every `step_cell` panic loses its `file:line`.
- **Two clocks that must be reset together.** `gpu/sim_thread.rs` gates its stats refresh on
  `last_stats_publish` but divides by `tps_timer`; resetting one without the other reports a whole
  batch over a near-zero window as a plausible-looking TPS. Go through `reset_tps_window`.
- **A stats sample encoded while the previous readback is pending is dropped.** `CounterReadback`
  skips its copy while a map is in flight, and the stats repeat the older values with no error.
  `stepping::submit_slice` debug-asserts against it, and `stepping::sample_stats` collects a
  readback still in flight before it encodes its own. A poll that gives up a map unmaps the staging
  buffer. A buffer left mapped or pending makes the next copy into it a validation error and fails
  every later map. A host that pipelines its samples learns from the `StatsPoll` that
  `poll_stats_readback` returns whether the latest one landed or failed. The
  GPU tracks in `henad-explore/src/exec/gpu.rs` record no sample while one reads back, and assert
  it outside their error scopes, where a panic would pass for a failed run.
- **A lost device fails quietly first.** After `Device::destroy` or a driver loss, polls and waits
  still succeed, and builds and readbacks fail as ordinary validation errors. wgpu reports the loss
  only through the device-lost callback, and for a destroyed device only once a poll finds its
  queue empty. `GpuContext::new` installs that callback and `GpuContext::is_lost` reads it. The GPU
  tracks drain the device and check it before writing any run that failed on the GPU. Otherwise
  every run left is written as `gpu_error` instead of being left for `--resume`. `stepping::wait`
  and `stepping::sample_stats` return `FaultKind::DeviceLost` once it is set, and a `Simulation`
  returns it in place of the errors the loss caused. Otherwise `--export-stats` and the benchmark
  step on a lost device and report the last stats it read.
- **A naga span of a composed module is not a place in the file.** naga_oil keeps a module's index
  above bit 21 of every span (`SPAN_SHIFT`, private in naga_oil, copied in `gpu/compose.rs`), and
  parses each file behind an import header whose length it keeps to itself. It keeps each line but
  writes every imported name decorated, so a column after one is off. `compose.rs` masks the index,
  reads the header's length back from an anchor function whose line naga_oil leaves alone,
  recomputes naga_oil's preprocessed text through its public `Preprocessor`, and maps a column by
  aligning the tokens of the line naga read with the line as written, through naga_oil's
  `Tokenizer`. A column counts characters. A validation error lists its spans from the function to
  the expression at fault, and is placed by the last. `naga_oil_span_encoding_is_unchanged` and the
  place tests of `gpu/tests/runtime_grid.rs` fail when naga_oil changes any of it.
- **Uniform layouts and a model's binding slots are generated.** The `build.rs` of henad-compute,
  henad-models and henad-app runs henad-build's `ShaderBuild` over that crate's shaders, and
  `henad_compute::include_shaders!()` at the crate root brings the output in from `OUT_DIR` as
  `shader_bindings` and `binding_decls`, each under one allow list, `unsafe_code` included. The
  macro and the generated code name `include!`, `concat!`, `env!` and `assert!` through `::core`.
  henad-compute and henad-models also call the hidden `stamp_source_hash`, and henad-explore's
  `build.rs` calls `stamp_engine_commit` alone.
  henad-compute names its entry points with `ShaderBuild::new("src/gpu")`, and the other two use
  `discover` over `src` and `src/ui`: every `.wgsl` file without a `#define_import_path` line.
  `discover` follows a symlink to a directory, and walks a directory reached twice once. It
  refuses a path component that is no Rust identifier or is a keyword, names that
  collide, a `henad.wgsl` file or a `henad` directory holding a `.wgsl` file in any case, which
  would shadow a shared module, and a first component the generated code uses at its root (`wgpu`,
  `bytemuck`, `std`, `core`, `alloc`, `_root`, `ShaderEntry`, `layout_asserts`, `bytemuck_impls`).
  `generate` refuses an import path that starts with one of them too, since the bindings name an
  imported module by its import path, and a quoted one by its file stem (`bindings_module`,
  `ShaderBuildError::ReservedImport`). A module or an imported file with a `@binding` line fails
  the build, as does an import cycle. `wgsl_bindgen` would follow a cycle until the stack
  overflows, and a panic of its own becomes `ShaderBuildError::Compose`. The binding constant is
  the path's components upper-cased and joined by `_`. `generate` copies the shared modules from
  henad-core into `OUT_DIR/henad_wgsl/henad/` and removes a copy henad-core no longer holds, runs
  `wgsl_bindgen` (pinned to `=0.23.3`, and `scripts/check_packaging.sh` holds
  `WGSL_BINDGEN_VERSION` to the pin) with its own rerun lines off, prints one
  `cargo:rerun-if-changed` for the shader root (and one per explicit entry and per file outside the
  root an entry imports), in the single-colon form that keeps a downstream MSRV below 1.77, skips
  the pass when the FNV-1a stamp of its inputs and of `output.rs` itself matches
  `shader_bindings.stamp`, and writes each file only when its bytes change. The inputs are every
  file `wgsl_bindgen`'s dependency tree reads, a quoted path import outside the root or a file
  without the `.wgsl` extension included. A crate with no shaders gets an empty `shader_bindings.rs` and loses its
  stamp. Otherwise the shaders' return would match the old stamp and keep the empty file.
  `ShaderBuildError`'s `Debug` writes its `Display`. Uniform structs, workgroup sizes and bind group
  layouts therefore come from the WGSL, and each model asserts its own struct against the generated
  one. Shared WGSL is reached with `#import henad::<module>`, resolved at build time for every
  compiled entry point. A grid rule's step, display and reduce shaders are composed at run time
  instead, through `gpu/compose.rs` behind `runtime-wgsl`. Their bindings and the step's `Params`
  block come from the engine's template (`gpu/runtime_grid/template.rs`), and none of the generated
  assertions above cover them. henad-build also reads each entry's `@group(0)` lines into
  `binding_decls`, in `@binding` order, and fails the build on any line holding `@binding` or
  `@group` in another form than `@group(G) @binding(N) var<...> name: Type;` (`BindingLine`), on a
  sampler, a sampled texture or another address space (`UnsupportedBinding`), or on a gap in the
  indices. A compile-time assertion in `binding_decls.rs` holds each list to the length of the
  generated `WgpuBindGroup0` layout, and `include_shaders!` asserts that the shaders were composed
  against the `SHARED_WGSL_FNV1A64` of the henad-core it links. henad-build's tests run `generate`
  against a scratch `OUT_DIR`, never Cargo. The engine resolves each name itself. `params`, `dims`,
  `output`, `cell_start`, `sorted`, `counters` and `partials` are reserved, and any other name is a
  `BufferSpec` label with an optional `_in` or `_out` suffix. The access mode picks the side.
  `henad-core/src/authoring/model/binding.rs` is the reference. An imported constant or type reaches
  the generated bindings exactly when an entry point references it, since naga keeps only what an
  entry point references. Hence `grid_dims.wgsl` for `Dims`, and the `CODES` array in `parity.wgsl`
  for `MOORE_ROW_MAJOR` and the other codes.
