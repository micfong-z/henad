---
title: Default models
description: The eight models that ship with Henad, and every parameter each one declares.
icon: material/cube-outline
---

# The models

Eight models ship in the registry.
Four of them run on the CPU, and each of those four has a GPU port running the same simulation entirely in compute shaders.

```bash
cargo run -p henad-cli -- --list
```

| Id | Name | Topology | Backend |
|---|---|---|---|
| `sir` | SIR Epidemic | Grid | CPU |
| `game_of_life` | Game of Life | Grid | CPU |
| `boids` | Boids Flocking | Agents | CPU |
| `ants` | Ant Foraging | Agents over a field | CPU |
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

### Game of Life

Conway's Game of Life on a toroidal grid.

| Id | Kind | Default | Range |
|---|---|---|---|
| `grid_width` | u32 | 1024 | 1 to 10000, or 16384 on the GPU |
| `grid_height` | u32 | 1024 | 1 to 10000, or 16384 on the GPU |
| `density` | f32 | 0.3 | 0 to 1 |

### SIR Epidemic

The classic SIR compartmental model on a 2D grid with a Moore neighbourhood.

| Id | Kind | Default | Range |
|---|---|---|---|
| `grid_width` | u32 | 1024 | 1 to 10000, or 16384 on the GPU |
| `grid_height` | u32 | 1024 | 1 to 10000, or 16384 on the GPU |
| `infection_rate` | f32 | 0.3 | 0 to 1 |
| `recovery_rate` | f32 | 0.05 | 0 to 1 |
| `initial_infected_pct` | f32 | 0.01 | 0 to 1 |

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

## Overriding a parameter

`--set` takes a parameter id and a value, and can be given more than once.

```bash
cargo run --release -p henad-cli -- ants \
  --set num_agents=1000000 --set world_width=4472 --set world_height=4472 \
  --steps 1000
```

Keep the world area proportional to the agent count and the density holds constant, which leaves two runs at different scales comparable.
