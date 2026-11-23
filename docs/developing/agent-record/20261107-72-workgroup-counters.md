---
date: 2026-11-07
title: "Workgroup counters"
description: "Row J of the #37 redesign in henad-graph's WGSL compiler. A step of an agents graph tallies each counter in workgroup memory and flushes it with one global atomic per workgroup, a zero amount adds nothing, and a deposit of 0, a negative or a NaN issues no atomic. No step returns early, and the results stay bit for bit as they were. The WGSL compiler's notes move to their own topic file."
icon: material/counter
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s3-workgroup-counters, three unsigned [draft] commits and this record after 6cdd292; the full check with HENAD_REQUIRE_GPU=1 passes every step but cargo deny and the workspace tests, which fail as the baseline does, on its three advisories and its one budget test"
baseline_commit: 79e6d1c
delta_state: "committed on `37-node-editor` by fast-forward: `ai: split the wgsl topic file out of henad-graph [draft]`, `feat: workgroup counters and no zero deposits [draft]`, `fix: s3 review findings [draft]` and this record, unsigned, for the maintainer to reword and sign"
---

# Workgroup counters

> S3 of the #37 redesign plan builds row J of round 1 in henad-graph's WGSL compiler.
> A step of an agents graph adds each `counter.add` into a workgroup array, skips a zero amount, and after a barrier flushes each counter's tally with one global `atomicAdd` per workgroup, the shape of the reduce leaf.
> A `field.deposit` of 0, of a negative value or of a NaN now issues no `atomicMax`.
> No step returns early: the grid step's bounds check and the agents field update's slot check wrap the body in a block, so a barrier at the end of `main` sits in uniform control flow.
> A step reports its tallies as workgroup bytes, which `Demand::shortfalls` holds to the device.
> The results are bit for bit as before, and every graph parity gate in henad-models passes unchanged.
> naga turned out to accept a barrier after an early return, so a WGSL text test holds the rule, and `agents/gpu-traps.md` records it.
> The two WGSL compiler sections of `agents/henad-graph.md` moved unchanged into a new `agents/henad-graph-wgsl.md`.

## State before

`37-node-editor` at `79e6d1c` held rounds 1 to 3 of #37 and records #61 to #70.
In `crates/henad-graph/src/wgsl/emit.rs`:

- `Operation::CounterAdd` wrote `atomicAdd(&counters[k], amount)` for every element, a zero amount included, so each agent of a step that counts issued one global atomic.
- `Operation::Deposit` wrote `atomicMax` of `select(0u, bitcast<u32>(v), v > 0.0f)` inside the cell and layer bounds check, so every depositing agent issued an atomic, 0 included.
- A grid step, action and colour shader returned early for invocations past the grid, and an agents field update returned early for invocations past its slots.
- A tick pass reported 0 workgroup bytes, and only a reduce shader declared workgroup memory.

`agents/henad-graph.md` held the whole crate at 35,835 characters, the WGSL compiler included.

## What was done

### The topic file split

The first commit moves "The WGSL compiler" and "The WGSL compiler over agents" out of `agents/henad-graph.md` into `agents/henad-graph-wgsl.md`, byte for byte, behind a two-line header.
`agents/henad-graph.md` keeps a one-line pointer where the grid section stood, and `AGENTS.md`'s topic table gains the row "henad-graph's WGSL compiler of grid and agents graphs".
The henad-graph row drops "WGSL compiler" from its list.

### Counters

A step that adds to a counter declares `var<workgroup> henad_counter_tally: array<atomic<u32>, N>`, one slot per counter its passes add to, numbered in the order the passes first add to each (`Emitter::counter_slots`).

- `counter.add` writes `let henad_amount_nK = ...;` and adds it into its slot only when it is not 0.
- `main` zeroes the slots (invocations below `N`) before a `workgroupBarrier()` at its top.
- After the passes, a second barrier, then the invocation whose local index equals each slot loads the tally and adds it to the counter's word of `counters` with one `atomicAdd` when it is not 0.
- Both barriers sit at the top level of `main`, outside every pass's guard.
- A tally wraps at 2^32 as the counter does, so the device total equals the host's wrapping sum.
- The step's `workgroup_bytes` are four per slot, and `AgentProgram::spec` already passes them to each step's `AgentPassSpec`.

