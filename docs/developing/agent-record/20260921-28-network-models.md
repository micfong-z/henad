---
date: 2026-09-21
title: "Models over a network"
description: A fifth authoring trait for models whose agents sit on a graph, an engine that keeps the graph current every tick and lays it out at publish cadence, ports of NetLogo's Virus on a Network and Team Assembly, and one-off actions on every trait.
icon: material/graph-outline
status: ai-generated
model: claude-opus-5 (Claude Code), the plan and one audit on claude-fable-5-1, the phase 8 audit and its fixes on claude-opus-5-5
issue: "#15"
state: implemented in eight phases and audited, `./check.sh` green, both models driven through the live app, both new fixtures equivalent against NetLogo 7.0.4
baseline_commit: 6e42f57
delta_state: phases 1 to 7 committed on `15-network-models` (42641d0 to d5f0e8e), phase 8 uncommitted
---

# Models over a network

> Nothing in Henad knew about edges.
> The views were a grid and a set of points, the renderer drew point sprites, and a population was allocated once and never changed size.
> Issue #15 asked for network models, with NetLogo's Virus on a Network and Team Assembly as the two to port, a rewire button for the first and a layout that keeps up with the second.
> The work added a `NetworkModel` trait with its own engine, a graph whose rows stay current under inserts and removals, a spring layout that runs at publish cadence under a time budget, an edge renderer, and the two models.
> The rewire button turned into a general mechanism: every trait, CPU and GPU, can now declare actions, and `henad-cli --act` replays them.
> The last phase wrote the documentation, a network tutorial with a compiled twin, and fixture procedures for comparing both models against NetLogo.
> An audit of that phase on a newer model found the Virus fixture's margins too wide to catch a wrong engine and a fault in a CLI fix.
> A review of the fixes found the layout spreading an unplaced node's `NaN` along its edges.
> All three are fixed here.
> Both models then read equivalent against NetLogo on every statistic their fixtures compare.

## State before

`TopologyHint` had two flags, `grid` and `agents`, and the views were `GridView` and `PointView`.
`agent_lanes!` allocated a fixed population at `alloc(n)`, and `docs/reference/primitives.md` listed dynamic populations under "Not provided".
A model could not declare anything for a user to trigger mid-run, and the CLI had no way to replay one.

`ParamKind` already had `Bool` and `Choice`, but no builder helpers, and a percentage was a plain `f32` with "%" in its label.

An earlier gap report had parked networks pending a scope decision.
Mesa wraps NetworkX and lays out once, Agents.jl puts agents on the nodes of a `GraphSpace`, MASON's `Network` is hash-map adjacency, and krABMaga's virus example uses a HashMap-backed field with fixed positions.
None of them separates layout cadence from tick cadence.

## What was done

New files marked **+**, modified marked **~**.
The tree covers the whole branch against `master`.
`about.toml` and `docs/license.html` rode in with the phase 5 commit from a parallel session (the `rfd` and `miniz_oxide` licence clarifies), and `56bc3f7` and `d68e442` are the comment sweeps.
None of those are listed.

