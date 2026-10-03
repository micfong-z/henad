---
date: 2026-10-02
title: "Library M10a: the testing kit"
description: "The tenth milestone of #48, first part. henad-explore gains henad_explore::testing behind a testing feature, the contract checks every registered model can run, with headless_test_device, and the example models' registry tests and test devices move onto it."
icon: material/package-variant
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#48"
state: M10a implemented with its review folded in, `HENAD_REQUIRE_GPU=1 ./check.sh` and the docs build green, the kit's wall time measured before and after the review
baseline_commit: d374503
delta_state: uncommitted on `48-library`, on top of M9's commit
---

# Library M10a: the testing kit

> M10a is the first part of the tenth of the eleven milestones of #48, which turns Henad into a library published on crates.io.
> `henad_explore::testing`, behind henad-explore's new `testing` feature, checks any model entry against what its state does and returns a report instead of panicking.
> It holds twenty `ModelCheck`s in four groups, `CheckSettings`, `ModelReport`, `TestDeviceRequest` and the native-only `headless_test_device`.
> The example models' registry tests became one kit run that asserts both conformance and each model's skipped checks, and every test device outside henad-compute now comes from `headless_test_device`.
> A review pass followed. It reordered `FullSubmission` so a poisoned device cannot pass it, made the settings' mistakes visible, and gave every check a self-test.
> `cpu/grid_engine.rs`'s thread-count test takes a 128 by 1024 grid, the named exception of [4.9.4].

## State before

`48-library` stood at 4e1bd2b (M8), with M9's work uncommitted in the main checkout, `HENAD_REQUIRE_GPU=1 ./check.sh` at 1046 tests.
The maintainer committed M9 as d374503 before the review pass.
`crates/henad-models/src/tests/registry.rs` held 22 tests, 14 of them contract tests over `example_models()` that picked GPU entries by backend.
They protected the ten example models alone, and a downstream author had none of them.
Headless device set-up and `HENAD_REQUIRE_GPU` parsing were copied six times: henad-compute's and henad-models' `support.rs`, henad-explore's `headless_device` and `baseline_device`, the CLI's `SmallGpuGrid` and golden test, and the app's `AppState::headless`.
`cpu/grid_engine.rs`'s `results_do_not_depend_on_the_thread_count` ran a 64 by 64 grid, one rayon leaf, and compared one job with itself.

## What was done

### The kit

`crates/henad-explore/src/testing/` is compiled with the `testing` feature, and always for henad-explore's own tests.

- `mod.rs` holds `ModelCheck` (20 variants, `ModelCheck::ALL` as a slice in run order), `check_model`, `check_model_set` and `assert_set_conforms`.
  Each check runs inside `catching`, or `catching_on` for a GPU model. A GPU check then waits for the device, and a fault its work left in the sink is that check's failure, so no fault outlives the check that raised it.
  A check never panics on a model's behalf.
  Stats are compared bit for bit through `stat_bits`, histograms included.
- `check_model` fails a check that `CheckSettings::exempt` exempts when the check does not apply (another backend, or a comparison of runs on a model that does not replay exactly). Under `HENAD_REQUIRE_GPU`, a check a missing device would skip fails instead.
- `check_model_set` returns a `SetReport`, the reports in the set's order plus `unknown_models`, every model id an override or an exemption names and the set lacks. `assert_set_conforms` installs the panic hook, panics with the report when anything failed, and prints it when the set passes, skipped checks included. libtest shows the print under `--nocapture`.
- `report.rs` holds `ModelReport` (`model_id`, `failures`, `skipped`, `passed`, `assert_passed`, plus `skip_reason` and `thread_count_jobs`), `SetReport`, `CheckFailure`, `SkippedCheck` and `SkipReason`: `OtherBackend`, `InexactReplay`, `Exempt(reason)`, `NoDevice`, `NativeOnly` and `OneJob`.
  A report's first line is a summary, "Model 'x' failed 2 of its checks: Actions, Views.", and the failures and the skipped checks follow, one to a line.
- `settings.rs` holds `CheckSettings` with `gpu`, `ticks` (20 by default, at least `MIN_TICKS`, 8), `thread_counts` (1 and 7), `set_text` and `exempt`.
  A check that builds takes `grid_width`, `grid_height`, `world_width` and `world_height` at 128 and `num_agents` at 256, never above the default and clamped to the bounds, then the overrides.
