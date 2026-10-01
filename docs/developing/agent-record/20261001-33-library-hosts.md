---
date: 2026-10-01
title: "Library M4: hosts take the set"
description: The fourth milestone of #48. The app and the CLI hold a ModelSet and find models by id, the app's selection is an id instead of an index, the two missing-model messages land, model_registry is gone, and golden tests pin --list, --params and --params --json to what 0.2.0 printed.
icon: material/package-variant
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#48"
state: M3 review folded in, M4 implemented, `HENAD_REQUIRE_GPU=1 ./check.sh` and the docs build green, golden output identical to 0.2.0's
baseline_commit: 6d9d3b7
delta_state: uncommitted on `48-library`
---

# Library M4: hosts take the set

> M4 is the fourth of the eleven milestones of #48, which turns Henad into a library published on crates.io.
> The session first folded in the maintainer's review of M3, then moved both hosts onto the `ModelSet` M3 introduced.
> The app keys its selection by model id, and resolves every id through the set when it needs the entry.
> The CLI resolves its positional id through `ModelSet::lookup`, and both hosts request their device for the set's `GpuNeeds`.
> A missing model now gets one of two messages, one for an id the build lacks and one for a GPU model on a machine without compute.
> `model_registry` and its test are gone, and the GPU filter that replaced them hides a GPU model wherever the adapter cannot run compute shaders.
> Golden files recorded from the `v0.2.0` tag pin `--list`, `--params` and `--params --json` byte for byte.

## State before

`48-library` stood at 6d9d3b7, the third of M3's three commits, with a clean tree.
The app held `registry: Vec<ModelEntry>` from `model_registry`, selected by `selected_model: usize`, and recorded the loaded model, an opened run's model and a results store's model as indices into that `Vec`.
The CLI listed its models through `model_registry` and found the positional id by the first match, with "unknown model 'x' (try --list)" for both a missing id and a GPU model on a machine without a device.
Both hosts sized their device to `example_models().gpu_needs()` inside the library code, and the sweep sessions and the stat-columns thread looked their entry up again by id among the example models.
The maintainer's review of M3 listed eight items, from stale AGENTS.md lines to a constant the cursor never reached.

## What was done

### The M3 review

1. AGENTS.md calls henad-explore a sibling of henad-models over henad-compute, names the three crates beside it in the wasm typecheck, describes the device a sweep acquires as sized to the entry's `GpuNeeds`, and states decision 2.14's rule as "one nearer henad-core in the tree".
   The `entry/` passage lost its two parallel frames and its trailing "which", and the binding-count note is reflowed.
2. `developing/architecture.md` draws compute under both models and explore, the hosts under explore, and models into the hosts for `example_models()`.
   It states the rule in place of "one strict dependency direction", and says the headless device serves the CLI and any sweep handed no device.
3. The CHANGELOG has the `replays_exactly` break and the migration note for the removed factory and capacity items.
4. `ModelSet`'s field and `new`'s parameter are named `build`.
5. `an_example_entry_inserted_into_another_set_keeps_its_source` matches the type path's crate prefix and type suffix, since `type_name` promises no exact form.
6. `RunCursor::new` checks `gpu_needs()` before it builds, and refuses a GPU model with `GPU_REFUSAL`.
   The build used to fail first, with the entry's own "no device" fault.
7. The comment wording: "Origin of a model's code", `demand` and `shortfalls` docs opening with "Returns", the `Factory` summary split from its detail, `character` for `c`, a full sentence on `gpu_needs`, one sentence for `InvalidId`, and "a model entry" in `model.rs`.
8. The two first-model pages lost their trailing "which" and parallel frame.
   Record #32 carries the gpu_boids caveat in its state line, splits its two double-sentence lines, lists the four files its tree missed, and notes that 5a056ec and 571f6a0 build their libraries but not their test targets.

### The app

- `HenadApp::new` takes the `ModelSet` to offer, and `wgpu_configuration` takes the `GpuNeeds` the device request closure captures.
  `main.rs` builds `example_models()` once and passes it to both.
  The wasm `main` builds the set after the thread pool starts, as the native one does before eframe.
- `AppState` holds the set as `models`.
  `selected_model` and `loaded_model` are `Option<String>`, and `selected_entry`, `loaded_entry` and `lookup` resolve them at use.
  `OpenedRun` lost `model_index`, since its replay already names the model.
  `ResultsStore` lost `model_index`, and Resume asks `lookup` for the store's `model_id`.
- `offered_models` filters the set to what runs here: every CPU model, and a GPU model only with a compute context.
  The app opens on the first offered model, and with nothing selected when none is offered, where the Model panel reads `NO_MODEL_RUNS`.