```
CHANGELOG.md                                     ~ Unreleased: actions, network models, Changed
AGENTS.md                                        ~ fifth trait, network files, actions
zensical.toml                                    ~ nav: network trait page, tutorial, this record
README.md                                        ~ counts, a working henad-cli command

crates/henad-core/src/
├── action.rs                                    + ActionDescriptor, actions!, action_seed
├── network.rs                                   + Network: node slots, edge list, rows with slack
├── authoring/
│   ├── model/
│   │   ├── network_model.rs                     + NetworkModel, Nodes, NodeCtx, SpringParams
│   │   ├── grid_model.rs, agent_model.rs        ~ ACTIONS and act
│   │   ├── gpu_grid_model.rs                    ~ GpuGridAction, action_params_bytes
│   │   ├── gpu_agent_model.rs                   ~ GpuAgentAction, PassId::Action, PassCtx.seed
│   │   └── mod.rs                               ~ pub mod network_model
│   └── primitives/rng.rs                        ~ next_index
├── export/state.rs                              ~ write_edges, point_rows, retired nodes skipped
├── helpers.rs                                   ~ bool_param, choice_param and their extractors
├── params.rs                                    ~ ParamFormat::Percent
├── metadata.rs                                  ~ Structure::Network
├── model.rs                                     ~ SimState: act, edge_view, set_layout, relax_layout
├── spatial_hash.rs                              ~ build_where
├── topology.rs                                  ~ TopologyHint.edges, NETWORK
├── view.rs                                      ~ EdgeView, a sample at the newest tick replaces it
└── lib.rs                                       ~ modules

crates/henad-compute/src/
├── cpu/
│   ├── network_engine.rs                        + NetworkModelState
│   ├── layout.rs                                + spring layout: cutoff, saturation, damping
│   ├── primitives/components.rs                 + connected component labels
│   ├── primitives/lanes_macro.rs                ~ grow, positions_mut
│   ├── grid_engine.rs, agent_engine.rs          ~ act
│   ├── field/ca.rs, field/scalar.rs             ~ grid_mut, field_mut for actions
│   ├── sim_thread.rs                            ~ Act, SetLayout, serial, view_ms, edge recycling
│   └── mod.rs                                   ~ modules
├── gpu/
│   ├── grid_engine.rs, agent_engine.rs          ~ action passes, encode_action
│   └── sim_thread.rs                            ~ Act
├── runner/mod.rs                                ~ MAX_VIEW_BUDGET_MS
├── runtime_info.rs                              ~ vertex_storage
└── snapshot.rs                                  ~ serial, view_ms, EdgeSnapshot

crates/henad-models/
├── build.rs                                     ~ five action shaders
├── src/
│   ├── virus_network/                           + mod.rs, lanes.rs, step.rs, wiring.rs
│   ├── team_assembly/                           + mod.rs, lanes.rs, assembly.rs, ring.rs, live.rs
│   ├── game_of_life.rs, sir.rs, boids/, ants/   ~ one or two actions each
│   ├── gpu_*/                                   ~ the same actions, + one .wgsl pass per action
│   ├── registry.rs                              ~ register_network_model, action tests
│   ├── lib.rs                                   ~ modules
│   └── tests/tutorial/
│       ├── virus.rs                             + the network tutorial's twin
│       ├── gpu_foraging.rs                      ~ PassId::Action arm
│       ├── parity.rs                            ~ the twin against virus_network
│       └── mod.rs                               ~ pub mod virus
└── tests/
    ├── consistency_virus_network.rs             +
    ├── consistency_team_assembly.rs             +
    └── fixtures/docs/
        ├── virus_network_fixture.md             + NetLogo procedure, margins and result
        └── team_assembly_fixture.md             + NetLogo procedure, margins and result

crates/henad-app/
├── build.rs                                     ~ edges.wgsl entry point
└── src/
    ├── lib.rs                                   ~ one chart and recording row per tick, the latest publish's
    ├── state.rs                                 ~ layout and edge settings, SetLayout on build
    └── ui/
        ├── mod.rs                               ~ mod edge_layer
        ├── edge_layer.rs, edges.wgsl            + instanced edges and arrowheads
        ├── world.wgsl                           + shared placement test and world-to-clip
        ├── agent_layer.rs, agents.wgsl          ~ owns the edge layer, hides retired nodes
        ├── viewport.rs                          ~ keyed on serial, Edges and Arrows checkboxes
        ├── pacing.rs                            ~ Layout, Layout while paused, Layout budget
        ├── params.rs                            ~ action buttons, percentage sliders
        ├── model.rs                             ~ Network topology, edge palette, a dropdown
        │                                          as tall as the window
        ├── performance.rs, system.rs            ~ Prepare view, Network edges
        ├── stats.rs                             ~ three decimals for a fraction
        └── export/                              ~ edges in the state export and the image, a held-back recording row

crates/henad-cli/src/
├── actions.rs                                   + --act schedule, the ticks a run fires (Fire)
├── main.rs                                      ~ --act, edges in --export, prepared stats samples,
│                                                  GPU actions timed as on the CPU
└── json_report.rs                               ~ actions in the summary

scripts/
├── compare_network.py                           + distributional comparison for both models
├── compare_sir.py                               ~ exit code 3 for bad input, whole tick sequence checked
├── validate_ports.py                            ~ reads exit code 2 only with a verdict
├── bench_matrix.py                              ~ team_assembly skipped by default
└── README.md                                    ~

docs/
├── authoring/network-models.md                  + the fifth trait
├── guide/first-model/virus-network.md           + network tutorial
├── authoring/                                   ~ index, parameters (Actions), statistics, views,
│                                                  determinism, registering, the four other trait pages,
│                                                  porting, performance, shaders
├── reference/                                   ~ models (ten), primitives (next_index), cli
├── developing/                                  ~ architecture, cpu-backend, gpu-backend
├── guide/                                       ~ app, first-model/game-of-life, ants, gpu-game-of-life
│                                                  and gpu-ants
├── index.md, benchmarks.md                      ~ counts, which models are benchmarked
├── assets/app/model-select.png                  ~ retaken with all ten models
└── developing/agent-record/…-28-…md             + this record
```

