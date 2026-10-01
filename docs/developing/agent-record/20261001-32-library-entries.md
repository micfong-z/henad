---
date: 2026-10-01
title: "Library M3: the entry layer"
description: The third milestone of #48. ModelEntry moves into henad-compute and holds no device, a ModelSet holds the models a host offers with each id once, henad-explore stops depending on the example models, and a sweep handed no device builds the entry it was handed.
icon: material/package-variant
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#48"
state: M3 implemented, `./check.sh` with HENAD_REQUIRE_GPU=1 and the docs build green, `--export-stats` identical to M2 for every model
baseline_commit: 21b1f6e
delta_state: uncommitted on `48-library`, in three handovers
---

# Library M3: the entry layer

> M3 is the third of the eleven milestones of #48, which turns Henad into a library published on crates.io.
> The type-erasure layer that henad-models held as `registry.rs` now lives in henad-compute as `henad_compute::entry`.
> A `ModelEntry` keeps its declarations behind accessors and an `Arc`, and takes its device when it builds, never when it registers.
> A new `ModelSet` holds the models a host offers, refuses a second model with an id it holds, and records the build of the crate that inserted each entry.
> henad-models now exports `example_models()`, and henad-explore no longer depends on henad-models outside its tests.
> A sweep handed no device for a GPU model builds the entry it was handed on a device of its own, where before it looked the model up again by id among the example models.
> The vestigial `Model` trait and both GPU descriptor types are gone, and `gpu_boids` declares that it does not replay exactly.
> `--export-stats` at seed 1 writes the bytes the M2 binary writes for every model, `gpu_boids` on its engine-owned column.

## State before

`48-library` stood at 21b1f6e, M2, with a clean tree.
`henad_models::registry` held `ModelEntry` with ten public fields, a boxed factory `Fn(&[ParamValue], Option<u64>)` and a boxed capacity closure.
`register_gpu_grid_model` and `register_gpu_agent_model` took a `GpuContext` and captured it in both closures.
`model_registry(Option<GpuContext>)` returned a `Vec<ModelEntry>`, the GPU half only with a device, and every host found a model by the first matching id.
`gpu_storage_bindings_needed()` took the maximum over a hand-written list of the four GPU model types.
henad-explore depended on henad-models: `acquire_headless` read that list, and `BoundModel` rebuilt a GPU entry from `model_registry` by id on the device it acquired.
henad-core held the `Model` trait, implemented only by `GpuGridModelDescriptor` and `GpuAgentModelDescriptor`, which nothing called.

## What was done

### henad-core

- `provenance.rs` holds `BuildInfo`, `build_info!` and `ModelSource`, all with private fields.
  `build_info!` expands to `BuildInfo::__from_env`, a hidden `const fn` over Cargo's package name and version and four `HENAD_BUILD_*` variables that nothing sets yet.
  It reads a dirty flag as `true` or `false` and a source hash as 16 lowercase hexadecimal digits, and anything else as unknown, so M7's build script writes those forms.
  `ModelSource` holds a type path and an optional build, and reads the build's package, version, commit, dirty flag and hash through accessors that return empty text or `None` until a set records one.
- `ModelMetadata` gains `replays_exactly`, and it and `Structure` derive `Clone`.
- `GpuGridModel` and `GpuAgentModel` gain `const REPLAYS_EXACTLY: bool = true`.
- The `Model` trait is gone. `model.rs` holds `SimState` alone.

### henad-compute

- `entry/mod.rs` holds `ModelEntry`, `ModelState`, the hidden `Factory` trait and the five `register_*` functions.
  An entry is one `Arc<EntryParts>`, and `Clone` is an `Arc` increment.
  The accessors carry the old field names: `id()`, `name()`, `description()`, `param_descriptors()`, `stat_descriptors()`, `action_descriptors()`, `topology_hint()` and `metadata()`, with `gpu_needs()`, `source()`, `param_index()` and `action_index()` beside them.
  `build(params, seed, gpu)` replaces `(entry.create)(params, seed)`, `demand` and `shortfalls` take the device's limits, and the hidden `wrap_factory` takes over from struct-update syntax in the four test harnesses that inject a fault.
  `Capacity` is `pub(crate)`.
  A GPU entry built with no device returns `Fault::refused` during `BUILDING`.
  Every `register_*` records `std::any::type_name::<M>()` as its source's type path, and a GPU one fills `gpu_needs` from the engine's `max_storage_bindings()` and copies `REPLAYS_EXACTLY` into the metadata.
  `register_gpu_*` map every parameter to on-reload and set the topology hint, the two things the removed descriptor types did.
