---
title: Model files
description: The model file format, a GPU grid model as one TOML file, and the template its WGSL step is written against.
icon: material/file-code
---

# Model files

A model file holds a **grid rule**, a GPU grid model written as data.
TOML declares the model's grid, states and parameters, and a WGSL function gives the next state of one cell.
Henad reads the file while the program runs, composes the step after an engine template, checks it, and runs it on the same GPU grid engine as a compiled GPU grid model.
Nothing is built with cargo.

A model file holds data and WGSL alone.
The WGSL runs on the GPU inside the checks of naga and wgpu, and the checks below keep its work bounded, so a model file is safe to share as source.

!!! info "Henad 0.4"

    This page describes Henad 0.4.

The command line loads a model file with [`--model-file`](cli.md#model-files).
A program reads one with `henad::rules::ModelFile::parse` and builds its entry with `ModelFile::entry`, both behind the facade's `model-files` feature, then adds the entry to its set with `ModelSet::replace`.

## An example

`crates/henad-explore/specs/models/life_rule.toml` in Henad's repository is Conway's Game of Life as a grid rule.
At the same grid size, density and seed it steps through the same cells as the example model `game_of_life`.

``` toml title="life_rule.toml"
--8<-- "crates/henad-explore/specs/models/life_rule.toml"
```

## The format

A model file is one TOML file.
Every table refuses a key it does not know, at the key's line.
Line ends can be `\n` or `\r\n`.

### Header

| Key | Value |
|---|---|
| `format` | `"henad-model"` |
| `format_version` | `1` |
| `kind` | `"gpu_grid"`, the one kind this version reads |
| `id` | Model id, a lowercase ASCII letter, then lowercase letters, digits and underscores. The command line's `MODEL`, a spec's `model`, `--list` and results folders name the model by it |
| `name` | Name the app and `--list` show. The id when left out |
| `description` | One line about the model. Empty when left out |

The three header keys are read before anything else.
A file of another format, version or kind is refused at that key, even when it holds keys this version does not know.

### `[grid]`

| Key | Value |
|---|---|
| `width` | Default width of the grid in cells, from 1 to 16384 |
| `height` | Default height of the grid in cells, from 1 to 16384 |
| `boundary` | `"torus"`, where the grid wraps at every edge, or `"bounded"`, where a neighbour past the edge reads as state 0. `"torus"` when left out |

The model's first two parameters are always `grid_width` and `grid_height`, from 1 to 16384, with these defaults.

### `[[state]]`

One table per state, at least one and at most 256.
The states are numbered from 0 in the order of their tables.

| Key | Value |
|---|---|
| `label` | Name of the state. The stat that counts its cells takes the label, and the step names the state by a constant made from it |
| `color` | Colour of a cell in the state, as `[red, green, blue]` or `[red, green, blue, alpha]` bytes. Alpha is 255 when left out |
| `initial` | Id of an `f32` parameter whose value is the state's share of the cells at tick 0. Left out for one state at least |

A state's constant is its label in upper snake case, made of the label's ASCII letters and digits: `Dead` gives `DEAD`, and `Infected cells` gives `INFECTED_CELLS`.
Two states that give the same constant are refused, as is a constant that starts with a digit, names something the template declares, or is a reserved word of WGSL.

At tick 0 the engine seeds the cells on the CPU from the run's seed, one draw per cell in row order.
The states with an `initial` share take their shares of the cells in the order of their tables, and the first state without one takes the rest.
`life_rule.toml` seeds exactly as `game_of_life` does.

### `[[param]]`

One table per parameter, at most 64, in the vocabulary of [`--params --json`](cli.md#parameters-as-json) without its `index`, `apply` and `default_index`.

| Key | Value |
|---|---|
| `id` | Id that `--set`, a spec file and the step use. The same grammar as the model id |
| `label` | Name the app shows. The id when left out |
| `kind` | `"f32"`, `"u32"`, `"bool"` or `"choice"` |
| `min`, `max` | Bounds of an `f32` or a `u32`, both included |
| `default` | Value at the start: a number, `true` or `false`, or a choice's option name |
| `step` | Step of an `f32`'s slider in the app. Optional |
| `options` | Option names of a `choice`, as a list of strings |
| `format` | `"percent"` to show an `f32` fraction as a percentage, or `"plain"`. `"plain"` when left out |

A key that does not fit the parameter's kind is refused, such as `step` on a `u32` or `options` on an `f32`.
The default has to lie within the bounds, and a choice's option names follow the rules of `choice_param`: no empty name, no name that reads as an index, no comma or colon in a name, no name `all`, and no name twice.
A sweep's level text splits at a comma, reads a colon as a range and `all` as every option, and could not list such a name.
An id cannot be `width`, `height`, `seed`, `tick`, `grid_width` or `grid_height`, start with `henad_`, or be a reserved word of WGSL.
Every parameter of a grid rule applies when the model is built.

### `[step]`

| Key | Value |
|---|---|
| `wgsl` | WGSL of the step, between `'''` |

The step is a literal string, and the file holds its text as written.
A problem in the step is then reported at its line in the file.
A string with escapes is refused, as is a step that holds `'''`.

### Code identity

The **canonical text** of a model file is the file written again: every key in a fixed order, every value in one form, and the step as written.
Two files that differ only in comments outside the step, key order, blank lines or the way a number is written, such as `0.30` and `0.3`, have the same canonical text.
A change inside the step, a comment included, or to any value is a change of code.

A sweep's manifest records the canonical text as `model.code`, with its hash.
A [resume](cli.md#resuming), a [search resume](cli.md#resuming-a-search) and a [merge](cli.md#shards-and-merging) of a model file's model are refused when the code differs from the code the folder's runs ran.

## The step

The step defines one function, which the engine calls once per cell and tick:

``` wgsl
fn step_cell(cell: u32, rng: ptr<function, u32>) -> u32
```

`cell` is the cell's state, and the function returns the cell's next state.
A result past the last state counts as the last state.
The step can also define helper functions, constants, structs and `var<private>` variables.

### What the template declares

The engine composes the step after a template.
The template declares the names below, and the step cannot declare them again.

| Name | What it is |
|---|---|
| `params` | The uniform of struct `Params`: `width`, `height` and `seed` as `u32`, then one field per parameter, named by its id |
| `neighbor(dx, dy)` | State of the cell `dx` to the east and `dy` to the south, both offsets `i32`. Past the edge of a bounded grid it reads 0 |
| `moore_count(state)` | Number of the eight cells around the cell that are in `state` |
| `von_neumann_count(state)` | Number of the four cells beside the cell that are in `state` |
| `NUM_STATES` | Number of states, as `u32` |
| `DEAD`, `ALIVE`, ... | One `u32` constant per state, its number |

A parameter's field in `Params` is an `f32` for an `f32` parameter, and a `u32` for every other kind: the value of a `u32`, `1` or `0` for a `bool`, and the option's index for a `choice`.
The step reads a parameter `rate` as `params.rate`.

The template also declares `main`, the one entry point, and names of its own that start with `henad_` or `HENAD_`.
The step cannot declare a name that starts with `henad_` in any case.

### Randomness

`rng` is a random state of its own for each cell and tick, a hash of the run's seed, the cell's index and the tick.
The step draws from it with the functions of the shared `henad::rng` module, such as `next_bits(rng)` and `next_float(rng, max)`, imported with a line such as `#import henad::rng::{next_bits, next_float}`.
[Authoring primitives](primitives.md#random) lists the module's functions.
A run is the same on every replay with its seed.

### Imports

A line `#import henad::<module>::{a, b}` or `#import henad::<module>::item` imports from the [shared modules](../authoring/shaders.md#shared-wgsl).
The modules `henad::rng` and `henad::space` hold most of what a step uses.
An imported function that breaks a rule below is refused at the step's call to it.

### What the engine owns

The engine owns the rest of the model.
It colours each cell by its state's colour, and counts the cells in each state, one stat per state named by the state's label.
A grid rule declares no actions.

## What is refused

The checks run when the file is loaded, before anything is built, and each problem is reported at its line and column in the file when it has one.

| Refused | Reason |
|---|---|
| `loop`, `for` and `while` | A step without loops does bounded work and always ends |
| `@group`, `@binding` and any other resource binding | The engine binds every buffer the step uses |
| An entry point (`@compute`, `@vertex`, `@fragment`) | The template's `main` runs the step |
| Atomics, barriers and module-scope variables outside `private` | Each cell runs on its own, and shares no memory with other cells |
| Any use of `state_in`, `state_out`, `tick_in` or `tick_out` | Other cells write the outputs in the same pass, and a read of the input by index can fall outside the grid. `neighbor` reads the input safely |
| A write through a pointer into memory the cell does not own | A cell writes only its own result, its locals and the step's `private` variables |
| A name the template declares, or one that starts with `henad_` | The template's own lines use them |
| A module-scope item named as a WGSL built-in, such as `fn min` | The template's own lines call the built-in |
| A `#` line other than `#import`, a line that opens with `enable`, `requires` or `diagnostic`, `override`, `virtual`, and a `::` path outside an `#import` | Henad composes the step itself, and would otherwise read these differently from WGSL |
| `f16` and `f64` | A step keeps to the capabilities of the WebGPU baseline. Browsers offer no more |

A problem in the template has no line in the file.
Neither has a problem with the rule as a whole, such as a missing state, or one naga finds after composing, such as an `f64` value. Each names the file alone.
The template's problems come from a step that does not fit it, such as a `step_cell` with another signature, and a note then shows the signature the engine calls.

## Limits

| Limit | Value |
|---|---|
| Text of the step | 64 KiB |
| States | 1 to 256 |
| Parameters | 64, besides `grid_width` and `grid_height` |
| Grid side | 1 to 16384 cells |
| Local variables of one function | 8192 bytes |
| `private` variables of the shader | 8192 bytes, the template's 8 bytes included |
| One value or constant | 8192 bytes |
| One call of `step_cell` | 65536 expressions and statements, counting the body of a called function once per call |

The byte limits are WGSL's own limits. A browser enforces them.
The step of `life_rule.toml` costs under 2048 of the 65536.

A rule binds at most five storage buffers in each pass, within the WebGPU baseline of eight.

The limits bound the work of one cell.
The time of a tick also grows with the grid, and a step near the limit on a large grid can take a long time per tick.
A submission of many such ticks can then trip the operating system's GPU watchdog, and the device can be lost.