- `lookup_message` turns a `ModelLookupError` into a sentence: "This build does not include model 'x'." or "Model 'x' needs a GPU, and this machine has none."
  Opening a run, starting or resuming a sweep, loading a spec and opening a results folder all report through it.
- `select_model` loads the defaults and clears the schedule, for the Model panel and the Sweep tab alike, where each did both by hand before.
- The sweep sessions clone the entry the set holds, and the stat-columns thread takes a clone of the entry and acquires a device sized to that entry's own needs.
- Panels that borrowed `app.registry` field by field now clone the entry first, an `Arc` increment, where a method call would borrow the whole state.

### The CLI

- `main` builds `example_models()`, acquires its device for the set's needs, and prints `--list` from the models this machine can run.
- `ModelSet::lookup` resolves the positional id.
  An id outside the set reads "this build does not include model 'x' (try --list)", and a GPU model with no device "model 'x' needs a GPU, and this machine has none".
- `docs/reference/cli.md` says which models `--list` prints and how a missing one is refused.

### henad-models and henad-explore

- `registry.rs` is gone.
  Its tests moved to `src/tests/registry.rs` and run over `example_models()`, without `registry_without_gpu_context_offers_no_gpu_models`.
- The two GPU tests that asked `model_registry` for a drivable GPU entry ask `lookup` with a device instead.
- henad-explore's tests find their entries in `example_models()`.
  The test support's `entry(id, gpu)` goes through `lookup`, and keeps its signature, so no pinned test's body changed.
- `schema_hashes_are_unchanged_since_0_2_0` now checks the four GPU models without a device too, since a schema reads only declarations.
  Its assertion is unchanged.

### Golden output

`crates/henad-cli/tests/golden.rs` runs the built binary and compares `--list`, and `--params` and `--params --json` for every example model, with files recorded from the `v0.2.0` tag.
`--list` keeps two files, and the test picks `list.txt` or `list-without-adapter.txt` by whether this machine has a compute adapter.
A GPU model's `--params` is compared only where one exists, and `HENAD_REQUIRE_GPU=1` fails the test on a machine without one.
henad-cli's package leaves out `tests/`, as henad-models' does.

The procedure is in `crates/henad-cli/tests/golden/README.md`.
The second `--list` came from the 0.2.0 binary under `sandbox-exec` with a profile that denies every IOKit connection, which leaves Metal with no adapter.
The same sandbox ran the current test binary, which then compared the no-adapter file and the six CPU models, and passed.
Under `HENAD_REQUIRE_GPU=1` in that sandbox, both tests failed as they should.

### The live app

A release build with `--features inspection`, driven through the egui MCP server on the Metal adapter:

1. The Model panel opened on SIR Epidemic and offered all ten models.
2. Picking SIR Epidemic (GPU) and pressing Build built it on the app's device, and a step reached tick 1.
3. The Sweep tab listed 12 outputs per run, the figure a failed stat-columns build would also give, so it says nothing about that build.
   Varying Infection Rate over 0.2 and 0.4 planned two runs, and Start ran them on the sweep's own device: "Sweep finished: 2 runs, 0 failed, in 415 ms".
4. Show results drew both configs, and Open at end on run 0 rebuilt it by id and stepped to tick 1000, where Susceptible read 1, as its row recorded.
5. Picking SIR Epidemic and pressing Build built the CPU model, with the Pacing panel switched to CPU controls.

The app's saved state was backed up before the run and restored after it.

### Docs

- `reference/models.md` is titled "Example models", and says the ten come from `example_models()`.
  The home page's card follows.
- The five first-model pages say a model set holds each id once, in place of "unique across the registry", and the two that said the app finds models "in the registry" say it picks from the set it was handed.
- The GPU Game of Life page says a GPU model asked for by id without an adapter is refused with a message saying it needs a GPU.

### Edited tree