- `entry/set.rs` holds `ModelSet`, `ModelSetError` and `ModelLookupError`.
  `insert` checks the id grammar, then the set's ids, and records the set's `BuildInfo` on an entry whose source has none.
  `extend` checks every id before it adds any, and keeps each entry's source.
  `lookup` returns `NotInSet` or `NeedsGpu`, and `gpu_needs()` merges the needs of every GPU entry.
  `DuplicateId` boxes its two sources, since clippy's `result_large_err` refused a 248-byte error.
- `gpu/limits.rs` holds `GpuNeeds`, re-exported as `henad_compute::gpu::GpuNeeds`, and `raise` takes it in place of a count.
- `GpuContext` gains an optional `Arc<RuntimeInfo>`, set by `with_runtime_info` and read by `runtime_info()`.
  `RuntimeInfo` and `HostInfo` derive `Clone`, for the one app test that hands `AppState::new` the information the headless device carries.
- `grid_init_rng` (in `cpu/field/ca.rs`) and `agent_init_rng` (in `cpu/agent_engine.rs`) hold the default-seed rule, `seed.map_or(INIT_SEED, mix_seed)`.
  `CaField::with_seed` and both `AgentModelState` constructors call them.
- `GpuGridModelDescriptor` and `GpuAgentModelDescriptor` are gone.

### henad-models

- `lib.rs` holds `example_models()`, which registers the ten models into one `ModelSet` built from henad-models' own `build_info!()`.
  The `cpu_entries` and `gpu_entries` snippet regions moved into it, and the GPU lines lost `entries.push(..(&ctx))`.
- `registry.rs` holds `model_registry` alone, a wrapper over `example_models()` that drops every GPU entry when handed `None`, and the registry tests.
  `gpu_storage_bindings_needed` and its guard test `the_declared_binding_need_matches_the_registry` are gone.
- `gpu_sir` and `gpu_game_of_life` seed through `grid_init_rng`, and `gpu_boids` and `gpu_ants` through `agent_init_rng`.
  `gpu_sir`'s second stream, `mix_seed(s ^ RNG_INIT_SEED)` with `RNG_INIT_SEED` for the default, stays as it was.
  The tutorial copies under `src/tests/tutorial/` keep their hand-written rule, since five guide pages include them whole and M10c moves them.
- `gpu_boids` declares `REPLAYS_EXACTLY = false`.

### henad-explore

- henad-models moved from `[dependencies]` to `[dev-dependencies]`, by path alone, so the published package carries no requirement on it.
  `cargo tree -p henad-explore -e normal -i henad-models` prints nothing.
- `acquire_headless(needs)` raises the limits to `needs` and returns the context with its `RuntimeInfo` attached.
- `BoundModel::new` acquires a device for `entry.gpu_needs()` and keeps the entry it was handed.
  `OwnDeviceError` lost its `Unregistered` variant and is now a newtype over `DeviceError`.
- `ProbeReport::build` refuses a GPU model with no device as `ProbeError::NoDevice` before it builds, as it did before through the missing context.
  `check_capacity` tells a GPU entry by `gpu_needs()`.
- The GPU tracks pass the executor's device to `build` and its limits to `demand`, and the probe passes the probe's device.
  Before, both read the limits of the device the entry had captured, which was the same device.
- The GPU cadence test lists the GPU entries whose `replays_exactly` holds, in place of its `EXEMPT` list naming `gpu_boids`.

### Hosts

henad-cli and henad-app import the entry types from `henad_compute::entry`, read every entry through its accessors, and request their devices for `example_models().gpu_needs()`.
The CLI passes its device to `build` and `new_gpu_state`, and keeps its message for a GPU model on a CPU-only path by checking `gpu_needs()` before it builds.
The app passes its compute device to `build` and the granted limits of its `RuntimeInfo` to `demand`.
Both still list their models through `model_registry`, which M4 replaces.

### Tests

New, all passing on the Metal adapter:

| Test | Crate | Checks |
|---|---|---|
| `a_model_set_refuses_a_duplicate_id` | henad-compute | A second `still` is refused with both type paths, and a refused `extend` adds nothing |
| `a_model_set_refuses_an_id_outside_the_grammar` | henad-compute | An uppercase letter, a hyphen, a leading digit and an empty id are refused, and `calm_2` joins |
| `a_set_keeps_the_source_of_an_entry_from_another_set` | henad-compute | An entry cloned from one set into another keeps the first set's build, through `insert` and `extend` |
| `a_cpu_model_needs_no_device` | henad-compute | A CPU-only set needs nothing, and `lookup` finds its model with no device |
| `an_example_entry_inserted_into_another_set_keeps_its_source` | henad-models | SIR inserted into a host's set still names henad-models and `henad_models::sir::SirGridModel` |
| `a_gpu_entry_refuses_to_build_without_a_device` | henad-models | Each GPU example returns a refused `Fault` during `BUILDING`, and `lookup` with no device returns `NeedsGpu` |
| `every_example_model_joins_the_set` | henad-models | All ten join, each with henad-models as its source's package |
| `every_gpu_entry_needs_the_bindings_its_widest_pass_binds` | henad-models | Each GPU entry's `GpuNeeds` equals its widest pass, and the set's needs equal the widest of all |
| `a_gpu_model_outside_the_example_set_sweeps_on_its_own_device` | henad-explore | GPU SIR under the id `outside_gpu_sir`, absent from the example set, sweeps with no device handed and completes both runs |
| `an_unstamped_build_reads_as_unknown` and three more | henad-core | `BuildInfo`'s parsing and `ModelSource`'s accessors |

