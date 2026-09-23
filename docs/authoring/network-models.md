---
title: Network models
description: Writing a population of nodes joined by edges with the NetworkModel trait, its graph and its two passes.
icon: material/graph-outline
---

# Network models

_See [Writing a CPU network model](../guide/first-model/virus-network.md) for a tutorial._

`NetworkModel` is the authoring trait for a population of nodes joined by edges.
Pick it when your individuals interact along edges that last from one tick to the next.
You declare the node lanes with `agent_lanes!`, then implement `init`, the passes your model needs and `stats`.

An agent model that reads its neighbours finds them by distance, through a spatial hash rebuilt from the positions every tick.
A network model reads them from its graph, and the graph changes only when the model changes it.
Nodes still have positions, but those belong to a layout the engine runs for drawing.
Nothing a tick decides should depend on them.
A network is also the one topology whose population can change during a run, as the model spawns and retires nodes.

`virus_network/` and `team_assembly/` are the two implementations to read alongside this page.
Virus on a Network leaves its edges where they are unless asked to rewire them.
It does its work in the node pass, where each susceptible node reads the state of its neighbours.
Team Assembly has no node pass at all.
Its global pass assembles one team a tick, joins every pair of members with an edge and retires the nodes that have been idle too long.
The graph grows and shrinks as it runs.

```text
virus_network/
  lanes.rs     the agent_lanes! declaration
  mod.rs       metadata, params, actions, prepare_view, stats
  step.rs      the node pass
  wiring.rs    the initial edges and the rewire
team_assembly/
  lanes.rs     the agent_lanes! declaration
  mod.rs       metadata, params, prepare_view, stats
  assembly.rs  the global pass and the initial teams
  live.rs      the live nodes in a dense list
  ring.rs      the live nodes filed by the tick they last joined a team
```

## Items you supply

Everything in the table below comes out of your impl.