The checker already keeps `counter.add` at the top of a tick pass over agents (its contexts allow no region, and no function holds a sink).
The compiler now also refuses one anywhere else, since only the agents step template flushes the tallies.

### Deposits

The test `v > 0.0f` moves into the guard beside the cell and layer bounds, and the `atomicMax` takes `bitcast<u32>(v)` with no `select`.
An `accum` word starts at 0 and never falls, so the `atomicMax` of 0 the old form issued changed nothing.

### No early return

- The grid step's `if (out of bounds) { return; }` became `if (in bounds) { ... }` around the cell lines and the body, as the grid reduce shader already did. Grid actions and the agents colour role share that template arm.
- The agents field update (`Role::Merge`) did the same with its slot check.
- Display shaders keep their early return. A display runs view passes, which hold no counter.

The text inside each new block is indented one level deeper than the block by `Text::indented`, which also moves the node ranges, and the grid reduce shader takes the same helper (see Review).

### Tests

| Test | File | What it holds |
|---|---|---|
| `each_counter_flushes_once_per_workgroup_after_a_top_level_barrier` | `src/tests/agent_wgsl.rs` | A pass adding to counter `a` twice and `b` once: a workgroup array of 2, one flush per counter word, three tally adds, a zero skip per add, exactly two barriers both at the top level of `main`, 8 workgroup bytes in the step's spec, and one flush of the ants starter's deliveries |
| `a_deposit_issues_no_atomic_unless_its_value_is_above_zero` | `src/tests/agent_wgsl.rs` | Every `atomicMax(&accum_` line of every agent starter and agents fixture sits right under an `if (henad_deposit_nK > 0.0f && ...` and holds no `select` |
| `no_step_returns_early` | `src/tests/wgsl.rs` | No `return` in `main` of any shader but the display of any starter or fixture, grid or agents, and each of the step, reduce, action, colour and field update roles met at least once |
| `a_counter_over_many_workgroups_equals_the_host_total` | `src/tests/gpu_cases.rs` | 1,000,000 agents (3907 workgroups), even indices adding 0 and odd ones a hash that wraps the total many times: the device counter after one step equals the host evaluator's `run_pass` total and the wrapping sum |

Two planted errors checked the tests.
A flush skipped in workgroup 7 made the device test fail (2139120640 against 2281055232).
An early `return` planted before the zeroing in the agents step passed `compose_wgsl` and every existing test.
naga's silence on barrier uniformity came to light this way.
`no_step_returns_early` and the barrier assertion fail on it.

### Storage bindings of each starter

From `HENAD_DUMP_WGSL` over henad-models' tests on this tree, the composed modules as the engine built them.
The engine's own index and float-sum shaders sit under the model's label too and are listed for completeness.

