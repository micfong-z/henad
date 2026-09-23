---
title: Default models
description: The ten models that ship with Henad, and every parameter each one declares.
icon: material/cube-outline
---

# The models

Ten models ship in the registry.
Six of them run on the CPU, and four of those six have a GPU port running the same simulation entirely in compute shaders.
The two network models run on the CPU only.

```bash
cargo run -p henad-cli -- --list
```

| Id | Name | Topology | Backend |
|---|---|---|---|
| `sir` | SIR Epidemic | Grid | CPU |
| `game_of_life` | Game of Life | Grid | CPU |
| `boids` | Boids Flocking | Agents | CPU |
| `ants` | Ant Foraging | Agents over a field | CPU |
| `virus_network` | Virus on a Network | Network | CPU |
| `team_assembly` | Team Assembly | Network | CPU |
| `gpu_sir` | SIR Epidemic (GPU) | Grid | GPU |
| `gpu_game_of_life` | Game of Life (GPU) | Grid | GPU |
| `gpu_boids` | Boids Flocking (GPU) | Agents | GPU |
| `gpu_ants` | Ant Foraging (GPU) | Agents over a field | GPU |

The four GPU entries appear only when wgpu finds an adapter with compute support.
Each GPU port seeds itself through its CPU counterpart's `init`, which makes tick 0 bit-identical between the two backends and a comparison between them fair.

## Parameters

Every model declares its parameters, and both front ends read the same declarations.
Print them with `--params`:

```bash
cargo run -p henad-cli -- boids --params
```

Each parameter is `live` or `reload`.
A live parameter takes effect on the next tick, while a reload one applies only when the model is rebuilt.
The engine prepends grid width and height to every grid model's list, and agent count, world width and world height to every agent model's.
A network model gets the same three as an agent model, with `num_agents` as its node count and the world as the area its nodes are placed and drawn in.
A percentage parameter holds a fraction from 0 to 1, and the app shows it as a percentage.

