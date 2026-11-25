---
title: Benchmarks
description: What the shipped models reach, on what hardware, and how it was measured.
icon: material/speedometer
---

# Benchmarks

Here is the performance of Henad measured against other ABM frameworks, on the grid and agent [models](reference/models.md) shipped with Henad.
The following common frameworks for ABM are compared:

- [MASON 22](https://people.cs.gmu.edu/~eclab/projects/mason/) - a multiagent simulation library written in Java
- [Agents.jl](https://juliadynamics.github.io/Agents.jl/stable/) - an agent-based modeling framework in pure Julia
- [NetLogo](https://www.netlogo.org) - a multi-agent modeling software
- [Mesa](https://mesa.readthedocs.io/latest/) - an agent-based modeling framework in Python
- [krABMaga](https://krabmaga.github.io/) - a discrete events simulation engine for agent-based modeling in Rust

All data are tested on one rented machine, with a 16-core **AMD Ryzen 9 9950X3D** CPU (32 threads), 60 GB of RAM and an **NVIDIA RTX 4090** GPU, at Henad commit [`885a0d2`](https://github.com/micfong-z/henad/commit/885a0d2) on 25 November 2026.
The machine's container allowed 31 of the 32 threads, so Henad's all-cores rows ran on 31 threads, and every other engine ran on one.
Every engine ran in the same sweep, with the following population sizes and step counts:

| Model | Population | Steps |
|---|---|---|
| Game of Life, SIR | 64², 128², 256², 512², 1024², 2048², 4096² | 100 |
| Boids | 1k, 3k, 10k, 30k, 100k, 300k, 1M | 100 |
| Ant foraging | 2k, 6k, 20k, 60k, 200k | 200 |

## Results

In the figures below, "over budget" indicates that the engine took longer than 1000 seconds to run the required repetitions.
On the smallest grids a GPU step costs mostly the fixed overhead of submitting work to the device, so a fast CPU engine can beat Henad's GPU row there, as Agents.jl and MASON do on SIR at 64².

### Game of Life

![Seconds for 100 steps](assets/benchmarks/game_of_life_seconds-light.svg#only-light){ loading=lazy }
![Seconds for 100 steps](assets/benchmarks/game_of_life_seconds-dark.svg#only-dark){ loading=lazy }

--8<-- "docs/assets/benchmarks/tables/ratio_game_of_life.snippet"

### SIR

![Seconds for 100 steps](assets/benchmarks/sir_seconds-light.svg#only-light){ loading=lazy }
![Seconds for 100 steps](assets/benchmarks/sir_seconds-dark.svg#only-dark){ loading=lazy }

--8<-- "docs/assets/benchmarks/tables/ratio_sir.snippet"

### Boids

![Seconds for 100 steps](assets/benchmarks/boids_seconds-light.svg#only-light){ loading=lazy }
![Seconds for 100 steps](assets/benchmarks/boids_seconds-dark.svg#only-dark){ loading=lazy }

--8<-- "docs/assets/benchmarks/tables/ratio_boids.snippet"

### Ant foraging

![Seconds for 200 steps](assets/benchmarks/ants_seconds-light.svg#only-light){ loading=lazy }
![Seconds for 200 steps](assets/benchmarks/ants_seconds-dark.svg#only-dark){ loading=lazy }

--8<-- "docs/assets/benchmarks/tables/ratio_ants.snippet"

## Lines of code

How much each model costs to express, counting neither blanks nor comments.
Each column compares the engines on one model.
Henad's files also declare parameters, statistics, actions and a palette, and NetLogo's hold the scenario setup as well as the rule.

--8<-- "docs/assets/benchmarks/tables/loc.snippet"

## Reproducing

See [`benchmarks/`](https://github.com/micfong-z/henad/tree/master/benchmarks) for reproduction scripts and methods.
