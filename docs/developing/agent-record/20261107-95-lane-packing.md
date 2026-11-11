---
date: 2026-11-07
title: "Lane packing"
description: "S29 of the #37 redesign, row K. A graph or sheet whose shader would bind more than 8 storage buffers, a fixed budget whatever the device, now packs lanes into shared u32 buffers, one segment per lane: plain lanes first, then dual lanes that share a writer step, never the position lane. The ants and boids sheets with one more Number property build on a baseline device, and a packed model steps as its unpacked twin. Tests read lanes by id."
icon: material/package-variant-closed
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s29-lane-packing at f9aa190, where the merge rebase found the branch already on the tip: three unsigned [draft] commits and this record; the full check with HENAD_REQUIRE_GPU=1 passes every step but cargo deny and the workspace tests, which fail as the baseline does, on its three advisories and its one budget test"
baseline_commit: e5ddef1
delta_state: "committed on `37-node-editor` by fast-forward: `chore: read lanes by id in tests [draft]`, `feat: pack lanes past eight bindings [draft]`, `fix: s29 review findings [draft]` and this record, unsigned, for the maintainer to reword and sign"
---

# Lane packing

> S29 builds row K with decision D50: when a generated shader would bind more than 8 storage buffers, the compiler packs a graph's lanes into shared buffers.
> The budget is fixed at the WebGPU default, never the device's own limit, so one file compiles to the same WGSL on every machine.
> Plain lanes pack first, then dual lanes, each packed buffer a `u32` array of one segment per lane, so neighbouring invocations still read neighbouring words.
> Lanes pack only with lanes of their domain, their writer step and their reruns, the drawable position lane never packs, and a packed buffer holds at most 128 MiB at the largest count the file sets.
> The ants and boids starters with one more Number property, and a grid sheet of three neighbour-read properties with one more, bind 9 storage buffers unpacked and 7, 8 and 5 packed, and build and run on a device of the default limits.
> The packed ants and the packed three strains step exactly as their unpacked twins, the ants across a press of Reset colony.
> Every test that read a graph's lane by buffer index now reads it by lane id, through `LaneBuffer` and a `GraphLanes` helper in henad-models.

## State before

- `37-node-editor` at `e5ddef1` (S24 merged).
- Every lane of a compiled graph had a buffer of its own at its lane position, `lane_<id>_<domain>`, and the grid's `GraphHooks`, the agents' `BufferSource::Lane`, the side tables, the position buffer and the reruns all took a lane's position for its buffer index.
- Recon `engine.md` section 2.7 counted the ants and boids starters at exactly 8 storage bindings in their tick pass, so one more property on either passed the WebGPU default.
- Tests in henad-models read lanes through `read_buffer(0)`, `read_buffer(1)` or a `lane_<id>_agents` label, and `planted_entry` planted whole buffers by label.
- The baseline of the full check is the gate's: `cargo deny` fails on RUSTSEC-2026-0370, 0371 and 0372, and the workspace tests on `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone.

## What was done

### Lanes read by id (first commit)

`wgsl/pack.rs` holds `LaneLayout`, which places each lane in a lane buffer (`LaneGroup`), and the public `LaneBuffer`: the buffer's position, the lane's words per element, `is_packed`, `segment_range` and `segment`, the lane's words from the whole buffer read back.
`GridProgram::lane_buffer(id)` and `AgentProgram::lane_buffer(id)` return it.
The grid's `GridBufferSpec`s and the agents' `AgentBufferSpec`s, `BufferSource::Lanes` (which replaces `BufferSource::Lane`) and the position buffer come from the layout.
The first commit kept every lane in its own buffer.

henad-models' `tests/lanes.rs` holds `GraphLanes` (`of`, `of_text`, `of_starter`, `place`, `read`), which compiles a graph once and reads a lane of a state by id.
Every lane read by buffer index or lane label in `graph_grid_parity.rs`, `graph_sir_parity.rs`, `graph_ants_parity.rs`, `graph_agent_parity.rs`, `graph_agent_runs.rs` and `graph_agent_repeat.rs` moved to it, and so did `graph_probes.rs` and `graph_repeat.rs`, which shared the removed `graph_cells`.
`planted_entry` plants a lane by id into its segment and a field by its label.
Fields and colours are no lanes and keep their labels.