### Phases 1 and 2: actions on every trait

An action is a small setup step the user asks for mid-run.
NetLogo writes one as a button and GAMA as a `user_command`.
The plan first had actions on the network trait only, and the maintainer asked for them on every existing model and for a shape the GPU traits could take too.

`henad-core/src/action.rs` holds `ActionDescriptor { id, label }` and the `actions!` macro.
`actions!` declares the list and an index const per entry.
`GridModel`, `AgentModel` and later `NetworkModel` take `ACTIONS` plus an `act` hook with the same arguments as `init`.
The GPU traits take `GpuGridAction` and `GpuAgentAction`, each a one-off compute pass, with `PassId::Action` and a `seed` in `PassCtx`.
Each press draws from its own stream (`action_seed`), so a press never moves the tick RNG and a run with the same presses at the same ticks reproduces.
`henad-cli --act ID@TICK` fires an action when the state reaches that tick, and a GPU run splits its batches around it.

Every shipped model got one: Randomise and Clear for Game of Life, Seed outbreak for SIR, Randomise headings for boids, and Reset colony for ants, on both backends.
Ants started as "Clear pheromone".
That action left the colony unable to rebuild a trail: a deposit is the neighbourhood maximum lifted by a reward that only a site grants, so a cleared field stayed at exactly zero for 200 ticks.
Reset colony clears both fields and re-runs `init`, and a run recovered to the untouched run's level.

### Phases 3 and 4: the graph, the trait and the engine

`henad-core/src/network.rs` holds `Network`: stable node slots with LIFO reuse, an edge list with a colour byte per edge, and in and out rows in compressed sparse row form with slack.
An insert writes into a row's slack or moves the row to the end with double capacity, and a removal is a swap inside the row.
The rows are always current, and there is no staleness contract.
Rows carry each entry's edge index beside its neighbour, since `retire` has to find a node's edges in the list.

The plan had a parallel counting-sort rebuild for the repack, and it was never written.
Release build on an M4 Pro under a load average of about 4, at 10 million nodes and average degree 6: the sequential rebuild from the edge list took 1.82 s, and a compaction `repack` that copies each row in order took 0.44 s.
With `repack`, building a random Virus graph went from 5.9 s to 4.5 s, and its heap from 1,569 MB to 1,106 MB.
`repack` replaced it, and a full `rebuild` is left for a flip of directedness.

