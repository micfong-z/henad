---
title: Choosing a trait
description: The four traits a Henad model can implement, and how to choose between them.
icon: material/shape-outline
---

# Choosing a trait

A Henad model is const metadata plus pure functions.
Allocation, double buffering, chunking, RNG seeding, parameter storage, the views and the runner interface all belong to the engine, and your code never has to mention a thread, a buffer or a frame.

Four traits cover the two topologies across the two backends.

|  | CPU | GPU |
|---|---|---|
| **Grid** — cellular automata over `u8` cells | [`GridModel`](grid-models.md) | [`GpuGridModel`](gpu-grid-models.md) |
| **Agents** — a population, optionally over a field | [`AgentModel`](agent-models.md) | [`GpuAgentModel`](gpu-agent-models.md) |

Pick the topology first.
A model holding one value per cell of a fixed lattice is a grid model.
If your state is a population moving through space, you want an agent model, and that still holds when those agents read and write a grid underneath themselves, since a [field](fields.md) covers that case.

Then pick the backend, and start on the CPU.
A CPU model is ordinary Rust, so `dbg!` and a test work on it.
The GPU version is WGSL plus a declared list of passes, and every GPU model in the repository was written after its CPU counterpart already worked.
Each one seeds itself through that counterpart's `init`, which keeps the pair comparable.
[Porting a model to the GPU](porting.md) picks up from there.

!!! note "`SimState` is not a fifth path"

    `Model` and `SimState` belong to the runner, which drives a state through them.
    Implement one of the four traits above and leave `SimState` to the engine.

## The five declarations

Whichever trait you pick, your model supplies the same five things.

**Identity.** The `NAME`, `ID` and `DESCRIPTION` consts, read by the app and by `henad-cli --list`.

**[Parameters](parameters.md).** Descriptors carrying an id, a kind, a default, a range and whether an edit applies live or on reload.
The engine prepends the parameters every CPU model of that topology needs, and a `GridModel` therefore never declares its own width and height.

**[Statistics](statistics.md).** A `STATS` list naming the series the history chart plots, together with a `stats` function that returns bare values in that order.

**An initial state.** An `init` that fills the grid or the lanes from the parameters and a seed.

**A step.** The kernel itself, which stays pure apart from the RNG it is handed.

Every model also declares a `PALETTE`, and [palettes and views](views.md) covers what the renderer does with it.

## Where to start

If you have not written a model before, the two tutorials build one end to end, and you can come back to this section as the reference.

<div class="grid cards" markdown>

-   **[Writing a grid model](../guide/first-model/game-of-life.md)**

    Builds Game of Life, from an empty directory to an entry in the dropdown.

-   **[Writing an agent model](../guide/first-model/ants.md)**

    Builds a foraging population over a pheromone field, the one composite model in the repository.

</div>

## Then

Once your model runs, [register it](registering.md) so that it appears in the app and the CLI, check it against the [determinism contract](determinism.md), and read [writing fast models](performance.md) before you scale it up.
