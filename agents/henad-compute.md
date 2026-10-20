# henad-compute

**henad-compute**: the engine machinery that turns an authoring impl into something runnable.
`cpu/` and `gpu/` are **siblings**, not a base and a specialisation, and mirror each other:
each has its own `sim_thread.rs` (runner), its `*_engine.rs` (authoring trait → runnable state)
and `primitives/` (shared building blocks). `snapshot.rs`, `runtime_info.rs` and
`display_scale.rs` sit above both, since either backend publishes through them. So does `runner/`,
which owns how a sim loop gets driven and the one place the two ways of driving one differ:
`runner/mod.rs` holds the `SimLoop` trait, `Pace`, the `SnapshotSlot` and the bounded stop
(`DEFAULT_STOP_TIMEOUT`, `RunnerShutdown`, `DetachedRunner`), with `runner/thread.rs` the native
driver and `runner/frame.rs` the wasm one. `entry/` type-erases a model for a host:
`ModelEntry` (declarations behind accessors and an `Arc`, and a factory that takes the device at
`build`), `ModelState`, the five `register_*` generics, and `ModelSet` (`entry/set.rs`). An entry
holds no device from its registration. The five `register_*` functions fill a crate-private
`EntryDeclarations` and call the crate-private `ModelEntry::new_cpu` or `new_gpu`, which wrap the
build in `catching` or `catching_on` (a GPU build with no device is refused) and derive the
metadata's backend. `an_entry_from_values_reads_like_the_registered_one` holds the two paths
equal. Keep it. A set refuses a duplicate id or one outside the grammar, records its `BuildInfo`
on an entry that has none unless its source is runtime or plugin code, and merges the entries'
`GpuNeeds` for the device request. `ModelSet::replace` puts an entry in place of the one with its
id, at its position, and returns it, or appends the entry and returns `None`, stamping as `insert`
does, and `ModelSet::remove` takes the entry with an id out and returns it, keeping the order. The
`register_*` functions are generic, and the engines and kernels monomorphise in the crate that
calls them. The GPU grid engine is the exception. henad-compute compiles it
once, and only the model's hooks monomorphise in the caller. `register_gpu_grid_model` reads one
`Arc<GpuGridSpec>` from the model and passes it to the crate-private `ModelEntry::from_grid_spec`,
whose builds and demand share it and which boxes the engine itself, as a grid rule's entry does.
`GpuGridState<M>` is the adapter a test or a host builds directly.
henad-compute instantiates no model outside its own tests.
`wrap_factory` and `Factory` are `#[doc(hidden)]` and public for other crates' tests alone. The
hidden `ModelEntry::__into_plugin` marks an entry as a plugin's, and henad-hot's
`DevHost::rebuild` calls it on every entry a plugin exports. The hidden `__with_code` gives a
test's entry a runtime source labelled with its id.
`grid_init_rng` and `agent_init_rng` hold the default-seed rule the CPU engines and the GPU ports
share. `simulation.rs` is the programmatic API over an entry. `RunSetup`
(`ModelEntry::setup`) holds values by id, a seed and scheduled actions, each checked when set
(`set` through `check_value`, `set_text` through `parse_value`, `from_parts` and `from_replay`
for positional values). `RunSetup::build` fires tick 0's actions before it returns, and a
`Simulation` fires each later tick's after the step that reaches it (`Fire::AfterStep`), testing
the schedule once per stretch. On native a CPU `Simulation` runs each call's model code inside
one `rayon::scope` (`run_sampled` once for the whole call, its callback inside it, hence the
`WasmNotSend` bounds), and a GPU one runs on the calling thread inside `catching_on`. A refused
scheduled action is a `Fault`. A `Simulation` keeps the message of the first fault a call
returned, and every later call that runs model code returns it as `FaultKind::Refused`, or
`FaultKind::DeviceLost` once the device is lost. A fault can leave a step half done, and a later
call would step on from it. `StatSample` reads stats by label, `views` prepares at the read,
and `write_state` is `--export`'s writer. `ParamValue` has exactly three `From` impls (`f32`,
`u32`, `bool`). `an_unsuffixed_literal_sets_a_parameter` fails to compile on a fourth for an
integer type other than `i32`, and a static check beside it on `From<i32>` or `From<f64>`, the
types an unsuffixed literal falls back to.
- `cpu/grid_engine.rs` (`GridModelState`), `cpu/agent_engine.rs` (`AgentModelState`) and
  `cpu/network_engine.rs` (`NetworkModelState`) each implement the whole `SimState` for their
  trait. `cpu/field/ca.rs` (`CaField`, a `GridModel` as
  a field layer) and `cpu/field/scalar.rs` (`ScalarField`, scatter-plus-decay `f32` layers) are
  the two `FieldLayer` impls. `cpu/layout.rs` is the spring layout for a network's node
  positions, NetLogo's `layout-spring` with three changes. An edge's pull levels off as it
  stretches, repulsion is cut off at a radius found through a `SpatialHash`, and a node slows
  down while its force keeps swinging, as in `ForceAtlas2`. Every `SpringParams` constant is in
  units of the mean spacing between nodes. `cpu/primitives/` holds `lanes_macro.rs`
  (`agent_lanes!`, for node lanes too), `chunked.rs` (chunk drivers and RNG seeding), `scatter.rs`
  (the many-agents-one-cell write path) and `components.rs` (connected components by min-label
  propagation). `AgentModelState` counts its model's own params once at construction, and a step
  builds no descriptor list. `cpu/sim_thread.rs` is the sim runner, a `SimLoop` with play/pause/TPS-capping,
  driven by whichever `runner::Driver` the target has.