`NetworkModel` is a fifth trait rather than a graph bolted onto `AgentModel`: the agent index is rebuilt from positions, the deposit pass has no RNG, and the population is fixed.
Bolting spawn and retire on would have touched every agent model for two callers.
The trait has a sequential global pass, the one place the graph changes, and a parallel node pass that reads the graph.
`Nodes` bundles lanes, graph and the model's `Aux`, so the node count changes in one place.
A retired node's position is NaN, the renderer, the density heatmap and the export read NaN as absent, and the layout skips a retired slot.
`stats` takes the aux immutably.
Changing `SimState::stats` to `&mut self` would have touched 26 call sites.
A derived walk runs in `prepare_view` instead and caches its answer in the aux.

`henad-compute/src/cpu/network_engine.rs` runs the tick as global pass, node pass, swap, and a repack when stale entries pass half the row storage.
`cpu/layout.rs` is NetLogo's `layout-spring` with repulsion cut off at a radius through the spatial hash, every constant in units of the mean spacing so the walk stays linear at any density.
`cpu/primitives/components.rs` labels connected components by min-label propagation with pointer jumping, without atomics.

The layout runs at publish cadence under a time budget, outside `step()`.
Positions are not a function of the tick, and a CLI run never lays out.
`Snapshot` gained a `serial` publish counter for the viewport to key its uploads on, and a `view_ms` the Performance tab shows as "Prepare view".

### Phase 5: Virus on a Network

`virus_network/` is split into `lanes.rs`, `mod.rs`, `step.rs` (the pull-form kernel) and `wiring.rs` (the generators and the rewire).
The generator is uniform G(n, m) by default, with a random geometric graph as the second choice.
NetLogo's own generator links each node to its nearest unlinked neighbour.
`directed` is a live parameter on the one entry, and a flip rebuilds the rows.
"Rewire a link" removes a random edge and adds a random non-edge, and "Keep Rewiring" does the same every tick, after NetLogo's Diffusion on a Directed Network.

Percent parameters became fractions in `0..=1` shown as percentages (`ParamFormat::Percent`), after the maintainer asked for the sign in the slider rather than the label.
The maintainer then caught that NetLogo's 10% slider cap had come across as Henad's.

Release CLI, flat-out `step()`, machine under a load average of about 4: 877 ticks a second at a million nodes and 75 at ten million, at 2.9 GB resident.

### Phase 6: drawing edges

`henad-app/src/ui/edges.wgsl` draws instanced lines whose vertex shader reads the node positions from the agent layer's buffers.
`edge_layer.rs` owns the pipeline and copies the edge list to the GPU only when its version or length changes.
Without `DownlevelFlags::VERTEX_STORAGE` the pipeline is not built and the System tab says so.
The maintainer asked for an Edges checkbox and for solid arrowheads on directed edges.
With Arrows off, a directed edge still fades towards its source.

Two layout decisions came from watching it.
A layout that kept relaxing while paused made a random graph pulse, so it now moves only on a publish that follows a tick, with an explicit "Layout while paused" checkbox.
Rewiring folded a geometric network into a blob.
A few long shortcuts pulled with linear springs outweighed the mesh.
The pull now saturates, `spring * S * tanh((d - rest) / S)`, with ForceAtlas2-style damping.
On the real Virus graphs at 10,000 nodes over 3,000 iterations, the spread of a geometric graph with 1% of its edges rewired went from 188 to 375 against a target of 397, and a random graph's move per iteration from 23 units to 0.045.
In a release build under load, an iteration averaged over the test graphs went from 2.5 ms to 1.7 ms.
Untangling a scrambled start got worse, and that was accepted.

A switch-off of the layout never reached the state: `on && state.set_layout(..)` short-circuited.
A screenshot diff after pressing "Rewire a link" while paused showed every node had moved, and a regression test now holds it.

### Phase 7: Team Assembly

`team_assembly/` is split into `lanes.rs`, `mod.rs`, `assembly.rs` (setup and the tick), `ring.rs` (retirement) and `live.rs` (the live-node list).
The rules follow NetLogo's source, read from the Models Library: setup nodes are incumbents, the age check runs before the increment, and the `q` draw happens on every incumbent pick.
Retirement is a bucket queue keyed by the tick of a node's last team, so a tick costs O(team_size² + retirees) rather than a scan.
Idle incumbents are drawn from a dense list of live nodes.
Slots stay at the peak population after a large cohort retires.
Components are labelled in `prepare_view` and cached in the aux by graph version.
`henad-cli --export-stats` now prepares each sample the way a publish does.

