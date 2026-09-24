---
date: 2026-09-24
title: "Virus on a Network on the GPU, parked"
description: A hand-written GPU port of Virus on a Network with its graph, rewiring and stats on the device, measured against the CPU model and parked, since its gain stays well short of the grid and agent ports'.
icon: material/note-text-outline
status: ai-generated
model: claude-opus-5-5 (Claude Code), exploration and plan review as multi-agent workflows on claude-opus-5-5
issue: "#46"
state: phases 1 to 3 of six implemented, plus a packed state copy, `./check.sh` green, then parked on the maintainer's call
baseline_commit: 5de4474
delta_state: phases 1 and 2 committed on `45-network-gpu-model` (7a3ee85, 33bc3ea), phase 3 and the packed state uncommitted
---

# Virus on a Network on the GPU, parked

> Issue #46 asked for network models on the GPU, following the grid and agent tracks, where two models were written by hand before a trait was extracted from them.
> Exploration found Team Assembly has no parallel work per tick, so only Virus on a Network was ported, and the `GpuNetworkModel` trait was deferred.
> The port keeps its edge list, adjacency rows, rewiring and stats on the GPU, starts bit-identical to the CPU at tick 0, and replays bit for bit.
> Measured against the CPU model, it gained about 5 times at a million nodes and lost at ten million.
> The neighbour reads missed the GPU's cache once the state array outgrew it.
> A 2-bit packed copy of the states partly fixed that.
> With packing the port gains about 6 times at a million nodes and 3 times at ten million, well short of the 8 to 18 times the grid and agent ports reach.
> The maintainer parked the work at that point, with phases 4 to 6 (drawing edges, a GPU layout, the docs) not started.

## State before

Master held the CPU `NetworkModel` trait with Virus on a Network and Team Assembly (record `-28`).
The GPU side had `GpuGridModel` and `GpuAgentModel` and nothing for graphs.
`GpuSnapshot` carried a display texture and agent lanes, and no edges.
`next_index` was Rust only, since its redraw needs a 64-bit product.

## What was done

New files marked **+**, modified marked **~**.
The tree covers the whole branch against `master`, committed or not.

```
AGENTS.md                                           ~ next_index is no longer Rust only
zensical.toml                                       ~ nav entry for this record
docs/reference/primitives.md                        ~ index_from_bits, the WGSL next_index, mul_wide
docs/developing/agent-record/20260924-29-…md        + this record

crates/henad-core/src/
├── authoring/primitives/rng.rs                     ~ index_from_bits, next_index built on it
└── metadata.rs                                     ~ Structure::GpuNetwork

crates/henad-compute/
├── build.rs                                        ~ rows shaders
└── src/
    ├── snapshot.rs                                 ~ GpuSnapshot::edges
    └── gpu/
        ├── mod.rs                                  ~ re-exports GpuRows and GpuEdges
        ├── capacity.rs                             ~ storage_in_layout, Demand::push_rows
        ├── agent_engine.rs                         ~ read_buffer through read_words
        ├── grid_engine.rs, sim_thread.rs           ~ the edges field
        ├── primitives/
        │   ├── rows.rs                             + GpuRows, a counting sort from an edge list
        │   ├── rows_count.wgsl, rows_scatter.wgsl  +
        │   ├── pipeline.rs                         ~ bind_group
        │   ├── readback.rs                         ~ read_words
        │   └── spatial_hash.rs, mod.rs             ~
        ├── shared/
        │   ├── graph.wgsl                          + Edge
        │   ├── rng.wgsl                            ~ mul_wide, index_from_bits, next_index
        │   └── parity.wgsl                         ~ two ops
        ├── tests/parity.rs                         ~ the index cases
        └── view/edges.rs, view/mod.rs              + GpuEdges

crates/henad-models/
├── build.rs                                        ~ six shaders
├── src/
│   ├── gpu_virus_network/                          + mod.rs, node_state.wgsl, step.wgsl, pack.wgsl,
│   │                                                 rewire.wgsl, recolor_nodes.wgsl, recolor_edges.wgsl
│   ├── virus_network/mod.rs, wiring.rs             ~ keep_rewiring and REWIRE_TRIES visible to the port
│   ├── registry.rs                                 ~ register_gpu_virus_network, the registry tests
│   ├── lib.rs                                      ~ module
│   └── tests/gpu_virus_network.rs, mod.rs          + 22 GPU tests
└── tests/consistency_virus_network.rs              ~ a_node_infected_this_tick_can_recover_this_tick

crates/henad-app/src/ui/
├── model.rs                                        ~ rows for Structure::GpuNetwork, the edge palette
└── export/image.rs                                 ~ a pattern with the new field
```