The registry tests now select GPU entries by `metadata().backend` in place of an id prefix, and build every entry on the test's own device.
`a_model_too_large_for_the_device_is_reported` and `every_gpu_entry_reports_its_capacity` no longer need a device, since demand reads only limits.
`registry_without_gpu_context_offers_no_gpu_models` stays with `model_registry` until M4.

### Handovers

The work is handed over in three parts, so the call-shape rewrites of the tests [4.9.4] pins each sit in a commit of their own and change no assertion.

1. The milestone itself, with the docs, AGENTS.md, the CHANGELOG and this record.
2. `a_pipelined_gpu_sample_matches_a_blocking_one`, whose helper `blocking_rows` calls `model.build(.., Some(ctx))` and `model.id()`.
3. `ants_results_do_not_depend_on_lane_width` (`ants.param_descriptors()`), `a_replay_of_a_planned_run_matches_its_sweep_row` (`sir.build(.., None)`) and `a_gpu_schedule_matches_export_stats` (`gpu_sir.build(.., Some(&ctx))`).

Every other pinned test kept its body. Their helpers, such as `instrument` and the test support's `entry`, changed in the first part.
The test targets of henad-explore compile only once all three parts are in, since the API change and the rewrites cannot both compile against one API.

### Docs

- `authoring/registering.md` names `example_models()` and `ModelSet`, gives the GPU functions without a context, describes `GpuNeeds` and `REPLAYS_EXACTLY`, and drops the claim that a function walks every model for the binding count.
- The five first-model pages register their model in `crates/henad-models/src/lib.rs`, and the two pages that include `cpu_entries` or `gpu_entries` include them from there.
  The GPU Game of Life page now says the entry needs no device and the hosts hide GPU models without one.
- `developing/gpu-backend.md` describes the binding count as the set's merged `GpuNeeds`.
- `developing/cpu-backend.md` and `authoring/index.md` no longer mention the `Model` trait.

### Edited tree

```text
.
├── AGENTS.md                           ~ the crate graph and decision 2.14's rule, entry/ and provenance.rs,
│                                         example_models(), BoundModel, GpuNeeds, the runner interface
├── CHANGELOG.md                        ~ Unreleased: Added, Changed, Removed
├── crates/
│   ├── henad-core/src/
│   │   ├── provenance.rs               + BuildInfo, build_info!, ModelSource
│   │   ├── lib.rs                      ~ pub mod provenance
│   │   ├── metadata.rs                 ~ replays_exactly, Clone
│   │   ├── model.rs                    - the Model trait
│   │   └── authoring/model/gpu_{grid,agent}_model.rs   ~ REPLAYS_EXACTLY
│   ├── henad-compute/src/
│   │   ├── entry/
│   │   │   ├── mod.rs                  + ModelEntry, ModelState, Factory, register_*
│   │   │   └── set.rs                  + ModelSet, ModelSetError, ModelLookupError
│   │   ├── lib.rs                      ~ pub mod entry
│   │   ├── gpu/limits.rs               ~ GpuNeeds, raise takes it
│   │   ├── gpu/mod.rs                  ~ runtime info on GpuContext, - descriptor re-exports
│   │   ├── gpu/{grid,agent}_engine.rs  - GpuGridModelDescriptor, GpuAgentModelDescriptor
│   │   ├── cpu/field/ca.rs             ~ grid_init_rng
│   │   ├── cpu/agent_engine.rs         ~ agent_init_rng
│   │   └── runtime_info.rs             ~ Clone
│   ├── henad-models/src/
│   │   ├── lib.rs                      ~ example_models(), with the snippet regions
│   │   ├── registry.rs                 ~ model_registry over example_models(), the registry tests
│   │   └── gpu_{sir,game_of_life,boids,ants}/mod.rs   ~ init rng helpers, gpu_boids' REPLAYS_EXACTLY
│   ├── henad-explore/
│   │   ├── Cargo.toml                  ~ henad-models as a path dev-dependency
│   │   └── src/
│   │       ├── device.rs               ~ acquire_headless(needs) -> GpuContext
│   │       ├── handle.rs               ~ BoundModel builds the entry it was handed
│   │       ├── probe.rs, cursor.rs, exec/*, sweep.rs, ...   ~ accessors, build, demand with limits
│   │       └── tests/                  ~ wrap_factory harnesses, the outside-the-set GPU test
│   ├── henad-cli/src/{main,explore,json_report}.rs        ~ accessors, device needs, build
│   └── henad-app/src/**               ~ accessors, device needs, build
├── docs/
│   ├── authoring/{registering,index}.md                    ~
│   ├── developing/{cpu-backend,gpu-backend}.md             ~
│   ├── guide/first-model/{game-of-life,ants,virus-network,gpu-game-of-life,gpu-ants}.md   ~
│   └── developing/agent-record/20261001-32-library-entries.md   +
└── zensical.toml                       ~ nav entry #32
```

