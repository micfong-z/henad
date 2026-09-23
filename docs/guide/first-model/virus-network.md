---
title: CPU Network model
description: Write a network model in Henad, building a virus that spreads over a random graph of nodes and edges.
icon: material/graph-outline
---

# Writing a CPU network model

In this tutorial we'll build the Virus on a Network model on the CPU from scratch, and it will automatically use all available cores.
This is classified as a **network model**, where every agent is a node and nodes are joined by edges.

This page assumes you have worked through our [CPU Agent model](ants.md) tutorial first, since a network model keeps its nodes in lanes the same way.

## What is Virus on a Network?

Before we write any code, let's be clear about what we are building.
Virus on a Network is a model of a virus spreading over a network of nodes.
Think of each node as a computer, and the virus as a computer virus that travels along the edges between them.
A node is always in one of three states: susceptible, infected or resistant.
Roughly, the rules are:

1. Nodes are scattered over the world and joined in random pairs, until they have a given number of neighbours on average.
   A few nodes start out infected.
2. Each tick, every infected node has a chance to infect each of its susceptible neighbours.
3. Every node runs a virus check once every few ticks.
   An infected node that runs its check has a chance to recover.
   A recovered node then has a chance to become resistant, and otherwise becomes susceptible again.
4. A resistant node never catches the virus again, and the edges touching it turn dark grey.

!!! info

    This model is a port of NetLogo's Virus on a Network model[^1].
    NetLogo repeatedly links a random node to its nearest unlinked node, and ours joins random pairs with no regard to distance.
    NetLogo stops the run once no node is infected, and ours keeps ticking.
    We'll also add two things that NetLogo's version does not have: directed edges, and rewiring, where an edge moves to a new pair of nodes.

## Moving from agents to a network

Assuming that you just built a [CPU Agent model](ants.md), here's a quick comparison of the two types of models.

|              | Agent model                                    | Network model                                                |
| ------------ | ---------------------------------------------- | ------------------------------------------------------------ |
| State        | one `Vec` per attribute, called a **lane**     | the same lanes, with one entry per node                      |
| Neighbours   | optional `SpatialHash::query_radius`           | the edges of a `Network`                                     |
| Transition   | a closure, called once per agent               | a closure, called once per node                              |
| Shared state | an optional [field](../../authoring/fields.md) | the graph, changed within a tick only by the **global pass** |
| Position     | part of the model                              | where the node is drawn                                      |

Nodes live in lanes, declared with the same `agent_lanes!` macro as the ants.
Edges live in a `Network` that the engine owns.
It keeps a list of every node's neighbours for fast reads, next to a plain edge list with one colour byte per edge for the viewport to draw.

A tick runs in two passes.
The **global pass** runs once, on one thread, and it is the only part of a tick that can change the graph.
The **node pass** then runs over every node in parallel, and every node can read the graph but none can change it.

``` mermaid
flowchart LR
    I["<code>fn init</code><br>configure nodes and edges"] --> G
    G["<code>fn run_global_pass</code><br>once per tick"] --> N["<code>fn run_node_pass</code><br>once per node"]
    N -->|each tick| G
    N -.->|on publish| V["<code>fn prepare_view</code>"]
    V -.-> T["<code>fn stats</code>"]
```

These pieces, plus `act` for a button press, are the ones we need to write ourselves.
The Henad engine handles the rest of the simulation, such as storing the graph, switching it between directed and undirected, splitting nodes across cores, handing each chunk its own random number generator, arranging the nodes on screen, and the snapshot the UI draws.

Let's set up by making a file at `crates/henad-models/src/virus.rs`.

## Node states (lanes)

A node needs to remember its state and how long ago it last ran a virus check.
We can declare this in lane form as follows:

``` { .rust .annotate title="crates/henad-models/src/virus.rs" }
use henad_compute::agent_lanes;

agent_lanes! {
    pub struct VirusLanes {
        read VirusRead;
        chunk VirusChunk;
        dual state / next_state: u8, // (1)!
        plain timer: u32 = 0, // (2)!
        plain pos_x: f32 = 0.0, // (3)!
        plain pos_y: f32 = 0.0,
    }
    color = state; // (4)!
}
```

1. Our first `dual` lane.
   We'll look at it more closely [below](#reading-neighbours).
2. The number of ticks since the node's last virus check.
   Only the node itself touches it, and a `plain` lane is enough.
3. `pos_x` and `pos_y` are still required.
   On a network they say where a node is drawn, and the model itself never reads them.
4. The state doubles as the node's palette index, as `has_food` did for the ants.

### Reading neighbours

The ants page declared every lane `plain`, because no ant ever read another ant's slot.
Nodes do read each other: a susceptible node looks at the state of every neighbour to decide whether it catches the virus.

A `plain` lane cannot support that.
The kernel only receives its own chunk's slice of it.
A node could not see a neighbour in another chunk at all.