| Starter | Shader | Storage bindings | Workgroup memory |
|---|---|---|---|
| game_of_life | step | 2: `lane_state_cells_in` (read), `lane_state_cells_out` | |
| | reduce | 2: `lane_state_cells` (read), `counters` | `henad_partial` 2 words |
| | display | 1: `lane_state_cells` (read), and the texture | |
| | action_clear_cells, action_randomise_cells | 1: `lane_state_cells` | |
| vote | step | 2: `lane_vote_cells_in` (read), `lane_vote_cells_out` | |
| | reduce | 2: `lane_vote_cells` (read), `counters` | `henad_partial` 2 words |
| | display | 1: `lane_vote_cells` (read), and the texture | |
| sir | step | 4: `lane_health_cells_in` (read), `lane_health_cells_out`, `henad_tick_in` (read), `henad_tick_out` | |
| | reduce | 2: `lane_health_cells` (read), `counters` | `henad_partial` 3 words |
| | display | 1: `lane_health_cells` (read), and the texture | |
| | action_outbreak | 1: `lane_health_cells` | |
| boids | flock | 7: `lane_pos_agents_in` (read), `lane_pos_agents_out`, `lane_vel_agents_in` (read), `lane_vel_agents_out`, `cell_start` (read), `sorted` (read), `counters` | |
| | color | 2: `lane_vel_agents` (read), `agent_colors` | |
| | reduce_leaf | 2: `lane_vel_agents` (read), `partials` | `block_sum` scratch 256 floats |
| | action_randomise_headings | 1: `lane_vel_agents` | |
| | engine: hash_count, hash_scan, hash_scan_add, hash_scatter, reduce | 3, 3, 2, 3, 2 | scan 256 words, reduce 256 floats |
| ants | walk | 7: `lane_pos_agents`, `lane_last_step_agents`, `lane_cargo_agents`, `lane_reward_held_agents`, `field_pheromone_cells` (read), `field_sites_cells` (read), `counters` | `henad_counter_tally` 1 word |
| | lay | 5: `lane_pos_agents` (read), `lane_cargo_agents` (read), `lane_reward_held_agents` (read), `field_pheromone_cells` (read), `accum_pheromone_cells` | |
| | update_pheromone | 2: `field_pheromone_cells`, `accum_pheromone_cells` | |
| | advance_tick | 1: `counters` | |
| | color | 2: `lane_cargo_agents` (read), `agent_colors` | |
| | display | 2: `field_pheromone_cells` (read), `field_sites_cells` (read), and the texture | |
| | reduce_leaf | 4: `lane_cargo_agents` (read), `field_pheromone_cells` (read), `counters`, `partials` | `block_sum` scratch 256 floats, `henad_partial` 1 word |
| | action_reset_colony | 1: `field_pheromone_cells` | |
| | engine: reduce | 2 | 256 floats |
| random_walk | walk | 3: `lane_pos_agents`, `lane_last_agents`, `counters` | |
| | advance_tick | 1: `counters` | |
| | color | 2: `lane_last_agents` (read), `agent_colors` | |
| | reduce_leaf | 2: `lane_pos_agents` (read), `partials` | `block_sum` scratch 256 floats |
| | engine: reduce | 2 | 256 floats |

The widest passes are boids' `flock` and ants' `walk` at 7 storage buffers, one under wgpu's default `max_storage_buffers_per_shader_stage` of 8.
J adds no binding: `counters` was bound by the ants' walk before, and the tally is workgroup memory.

### Docs and topic files

- `agents/henad-graph-wgsl.md` (new, 10,664 characters): the two moved sections, a line on the step's bounds block, a line on the field update's slot block, and a paragraph "Counters and deposits" with its tests.
- `agents/henad-graph.md` (27,294 characters): the pointer paragraph, and the test list names the early-return test and the counter and deposit forms.
- `agents/gpu-traps.md` (15,616 characters): "A workgroup barrier needs uniform control flow, and naga does not check it."
- `AGENTS.md`: the topic table row.
- `CHANGELOG.md`: one line under Changed, with "Results unchanged."

### Edited tree

```
AGENTS.md                                    the henad-graph-wgsl.md row
CHANGELOG.md                                 workgroup counters, no zero deposits, no early return
agents/
├── gpu-traps.md                             a barrier needs uniform control flow, naga does not check it
├── henad-graph-wgsl.md                      new: the WGSL compiler sections, counters and deposits
└── henad-graph.md                           the two sections moved out, a pointer, the test list
crates/henad-graph/src/
├── tests/
│   ├── agent_wgsl.rs                        counter flushes and barriers, deposits under their guard
│   ├── gpu_cases.rs                         a counter over a million agents equals the host total
│   └── wgsl.rs                              no shader but the display returns early
└── wgsl/emit.rs                             counter_slots, the tally, the flush, the deposit guard, guarded blocks,
                                             Text::indented
```

### Review

The review of S3 raised four nits, and all four are fixed in `fix: s3 review findings [draft]` (`b71de16` after the rebase).

- **Indentation inside the guarded blocks.** The grid step, the grid actions on its template, the grid reduce shader and the agents field update wrote their body one level shallower than the enclosing `if`.
  The reviewer suggested folding the bounds test into each pass's guard.
  That fits the grid step but leaves the field update's own lines (the `accum` load and reset) without a guard, so a helper was taken instead.
  `Text::indented` adds four spaces before each non-empty line and moves the node ranges with them, and each guarded block now passes its body through it.
  `indented_text_moves_its_ranges_with_its_lines` holds the helper, and a dump of the starters shows game_of_life's step, sir's reduce and the ants' pheromone update nested correctly.
