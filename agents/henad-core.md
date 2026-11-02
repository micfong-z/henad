# henad-core

**henad-core**: no dependencies on other crates — not even wgpu or bytemuck, which is why the two
GPU traits describe their shaders as `&'static str` and their buffers as plain bytes. Defines the
abstractions everything else builds on. `authoring/` is the model authoring API and splits in two.
`authoring/model/` holds the five traits a model implements,
one per (topology × backend): `GridModel` (`authoring/model/grid_model.rs`) for cellular automata,
`AgentModel` (`authoring/model/agent_model.rs`) for agent populations, `NetworkModel`
(`authoring/model/network_model.rs`) for nodes joined by edges, `GpuGridModel`
(`authoring/model/gpu_grid_model.rs`) for shader-resident grids and `GpuAgentModel`
(`authoring/model/gpu_agent_model.rs`) for shader-resident populations, plus `FieldLayer`
(`authoring/model/field.rs`),
the grid slot an `AgentModel` sits over. `authoring/primitives/` is the primitive vocabulary those
kernels call — wrapping, neighbourhoods, distances, random draws — most paired with a WGSL twin
under `authoring/primitives/wgsl/`, and each pure one pinned to its twin by a parity test.
`wgsl/` holds the five shared modules a shader imports as `henad::dispatch`, `henad::dims`,
`henad::rng`, `henad::space` and `henad::reduce_tree`, as text in `SHARED_WGSL_MODULES`, with
their `SHARED_WGSL_FNV1A64` computed by a `const` FNV-1a (`Fnv1a64`'s writes are `const fn`).
An interface change to one ships only in a breaking release. User shaders import these paths.
`space::offsets`, `space::for_each_neighbor`, `rng::mix_seed` and `rng::next_index` are Rust
only. `next_index`'s redraw needs a 64-bit product, and WGSL has no 64-bit integers.
`docs/reference/primitives.md` is the index, marks each Rust-only and WGSL-only entry, and
records what is deliberately absent.
`SimState` (`model.rs`) is the _runner_
interface the sim thread drives, not an authoring API — that split is why the traits live under
`authoring/model/` and this one does not. `provenance.rs` holds `BuildInfo`, the identity of one
compiled crate that `build_info!` returns for the crate it expands in, and `ModelSource`, an
entry's type path and the build of the crate that registered it. A build script stamps the
commit, the dirty flag and the source hash through henad-build, and they read as unknown in a
crate whose script does not. `__VERSION` is henad-core's version, which has no build script.
A `ModelSource` also holds a `CodeOrigin` (`Compiled` or `Runtime`, non-exhaustive).
`ModelSource::runtime(label, code)` is a model composed at run time, typed `runtime:<label>`, with
no build, and its `ModelCode` holds a format, the canonical text and the FNV-1a hash of the tag
`henad.model-code.1`, the format and the text (`model_code_hash_is_pinned`, keep it).
`code_hash()` is that hash for runtime code and the registering crate's source hash for compiled
code. Equality covers the origin and the code.
`diagnostic.rs` holds `CodeDiagnostic` (severity, message, file, byte span, line and character
column counted from 1, and notes), `#[non_exhaustive]` and built through `error` and the `with_`
methods, with `at(text, span)` reading the line and column from a text, and the
`#[non_exhaustive]` `DiagnosticSeverity`, whose one variant is `Error`. Its `Display` writes
`file:line:column: severity: message` with each control character escaped (`write_escaped`), so
a key or a label quoted from a file never reaches a terminal raw. `helpers::check_choice_options`
is the option-name check `choice_param` panics on, for options read at run time.
Also the `Grid2D<T>`
double-buffered SoA grid (`grid.rs`),
the counting-sort `SpatialHash` and the `HashGrid` cell geometry both backends share
(`spatial_hash.rs`), the `Network` graph a `NetworkModel` works on (`network.rs`), param
descriptors and `ParamStore` (`params.rs`), `ActionDescriptor`, the `actions!` macro,
`action_seed` and the `Schedule` of `--act` entries with its `Fire` rule (`action.rs`),
stat/view types consumed by the UI (`view.rs`), and small shared
helpers such as the param and stat builders (`helpers.rs`). `xorshift64` sits with the random
draws in `authoring/primitives/rng.rs`. `Extent` is re-exported at the crate root.
`ParamDescriptor`, `StatDescriptor`, `StatEntry` and `ActionDescriptor` keep private fields over
`Cow<'static, str>`, read through accessors. The helpers and the `const fn new` of
`StatDescriptor` and `ActionDescriptor` borrow. `StatDescriptor::owned` and
`ActionDescriptor::owned` own, and `ParamDescriptor::new` takes either. `ParamKind::Choice` holds
a `ChoiceOptions` of either kind. Model code never changes with the storage. `ModelMetadata`
(built through `new`) and `Structure` are `#[non_exhaustive]`, and a match on `Structure` outside
henad-core takes a wildcard arm. The arms sit in the kit's `declared.rs` (`prepended_ids`, and
`metadata`, whose wildcard fails the check), `size_for_jobs` in `determinism.rs`, the app's
`structure_rows`, and a `matches!` in `benchmark.rs`, and a new variant takes an arm in each.
`LaneSpec`, `BufferSpec` and `PassSpec` stay `&'static`.
`Network` keeps its adjacency rows in compressed sparse row (CSR) form with slack, beside an edge
list the view draws. A full row relocates to the end, the engine repacks after a tick once
`should_repack()` finds too much stale space, and a full `rebuild()` runs only when the graph
changes direction. `version()` goes up whenever the edges, their colours or their direction
change. `repack`, `should_repack`, `rebuild`, `set_directed` and the raw `spawn` and `retire` are
`#[doc(hidden)]`, since the engine calls them and a model goes through `Nodes`.
`explore/` holds the parts of a sweep that need no engine, and builds on wasm like the rest of the
crate. `value.rs` reads a param value written as text and checks it against its descriptor
(`parse_value`, `resolve_params`, `parse_overrides`), and `format_value` writes one back. `--set`
goes through it. A choice reads an option's name before its index, and `helpers::choice_param`
panics on an option name that reads as an unsigned integer, appears twice, holds a comma or a
colon, or is `all`, since `--vary` level text could not list it. `spec.rs` is a
sweep as written, a `SweepSpec` of `BlockSpec`s with its
`RunSettings`, `MeasureSettings`, `SeedSettings` and `ActionSpec`s. `SweepSpec::plan` (`plan.rs`)
checks it against a `ModelSchema` and lists every `Config`, its param values and one tick per
action, into a `Plan`, where run `run_id` is replicate `run_id % replicates` of config
`run_id / replicates`. It refuses an unknown id, a value out of bounds, a param both fixed and
varied or varied twice in a block, a zip of unequal lengths, an `F32` range with no step outside
a sampled design, a factor with no levels (`DesignError::NoLevels`), an undeclared action, a
name two actions share, a stop or `first` threshold that is not finite (`MeasurePlan::check`,
through `StopSpec::check_threshold` and `Comparison::check`), and more than `MAX_RUNS` runs
(2^24, equal to `MAX_CONFIGS`, `PlanError::TooManyRuns`), all before any run. An action due
past the last tick is only a `PlanWarning`. `Plan::schedule` orders a config's actions
by tick, then by spec order, and a `Shard` (`I/N`) takes the runs with `run_id % N == I`.
`factor.rs` resolves a factor over a param or an action's tick (`FactorTarget`) from a list, an
inclusive range or `all`, and `LevelSpec::parse` reads the level text of `--vary`. It and a plan
ignore the spaces around each listed value.
`inclusive_steps` computes value `i` as `min + i * step` and never accumulates, and a sampled
design takes a range with no step as a whole `FactorDomain`. `design.rs` combines a block's
factors under a `DesignKind`: `Factorial` (the first factor slowest, the last fastest), `Zip`,
`Random`, `LatinHypercube` or `Table`. A Latin hypercube gives each factor its own Fisher-Yates
permutation of the strata. A continuous factor takes one jittered point per stratum, computed in
`f64` and clamped after the cast, and a factor of `m` levels takes level `stratum * m / N` in
`u128`, the lowest level its stratum covers, so each level lands in `N / m` configs, rounded down
or up. That lattice is deliberate. It balances the levels exactly, and the design seed decides
only which config takes each level. `design_rng.rs` (`DesignRng`)
draws both sampled designs from a design seed through `xorshift64` and `next_index` in integer
arithmetic, with a `u64` path for a full `u32` span, and a seed gives the same design on every
platform. `design_csv.rs` reads a design table, one column per param id or `action.<name>`,
through `parse_records`. `seed.rs` derives each run's seed from one root through `mix_seed`, and
`design_seed` a sampled block's when the spec gives none. Under `SeedScheme::Common`, replicate
`r` of every config shares a seed (common random numbers), and `independent_run_seed` mixes the
config in. `measure.rs` fixes the sampled ticks, `warmup + j * stats_every` plus the total
(`MeasurePlan`), and folds each sample into the reducers and a `SeriesBuffer` (`Sampler`),
skipping and flagging a value that is not finite. `Sampler::push` reports whether the
`StopCondition` holds at that sample. `stop.rs` reads a condition such as `Infected <= 0`, split
at the last run of comparator characters so a label can hold spaces and comparators, and NaN
never meets one. `reducer.rs` binds
the `ReducerKind`s to `StatColumns` indices: final, min, max and mean by default, and argmax,
argmin, `first<=10` (`FirstCrossing`) and `mean@200..600` (`WindowMean`) on request. `outcome.rs`
holds `PlannedRun`, `RunStatus` (`timed_out` among the failures), `StopReason` and `RunOutcome`,
`summary.rs` the replicate statistics of `summary.csv` (`RunningMoments`, `student_t_975`,
`SummaryAccumulator`), and `fingerprint.rs` the FNV-1a hashes naming a schema, a plan and a run.
`schema_hashes_are_unchanged_since_0_2_0` (henad-explore's `schema.rs`) pins every example
model's schema hash to the value 0.2.0 recorded, through the procedure in
`crates/henad-models/tests/fixtures/docs/schema-hashes-0.2.0.md`. Keep it.
`replay.rs` holds `Replay`, the model, params, seed, schedule and ticks of one run as a live
simulation rebuilds it, and `Plan::replay(run_id)` builds one from a plan. `plan_hash` leaves out
the replicate count and the timeout, and a resume can change both. Unlike `DefaultHasher`,
FNV-1a is stable across Rust releases. `export/csv.rs` formats and escapes CSV fields and reads
RFC 4180 records back, and `StatColumns` (`export/stats_csv.rs`) is the column layout
`StatsWriter` fixes from the first sample. A sweep fixes its `series.csv` columns, reducer
bindings and stop column with the same type.
`explore/search/` holds the searches, pure and seeded like the designs. `mod.rs` is the ask/tell
`Searcher` trait. `ask(max)` returns up to `max` `Candidate`s, each an id, a `Genome`, a
`replicate_offset` and a `CandidateOrigin`, and `tell` takes their `Evaluation`s sorted by
candidate id, one row of watched column values per replicate. `SearchSpec` holds the algorithm,
`max_evaluations`, `batch_size`, an optional `Objective` (`column`, `Goal`, `Aggregate`) and the
space as `FactorSpec`s, and `SearchSpec::searcher` builds the searcher from `search_seed(root)`.
`CandidateTracker` numbers the candidates and holds the budget, a count of evaluations with the
re-evaluations among them. `genome.rs` (`SearchSpace`, `Genome`) resolves the space as a sampled
design does, keeps a repeated listed level once at its first place (0.0 and -0.0 as one level),
and decodes a gene `u` in `[0, 1]`: linearly for an `f32` range, and as
`floor(u * m)` for whole numbers and listed levels. A mutation of an ordered gene is the
triangular step `u + scale * (r1 + r2 - 1)` reflected at the bounds, a listed-level gene is drawn
again, and crossover is uniform. No draw goes through libm, and a trajectory is the same on every
platform. `evaluation_log.rs` (`EvaluationLog`) files every replicate value under its candidate,
a re-evaluation's under the candidate it repeats, and scores a failed replicate as `Goal::worst`.
Among equal scores the lower id ranks first. The four searchers are `random.rs`, `hill_climb.rs`
((1 + lambda) with lambda the batch size, a restart after `patience` batches without a strict
improvement, an optional re-evaluation of the incumbent, and starting points drawn through
`draw_config` as neighbours are, so no two first evaluations share a config), `genetic.rs`
(generational, tournament selection, uniform crossover, per-gene mutation, children bred as a
generation starts from the fitness before its re-evaluations, `elite_count` survivors chosen after
`ceil(reevaluate_fraction * population)` re-evaluations of the best members, fitness pooled over
every replicate a member has, no batch spanning two generations, and `MAX_POPULATION` and
`MAX_TOURNAMENT_SIZE` of `1 << 16`) and `pse.rs` (Pattern Space
Exploration over two `PatternAxis` outputs, with a `BTreeMap` archive keyed by `PatternCell`
holding the hits and the first candidate to land, and parents taken from a two-way tournament on
fewest hits after `initial_samples` random candidates). The initial samples fill batches of their
own, and without any the first candidate is alone in its batch. After them, an ask on an empty
archive draws a whole random batch. `PatternSpaceSettings::batch_count` gives the number of
batches, and `PatternAxis::cell_index` puts NaN in cell 0, outside. An axis with neither `min` nor
`max` has an automatic range: the searcher holds every evaluation until the initial samples are
told, takes the range from their outputs (`automatic_range`, 5% margin), then files the held ones
in candidate order. `Searcher::pattern_settings` returns the settings once both axes have a range,
and `SearchUpdate`, `SearchHistory` and the manifest's `axis_ranges` carry them.
`SearchHistory::read` takes the range again from the initial samples' rows of `evaluations.csv`
and places the rows written before it. `search/tests/` holds the driver the searcher tests share
(`support.rs`) and the protocol tests every searcher passes.
