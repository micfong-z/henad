# henad-explore

**henad-explore**: sweeps and searches, a sibling of henad-models over henad-compute, below the
two front ends. `sweep.rs` holds `run_spec`, which runs a sweep, or a search for a spec with a
`[search]` table, into a `SweepOutput`, and `plan_spec`, the dry run, which reads a folder as a
resume would. Both acquire a device for a GPU model handed none (`sweep_device`), and the
manifest records the adapter of the context the sweep steps on. `SweepOptions::new(provenance)`
gives the defaults, and `apply_execution` copies a spec's `[execution]` table, which a host
applies before its own settings. A sweep plans a `SweepSpec`, checks every config of a GPU model
against the device (`probe.rs`, `check_capacity`), and builds the first config without a fault
(`ProbeReport`) to fix the stat columns and bind the reducers and the stop condition. It builds
the last config as well, and the larger of the two sizes the lanes or tracks and the projected
memory. A config that faults is left for its runs to record. A scatter grid sizes its scratch to
the pool. Lanes narrower than the global pool get the probe rebuilt on a pool of their width, and
the memory cap reads that build. It then writes the manifest, runs every pending run, rebuilds the
summary and replaces the manifest. A sweep that fails after the manifest exists marks it `failed`
when it can. A dry run stops after the probes. `spec_file.rs` is the TOML form of a spec
(`SpecFile`), and `LoadedSpec` a spec file read whole, with its `SpecSource` and
`ExecutionTable`. A sweep, a search and a pumped sweep install no panic hook. A host's `main`
does, as do `henad_cli::run`, the app's entry points and `assert_set_conforms`, and the test
helpers `support::sweep` and `sweep_with` install it before each sweep. Every table is
`deny_unknown_fields`. A param value stays the text `--set` takes, and a spec file and a command
line hand `parse_value` the same text. A `table` block's `file` is
read relative to the spec file, and only a table design's. A path that is absolute or holds `.` or
`..` is refused (`SpecFileError::TablePath`). The manifest's copy of the spec carries the table
inline as `table_text`, and the writer emits `design_seed` for a sampled design alone. `specs/`
holds real spec files and a design table. The docs include each spec by `--8<--` region and the
table whole, and a test plans the specs. `schema.rs` (`schema_json`) is the `--params --json`
object, also embedded in the manifest. `cursor.rs` (`RunCursor`) owns one CPU run from its build
to its `RunOutcome`. `advance(max_steps)` steps it a slice at a time and takes every sample due on
the way, `prepare_view` then `stats()`. It fires tick 0's actions before the first step and each
later tick's after the step that reaches it (`Fire::AfterStep`), through `Schedule::run_due`. The
stepper tests for an empty schedule once per slice, and a run with no actions steps with nothing
else in its loop. A refused action goes in the note and leaves the status alone. The run ends on
the first sample where its stop condition holds, or after the first slice past its timeout
(`timed_out`, checked by wall clock between slices). A fault ends the run with its status, and the
samples before it are kept. A cursor refuses a GPU model. A GPU run steps on a track of
`exec/gpu.rs` under the same rules, and both end a run through `run_outcome`. `exec/mod.rs` holds
`Concurrency`, `choose_layout` (the auto rule), `ExecutionLayout`, `GpuTrackDepth`, `SweepControl`
(pause and abort, one atomic load per slice), `Executor` and the reorder buffer that commits
outcomes to a `RunSink` in request order. `choose_layout` sizes CPU lanes from the probe's
`parallel_jobs`, and GPU tracks as `gpu_memory_budget` over the probe's `Demand`, at most
`MAX_AUTO_GPU_TRACKS`, with a single track from a population of `LARGE_GPU_POPULATION`. Each run
starts its slices at one step. A slice carried over from a lighter run once held off a pause for
minutes. `exec/cpu.rs` runs one `thread::scope` lane per rayon pool, each pool built once in
`Executor::new` with `threads_per_lane` workers, and each run built and stepped inside
`pool.install`. A single lane gets a pool of its own as well, even at the global width, so a
paused run waits on a worker of its lane's pool and never on one of the global pool. Other hosts'
passes share the global pool. A target that cannot spawn threads builds no pool and drives each
run on the calling thread.
A panic on a lane's thread or in the sink aborts the `SweepControl` (`AbortOnPanic`). The other
lanes then stop within a slice instead of stepping runs nobody commits. `exec/gpu.rs`
(`run_on_tracks`) is the counterpart of `exec/cpu.rs` for a GPU model. Error scopes are
thread-local, and every `GpuTrack` lives on the calling thread. The `Interleaver` builds a queued
run once a track is free and the summed `entry.demand()` bytes of the live runs and its own fit
the budget (`--gpu-memory`, else the device's `max_buffer_size`). A run that fits beside none runs
alone. An out-of-memory build beside live runs goes back to the head of the queue, drops the track
cap by one and holds admission until a live run ends. Each round reads the `SweepControl` once and
visits every track once. A visit takes a landed sample through `poll_stats_readback(false)`,
submits the actions due at the track's tick, each on its own, then one `stepping::submit_slice` of
at most `steps_per_submission` steps, cut at the next tick a sample or an action is due at. A
track keeps at most `submissions_per_track` buffers on the device, and records the interval after
a sample while that sample reads back. The sample's tick is kept at encode time. By the time the
readback lands, `state.tick()` has moved on. A round in which no track moved waits for the oldest
submission through `stepping::await_submission`. A blocking readback poll would drain the whole
device and stall every track. Each action and slice runs inside `catching_on`, and its fault ends
its own track. A fault left in `ctx.faults` ends every live track, and a run that ended earlier in
the round keeps its end. A run ends at its last landed sample, and the steps recorded past a
sample that stops it are dropped with the state. Each round's time is split evenly between the
live tracks, and a track's clock counts its share, builds and pauses aside. The timeout, `wall_ms`
and `steps_per_s` read that clock. A lost device ends the batch as `BatchEnd::DeviceLost`, the
manifest reads `incomplete`, and `--resume` runs the rest. `output/`
writes `runs.csv` (an `action.<name>` column per action after the params), `series.csv`,
`summary.csv` (rebuilt at the end of a sweep, a resume or a merge by `write_summary`, which
streams `runs.csv` one record at a time and holds one cell string per config) and
`manifest.json` (written `running`, then replaced by a rename). `OutputDir` refuses a directory
that holds any of the four, a search table or any `*.staged` file unless the sweep resumes, and
`OutputWriter` writes and flushes a run's series before its row. An `OutputDir` holds the
operating system's advisory lock on `.lock` (`DirLock`, `File::try_lock`) while it lives, and a
second writer gets `OutputError::Locked`. The file stays when the writer ends, and the next writer
locks it again. The lock goes with a killed process. A resume, of a sweep or a search, opens the
directory in `SweepPreparation::new` or `SearchPreparation::new`, before its scan reads the
tables, and holds the lock through its last write. A writer creates afresh each file it replaces
(`create_fresh`), appends to and cuts `runs.csv` and `series.csv` only after `open` refuses a
symbolic link in place of either, and `check_free` counts a dangling link as a result file.
`output/read.rs` reads the tables
back, keeping complete records only (`RunsCsv`, `SeriesScan`), and `merge_series` interleaves
series segments by run id. `record_ends` follows the CSV grammar (`RecordScan`), and a quote
inside an unquoted field fails its record instead of hiding the records after it.
`output/resume.rs` (`ResumeScan`) accepts a directory only with the same model id, plan hash,
schema hash and shard, and no more replicates than the sweep runs. It keeps `ok`, `non_finite`
and, without `--retry-failed`, failed runs, always reruns a `timed_out` one, renumbers kept runs when the
replicate count rises, and repairs the tables by truncation or by a rewrite. A rewrite stages both
tables as `*.staged`, marks them complete with `tables.staged`, then renames both, and
`OutputDir::open` finishes a rename a process left part done. `merge.rs` joins shard directories
of one plan, replicate count and shard count, merges both tables by run id, rebuilds the summary
and records the inputs in `merged_shards`. It credits the last session of a `running` or `failed`
shard as a resume would, and `replays_exactly` reads `true` only when every shard's does. Missing
runs are a warning and an `incomplete` manifest. A resume of the merged directory fills them in.
Provenance: `ENGINE_BUILD` (`lib.rs`) is
Henad's build, stamped by henad-explore's `build.rs`, and `RecordedBuild::engine()` records it
under the name `henad` with `crate_versions` and `crate_hashes` (henad-compute's from the hidden
`__COMPUTE_BUILD`). `Provenance::new(host, arguments)` holds it beside the host's build. Every
manifest session records `engine`, `host` and `model_source` as `RecordedBuild`s, and
`model.replays_exactly` reads `true` when absent. `RecordedBuild::same_build` compares package,
version, the engine crates' versions and hashes, then the commits of two clean builds, then the
source hashes, and two unidentified builds are never the same. A commit with `dirty: null` counts
as clean only without a source hash, as in a 0.2 session. A resume, a search resume and a
merge warn `SweepWarning::BuildChanged` for each differing build of `BuildRole::Engine` or `Model`
(`build_warnings`, `Manifest::recorded_builds`), a merge's with `between_shards` set. A merge
compares each other shard's builds with those of the lowest shard that records any.
`recorded_builds` leaves out a 0.3 session that ran nothing (`ran` 0, unless it is the last
session of a `running` or `failed` manifest) and lists unidentified builds alike once
(`RecordedBuild::reads_as`). The host is never compared. A 0.2 session reads
as the engine build of its commit and the folder's version, and a resume or a merge writes that
build into it (`record_session_engines`). `ResultSet::recorded_builds` reads them.
`a_0_2_0_manifest_still_resumes` resumes the 0.2.0 folder in `tests/fixtures/`, recorded by the
procedure in its `README.md`, and `a_0_2_0_manifest_still_replays_and_merges` replays and merges
it. Both fail when the folder is missing. `src/tests/provenance.rs` holds the comparisons. Keep
them.
Code identity: a model build records `origin` (`runtime`, absent for compiled code) and
`code_hash`, both optional and left out when absent, and `ManifestModel.code`
(`ManifestModelCode`) carries a runtime model's text inline. `FORMAT_VERSION` stays 1.
`RecordedBuild` is `#[non_exhaustive]`. When either build records an origin, `same_build` holds
only when both are runtime code with one known code hash, so runtime code is never the same build
as compiled code. `is_identified` and `describe` account for it. The crate-private
`code_differs` is the rejection rule: either build has an origin and the two are not the same
build. `check_code` (`output/resume.rs`) applies it after the model id and before the schema hash
for a sweep resume (`ResumeScan::read` takes the `ModelSource`) and a search resume, against
every session that wrote runs (`Manifest::run_model_builds`), a session with no model build
counting as other code, and returns `ResumeError::CodeChanged`. Both resumes also call
`check_directory_code` before their probe build, which then never builds changed code. A merge
compares every shard's builds with those of each lower shard, after the model id and before the
plan checks (`MergeError::CodeChanged`). Compiled against compiled keeps `BuildChanged`.
`schema_json` writes `origin` and `code_hash` only for a model that is not compiled, and
`ResultSet::code_matches` and `Manifest::code_matches` answer the rule for a host.
`a_value_built_entry_sweeps_byte_identically_to_the_registered_one`,
`a_resume_refuses_runtime_code_that_changed`, `a_compiled_build_change_still_only_warns`,
`a_merge_refuses_shards_of_two_codes`, `a_manifest_carries_runtime_code_inline` and
`two_codes_under_one_label_are_listed_apart` (`src/tests/provenance.rs`),
`a_search_resume_refuses_changed_code` (`src/tests/search.rs`) and
`schema_json_names_code_only_for_runtime_models` (`schema.rs`) hold it. Keep them.
Behind the `model-files` feature, which turns on henad-compute's `runtime-wgsl`, `model_file.rs`
holds `ModelFile`, a grid rule as one TOML file. `parse` reads line ends as `\n` and maps every
span back to the text as passed (`LineEnds`). It reads the header keys first, ignoring the rest,
so a later version or another kind reads as one before an unknown key is refused, then refuses
an unknown key in every table. The file holds `format = "henad-model"`, `format_version = 1` and
`kind = "gpu_grid"`, then `id`, `name` and `description`, `[grid]` (width, height, boundary),
`[[state]]` (label, a colour of 3 or 4 bytes, `initial`), `[[param]]` in the `--params --json`
vocabulary, and `[step] wgsl`, a string the file holds verbatim, as a `'''` literal does. A step
holding `'''` is refused by `parse` and `from_rule` alike, since the canonical text could not
hold it, as is a step holding an ASCII control character other than a tab or a line end (the C1
characters pass, as TOML allows them). `from_rule` (which reads its canonical text back through
`parse`), `rule`, `to_toml` (the canonical text: keys in a fixed order, one form per value, the
step verbatim),
`code` (a `ModelCode` of `GridRule::CODE_FORMAT` over the canonical text) and `entry`, which runs
`GridRule::check_declarations` and places each problem at its table or key, then compiles and
places each problem of the step at its file line by the string's start offset. `ModelFileError`
is `Toml`, `Format` or `Rule`, each of `CodeDiagnostic`s. `specs/models/` holds the example model
files, which the spec tests, reading the top level of `specs/`, leave alone.
`docs/reference/model-files.md` includes `life_rule.toml` whole, and henad-cli's model file tests
edit copies of it by its text.
`src/tests/model_file.rs` holds `a_model_file_round_trips_through_to_toml`,
`a_step_holds_what_a_literal_string_holds`, `unknown_keys_are_refused`,
`a_declaration_error_points_at_its_table`, `control_characters_in_the_declarations_are_refused`,
`the_canonical_code_ignores_formatting`, `a_wgsl_error_points_at_its_file_line` and
`every_example_model_file_loads`. Keep them.
`progress.rs` is the `Progress` trait the host renders. The library never prints. `device.rs`
(`acquire_headless`) acquires a GPU device with no window or surface, at the WebGPU baseline
raised by `gpu::limits::raise`. An adapter below the baseline, as a GL adapter can be, gets
`DeviceError::BelowBaseline`, and the CLI then runs CPU models only. It is native only. `pollster`
blocks on the request, and a browser cannot block. The crate is in the wasm typecheck with
henad-core, henad-compute and henad-models.
Native-only code sits behind `#[cfg(not(target_arch = "wasm32"))]`. `handle.rs` (`SweepRun`) is a
host's handle on a sweep, with one API on native and in a browser, described under "Sim runs off
the UI thread". `SweepRun::start` plans the spec before it returns, and a browser refuses a GPU
model with `SweepStartError::GpuNeedsNative`. The `gpu` a host passes to `start` or
`resume_directory` is a device it shares with the sweep, `FaultSink` included. Handed none for a
GPU model, the sweep thread acquires a device sized to the entry's `GpuNeeds` through
`acquire_headless`, builds the entry it was handed on it (`sweep_device`), and records that device
in the manifest. `SweepRunOptions::new(provenance)` gives the defaults, and its `spec_source`
goes into the manifest. A device it cannot acquire fails the sweep with `SweepEvent::Failed`.
`acquire_headless` returns the `GpuContext` alone, with its `RuntimeInfo` attached and read back
through `runtime_info()`.
`SweepOutput::Memory` writes the four files through the same writers over `Vec<u8>`
(`output/memory.rs`, `SweepFiles`) and hands them over in the `SweepRecord`.
`SweepOutput::Directory` is native only. `SweepRunOptions::memory_budget` and `gpu_memory_budget`
are the budgets of `--memory` and `--gpu-memory`. `SweepRunOptions::series_budget` caps the bytes of
series the `RunFinished` events carry. From the first run past it, every run arrives without its
series, and the files keep them all. `SweepRun::resume_directory` resumes a folder with the spec
its manifest records, along the path `--resume` takes. `pumped.rs` (`PumpedSweep`) is the
browser's executor, a `runner::SimLoop` that makes one probe build per pump (`PlanProbe`),
prepares the sweep in a pump of its own, then builds and steps one CPU `RunCursor` at a time
into memory, a slice of about half `PUMP_BUDGET_MS` per pump. A pause holds only the runs, and a
sweep or search paused after its last run still ends. It compiles for wasm32 and for tests, and
the tests pump it on native. `result_set.rs` (`ResultSet`) reads an output directory back, through
`open_dir` on native or `from_files` over the bytes of picked files. It needs `manifest.json` and
`runs.csv`, reads complete records only, and holds `series.csv` run by run in file order while a
byte budget lasts. A run the budget cuts holds none of its rows. A `runs.csv` with no complete
header, as a sweep stopped before its first run leaves it, opens with no runs and no value or
reducer columns. `series.csv`'s header has to name `run_id`, `tick` and the manifest's stat
columns. `summary.csv` is left unread. `ResultSet::replay` plans the recorded spec and returns a
run's `Replay`, refusing a run whose config, replicate, seed or run key differ from its row. The
run key hashes the model's declarations too, and is compared only while `schema_matches` holds.
`read_directory_series` reads the series of chosen runs later, given the stat column names.
`testing/`, behind the `testing` feature and always built for henad-explore's own tests, is the
kit a model's tests run. `check_model` runs every `ModelCheck` that applies and returns a
`ModelReport` of `CheckFailure`s and `SkippedCheck`s. It never panics on a model's behalf: a
panic, a device error or a broken contract is a failure of its check, and a GPU check waits for
the device before it ends, so no fault outlives the check that raised it. `check_model_set`
returns a `SetReport`, which also names every model id an override or an exemption names and the
set lacks. `assert_set_conforms` installs the panic hook, panics with every failure of a set, and
prints the report of a set that passes, skipped checks included. An exemption of a check that
does not apply fails that check, and under `HENAD_REQUIRE_GPU` a check a missing device would
skip fails. An override the model refuses fails every check that builds it, one a missing device
skips included. `declared.rs` reads the
declarations and builds nothing, `DefaultSetup` among them, the `RunSetup::from_parts` check the
app's Build makes. `built.rs` builds once per check, `determinism.rs` compares two runs, and
`gpu.rs` (native only) steps a GPU model on the settings' device. A check that builds takes the
five size parameters at 128, or 256 agents, never above the default. `ThreadCount` sets the size
so a step splits into twice the high thread count in jobs: `num_agents` at that many chunks, or
the largest `grid_height` that splits into no more, found by building. One job at every size
within bounds skips it as `OneJob`, one job at a size an override sets as `OneJobAtOverride`,
and wasm32 skips it as `NativeOnly`. The GPU checks and a GPU
model's `Actions` build at the declared defaults. A watchdog trips on time, and a small model
never trips it. `FullSubmission` runs `MAX_STEPS_PER_SUBMISSION` single steps first and reads
their stats, then one full submission, and compares the two. A device the full submission
poisons reads zeros in every later run, and the other order compares zeros with zeros. A model
that does not replay exactly runs the full submission alone, and fails when it declares stats and
every one reads zero. A lost device fails it. A GPU model skips `ThreadCount` (`OtherBackend`),
and one that does not replay exactly skips `SameSeed`, `SeedSensitivity` and `SamplingCadence`
(`InexactReplay`).
`SeedSensitivity` compares the stats and the exported state. `CheckSettings::ticks` refuses fewer
than `MIN_TICKS`. `CheckSettings::set_text` overrides a value in every check that builds, and
`exempt` skips a check with a reason. `headless_test_device` (native only) acquires a device for
a `TestDeviceRequest`, the baseline or `raised(needs)`, with optional features, and a missing
feature returns `None` even under `HENAD_REQUIRE_GPU`. An adapter below the baseline gives no
device for either request, as `acquire_headless` refuses it.
`src/tests/` holds `support.rs` (the headless device, scratch directories, a sweep helper,
`OutputTables`, `CommitLimit`, and `ticks_seen` with `HOLD_WINDOW` for pause checks), `broken.rs`
(`GridModel`s that panic or report a value that is not finite, registered through the public
`register_grid_model`, the `grid_model!` models with one broken declaration or build each,
`SharedAccumulator`, whose kernel shares a counter across chunks, `DeclaresNumAgents`,
`CountsViews`, a network model whose `prepare_view` writes a lane, `DefaultOutOfBounds`, whose
default lies outside its bounds, `OversizedGpuSir`, whose defaults pass the WebGPU baseline,
`BuggyState`, which adds one `Bug` to a CPU state, `BuggyGpuState`, which adds one `GpuBug` to a
GPU state, such as reading zeros on every state of its entry after a full submission, and the
sound `PlacesCellBySeed` and `StatlessGpuBoids`, which the kit passes), `kit.rs` (each broken
model fails exactly the checks its bug breaks), and the
determinism, failure, run control, resume, shard, GPU sweep, GPU
track (`tracks.rs`), handle, result set and replay tests. For a model that replays exactly (every
model but `gpu_boids`), the three CSVs are byte-identical apart from `TIMING_COLUMNS` at any lane
or track count, for merged shards against an unsharded sweep, and for a resumed sweep against a
fresh one, timed-out runs aside. A sweep held in memory, on a thread or pumped, writes the bytes a
directory sweep writes. `OutputTables` compares `series.csv` and `summary.csv` byte for byte, and
`runs.csv` with its timing fields emptied.
`a_sweep_writes_the_same_files_at_any_concurrency`, `interleaved_gpu_runs_match_sequential_ones`,
`ants_results_do_not_depend_on_lane_width`, `merged_shards_equal_an_unsharded_sweep`,
`a_resumed_sweep_skips_finished_runs_and_matches_a_fresh_one`,
`memory_output_equals_directory_output` and
`the_pumped_sweep_writes_what_a_directory_sweep_writes` hold that line.
`a_pipelined_gpu_sample_matches_a_blocking_one` pins a track's pipelined sample to
`stepping::sample_stats`. `gpu_boids` matches only in the parts the engine owns, every `runs.csv`
field up to `population` and the run and tick of each series row, and
`interleaved_gpu_boids_runs_commit_in_plan_order` pins them.
`every_gpu_model_that_replays_has_a_track_case` holds the cases of the two GPU tests above to
every GPU model that replays exactly. Keep them.
`search_run.rs` runs a spec with a `[search]` table, behind `run_spec`. `SearchPlan` plans the fixed
values and actions as a one-config `Plan`, and a search spec with blocks is refused, as is a batch
of more than `MAX_RUNS` runs (`SearchPlanError::BatchTooLarge`). It resolves
the space, checks that every watched column is a reducer column, and hashes the settings that fix
the trajectory with `search_hash`. Unlike `plan_hash`, that hash covers the replicate count, and
it covers the budget. The loop asks the searcher for a batch, runs it through the sweep's
`Executor::run_batch` on lanes or tracks, and tells the searcher the watched reducer values. Run
`r` of candidate `c` has run id `c * replicates + r`, config id `c`, and replicate index
`replicate_offset + r`, seeded by the spec's scheme. Common random numbers then hold across
candidates, and a re-evaluation runs on fresh seeds. `SearchSession::tell` turns each batch into
a `SearchUpdate`, reported as `ProgressEvent::SearchBatchTold`. `output/search_tables.rs` writes
`evaluations.csv`, `batches.csv` and, for a genetic algorithm, `generations.csv` batch by batch,
then `best.csv` or `archive.csv` at the end, and `SearchHistory` reads them back for the app. It
finds the columns after a config by their place from the end of the header, since a param id can
share their names, tells a PSE table by its last column, and checks the trailing cells of a
scored `evaluations.csv`. A
resume replays the searcher from its seed and reads every recorded run's values back from
`runs.csv` (`RecordedSearch`) in place of running it, failed and timed-out runs included. It
refuses a run whose `run_key` differs from the run asked for, and replays every recorded batch
once in `SearchPreparation::new` to check them all before a table is written. A search refuses
`--shard` and `--retry-failed`. The manifest reads `mode` `search`, holds the hash of the fixed
values and actions as `plan_hash` and `null` as `plan.configs`, and adds a `ManifestSearch` with
the search hash. A search that fails records its standing at the failure there. `SweepRun` and
`PumpedSweep` run searches as well, and `ResultSet` reads
a search folder back with its tables. `specs/sir_search_genetic.toml` and
`specs/sir_search_pse.toml` are the example searches the docs include. `src/tests/search.rs` holds
`every_example_search_spec_parses`, `a_search_writes_the_same_tables_at_any_concurrency`,
`a_resumed_search_follows_the_same_trajectory`,
`a_resume_that_meets_a_changed_run_leaves_the_directory_alone` and
`a_gpu_search_writes_the_same_tables_on_any_track_count`. Keep them. A resume plans the spec its
manifest records, and the workspace turns on serde_json's `float_roundtrip` so that a bound or a
rate reads back bit for bit. `a_search_resumes_from_a_manifest_holding_a_seventeen_digit_axis_bound`
(`src/tests/manifest_floats.rs`) holds it. Keep it.