| Item | Role |
|---|---|
| `NAME`, `ID`, `DESCRIPTION` | Identity, read by the app and by `henad-cli --list` |
| `PALETTE` | One RGBA colour per node colour index. See [palettes and views](views.md) |
| `EDGE_PALETTE` | One RGBA colour per edge colour byte. See [colouring edges](views.md#colouring-edges) |
| `STATS` | The series the history chart plots. See [statistics](statistics.md) |
| `ACTIONS` | One-off changes to the state, one button each, empty by default. See [actions](parameters.md#actions) |
| `CHUNK` | Nodes per chunk of the node pass, 512 by default |
| `DEFAULT_NODES` | The default node count |
| `MAX_NODES` | The largest node count the parameter accepts, 10,000,000 by default |
| `DEFAULT_EXTENT` | The default world size, for the layout to spread the nodes over |
| `LAYOUT` | The spring constants, `SpringParams::DEFAULT` by default |
| `type Lanes` | The node lanes, declared with `agent_lanes!` |
| `type Params` | Hot parameters, rebuilt once a tick |
| `type Aux` | Model state kept outside the lanes and the graph, or `()`. The engine creates it with `Default` before `init` |
| `param_descriptors` | This model's own parameters. See [parameters](parameters.md) |
| `from_params` | Extracts `Params` from the model's own values and the extent |
| `directed` | Whether the edges are directed, read every tick. `false` by default |
| `init` | Seeds the lanes and the edges |
| `run_global_pass` | The sequential pass, and the one that can change the graph |
| `run_node_pass` | The parallel pass over every slot |
| `act` | Runs one entry of `ACTIONS` |
| `prepare_view` | Readies the state for drawing, before each snapshot |
| `stats` | The reduction, in `STATS` order |
| `aux_heap_bytes` | Heap memory held by `Aux`, 0 by default |

Of the functions, only `param_descriptors`, `from_params`, `init` and `stats` lack a default.

## Lanes

```rust
--8<-- "crates/henad-models/src/virus_network/lanes.rs:lanes"
```

Node lanes are declared exactly as [agent lanes](agent-models.md#lanes) are, with the same `dual` and `plain` kinds and the same generated views.
Each lane holds one entry per slot, retired slots included.
Lanes named `pos_x` and `pos_y` are required.
The point view draws them and the layout moves them.
The `color = <lane>` entry names the lane the renderer reads for indices into `PALETTE`.

Virus on a Network declares `state` as `dual`, because each node reads its neighbours' state while it writes its own.
Team Assembly declares every lane `plain`.
Its node pass is empty and its global pass runs on one thread.
No lane needs a second buffer.

```rust
--8<-- "crates/henad-models/src/team_assembly/lanes.rs:lanes"
```

## The graph

The graph is a `Network`, from `henad_core::network`.
It keeps an edge list for drawing, and a row of neighbours per node for the kernels to walk.

### Slots

Every node lives in a slot, and its index is the slot's index for as long as it lives.
`slot_count` counts every slot, `node_count` counts the live nodes, and `contains_node(i)` returns whether slot `i` holds one.

Spawn and retire nodes through `Nodes::spawn` and `Nodes::retire`.
`Nodes::spawn` grows every lane to fit, and `Nodes::retire` also sets the node's position to `NaN`.
The graph has its own `spawn` and `retire`, but those leave the lanes untouched.

`retire` removes every edge touching the node and frees its slot.
`spawn` takes the most recently freed slot first, and appends a new one only when no slot is free.
Slots are never compacted, and [retired nodes](#retired-nodes) covers what that means for the lanes.

### Edges

`add_edge(a, b, color)` appends an edge from `a` to `b` and returns its index in the edge list.
The colour byte indexes `EDGE_PALETTE`.
Repeated edges are allowed, and a debug build panics on a self loop.
Virus on a Network's random graph generator and its rewire check both directions for an existing edge before adding one, and its graph stays simple.

`edges()` returns the list as three slices, `(src, dst, color)`, and `edge_count()` returns its length.
`remove_edge(e)` swaps the last edge into index `e`.
Note that an edge index holds only until the next removal, and retiring a node removes edges too.

`edge_between(a, b)` returns the index of an edge from `a` to `b` if there is one, and `has_edge(a, b)` returns whether there is.
On an undirected graph an edge either way counts, and the shorter of the two rows is searched.
On a directed graph the search covers the out-row of `a`.
Team Assembly calls `edge_between` for each pair in a new team, and recolours the edge a pair already has instead of adding a second one.

### Rows

Each node's row lists its neighbours, and the rows are stored in compressed sparse row (CSR) format.
`in_neighbors(i)` returns the nodes with an edge to `i`, and `out_neighbors(i)` returns the nodes `i` has an edge to.
`degree(i)` is the length of the row, and on a directed graph it adds the two rows together.
On an undirected graph one row answers both calls, and each edge appears in the rows of both its ends.

A row keeps some slack.
A row with no room left moves to the end of the storage with double the capacity, and its old space goes stale.
The engine repacks every row once after `init`, and again after any tick that leaves more than half the storage stale.
A repack keeps the order within each row.
That order follows from the sequence of edits and nothing else, and a kernel must not read meaning into it.
See [determinism and testing](determinism.md#network-models).

### Directed and undirected

`directed(params)` is read at the start of every tick.
A live parameter can switch it, as Virus on a Network's Directed parameter does.
The edge list keeps each edge's `src` and `dst` either way.
Switching the direction changes only how the rows read those edges, and rebuilds every row from the edge list.

### The version

`version()` counts changes to the edges, and a snapshot copies the edge list only when the version or the edge count has changed.
[Colouring edges](views.md#colouring-edges) lists the calls that move it, and walks through Virus on a Network's recolour.

## What the hooks receive

```rust
--8<-- "crates/henad-core/src/authoring/model/network_model.rs:nodes"
```

`init`, `run_global_pass`, `act` and `prepare_view` each receive a `Nodes`.
It lends out the lanes, the graph and the `Aux` mutably, and adds `spawn` and `retire` on top.

```rust
--8<-- "crates/henad-core/src/authoring/model/network_model.rs:node_ctx"
```

The node pass receives a `NodeCtx` instead.
The graph is a shared reference there, and every worker reads it at once.
`NodeCtx` carries no `Aux`, and the node pass cannot reach the model's own state.

## The tick

```text
1  from_params       extract the hot parameters
2  directed          rebuild the rows if it changed
3  run_global_pass   sequential, and free to change the graph
4  run_node_pass     parallel over every slot
5  swap              the dual lanes
6  repack            the rows, when the stale space has grown too large
```

Both passes receive the tick count from before the step, 0 on the first tick.
`prepare_view` runs before each snapshot, including the one published before the first tick, and receives the number of ticks completed.
Team Assembly stamps `tick + 1` into its lanes during the global pass, and a stamp then equals the count `prepare_view` sees after that tick.

### The global pass

```rust
fn run_global_pass(nodes: &mut Nodes<'_, Self>, params: &Self::Params, extent: Extent, rng: &mut u64, tick: u64);
```

The global pass runs on one thread before the node pass.
Within a tick it is the only code that can change the graph, and every spawn, retirement and edge edit a tick makes belongs here.
`rng` is a single stream that carries over from tick to tick.

```rust
--8<-- "crates/henad-models/src/virus_network/mod.rs:global_pass"
```

Virus on a Network rewires one edge a tick here while Keep Rewiring is on.
Team Assembly's whole tick happens in this pass.
It picks each member of one team, joins every pair of them with an edge and retires the nodes idle for more than `max_downtime` ticks.

??? example "Team Assembly's global pass"

    ```rust
    --8<-- "crates/henad-models/src/team_assembly/assembly.rs:assemble"
    ```

### The node pass

```rust
fn run_node_pass(lanes: &mut Self::Lanes, ctx: &NodeCtx<'_, Self>, seed: u64, tick: u64);
```

The node pass runs over every slot in parallel.
It is usually one call to the generated `lanes.run_pass`, as an agent model's step pass is.

```rust
--8<-- "crates/henad-models/src/virus_network/step.rs:node_pass"

--8<-- "crates/henad-models/src/virus_network/step.rs:step_node"
```

`step_node` walks `graph.in_neighbors(i)` when node `i` is susceptible, reads the neighbours' current `state` through `read`, and writes node `i`'s next state through `out`.
The two indices work as they do for an [agent model](agent-models.md#the-step-pass), with `i` for reading and `k` for writing.

Retired slots are in the pass too.
A retired node's rows are empty, but its lanes are still there.
A kernel that writes them can check `ctx.graph.contains_node(i as u32)` first.

### CHUNK

`const CHUNK: usize` sets how many nodes each chunk of the node pass covers, and each chunk draws from an RNG stream seeded from its index.
It must be a fixed const, for the reason given under [agent models](agent-models.md#chunk).
The default is 512, and neither shipped model overrides it.
Chunks cover slots, and retired slots count towards them.

## Retired nodes

`Nodes::retire` sets the node's position to `NaN`, and [retired nodes](views.md#retired-nodes) covers how drawing, the layout and the state export treat the slot.

A slot's lane entries outlive its node.
The Population row of the Performance tab counts the live nodes, while the lanes and the node pass still cover every slot.
A spawn that reuses a slot keeps whatever the retired node left in its lanes, `NaN` position included.
A slot appended at the end starts at each lane's initial value instead, the declared one for a `plain` lane and the type's default for a `dual` one.
The model has to write every lane the new node reads.
Team Assembly sets `spawn_tick` and `team_tick` on each newcomer, and places it near the first incumbent in its team.

After a large cohort retires, most of the slots can be empty.
Team Assembly keeps a dense list of its live nodes in its `Aux` and draws from that list.
A draw from it costs nothing per empty slot.
Its retirement queue lives there too, and finds the nodes due to retire without a scan.

## Views and statistics

```rust
fn prepare_view(nodes: &mut Nodes<'_, Self>, tick: u64);
```

`prepare_view` runs before each snapshot, with the lanes, the graph and the `Aux` all writable.
Virus on a Network greys every edge with a resistant end here, as [colouring edges](views.md#colouring-edges) shows.
Team Assembly colours the members of the latest team here.
The hook runs as often as snapshots go out, and a tick must never read anything the hook writes.

```rust
fn stats(lanes: &Self::Lanes, graph: &Network, aux: &Self::Aux) -> Vec<StatValue>;
```

`stats` borrows the `Aux` immutably, and a stat that needs a walk of the graph is computed in `prepare_view` and kept in the `Aux` for `stats` to read.
[Statistics](statistics.md#network-models) works through Team Assembly's connected components, and covers what `stats` returns before the first labelling.

`aux_heap_bytes` reports the heap memory the `Aux` holds, and defaults to 0.
The engine adds it to the state's heap count, and the Performance tab shows that count as Sim memory.

## Layout

Node positions belong to the engine once the state is built.
A model places nodes in `init` and when it spawns one, and a spring layout moves them from then on.

The layout is NetLogo's `layout-spring` with three changes, and lives in `henad_compute::cpu::layout`.
The pull along an edge levels off as the edge stretches.
It is `spring * S * tanh((d - rest) / S)` with `S` the saturation length, divided by the mean degree of the two ends as in NetLogo.
Repulsion stops at the cutoff radius, and the nodes within it are found through a spatial hash.
Each node slows down while its force keeps swinging, as in ForceAtlas2.
The layout keeps every node inside the world and does not wrap at its edges.

```rust
--8<-- "crates/henad-core/src/authoring/model/network_model.rs:spring_params"
```

Every constant is in units of the mean spacing between nodes, `sqrt(width * height / nodes)`.
A model overrides `LAYOUT` to change them.
Team Assembly sets `length` to 0, `spring` to 0.18 and `saturation` to 5.

The layout runs when a snapshot is published.
`step` never calls it.
Each publish runs iterations until the Layout budget is spent, and always at least one.
It moves nodes on a publish that follows a tick, and on every publish while paused when Layout while paused is on.
The Pacing tab holds the Layout and Layout while paused checkboxes and the Layout budget slider.
The app starts with the layout on and a budget of 4 ms.
See the [app tour](../guide/app.md#pacing-tab).
`henad-cli` never runs the layout.

Positions are therefore not a function of the tick, and a model must keep them out of anything a tick decides.
Team Assembly reads a position in its global pass to place newcomers near their team, and that only changes the picture.

## Actions

```rust
--8<-- "crates/henad-models/src/virus_network/mod.rs:actions"
```

```rust
fn act(action: usize, nodes: &mut Nodes<'_, Self>, extent: Extent, params: &[ParamValue], rng: &mut u64);
```

`act` runs one entry of `ACTIONS` between two ticks.
It receives the same `Nodes` as the global pass, and can change the graph.
`params` is the model's own slice of raw values, as `init` receives it, and `rng` is a stream of its own.
Virus on a Network's Rewire a link action moves one edge through the same `wiring::rewire` that Keep Rewiring calls every tick.
[Actions](parameters.md#actions) covers the declaration, the buttons and `--act`.

## Prepended parameters

The engine prepends the node count, the world width and the world height at indices 0, 1 and 2, from `DEFAULT_NODES`, `MAX_NODES` and `DEFAULT_EXTENT`.
All three are reload-only.
The model's own parameters follow, and `init`, `from_params` and `act` each receive that own slice.

The node count keeps the id `num_agents`, and the app labels it Number of Nodes.
The benchmark scripts look the count up by that id, and `scripts/bench_matrix.py` sweeps a network model over node counts as it sweeps an agent model over agent counts.
Team Assembly reads the count as its initial population only, and its population afterwards depends on its own parameters.
`bench_matrix.py` leaves Team Assembly out unless `--models` names it.

Team Assembly's own parameters follow the three.
`max_downtime` sets how many ticks a node can stay idle before it retires.

```rust
--8<-- "crates/henad-models/src/team_assembly/mod.rs:params"
```

## Testing

Both models carry `results_do_not_depend_on_the_thread_count` and `results_do_not_depend_on_the_publish_cadence`.
The first runs at 1 and at 7 threads and compares the two runs bit for bit.
The second runs the model twice, calling `prepare_view` after every tick in one run and never in the other, and asserts that both end in the same state.
[Determinism and testing](determinism.md#network-models) covers both tests and the RNG streams they rely on.

`NetworkModelState::from_graph` builds a state, runs `init`, and then hands a closure the `Nodes` to set up a particular graph.
Virus on a Network's recolour test uses it to make every third node resistant.
Note that anything `init` records in the `Aux` about the graph can be out of date once the closure has run.
Team Assembly lists its live nodes on the first tick, after any closure has run, instead of in `init`.

## Left to the engine

With the trait implemented, the engine handles all of the following:

- Allocates every lane, grows the lanes as nodes spawn, and swaps the `dual` ones after the node pass.
- Builds the graph with every node and no edges, and repacks its rows after `init` and whenever a tick leaves too much stale space.
- Reads `directed` every tick, and rebuilds the rows when it changes.
- Seeds the node pass, the global pass, the actions and the layout from separate streams.
- Splits the node pass into `CHUNK`-sized chunks across rayon, and seeds a generator per chunk per tick.
- Runs the spring layout on publish, within the budget the Pacing tab sets.
- Stores the parameters, and rejects any edit to a reload-only one.
- Builds the point view, the edge view, the history chart and the snapshots.

## Next

- [Writing a network model](../guide/first-model/virus-network.md) builds Virus on a Network.
- [Palettes and views](views.md) covers the edge layer, edge colours and retired nodes.
- [Determinism and testing](determinism.md#network-models) describes the two tests both shipped network models carry, and when yours needs them.
- [Agent models](agent-models.md) covers the lanes and `run_pass` in more depth.