### Packing (second commit)

`pack::choose` emits every shader with each lane unpacked, and only when some shader binds more than `limits::MAX_STORAGE_BINDINGS` (8) storage buffers does it emit again with the plain lanes packed, then with the dual lanes too.
Below the budget nothing changes, and every starter's shaders are byte for byte as before.
`LaneLayout::packed` groups the packable lanes by domain, by the writer step of a dual lane and by the actions whose reruns write the lane (`rerun_writers`).
A group closes at `limits::MAX_PACKED_BUFFER_BYTES` (128 MiB) at the domain's largest count, `max_count` agents or the cells of the default world, and a group of one lane keeps its own buffer.
A packed buffer is `lanes_<domain>_<k>`, with `_in` and `_out` when it is dual.

The emitter reads and writes a packed lane at `<offset>u * <count> + (<index>)`, `2u * (<index>)` for a `vec2`, where `<count>` is `params.henad_agents`, `params.henad_cells` or the grid's width times height.
Reads go through `bitcast<f32>` or `bitcast<i32>`, a `vec2` from two words, writes through `bitcast<u32>`, and a read at another element is bounded by the count instead of `arrayLength`.
Each lane buffer a shader uses is bound once, read-write when any of its lanes is written there.
The two binding loops, the side tables (rows now by buffer, through the layout), the grid's `buffer_lens`, seed and rerun uploads (`GraphHooks::lane_buffers`) and the agents' sources follow the layout.
Both compile functions moved their emission into a closure over a layout: the grid's whole plan list, and for agents the per-plan sides (`ShaderPlan::sides` is now the step or `None`).
henad-graph's `gpu_cases.rs` reads a case's lanes through the layout, since a radius-fold case on its 16-binding device now packs.

### Tests

- henad-graph `tests/packing.rs`:
  - `a_model_past_8_bindings_compiles_to_8`: the three strains alone bind exactly 8 and pack nothing. The ants starter, the boids starter and the three strains, each with one more Number property that a line adds 1 to every tick, bind 9 unpacked and at most 8 packed (7, 8 and 5), pack the lane named, and build and run three ticks on `baseline_test_device`, a device of `wgpu::Limits::default()`.
  - `packing_never_depends_on_the_device`: two compiles of the packed ants give the same composed texts with `lanes_agents_0`, and the one entry builds on a baseline device and on a raised one, the energy lane reading 2 after two ticks on both.
  - `a_packed_model_steps_as_its_unpacked_twin`: the packed ants and three strains against twins compiled with every lane unpacked (the thread-local `pack::UNPACKED` switch, test builds only) on the raised device, 20 ticks from one seed, then for the ants a press of Reset colony and 5 ticks: every lane equal, and every other buffer of the ants equal.
- henad-graph `wgsl/pack.rs`: `the_fixed_limits_are_the_webgpu_defaults`, `lanes_pack_only_with_lanes_of_their_domain_writer_and_reruns` (labels, offsets and a `vec2` segment), `a_packed_buffer_holds_at_most_the_baseline_binding_size_at_the_largest_count` (five lanes of ten million agents in buffers of three and two) and `a_lane_no_buffer_holds_has_no_place`.
- henad-graph `tests/sheets/three_strains.toml`: a grid sheet of three SIR strains, each counted at the eight neighbours with chances that draw.
- henad-graph `tests/support.rs`: `baseline_test_device` beside `test_device`, both through `device_with`.
- Every graph and sheet parity gate passes unchanged.

### Edited tree