- `gpu/grid_engine.rs` (`GpuGridState`) and `gpu/agent_engine.rs` (`GpuAgentState`) are the
  engines for the two GPU traits, mirroring their `cpu/` namesakes. `gpu/sim_thread.rs` is the
  batching GPU runner and `gpu/timing.rs` its adaptive-batch controller. `gpu/view/` is what a
  model hands the UI (`display.rs` for a texture layer, `agents.rs` for lane buffers drawn in
  place). `gpu/primitives/` holds the GPU counterparts of henad-core's data structures —
  `spatial_hash.rs`, `prefix_scan.rs`, `reduce.rs`, `readback.rs` — plus `dispatch.rs` and
  `pipeline.rs`. The `henad::dispatch` module a pass imports and the `reduce_tree` fold a reduce
  leaf repeats are WGSL in henad-core's `authoring/primitives/wgsl/`. `gpu/grid_dims.wgsl` is an
  entry point that is never dispatched. It brings `henad::dims::Dims` into the generated
  bindings, and `grid_engine.rs`'s `Dims` is a `pub(crate)` alias of that struct.
  The grid engine is non-generic. `gpu/grid_spec.rs` holds `GpuGridSpec`, a `GpuGridModel`'s
  declarations as values: its shader sources and binding lists as `Cow`s, borrowed from the
  consts, and its functions behind the private `GridHooks` trait. `GpuGridEngine` builds every
  wgpu object from a spec and implements `SimState` and `GpuSimState`, and `GpuGridState<M>`
  reads a spec from `M` and forwards every call. `GpuGridSpec` and `GpuGridEngine` are
  `pub(crate)`. `demand`, `max_storage_bindings` and the contract asserts read the spec. The
  engine computes each pass's workgroups and its buffer bytes at construction, and a step calls
  no hook. `GridHooks::step_params_bytes` takes the run's seed, which a model type ignores and a
  grid rule writes into its uniform block.
  `gpu/tests/parity.wgsl` is the parity test's shader, with a `CODES` array that references
  each boundary and table code the test reads.
  `gpu/limits.rs` is what raises the device past the WebGPU baseline, and `gpu/capacity.rs`
  is what asks whether a model fits the device before anything is allocated.
  `gpu/contracts.rs` holds the asserts both engines make first: each shader's `main` declares the
  `@workgroup_size` its pass dispatches (read from the literal naga writes, skipped for anything
  else), and no buffer label is reserved or ends in `_in` or `_out`. Either mismatch validates
  and runs wrong.
  `gpu/stepping.rs` steps a `GpuSimState` from a host with no sim thread, native only. It submits
  at most `MAX_STEPS_PER_SUBMISSION` steps per command buffer, fires a `Schedule` under either
  `Fire` rule, waits once at the end of a run, and blocks on a stats readback. `submit_slice`
  submits one command buffer of steps, with the stats passes when sampling, and returns its
  `SubmissionIndex` without waiting. Its stats passes are `GpuSimState::encode_stats_passes`, the
  snapshot passes without the display pass. `await_submission` waits for one submission and those
  before it.
  A shared file name always means _counterpart_, never coincidence: `cpu/sim_thread.rs` and
  `gpu/sim_thread.rs`, `cpu/agent_engine.rs` and `gpu/agent_engine.rs`, `ants/step.rs`,
  `boids/step.rs` and `virus_network/step.rs`, `henad-core/src/spatial_hash.rs` and its
  GPU twin. Two unrelated things must not share a basename.
  The one name carrying three meanings is `primitives`, so keep them straight: `cpu/primitives/`
  and `gpu/primitives/` are engine internals and are counterparts of each other, while
  `henad-core/src/authoring/primitives/` is the model-author-facing vocabulary and is a
  counterpart of its own `wgsl/` directory instead.
