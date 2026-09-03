---
title: CPU Grid model
description: A step-by-step tutorial that builds Conway's Game of Life as a Henad grid model.
icon: material/grid
---

# Writing a CPU grid model

In this tutorial we'll build Conway's Game of Life (Life) on the CPU from scratch, which automatically uses all available cores.

Before starting, you'll need a working checkout of the repository.
See [Installation](../installation.md) for instructions.

## What we will write

A [`GridModel`](../../authoring/grid-models.md) is a [trait](https://doc.rust-lang.org/rust-by-example/trait.html) that describes the update rule for a model over `u8` cells.
We describe what one cell becomes, given its own value and the values of its neighbours, and the Henad engine automatically handles the rest of the simulation under the hood.

``` mermaid
flowchart LR
    I["<code>fn init</code><br>configure initial state"] --> S
    S["<code>fn step_cell</code><br>once per cell"] -->|each tick| W["Henad engine handles tick update"]
    W --> S
    S -.->|on publish| T["<code>fn stats</code>"]
```

`init`, `step_cell` and `stats` are the pieces we need to write ourselves.

The Henad engine handles the rest of the simulation, such as the second grid buffer and the swap between them, splitting rows across cores, wrapping at the edges, handing each row its own random number generator, and the snapshot the UI draws.
None of those machinery will appear in our code.

## Update rule `step_cell`

Let's begin by making a file at `crates/henad-models/src/life.rs`.

Let's write down the update rule for a single cell first, since that's the most obvious part of the model.
In Life, the rules are as follows:[^1]

1. Any live cell with fewer than two live neighbours dies, as if by underpopulation.
2. Any live cell with two or three live neighbours lives on to the next generation.
3. Any live cell with more than three live neighbours dies, as if by overpopulation.
4. Any dead cell with exactly three live neighbours becomes a live cell, as if by reproduction.

A cell holds a single `u8`, so we can give the two states names and write the rule as a plain function:

``` { .rust .annotate title="crates/henad-models/src/life.rs" }
const DEAD: u8 = 0;
const ALIVE: u8 = 1;

fn step_cell(cell: u8, neighbors: &[u8]) -> u8 {
    let alive_count: u8 = neighbors.iter().sum();
    match (cell, alive_count) {
        (ALIVE, 2..=3) | (DEAD, 3) => ALIVE, // (1)!
        _ => DEAD,
    }
}
```

1. Both surviving cases fit in this match arm, and everything the match does not name falls through to dead.

This function is, in fact, all of the actual model logic.
We just need a few more pieces to register this model for actual use.

!!! tip

    Each `GridModel` cell has 256 states available, though Life currently only needs two of them.
    A model that needs more than 256 states is better served by an [agent model](ants.md) with a field rather than a grid.

## Implementing `GridModel`

Now that the rule exists, let's actually start to implement the trait.

``` rust title="crates/henad-models/src/life.rs"
use henad_core::authoring::model::grid_model::GridModel;

pub struct LifeModel;

impl GridModel for LifeModel {}
```

Notice that the struct is empty, which is the intended shape for a grid model.
A model in Henad is just const metadata plus pure functions, and the grid data is handled by the engine.

This won't compile yet, because the `impl` block is still empty.
Let's run `cargo check` and see what the compiler says is missing:

``` text title="cargo check -p henad-models"
error[E0046]: not all trait items implemented, missing: `NAME`, `ID`, `DESCRIPTION`, `PALETTE`,
              `NEIGHBORHOOD`, `STATS`, `Params`, `param_descriptors`, `from_params`, `init`,
              `step_cell`, `stats`
  --> crates/henad-models/src/life.rs:6:1
   |
 6 | impl GridModel for LifeModel {}
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ missing 12 items in implementation
   |
   = help: implement the missing item: `const NAME: &'static str = "";`
   = help: implement the missing item: `const NEIGHBORHOOD: NeighborhoodKind = /* value */;`
   = help: implement the missing item: `fn step_cell(_: u8, _: &[u8], _: &<Self as GridModel>::Params, _: &mut u64) -> u8 { todo!() }`
```

12 items sounds like a lot, but many of them are just one line each.
We'll work down the list for the rest of this tutorial.

### Identity

The `impl` starts with the `NAME`, `ID` and `DESCRIPTION` of the model, which are used in the model picker and `henad-cli --list`.

``` { .rust .annotate title="crates/henad-models/src/life.rs" }
impl GridModel for LifeModel {
    const NAME: &'static str = "Game of Life";
    const ID: &'static str = "life"; // (1)!
    const DESCRIPTION: &'static str = "Conway's Game of Life on a toroidal grid"; // (2)!
}
```

1. The ID is the model's handle, and you can run this model with `cargo run -p henad-cli -- [ID]`.
   IDs have to be unique across the registry, and because the default Game of Life already has `game_of_life` by default, we need a different ID while both models stay registered.
2. The description shows up next to the model in the picker, so keep it to one line saying what the model actually is.

### Colours

Next comes `PALETTE`, which the renderer indexes by the cell value itself.
A cell holding `1` is drawn in `PALETTE[1]`, so the order of the palette entries has to match the order we gave `DEAD` and `ALIVE`.
Each entry is four RGBA bytes.

``` rust title="crates/henad-models/src/life.rs"
const PALETTE: [[u8; 4]; 2] = [
    [0x15, 0x15, 0x15, 0xFF], // Dead
    [0x00, 0xE6, 0x76, 0xFF], // Alive
];
```

We placed this outside the `impl` block since later on we can `pub` and then reference it from in the [GPU implementation](gpu-game-of-life.md).
Then we point the trait at the array.

``` rust title="crates/henad-models/src/life.rs"
    const PALETTE: &'static [[u8; 4]] = &PALETTE;
```

### Neighbours

The `NEIGHBORHOOD` const decides how long the slice passed to `step_cell` is, and in what order the neighbours arrive in it.

We will be using the Moore neighbourhood, so the slice passed to `step_cell` will always have eight entries.

``` rust title="crates/henad-models/src/life.rs"
    const NEIGHBORHOOD: NeighborhoodKind = NeighborhoodKind::Moore;
```

A model can index the slice by position, and `neighbors[i]` will always be the same neighbour position (numbered `i`) as shown below.
Life doesn't need to know which neighbour is which, only how many are alive, but any anisotropic model certainly will.

=== "Moore"

    The Moore neighbourhood contains all eight surrounding cells, which is exactly what Life needs.

    ``` text
    0 1 2
    3 · 4
    5 6 7
    ```

=== "Von Neumann"

    The Von Neumann neighbourhood contains only the four orthogonal cells, in the same reading order.

    ``` text
    · 0 ·
    1 · 2
    · 3 ·
    ```

While we're here, let's add the import this needs:

``` rust title="crates/henad-models/src/life.rs"
use henad_core::topology::NeighborhoodKind;
```

### Update rule

We already wrote `step_cell` at the top of the page, but the trait's version takes two more arguments.
Let's move the function into the impl block and extend its signature:

``` { .rust .annotate title="crates/henad-models/src/life.rs" }
    fn step_cell(cell: u8, neighbors: &[u8], _params: &(), _rng: &mut u64) -> u8 { // (1)!
        let alive_count: u8 = neighbors.iter().map(|&n| n & 1).sum();
        match (cell, alive_count) {
            (ALIVE, 2..=3) | (DEAD, 3) => ALIVE,
            _ => DEAD,
        }
    }
```

1. Life reads neither of the new arguments, hence the underscores. We'll cover `params` [below](#parameters), and `rng` is a random number generator private to this row and this tick, which SIR draws from twice per cell.

Two associated items are required, but we won't implement this in this tutorial.

``` rust title="crates/henad-models/src/life.rs"
    type Params = ();

    fn from_params(_params: &[ParamValue]) {}
```

??? tip "Details of `type Params` and `fn from_params`"

    These two items exist for models whose rule reads parameters *while the simulation runs*.

    Notice that `step_cell` receives a `&Self::Params` rather than the raw `&[ParamValue]` slice.
    Reading the slice directly would mean matching a `ParamValue` enum inside a loop that runs once per cell, millions of times per tick.
    Instead, `from_params` runs once per tick, extracts everything the rule needs into a plain struct, and every cell of that tick shares the result.
    Because the extraction happens every tick, a live slider edit reaches the very next tick with no extra work from the model.

    The struct is also the home for anything the rule would otherwise recompute per cell, such as a squared radius or a reciprocal.

    The default SIR model has a concrete example.
    It declares a `SirParams` struct holding its two probabilities and extracts them like this:

    ```rust title="crates/henad-models/src/sir.rs"
    --8<-- "crates/henad-models/src/sir.rs:from_params"
    ```

    Life needs none of this, because its rule reads no parameters at all: the one parameter we add [below](#parameters) is only read by `init`, once, when the grid is built.
    So `type Params = ()` and an empty `from_params` are this model's finished form rather than a stub we are leaving for later.

    The [Parameters](../../authoring/parameters.md#hot-parameters) page covers the mechanism in full.

!!! warning "`step_cell` has to be pure"

    The function runs on every core at once, on a cell whose neighbours are being stepped at the same moment.
    Reading anything outside its four arguments, or writing anything at all, can introduce undefined behaviour and race conditions.

    Notice also that nothing we wrote wraps or bounds-checks a coordinate.
    The engine automatically hands the correct slice of neighbours, and the grid is toroidal so that every cell has eight neighbours.

### Finishing up

We just need to implement `init`, `stats` and `param_descriptors`, which are all straightforward.
The first is `init`, which fills the grid before tick 0, and starting with about a third of the cells alive gives Life a reasonable opening state:

``` { .rust .annotate title="crates/henad-models/src/life.rs" }
    fn init(grid: &mut Grid2D<u8>, _params: &[ParamValue], rng: &mut u64) {
        let threshold = (0.3 * u32::MAX as f32) as u32; // (1)!
        for cell in grid.current_mut().iter_mut() {
            *cell = if below(next_bits(rng), threshold) { ALIVE } else { DEAD }; // (2)!
        }
    }
```

1. We turn the probability into a `u32` threshold once, outside the loop, because comparing integers rather than floats keeps a seeded run identical on a machine that rounds differently.
2. `next_bits` advances the generator and returns 32 fresh bits, and `below` performs the Bernoulli trial on them. See [primitives reference](../../reference/primitives.md) for more details.

The [statistics](#statistics) can also wait until later in the tutorial, so for now we just return an empty list and an empty vector.

``` rust title="crates/henad-models/src/life.rs"
    const STATS: &'static [StatDescriptor] = &[];

    fn stats(_grid: &Grid2D<u8>) -> Vec<StatValue> {
        Vec::new()
    }

    fn param_descriptors() -> Vec<ParamDescriptor> {
        Vec::new()
    }
```

Once we add the imports these functions need, the file compiles:

``` rust title="crates/henad-models/src/life.rs"
use henad_core::authoring::primitives::rng::{below, next_bits};
use henad_core::grid::Grid2D;
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::view::{StatDescriptor, StatValue};
```

## Running it

The model compiles, but the app can only pick models it finds in the registry, so we have to register it.
First we declare the module,

``` rust title="crates/henad-models/src/lib.rs"
pub mod life;
```

then we add a line to the registry, next to the four entries already there:

``` rust title="crates/henad-models/src/registry.rs"
register_grid_model::<crate::life::LifeModel>(),
```

For context, here is the list our line joins:

``` rust title="crates/henad-models/src/registry.rs"
--8<-- "crates/henad-models/src/registry.rs:cpu_entries"
```

`register_grid_model` type-erases the model into a `ModelEntry`.
The name, description, parameters, stat series and topology are all read back off the trait, so an entry carries nothing hand-written that could go wrong.

With the entry in place, we can finally run the model.
Make sure that `--release` is present to reach full performance.

=== "Desktop app"

    ``` bash
    cargo run --release --bin henad-app
    ```

    Our model shows up as the second Game of Life in the picker.
    See [App tour](../app.md) for a quick overview of the UI.

=== "Headless"

    ``` bash
    cargo run --release -p henad-cli -- life --steps 500 --reps 3
    ```

    This runs with no rendering, no sim thread and no pacing, so the reported number measures nothing but `step()`.
    Add `--set grid_width=4096 --set grid_height=4096` to see the model at scale, which reaches around 950 steps a second on a 24-core desktop.

=== "Browser"

    ``` bash
    ./scripts/build_web.sh serve --release
    ```

    Then open `http://localhost:8080`.
    See [App tour](../app.md) for a quick overview of the UI.

You should see gliders crawling across the viewport.
We are now good to implement the missing features: a way to change the starting density without editing the source, and a chart.

## Parameters

Let's deal with the density first.
To make it adjustable, we hoist the hard-coded `0.3` out of `init` and declare it as a parameter:

``` rust title="crates/henad-models/src/life.rs"
henad_core::params! {
    const DENSITY = f32_param("density", "Initial Density", 0.3, 0.0, 1.0, Some(0.01)).on_reload();
}
```

The macro generates two things: a `descriptors()` function returning the list in declaration order, and a `const` per entry holding that entry's index.

`f32_param` takes the ID, the label the UI shows, then the default, the minimum, the maximum, and an optional slider step.

The `.on_reload()` marks the parameter as taking effect only on the next reload.
Because `init` is the only code that looks at the density, dragging that slider on a running grid could achieve nothing.
This will affect its appearance in the UI.

Next we forward the descriptors and read the value in `init`:

``` rust title="crates/henad-models/src/life.rs" hl_lines="2 6"
    fn param_descriptors() -> Vec<ParamDescriptor> {
        descriptors()
    }

    fn init(grid: &mut Grid2D<u8>, params: &[ParamValue], rng: &mut u64) {
        let density = extract_f32(params, DENSITY, 0.3);
        let threshold = (density * u32::MAX as f32) as u32;
        for cell in grid.current_mut().iter_mut() {
            *cell = if below(next_bits(rng), threshold) { ALIVE } else { DEAD };
        }
    }
```

`f32_param` and `extract_f32` both come from `henad_core::helpers`.

Grid width and height belong to every grid model, so the engine prepends them rather than making each model declare its own.
An operator sees the composed list.
Here it is for the default Game of Life, which declares exactly what we just wrote:

``` text title="cargo run -p henad-cli -- game_of_life --params"
parameters for game_of_life (Game of Life):
  index=0 id=grid_width kind=u32 default=1024 min=1 max=10000 apply=reload label="Grid Width"
  index=1 id=grid_height kind=u32 default=1024 min=1 max=10000 apply=reload label="Grid Height"
  index=2 id=density kind=f32 default=0.3 min=0 max=1 apply=reload label="Initial Density"
```

!!! note "Indexes looking weird?"

    You might notice that density sits at index 2 in that list while `DENSITY` reads 0, and both are right.
    The engine hands `init` and `from_params` the model's _own_ slice, starting after whatever got prepended, so `#!rust extract_f32(params, DENSITY, 0.3)` reads the correct value either way.
    This also means your indices cannot shift under you.

## Statistics

The statistics is the last missing piece.
`STATS` declares the series once, and `stats` returns bare numbers in that same order, which keeps labels and colours in one place so that a series cannot end up mislabelled.

``` rust title="crates/henad-models/src/life.rs"
    const STATS: &'static [StatDescriptor] = &[StatDescriptor::new("Alive", PALETTE[1])];

    fn stats(grid: &Grid2D<u8>) -> Vec<StatValue> {
        vec![StatValue::Scalar(count_alive(grid.current()) as f64)]
    }
```

Good news is that the engine provides us with a parallel reduction primitive `reduce_chunks`, so we can count the alive cells in parallel without writing any threading code ourselves.

``` { .rust .annotate title="crates/henad-models/src/life.rs" }
fn count_alive(cells: &[u8]) -> u64 {
    reduce_chunks(
        cells.len(),
        STATS_CHUNK, // (1)!
        |r| cells[r].iter().filter(|&&c| c == ALIVE).count() as u64, // (2)!
        |a, b| a + b, // (3)!
        0,
    )
}
```

1. `STATS_CHUNK` is the number of cells per chunk in a reduction, 8192, shared by every model so that nobody has to pick a number.
2. This closure maps one chunk, in parallel with every other chunk. It takes a range rather than a slice so that a caller can read several lanes per chunk.
3. This is the fold, applied **in chunk order** rather than completion order. Counting integers would survive any order, but a float sum would not, and numbers that shift with rayon's scheduling are not reproducible.

``` rust title="crates/henad-models/src/life.rs"
use henad_compute::cpu::primitives::chunked::{STATS_CHUNK, reduce_chunks};
```

Before we move on, be aware that `stats` runs when a snapshot is published rather than on every tick, so this likely runs at a much lower frequency than `step_cell`.

## Testing

To convince ourselves the rule is right, let's write a test.
A [blinker](https://conwaylife.com/wiki/Blinker) is three cells in a row, and it rotates every tick before coming back to its original shape after two.
That small pattern can catch a wrong neighbour order, a missing wrap and a swapped buffer all in one go.

``` { .rust .annotate title="crates/henad-models/src/life.rs" }
#[test]
fn a_blinker_rotates_and_comes_back() {
    use henad_compute::cpu::grid_engine::GridModelState;
    use henad_core::model::SimState as _;

    // A 5x5 grid, so the pattern stays clear of the wrap.
    let params = vec![ParamValue::U32(5), ParamValue::U32(5), ParamValue::F32(0.0)]; // (1)!
    let horizontal = {
        let mut cells = vec![DEAD; 25];
        cells[11] = ALIVE;
        cells[12] = ALIVE;
        cells[13] = ALIVE;
        cells
    };
    let vertical = {
        let mut cells = vec![DEAD; 25];
        cells[7] = ALIVE;
        cells[12] = ALIVE;
        cells[17] = ALIVE;
        cells
    };

    let mut state = GridModelState::<LifeModel>::from_cells(&params, &horizontal) // (2)!
        .expect("the cell buffer matches the declared grid size");

    state.step();
    assert_eq!(
        state.grid_view().expect("grid view").cells,
        &vertical[..],
        "the blinker did not rotate on the first tick"
    );

    state.step();
    assert_eq!(
        state.grid_view().expect("grid view").cells,
        &horizontal[..],
        "the blinker did not come back on the second"
    );
}
```

1. This is the full parameter list, so width and height come first. Density is 0 because `from_cells` never calls `init`.
2. `from_cells` builds a state from an exact cell buffer rather than from a seed, so a test can start from a pattern you drew by hand.

A pattern touching the edge exercises the wrap instead, and that behaviour belongs in a case of its own.

Registering the model also opted us into the registry tests, which check that a model's declared parameters, topology and stat series match what its state actually produces.

## The finished file

Here is everything we wrote on this page, gathered into one file.

??? example "`life.rs` completed"

    ``` rust
    --8<-- "crates/henad-models/src/tests/tutorial/life.rs"
    ```

The listing above is stored in the repository at [`crates/henad-models/src/tests/tutorial/life.rs`](https://github.com/micfong-z/henad/blob/master/crates/henad-models/src/tests/tutorial/life.rs).

The actual default model is at [`crates/henad-models/src/game_of_life.rs`](https://github.com/micfong-z/henad/blob/master/crates/henad-models/src/game_of_life.rs).
It runs under its own ID, with a `pub` palette that its GPU port reuses.

Notice that we only wrote **49** lines for the entire model (excluding imports and the test, and still a lot of lines only contain a single curly brace).
The reason is everything the engine handled on our behalf:

- Grid allocation, the second buffer, and the pointer swap between them.
- The parallel split, which hands rows to rayon, natively and on the web alike.
- Toroidal wrapping, on both axes.
- Random number seeding, derived per row and per tick so that results never depend on the thread count.
- Parameter storage, and rejecting an edit to a reload-only parameter.
- The display texture, the history chart, snapshot publishing, and the sim thread that keeps stepping off the UI thread.

## Next

The [CPU Agent model](ants.md) tutorial builds a population of agents that moves through space and lays trails on a grid underneath itself.

[^1]: Johnston, Nathaniel; Greene, Dave (2022). _[Conway's Game of Life Mathematics and Construction](https://conwaylife.com/book/conway_life_book.pdf)_ (PDF).