```
CHANGELOG.md                                       lane packing
agents/
├── gpu-traps.md                                   the fixed binding budget
├── henad-graph-wgsl.md                            lane buffers and packing
└── performance.md                                 the budget, segments never interleaved
crates/henad-graph/src/
├── limits.rs                                      MAX_STORAGE_BINDINGS, MAX_PACKED_BUFFER_BYTES
├── tests/
│   ├── gpu_cases.rs                               a case's lanes through the layout
│   ├── mod.rs                                     packing
│   ├── packing.rs                                 new
│   ├── sheets/three_strains.toml                  new
│   └── support.rs                                 baseline_test_device, device_with
└── wgsl/
    ├── agent_hooks.rs                             BufferSource::Lanes
    ├── agents.rs                                  lane entries, emission over a layout, buffers from it, lane_buffer
    ├── emit.rs                                    layout in EmitInput, packed_word, count_of, store, bound_lanes
    ├── hooks.rs                                   lane_buffers: lengths, seeds and reruns per lane buffer
    ├── mod.rs                                     emission over a layout, rerun_writers, grid_cells, lane_buffer
    └── pack.rs                                    new: LaneBuffer, LaneLayout, Packing, choose
crates/henad-models/src/tests/
├── graph_agent_parity.rs                          lanes by id
├── graph_agent_repeat.rs                          lanes by id
├── graph_agent_runs.rs                            lanes by id
├── graph_ants_parity.rs                           lanes by id, planted_entry by lane id
├── graph_grid_parity.rs                           lanes by id, graph_cells removed
├── graph_probes.rs                                lanes by id
├── graph_repeat.rs                                lanes by id
├── graph_sir_parity.rs                            lanes by id, fixture_text
├── lanes.rs                                       new: GraphLanes
└── mod.rs                                         lanes
```

### Deviations from the plan

- **Dual lanes pack per writer step.**
  The plan packs the dual lanes "as segments at word offsets in one buffer pair".
  A buffer pair has one side row per step, and two dual lanes written by different steps need different rows in the steps between their writers, so dual lanes share a pair only with dual lanes of the same writer step.
  The three strains, whose three dual lanes share one writer, show the single pair the plan describes.
- **Lanes pack only with lanes the same actions rerun.**
  A rerun uploads a whole buffer from fresh host lanes, and a lane the rerun does not write would be reset with it.
  Grouping by the rerun set keeps the uploads whole with no change to henad-compute.
  In the ants every lane of an ant is rerun by Reset colony, so they pack together.
- **The split uses a fixed size at a count from the file.**
  The plan splits a packed buffer "past `max_storage_buffer_binding_size`", the device's value, which would make the shaders depend on the device and on the parameters.
  The split uses the WebGPU default (128 MiB) at the domain's largest count the file sets: `max_count` for agents, and for cells the default world's cells, since the size parameters' upper bounds (16384 squared cells, a 10000 by 10000 world) put one lane alone past it.
- **A model still past 8 after packing compiles.**
  Section 8.4 of `lowering.md` makes that a diagnostic.
  It compiles as before and a device with fewer bindings refuses it at build with the engine's shortfall, so a model that runs on a raised device today keeps running there.
- **The whole arena packs.**
  Section 8.4 merges "the smallest lanes first" until a pass fits.
  Once packing is needed, every packable lane of a group packs, which keeps the layout one function of the file and costs a packed read no more than an unpacked one beyond the offset.
- **More tests moved.**
  `graph_probes.rs` and `graph_repeat.rs` read through `graph_cells`, which the first commit removed, so they moved too.
  The equality of a packed model with its unpacked twin is tested in henad-graph, where a crate-private switch can compile the twin, rather than in henad-models.

## State after

- A graph or sheet whose shader would pass 8 storage bindings compiles to shaders within 8 wherever packing can bring it there, and runs on a browser or a baseline device.
- Below the budget every model compiles exactly as before.
- Tests and helpers read a graph's lanes through `LaneBuffer`, whatever buffer holds them.
- `37-node-editor` holds the three unsigned commits, the test helper, the packing and the review's fixes, fast-forwarded with no merge-check fix.

### Merge check

- The merge rebase onto `37-node-editor` found the branch already on its tip, `e5ddef1`, so no commit was replayed and no conflict arose.
- The full check then ran on the branch with `HENAD_REQUIRE_GPU=1`.
  Every step exits 0 but two, and both fail as the baseline does.
  `cargo deny` reports the baseline's three advisories, RUSTSEC-2026-0370, 0371 and 0372, and no other.
  The workspace tests fail on the baseline's budget test `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone, in the one failing target (henad-models' library), with no new failing name and no aborted test binary in the log.
- No baseline test passes now.

## Issues found & future directions