- **The barrier test rested on indentation.** `each_counter_flushes_once_per_workgroup_after_a_top_level_barrier` now counts the brace depth at each `workgroupBarrier()` line of `main` and requires 1 for both.
- **The early-return test missed roles on the rewritten arm.** `no_step_returns_early` now covers every role but the display, the grid reduce and the agents reduce included, and asserts that the step, reduce, action, colour and field update roles each appear at least once in the starters and fixtures.
  The test keeps its name, which the trap entry and the WGSL topic file cite.
  Its doc, the test list of `agents/henad-graph.md` and the test line of `agents/henad-graph-wgsl.md` name the wider scope.
- **A 149-character line in `agents/henad-graph.md`.** The test-list paragraph is rewrapped at 100 columns.
  Line 230 of that file, at 158 characters, predates S3 and is left alone.
- **This record.** The planted-error sentence is split into three, and the storage-binding limit names wgpu's default `max_storage_buffers_per_shader_stage`.

After the fixes, `cargo fmt --all -- --check` and `cargo clippy -p henad-graph --all-targets --all-features -- -D warnings -W clippy::all` pass.
`HENAD_REQUIRE_GPU=1 cargo test -p henad-graph --all-targets --all-features` passes 179 and ignores 1.
`HENAD_REQUIRE_GPU=1 cargo test -p henad-models --all-features --lib graph` passes 42, ignores 1 and fails only the baseline budget test with the baseline's message.

### Deviations from the plan

- **The record commit is left to the merge step.** The milestone's task asks for the record draft outside the repository, to be numbered and committed at the merge, so the branch holds the first two planned commits.
- **The field update lost its early return too.** The plan names the grid step alone. The trap entry says no tick pass returns early, and the field update is a tick pass the compiler adds, so it takes the same guarded block. It holds no counter today.
- **The grid step's guarded block carries no barrier yet.** Counters exist only in agents worlds, so on a grid the change is shape alone, as the plan's scope lists it.
- **The compiler refuses a `counter.add` outside an agents step.** The checker already prevents one, and the refusal keeps the flush from being skipped silently if a later context opens.
- **The acceptance line "naga accepts the barrier's uniformity" holds less than it reads.** naga accepted a barrier after a planted early return as well. Every starter and corpus file compiles, and the uniformity rests on the text tests and the trap entry.

## State after

- `redesign/s3-workgroup-counters` held three unsigned `[draft]` commits after `79e6d1c`, the third holding the review fixes, and after the rebase it holds them after `6cdd292`.
- `cargo fmt --all -- --check` passes.
- `cargo clippy -p henad-graph -p henad-models --all-targets --all-features -- -D warnings -W clippy::all` passes.
- `HENAD_REQUIRE_GPU=1 cargo test -p henad-graph -p henad-models --all-targets --all-features --no-fail-fast`: henad-graph 178 passed and 1 ignored (the Metal float Repeat gate), 179 passed after the review, henad-models' library 159 passed, 1 ignored and 1 failed, `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` with the baseline's message (`boids.toml: 14816908000000 units in one submission`), and every other henad-models test binary passes.
  The parity gates `graph_ants_parity`, `graph_agent_parity`, `graph_grid_parity`, `graph_sir_parity` and `graph_agent_runs` pass but for that budget test.
