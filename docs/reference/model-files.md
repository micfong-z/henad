---
title: Model files
description: The model file format, a grid rule, a graph model or a sheet as one TOML file, and the template a grid rule's WGSL step is written against.
icon: material/file-code
---

# Model files

A model file holds one model as one TOML file, and its `kind` key names the kind of model:

- a **grid rule** (`gpu_grid`), a GPU grid model written as data,
- a **graph model** (`graph`), the node graph the Editor writes, described under [Fields of a graph model](#fields-of-a-graph-model),
- a **sheet** (`sheet`), a model built on the sheet and its cards without nodes, described under [Sheets](#sheets).

Most of this page describes grid rules.
A grid rule's TOML declares the model's grid, states and parameters, and a WGSL function gives the next state of one cell.
Henad reads the file while the program runs, composes the step after an engine template, checks it, and runs it on the same GPU grid engine as a compiled GPU grid model.
Nothing is built with cargo.

A model file holds data and WGSL alone, and runs no native code.
The WGSL runs on the GPU inside the checks of naga and wgpu.
The checks below refuse loops and bound the work of one cell, so every step ends.
A heavy step on a large grid can still take long enough per tick to lose the GPU device, as [Limits](#limits) describes.

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
| `kind` | `"gpu_grid"` for a grid rule. `"graph"` and `"sheet"` name the other two kinds |
| `id` | Model id, a lowercase ASCII letter, then lowercase letters, digits and underscores. The command line's `MODEL`, a spec's `model`, `--list` and results folders name the model by it |
| `name` | Name the app and `--list` show. The id when left out |
| `description` | One line about the model. Empty when left out |

The name and the description hold no control character, such as a tab or an escape, and neither do a state's label, a parameter's label or an option name.

The three header keys are read before anything else.
A file of another format, version or kind is refused at that key, even when it holds keys this version does not know.
A refused kind's message lists the three kinds this version reads.

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
A grid rule refuses the keys `hidden` and `tick_unit`, which only a graph file reads, at the table's line.

#### Time scale

A graph file, the other kind of model file, reads its `[[param]]` tables with the keys above and two more.

| Key | Value |
|---|---|
| `hidden` | `true` marks the parameter hidden, as `--params` shows it. `--set`, a sweep and a spec file still reach it. Written only when `true` |
| `tick_unit` | Unit of one tick: `"second"`, `"minute"`, `"hour"`, `"day"`, `"week"`, `"month"` or `"year"`. Set on the one parameter that holds the length of one tick |

The parameter with a `tick_unit` is the model's tick length, as a sheet with rates lowers its Time line.
It is an `f32` with a `min` above 0 and `hidden = true`, and a file sets `tick_unit` on one parameter at most.
A month is 30.44 days and a year 365.25 days.
`--export-stats`, a sweep's `series.csv` and the app's Export tab write a `time` column after `tick`, the tick times the run's tick length, and `--params --json` names the unit under `time`.
A tick length is a parameter like any other in the code identity and the schema, and a sweep over it, such as `--vary tick_length=0.25:1:0.25`, compares one model at several tick lengths.
The app leaves a hidden parameter out of its Parameters tab and offers it in the Sweep tab, and its tick counter and plots read in the unit of one tick.
Neither key changes the code of a file that leaves both out.

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
| A write through a pointer, or a pointer passed to a function, into memory the cell does not own | A cell writes only its own result, its locals and the step's `private` variables |
| A name the template declares, or one that starts with `henad_` | The template's own lines use them |
| A module-scope item named as a WGSL built-in, such as `fn min` | The template's own lines call the built-in |
| A `#` line other than `#import`, a line that opens with `enable`, `requires` or `diagnostic`, `override`, `virtual`, and a `::` path outside an `#import` | Henad composes the step itself, and would otherwise read these differently from WGSL |
| A vertical tab, a form feed, U+0085, U+2028 or U+2029 | WGSL ends a line at each of them, and Henad's composer and checks end a line only at a line feed or a carriage return |
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

## Fields of a graph model

A file of kind `graph` holds a graph model, which the Editor writes in its canonical form.
The types of [`henad-graph`](https://docs.rs/henad-graph) document its tables.
A graph model of agents can hold `[[field]]` tables, layers of values that tile the world.
A tick pass reads a field with `field.get` and deposits into it with `field.deposit`.

| Key | Value |
|---|---|
| `id` | Id of the field |
| `type` | `"f32"`, or a category as `"cat:<id>"` for a static field |
| `layers` | Names of the layers, `["value"]` when left out |
| `static` | `true` for a field that only init passes write and that takes no deposits |
| `combine` | Rule that merges the deposits of a field that is not static: `"max"` or `"sum"` |
| `resolution` | Step that a `"sum"` counts deposits in, a power of two from 1 down to 2^-24 written as a number, such as `1.52587890625e-5` for 2^-16. Required with `"sum"`, and refused with any other rule |
| `finish` | Id of a function run on each cell after the merge. Optional |
| `initial` | Value of every cell at the start. When left out, 0 or the category's first variant |

A deposit below 0 counts as 0, and so does a NaN.
After the last tick pass of each tick, the field update merges the deposits into the field, then runs `finish` on each cell.
With `"max"`, a cell becomes the larger of itself and its largest deposit.
With `"sum"`, each deposit is rounded half to even to a whole number of resolutions, the numbers add up in one 32-bit word per cell, and the cell becomes itself plus the word times the resolution.
Whole numbers add up in any order to the same word, so a run replays exactly on one device and the word on the device equals the word the host evaluator computes.
A total under 2^24 resolutions merges exactly.

The word wraps once a cell takes 2^32 times the resolution in one tick, 65536 at 2^-16.
A deposit whose amount is a number or a parameter reaching that bound is refused when the file is checked, with the bound in the message.
Any other deposit is not checked against it, so a model whose cells can take that much in a tick needs a coarser resolution.

A resolution that is not a power of two, such as 0.001, is refused at its line, with the nearest allowed value in the message.
A power of two keeps the products of the deposit and the merge exact on every device, including one that compiles with fast math.

## Results across devices

A graph model or a sheet on a grid replays exactly with one seed on one device.
A model of agents replays exactly too, unless a look at the agents around adds up floats or depends on the order of its visits.
In a graph, that look is an `agent.radius_fold` in a tick pass whose body holds a `carry`, adds up a float accumulator with `sum`, or reads the fold's `k` or a `prev.<acc>`.
The device's index lists the agents of each cell in the order its atomic writes resolve, and that order can change between two runs.
A float sum over the same agents in another order can round to another result, and two runs at one seed can then differ.
The boids sheet and node graph starter, and the bounded confidence sheet, average over the agents around them and do not replay exactly.
Every other sheet and node graph starter replays exactly.

A sweep's manifest records `replays_exactly` as `false` for such a model.
In the desktop app, [**Compare runs**](../guide/editor.md#converting-to-a-node-graph) names no tick of a difference for it, and reads "Bounded confidence does not replay exactly. Two runs with the same seed can differ, so compare Bounded confidence (expert) with Bounded confidence in the plot." for the bounded confidence sheet.
The [Results tab](../guide/app.md#opening-a-run) shows "This model does not replay exactly. The opened run might differ from the recorded run." beside <span class="ui" markdown>:material-play-box-outline: Open</span>, and the Playback tab shows it under the opened run's name.
The [Model tab](../guide/app.md#metadata) shows a cross in its **Exact replay** row for such a model.

In a model that replays exactly, a computation over whole numbers alone gives the same result on every device, and the same as the host evaluator.
Float results can differ in their last places between devices, and between two shapes of one computation on one device, such as a Repeat and the same body written out in copies.
A device may fuse a multiply with an add, or reorder float operations.
A whole number worked out from floats, such as a count of agents within a distance or a state chosen by comparing two floats, can differ wherever those floats differ.

## Sheets

A file of kind `sheet` holds a sheet: the model in the order a reader meets it, with its world, its properties, its parameters, measures, colours and buttons, and a card of lines for each kind of thing.
Henad reads the sheet, checks it, and lowers it to a graph model, which then builds and runs as a graph model does.
This version lowers sheets on a grid, and sheets of one kind of agent in open space, described under [A sheet of agents](#a-sheet-of-agents).
A line this version cannot lower yet is refused at its table with a message that says so.

`crates/henad-graph/sheets/game_of_life.toml` in Henad's repository is Conway's Game of Life as a sheet.
The comment above each card is the card's heading, and the comment above each line is the sentence the line reads as.
The canonical writer writes every comment again from the sheet, and a comment changes nothing the model computes.

``` toml title="game_of_life.toml"
--8<-- "crates/henad-graph/sheets/game_of_life.toml"
```

The command line runs a sheet with [`--model-file`](cli.md#model-files) like any other model file.
A program reads one with `henad::rules::ModelFile::parse`, and `ModelFile::sheet` returns the sheet.

### The format of a sheet

After the header, the `[sheet]` table holds the sheet's `version`, `1` in this version, its `id`, its `name`, an optional `purpose`, and `next_line`, the number the next new line takes.
A sheet of a newer version is refused at its `version`.
The other tables follow in this order: `[world]`, `[time]`, the properties of each kind of thing (`[[cells.property]]` on a grid, an `[[agents]]` table with its `[[agents.property]]` tables in open space), `[[parameter]]`, `[[measure]]`, a colour rule for each kind of thing (`[color.cells]`, or `[color.<kind>]`), `[[button]]` and `[[card]]`.
Each line of a card is a `[card.line.<id>]` table, its id `l` and a number unique in the file.
Every table refuses a key it does not know, at the key's line.

### Time and rates

The `[time]` table sets how long one tick is, a `tick_length` above 0 and a `tick_unit` from `"second"` to `"year"`.
A sheet with a `[time]` table lowers to a graph whose first parameter is the hidden `tick_length`, in the tick's unit, as [Time scale](#time-scale) describes.
Its default is the sheet's length, its `min` a hundredth of it and its `max` 30 times it.
A sweep varies it like any other parameter, and every tick length runs the same code with the same keys.

A rate, `kind = "rate"`, holds a `rate`, its `rate_unit` and an optional `tries`, the count it is taken for each of.
It stands as a line of its own, as the condition of a change or a When, or inside an all of, an any of or a none of.
A rate `r` per unit holds within one tick with the chance 1 − e^(−r Δt), where Δt is one tick counted in the rate's unit, and for each of a count `n` with the chance 1 − e^(−r Δt n).
A rate in another unit than the tick's is multiplied by the ratio of the two units, so a rate per week in a model of days is divided by 7.
A value that reads only parameters and typed numbers is the same for every cell, and the host computes it once per build.
Such a rate with no count keeps its chance to 2^-32, so a rate as small as 1e-8 per tick still holds at its own chance.
Every other rate and every chance is resolved to 2^-24: a chance per tick below about 1e-6 runs measurably off its value, a chance below 2^-24 runs at 2^-24, and such a rate below about 3e-8 per tick never holds.
A rate for each of the cells around reads e^(−r Δt k) for each count `k` from a table the host computes, and the device computes no exponential.
A rate worked out for each cell, or for each of a count of agents, takes one exponential per element.

Two or more Become lines that leave one state of one property at a rate each, the rate standing as the condition or as an item of an all of, compete.
A cell leaves with the chance of the sum of their rates, and each line takes the share its rate gives it, whatever the order of the lines.
The share of the cells each line takes then follows the ratio of the rates at any tick length.
A group whose rates read only parameters and typed numbers, with no count and nothing else in their conditions, keeps each share to 2^-32, as a lone rate with no count does.
Any other group is resolved to 2^-24, and below a sum of about 1e-6 per tick its shares run measurably off the ratio of the rates.
The group draws one number with the key of its highest line, and the other lines keep their keys for when they leave it.
A rate beside a chance from the same state keeps "otherwise", and the higher line wins.
A cell with a sum of rates of 0, such as one with no infected neighbour when every rate is for each infected neighbour, never leaves.

A rate does not run while the sheet has no `[time]` table, and its line reads "Not run yet: set how long one tick is, under World on the sheet".
A rate read from a parameter whose Unit names another unit, such as a Unit of "per day" on a line per week, keeps the line's unit, with a warning at the line.
A button's card takes no rate.

`crates/henad-graph/sheets/sir.toml` in Henad's repository is the SIR epidemic with rates per day and one tick of 1 day.
Its infection line reads "Become Infected from Susceptible, at rate Infection rate per day for each infected neighbour".

### A sheet of agents

A sheet whose `[world]` is `made_of = "agents"` holds one kind of agent, its `count` at the start and its `max_count`, the most a run can hold.
Its Position starts anywhere at random, an Arrow as a random direction, and its Heading follows the direction of its last move.
Its card can look at the agents within a distance, count them or sum up what it sees, then steer a property, keep a value between two limits, and move by an arrow, and a Repeat adds up its moves and steers.
An average, a largest and a smallest of no one are 0, and a Steer towards a value that reads one, directly or through a Calculate, adds nothing while its look finds no one.
A Steer towards a Choose or a Depending on follows the value it gives, so a Choose whose row tests the look's count can give another value while the look finds no one.
A Become or Set inside a Repeat applies when its condition holds at any time round.
Among the changes of one property inside one Repeat, each time round tries them from the top, and the first whose condition holds decides for that time round.
The first time round with a decision is final, so a lower line that held at an earlier time round wins over a higher line that held later.
For example, "Become A from S, with chance 0.1" above "otherwise become B from S, with chance 0.5", repeated 4 times, decides a time round for A with chance 0.1 and for B with chance 0.9 × 0.5 = 0.45.
Then 0.1 × (1 + 0.45 + 0.45² + 0.45³) = 0.174 of the agents in S become A, 0.45 × (1 + 0.45 + 0.45² + 0.45³) = 0.785 become B, and the other 0.041 stay in S.
A Become from a state never decides for an agent in another state.
Between a change inside a Repeat and a change of the same property outside it, or inside another Repeat, the higher line wins.

The agents can walk on a ground of cells, one per unit of the world.
A ground property holds states painted at the start, or a number that agents mark with Leave, keeping the largest mark or adding the marks up, and that fades by its share kept per tick, from 0 to 1.
A property that adds up its marks rounds each mark to a multiple of 2^-16, so a mark below 2^-17 adds nothing, and one cell adds up at most 65536 in a tick and starts again from 0 past it.
A Leave of a number or a parameter on such a property warns at its line when the kind's `max_count` agents, all on one cell, can leave more than that in a tick.
A card can pick the open cell around an agent with the most or least of a ground property, or one at random, look at the cell here or ahead, read a property of a place, move to a place, leave a mark and record an event that a measure counts.
Cells in a blocked state of the ground are not open.
The first Move to whose condition holds decides, and an agent whose place there is not open stays where it is.
The looks and picks of a card over one neighbourhood share one walk of the cells around.
Agents that move to places can live in a world with walls, and other worlds wrap at their edges.
In a world with walls a look at agents sees no one across an edge, and its offset and distance to them do not wrap.

`crates/henad-graph/sheets/random_walk.toml` is a random walk as a sheet.
`boids.toml` and `bounded_confidence.toml` beside it are flocking and opinion dynamics, and `ants.toml` is ant foraging over a painted ground with two trails.

``` toml title="random_walk.toml"
--8<-- "crates/henad-graph/sheets/random_walk.toml"
```

The looks at agents of one card share one walk of the agents around each agent, at the largest of their distances, and the neighbour index takes its size from the largest distance as the parameters stand.
A look past the most agents one agent can visit stops the run with a message that names the look's line, such as "line 1, Flockmates".
A program asks `SheetFile::shortfalls` whether a population fits the work one run may take, and a model that does not fit is named the same way.
A larger world spreads the same agents thinner and a look meets fewer of them, so a population refused in the declared world can fit in a wider and taller one.

This version refuses a second kind of agent, walls around agents that move by an arrow, a blocked ground under agents that move by an arrow, the cell ahead of agents that move by an arrow, a start on a ground state painted larger than one cell, and Keep between, When, Repeat, Leave, Record and Move to inside a Repeat, each at its table.
A card changes a property by Set or by Steer, and moves by an arrow or to a place.
A line that changes a property the other way is refused with a message naming the line that came first.

### Problems at their lines

Every problem is reported at the table of the sheet it is about, and a line's problem at its `[card.line.<id>]` table.
A warning keeps the model running, and a line with a blank slot is left out of the model with a warning that says what to fill in.
A sheet with no property reads with a warning at `[world]`, and is refused at the same table with the same words when it builds.
The lowered graph is checked as well, and a problem found there is placed at the table of the line its node was lowered from, or at `[sheet]` when no line owns the node.
Such a problem carries no node or pass of the graph.
Its message keeps the wording of the graph's checks, since it points to a fault in the lowering more than to a mistake in the sheet.
[`--print-lowered`](cli.md#model-files) prints the lowered graph for a reader who wants to see it.

### Code identity of a sheet

The code of a sheet is the code of the graph it lowers to, the graph's semantic text in the format `henad-graph/1`.
A sweep's manifest records that text as `model.code`, so a results folder replays without reading the sheet.
The sheet's name, the ids of its parameters, states, properties, cards, lines and buttons, the names of its states, parameters and measures, its parameter defaults, bounds and steps, its colours and the keys of its draws enter the code through the graph.
A comment, a note, the `purpose`, a parameter's unit and meaning, a measure's id and the name of a result line stay outside it, and so does a new order of two lines that cannot change the result, such as two Steers of one property.
The draws of a button's press also depend on the button's place among the sheet's buttons, so a button added or deleted above it changes the numbers its presses draw, while the keys of its lines stay the same.
A [resume](cli.md#resuming) of a sheet's sweep is refused when the code differs, as for any model file.

### A sheet converted to a graph model

A program converts a sheet to a graph model with `henad::graph::sheet::convert`.
The conversion runs one way and leaves the sheet as it is.
It returns a new file of kind `graph` holding the graph the sheet lowers to, under the sheet's id with `_expert` added and its name with " (expert)" added, numbered when a model already uses them.
Every node keeps its id and the key of its draws, so the graph model runs tick for tick like the sheet at one seed.

The converted graph model has its own code.
Its id, name and description enter its semantic text, so its code differs from the sheet's before any edit, and a sweep of it is a sweep of another model.
The `origin` record of its `[editor]` region names the sheet: `code` is the sheet's code, and `copy_code` the graph model's own code at conversion.
`kind`, `model`, `name` and `file` name the sheet and its file, `hash` is the FNV-1a hash of the sheet's canonical text, and `converted` the date.
The app lists the changes of a copy only while the sheet's code still hashes to `code`, so an edit of a note on the sheet leaves them listed.
Like the rest of the region, the record stays outside the code.

Each `[[editor.frame]]` of the converted file holds the nodes of one line, the nodes several lines share, or the nodes of a part of the sheet such as a start or a measure.
Its `line` key, or its `lines` key for nodes several lines share, records the line ids, or the parts of the sheet, the nodes came from.
A sheet with an error, or with a line that does not run yet, does not convert.

### The sheet in a results folder

A sweep of a sheet also records the sheet's canonical text in its manifest, as `model.author`, beside the lowered graph's `model.code`.

``` json
"author": {
  "format": "henad-sheet/1",
  "fnv1a64": "…",
  "text": "format = \"henad-model\"\n…"
}
```

The sheet's text stays outside the code, so a note or a comment edited between two sessions of one sweep keeps the folder resumable.
The manifest then holds the sheet as the latest session ran it.
A merge keeps the sheet text of the lowest shard.
A replay runs the graph that `model.code` records and never reads the sheet, so a folder still replays when a later Henad reads the sheet differently.
In the desktop app, <span class="ui" markdown>:material-pencil-outline: Open in Editor</span> in the [Results tab](../guide/app.md#opening-results) opens the recorded sheet, and opens the recorded graph when the folder records no sheet.
A program reads the text with `ResultSet::author`.
A folder that a model file of another kind or a compiled model wrote leaves `model.author` out.

### Limits of a sheet

| Limit | Value |
|---|---|
| Text of the sheet | 1 MiB |
| Lines of one card, the lines inside When and Repeat lines included | 256 |
| Tables and lists nested inside one another | 24 deep |
| Groups nested in one formula | 32 deep |
| Operations nested in one formula | 16 deep |
| An id | 48 bytes |
| Nodes the lowering writes for one line | 999 |
| Nodes the lowering writes for the sheet outside every line | 999 |
