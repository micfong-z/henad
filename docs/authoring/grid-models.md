---
title: Grid models
description: Writing a cellular automaton over u8 cells with the GridModel trait.
icon: material/grid
---

# Grid models

_See [Writing a CPU grid model](../guide/first-model/game-of-life.md) for a tutorial._

`GridModel` is the authoring trait for a cellular automaton, a grid of `u8` cells where every cell advances by one rule per tick.
It fits a world made of small integer states, each cell updating from what surrounds it.
You implement `init`, `step_cell`, `stats` and the consts, and the engine supplies everything else, including the parallel row-wise step.

```rust
--8<-- "crates/henad-models/src/game_of_life.rs:step_cell"
```

Read `game_of_life.rs` and `sir.rs` alongside this page.
Game of Life is the smaller of the two and draws no random numbers during a step, which makes it the easier one to copy from when you begin.

## Items you supply

Everything in the table below comes out of your impl.

| Item | Role |
|---|---|
| `NAME`, `ID`, `DESCRIPTION` | Identity, read by the app and by `henad-cli --list` |
| `PALETTE` | One RGBA colour per cell value. See [palettes and views](views.md) |
| `NEIGHBORHOOD` | `Moore` or `VonNeumann`, deciding the neighbours `step_cell` receives |
| `STATS` | The series the history chart plots. See [statistics](statistics.md) |
| `type Params` | Hot parameters, rebuilt once a tick |
| `param_descriptors` | This model's own parameters. See [parameters](parameters.md) |
| `from_params` | Extracts `Params` from a value slice |
| `init` | Fills the grid, given the parameters and a seed |
| `step_cell` | The rule |
| `stats` | The reduction, in `STATS` order |

## The rule

```rust
fn step_cell(cell: u8, neighbors: &[u8], params: &Self::Params, rng: &mut u64) -> u8;
```

`step_cell` receives its neighbours already gathered, in the order your declared `NEIGHBORHOOD` fixes.
Keep the function pure apart from the `rng` it is handed.
The engine steps rows in parallel and makes no guarantee about which row lands on which core.

Neighbours arrive row-major, with `dy` on the outer axis and `dx` on the inner.
This ordering is published API, and a test asserts it against the tables in [authoring primitives](../reference/primitives.md).

=== "Moore, 8 neighbours"

    ```text
    0 1 2      (-1,-1) ( 0,-1) (+1,-1)
    3 . 4      (-1, 0)         (+1, 0)
    5 6 7      (-1,+1) ( 0,+1) (+1,+1)
    ```

=== "Von Neumann, 4 neighbours"

    ```text
    . 0 .      ( 0,-1)
    1 . 2      (-1, 0)         (+1, 0)
    . 3 .      ( 0,+1)
    ```

`dy` runs south, matching the display's downward y axis.
The grid is a torus on both axes, and the engine wraps every coordinate before your rule runs, so `step_cell` never sees an edge.

## Hot parameters

Your rule reads a `&Self::Params`.
Handing it the raw value slice would put a `ParamValue` match inside a loop that runs once per cell, millions of times a tick, and `from_params` avoids that by running once at the start of a tick.
Every cell of that tick then shares the result.

```rust
--8<-- "crates/henad-models/src/sir.rs:from_params"
```

A model with nothing to extract, such as Game of Life, sets `type Params = ()` and writes an empty `from_params`.
Anything a rule would otherwise recompute per cell also belongs here, such as a squared radius or a reciprocal.

## Width and height

The engine prepends grid width and height at indices 0 and 1, and no grid model declares them itself.
Its own parameters start at index 2, and the indices `params!` generates are already relative to the model's own slice.

`init` and `from_params` are both handed that own slice.
The engine can then gain a parameter of its own without shifting anything a model reads.

## The initial state

```rust
fn init(grid: &mut Grid2D<u8>, params: &[ParamValue], rng: &mut u64);
```

`init` runs once, sequentially, on the current side of a freshly allocated grid.
The `rng` argument is a plain `u64` xorshift state, advanced through the [random primitives](../reference/primitives.md#random).

A GPU port of the model calls this same function to seed its buffers, which keeps tick 0 bit-identical between the two backends.
See [porting a model to the GPU](porting.md).

## Left to the engine

With the trait implemented, the engine covers the rest:

- Allocates the grid and its second buffer, and swaps the two after every tick.
- Splits the step by row across rayon, on native and on the web alike.
- Wraps both axes, peeling the x wrap off the row loop so the interior runs without a modulo.
- Seeds an RNG per row per tick from the row index, never from anything a worker mutates.
- Stores the parameters, and rejects any edit to a reload-only one.
- Builds the grid view, the display texture, the history chart and the snapshots.

## Next

- [Writing a grid model](../guide/first-model/game-of-life.md) builds one from an empty directory.
- [Statistics](statistics.md) explains the reduction and why it runs on publish rather than every tick.
- [GPU grid models](gpu-grid-models.md) covers the same topology written in compute shaders.