A `dual` lane has two sides under two names.
The kernel reads every node's `state` through `VirusRead`, and writes its own next state through `VirusChunk`.
Once the node pass is done, the engine swaps the two sides.
Every node sees its neighbours as they were at the start of the tick, whichever core reached them first.

The macro expects every `dual` lane before the first `plain` one.
A `dual` lane also takes no initial value.
Both of its sides start at the type's default, 0 for a `u8`.
Our states start from that default, with 0 for susceptible:

``` rust title="crates/henad-models/src/virus.rs"
pub const SUSCEPTIBLE: u8 = 0;
pub const INFECTED: u8 = 1;
pub const RESISTANT: u8 = 2;

pub const PALETTE: [[u8; 4]; 3] = [
    [0x00, 0x7A, 0xF5, 0xFF], // Susceptible
    [0xE4, 0x37, 0x48, 0xFF], // Infected
    [0x80, 0x80, 0x80, 0xFF], // Resistant
];
```

Edges get a colour too, one byte per edge, and it indexes a palette of its own:

``` rust title="crates/henad-models/src/virus.rs"
pub const EDGE_OPEN: u8 = 0;
pub const EDGE_BLOCKED: u8 = 1;

pub const EDGE_PALETTE: [[u8; 4]; 2] = [
    [0xC8, 0xC8, 0xC8, 0xB0], // Open
    [0x50, 0x50, 0x50, 0x90], // Blocked
];
```

The fourth byte of each entry is alpha, and both edge colours are partly transparent.
Edges are drawn under the nodes.

## Implementing `NetworkModel`