### Scope, from exploration and planning

Six readers mapped the GPU engines, the GPU primitives, both network models, the app and CLI paths, the porting history and prior art, and a critic checked their claims against the code.
A Team Assembly tick assembles one team of 3 to 8 nodes in about a microsecond on the CPU, and has nothing to run in parallel.
A GPU step costs at least several microseconds.
By the exploration's estimate a port would run 15 to 200 times slower, and it would bend any trait towards sequential single-thread passes.
The maintainer chose to port Virus alone and defer the trait, with NetLogo's Diffusion on a Directed Network named as the candidate second model.
The graph lives on the GPU, every parameter is reload-only as on every GPU model, and a four-lens review of the plan against the code shaped the details below.

### Phase 1: GPU graph primitives

`GpuRows` builds adjacency rows from an `array<Edge>` with the count, scan and scatter chain `GpuSpatialHash` uses, keyed by endpoint.
Node `i` owns an in-row and an out-row in one table of `2n + 1` offsets, and an undirected graph files every neighbour in the in-row, as `Network` does.
Order within a row is whatever the atomics resolve in, and every consumer here is order-independent.

`index_from_bits(bits, n)` is one step of Lemire's bounded draw, returning `None` for a word that must be redrawn.
`next_index` now loops over it, and a test holds its output to the old loop's on eleven ranges.
The WGSL twin builds the 64-bit product from 16-bit halves in `mul_wide`, and the parity driver holds both to Rust up to `u32::MAX`.
A `random_float` scaled by `n` resolves only 2²⁴ values, and at ten million nodes it favours some indices twice as often as others.

`read_words` replaced two copies of a blocking buffer readback.

### Phase 2: the model on a static graph

`GpuVirusNetwork` implements `SimState` and `GpuSimState` by hand, as `gpu_boids` did before its trait.
It seeds through the CPU `NetworkModelState`, uploads the lanes and the edge list, and builds the rows on the GPU.
Tick 0 is then bit-identical to the CPU in states, timers, positions, the edge list, the rows as multisets, the colours and the counts.

A susceptible node multiplies up its chance of escaping each infected in-neighbour and takes one draw against the product.
The outcome has the same distribution as the CPU's draw per neighbour.
Infection, recovery and resistance each take their own word from the node's stream.
With one word shared, a node could never recover in the tick it caught the virus, the second wrong engine in `virus_network_fixture.md`, and none of the existing tests would have noticed.
The CPU suite gained `a_node_infected_this_tick_can_recover_this_tick` as well.

At each publish, one pass paints the nodes and counts S, I and R into cleared `u32` counters, and a second greys every edge touching a resistant node.
The registry entry is bespoke, with a new `Structure::GpuNetwork`, and every registry contract runs on it, the baseline-device build included.
`GpuSnapshot` gained an `edges` field, and the GPU topology test now checks it.

### Phase 3: rewiring on the GPU

A single-invocation pass mirrors `wiring::rewire`.
It draws pairs with the bounded draw up to `REWIRE_TRIES` times, checks `joined` against the in-rows in both directions on a directed graph, and leaves the edge list in the order `remove_edge` and `add_edge` leave it on the CPU.
Keep Rewiring runs it before each tick's node pass, and "Rewire a link" runs it once, each followed by a full rows rebuild.
The tick and the action each draw from an RNG word of their own in a storage buffer, advanced by the shader.
Two presses recorded into one encoder therefore draw twice.

