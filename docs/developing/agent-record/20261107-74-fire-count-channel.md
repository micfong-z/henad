---
date: 2026-11-07
title: "Fire-count channel"
description: "The engine half of row X1 of the #37 redesign. Both GPU engines count the fires of a model's probes on the last step of each batch through a second uniform block per step pass, and publish them in Snapshot.fire_counts with the tick they counted. henad-graph compiles probes beside a graph, outside its text and code identity, and its host evaluator counts the same probes."
icon: material/counter
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s8-fire-counts, two unsigned [draft] commits and this record after 15d8717; the full check with HENAD_REQUIRE_GPU=1 passes every step but cargo deny and the workspace tests, which fail as the baseline does, on its three advisories and its one budget test"
baseline_commit: f4088d7
delta_state: "committed on `37-node-editor` by fast-forward: `feat: fire-count channel in the GPU engines [draft]`, `fix: s8 review findings [draft]` and this record, unsigned, for the maintainer to reword and sign"
---

# Fire-count channel

> S8 of the #37 redesign plan builds the engine half of row X1, which the maintainer's answer 9a needs before fire counts are on by default (decision D48).
> `GpuGridSpec` and `GpuAgentSpec` gain a `fire_counters` range of their counters, empty by default.
> Each engine clears that range before the last step of each `encode_steps` call and runs that step through a second uniform buffer and bind group pair per step pass, whose block holds the counting flag set, so no storage binding is added for the flag.
> The grid engine's stats clear now leaves the range alone, and the counts read back with the reduce tallies in one readback.
> `GpuSimState::fire_counts` returns them with the tick the counted step reached, and the GPU runner publishes them in the new `Snapshot.fire_counts`, empty on the CPU path.
> In henad-graph, `GridProgram::compile_with_probes` and `AgentProgram::compile_with_probes` take a `Probes` list of `bool` node outputs beside the file.
> A probed step tallies each probe in workgroup memory under `params.henad_count_fires != 0u` and flushes one global atomic per probe per workgroup, the shape S3 gave counters.
> The probes change no byte of the file and leave its code identity alone, and `HostEvaluator::run_pass_counting` counts the same probes on the host.
> On the Game of Life starter written with two transition rows, the device's counts equal the host's every tick for 50 ticks and after a call of 64 steps.

## State before