This is very similar to [Implementing `AgentModel`](ants.md#implementing-agentmodel).

``` rust title="crates/henad-models/src/virus.rs"
use henad_core::authoring::model::network_model::NetworkModel;

pub struct VirusModel;

impl NetworkModel for VirusModel {}
```

### Identity and metadata

``` { .rust .annotate title="crates/henad-models/src/virus.rs" }
impl NetworkModel for VirusModel {
    const NAME: &'static str = "Virus on a Network";
    const ID: &'static str = "virus"; // (1)!
    const DESCRIPTION: &'static str = "A virus spreading over a network.";
    const PALETTE: &'static [[u8; 4]] = &PALETTE;
    const EDGE_PALETTE: &'static [[u8; 4]] = &EDGE_PALETTE;
    const STATS: &'static [StatDescriptor] = &[
        StatDescriptor::new("Susceptible", PALETTE[0]),
        StatDescriptor::new("Infected", PALETTE[1]),
        StatDescriptor::new("Resistant", PALETTE[2]),
    ];
    const DEFAULT_NODES: u32 = 10_000; // (2)!
    const DEFAULT_EXTENT: Extent = Extent { w: 1_000.0, h: 1_000.0 }; // (3)!

    type Lanes = VirusLanes;
    type Params = VirusParams; // (4)!
    type Aux = (); // (5)!
}
```

1. The shipped model already uses the ID `virus_network`, and IDs have to be unique across the registry.
2. The engine prepends the node count, world width and world height to the parameter list, and these two consts supply their defaults.
   `MAX_NODES` sets the upper bound, and we keep its default of ten million.
3. The world is only used for drawing.
   `init` scatters the nodes over it, and the layout moves them around inside it.
4. The hot parameters, extracted once per tick.
   We'll write this struct [below](#hot-parameters).
5. `Aux` holds any state the model keeps outside the lanes and the graph.
   Ours keeps none.

`CHUNK` keeps its default of 512 nodes per chunk, for the reasons given in [Deciding on `CHUNK`](ants.md#deciding-on-chunk).

Here are the other imports that `impl` relies on, along with a few that the functions below will need:

``` rust title="crates/henad-models/src/virus.rs"
use henad_core::authoring::model::field::Extent;
use henad_core::authoring::model::network_model::{NodeCtx, Nodes};
use henad_core::network::Network;
use henad_core::view::{StatDescriptor, StatValue};
```

### Parameters

The model declares eight parameters of its own:

``` rust title="crates/henad-models/src/virus.rs"
use henad_core::helpers::{bool_param, extract_bool, extract_f32, extract_u32, f32_param, u32_param};
use henad_core::params::{ParamDescriptor, ParamValue};

henad_core::params! {
    const AVERAGE_NODE_DEGREE = u32_param("average_node_degree", "Average Node Degree", 6, 1, 20).on_reload();
    const INITIAL_OUTBREAK_SIZE =
        u32_param("initial_outbreak_size", "Initial Outbreak Size", 3, 1, 10_000).on_reload();
    const VIRUS_SPREAD_CHANCE =
        f32_param("virus_spread_chance", "Virus Spread Chance", 0.025, 0.0, 1.0, Some(0.001)).percent();
    const VIRUS_CHECK_FREQUENCY = u32_param("virus_check_frequency", "Virus Check Frequency", 1, 1, 20);
    const RECOVERY_CHANCE = f32_param("recovery_chance", "Recovery Chance", 0.05, 0.0, 1.0, Some(0.001)).percent();
    const GAIN_RESISTANCE_CHANCE =
        f32_param("gain_resistance_chance", "Gain Resistance Chance", 0.05, 0.0, 1.0, Some(0.01)).percent();
    const DIRECTED = bool_param("directed", "Directed", false);
    const KEEP_REWIRING = bool_param("keep_rewiring", "Keep Rewiring", false);
}
```

Only `init` reads the degree and the outbreak size.
Both take `.on_reload()`, as the density did in Life.

The three chances are probabilities, stored as fractions from 0 to 1.
NetLogo's sliders show them as percentages.
`.percent()` asks the Parameters tab to do the same, where `0.025` reads `2.5%`.
Only the display changes.
The model still reads `0.025`, and `--set virus_spread_chance=0.1` on the command line means 10%.

`bool_param` declares a bool parameter, and the Parameters tab draws it as a checkbox.
Both of ours are live, and can be flipped while the model runs.

The shipped model prints the same list, with one extra entry at index 10:

``` text title="cargo run -p henad-cli -- virus_network --params" hl_lines="2 3 4"
parameters for virus_network (Virus on a Network):
  index=0 id=num_agents kind=u32 default=10000 min=1 max=10000000 apply=reload label="Number of Nodes"
  index=1 id=world_width kind=f32 default=1000 min=1 max=10000 apply=reload label="World Width"
  index=2 id=world_height kind=f32 default=1000 min=1 max=10000 apply=reload label="World Height"
  index=3 id=average_node_degree kind=u32 default=6 min=1 max=20 apply=reload label="Average Node Degree"
  index=4 id=initial_outbreak_size kind=u32 default=3 min=1 max=10000 apply=reload label="Initial Outbreak Size"
  index=5 id=virus_spread_chance kind=f32 default=0.025 min=0 max=1 apply=live format=percent label="Virus Spread Chance"
  index=6 id=virus_check_frequency kind=u32 default=1 min=1 max=20 apply=live label="Virus Check Frequency"
  index=7 id=recovery_chance kind=f32 default=0.05 min=0 max=1 apply=live format=percent label="Recovery Chance"
  index=8 id=gain_resistance_chance kind=f32 default=0.05 min=0 max=1 apply=live format=percent label="Gain Resistance Chance"
  index=9 id=directed kind=bool default=false apply=live label="Directed"
  index=10 id=network kind=choice default=0 options=Random|Geometric apply=reload label="Network"
  index=11 id=keep_rewiring kind=bool default=false apply=live label="Keep Rewiring"
```

The highlighted lines are the three the engine prepends.
The node count keeps the ID `num_agents`, the same one an agent model uses.

!!! tip "What is index 10?"

    The shipped model declares one more parameter than ours, a `choice_param` named Network.
    It picks between the random graph we write below and a random geometric graph.
    The geometric graph joins every pair of nodes closer than some distance.
    The geometric generator lives in [`crates/henad-models/src/virus_network/wiring.rs`](https://github.com/micfong-z/henad/blob/master/crates/henad-models/src/virus_network/wiring.rs).
    Our list has no Network entry, and ends with Keep Rewiring at index 10.

#### Hot parameters

``` rust title="crates/henad-models/src/virus.rs"
pub struct VirusParams {
    spread_chance: f32,
    check_frequency: u32,
    recovery_chance: f32,
    resistance_chance: f32,
    directed: bool,
    keep_rewiring: bool,
}

    fn param_descriptors() -> Vec<ParamDescriptor> {
        descriptors()
    }

    fn from_params(params: &[ParamValue], _extent: Extent) -> VirusParams {
        VirusParams {
            spread_chance: extract_f32(params, VIRUS_SPREAD_CHANCE, 0.025),
            check_frequency: extract_u32(params, VIRUS_CHECK_FREQUENCY, 1).max(1),
            recovery_chance: extract_f32(params, RECOVERY_CHANCE, 0.05),
            resistance_chance: extract_f32(params, GAIN_RESISTANCE_CHANCE, 0.05),
            directed: extract_bool(params, DIRECTED, false),
            keep_rewiring: extract_bool(params, KEEP_REWIRING, false),
        }
    }
```

`_extent` goes unused.
Nothing in `VirusParams` depends on the size of the world.

### Directed edges

One more function reads the hot parameters:

``` rust title="crates/henad-models/src/virus.rs"
    fn directed(params: &VirusParams) -> bool {
        params.directed
    }
```

The engine calls it at the start of every tick, and switches the graph over whenever the answer changes.
Ticking the Directed checkbox takes effect on the next tick.

Switching keeps every edge and only changes how it is read.
An undirected edge joins its two ends both ways, and a directed one runs from its first end to its second.
Our kernel will read neighbours through `in_neighbors`.
On a directed graph, it lists only the nodes with an edge pointing at this one.
Once Directed is ticked, the virus can only travel in the direction of an edge.

Without this function a model stays undirected.

## Initialisation

`init` is called once to fill the lanes and draw the edges.
It receives a `Nodes` instead of the lanes alone.
A `Nodes` bundles mutable access to the lanes, the graph and the aux.
The graph starts out with every node and no edges.

``` { .rust .annotate title="crates/henad-models/src/virus.rs" }
/// Fraction of the world that nodes are placed in. The rest is a margin along the borders.
const PLACED: f32 = 0.95;

    fn init(nodes: &mut Nodes<'_, Self>, extent: Extent, params: &[ParamValue], rng: &mut u64) {
        let lanes = &mut *nodes.lanes; // (1)!
        let check_frequency = extract_u32(params, VIRUS_CHECK_FREQUENCY, 1).max(1);
        let area = Extent {
            w: extent.w * PLACED,
            h: extent.h * PLACED,
        };
        let (margin_x, margin_y) = (0.5 * (extent.w - area.w), 0.5 * (extent.h - area.h));
        for i in 0..lanes.pos_x.len() {
            lanes.pos_x[i] = margin_x + next_float(rng, area.w);
            lanes.pos_y[i] = margin_y + next_float(rng, area.h);
            lanes.timer[i] = next_index(rng, check_frequency); // (2)!
        }

        let degree = extract_u32(params, AVERAGE_NODE_DEGREE, 6);
        random_graph(nodes.graph, degree, rng); // (3)!

        infect_distinct(&mut lanes.state, extract_u32(params, INITIAL_OUTBREAK_SIZE, 3), rng);
    }
```

1. A shorter name for the lanes.
   It borrows only the `lanes` field, and leaves `nodes.graph` free for a few lines down.
2. `next_index(rng, n)` draws a whole number below `n`, like NetLogo's `random n`.
   A random starting timer spreads the checks out.
   Otherwise every node would check on the same tick.
3. We'll write `random_graph` and `infect_distinct` next.

Like NetLogo, we keep the nodes off the outer 2.5% of the world on each side.

The order of the draws matters.
Positions and timers come first, then the edges, then the outbreak.
Moving one of them around would give a different run from the same seed.

Both draws come from one import:

``` rust title="crates/henad-models/src/virus.rs"
use henad_core::authoring::primitives::rng::{next_float, next_index};
```

### A random graph

Rule 1 joins random pairs of nodes.
We need two small helpers first:

``` { .rust .annotate title="crates/henad-models/src/virus.rs" }
/// Draws two distinct nodes uniformly at random.
fn random_pair(n: u32, rng: &mut u64) -> (u32, u32) {
    let a = next_index(rng, n);
    let b = next_index(rng, n - 1); // (1)!
    (a, if b >= a { b + 1 } else { b })
}

/// Returns whether an edge joins `a` and `b` in either direction.
fn joined(graph: &Network, a: u32, b: u32) -> bool {
    graph.has_edge(a, b) || (graph.directed() && graph.has_edge(b, a)) // (2)!
}
```

1. The second draw picks from the `n - 1` nodes other than `a`, by stepping over `a`.
   The two ends always differ, with no redraw needed.
   An edge from a node to itself has no meaning here, and `add_edge` asserts against one in debug builds.
2. On an undirected graph, `has_edge` already finds an edge in either direction.
   On a directed graph it only looks from `a` to `b`, and we look the other way ourselves.
   Otherwise switching back to undirected could find two edges between the same pair of nodes.

Then the generator itself:

``` { .rust .annotate title="crates/henad-models/src/virus.rs" }
/// Joins random pairs of nodes until the average degree reaches `degree`.
fn random_graph(graph: &mut Network, degree: u32, rng: &mut u64) {
    let n = graph.slot_count() as u64; // (1)!
    if n < 2 {
        return;
    }
    let wanted = (u64::from(degree) * n).div_ceil(2).min(n * (n - 1) / 2) as usize; // (2)!
    while graph.edge_count() < wanted {
        let (a, b) = random_pair(n as u32, rng);
        if !joined(graph, a, b) { // (3)!
            graph.add_edge(a, b, EDGE_OPEN);
        }
    }
}
```

1. `slot_count` counts every slot in the graph, retired ones included.
   This model never retires a node, and here it equals the node count.
2. Each edge adds one to the degree of both of its ends, so an average degree `d` over `n` nodes takes `d * n / 2` edges.
   The `min` stops at a complete graph, where the loop would otherwise never finish.
3. A pair that is already joined is skipped.
   Every simple graph with this many edges is then equally likely.
   This is the random graph known as G(n, m).

`random_pair` draws its two ends in random order, and each edge keeps that order as its direction.
When Directed is ticked, every edge points a random way.

### The outbreak

Rule 1 also infects a few nodes at the start.
We could pick nodes at random and draw again on a repeat, but that slows down badly as the outbreak size nears the node count.
Most draws then land on a node that is already infected.
Floyd's sampling picks `count` distinct nodes with exactly `count` draws:

``` { .rust .annotate title="crates/henad-models/src/virus.rs" }
/// Infects `count` distinct nodes, like NetLogo's `n-of`.
fn infect_distinct(state: &mut [u8], count: u32, rng: &mut u64) {
    let n = state.len() as u32;
    for j in n - count.min(n)..n { // (1)!
        let t = next_index(rng, j + 1);
        let pick = if state[t as usize] == INFECTED { j } else { t }; // (2)!
        state[pick as usize] = INFECTED;
    }
}
```

1. An outbreak larger than the network infects every node.
2. Every earlier round drew from a smaller range than this round's `0..=j`.
   Node `j` itself cannot have been picked yet, and a repeat takes `j` instead.

The state lane doubles as the record of which nodes are already picked.

## The node pass

The node pass takes the role `step_cell` played on the grid page.
One node decides its own next state and timer, and writes nothing else.

``` { .rust .annotate title="crates/henad-models/src/virus.rs" }
#[inline]
fn step_node(
    i: usize,
    k: usize,
    read: VirusRead<'_>,
    out: &mut VirusChunk<'_>,
    graph: &Network,
    params: &VirusParams,
    rng: &mut u64,
) {
    let timer = out.timer[k] + 1;
    let timer = if timer >= params.check_frequency { 0 } else { timer }; // (1)!
    out.timer[k] = timer;

    let mut state = read.state[i]; // (2)!
    if state == SUSCEPTIBLE {
        for &j in graph.in_neighbors(i as u32) { // (3)!
            if read.state[j as usize] == INFECTED && next_float(rng, 1.0) < params.spread_chance {
                state = INFECTED;
                break; // (4)!
            }
        }
    }
    if state == INFECTED && timer == 0 && next_float(rng, 1.0) < params.recovery_chance { // (5)!
        state = if next_float(rng, 1.0) < params.resistance_chance {
            RESISTANT
        } else {
            SUSCEPTIBLE
        };
    }
    out.state[k] = state; // (6)!
}
```

1. The timer counts up and wraps back to 0 every `check_frequency` ticks.
   A check is due on a tick where it reads 0.
2. `read` holds every node's state as it was at the start of the tick, indexed by the global index `i`.
   `out` holds this chunk's lanes, indexed by `k`, the position within the chunk.
3. See rule 2, and the section [below](#pulling-the-infection).
4. One infected neighbour is enough, and further draws could not change the outcome.
5. See rule 3.
   This runs after spreading.
   As in NetLogo, a node infected on this tick can already recover on it.
6. This writes the next side of the `dual` lane.

### Pulling the infection

NetLogo's rule pushes the virus: each infected node asks each of its neighbours to roll for infection.
Pushing would have an infected node write into its neighbours' next states.
The kernel only receives its own chunk's slice, and a neighbour can sit in another chunk.
The neighbour also writes its own next state in the same pass, and whichever write came last would win.

So we turn the rule around.
Each susceptible node looks at its own in-neighbours, and rolls once for every infected one it finds.
It writes only its own state.
Either way, every infected neighbour gives a susceptible node one independent chance, drawn against the states at the start of the tick, and the chance of catching the virus comes out the same.

### Running it over the nodes

As with the ants, the `run_pass` method generated by `agent_lanes!` handles the chunking and the seeding.

``` { .rust .annotate title="crates/henad-models/src/virus.rs" }
    fn run_node_pass(lanes: &mut VirusLanes, ctx: &NodeCtx<'_, Self>, seed: u64, tick: u64) {
        let (graph, params) = (ctx.graph, ctx.params); // (1)!
        lanes.run_pass(Self::CHUNK, seed, tick, |i, k, read, out, rng| {
            step_node(i, k, read, out, graph, params, rng); // (2)!
        });
    }
```

1. `NodeCtx` bundles the graph, the hot parameters and the extent.
   The graph arrives behind a shared reference, and the borrow checker keeps the node pass from changing it.
2. The closure returns nothing.
   A network model has no tally to fold.

## Colouring edges

Rule 4 greys every edge that touches a resistant node.
NetLogo does this the moment a node turns resistant, but our node pass cannot touch the graph.

Edge colours only matter when something draws them.
`prepare_view` runs each time a snapshot is published, just before the snapshot is built, and it can change the graph.

First, the rule for a single edge:

``` rust title="crates/henad-models/src/virus.rs"
/// Returns the colour of an edge between nodes in states `a` and `b`.
fn edge_color(a: u8, b: u8) -> u8 {
    if a == RESISTANT || b == RESISTANT {
        EDGE_BLOCKED
    } else {
        EDGE_OPEN
    }
}
```

Then a first version of `prepare_view` that walks every edge:

``` { .rust .annotate title="crates/henad-models/src/virus.rs" }
    fn prepare_view(nodes: &mut Nodes<'_, Self>, _tick: u64) {
        let state = &nodes.lanes.state; // (1)!
        nodes.graph.update_colors(|src, dst, color| { // (2)!
            let mut changed = false;
            for e in 0..color.len() {
                let wanted = edge_color(state[src[e] as usize], state[dst[e] as usize]);
                if color[e] != wanted {
                    color[e] = wanted;
                    changed = true;
                }
            }
            changed // (3)!
        });
    }
```

1. The two sides of `state` were swapped at the end of the last tick.
   This side holds the newest state of every node.
2. `update_colors` hands the closure the edge list, as the two endpoint slices `src` and `dst`, and a mutable slice of the colours.
3. The closure returns whether it changed any colour, and the graph's version moves only if it did.

The version tells the snapshot whether it has to copy the edge list again.
A closure that always returned `true` would have every publish copy every edge, even when nothing had changed.

This works, but it walks the edges on a single core.
At the default degree of 6, the edge list holds three edges for every node.
Let's split it into chunks instead, the way Life counted its cells:

``` { .rust .annotate title="crates/henad-models/src/virus.rs" }
/// Number of edges per chunk of the recolour pass.
const EDGE_CHUNK: usize = 8192;

/// Greys every edge that touches a resistant node, and returns whether any edge changed.
fn recolor(src: &[u32], dst: &[u32], color: &mut [u8], state: &[u8]) -> bool {
    let wanted = |e: usize| edge_color(state[src[e] as usize], state[dst[e] as usize]);
    let stale = {
        let color = &*color;
        reduce_chunks( // (1)!
            color.len(),
            EDGE_CHUNK,
            |mut range| range.any(|e| color[e] != wanted(e)),
            |a, b| a || b,
            false,
        )
    };
    if stale {
        for_each_chunk_mut!(color, EDGE_CHUNK, |_c, base, slice| { // (2)!
            for (k, c) in slice.iter_mut().enumerate() {
                *c = wanted(base + k);
            }
        });
    }
    stale
}
```

1. A first pass that only reads.
   A publish while paused, or after the outbreak has died out, finds no edge to change and stops here.
2. `for_each_chunk_mut!` hands each chunk its own slice to write, in parallel, and `base` is the index of the chunk's first edge.
   Once anything is stale, every colour is written again.
   We never have to track which edges changed.

`prepare_view` then shrinks to a single call:

``` rust title="crates/henad-models/src/virus.rs"
    fn prepare_view(nodes: &mut Nodes<'_, Self>, _tick: u64) {
        let state = &nodes.lanes.state;
        nodes
            .graph
            .update_colors(|src, dst, color| recolor(src, dst, color, state));
    }
```

``` rust title="crates/henad-models/src/virus.rs"
use henad_compute::cpu::primitives::chunked::{STATS_CHUNK, reduce_chunks};
use henad_compute::for_each_chunk_mut;
```

`STATS_CHUNK` is for the next section.

## Statistics

We report how many nodes are in each state.

``` { .rust .annotate title="crates/henad-models/src/virus.rs" }
    fn stats(lanes: &VirusLanes, _graph: &Network, (): &()) -> Vec<StatValue> { // (1)!
        count_states(&lanes.state)
            .into_iter()
            .map(|count| StatValue::Scalar(count as f64))
            .collect()
    }
```

1. `stats` also receives the graph and the aux, and it can read both but change neither.
   `(): &()` matches our empty aux.

The count is a parallel reduction, like the one in Life:

``` { .rust .annotate title="crates/henad-models/src/virus.rs" }
/// Returns the numbers of susceptible, infected and resistant nodes.
fn count_states(state: &[u8]) -> [u64; 3] {
    reduce_chunks(
        state.len(),
        STATS_CHUNK,
        |range| {
            let mut counts = [0u64; 3];
            for &s in &state[range] {
                counts[usize::from(s)] += 1; // (1)!
            }
            counts
        },
        |a, b| [a[0] + b[0], a[1] + b[1], a[2] + b[2]],
        [0; 3],
    )
}
```

1. The state is an index here too.
   One pass counts all three states, in the same order as `STATS`.

## Rewiring

So far the graph never changes after `init`.
The last two things we add both move edges while the virus spreads.
Both use the same rewire.
It picks an edge at random and moves it to a random pair of nodes that are not joined yet.

This follows `rewire-a-link` from NetLogo's [Diffusion on a Directed Network](https://ccl.northwestern.edu/netlogo/models/DiffusiononaDirectedNetwork) model, except that any pair that is not joined can receive the edge.

``` { .rust .annotate title="crates/henad-models/src/virus.rs" }
/// Number of draws a rewire makes before giving up.
const REWIRE_TRIES: u32 = 64;

/// Moves one random edge to a random pair of nodes that are not already joined.
fn rewire(graph: &mut Network, state: &[u8], rng: &mut u64) {
    let (n, m) = (graph.slot_count() as u32, graph.edge_count() as u32);
    if n < 2 || m == 0 {
        return;
    }
    for _ in 0..REWIRE_TRIES { // (1)!
        let (a, b) = random_pair(n, rng);
        if !joined(graph, a, b) {
            graph.remove_edge(next_index(rng, m)); // (2)!
            graph.add_edge(a, b, edge_color(state[a as usize], state[b as usize])); // (3)!
            return;
        }
    }
}
```

1. A nearly complete graph has almost no free pair to move an edge to.
   After 64 failed draws, the rewire gives up and leaves the graph as it was.
2. `remove_edge` fills the gap with the last edge in the list, so an edge's index only holds until the next removal.
3. The new edge takes its colour from its two ends straight away, by the same rule as the recolour.

The edge count stays the same, and so does the average degree.

### An action

An action is a one-off change to the state that the user triggers between two ticks.
Declaring one works much like declaring parameters:

``` { .rust .annotate title="crates/henad-models/src/virus.rs" }
use henad_core::action::ActionDescriptor;

henad_core::actions! {
    const REWIRE = ActionDescriptor::new("rewire", "Rewire a link"); // (1)!
}
```

1. `henad-cli --act` matches on the ID, and the label goes on the button.

`actions!` gives each entry a `const` holding its index, and collects the descriptors into `ACTION_SPECS`.
The impl points `ACTIONS` at that list and runs the rewire when the button is pressed:

``` { .rust .annotate title="crates/henad-models/src/virus.rs" }
    const ACTIONS: &'static [ActionDescriptor] = ACTION_SPECS;

    #[expect(clippy::single_match, reason = "for future multi-action extendability")] // (1)!
    fn act(action: usize, nodes: &mut Nodes<'_, Self>, _extent: Extent, _params: &[ParamValue], rng: &mut u64) {
        match action { // (2)!
            REWIRE => rewire(nodes.graph, &nodes.lanes.state, rng),
            _ => {}
        }
    }
```

1. Clippy prefers an `if` to a `match` with a single arm.
   `#[expect]` tells it the `match` is deliberate, and a second action is one more arm.
   The attribute comes out with that second arm, or Clippy warns that the expectation is unfulfilled.
2. `action` is the index of the entry that was pressed.

The Parameters tab draws one button per entry in `ACTIONS`, under the parameter widgets.
The button stays disabled until the model is built.

!!! note "Actions draw from their own stream"

    The `rng` handed to `act` is a random number stream of its own, and a press leaves the streams the ticks draw from untouched.
    The same seed and the same presses at the same ticks give the same run.

### Keep rewiring

The Keep Rewiring parameter runs the same rewire once per tick, in the global pass:

``` rust title="crates/henad-models/src/virus.rs"
    fn run_global_pass(nodes: &mut Nodes<'_, Self>, params: &VirusParams, _extent: Extent, rng: &mut u64, _tick: u64) {
        if params.keep_rewiring {
            rewire(nodes.graph, &nodes.lanes.state, rng);
        }
    }
```

The global pass sees the states the last tick left behind, and draws from a random number stream of its own.

## Running it

That finishes the model.
Declare the module and register it, and then we can run it.

``` rust title="crates/henad-models/src/lib.rs"
pub mod virus;
```

``` rust title="crates/henad-models/src/registry.rs"
register_network_model::<crate::virus::VirusModel>(),
```

=== "Desktop app"

    ``` bash
    cargo run --release --bin henad-app
    ```

    Pick the second Virus on a Network, press Build, and set it playing.
    The three infected nodes turn a good part of the graph red, and the red then fades as more and more nodes turn grey.
    See [App tour](../app.md) for a quick overview of the UI.

=== "Headless"

    ``` bash
    cargo run --release -p henad-cli -- virus --steps 1000 --reps 3
    ```

    A timed run never publishes a snapshot, and the timing leaves out `prepare_view` and the layout.

    To press Rewire a link part way through a run, name the action and the tick:

    ``` bash
    cargo run --release -p henad-cli -- virus --steps 1000 --act rewire@500
    ```

=== "Browser"

    ``` bash
    ./scripts/build_web.sh serve --release
    ```

    Then open `http://localhost:8080`.
    See [App tour](../app.md) for a quick overview of the UI.

### Layout

After Build, the nodes sit wherever `init` scattered them, and the edges cross the world in every direction.
While Layout is ticked in the [Pacing tab](../app.md#pacing-tab), a spring layout arranges the nodes as the model runs.
It is ticked by default.
In Sprites mode, the Edges and Arrows checkboxes in the [Viewport tab](../app.md#viewport-tab) control how the edges are drawn.

The layout runs as a snapshot is published, outside `step()`, and it only moves positions.
Our model never reads a position, so the layout cannot change how the virus spreads.
A model can tune the springs through its `LAYOUT` const, and ours keeps the defaults.

## Testing

A virus is random, but a spread chance of 1 and a recovery chance of 0 take the randomness out of it.
The virus then moves exactly one edge per tick.
A path of five nodes, with the outbreak at one end and a resistant node in the middle, checks rules 2 and 4 in one go.

``` { .rust .annotate title="crates/henad-models/src/virus.rs" }
#[test]
fn the_virus_walks_a_path_and_stops_at_a_resistant_node() {
    use henad_compute::cpu::network_engine::NetworkModelState;
    use henad_core::model::SimState as _;

    let params = vec![ // (1)!
        ParamValue::U32(5),      // num_agents
        ParamValue::F32(100.0),  // world_width
        ParamValue::F32(100.0),  // world_height
        ParamValue::U32(1),      // average_node_degree
        ParamValue::U32(1),      // initial_outbreak_size
        ParamValue::F32(1.0),    // virus_spread_chance
        ParamValue::U32(1),      // virus_check_frequency
        ParamValue::F32(0.0),    // recovery_chance
        ParamValue::F32(0.0),    // gain_resistance_chance
        ParamValue::Bool(false), // directed
        ParamValue::Bool(false), // keep_rewiring
    ];

    // A path from 0 to 4, with the outbreak at one end and a resistant node in the middle.
    let mut state = NetworkModelState::<VirusModel>::from_graph(&params, None, |nodes, _extent| { // (2)!
        *nodes.graph = Network::new(5, false); // (3)!
        for i in 0..4 {
            nodes.graph.add_edge(i, i + 1, EDGE_OPEN);
        }
        nodes
            .lanes
            .state
            .copy_from_slice(&[INFECTED, SUSCEPTIBLE, RESISTANT, SUSCEPTIBLE, SUSCEPTIBLE]);
    });

    state.step();
    assert_eq!(
        state.lanes().state,
        [INFECTED, INFECTED, RESISTANT, SUSCEPTIBLE, SUSCEPTIBLE],
        "the virus did not reach the next node"
    );

    for _ in 0..10 {
        state.step();
    }
    assert_eq!(
        state.lanes().state,
        [INFECTED, INFECTED, RESISTANT, SUSCEPTIBLE, SUSCEPTIBLE],
        "the virus crossed a resistant node"
    );

    state.prepare_view(); // (4)!
    let (_, _, color) = state.graph().edges();
    assert_eq!(
        color,
        [EDGE_OPEN, EDGE_BLOCKED, EDGE_BLOCKED, EDGE_OPEN],
        "the two edges touching node 2 are not grey"
    );
}
```

1. This is the full parameter list, with the engine's three first.
   The degree and the outbreak size only matter to `init`, whose graph and outbreak the closure below replaces.
2. Unlike `from_cells` on the grid page, `from_graph` still runs `init`.
   It then hands the closure the nodes to change before the first tick.
3. This throws away the random graph `init` drew.
   `Network::new(5, false)` holds five nodes and no edges.
4. The test calls `prepare_view` itself, the way a publish would, before it reads the colours.

Registering the model also opted us into the registry tests.
They check that its declared parameters, topology, stat series and actions match what its state actually does.

## The finished file

Here is everything we wrote on this page, gathered into one file.

??? example "`virus.rs` completed"

    ``` rust
    --8<-- "crates/henad-models/src/tests/tutorial/virus.rs"
    ```

The listing above is stored in the repository at [`crates/henad-models/src/tests/tutorial/virus.rs`](https://github.com/micfong-z/henad/blob/master/crates/henad-models/src/tests/tutorial/virus.rs).

The actual default model is at [`crates/henad-models/src/virus_network/`](https://github.com/micfong-z/henad/tree/master/crates/henad-models/src/virus_network).
It splits the same code across four files, and adds the geometric generator.

## Next

- [Writing a GPU grid model](gpu-game-of-life.md) and [writing a GPU agent model](gpu-ants.md) take the grid and agent models onto the GPU.
  Henad has no GPU trait for network models.
- [Network models](../../authoring/network-models.md) covers the `NetworkModel` trait from the reference side.
- The shipped Team Assembly model, in [`crates/henad-models/src/team_assembly/`](https://github.com/micfong-z/henad/tree/master/crates/henad-models/src/team_assembly), is a second network model, one that spawns and retires nodes as it runs.

[^1]: Stonedahl, F.; Wilensky, U. (2008). _[NetLogo Virus on a Network model](https://ccl.northwestern.edu/netlogo/models/VirusonaNetwork)_. Center for Connected Learning and Computer-Based Modeling, Northwestern University, Evanston, IL.