- **Packing shrinks the largest world on a baseline device.** A packed buffer of three cell lanes reaches 128 MiB at a third of the cells one lane does, so a grid enlarged far past its default size can be refused on a baseline device where the unpacked model, had it fit the bindings, would not. The engine's shortfall names the packed buffer `lanes_cells_0`, which a researcher does not know; mapping it to the lanes it holds is open.
- **A dual lane written again by a later step** is bound as `_in` and `_out` on the same side, as before packing. No checked file reaches it; packing changes nothing there.
- **The cost of packing is unmeasured.** A packed access adds one uniform multiply and a `bitcast`, and the segments keep reads coalesced. The commands below are for the maintainer.

## Decisions adopted for review

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D50 | When lanes pack | recon `engine.md` | Past a fixed budget of 8 storage bindings, never the device's limit, plain lanes first, never position or colour | S29 |

S29 also takes the choices listed under the deviations: dual lanes per writer step, lanes per rerun set, the fixed split size at a count from the file, a model past 8 after packing compiled as it is, and the whole arena packed.

## Review

A reviewer with a fresh context found one should-fix item and three nits, all fixed in `fix: s29 review findings [draft]`.
The should-fix item was that no test checked a packed `i32` lane or a packed `vec2` lane at a non-zero offset, so two of the four packed code shapes in the emitter's `load` and `store` were never compared with anything.
The twin test now runs the ants starter with three more properties after its own (a Number that gains 1, a Whole number that starts at -5 and loses 2, and an Arrow that starts in a random direction and doubles), which packs `tally` as an `i32` at offset 3 and `drift` as a `vec2<f32>` at offset 4 beside Cargo, Reward held and Energy.
That sheet replaces the ants with one more Number in the twin test, since it holds the same lanes and more, and the test asserts both new lanes are packed at a non-zero offset.
Each shape fails the test when broken by hand: an `i32` read as `i32(word & 0xff)` makes `tally` differ from its twin, and a `vec2` read of the first word twice makes `drift` differ.
The nits: the module doc of `wgsl/mod.rs` now names the packed buffers and links `LaneBuffer` (the review's link to the private `pack` module would fail `cargo doc -D warnings`), and the three docs the review quoted lose their parallel frame and trailing "which" clauses.
The third nit moved the grid cases of `gpu_cases.rs` and both helpers of `gpu_repeat.rs` from one buffer per lane, labelled `lane_<id>_cells`, to `support::lane_buffers` and `support::lane_words` over `compiled.layout`, and `sheet_agents.rs` now reads its hit lane through `AgentProgram::lane_buffer`.
The topic file's sentence names all three files.

## Benchmark commands for the maintainer

The plan asks for the packed ants against the unpacked at 1M ants.
The compiler has no switch outside test builds, so the closest pair on the CLI is the ants starter (8 bindings, unpacked) against the ants starter with one more Number property (packed, the cargo, reward and energy lanes in `lanes_agents_0`).
The packed model also adds one to its extra lane each tick, which a reader of the numbers subtracts by eye.
Make the packed sheet as the test does, then run the two interleaved on a cooled machine as `agents/performance.md` asks:

```bash
python3 - <<'EOF'
text = open("crates/henad-graph/sheets/ants.toml").read()
text = text.replace("next_line = 17", "next_line = 18", 1)
text = text.replace('"l12", "l16"]', '"l12", "l16", "l17"]', 1)
text = text.replace("[ground]\n", '[[agents.property]]\nid = "energy"\nname = "Energy"\ntype = "number"\n\n'
                    '[agents.property.starts_as]\nrule = "value"\nvalue = 0\n\n[ground]\n', 1)
text += ('\n[card.line.l17]\nkind = "set"\nproperty = "energy"\n\n[card.line.l17.value]\nkind = "calculate"\n'
         'formula = "my.energy + 1"\n\n[card.line.l17.condition]\nkind = "always"\n')
open("/tmp/ants_energy.toml", "w").write(text)
EOF
for rep in 1 2 3; do
  cargo run --release -p henad-cli -- --model-file crates/henad-graph/sheets/ants.toml \
    --set num_agents=1000000 --steps 500 --reps 1 --json
  cargo run --release -p henad-cli -- --model-file /tmp/ants_energy.toml \
    --set num_agents=1000000 --steps 500 --reps 1 --json
done
```

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