- `declared.rs`: `ModelId` (through `ModelSet::insert` into an empty set, so the grammar has one home), `ParamIds` (naming a repeat of an engine-prepended id), `StatLabels`, `ActionIds`, `Palette`, `Metadata`, `DefaultSetup` and `DefaultsFit`.
- `built.rs`: `ApplyModes`, `Views` (the factory's arm, then the CPU views or the GPU layers against the hint), `ParallelJobs`, `Actions` and `StatCount`, each on a build of its own. A GPU model's `Actions` builds at the declared defaults.
- `determinism.rs`: `ThreadCount`, `SameSeed`, `SeedSensitivity` and `SamplingCadence`, through `RunSetup::from_parts` and `Simulation`, each comparing the stats and, for a CPU model, `write_state`'s bytes.
  `ThreadCount` sizes the work for twice the high thread count, 14 jobs: an agent or network model takes `num_agents` at 14 chunks from `Structure::Agents` or `Structure::Network`, and a grid model the largest `grid_height` that splits into no more than 14 jobs, found by doubling and bisecting over builds, 896 rows at 128 columns.
  An override of the size parameter keeps its value, one job at every size within bounds skips the check as `OneJob`, and wasm32 skips it as `NativeOnly`.
  A model that does not replay exactly skips `SameSeed`, `SeedSensitivity` and `SamplingCadence`.
- `gpu.rs` (native only): `BaselineBuild` (the build succeeds exactly when the shortfalls against the device's limits are empty), `FullSubmission` and `SampledSlice`.
  `FullSubmission`, on a model that replays exactly, first runs 64 submissions of one step and reads their stats, then the old full submission (64 steps and the snapshot passes in one command buffer, then a blocking readback), and compares the two. A device the full submission poisons reads zeros in every later run, and the other order compares zeros with zeros. It fails on a lost device, a readback that did not land, or a tick other than 64. `gpu_boids` keeps the old form after the full submission: some stat is not zero.
- `device.rs`: `TestDeviceRequest` (`baseline`, `raised(needs)`, `features`), `headless_test_device`, which asks for a high-performance adapter, returns `None` for a missing optional feature even under `HENAD_REQUIRE_GPU`, and attaches the adapter's `RuntimeInfo`, and the one reading of `HENAD_REQUIRE_GPU` the kit makes.
- `henad_compute::gpu::wgpu` re-exports wgpu, the path [4.1] names, and `gpu_game_of_life`'s timing test names `henad_compute::gpu::wgpu::Features::TIMESTAMP_QUERY` through it.

### The old registry tests, check by check

No assertion of the 14 contract tests was dropped.

| Old test | Covered by |
|---|---|
| `every_default_setup_passes_the_checks_of_from_parts` (the M6 review's check) | `DefaultSetup`, both assertions. [4.8]'s `DefaultsFit` reads the GPU limits only and does not cover it |
| `declared_apply_mode_matches_what_the_state_accepts` | `ApplyModes` |
| `declared_topology_matches_the_views_the_state_returns` | `Views`, CPU arm |
| `declared_topology_matches_the_layers_a_gpu_state_publishes` | `Views`, GPU arm |
| `only_a_cpu_model_reports_how_far_a_step_splits` | `ParallelJobs` |
| `declared_metadata_matches_the_entry_it_describes` | `Metadata` (capacity, needs, CPU replay, structure against hint and backend), and `Views` for the factory's arm |
| `every_declared_action_is_accepted_by_the_state` | `Actions`, at the defaults on a GPU model as before |
| `action_ids_are_unique_within_a_model` | `ActionIds` |
| `a_declared_palette_has_colours_in_it` | `Palette` |
| `every_declared_stat_series_gets_a_value` | `StatCount` |
| `every_gpu_model_builds_on_a_baseline_device` | `BaselineBuild` (the build and the agreeing demand) and `DefaultsFit` (the shortfalls against `Limits::default()`) |
| `every_gpu_entry_reports_its_capacity` | `Metadata` (a GPU demand of some bytes, no CPU demand) |
| `a_full_submission_executes_every_step` | `FullSubmission`. For a model that replays exactly, the non-zero stat became a comparison with single steps run before it |
| `a_sampled_slice_reads_back_what_a_snapshot_does` | `SampledSlice`, and the fault-sink check of every GPU check |

Kept beside `example_models()`: `every_gpu_entry_needs_the_bindings_its_widest_pass_binds`, `a_model_too_large_for_the_device_is_reported`, `every_example_model_joins_the_set`, `an_example_entry_inserted_into_another_set_keeps_its_source` and `a_gpu_entry_refuses_to_build_without_a_device`.
[4.8] drops `StorageBuffers` from the kit, and the first of these keeps that assertion for the example models.
The engine's own tests stay: `a_model_that_panics_while_building_comes_back_as_a_fault`, `a_kernel_that_panics_mid_step_keeps_its_location` and `a_model_entry_can_be_shared_between_threads`.

`the_example_models_conform` runs the kit once through `check_model_set`, asserts that the set report passed, then checks every model's skip list exactly: the four GPU checks as `OtherBackend` on a CPU model, `ThreadCount` as `OtherBackend` on every GPU model, `SameSeed`, `SeedSensitivity` and `SamplingCadence` as `InexactReplay` on `gpu_boids`, and `NoDevice` on the GPU models' builds when the machine gives no device.
It also asserts that every CPU model reached more than one job.
`the_example_gpu_models_are_the_gpu_backend_entries` asserts that the GPU models are the four `gpu_` entries, picked by backend, and runs no kit.

### Test devices

- henad-models' `src/tests/support.rs` is deleted. Each GPU model's `headless_context`, the tutorial's GPU tests, the parity tests and `simulation.rs` call `headless_test_device(&TestDeviceRequest::baseline())`.
- `gpu_game_of_life`'s timing test asks for `TestDeviceRequest::baseline().features(henad_compute::gpu::wgpu::Features::TIMESTAMP_QUERY)`, and still skips on an adapter without it.
- henad-explore's `headless_device` and `baseline_device` are one line each over the kit, raised to the example set's needs and at the baseline.
- The CLI's `SmallGpuGrid::new`, its golden test's `has_adapter` and the app's `AppState::headless` go through `TestDeviceRequest::raised`.
- henad-compute keeps its own helper, as [4.8] says.
- henad-models takes henad-explore as a path dev-dependency with `testing`, and its unused `pollster` dev-dependency went. henad-cli and henad-app add `testing` to the henad-explore they already depend on, for their tests.

### Kit self-tests

`crates/henad-explore/src/tests/kit.rs` runs the kit over each model of `tests/broken.rs` and expects exactly the failing checks. Fourteen of the twenty checks have a self-test that fails them, and the issues below list the other six.

| Self-test | Model or wrapper | Fails exactly |
|---|---|---|
| `a_bad_id_fails_the_model_id_check` | `BadId` ("Bad Id") | `ModelId` |
| `a_parameter_repeating_an_engine_id_fails_the_param_ids_check` | `DeclaresNumAgents`, an agent model | `ParamIds` |
| `a_repeated_stat_label_fails_the_stat_labels_check` | `RepeatsStatLabel` | `StatLabels` |
| `a_repeated_action_id_fails_the_action_ids_check` | `RepeatsActionId` | `ActionIds` |
| `an_empty_palette_fails_the_palette_check` | `EmptyPalette` | `Palette` |
| `a_state_that_accepts_every_edit_fails_the_apply_modes_check` | `BuggyState` over `sir`, `Bug::AcceptsEveryEdit` | `ApplyModes` |
| `a_state_that_hides_its_grid_fails_the_views_check` | `Bug::HidesGrid` | `Views` |
| `a_state_that_refuses_its_actions_fails_the_actions_check` | `Bug::RefusesActions` | `Actions` |
| `a_state_that_drops_a_stat_fails_the_stat_count_check` | `Bug::DropsLastStat` | `StatCount` |
| `a_build_that_reads_the_pool_width_fails_the_thread_count_check` | `ReadsPoolWidth`, as many live cells in its first row as the pool has threads | `ThreadCount` |
| `a_shared_accumulator_fails_the_thread_count_check` | `SharedAccumulator`, a `Mutex<u64>` its chunks share | `ThreadCount` among others, at 14 jobs |
| `a_build_that_differs_from_the_last_fails_every_check_comparing_two_builds` | `CountsBuilds`, a static counter mixed into `init` | `ThreadCount`, `SameSeed`, `SamplingCadence` |
| `a_model_that_draws_no_random_number_fails_the_seed_sensitivity_check` | `DividesByParam` at its defaults | `SeedSensitivity`, and passes with the exemption the report lists |
| `a_view_preparation_that_writes_a_lane_fails_the_sampling_cadence_check` | `CountsViews`, a network model whose `prepare_view` counts in a lane | `SamplingCadence` |
| `a_full_submission_that_reads_zeros_fails_the_full_submission_check` | `ZeroesFullSubmissions` over `gpu_game_of_life` | `FullSubmission` |
| `a_kernel_panic_fails_every_check_that_steps` | `DividesByParam` at `divisor=0` | the four determinism checks, naming `divide by zero` and `broken.rs` |
| `a_build_panic_fails_every_check_that_builds` | `DividesByParam` at `init_divisor=0` | the nine checks that build |
| `a_stat_that_stops_being_finite_breaks_no_contract` | `InverseOfCountdown` | `SeedSensitivity` alone |
| `an_exemption_of_a_check_that_does_not_apply_fails_it` | Game of Life exempting `FullSubmission` | `FullSubmission` |

`DividesByParam` and `InverseOfCountdown` draw no random number and fail `SeedSensitivity` on purpose. The grid models above come from a `grid_model!` macro in `broken.rs`, each with one broken declaration or build.
`BuggyState` replaces `RefusesActions`, which `run_control.rs` used, and forwards the views it did not.
`ZeroesFullSubmissions` shares one stopped flag among every state its entry builds, since every buffer of a device the watchdog stopped reads zero. Put back in the old order (the full submission first), `FullSubmission` passed that model. The mutation was run and reverted.
Three more: Game of Life passes at 14 jobs and skips the four GPU checks as `OtherBackend`, a set report names `game_of_lfe` and `missing` from an override and an exemption, and `assert_set_conforms` panics with "1 of 1 models failed their checks" and the model's summary line.
No self-test covers `Metadata`, `DefaultSetup`, `DefaultsFit`, `ParallelJobs`, `BaselineBuild` or `SampledSlice`. A model breaking one of them needs a hand-built entry, or a GPU model past the baseline.
Correction after a later review: `register_grid_model` accepts a default outside its bounds, so `DefaultSetup` needs no hand-built entry, and `a_default_outside_its_bounds_fails_every_check_through_a_run_setup` now covers it.

### The review

The maintainer's review of the first pass came in eight parts, all folded in above.

1. `FullSubmission` compared zeros with zeros on a poisoned device. The single steps now run first, a lost device fails, and `ZeroesFullSubmissions` pins it.
2. A missing device under `HENAD_REQUIRE_GPU` skipped the GPU checks silently. They now fail, through the kit's one reading of the variable, and `assert_set_conforms` prints the skipped checks of a set that passes.
3. `SeedSensitivity` compared the stats alone. It now compares the exported state too, and skips as `InexactReplay` on a model that does not replay exactly.
4. Settings naming a model the set lacks were ignored. `SetReport::unknown_models` names them, and an exemption of a check that does not apply fails it.
5. A GPU model's `Actions` now runs at its declared defaults, as the old test did.
6. Self-tests for every check that had none, the deterministic thread-count one included, and `kit.rs`'s doc and AGENTS.md no longer say "only that one".
7. `ModelCheck::ALL` became a slice, `check_model` and `check_model_set` take `#[must_use]`, `ThreadCount` skips as `NativeOnly` on wasm32, leftover faults are reported against the check that left them, `CheckSettings::ticks` refuses fewer than 8, the registry runs the kit once, `assert_passed` opens with a summary line, and the determinism page says a model past the baseline exempts `DefaultsFit`.
8. Rerun, remeasure and propose two commits, below.

### The named exception

`cpu/grid_engine.rs`'s `results_do_not_depend_on_the_thread_count` takes a 128 by 1024 grid, 16 jobs of 64 rows, with its assertion unchanged.
[4.9.4] asks for this in a commit of its own.

### Docs and AGENTS.md

- `authoring/determinism.md`'s "Tests the registry brings" became "The testing kit", with the example models' test and an exemption included by `--8<--` region from `registry.rs` and `kit.rs`, a table of the checks, the sizes, the skips, the `DefaultsFit` exemption for a model past the baseline, the settings' mistakes and the panic hook.
- `registering.md`, `porting.md`, `parameters.md` and `statistics.md` name the kit's check where they said "a registry test".
- AGENTS.md: the test-module rule and the two `support.rs` files, the dev-only edge from henad-models to henad-explore, the crate list, the registry tests, a `testing/` passage under henad-explore, the broken models and `kit.rs`, "Adding a new model", the thread-count and watchdog notes, and `BaselineBuild` in place of `every_gpu_model_builds_on_a_baseline_device`.
- CHANGELOG: Added lines for the kit, `headless_test_device` with `TestDeviceRequest`, and `henad_compute::gpu::wgpu`. Nothing public was removed or changed, and no line carries "Breaking:".

### Edited tree

```text
.
├── AGENTS.md                               ~ the kit, the test devices, the dev-only edge
├── CHANGELOG.md                            ~ M10a's Added lines
├── Cargo.lock                              ~ henad-models' dev-dependencies
├── zensical.toml                           ~ nav entry #39
├── crates/henad-compute/src/
│   ├── gpu/mod.rs                          ~ pub use wgpu
│   └── cpu/grid_engine.rs                  ~ the thread-count test at 128 by 1024
├── crates/henad-explore/
│   ├── Cargo.toml                          ~ testing feature
│   └── src/
│       ├── lib.rs                          ~ pub mod testing
│       ├── testing/                        + mod, report, settings, device, declared, built, determinism, gpu
│       └── tests/
│           ├── mod.rs                      ~ mod kit
│           ├── kit.rs                      + a self-test per check, the set report, assert_set_conforms
│           ├── broken.rs                   ~ grid_model!, the agent, network and wrapper fixtures, BuggyState
│           ├── run_control.rs              ~ BuggyState in place of RefusesActions
│           └── support.rs                  ~ headless_device and baseline_device over the kit
├── crates/henad-models/
│   ├── Cargo.toml                          ~ henad-explore with testing, pollster out
│   └── src/
│       ├── gpu_{game_of_life,sir,boids,ants}/mod.rs   ~ headless_test_device, the timing request
│       └── tests/
│           ├── mod.rs                      ~ support gone
│           ├── support.rs                  - replaced by headless_test_device
│           ├── registry.rs                 ~ one kit run with the skip lists, the guards
│           ├── simulation.rs               ~ headless_test_device
│           └── tutorial/{gpu_life,gpu_foraging,parity}.rs   ~ headless_test_device
├── crates/henad-cli/
│   ├── Cargo.toml                          ~ henad-explore with testing for the tests
│   ├── src/lib.rs                          ~ SmallGpuGrid over the kit
│   └── tests/golden.rs                     ~ has_adapter over the kit
├── crates/henad-app/
│   ├── Cargo.toml                          ~ henad-explore with testing for the tests
│   └── src/state.rs                        ~ AppState::headless over the kit
└── docs/
    ├── authoring/{determinism,registering,porting,parameters,statistics}.md   ~ the kit
    └── developing/agent-record/20261002-39-library-testing-kit.md   +
```

## State after

M10a is implemented with its review and uncommitted on `48-library`, on top of d374503, in the main checkout as asked. Nothing is staged.

- `HENAD_REQUIRE_GPU=1 ./check.sh` passes: 1057 tests, none failed, with the wasm32 typecheck, packaging, cargo-deny, docs and the web build. M9 passed at 1046, and the first M10a pass at 1042. The 14 contract tests became 2, and `kit.rs` holds 23.
- check.sh's `--all-features` wasm32 line typechecks henad-explore with `testing`. A first pass warned on `TestDeviceRequest::needs`, which only native code reads, and it is gated.
- `uv run --locked zensical build` passes with no issues, and the determinism page renders both included regions.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings -W clippy::all` passes.

### The kit's wall time

Measured under `cargo test` on this machine, where the workspace members build at opt-level 0 and dependencies at 2, with `HENAD_REQUIRE_GPU=1` and the baseline device.

- First pass, `the_example_models_conform` alone with `--exact --test-threads=1`, as a whole process: 10.33 s on the first run, then 5.44 s and 5.44 s.
- After the review, the one kit run of the registry, measured the same way four times: 5.91, 5.71, 5.70 and 5.82 s. The review added 64 single steps per exact GPU model to `FullSubmission`, and two runs to `SeedSensitivity`'s export. The registry used to run the kit twice, and now runs it once, so `cargo test -p henad-models` spends about five seconds less.
- Per model after the review, timed by a temporary test that was removed afterwards: sir 0.24 s, boids 0.43 s, game_of_life 0.30 s, ants 0.57 s, virus_network 0.06 s, team_assembly 0.03 s, gpu_game_of_life 0.13 s, gpu_sir 0.19 s, gpu_boids 0.51 s, gpu_ants 0.08 s. That is 2.5 s of checks, against 2.4 s before, and every CPU model reached 14 jobs.
- The rest of a process's time is the test binary's start and the device request.

[7.1] holds root overrides of the opt-level for henad-models and henad-compute in reserve before any shrinking of `ThreadCount`'s size, and at these times neither is needed.

Proposed commits, in order:

1. `test: grid thread-count test at 16 jobs`, `crates/henad-compute/src/cpu/grid_engine.rs` alone, the named exception of [4.9.4].
2. `feat: testing kit`, everything else.

## Issues found & future directions

- **The GPU checks build at the declared defaults.** [4.8] has every built check shrink the sizes. `BaselineBuild`, `FullSubmission`, `SampledSlice` and a GPU model's `Actions` build at the defaults plus the overrides instead, as the old registry tests did. A watchdog trips on the time one command buffer runs, and a 128 by 128 grid never runs long enough to trip it, which would leave `FullSubmission` guarding nothing. `DefaultsFit` bounds the defaults to the baseline already. The other GPU builds shrink.
- **Additions to [4.8]'s API.** `ModelCheck::DefaultSetup` (the M6 review's check, which `DefaultsFit` does not cover), `ModelCheck::ALL`, `SkipReason` as an enum in place of a reason string, `ModelReport::skip_reason` and `thread_count_jobs` (the job count [4.8] asks the report to record), `SetReport` as `check_model_set`'s return in place of `Vec<ModelReport>`, `MIN_TICKS`, `Display` on the report types, and `CheckSettings::thread_counts` panicking on a low count not below the high one. `CheckSettings` implements `Default` by hand, for 20 ticks and 1 and 7 threads. `#[must_use]` sits on `check_model` and `check_model_set` at the review's request, an exception to AGENTS.md's rule against adding it by reflex.
- **`gpu_boids` now skips `SeedSensitivity`.** [7]'s pin names `SameSeed` and `SamplingCadence`, and the review adds the third. Two seeds of a model that does not replay exactly differ whether or not the seed reaches it.
- **Seeds and sizes of the built checks.** The old tests built with the default seed at the declared sizes. The kit builds on seed 1 at the shrunk sizes, and `SeedSensitivity` compares seeds 1 and 2.
- **A GPU model on wasm32 skips every check that builds as `NativeOnly`**, where [4.8] names the three GPU checks. `Simulation` refuses a GPU model in a browser, so the determinism checks could only fail there.
- **`assert_set_conforms` prints to standard output.** The workspace warns on `print_stdout`, and the call carries an `expect` with its reason.
- **The grid search builds up to a dozen times.** It knows no `rows_per_leaf`, which is private to henad-compute, and reads `parallel_jobs` from builds. At 128 columns that is 12 small builds.
- **No self-test covers `Metadata`, `DefaultSetup`, `DefaultsFit`, `ParallelJobs`, `BaselineBuild` or `SampledSlice`.** Breaking one takes an entry built by hand, which `register_*` cannot produce, or a GPU model past the baseline.
  Correction after a later review: `register_grid_model` accepts a default outside its bounds, so `DefaultSetup` needs no hand-built entry, and `a_default_outside_its_bounds_fails_every_check_through_a_run_setup` now covers it.
- **A missing device under `HENAD_REQUIRE_GPU` has no self-test.** Setting the variable inside a test changes it for every test of the process, and `set_var` is `unsafe` on edition 2024, which the workspace denies. check.sh's run sets it for every test, and a kit run without a device would fail there.
- **`authoring/testing.md` is not written.** [8] gives it its own page with the template's test, and the template arrives in M10d. The determinism page carries the kit until then.
- **henad-models' `flume` dev-dependency has no user**, before and after M10a.
- **`cargo clippy` for wasm32 on henad-compute fails on `drop_non_drop` in `gpu/sim_thread.rs`**, which M10a does not touch. check.sh runs no wasm32 clippy, and M10b's nightly atomics clippy will meet it.
- **Next.** M10b, the facade, depends on M8, M9 and M10a, and re-exports the kit as `henad::testing`.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