An audit of this phase kept 27 findings and a review of the fixes found 15 more.
All were fixed apart from four left open, listed under Issues found (2 to 4).
The fixes widened the tick lanes to `u64` and added the live list, a ring that only grows, collaborator candidates built up over a tick, `aux_heap_bytes`, and `bench_matrix.py` skipping the model by default.
The model's node count is only the starting population.

Indicative only, release CLI, flat-out `step()`, machine under load, and measured before the audit fixes above: the population settles at about 3.1 times `max_downtime` at `max_downtime` 32,000 and 320,000 (about 3.3 at the default of 40), and the model ran 1.3 to 1.6 million ticks a second at 100,000 live nodes and 0.6 to 0.8 million at a million.

### Phase 8: documentation

The docs were drafted by seven agents in parallel, one per group of pages, from a shared brief listing where the plan and the code had parted ways.
Every page then went through a fact pass against the code and a style pass against AGENTS.md and, at the maintainer's request, a list drawn from Wikipedia's "Signs of AI writing".

`docs/authoring/network-models.md` is the reference page for the fifth trait, and `docs/guide/first-model/virus-network.md` is a tutorial that builds Virus on a Network as a model of its own.
Like the other tutorials it writes its code out by hand, and `crates/henad-models/src/tests/tutorial/virus.rs` holds the state a reader reaches at the end.
`parity.rs` steps that twin beside `virus_network` and demands the same bits, and the twin cannot drift from the shipped model.
The page's hand-written blocks are checked against the twin by hand, and the finished file is included whole at the end.

The two fixture docs follow `sir_fixture.md`.
Virus on a Network keeps the library model's rules and swaps its spatially clustered generator for the G(n, m) loop Henad uses, since Henad has no exact counterpart to the nearest-unlinked-neighbour rule.
Team Assembly is used unchanged, with `layout?` off.
Its statistics are means over ticks 201 to 1000.
A single tick is too noisy to compare: the giant component share at tick 1000 varies by 40% between runs, where its mean over the window varies by about 6%.

`scripts/compare_network.py` is a sibling of `compare_sir.py` rather than a generalisation of it.
`validate_ports.py` drives `compare_sir.py`, and there are no network ports yet.
It imports the interval and verdict code from there and holds one statistics table per model.

The margins were first set at about four times the 95% half-width, as SIR's are, with 50 replicates a side.
A Henad run with `recovery_chance` at 4.5% instead of 5% read only inconclusive against them, and they were tightened to about three.
The audit below found that this fitted the margins to one draw of seeds: two of four seed pairings read different, and two inconclusive.
It also built two wrong NetLogo engines from the procedure, one letting a node infected earlier in the tick spread in the same tick and one running the recovery checks before the spread.
Both read equivalent against Henad at 200 and 400 replicates under those margins.
At 50 the second read inconclusive on the peak, and the extra replicates an inconclusive verdict asks for turned it equivalent.

The Virus fixture now takes 400 replicates a side, with margins at three times the half-width that 400 Henad runs give: ±0.0075 on the peak, ±2.5 ticks on its tick and ±0.0053 on the final resistant fraction.
Under them the first wrong engine reads different on the tick of the peak (+3.42 ± 0.87) and the second on the peak (-0.01573 ± 0.00249), on both Henad seed ranges tried.
Headless NetLogo ran 400 replicates of a variant in about a minute and a half.
Team Assembly keeps 50, since a `max_downtime` of 41 against 40 reads different on the link count on every seed range tried.
Each document keeps its Henad-against-Henad check in a Checking the margins subsection under Margins.