- `cargo check -p henad-graph --all-features --lib --target wasm32-unknown-unknown` and `RUSTDOCFLAGS="-D warnings" cargo doc -p henad-graph --no-deps --all-features` pass.
- The merge check rebased the branch onto `37-node-editor` at `6cdd292`, which held S6 (record #71), with no conflict, and ran the full check of plan section 2.3 on the rebased branch at `b71de16`.
  Every step exits as the baseline does.
  The native and wasm32 typechecks, fmt, packaging, clippy, the doc tests, the README program, rustdoc, the scripts' tests and the web build exit 0.
  `cargo deny` exits 1 on RUSTSEC-2026-0370, 0371 and 0372, the baseline's three advisories.
  The workspace tests exit 101 on one test, the baseline's `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` in henad-models with the baseline's message, and its log shows no other test binary that failed or aborted.
  No baseline test now passes.
- `37-node-editor` fast-forwarded to the branch with this record, and the worktree and the branch were removed.

## Issues found & future directions

- **naga does not enforce barrier uniformity.** A future emitter form that returns early, or puts a barrier inside a pass's guard, compiles and runs with undefined results. The text tests catch the forms written today, and S8's probes and S15's sum merge will add barriers of their own, so each needs the same test.
- **J's effect on speed is unmeasured.** The ants' walk issued one global atomic per ant per step for its deliveries, and every laying ant one `atomicMax`. No speed-up is claimed until the commands below run.
- **Boids hold no counter and no deposit**, so their graphs take no part of J. They stay in the commands as a control.
- **The tally zeroing** costs a barrier that wgpu's zero-initialised workgroup memory might make unnecessary. The reduce leaf zeroes its tallies the same way, and the commands below would show whether either barrier matters.

## Decisions adopted for review

None of the rows of plan section 6 names S3, and the gate added none for it.
D48 (fire counts on by default, after change J, built in S8 and S31) depends on this milestone: S8 tallies each probe as S3 tallies a counter.
The deviations above are the choices made in the maintainer's absence.

## Benchmark commands left for the maintainer

The ants and boids graphs against their stock ports, interleaved, before and after J.
Close other work, let the machine cool, and check that at least 12 GB is free, since the old tree builds its own `target`.

```bash
# Once, from the root of a checkout holding this change: an old tree at the base commit and both release binaries.
NEW_TREE=$(git rev-parse --show-toplevel)
BENCH=$(mktemp -d)
git worktree add "$BENCH/old" 79e6d1c
(cd "$BENCH/old" && cargo build --release -p henad-cli)
(cd "$NEW_TREE" && cargo build --release -p henad-cli)
OLD=$BENCH/old/target/release/henad-cli
NEW=$NEW_TREE/target/release/henad-cli
FIX=$NEW_TREE/crates/henad-models/tests/fixtures/graphs
ST=$NEW_TREE/crates/henad-graph/starters
ANTS="--set num_agents=1000000 --set world_width=1000 --set world_height=1000"

# Interleaved: in each round the stock port, then each graph on the old binary and on the new one.
for round in 1 2 3 4 5; do
  $NEW gpu_boids --steps 2000 --reps 5 --json > "$BENCH/boids-stock-$round.jsonl"
  for graph in "$FIX/gpu_boids.toml" "$ST/boids.toml"; do
    name=$(basename "$graph" .toml)
    $OLD --model-file "$graph" --steps 2000 --reps 5 --json > "$BENCH/boids-$name-old-$round.jsonl"
    $NEW --model-file "$graph" --steps 2000 --reps 5 --json > "$BENCH/boids-$name-new-$round.jsonl"
  done
  $NEW gpu_ants --steps 2000 --reps 5 $ANTS --json > "$BENCH/ants-stock-$round.jsonl"
  for graph in "$FIX/gpu_ants.toml" "$ST/ants.toml"; do
    name=$(basename "$graph" .toml)
    $OLD --model-file "$graph" --steps 2000 --reps 5 $ANTS --json > "$BENCH/ants-$name-old-$round.jsonl"
    $NEW --model-file "$graph" --steps 2000 --reps 5 $ANTS --json > "$BENCH/ants-$name-new-$round.jsonl"
  done
done

# Per graph, the median over rounds of each round's median rep over the stock port's in the same round, old and new.
python3 - "$BENCH" <<'PY'
import json, statistics, sys
bench = sys.argv[1]
def median_elapsed(path):
    lines = (json.loads(line) for line in open(path))
    return statistics.median(line["elapsed_s"] for line in lines if line["kind"] == "rep")
for model, graphs in (("boids", ("gpu_boids", "boids")), ("ants", ("gpu_ants", "ants"))):
    for graph in graphs:
        for side in ("old", "new"):
            ratios = [
                median_elapsed(f"{bench}/{model}-{graph}-{side}-{r}.jsonl")
                / median_elapsed(f"{bench}/{model}-stock-{r}.jsonl")
                for r in range(1, 6)
            ]
            print(model, graph, side, f"{statistics.median(ratios):.3f}", [f"{ratio:.3f}" for ratio in ratios])
PY

git worktree remove --force "$BENCH/old"
```

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