```text
.
├── AGENTS.md                           ~ M3 review items, the app's set and ids, the CLI's lookup, golden tests,
│                                         the registry tests' new home
├── CHANGELOG.md                        ~ Unreleased: the M3 review lines, M4's Added, Changed and Removed
├── zensical.toml                       ~ nav entry #33
├── crates/
│   ├── henad-core/src/
│   │   ├── model.rs                    ~ "a model entry"
│   │   └── provenance.rs               ~ "Origin of a model's code"
│   ├── henad-compute/src/entry/
│   │   ├── mod.rs                      ~ doc wording
│   │   └── set.rs                      ~ `build` field, doc wording, InvalidId message
│   ├── henad-models/src/
│   │   ├── registry.rs                 - model_registry
│   │   ├── lib.rs                      ~ - pub mod registry
│   │   ├── tests/mod.rs                ~ mod registry
│   │   ├── tests/registry.rs           + the registry tests, over example_models()
│   │   └── gpu_{game_of_life,sir}/mod.rs   ~ lookup with a device
│   ├── henad-explore/src/
│   │   ├── cursor.rs                   ~ refuses a GPU entry before it builds
│   │   ├── schema.rs                   ~ the schema-hash test covers GPU models without a device
│   │   ├── exec/mod.rs, probe.rs, spec_file.rs   ~ tests on example_models()
│   │   └── tests/{support,determinism,gpu,search,tracks}.rs   ~ example_models(), entry through lookup
│   ├── henad-cli/
│   │   ├── Cargo.toml                  ~ exclude /tests/
│   │   ├── src/main.rs                 ~ the set, offered(), lookup and the two messages
│   │   ├── src/explore.rs              ~ tests on example_models()
│   │   └── tests/
│   │       ├── golden.rs               + golden tests
│   │       └── golden/                 + README.md (procedure), list.txt, list-without-adapter.txt,
│   │                                     params/<id>.txt and params-json/<id>.json for ten models
│   └── henad-app/src/
│       ├── lib.rs, main.rs, init.rs    ~ HenadApp::new(cc, models), wgpu_configuration(needs)
│       ├── state.rs                    ~ models, ids, offered_models, lookup, select_model, lookup_message
│       ├── ui/model.rs                 ~ the offered list, NO_MODEL_RUNS
│       ├── ui/{params,fault,pacing}.rs, ui/export/{mod,metadata}.rs   ~ selected_entry, loaded_entry
│       ├── ui/sweep/{mod,session}.rs   ~ cloned entries, lookup, select_model
│       ├── ui/results/{mod,store}.rs   ~ no model_index, results resolved through lookup
│       └── ui/{results,sweep}/*.rs     ~ tests on example_models()
└── docs/
    ├── developing/architecture.md      ~ the crate graph and the rule
    ├── developing/agent-record/20261001-32-library-entries.md   ~ review item 8
    ├── developing/agent-record/20261001-33-library-hosts.md     +
    ├── guide/first-model/{game-of-life,ants,virus-network,gpu-game-of-life,gpu-ants}.md   ~
    ├── index.md                        ~ "Example models"
    └── reference/{models,cli}.md       ~
```

### Hot paths

M4 has no timing gate, and no tick changed.
The panels resolve the selected entry by id once per frame, a linear search over ten entries, where they indexed a `Vec` before.
The Sweep tab and a few panels clone the entry they read, one `Arc` increment per frame.
Neither sits on a path a sim thread or a sweep steps through.

## State after

M4 is implemented and uncommitted on `48-library`, on top of 6d9d3b7.
The workspace version stays 0.2.0.

- `HENAD_REQUIRE_GPU=1 ./check.sh` passes: 975 tests, none failed, with the packaging, cargo-deny and docs steps and the web build.
  That is M3's 973, plus the two golden tests and `an_app_without_compute_hides_gpu_models_and_names_each_missing_one`, less `registry_without_gpu_context_offers_no_gpu_models`.
  Two earlier runs failed on `cargo fmt` and on clippy's `print_stderr` in the golden test's skip note, both fixed.
- `uv run --locked zensical build` passes with its path checks.
- The golden tests pass with the Metal adapter, and in a sandbox without it.
- The live app picked, built, swept and replayed a GPU model, and built a CPU one.

## Issues found & future directions

- **The filter lives in each host.** `offered` in the CLI and `offered` in `state.rs` are the same one-line filter. The design keeps `retain` and `remove` off `ModelSet` until a caller needs one, and M8 and M9 move both hosts behind `CliOptions` and `AppOptions`, where a shared helper can be weighed with two callers in view.
- **`HenadApp::new` is an interim shape.** M9's `AppOptions` carries the set, the product name and the host's build, and `HenadApp` becomes crate-private then.
- **A results table's second row sits under its scroll bar.** With a short table area, egui's horizontal scroll bar overlays the last row, and a click there scrolls instead of selecting. Noticed while driving the app, and left alone.
- **No live check without compute.** The app's no-compute path is covered by `an_app_without_compute_hides_gpu_models_and_names_each_missing_one`, which builds the state with no compute context. The window itself was never run on such an adapter, and M9 adds `a_gpu_only_set_without_compute_opens_with_nothing_selected`.
- **Next.** M5 (shared WGSL and henad-build) depends on M1 alone, M6 (the programmatic API) on M3.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