The fact pass found NetLogo installed on the machine and ran both appended procedures headless, two replicates each.
They compile, write the documented columns from tick 0 to tick 1000, and read into `compare_network.py`.
The audit also ran the correct procedures headless in scratch, 400 Virus and 50 Team Assembly replicates, and both read equivalent on every statistic.
Those runs were not kept.
Procedures that follow the steps as first written would have put their CSVs beside the library model inside the NetLogo install, both models in one folder.
Each now saves its model to a folder of its own, and `compare_network.py` reads only its own model's files.

Checking the pages against the code turned up faults in the code itself, all fixed here.
`henad-cli` fired a GPU action twice when its tick fell on a boundary between two calls to `run_gpu_steps_acting`: the end of the warm-up, or any multiple of `--stats-every`.
A GPU SIR run with `--act seed_outbreak@10` ended with 27 more recovered cells when sampled every 10 ticks than every 7.
An action now fires as the state arrives at its tick, each caller fires tick 0 itself, and the two runs agree.
The `--act` help pointed at `--params` for action ids, which `--params` does not print, and now says the error lists them.
Game of Life's Randomise was documented, in the Rust and in the WGSL, as using the density the slider now reads.
The density is reload-only, so both use the density the model was built with, and the comments say so.
The tutorial's parity tests could not see a rewired edge added in the wrong colour.
The final recolour painted over it.
They now hash the edge list after every tick, and a planted mutant fails them.
Two doc comments were out of date as well: the `cpu/mod.rs` module doc still named two engines, and `recolor` in `virus_network/mod.rs` said most publishes find nothing to change.
During an outbreak that is false.

After the upgrade to Opus 5.5, the maintainer asked for a fresh audit of phase 8.
Seven auditors, one per lens, each followed by an adversarial verifier and then a critic looking for gaps, kept 62 findings, about 45 once duplicates across lenses were merged.
The fixture margins above were the largest.
The earlier passes also missed a fault in the double-fire fix itself.
The GPU benchmark then timed different action ticks from the CPU one, at both edges of the timed window.
`actions::Fire` now names the two firing rules, one per CPU loop, and a pure `Schedule::fire_ticks` decides which ticks a GPU run fires under the rule of the loop it mirrors.
A GPU run now times exactly the ticks the CPU loop times, and waits for the action on the last tick outside the timer.
Two device-free tests and two GPU tests hold this, and each fails when the old loop is put back.
A review of the fixes found a real engine fault.
A spawn into a reused slot that the model has not placed yet sits at `NaN`, and the layout spread that `NaN` along the node's edges to every node it reached.
The layout now skips a node without a finite position, and a test holds it.
The scripts changed as well.
`compare_sir.py` and `compare_network.py` exit 3 on bad input or a failed `henad-cli` run, where they used to exit 1 or 2, and `validate_ports.py` reads exit 2 as inconclusive only when a verdict was printed.
`compare_network.py` reads each model's files from a folder of its own, and refuses stale replicates.
The rest were in the docs: a sentence a phase 8 edit had made false on the GPU ants page, a tutorial import a reader would have added twice, stale text in `AGENTS.md` and `shaders.md` about hand-written binding slots, and wording across the pages.

Last, at the maintainer's request, the NetLogo reference for both fixtures was generated in the NetLogo 7.0.4 app, driven through computer use.
Each library model was copied into a folder of its own and given the documented edits on disk.
The procedures make the same edits in the Code tab.
A diff against the library copy showed nothing else.
The Command Center lines from the procedures then ran with View Updates unchecked, 400 Virus replicates in about two and a half minutes and 50 Team Assembly replicates in about twenty seconds.
Every Virus file reports 1000 nodes and 3000 links, so the app does not clamp a slider value set from code.
Both models read equivalent on every statistic, and each fixture doc's Result section now holds its table.
At 50 seeds, two Team Assembly intervals excluded zero while staying inside their margins, and the KS test rejected two statistics.
Seeds 51 to 2000, from the same procedure with the range widened, left every interval around zero, and no KS test rejected.
Seeds 1 to 500 flagged the newcomer-newcomer share instead, and seeds 501 to 2000 alone put it back around zero.
Neither gap persisted on fresh seeds.
The replicate CSVs are not committed, as SIR's are not.