- Behind the `runtime-wgsl` feature (naga 30 and naga_oil 0.23 with `default-features = false`,
  native and wasm32), `gpu/compose.rs` is the one module that calls naga_oil, decision 9 of #49.
  `compose_wgsl` adds the shared modules in `SHARED_WGSL_MODULES` order and then the caller's
  `#define_import_path` modules, composes with `make_naga_module`, validates with every flag and
  capability and writes with `WriterFlags::empty()`, the steps of `wgsl_bindgen` 0.23.3's
  `EmbedSource`. A stock shader composes to its `SHADER_STRING`, whose raw-string literal opens
  with one extra `\n`. A directive check reads every file first. It accepts a one-line item
  `#import` of a known module and a module's `#define_import_path`, and refuses every other `#`
  line, a `#`, `::` or `"` elsewhere in code, `enable`, `requires` and `diagnostic`, `virtual` and
  `override`, `@binding(auto)` and naga_oil's own name decoration. naga_oil panics decoding a
  decoration it did not write, as on `override fn pcg_hashX_naga_oil_mod_X1X(`. It also refuses
  a vertical tab, a form feed, U+0085, U+2028 and U+2029 (`find_wgsl_only_line_break`). naga ends
  a `//` comment at each, and naga_oil and the checks read the rest of the line as comment, so a
  `fn min` behind one would pass every text check unread. The call into
  naga_oil also runs inside `catching`, a backstop on native. Diagnostics point into the caller's
  text (see the GPU traps). `runtime_composition_matches_build_time_shader_strings`
  (`gpu/tests/compose.rs`, henad-compute's seven entries) and henad-models'
  `runtime_composition_matches_every_stock_shader` and
  `compose_never_panics_on_the_mutation_corpus` (`src/tests/runtime_parity.rs`, 3600 edits) hold
  it. Keep them.
- `gpu/runtime_grid/` is the grid rule. `GridRule` (`#[non_exhaustive]`, public fields: id, name,
  description, params, `RuleState`s with a label, a colour and an optional `initial` f32 parameter
  id, `Boundary`, default width and height, step text) compiles into a `CompiledGridRule`.
  `GridRule::life()` returns `LIFE_STEP` (`life_step.wgsl`, outside `ENTRY_POINTS`) with Dead,
  then Alive at the share `density`. `compile` checks the declarations first (`checks.rs`, public
  as `check_declarations`, each problem with the `RuleDeclaration` it is about: the id grammar,
  no control character in the name, the description, a label or an option (`control_message`),
  sides up to `MAX_GRID_SIDE`, `MAX_STATES`, `MAX_PARAMS`, parameter ids outside WGSL's reserved
  words, `width`, `height`, `seed`, `tick`, the grid's two and `henad_`, a default within its
  bounds, a positive finite step, choice options that `check_choice_options` passes, percent on
  an f32 alone, one constant per state outside the template's names and WGSL's reserved words,
  an f32 `initial`, one state without a share). Then the text (`MAX_STEP_BYTES`, a line break
  only naga reads, as in the directive check, a template name
  or a `henad_` name declared again, a WGSL predeclared name
  (`naga::keywords::wgsl::BUILTIN_IDENTIFIERS`) declared at module scope, where it would replace
  the built-in in the template's own lines, `@group`, `@binding` and an entry point, blanks or a
  comment after the `@` included, and a `step_cell`). It composes the text after the template
  (`template.rs`) through `compose`, whose base is the template's length, reads naga_oil's header
  offset back from the template's `neighbor`, and walks the IR of every function of the text, and
  once each imported function it calls. The walk refuses a loop, an atomic, a barrier and any
  other statement but `if`, `switch`, a store and a call, a store outside the function's locals, a
  pointer argument and the text's own `private` variables, a call passed a pointer into any other
  global, such as the template's `henad_cell`, and a reference to an engine buffer other than
  `params`. A read of `state_out` or `tick_out` races the other cells, and one of
  `state_in` by index reads past the grid as the backend's bounds policy decides. It also refuses
  a module-scope variable outside `private`, a binding, an entry point other than `main`, a
  function, constant, `private` variable or struct of the text named as a WGSL built-in
  (`check_builtin_names`, behind the text check), a
  function whose locals pass `MAX_FUNCTION_BYTES` (8192), the shader's `private` variables past
  `MAX_PRIVATE_BYTES` (8192, the template's `henad_cell` included), a value or constant past 8192
  bytes, and a `step_cell` whose one call runs more than `MAX_STEP_COST` (65536) expressions and
  statements, each callee's body counted once per call (`function_costs`, in handle order, since
  naga puts a callee before its caller). Without that limit, a chain of helpers each calling the
  one before twice holds no loop and runs 2^60 calls. The template comes first, so the text's
  lines keep their numbers after the template's. It imports `pcg_hash`, `offset_cell`, `TORUS` and
  `BOUNDED` under `henad_` and `HENAD_` aliases, and declares `Params` (`width`, `height`, `seed`,
  then one 4-byte field per parameter, `f32` for an F32 and `u32` otherwise), the five bindings
  (`state` and the one-word `tick` ping-pong, `params` is a uniform), `NUM_STATES`, one
  upper-snake constant per state, `neighbor` (state 0 past a bounded edge), `moore_count`,
  `von_neumann_count`, and `main`, which seeds the cell's `rng` from `pcg_hash(seed ^
  pcg_hash(index ^ pcg_hash(tick)))`, clamps the result below `NUM_STATES` and advances the tick.
  The engine owns the display shader (a palette const) and the reduce shader (one count per state,
  one stat per state), and a problem in either has no place. `CompiledGridRule::entry(code)`
  builds a `GpuGridSpec` with `RuleHooks` and calls `ModelEntry::from_grid_spec` with
  `ModelSource::runtime`. The hooks seed the cells on the CPU from `grid_init_rng`, the states
  with a share taking the low words in order below the running f32 total times `u32::MAX`, so the
  Life rule starts from `game_of_life::init`'s cells, and pad the uniform block to 16 bytes.
  `GridRule::CODE_FORMAT` (`henad-grid-rule/1`) is the code's format, and goes up with any change
  to what a rule's text does. `gpu/tests/runtime_grid.rs` holds
  `naga_oil_span_encoding_is_unchanged`, `a_syntax_error_points_at_the_user_line_and_column`,
  `an_error_inside_the_template_has_no_user_line`, `a_loop_in_user_code_is_refused_at_its_line`,
  `bindings_and_foreign_directives_in_user_text_are_refused`,
  `a_line_break_only_wgsl_reads_is_refused`, `the_module_check_refuses_a_builtin_name`,
  `shared_writes_and_barriers_are_refused`, `oversized_arrays_and_text_are_refused`,
  `a_call_chain_past_the_work_limit_is_refused`, `a_param_named_seed_is_refused`,
  `a_parameter_its_kind_cannot_hold_is_refused`, `two_states_with_one_constant_name_are_refused`,
  `the_params_block_matches_the_naga_layout`, `a_grid_rule_binds_at_most_five_storage_buffers`,
  `a_result_past_the_last_state_is_clamped` and `a_random_rule_draws_a_fresh_stream_each_tick`
  (each tick's Alive count against the stream recomputed on the CPU), and henad-models holds
  `the_life_starter_matches_game_of_life` (128 by 128, 64 ticks, the Alive count every tick and
  the cells every 8, read back through the display) and `a_grid_rule_conforms` (the Life starter
  and a rule that draws every cell afresh each tick). Keep them.