### Hot paths

M3 has no timing gate, and no hot path changed.
The `register_*` functions moved from henad-models to henad-compute, but they are generic and every closure in them is generic over the model, so each engine and its kernels still monomorphise in the crate that calls `register_*`: henad-models for the example models.
henad-compute instantiates no engine outside its own tests.
A tick still makes one virtual call, `SimState::step` or a `GpuSimState` encode, on the state the factory boxed.
The factory moved from a `Box<dyn Factory>` to an `Arc<dyn Factory>`, called once per build.
`ModelEntry::demand` now reads `device.limits()` from the caller's device on each GPU track admission, where the captured closure read it from its own.
The CLI's benchmark builds its probe state with the device, outside the timed loop, and `new_cpu_state` checks `gpu_needs()` before it builds.
M6's gate should still compare a GPU benchmark run, since `bench_gpu` now passes its context through `new_gpu_state`.

### Equivalence

Both binaries were release builds of `henad-cli` on rustc 1.97.1: M2 from a `git archive` of 21b1f6e with a target directory of its own, and M3 from the working tree.
`henad-cli <id> --export-stats <file> --seed 1 --steps 300` wrote identical bytes from both for sir, boids, game_of_life, ants, virus_network, team_assembly, gpu_game_of_life, gpu_sir and gpu_ants.
For gpu_boids the header, the 302 lines and the tick column match, and so does the row at tick 0.
Two runs of the M2 binary on gpu_boids already differ from each other past tick 0, as its new `REPLAYS_EXACTLY = false` says.

## State after

M3 is implemented and uncommitted on `48-library`, on top of 21b1f6e.
The workspace version stays 0.2.0.

- `HENAD_REQUIRE_GPU=1 ./check.sh` passes: 973 tests, none failed, twelve more than M2, with the packaging, cargo-deny and docs steps and the web build.
  The twelve are the thirteen new tests above, less `the_declared_binding_need_matches_the_registry`.
  Its first run failed at the web build, on a wasm32-only read of `entry.metadata` in the Sweep tab that the native checks never compile.
- `uv run --locked zensical build` passes with its path checks.
- `cargo tree -p henad-explore -e normal -i henad-models` prints nothing.
- `--export-stats` matches the M2 binary for every model, as above.

## Issues found & future directions

- **`model_registry` still stands.** The app and the CLI list their models through it and index a `Vec`, and the stat-columns thread still finds its entry again by id. M4 hands both hosts the set, keys the app by id, and removes the wrapper with `registry_without_gpu_context_offers_no_gpu_models`.
- **`ModelEntry::schema()` and `setup()` are not here.** `schema()` replaces henad-explore's `model_schema` in M6, and `setup()` returns the `RunSetup` M6 adds.
- **The registry tests sit in `registry.rs`.** They test `example_models()` now, and move with the file when M4 deletes the wrapper, or onto the kit in M10a.
- **The error boxes its sources.** `ModelSetError::DuplicateId` holds `Box<ModelSource>` twice where the design wrote `ModelSource`, to keep the error small.
- **`RuntimeInfo` is `Clone`.** The app still takes a `RuntimeInfo` in `AppState::new`, and one session test clones the one the headless device carries. M9's `AppOptions` can take the context alone.
- **The tutorial copies keep their seed rule by hand.** `gpu_life.rs` and `gpu_foraging.rs` under `src/tests/tutorial/` write `seed.map_or(GRID_INIT_SEED, mix_seed)` and its agent twin, since guide pages include them whole. M10c can switch them to the helpers, which the authoring prelude re-exports from M10b.
- **Next.** M4, hosts that take the set, which depends on M3 alone, and M5, which depends on M1 alone.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