After the maintainer committed the branch and opened the pull request, CodeRabbit left eleven comments, and the maintainer asked for them to be resolved.
Five agents, one per group of files, checked each against the code, and a reviewer per group checked their verdicts.
Nine are fixed.
The chart and the recording had kept one row per tick by dropping any publish at a tick already recorded.
A press while paused publishes at the same tick, so its effect missed that tick's row, where `henad-cli --export-stats` samples the tick after the action.
The newest row is now replaced instead: `StatsHistory` overwrites its newest entry when a sample repeats its tick, and the recording holds its last row back until the tick moves or it stops.
Team Assembly keyed its component cache on the graph's version alone, and a spawn leaves the version where it is.
The shipped parameters always add an edge in the same tick, but a `team_size` of 1 read stale components, and the key is now the version and the node count.
The Edges and Arrows checkboxes are hidden on a GPU without the edge layer.
The comparison scripts check that the ticks run 0 to `--steps` one row at a time, refuse a non-finite statistic, and exit 3 when `henad-cli` cannot start.
`compare_sir.py` had never read the tick column, and a dropped row before the peak moved its tick of the peak.
The docs now say a tick may read positions to place a new node, and the Virus and ants tutorials say the empty `impl` is filled in below.
`AGENTS.md` no longer names the ignored `results/` and `site/` paths.
Two needed no code change.
No shipped GPU action binds one buffer both read-only and read-write, and wgpu reports such a binding as a validation error at dispatch.
The fault modal shows it.
No caller records two presses of one action into one encoder, and `GpuSimState::encode_action` now states that contract.

## State after

`./check.sh` is green after phase 8, the web build included.
`uv run zensical build` reports no issues, and a script over the built site found every link and anchor on the changed pages resolving.

Each phase ended green before the next began.
The branch adds the network graph's own tests, the layout and component tests, the engine tests, eleven Virus on a Network and fourteen Team Assembly consistency tests, and thread-count and publish-cadence tests for both models.
Every new test in phases 6 and 7 was checked by planting the bug it targets and watching it fail.
Phase 8 adds three parity tests for the tutorial: a run with Keep Rewiring on and Directed switched twice, a directed run with Rewire a link pressed every ten ticks, and one comparing the declared parameters, actions, palettes and constants.

Both models were driven through the live app over the egui inspection port, and again in a smoke test after the audit fixes, in a release build.
Virus on a Network was checked for edges drawn, arrowheads on a directed graph, "Rewire a link" changing one edge while paused (228 pixels changed, no node moved), Keep Rewiring, and the layout frozen with Layout off.
Team Assembly ran about 3 million ticks at unlimited TPS.
The population held between 120 and 130, memory stayed at about 40 KB, and no retired node was drawn in Sprites or Density.
The latest team showed its incumbent in yellow and its newcomers in blue, all four link colours appeared, and the component charts moved.
Paused frames with Layout while paused off were pixel-identical, and with it on the layout ran at the 4 ms budget.
GPU SIR built and rendered after its shaders gained snippet markers.
The smoke test found the model dropdown had outgrown egui's default popup height of 200 points, which hid Ant Foraging (GPU) behind a scroll.
The dropdown is now as tall as the window, and the app tour's screenshot was retaken with all ten models.
Not driven: an image export with edges, which needs the native save dialog, and a GPU without `VERTEX_STORAGE`.

The measurements the plan left open were taken after the smoke test, on an idle machine: release build, Apple M4 Pro, 14 threads.
One layout iteration took 5.8 ms on Virus's 100,000-node random graph and 6.2 ms on the geometric one, and 84 ms and 85 ms at a million nodes.
Component labelling took 2.0 ms and 49.5 ms at 100,000 nodes, and 12 ms and 1.06 s at a million.
On the geometric graph each round of label propagation reaches little further than the last, so it needs far more rounds.
Team Assembly at `max_downtime` 320,000 reached 999,630 live nodes after 1,280,000 ticks in 1.4 s, and labelling its components took 75 ms.
One layout iteration on that state took 322 s at first, falling to 8 s by the sixth, and Issue 3 below explains why.