The first measurement of this phase showed the rebuild dominating.

| Nodes | Keep Rewiring off | Keep Rewiring on |
|---|---|---|
| 1M | ~3,400 ticks/s | ~250 ticks/s |
| 10M | ~80 ticks/s | ~8.7 ticks/s |

The CPU pays O(degree) for a rewire and runs at about the same rate either way.
The maintainer chose in-place row edits, and a design took shape: rows built with slack, a length per row, a rewire editing its four rows in place, and a full rebuild that runs only when a row overflows.
An overflow flag would switch the rebuild on through indirect dispatches, with an indirect mode on `PrefixScan`, and the host would never wait.
It was drafted and never wired in. The measurements below stopped the work first.

### Re-evaluation

The maintainer asked for a check that the port was still worth it before going further.
An interleaved comparison with Keep Rewiring off gave the first table below.
The GPU's cost per node tripled between one and four million nodes while the CPU's stayed flat, and the GPU fell behind at ten million.

The hypothesis was that the random neighbour reads miss the GPU's cache once the `u32` state array passes a few megabytes, where the CPU's `u8` lane stays resident.
A throwaway edit confined the reads to a smaller array at ten million nodes, keeping their number:

| Memory the reads touch | GPU ticks/s at 10M |
|---|---|
| 40 MB, every `u32` state | 60 |
| 10 MB | 145 |
| 2.5 MB | 250 |

The node pass now reads every state, its own and its neighbours', from a 2-bit packed copy, 2.5 MB at ten million nodes.
A pack pass rebuilds the copy after each node pass.
The `u32` state became a single buffer and the ping-pong went away.
The second table is the result.

| Nodes | CPU | GPU | GPU ÷ CPU | CPU, second run | GPU packed | packed ÷ CPU |
|---|---|---|---|---|---|---|
| 10k | 29,879 | 37,354 | 1.25 | 27,378 | 20,116 | 0.73 |
| 100k | 6,387 | 23,372 | 3.7 | 6,270 | 18,429 | 2.9 |
| 1M | 699 | 3,581 | 5.1 | 670 | 3,978 | 5.9 |
| 4M | 231 | 281 | 1.2 | 230 | 625 | 2.7 |
| 10M | 81 | 59 | 0.73 | 80 | 243 | 3.0 |

Every number here is indicative.
Release `henad-cli`, a flat-out `step()` counter, ticks per second averaged over two reps, default parameters (an undirected G(n, m) graph of mean degree 6), on an Apple M4 Pro under a load average of 2.5 to 4.
The packed port was timed in a second run, interleaved with a CPU run of its own, and each ratio is against the CPU run beside it.
The GPU runs 64 steps per submission.

The grid and agent ports gained about 8 to 18 times on the same machine.
A Virus tick is about six random reads per node with almost no arithmetic.
On the M4 Pro the CPU and GPU share one memory system, and the GPU has little bandwidth to spare over the CPU.
At the small end the extra pack pass costs more than it saves.
The maintainer parked #46 at this point.

## State after

`./check.sh` is green with `HENAD_REQUIRE_GPU=1`, 439 tests passing.
The branch `45-network-gpu-model` holds phases 1 and 2 as commits, with phase 3 and the packed state uncommitted on top, and is not to be merged as it stands.

Twenty-two GPU tests hold the port.
They cover tick-0 identity for both generators in both directions, replay, independence from batching and publish cadence with and without rewiring, the rate extremes, the closed-form rates the CPU suite checks, an exact one-step infection check against the edge list, the recolour, the packed copy, the rewire's edge count, simplicity and order, the rows after rewires, the separate RNG streams, and graphs with one node or no edges.
Every new test was checked by planting the fault it targets and watching it fail.
Two plants passed at first, an edge written into the wrong slot and a stream that never advanced, and the tests for edge order and stream advance came from them.

The app builds and runs GPU Virus: nodes draw and change colour, the stats and charts move, and a GPU step took about 12 µs at 10,000 nodes.
Edges are not drawn. Drawing them was phase 4.
The CLI benchmarks and exports it through the generic GPU path.