Most models also declare actions, one-off changes to the state.
The app draws each action as a button in the [Parameters tab](../guide/app.md#parameters-tab), and `henad-cli --act` runs one at a given tick (see [the command line](cli.md)).
A GPU port declares the same actions as its CPU model, under the same ids.

### Game of Life

Conway's Game of Life on a toroidal grid.

| Id | Kind | Default | Range |
|---|---|---|---|
| `grid_width` | u32 | 1024 | 1 to 10000, or 16384 on the GPU |
| `grid_height` | u32 | 1024 | 1 to 10000, or 16384 on the GPU |
| `density` | f32 | 0.3 | 0 to 1 |

The `randomise` action refills the grid at the `density` the model was built with, and `clear` kills every cell.

### SIR Epidemic

The classic SIR compartmental model on a 2D grid with a Moore neighbourhood.

| Id | Kind | Default | Range |
|---|---|---|---|
| `grid_width` | u32 | 1024 | 1 to 10000, or 16384 on the GPU |
| `grid_height` | u32 | 1024 | 1 to 10000, or 16384 on the GPU |
| `infection_rate` | f32 | 0.3 | 0 to 1 |
| `recovery_rate` | f32 | 0.05 | 0 to 1 |
| `initial_infected_pct` | f32 | 0.01 | 0 to 1 |

The `seed_outbreak` action infects each susceptible cell with the probability `initial_infected_pct` had when the model was built.

### Boids Flocking

Flocking over a torus.
A spatial hash, rebuilt every tick, answers the neighbour queries.

| Id | Kind | Default | Range |
|---|---|---|---|
| `num_agents` | u32 | 50000 | 1 to 1000000 |
| `world_width` | f32 | 1000 | 1 to 10000 |
| `world_height` | f32 | 1000 | 1 to 10000 |
| `visual_range` | f32 | 50 | 1 to 200 |
| `protected_range` | f32 | 8 | 0.5 to 50 |
| `separation` | f32 | 0.05 | 0 to 2 |
| `alignment` | f32 | 0.05 | 0 to 2 |
| `cohesion` | f32 | 0.0005 | 0 to 0.01 |
| `max_speed` | f32 | 15 | 1 to 50 |
| `min_speed` | f32 | 3 | 0.5 to 20 |

The `randomise_headings` action points every boid in a random direction without changing its speed.

### Ant Foraging

A population over a pheromone field, the one composite model in the registry.
Ants deposit into a scalar field that decays each tick, then steer by the values they read back.

| Id | Kind | Default | Range |
|---|---|---|---|
| `num_agents` | u32 | 2000 | 1 to 5000000 |
| `world_width` | f32 | 200 | 1 to 10000 |
| `world_height` | f32 | 200 | 1 to 10000 |
| `update_cutdown` | f32 | 0.9 | 0.5 to 1 |
| `reward` | f32 | 1 | 0.1 to 10 |
| `momentum` | f32 | 0.8 | 0 to 1 |
| `random_action` | f32 | 0.1 | 0 to 1 |
| `evaporation` | f32 | 0.999 | 0.9 to 1 |

The `reset_colony` action clears both pheromone trails and puts every ant back on the nest.

### Virus on a Network

NetLogo's Virus on a Network, with rewiring and a directed variant added.
Each infected neighbour of a susceptible node infects it with probability `virus_spread_chance`, independently of the others.
Every `virus_check_frequency` ticks an infected node recovers with probability `recovery_chance`.
A node that recovers becomes resistant with probability `gain_resistance_chance`, and susceptible again otherwise.
Each node keeps its own check timer, started at a random offset.
The stats count susceptible, infected and resistant nodes.

| Id | Kind | Default | Range |
|---|---|---|---|
| `num_agents` | u32 | 10000 | 1 to 10000000 |
| `world_width` | f32 | 1000 | 1 to 10000 |
| `world_height` | f32 | 1000 | 1 to 10000 |
| `average_node_degree` | u32 | 6 | 1 to 20 |
| `initial_outbreak_size` | u32 | 3 | 1 to 10000 |
| `virus_spread_chance` | f32 | 0.025 | 0 to 1 |
| `virus_check_frequency` | u32 | 1 | 1 to 20 |
| `recovery_chance` | f32 | 0.05 | 0 to 1 |
| `gain_resistance_chance` | f32 | 0.05 | 0 to 1 |
| `directed` | bool | false | true or false |
| `network` | choice | `Random` | `Random` or `Geometric` |
| `keep_rewiring` | bool | false | true or false |

`network` picks the generator.
`Random` draws a uniform random graph with `average_node_degree * num_agents / 2` edges, rounded up.
`Geometric` joins every pair of nodes closer than a radius chosen so that the mean degree comes out at `average_node_degree`.
Either way each edge points in a random direction.

`directed` can be switched mid-run.
Turned on, it lets the virus cross an edge only in that edge's direction.
Turning it on or off keeps every edge and its direction.

`keep_rewiring` moves one random edge to a random unjoined pair of nodes each tick.
The `rewire` action, the Rewire a link button, moves one edge the same way on demand.
A rewire keeps the edge count and never joins a pair twice.

The model differs from NetLogo's in these ways.

- NetLogo builds a spatially clustered network.
  It links a random node to its nearest unlinked node until the edge count is reached.
  `Random` keeps that edge count with no spatial structure, and `Geometric` is the spatial option.
- A run carries on after the last infected node recovers.
  NetLogo's stops there.
- Recovery and resistance are decided by a real-valued draw.
  NetLogo draws an integer with `random 100`, and the two agree at whole percentages.
- `average_node_degree` stops at 20.
  NetLogo's slider reaches one less than the node count.
- `virus_spread_chance` and `recovery_chance` reach 100%, and the node count reaches 10,000,000, with 10,000 by default.
  NetLogo's sliders stop at 10% and 300 nodes, with 150 nodes by default.
- `directed`, `keep_rewiring` and the `rewire` action are additions.
  Rewiring follows `rewire-a-link` from NetLogo's Diffusion on a Directed Network, except that any unjoined pair can take the edge.

### Team Assembly

NetLogo's Team Assembly, after Guimerà, Uzzi, Spiro and Amaral (2005).
Each tick one team of `team_size` members is assembled, and every pair of its members is joined by an edge.
A member is an incumbent with probability `p` and a newcomer otherwise.
With probability `q` an incumbent member is drawn uniformly from the previous collaborators of the team so far.
Otherwise, or when the team has none, it is drawn uniformly from the incumbents outside the team.
A node that goes more than `max_downtime` ticks without joining a team retires, and its edges go with it.

| Id | Kind | Default | Range |
|---|---|---|---|
| `num_agents` | u32 | 4 | 1 to 10000000 |
| `world_width` | f32 | 100 | 1 to 10000 |
| `world_height` | f32 | 100 | 1 to 10000 |
| `team_size` | u32 | 4 | 3 to 8 |
| `max_downtime` | u32 | 40 | 7 to 1000000 |
| `p` | f32 | 0.4 | 0 to 1 |
| `q` | f32 | 0.65 | 0 to 1 |

`num_agents` is the starting population, split into teams of `team_size`, each with an edge between every pair of its members.
The population then grows with each newcomer and shrinks with each retirement.
A larger `max_downtime` gives a larger steady-state population.

Newcomer-Newcomer Links, Newcomer-Incumbent Links, Incumbent-Incumbent Links and Previous Collaborator Links count the edges by type.
An edge takes its type from its ends when it is made, and becomes a previous collaborator edge when the same pair meets again in a later team.
Giant Component Share is the fraction of nodes in the largest connected component.
Mean Component Size is the node count divided by the number of components, with an isolated node counted as a component of its own.
These two are recomputed when a snapshot is published, and only if the graph has changed since the last one.

Team Assembly declares no actions.

The model differs from NetLogo's in these ways.

- NetLogo's setup is a single team.
  A node count equal to `team_size` gives the same start, as the defaults do.
- `max_downtime` reaches 1,000,000.
  NetLogo's slider stops at 100.
- When a member is to be an incumbent and no node is left outside the team, a newcomer joins instead.
  NetLogo stops with an error there.
- Newcomers are placed around the team's first incumbent, or around the centre of the world if the team has none.
  NetLogo creates them at the origin and leaves the rest to its layout.
- The layout runs in the app, outside the model, and positions are for drawing only.
  NetLogo's `go` runs its layout as part of each tick while `layout?` is on.
- Every node is drawn at one size.
  NetLogo draws the last team's members at twice the size of the other nodes.

## Overriding a parameter

`--set` takes a parameter id and a value, and can be given more than once.

```bash
cargo run --release -p henad-cli -- ants \
  --set num_agents=1000000 --set world_width=4472 --set world_height=4472 \
  --steps 1000
```

Keep the world area proportional to the agent count and the density holds constant, which leaves two runs at different scales comparable.