`37-node-editor` at `98be226` held the gate (S0), the editor model enum (S6, record #71) and the workgroup counters (S3, record #72).

- A GPU engine had no way to count anything on one step alone. Its `counters` were the reduce tallies (grid) or the fault word, the tick, the document's counters and the reduce tallies (agents).
- The grid engine cleared the whole readback before every reduce pass (`self.readback.encode_clear(encoder)`).
- `Snapshot` carried the tick, the stats and the view, and nothing per line of a model.
- A grid step bound no `counters`, and `counter.add` on a grid was refused by the checker.

## What was done

### henad-compute

- **Spec fields.** `GpuGridSpec::fire_counters` and `GpuAgentSpec::fire_counters`, a `Range<usize>` of `counters`, empty from `new` and in `from_model`. Both `assert_contracts` refuse a range that ends past `counters`, and both `Debug` impls print it.
- **The counting flag.** `GridPassCtx` gains `counting: bool`, and `AgentRun` gains `counting` with `with_counting`. `AgentRun::new` leaves it false. henad-core's `PassCtx` is untouched (see Deviations).
- **Counting blocks.** With a non-empty range, every step pass that binds `params` builds a second uniform buffer, written once at build from `pass_params_bytes` (grid) or `run_params_bytes` (agents) with the flag set, and a second pair of bind groups over it. `BuiltPass` and `EncodedPass` hold them in `counting`, and `encode` takes a `counting` argument.
- **The counted step.** `encode_steps` clears the range with `encode_clear_range` before the last step of the call and binds the counting blocks for that step alone. The engine records the tick that step reached (`counted_tick`).
- **The grid's stats clear** clears every word but the range, in two `encode_clear_range` calls, and a spec with no range keeps the whole clear. The agent engine already cleared only `cleared_counters`.
- **The tick of the counts.** `CounterReadback` gains a crate-private tag on each copy (`encode_copy_tagged`), moved to the map when it starts and to `values_tag` when it lands. Each engine tags its stats copy with `counted_tick`. A copy skipped while a map is in flight keeps the earlier tag, so the tick always matches the values.
- **Publishing.** `FireCounts { tick, counts }` (`snapshot.rs`, `#[non_exhaustive]`, `new`, `is_empty`) and `GpuSimState::fire_counts`, provided with an empty default. Both engines and both adapters implement it. `GpuSimThread::publish_snapshot` fills `Snapshot.fire_counts`, and the CPU runner leaves it empty. The facade re-exports `FireCounts` in `henad::runner` beside `Snapshot`.

### henad-graph

- **`Probes`** (`wgsl/probes.rs`, re-exported as `henad_graph::wgsl::Probes`): a list of `PortRef`s, each a `bool` output of a node in a tick pass. `probes::resolve` finds each in the evaluator's programs and refuses one that names no output of a tick pass, an output of a `math.expr` or `fn.call` (both expanded into copies by normalisation), a node with a `region`, or an output of another type. A probe of a pass that `enabled = false` turns off keeps its word and builds no tally.
- **Compilation.** `GridProgram::compile_with_probes` and `AgentProgram::compile_with_probes`, with `compile` passing an empty list. `compile_shaders`, `compile_parts` and `compile_agent_shaders` take the list.
- **Emission.** After the element's instructions in a step, each probe of the pass writes `if (params.henad_count_fires != 0u && <output>) { atomicAdd(&henad_fire_tally[slot], 1u); }`. `main` zeroes the slots and barriers at its top, and at its end barriers and adds each non-zero slot to `counters[fire_base + i]` from invocation `slot % 256`, both inside `if (params.henad_count_fires != 0u)` at the top level. The flag is a uniform, so the branch is uniform, and a step that does not count pays one test. A grid step with probes binds `counters` as a runtime-sized array and takes `local_invocation_index`.
- **Uniform member.** `henad_count_fires`, `FieldSource::CountFires`, from `Dispatch::counting`, which `GraphHooks::block_bytes` fills from `GridPassCtx::counting` and `AgentGraphHooks::block_bytes` from `AgentRun::counting`.
- **Counters layout.** On a grid: the reduce tallies (at least 1), then one word per probe, and the spec's `counters` grows by the probes. Over agents: fault, tick, the document's counters, the leaf's tallies, then the probes, in `counter_words`, which every shader's fixed-size `counters` declaration takes. A graph with no probes compiles to the same shaders as before.
- **Workgroup bytes.** Four per probe slot. `GeneratedShader` now keeps the emitted `workgroup_bytes` for grid shaders too, and `pass_spec` copies it, which equals the reduce's old formula.
- **Host evaluator.** `HostEvaluator::run_pass_counting(pass, lanes, params, context, stream, probes, counts)` runs a pass as `run_pass` does and adds 1 to a probe's count for each element where its output holds, through `run::run_counting`. One `counts` runs through every tick pass of a step.

### Tests

| Test | File | What it holds |
|---|---|---|
| `a_counted_step_reads_the_last_steps_count_in_calls_of_1_3_and_64` | `henad-compute/src/gpu/tests/fire_counts.rs` | A grid spec and an agents spec whose step adds each element's value to the fire word under the flag: after calls of 1, 3 and 64 steps, the counts are the last step's alone with the tick it reached, and the stats read the same tick |
| `a_grid_reads_its_tallies_and_its_fire_range_in_one_readback` | same | The tally and the counts from one readback, and two stats reads with no step between leave the counts and their tick as the counted step left them |
| `a_spec_with_no_fire_range_has_empty_counts` | same | A spec with an empty range counts nothing |
| `a_probe_on_each_transition_row_counts_as_the_host_does` | `henad-graph/src/tests/probes.rs` | The Game of Life starter with its step as two `cat.transition` rows, probes on `fired.birth`, `fired.death` and the test of 3 neighbours, 61 by 37: the device's counts equal the host evaluator's every tick for 50 ticks and after a call of 64 steps, both rows fire on every tick, and the cells end equal |
| `a_probe_over_agents_counts_as_the_host_does` | same | 1100 agents, a probe on "value below 600" with the value the index plus the tick: device equal to host every tick for 20 ticks and after 64 steps, and the host's counts equal `600 - tick` |
| `probes_leave_the_code_identity_and_text_alone` | same | The canonical text and the code hash unchanged, no probes compile to the shaders of `compile`, and probes change the step shaders alone |
| `a_probe_on_no_bool_output_of_a_tick_pass_is_refused`, `a_probe_inside_a_region_is_refused`, `a_probe_of_a_pass_that_never_runs_builds_no_tally` | same | The refusals, and a probe of a disabled pass |
| `probes_add_at_most_one_storage_binding_and_no_starter_passes_eight` | `henad-models/src/tests/graph_probes.rs` | Every starter probed on each top-level `bool` output of its tick passes binds at most one more storage buffer and at most 8 |
| `a_probed_starter_steps_as_the_unprobed_one` | same | The SIR starter with its six probes equals the unprobed starter in cells and stats every tick for 40 ticks and after 64 steps |

Every graph parity gate and `graph_*_runs` test passes unchanged, apart from the baseline's budget test.

### Storage bindings per starter

The most storage buffers any pass binds, from `max_storage_bindings()` of each spec, with no probe, with one, and with a probe on every top-level `bool` output of the tick passes.

| Starter | No probe | One probe | Every probe | Probes |
|---|---|---|---|---|
| Game of Life (starter, `grid.life_like`) | 2 | none to place | 2 | 0 |
| Game of Life with two transition rows (the test's form) | 2 | 3 | 3 | 3 |
| Vote | 2 | 3 | 3 | 1 |
| SIR | 4 | 5 | 5 | 6 |
| Boids | 7 | none to place | 7 | 0 |
| Ants | 7 | 7 | 7 | 13 |
| Random walk | 3 | none to place | 3 | 0 |

A grid step gains `counters`, one binding, and an agents step binds it already.
No starter passes the WebGPU default of 8.
The plan expected Game of Life to go from 4 to 5, after recon's count of a tick buffer pair, but neither the starter nor round 1's lowered Game of Life reads the tick, so the step binds 2 and goes to 3.
Boids' only `bool` outputs in its tick pass sit inside its radius fold, which a probe refuses.

### Docs and topic files

- `agents/henad-compute.md` (27,434 characters): `fire_counters`, the counting blocks, the counted step, the stats clear, the copy tag, `GpuSimState::fire_counts`, `Snapshot.fire_counts` and the tests.
- `agents/henad-graph-wgsl.md` (13,272 characters): a paragraph "Probes" with the resolution, the emission, the layout, the host count and the tests.
- `agents/sim-runner.md` (8,517 characters): the counted step follows the runner's chunks, so a snapshot reads the step before it.
- `agents/gpu-traps.md`: no change. No trap appeared.
- `CHANGELOG.md`: one line under Added, one breaking line under Changed for `Snapshot.fire_counts` and the two spec fields, and the `Snapshot` literal under Migrating to 0.4.

### Edited tree

```
CHANGELOG.md                                   fire-count channel, Snapshot.fire_counts breaking, migration
agents/
├── henad-compute.md                           fire_counters, counting blocks, counted step, fire_counts
├── henad-graph-wgsl.md                        Probes
└── sim-runner.md                              the counted step follows the runner's chunks
crates/
├── henad/src/lib.rs                           FireCounts in henad::runner
├── henad-compute/src/
│   ├── cpu/sim_thread.rs                      an empty fire_counts in the CPU snapshot
│   ├── gpu/
│   │   ├── agent_engine.rs                    counting binds, the counted step, the copy tag, fire_counts
│   │   ├── agent_spec.rs                      fire_counters, AgentRun::counting and with_counting
│   │   ├── grid_engine.rs                     SideBinds, counting binds, the counted step, the stats clear, fire_counts
│   │   ├── grid_spec.rs                       fire_counters, GridPassCtx::counting
│   │   ├── primitives/readback.rs             encode_copy_tagged, values_tag
│   │   ├── sim_thread.rs                      GpuSimState::fire_counts, Snapshot.fire_counts
│   │   └── tests/
│   │       ├── fire_counts.rs                 new: the counted step of both engines
│   │       ├── mod.rs                         the module
│   │       └── runtime_grid.rs                counting: false in a context literal
│   └── snapshot.rs                            FireCounts, Snapshot.fire_counts
├── henad-graph/src/
│   ├── host/
│   │   ├── mod.rs                             run_pass_counting
│   │   └── run.rs                             run_counting
│   ├── tests/
│   │   ├── gpu_cases.rs, gpu_repeat.rs        the new arguments
│   │   ├── mod.rs                             the module
│   │   └── probes.rs                          new: device against host, identity, refusals
│   └── wgsl/
│       ├── agent_hooks.rs, hooks.rs           the counting flag into the block
│       ├── agents.rs                          compile_with_probes, the fire range after the tallies
│       ├── emit.rs                            probe tallies, the zeroing and the flush, the grid step's counters
│       ├── mod.rs                             compile_with_probes, the fire range, workgroup bytes of every shader
│       ├── probes.rs                          new: Probes, resolve
│       └── uniform.rs                         FieldSource::CountFires, Dispatch::counting
└── henad-models/src/tests/
    ├── graph_probes.rs                        new: storage bindings, a probed starter steps as the unprobed one
    └── mod.rs                                 the module
```

### Deviations from the plan

- **The agent flag sits on `AgentRun`, not on `PassCtx`.** Recon proposed `counting` on `GridPassCtx` and the agent `PassCtx`. `PassCtx` lives in henad-core, is built by struct literal and is not `#[non_exhaustive]`, so a field there would break every hand-written `GpuAgentModel`'s callers and reach outside the milestone's two crates. `AgentRun`, the run values `run_params_bytes` already takes, is `#[non_exhaustive]` and in henad-compute, and the graph hooks read the flag from it.
- **The Game of Life test rewrites the starter's step.** The acceptance asks for a probe on each `cat.transition` row of the Game of Life graph starter, but the starter steps with `grid.life_like`, which has no rows. The test replaces that node with round 1's two transition rows (birth and death, with the same comparisons as round 1's lowered Game of Life) and probes them, plus the comparison "3 neighbours". The rewrite is inline text in the test, so it shares no file with S1b's fixtures.
- **A probe names a node output, not a pass.** `Probes` holds `PortRef`s, since node ids are unique across a file and a stat already names one by `PortRef`. The tick pass is found from the node.
- **Probes refuse a node in a region and an expanded kind.** A region body runs once per iteration and its values are out of scope at the pass's top level, and a `math.expr` or `fn.call` output has no name in the compiled program. Neither was in the plan. S31's probes come from author lines, which lower to neither.
- **The facade re-exports `FireCounts`.** `Snapshot.fire_counts` is a public field, and its type would otherwise be unnameable through `henad::runner`.
- **Storage bindings differ from the plan's numbers**, as the table above says.
- **Only `Snapshot` is breaking.** Plan section 9 lists the new `fire_counters` fields of `GpuGridSpec` and `GpuAgentSpec` as breaking. Neither type exists at v0.3.0, and both are `#[non_exhaustive]`, so the CHANGELOG lists the two fields under Added alone and names only `Snapshot.fire_counts` as breaking.
- **No record commit.** The task asks for the record draft outside the repository, numbered and committed at the merge, so the branch holds the feature commit and the review fix.

### Review

The review found two "should fix" items and five nits, and all seven held against the code.
The record cited a gitignored path for round 1's lowered Game of Life, and now names the comparisons alone.
The CHANGELOG called the two spec fields breaking, though neither spec type exists at v0.3.0 and both are `#[non_exhaustive]`, so its breaking line now names `Snapshot` alone, with a deviation above.
The module doc of `graph_probes.rs` now says the probed SIR starter is compared with the unprobed one.
The doc paragraphs of `wgsl/uniform.rs` and `wgsl/mod.rs` were re-broken within 120 columns, and the two long WGSL literals in `emit.rs` break one token earlier, with the emitted text unchanged.
The `fire_counters` clause of `agents/henad-compute.md` was rewrapped to the file's width.
The trailing "which" clauses on `GridPassCtx::counting`, `AgentRun::counting` and `FieldSource::CountFires` became two sentences each.
The topic-file sizes were counted again after the fix.
No fix changes behaviour, so none takes a new test.
After the fix, `cargo fmt --check`, clippy with `-D warnings` over henad-compute, henad-graph and henad-models, rustdoc with `-D warnings` over henad-compute and henad-graph, and `HENAD_REQUIRE_GPU=1` tests over the three crates pass, with henad-models' baseline budget test the only failure.

## State after

- `redesign/s8-fire-counts` held two unsigned `[draft]` commits after `98be226`, the feature and the review fix, and after the rebase it holds them after `15d8717`.
- `cargo fmt --all -- --check` passes.
- `cargo clippy -p henad-compute -p henad-graph -p henad-models -p henad --all-targets --all-features -- -D warnings -W clippy::all` passes.
- `HENAD_REQUIRE_GPU=1 cargo test -p henad-compute -p henad-graph -p henad-models -p henad --all-targets --all-features --no-fail-fast`: henad-compute 240 passed, henad-graph 185 passed and 1 ignored, henad-models' library 161 passed, 1 ignored and 1 failed, `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` as in the baseline, and every other test binary passes.
- `cargo check --workspace --all-targets --all-features` passes.
- The wasm32 typecheck of `./check.sh` over henad-core, henad-compute, henad-models, henad-graph, henad-explore and henad-app, and the facade's two, pass.
- `RUSTDOCFLAGS="-D warnings" cargo doc -p henad-compute -p henad-graph -p henad --no-deps --all-features` and the doc tests of henad-compute and henad-graph pass.
- The merge check first rebased the branch onto `37-node-editor` at `98be226`, its own base, and ran the full check on it.
  Every step exited as the baseline does.
  When the fast-forward was tried, S1a (record #73) had merged in the meantime and moved the tip to `15d8717`, so the branch was rebased again and the check repeated.
  The second rebase met one conflict, in `CHANGELOG.md`'s Added list, resolved by keeping both lines, and auto-merged the module list of henad-graph's `src/tests/mod.rs` and the facade's `henad::runner` re-exports.
- The full check of plan section 2.3 ran on the rebased branch at `1cdc81a`.
  Every step exits as the baseline does.
  The native and wasm32 typechecks, fmt, packaging, clippy, the doc tests, the README program, rustdoc, the scripts' tests and the web build exit 0.
  `cargo deny` exits 1 on RUSTSEC-2026-0370, 0371 and 0372, the baseline's three advisories.
  The workspace tests exit 101 on one test, the baseline's `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` in henad-models with the baseline's message (`boids.toml: 14816908000000 units in one submission`), and its log shows no other test binary that failed or aborted.
  No baseline test now passes.
- `37-node-editor` fast-forwarded to the branch with this record, and the worktree and the branch were removed.

## Issues found & future directions

- **Nothing sets a fire range outside tests yet.** `ModelFile::entry` builds graphs with no probes, so the CLI, the app and sweeps run as before. S31 names probes by author line and sets the range in `ModelFile::entry`, as D48 says.
- **The cost of the counting step is unmeasured.** A counted step pays the workgroup tallies, two barriers and one global atomic per probe per workgroup, and every other step one uniform test. The commands below measure it on Game of Life at 4096 by 4096.
- **A sweep counts too.** Each command buffer of a sweep counts its last step, one step in 64 when the run goes flat out, and nothing reads those counts. An `EngineOptions`-style switch could drop them if the measurement shows a cost.
- **The tally zeroing costs a barrier** that wgpu's zero-initialised workgroup memory might make unnecessary, as S3's record noted for counters. Here it runs on the counted step alone.
- **Each counting block is written once at build.** Every parameter of a GPU graph is reload-only, so the block never changes. L1 (live parameters, S24) will need to write both blocks of a step on an edit.
- **naga does not check barrier uniformity** (`agents/gpu-traps.md`). The probe barriers sit at the top level of `main` under a uniform test, and `no_step_returns_early` still holds every starter's shaders.

## Decisions adopted for review

| Id | Topic | Sources | Decision | Milestones |
| --- | --- | --- | --- | --- |
| D48 | Fire counts | recon `engine.md`, answer 9a, round1 question 9's recommendation | **Departure** from round1 question 9's recommendation, "(a), measured first at 10M cells". Probes beside the IR, outside its text and identity, counted on the last step of each `encode_steps` call, published in `Snapshot.fire_counts`, never as stats, on by default before the measurement. The maintainer's answer 9a reads "On by default, after change J" with no measurement condition, the counting step runs at most once per `encode_steps` call (once per 64 steps in a sweep or a CLI run), and its only frequent case is the app at one tick per snapshot, where a researcher watches the counts. `ModelFile::entry` sets the range in one place (S31), so an empty range turns counting off for every sheet if the maintainer's measurement asks for it | S8, S31 |

S8 builds the engine half of D48: the probes, the counted step and `Snapshot.fire_counts`.
Counting is not yet on for any model, since `ModelFile::entry` sets no range until S31.
The deviations above are the further choices made in the maintainer's absence.

## Benchmark commands left for the maintainer

Game of Life at 4096 by 4096 with and without the counting step, at one tick per snapshot and at 64 steps per snapshot, as round 1's question 9 asked before counts go on (D48).
No CLI route builds a probed graph before S31, so a small program builds both entries from the Game of Life starter with its step written as two transition rows, and probes those rows in one of them.
Each round builds a fresh state per entry and batch, warms up 64 steps, and times 1024 steps with a snapshot after each batch, the plain and the probed entry interleaved.
Close other work and let the machine cool first.

```bash
# From the root of the checkout holding this change.
HENAD=$(git rev-parse --show-toplevel)
BENCH=$(mktemp -d)
mkdir -p "$BENCH/src"
cp "$HENAD/Cargo.lock" "$HENAD/rust-toolchain" "$BENCH/"
cat > "$BENCH/Cargo.toml" <<EOF
[package]
name = "fire-count-bench"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
henad-compute = { path = "$HENAD/crates/henad-compute" }
henad-core = { path = "$HENAD/crates/henad-core" }
henad-explore = { path = "$HENAD/crates/henad-explore" }
henad-graph = { path = "$HENAD/crates/henad-graph" }

[workspace]
EOF
cat > "$BENCH/src/main.rs" <<'EOF'
//! Game of Life at a side of 4096 with and without probes, one tick per snapshot and 64.
#![recursion_limit = "256"]

use std::time::Instant;

use henad_compute::entry::{ModelEntry, ModelState};
use henad_compute::gpu::sim_thread::GpuSimState;
use henad_compute::gpu::{GpuContext, GpuNeeds, wgpu};
use henad_core::params::ParamValue;
use henad_graph::GraphFile;
use henad_graph::ids::NodeId;
use henad_graph::value::PortRef;
use henad_graph::wgsl::{GridProgram, Probes};

const LIFE_LIKE: &str = "[[pass.node]]\nid = \"n12\"\nop = \"grid.life_like\"\nconfig.category = \"life\"\n\
config.birth = [3]\nconfig.survive = [2, 3]\nconfig.dead = \"Dead\"\nconfig.alive = \"Alive\"\nin.cell = \"n10\"\n\
in.count = \"n11\"\n";

const ROWS: &str = "[[pass.node]]\nid = \"n14\"\nop = \"cmp.eq\"\nin.a = \"n11\"\nin.b = { u32 = 3 }\n\n\
[[pass.node]]\nid = \"n15\"\nop = \"cmp.ne\"\nin.a = \"n11\"\nin.b = { u32 = 2 }\n\n\
[[pass.node]]\nid = \"n16\"\nop = \"cmp.ne\"\nin.a = \"n11\"\nin.b = { u32 = 3 }\n\n\
[[pass.node]]\nid = \"n17\"\nop = \"logic.and\"\nin.a = \"n15\"\nin.b = \"n16\"\n\n\
[[pass.node]]\nid = \"n12\"\nop = \"cat.transition\"\nconfig.category = \"life\"\nconfig.rules = [\n\
  { id = \"birth\", from = \"Dead\", to = \"Alive\", phase = 0 },\n\
  { id = \"death\", from = \"Alive\", to = \"Dead\", phase = 0 },\n]\nin.state = \"n10\"\n\
in.rule.birth.when = \"n14\"\nin.rule.death.when = \"n17\"\n";

fn run(ctx: &GpuContext, state: &mut dyn GpuSimState, steps: u32, batch: u32) {
    let mut done = 0;
    while done < steps {
        let mut encoder = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        state.encode_steps(&mut encoder, batch, None);
        state.encode_snapshot_passes(&mut encoder);
        ctx.queue.submit(Some(encoder.finish()));
        state.begin_stats_readback();
        state.poll_stats_readback(&ctx.device, false);
        done += batch;
    }
    ctx.device.poll(wgpu::PollType::wait_indefinitely()).expect("the device drains");
    state.poll_stats_readback(&ctx.device, true);
}

fn main() {
    let side: u32 = std::env::args().nth(1).map_or(4096, |arg| arg.parse().expect("a side"));
    let ctx = henad_explore::device::acquire_headless(GpuNeeds::default()).expect("a device");
    let starter = henad_graph::starters::starter("game_of_life.toml").expect("the starter");
    assert!(starter.text.contains(LIFE_LIKE));
    let text = starter.text.replace(LIFE_LIKE, ROWS).replace("in.value = \"n12\"", "in.value = \"n12.next\"");
    let file = GraphFile::parse(&text).expect("the file reads");
    let port = |node: u32, name: &str| PortRef { node: NodeId::new(node).expect("a node"), port: Some(name.into()) };
    let probes = Probes::new(vec![port(12, "fired.birth"), port(12, "fired.death")]);
    let plain = GridProgram::compile(&file).expect("compiles").entry();
    let probed = GridProgram::compile_with_probes(&file, &probes).expect("compiles").entry();
    let params = [ParamValue::U32(side), ParamValue::U32(side), ParamValue::F32(0.3)];
    let steps = 1024;
    let entries: [(&str, &ModelEntry); 2] = [("plain", &plain), ("probed", &probed)];
    for round in 1..=5 {
        for (name, entry) in entries {
            for batch in [1, 64] {
                let ModelState::Gpu(mut state) = entry.build(&params, Some(1), Some(&ctx)).expect("builds") else {
                    panic!("built on the CPU");
                };
                run(&ctx, &mut *state, 64, batch);
                let start = Instant::now();
                run(&ctx, &mut *state, steps, batch);
                let ms = start.elapsed().as_secs_f64() * 1000.0 / f64::from(steps);
                println!("round {round} {name} batch {batch}: {ms:.4} ms/step, counts {:?}", state.fire_counts());
            }
        }
    }
}
EOF
(cd "$BENCH" && cargo run --release -- 4096) | tee "$BENCH/fire-counts.txt"
# Compare per round: probed against plain at batch 1 (one tick per snapshot) and at batch 64.
echo "results in $BENCH/fire-counts.txt; remove $BENCH once they are kept"
```

The program builds its own `target` under `$BENCH`, which takes a few minutes and a few gigabytes.
The probed lines also print the counts of the last counted step, which should be non-zero for both rows.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