## Issues found & future directions

1. **A node pass of sparse gathers gains little on the M4 Pro.**
   The packed port reaches about 6 times the CPU near a million nodes and about 3 times from four million up.
   Each tick is bound by random neighbour reads and by sequential traffic in the timer, RNG words, rows and entries, and both backends share one memory system.
   Trimming more is possible: randomness from a per-step counter in place of the RNG word per node, the timer as a fixed phase, and the pack folded into the node pass.
   An estimate, not measured, puts ten million nodes near 4 times after that.
2. **The RTX 4090 is unmeasured.**
   A discrete card's bandwidth over a desktop CPU's is far larger than the M4 Pro GPU's over its own CPU, and a gather-bound kernel might gain much more there.
   The comparison to run on it:

   ```bash
   for n in 10000 100000 1000000 4000000 10000000; do for m in virus_network gpu_virus_network; do ./target/release/henad-cli $m --set num_agents=$n --steps 256 --warmup 32 --global-warmup 32 --reps 3; done; done
   ```

3. **The paper compares a GPU model with FLAME GPU, not with Henad's CPU backend.**
   FLAME GPU's graph is static, and a host-side edit costs two full radix sorts and a sync with the host.
   A GPU network with rewiring on the device could beat it on engine grounds whatever its gain over Henad's CPU backend.
   That alone could justify revisiting #46.
4. **Keep Rewiring rebuilds every row after every rewire.**
   The in-place design under Phase 3 is the fix, and none of it is in the repository.
   The overflow rate depends on the slack.
   At two slots per row, an estimate puts a ten-million-node graph at a rebuild about every thousand ticks, and a thousand-node one every twenty or so.
5. **Phases 4 to 6 were not started.**
   Edges are not drawn for a GPU snapshot, there is no GPU layout, and the docs pages and `AGENTS.md` still describe network models as CPU only, apart from the bounded draw.
6. **The phase 1 pieces have no caller outside this branch.**
   `GpuRows`, `GpuEdges` and the WGSL `next_index` serve only `gpu_virus_network`.
   `index_from_bits` and the rebased `next_index` stand on their own, and the CPU output is unchanged.
7. **Found during exploration and left alone:**
   - The reduce leaf writes past the end of `partials` once folded groups pass 65,535 × 256 elements (`gpu/primitives/reduce.rs`). It is live in `gpu_ants` at a 4096² world.
   - The GPU grid ports reimplement their seeding, while `AGENTS.md`, `porting.md` and `models.md` say they call the CPU `init`.
   - `gpu-agent-models.md` says `index_cell_size` is read every tick, and `registry.rs` has a stale note that nothing else reads `topology_hint`.
8. **Team Assembly stays a CPU model.**
   The exploration's reasoning is under Scope, and a GPU trait built with it as the second example would carry growable slots and sequential passes.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)

<!-- Seeded by the agent: what the human did this session, from the agent's point of view.
     Raw material to reframe, not notes. Delete this block once rewritten.

     - Opened #46 as a plan-only session and asked for the two-models-then-trait workstyle, inviting an alternative if it was better.
     - Chose to port Virus alone and defer the trait, over the agent's recommendation of adding Diffusion on a Directed Network as a second model.
     - Chose a GPU spring layout as the last phase, reload-only params, a GPU-resident graph, NetLogo as the gate with a CPU-against-GPU diagnostic allowed, and to review diffs rather than write any kernel by hand.
     - Corrected the comment style mid-plan: network.rs format but fewer comments, no sentence opening with "what", "how" or "why", and concrete identifiers such as `dest` for `to`. Asked for the rule to be researched against guidance on excessive docstrings.
     - Created the branch `45-network-gpu-model` from master once #45 merged, and committed phases 1 and 2 after reviewing each.
     - Chose in-place row edits when the full rebuild made Keep Rewiring slower on the GPU than on the CPU.
     - Stopped the in-place work to ask whether GPU network models were still worth it, given gains below the grid and agent ports'.
     - Chose to pack the state and decide, then parked #46 on the packed numbers.
-->