Both fixtures read equivalent against NetLogo 7.0.4 on every statistic, Virus at 400 replicates a side and Team Assembly at 50.
Henad against Henad reads equivalent on all ten statistics as well.
The planted `max_downtime` of 41 in Team Assembly reads different on the link count.
The first wrong NetLogo engine for Virus reads different on the tick of the peak and inconclusive on the peak, and the second reads different on the peak.

## Issues found & future directions

1. **The Virus comparison covers one graph, and one wrong engine clears its margin only narrowly.**
   The first wrong Virus engine clears the tick-of-peak margin only narrowly, and another draw of seeds could read inconclusive.
   The Virus comparison covers the undirected G(n, m) graph only.
   The `directed` parameter, the geometric generator and rewiring are checked by Henad's own consistency tests and nothing else.
2. **Component labelling is exact and unbounded.**
   Team Assembly labels its components on every publish that follows a graph change, with no time budget.
   At a million live nodes that takes 75 ms per publish, and on a million-node geometric graph 1.06 s.
   A budget would make `--export-stats` depend on the machine.
   Adding one is a design decision rather than a patch.
3. **Team Assembly piles its newcomers onto a few points, and the layout has no bound on one iteration.**
   A team with no incumbent is placed on a fixed polygon at the world's centre, and newcomers joining an incumbent go to fixed offsets around it.
   With no layout running, as in the CLI or in an app whose layout falls behind at unlimited TPS, nodes stack on those points.
   At a million live nodes all of them sat inside a 15 by 17 box, and one hash cell 0.25 units wide held 332,319 of them.
   The layout compares every pair of nodes in reach, and a publish always runs one iteration, so a publish stalled for 322 s.
   At the default `max_downtime` of 40 the population is about 130, and nothing shows.
   Spreading the placement and bounding the layout's work per node are both open, and the choice is the maintainer's.
4. **The giant component share is on the same chart axis as the link counts.**
   It runs from 0 to 1 beside counts in the hundreds, and hiding the link series from the legend is the only way to read it.
5. **The benchmark's `population` is the count after warm-up.**
   For Team Assembly the node count is only the starting population, and `bench_matrix.py` skips the model by default.
   The CLI prints a note when the population moves during the timed steps, and the JSON field is left as it was.
   `--exclude` replaces the default skip list rather than adding to it.
6. **Positions are not a function of the tick.**
   The layout runs at publish cadence.
   A state exported from the app and one exported from the CLI differ in positions and in nothing else.
   Linear springs untangled a scrambled start better than the saturating pull does, and that cost was accepted for a stable picture.
7. **A GPU benchmark with `--warmup 0` times its own set-up.**
   A GPU state's construction queues work without waiting for it, and the first timed wait pays for it.
   The CPU keeps construction out of the timer.
   Draining the queue before the timer would change what the GPU numbers measure, and is left for a decision.
8. **A GPU pass that binds one buffer both read-only and read-write is caught only at dispatch.**
   An in-place action, or a step pass over a buffer that is not double-buffered, resolves both bindings to one buffer.
   No model does this, and the validation error reaches the fault modal, but `capacity.rs` does not check it before the build.
9. **A button press in the app is not recorded.**
   `--act` replays presses in the CLI, but nothing writes the app's presses into an export.
   A GUI session cannot yet be replayed headless.
10. **Left for follow-up issues.**
   Ports of both models to Mesa, MASON, Agents.jl and krABMaga, with their `LADDER` and `GATES` entries, and the sweep.
   Zoom and pan in the viewport, a size per node, and a GPU twin of `NetworkModel`.
   The row layout already has the shape a shader wants.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)

Quite a bit commit.
We'll continue audit the models in the future phases.
