---
title: The command line
description: Every flag the headless henad-cli runner takes, for benchmarks, parameter sweeps and searches.
icon: material/console
---

# The command line

`henad-cli` is a headless benchmark runner that steps a model in a bare loop, with no rendering, no sim thread and no pacing.
A measurement therefore times `step()` and nothing else.
It also runs [parameter sweeps](#sweeps) and [searches](#searches).
A sweep builds a model many times over a set of parameter values, and writes every run's results to a directory.
A search picks its parameter values a batch at a time, from the results of the batches before.

```text
henad-cli [OPTIONS] [MODEL]
```

`MODEL` is a model id, as printed by `--list`.
On a machine without a compute adapter `--list` leaves the GPU models out, and naming one is refused with the message that it needs a GPU.
An id the build does not include is refused as well.
A sweep or search read from `--spec` takes the model from the spec file, and `MODEL` can be left out.

!!! warning "Release mode"

    Always build with `--release`.
    A debug build steps one to two orders of magnitude slower, and its timings mean nothing.

## Flags

| Flag | Default | Effect |
|---|---|---|
| `--list` | | Print the ids of the models this machine can run and exit |
| `--params` | | Print the model's parameters, with kinds, defaults and ranges, and exit. With `--json`, print them as [one JSON line](#parameters-as-json) |
| `--info` | | Print host and GPU details. Without a model or a sweep, prints and exits. Otherwise, prints as a provenance header |
| `--json` | | Emit one JSON object per line instead of the human report, for a driver to parse |
| `--threads <N>` | 0 | Worker threads for CPU models. 0 leaves rayon's own choice, one per logical cpu |
| `--set <ID=VALUE>` | | Override one parameter. Repeatable |
| `--act <ID@TICK>` | | Run one of the model's actions at that tick. Repeatable. In a sweep, every run fires it |
| `--steps <N>` | 1000 | Steps to run and time per rep, or per run of a sweep |
| `--reps <N>` | 1 | Independent timed runs, each on a freshly created state. In a sweep, the replicates of each config |
| `--warmup <N>` | 0 | Untimed steps before each rep, on that rep's own state, to reach a steady sim regime. In a sweep, steps before the first sample |
| `--global-warmup <N>` | 0 | Untimed steps once before the timed reps, to ramp GPU clocks and pay first-use compilation |
| `--seed <SEED>` | model default | RNG seed. In a sweep, the root seed that every run's seed and design seed come from, 0 when left out |
| `--export <PATH>` | | Write the final state after warmup and steps to this path, then exit |
| `--export-stats <PATH>` | | Write the per-tick stat series to this path as CSV, then exit |
| `--stats-every <N>` | 1 | Sample stats every N ticks when using `--export-stats` or sweeping |
| `-h`, `--help` | | Print help |
| `-V`, `--version` | | Print version |

The flags that run a sweep have [a table of their own](#sweep-flags).

`--set` takes a number for a `u32` or `f32` parameter, `true` or `false` for a `bool`, and an option's name or index for a `choice`, as in `--set network=Geometric`.
A parameter that `--params` marks `format=percent` still takes a fraction, and `--set virus_spread_chance=0.1` sets it to 10%.

Both export formats are shared with the app (see [:material-application-export: Export tab](../guide/app.md#export-tab) for details).

## Examples

Time 500 steps of a 4096² Game of Life across three reps:

```bash
cargo run --release -p henad-cli -- game_of_life \
  --set grid_width=4096 --set grid_height=4096 --steps 500 --reps 3
```

Let a GPU model reach its steady state before anything is timed:

```bash
cargo run --release -p henad-cli -- gpu_boids --global-warmup 1000 --steps 10000 --reps 3
```

Record the per-tick stat series instead of a timing:

```bash
cargo run --release -p henad-cli -- sir --steps 2000 --stats-every 10 --export-stats sir.csv
```

Run a model's action part way through, the replayable form of the buttons the app draws.
Ids are listed with each model in [the models](models.md), and each rep replays the same schedule:

```bash
cargo run --release -p henad-cli -- game_of_life --steps 1000 --act clear@500 --export final.txt
```

An action fires when the state reaches that tick, before the step that leaves it, and warm-up ticks count towards it.
It draws from a stream of its own, so firing one leaves the tick's own draws where they were and the run stays reproducible from `--seed`.
A GPU model runs its action as a compute pass of its own, between two batches of steps.
An action due at any tick from the end of warm-up up to, but not including, the tick the run stops on is timed with the steps.
One due on the tick the run stops on runs after the timer stops.
With `--export-stats`, the row for a tick is sampled after that tick's actions.
An id the model does not declare is refused, and the error names the ids it does.
An action due after the tick the run stops on never fires, and the runner warns about it on stderr.

Pin the worker count, which is how the cross-engine comparison separates its one-thread row from
its all-cores one. It runs this twice, at `--threads 1` and `--threads 0`:

```bash
cargo run --release -p henad-cli -- boids --threads 1 --steps 100 --reps 5 --json
```

Let a network model's population settle before timing starts:

```bash
cargo run --release -p henad-cli -- team_assembly --warmup 1000 --steps 1000
```

A network model can add and remove nodes as it runs.
Updates per second are computed from the population after warm-up, and the JSON `population` is that same count.
If the population moves by more than a tenth during the timed steps, or by more than three times its square root when that is larger, a note on stderr says so.

## Export files

`--export` writes one section for each view the model has, each opening with a marker line.
A grid is `# grid WxH`, then one line of comma-separated cell indices per row.
Agents and nodes are `# points N`, then an `x,y,color` header and one row per point.
The `color` column is left out for a model without a colour lane.
A network model adds `# edges N`, then a `src,dst,color` header and one row per edge.

```text
# points 5
x,y,color
655.5882,154.33028,1
216.5182,938.7303,1
57.76192,169.39508,1
302.25052,392.0565,0
61.964302,547.02985,0
# edges 5
src,dst,color
3,2,0
1,0,0
2,0,0
4,1,0
2,1,0
```

`src` and `dst` are rows of the point section, counted from 0, and `color` indexes the model's edge palette.
On a directed graph each edge runs from `src` to `dst`.
A retired node has no row, and the rows after it close up, so a node's row can be lower than its index in the model.
Retiring a node removes its edges.
An edge whose end has no finite position, such as a reused slot the model has not placed yet, is left out.
The CLI never runs the app's spring layout, and a network model's points sit where the model placed them.
`--export` works on CPU models only.

Both exports step the model as a program does, through `henad_compute::simulation::Simulation`.
`Simulation::write_state` writes the bytes `--export` writes, and `Simulation::run_sampled` takes the samples `--export-stats` writes.
Tick 0's actions fire before the first step, so `--export --steps 0 --act seed_outbreak@0` writes the grid after the outbreak.
A CPU model's run enters the worker pool once for all its steps and samples.

For a CPU model, `--export-stats` prepares the model's view before each sample, as the app does before it publishes a snapshot.
A stat computed during that preparation, such as Team Assembly's component stats, is then current in every row.
Each sample also pays for the preparation, and `--stats-every` spaces the samples out.

## Sweeps

`--out`, `--spec` or `--dry-run` turns a command line into a sweep.
A sweep plans a list of configs, each one full set of parameter values and action ticks, and runs every config `--reps` times with a different seed each time.
`--merge` joins the directories of a sweep split into [shards](#shards-and-merging).
The [parameter sweeps guide](../guide/sweeps.md) walks through one from the first command to reading the results.

```text
henad-cli [OPTIONS] MODEL --out DIR [--vary ID=LEVELS]...
henad-cli [OPTIONS] --spec FILE --out DIR
henad-cli --merge DIR... --out DIR
```

Without `--vary` or `--design`, a sweep runs a single config, the model's defaults with any `--set` applied.
A spec file with a `[search]` table runs a [search](#searches) instead.

### Sweep flags

| Flag | Default | Effect |
|---|---|---|
| `--out <DIR>` | | Run the sweep and write its results to this directory. With `--merge`, write the merged shards to this directory |
| `--spec <FILE>` | | Read the sweep from a TOML spec file, or run the [search](#searches) of one with a `[search]` table |
| `--dry-run` | | Print the sweep's plan and write nothing. The first config that builds without a fault and the last config are built as a check |
| `--vary <ID=LEVELS>` | | Vary one parameter over [its levels](#levels), or the tick of an action with `action.NAME=LEVELS`. Repeatable |
| `--zip` | | Pair the levels of every `--vary` by position |
| `--sample <lhs:N\|random:N>` | | Draw `N` configs from the `--vary` levels and ranges, spread as a Latin hypercube or drawn uniformly. See [designs](#designs) |
| `--design-seed <N>` | from `--seed` | Seed for the `--sample` draws |
| `--design <FILE>` | | Run one config per row of a [design table](#designs) |
| `--independent-seeds` | | Give every run a seed of its own |
| `--series-every <N>` | `--stats-every` | Keep a row of `series.csv` every N ticks, a multiple of `--stats-every`. 0 writes no series |
| `--reduce <COLUMN:KIND>` | | Add a [reducer](#sampling-and-reducers). Repeatable |
| `--no-default-reducers` | | Drop the four reducers every stat column gets by default |
| `--stop <CONDITION>` | | End a run at the first sample where a [stop condition](#stop-conditions) holds |
| `--timeout <SECONDS>` | | End a run after this many seconds of wall-clock time and record it as [`timed_out`](#timeouts) |
| `--concurrent <N\|auto>` | auto | Number of concurrent runs, in CPU lanes or on GPU tracks. See [concurrency](#concurrency) |
| `--memory <BYTES>` | | Combined memory limit in bytes for the CPU lanes |
| `--gpu-memory <BYTES>` | the device's largest buffer size | Combined GPU memory limit in bytes for the runs on GPU tracks |
| `--shard <I/N>` | `0/1` | Run only the runs whose `run_id` leaves remainder `I` when divided by `N` |
| `--resume` | | Resume the sweep in the `--out` directory and run only the missing and timed-out runs. See [resuming](#resuming) |
| `--retry-failed` | | With `--resume`, also rerun failed runs |
| `--merge <DIR>...` | | Merge the directories of a sweep's shards into the `--out` directory. Takes no model |

`--out`, `--spec` and `--dry-run` each ask for a sweep, and every other flag in this table apart from `--merge` needs one of them.
`--resume` needs `--out`, and `--retry-failed` needs `--resume`.
`--zip` and `--sample` need `--vary` and cannot be combined, and `--design-seed` needs `--sample`.
`--design` cannot be combined with `--vary`, `--zip` or `--sample`.
A dry run with `--out` checks the directory and still writes nothing.

`--set`, `--act`, `--steps`, `--warmup`, `--reps`, `--seed`, `--stats-every`, `--threads` and `--json` from the main table apply to a sweep too.
`--list`, `--export`, `--export-stats` and `--global-warmup` cannot be combined with a sweep, and `--params` cannot be combined with `--out` or `--dry-run`.
`--merge` takes `--out` and `--json`, and refuses every flag that plans or runs a sweep.

`--spec` refuses every flag that changes a result, and the spec file is then the whole record of the sweep.
The refused flags are `--set`, `--act`, `--steps`, `--warmup`, `--reps`, `--seed`, `--stats-every`, `--vary`, `--zip`, `--sample`, `--design-seed`, `--design`, `--independent-seeds`, `--series-every`, `--stop`, `--reduce`, `--no-default-reducers` and `--timeout`.
`--concurrent`, `--memory` and `--gpu-memory` override the keys `concurrent`, `memory` and `gpu_memory` of the spec's `[execution]` table.
`--shard`, `--resume` and `--retry-failed` work with a spec as they do with flags.
A `MODEL` given beside `--spec` has to match the spec's `model`.
The guide describes [the spec file](../guide/sweeps.md#spec-files).

### Levels

`--vary` takes a parameter id, or `action.NAME` for the tick of an action, and its levels in one of four forms.

| Levels | Example | Values |
|---|---|---|
| `v1,v2,...` | `network=Random,Geometric` | Each value in the list, written as `--set` takes it |
| `min:max:step` | `infection_rate=0.1:0.5:0.1` | `min`, `min + step`, `min + 2 * step` and so on, up to and including `max` |
| `min:max` | `infection_rate=0.05:0.95` | Under `--sample`, every value from `min` to `max`. Elsewhere, the integers from `min` to `max` for a `u32` parameter or an action tick, and refused for an `f32` |
| `all` | `directed=all` | Every value of a `bool` or `choice` parameter |

Spaces around the levels and around each listed value are ignored.
Value `i` of a range is `min + i * step`, computed from `i` alone.
Rounding errors do not build up along the range, and `0:1:0.1` gives 11 values ending at exactly 1.
A range stops at the last value not past `max`, and a last value within rounding error of `max` is `max` itself.
A `u32` range can leave out its step and then steps by 1, as in `initial_outbreak_size=1:5`.
An `f32` range needs a step, except under `--sample`.
A range over a `bool` or `choice` parameter is refused, and so is `all` over a number.

The levels of an action are whole ticks from 0.
`action.NAME` names an action that `--act` adds.
An action is named by its id, a second `--act` with the same id by `ID_2`, a third by `ID_3`, and so on, skipping any name an earlier `--act` has taken.

### Designs

A sweep from flags has one block of configs, and its design is one of these.

| Design | Flags | Configs |
|---|---|---|
| `factorial` | `--vary` | Every combination of levels |
| `zip` | `--vary` and `--zip` | Config `i` takes level `i` of every `--vary` |
| `lhs` | `--vary` and `--sample lhs:N` | `N` configs in a Latin hypercube |
| `random` | `--vary` and `--sample random:N` | `N` configs, every value drawn on its own |
| `table` | `--design FILE` | One config per row of a CSV file |

A spec file names the design of each `[[block]]` the same way, and a block without one is a `factorial`.

A factorial changes the first `--vary` slowest.
`--vary infection_rate=0.1,0.2 --vary recovery_rate=0.05,0.1` runs the pairs `(0.1, 0.05)`, `(0.1, 0.1)`, `(0.2, 0.05)` and `(0.2, 0.1)`, in that order.
A zip needs the same number of levels in every `--vary`.

A Latin hypercube of `N` configs gives each factor its own random order of the strata 0 to `N - 1`, and config `k` takes stratum `order[k]`.
An `f32` range with no step takes the value `min + (stratum + u) / N * (max - min)`, with `u` drawn uniformly from `[0, 1)`.
The value is computed in 64 bits, then rounded to `f32` and clamped to the range.
A factor with `m` values takes the value at index `stratum * m / N`, rounded down and counted from 0, and each value then appears in `N / m` configs, rounded down or up.
That covers a list, `all`, a stepped range and an integer range with no step.

A random design draws every value of every config on its own.
An `f32` range with no step takes `min + u * (max - min)`, and a factor with `m` values takes each of them with chance `1 / m`.

Both sampled designs draw from the block's design seed.
The seed is `--design-seed`, or `design_seed` in a spec block, and otherwise `mix_seed(mix_seed(root ^ DESIGN_SALT) + block)`, where `block` counts the blocks from 0.
A design seed gives the same configs on every machine.
The plan and the manifest record the design seed of each sampled block.

A design table is CSV.
Its header names one parameter id or `action.NAME` per column, and row `i` after the header is config `i` of the block, counted from 0.
Under `--design` the table is the sweep's only block, and row `i` is config `i` of the sweep.
Values are written as `--set` takes them: a number, `true` or `false`, or an option's name or index.
Spaces around a field, blank lines and a leading byte order mark are ignored.
A parameter or action the table leaves out keeps its `--set` value, its default or its `--act` tick.
In a spec file, a `table` block names its table with `file`, relative to the spec file, or holds its text in `table_text`, and takes no factors.
The path cannot be absolute or hold `..`, and the table sits in the spec file's directory or below it.

### Checks before the first run

The sweep is checked against the model before any run starts.
Each of these refuses it, with an error that names the cause:

- a parameter id the model does not have, with the ids it does have in the error
- a value outside the parameter's range, or a `choice` value that names no option
- a parameter fixed with `--set` and varied too, or varied twice
- `--zip` over levels of different lengths
- an `f32` range with no step, outside `--sample`
- a sampled design with no samples or no factors, or a factor with an empty list of `values`
- a design table with a column that names nothing, a column named twice, a row of another width than the header, a value its column refuses, or no rows
- an action the model does not declare, two actions with one name, or `--vary action.NAME` for an action no `--act` adds
- an action tick that is not a non-negative integer
- a `--stats-every` of 0, or a `--series-every` that is not a multiple of `--stats-every`
- a stop condition or reducer over a column the model does not report, or a reducer of an unknown kind
- on a GPU model, a config the device cannot hold, with the first few such configs and their limits in the error

An action due after the last tick runs nothing, and the plan warns about it without refusing the sweep.

### Seeds

Replicate `r` of every config is built with the seed `mix_seed(mix_seed(root ^ SWEEP_SALT) + r)`, where `root` is `--seed` and the addition wraps.
Every config then shares the seeds of its replicates, a scheme known as common random numbers.
`--independent-seeds` mixes the config id in as well, and every run gets a seed of its own.
Its formula is `mix_seed(mix_seed((mix_seed(root ^ CONFIG_SALT) + config_id) ^ SWEEP_SALT) + r)`.
Both formulas use [`mix_seed`](primitives.md#mix_seed), and the manifest records the one a sweep used.
They differ from the benchmark's `base + i`.
A sampled design draws its configs from a design seed of its own, described under [designs](#designs).

Every row of `runs.csv` records its seed.
Passing it to `--seed`, with the row's parameter values as `--set`, its action ticks as `--act`, and the sweep's `--warmup` and `--steps`, rebuilds the run on its own for `--export-stats` or `--export`.

### Sampling and reducers

A run steps `--warmup` plus `--steps` ticks.
It samples its stats at the end of the warm-up, every `--stats-every` ticks after that, and at its last tick.
A sample of a CPU model prepares the view before reading the stats, as `--export-stats` does.
Sampling never changes a run, and a run sampled every tick ends in the same state as one sampled every tenth tick.

A reducer folds a run's samples of one stat column into one value.

| Kind | Value |
|---|---|
| `final` | Last finite sampled value |
| `min` | Least sample |
| `max` | Greatest sample |
| `mean` | Mean of the samples |
| `argmax` | First sampled tick of the greatest sample |
| `argmin` | First sampled tick of the least sample |
| `first` then a comparison, as in `first<=10` | First sampled tick whose value passes the comparison. The comparator is one of `<`, `<=`, `>`, `>=`, `==` and `!=` |
| `mean@START..END`, as in `mean@200..600` | Mean of the samples from tick `START` to tick `END`, both included |

Every stat column gets the first four, apart from a histogram's bucket columns.
`--no-default-reducers` drops them, and `--reduce COLUMN:KIND` adds one, as in `--reduce Infected:max`.
`COLUMN` is a column of `series.csv`, such as `Infected` or `Average Velocity.x`.
A bare vector or histogram label, such as `Average Velocity`, names its `.magnitude` or `.total` column.
Each reducer adds a column to `runs.csv`, named `COLUMN:KIND` as in `Infected:max` or `Average Velocity.magnitude:max`.
A `first` kind writes its threshold in its shortest form, and `Infected:first<=10.0` makes the column `Infected:first<=10`.
A spec file lists reducers in `[measure]` as `reducers = [{ column = "Infected", kinds = ["argmax", "first<=10"] }]`.

Every reducer skips a sample that is not finite, and the run's status becomes `non_finite`.
A reducer that sees no finite sample is empty.
So is a `first` kind whose comparison never holds, and a `mean@` kind with no sample in its window.

### Stop conditions

`--stop` ends a run at the first sample where a condition holds, as in `--stop 'Infected <= 0'`.
A condition reads `COLUMN COMPARATOR THRESHOLD`.
`COLUMN` is the text before the first `<`, `>`, `=` or `!`, trimmed, and a label with spaces works, as in `Giant Component Share >= 0.5`.
It names a column as a reducer does.
`COMPARATOR` is one of `<`, `<=`, `>`, `>=`, `==` and `!=`, and `THRESHOLD` is a finite number.

The condition is checked at every sample and never between two samples.
A NaN never passes it.
A run whose condition holds ends on that sample, with `stop_reason` `condition` and the sample's tick in `ticks`.
The run does not count as failed, and its series and reducers end on that sample.
Actions due after it never fire.

A spec file writes the condition in `[run]`, as `stop = { condition = "Infected <= 0", min_tick = 20 }`.
`min_tick` is the first tick at which the condition can end a run, 0 when left out.
The command line has no flag for it.

### Actions

`--act ID@TICK` fires an action in every run of a sweep, and each `[[action]]` table of a spec file does the same with its `id` and `tick`.
A sweep names each action, and names are unique.
A spec file's `name` is the id when left out.
On the command line the name is the id, then `ID_2` for a second `--act` with the same id, `ID_3` for a third, and so on, skipping any name an earlier `--act` has taken.
`--vary action.NAME=LEVELS`, or a factor with `action = "NAME"`, varies that action's tick.
`runs.csv` and `summary.csv` hold a column `action.NAME` per action, after the parameters, with the action's tick in the config.

An action due at tick 0 fires before the first step.
One due at a later tick fires after the step that reaches it, before that tick's sample.
Warm-up ticks count toward the tick, as in a benchmark.
Actions due at one tick fire in the order they are listed, and each fires at most once per run.
An action due past the run's last tick never fires, and the plan warns about it.
A refused action is noted in the `note` column as `model refused action 'ID' at tick TICK`, and the run keeps its status.

### Timeouts

`--timeout SECONDS`, or `timeout_s` in a spec file's `[run]`, limits the wall-clock time of each run.
The seconds can be fractional.
The clock counts stepping and sampling, leaves out the build, and is read between slices of steps.
On a GPU track it counts the run's share of the time the sweep spends on the tracks, as [concurrency](#concurrency) describes.
A run ends after the first slice that takes it past its limit, with status `timed_out`, stop reason `timeout`, the tick it reached in `ticks`, and a note such as `timed out after 600 s at tick 4096`.
A timed-out run depends on the machine and its load, and none of the promises of identical files on this page covers it.
The timeout is not part of the plan hash, and a [resume](#resuming) runs a timed-out run again, under any timeout.

### Concurrency

A CPU model steps several runs at once, each in a lane with a thread pool of its own.
With `auto`, the lanes are sized from two builds, of the first config that builds without a fault and of the last config, and the one that holds more memory decides.
A config whose build faults is left for its runs to record, and a sweep whose first eight configs all fault stops before any run.
A lane gets one thread for every four jobs one step splits into, up to every worker, and the workers are split into lanes of that width.
A model that does not report its jobs counts one job per 4096 of its population.
`--concurrent N` runs `N` lanes instead, and splits the workers evenly between them.
Either way the lane count is capped by the number of runs, and by `--memory` divided by the memory that build holds on a pool as wide as one lane.
The workers are those `--threads` asks for, or one per logical cpu.
A single lane steps each run on the whole worker pool, as a benchmark rep does.

A GPU model steps several runs on one device, each on a track.
The tracks share one thread, and the sweep visits them in turn.
A visit collects the track's sample once its readback lands, then submits the actions due at the track's tick, each in a command buffer of its own.
It then submits one command buffer of at most 64 steps, up to the next tick a sample or an action is due at.
A sample due at that tick puts its stats passes after the steps in the same buffer, unless an action is due there too.
Its stats passes then follow the actions in a buffer of their own.
A command buffer never holds the steps of two runs, and each track keeps at most two on the device.
A sample reads back while its track submits the steps after it, and the track submits no further sample until that readback lands.
When no track can move, the sweep waits for the oldest command buffer on the device.

With `auto`, a GPU model gets as many tracks as the GPU memory budget holds runs the size of the larger of the two builds, up to 4.
A model whose population is 1,048,576 or more gets one track.
`--concurrent N` runs `N` tracks instead.
Either way the track count is capped by the number of runs.

The GPU memory budget is `--gpu-memory`.
Without it, the device's largest buffer stands in for the budget, since wgpu reports no total for a device's memory.
A run's demand is the bytes of the buffers and the display texture its model declares.
A run is built once a track is free and its demand fits the budget beside the demand of the live runs.
A run that fits beside no other run is built once no other run is live, and steps alone.
A build that runs out of device memory while other runs are live goes back to the head of the queue and waits for a live run to end, and the track count drops by one for the rest of the sweep.

A fault while a track submits its run's work, such as a panic or a validation error, ends that run alone, and the other tracks carry on.
An error the device reports that Henad cannot trace to one run ends every live run with status `gpu_error`.
A lost device stops the sweep.
The runs written before the loss stay, the manifest reads `incomplete`, `explore_end` reads `device_lost`, and the command exits with status 1.
No run that failed because of the loss is written, and `--resume` runs those with the rest.

A GPU run's clock starts once its model is built and stops at its last sample.
It leaves out the builds of other runs and every pause.
With several tracks, the time the sweep spends visiting them is split evenly between their runs, and the clock counts the run's share.
`wall_ms`, `steps_per_s` and the timeout all read that clock, and `--concurrent 1` times each run on its own.
A GPU run that times out gives the tick of its last sample in `ticks`.

Runs are written in plan order at any lane or track count.
All three CSV files come out the same byte for byte at any `--concurrent`, apart from the `build_ms`, `wall_ms` and `steps_per_s` columns.
A rerun of the sweep gives the same three files as well, and so do shards joined by `--merge` and a sweep finished by `--resume`.
Two exceptions apply.
A run that timed out ends wherever the clock caught it.
`gpu_boids` leaves the order of the boids within a cell of its neighbour index open, and two sweeps of it differ.

### Output directory

`--out` creates the directory and any missing parents.
A directory that holds `runs.csv`, `series.csv`, `summary.csv`, `manifest.json` or one of the [search tables](#search-tables) is refused unless `--resume` is given, and other files in it are left alone.
A file ending in `.staged` counts as well.
A [resume](#resuming) or a [merge](#shards-and-merging) that stopped while replacing its tables leaves one behind.

| File | Content |
|---|---|
| `runs.csv` | One row per run |
| `series.csv` | The sampled stat rows of every run |
| `summary.csv` | Statistics over the replicates of each config |
| `manifest.json` | Settings, provenance, plan and result counts of the sweep |

A run is written once every run before it in the plan is written, its rows of `series.csv` first and then its row of `runs.csv`, each flushed.
A sweep stopped part way keeps every run it wrote, and its manifest still reads `running`.
Once the last run is in, `summary.csv` is written from `runs.csv` and the manifest is replaced in a single rename.

#### `runs.csv`

| Column | Content |
|---|---|
| `run_id` | Position of the run in the plan, `config_id * reps + rep` |
| `config_id` | Config of the run, counted from 0 across every block |
| `block` | Block of the config in a spec file, 0 for a sweep from flags |
| `rep` | Replicate, counted from 0 |
| `seed` | Seed the model was built with |
| `run_key` | Hash of the run's settings, parameter values, action ticks and seed, as 16 hexadecimal digits |
| One per parameter | Value of the parameter, in the order `--params` lists them. A `choice` is written as its option name |
| One per action | Tick of the action in the run's config, headed `action.NAME`, in the order the actions are listed |
| `status` | One of the statuses below |
| `stop_reason` | `steps` for a run that reached its last tick, `condition` for one its stop condition ended, `fault` for one a fault ended, and `timeout` for one past its timeout |
| `ticks` | Tick the run ended on. A GPU run that faulted or timed out gives the tick of its last sample |
| `population` | Population at the run's last sample |
| `build_ms` | Milliseconds spent building the model |
| `wall_ms` | Milliseconds spent stepping and sampling |
| `steps_per_s` | Ticks per second over `wall_ms` |
| One per reducer | Value of the reducer, empty when there is none |
| `note` | Actions the model refused, then the fault message with its source location, the timeout, or the first value that was not finite, separated by `; ` |

| Status | Meaning |
|---|---|
| `ok` | The run reached its last tick or its stop condition, and every sample was finite |
| `non_finite` | The run reached its last tick or its stop condition, and some sample was NaN or infinite |
| `panicked` | The model panicked while building or stepping |
| `gpu_error` | The GPU reported an error |
| `refused` | The host refused to build the model |
| `shape_error` | A sample no longer fit the sweep's stat columns, such as a histogram whose bucket count changed |
| `timed_out` | The run passed its timeout |

The last five count as failed.
A failed run keeps the series rows it sampled before it ended, and the sweep carries on with the next run.

#### `series.csv`

The columns are `run_id`, `tick`, then one per stat column, headed as `--export-stats` heads them.
A vector stat writes `.x`, `.y` and `.magnitude` columns, and a histogram one column per bucket plus `.total`.
A run's rows are contiguous and in tick order.
They hold every sample on the `--series-every` cadence, counted from the end of the warm-up, and the run's last sample.
A value that is not finite is an empty cell.

#### `summary.csv`

One row per config, rebuilt from `runs.csv` at the end of the sweep, of a resume and of a merge.

| Column | Content |
|---|---|
| `config_id`, `block` | As in `runs.csv` |
| One per parameter | As in `runs.csv` |
| One per action | As in `runs.csv` |
| `runs` | Runs of the config |
| `ok` | Runs with status `ok` |
| `failed` | Runs that ended on a fault or a timeout |
| `ticks:mean` | Mean tick the runs ended on |
| `R:mean`, `R:sd`, `R:n`, `R:ci95_low`, `R:ci95_high` | Five columns for each reducer column `R` of `runs.csv` |

A `non_finite` run counts in neither `ok` nor `failed`.
The statistics cover every run that did not fail, `non_finite` runs included.
`n` counts the finite values, and `sd` divides by `n - 1`.
The 95% confidence interval for the mean is `mean ± t * sd / sqrt(n)`, where `t` is the 97.5% quantile of Student's t distribution with `n - 1` degrees of freedom.
`sd` and the interval are empty below two values.

#### `manifest.json`

| Field | Content |
|---|---|
| `format`, `format_version` | `henad-explore` and 1 |
| `mode` | `sweep`, or `search` for a [search](#search-tables) |
| `status` | `running` until the sweep ends, then `complete`, `aborted` for a sweep stopped before its last run, `failed` for a sweep or merge an error ended outside any run, or `incomplete` for a sweep that lost its GPU device or a merge that lacks some runs |
| `engine` | Name, version, commit, commit date and whether the build was a debug build |
| `model` | Id, name, backend, schema hash, and the schema [`--params --json`](#parameters-as-json) prints |
| `spec` | The sweep in the form of a spec file, flags included, with a design table's text under `table_text` |
| `spec_source` | Path and text of the spec file, and the path and hash of each design table it reads. `path` and `toml` are `null` for a sweep from flags, and `tables` lists its `--design` file |
| `argv` | The command line |
| `plan` | Plan hash, results fingerprint, the counts of configs, replicates and runs, and the design, config count and design seed of each block |
| `seeds` | Root seed, seed scheme and its formula |
| `columns` | Names of the stat columns and the reducer columns |
| `shard` | Share of the plan the directory holds, as `index` and `count`. Index 0 of 1 for a whole sweep or a merge |
| `execution` | Backend, concurrency, lanes, threads per lane, GPU runs at once, projected bytes, and the memory and GPU memory budgets |
| `runtime` | Operating system, architecture, logical cpus, worker threads, and the GPU adapter with its limits when there is one |
| `timestamps` | Start and end, in milliseconds since the Unix epoch and as RFC 3339 text in UTC |
| `sessions` | One entry per process that wrote runs to the directory, with its start, its commit, the runs it kept as `skipped` and the runs it wrote as `ran` |
| `results` | Row counts `rows`, `ok`, `non_finite` and `failed`, `null` while the sweep runs |
| `merged_shards` | Directories a merge read, as `--merge` names them. `null` for a sweep that ran in the directory |
| `search` | A search's budget and standing, described under [search tables](#search-tables). `null` for a sweep |

A failed sweep or merge marks its manifest `failed` when the manifest can still be written.
A directory in any status can be [resumed](#resuming), a `complete` one included.

### Resuming

`--resume` adds to the results in the `--out` directory instead of refusing it.
A directory with no results starts a fresh sweep.

The resume reads the manifest first.
It refuses a directory whose plan hash, model schema hash or shard differs from the sweep's.
The plan hash covers the model, the configs, the design seeds, the fixed values, the actions, the steps and warm-up, the sampling and series cadence, the stop condition, the reducers, and the seed root and scheme.
It leaves out the replicate count and the timeout, and a resume can change both.
A build whose commit differs from the last session's gets a warning.

The resume then repairs the tables.
A partial last record of `runs.csv` or `series.csv` is cut off, and so are the series rows of any run with no row in `runs.csv`.
It keeps every run with status `ok` or `non_finite`, and every run that failed on a fault unless `--retry-failed` is given.
A `timed_out` run always runs again.
The runs kept are skipped, and the rest run.

A higher replicate count keeps every run on disk, gives each the run id it has in the larger plan, and runs the new replicates alone.
A lower count is refused.
In a sharded directory, a new count that would move a run to another shard is refused too.

Dropped rows and new run ids make the resume rewrite both tables.
Each is written in full beside the original as `runs.csv.staged` or `series.csv.staged`, then a file `tables.staged` marks both complete, and both are renamed into place.
The next resume finishes a rename that a process left part done.
Once the new runs are in, the tables are put in run order where they are not already, `summary.csv` is rebuilt, and the manifest gains a session.
The finished directory holds the same three CSV files as a sweep run without a break, apart from the timing columns.

With `--dry-run`, a resume prints how many runs it would skip and run, and changes nothing.

### Shards and merging

`--shard I/N` runs the runs whose `run_id` leaves remainder `I` when divided by `N`, and records the shard in the manifest.
Every shard plans the whole sweep, and needs the same command line or spec as the others.

`--merge DIR... --out DIR` joins shard directories into a new directory.
It refuses inputs that differ in plan hash, schema hash, replicate count, shard count or columns, and two inputs that hold one shard.
It also refuses a row whose `run_id` is not `config_id * reps + rep` at the replicate count of its manifest.
A resume that raises a shard's replicate count and ends before renumbering the shard's rows leaves such rows, and resuming that shard again renumbers them.
It merges `runs.csv` and `series.csv` in run order, leaving out a partial last record and the series rows of runs with no row in `runs.csv`, and rebuilds `summary.csv`.
Both tables are staged and renamed into place together, as a resume that rewrites them does.
The merged manifest is that of the lowest shard, with shard 0 of 1, the sessions of every shard, and the inputs under `merged_shards`.

A run of the plan that no input holds is reported as a warning, and the merged manifest reads `incomplete`.
`--resume` on the merged directory, without `--shard`, runs the missing runs.
The merged files are the same as those of the sweep run in one piece, apart from the timing columns.

### Progress

The plan comes first, on stderr, or on stdout for a dry run.

```text
sweep of SIR Epidemic (sir): 5 configs x 5 replicates = 25 runs
  block 0:      factorial, 5 configs
  layout:       1 lane of 14 threads
  memory:       2.0 MiB projected for concurrent runs
  series rows:  12525
```

Each block gets a line with its design, its config count, and its design seed when it draws one.
A sharded sweep adds a `shard` line with its share of the runs, and a resume adds a `resume` line with the runs it skips and the runs left to run.
The layout line of a GPU model counts its tracks, as in `4 GPU runs at a time`.
The memory line is the lane count times the memory one run holds in its lane, or for a GPU model the track count times one run's demand.
The last line is the number of rows `series.csv` gains from the runs to run, as if each reaches its last tick, and a warning follows it past ten million.
Warnings, such as an action due past the last tick, go to stderr as lines starting `warning:`.

While the runs step, a progress line counts the runs done and estimates the time left.
On a terminal it rewrites itself, and elsewhere a new line is logged every five seconds.
A run that does not end `ok` gets a line of its own with its status and note, and the sweep ends with a line of counts.
A merge prints one line of counts.

Under `--json`, the sweep and the merge write JSON lines to stdout in place of the text.

| `kind` | Sent | Fields |
|---|---|---|
| `explore_plan` | Once, after planning | `model`, `backend`, `configs` (`null` for a search), `replicates`, `runs`, `blocks` (each with `design`, `configs` and `design_seed`), `shard` (`index` and `count`), `skipped`, `pending`, `cpu_lanes`, `threads_per_lane`, `gpu_tracks`, `projected_bytes`, `series_rows`, `dry_run`, and `search`, `null` for a sweep |
| `explore_run` | Once per run as it is written, in plan order | `run_id`, `config_id`, `rep`, `seed`, `status`, `stop_reason`, `ticks`, `wall_ms` |
| `explore_progress` | At most once a second | `done` and `total` over the runs this process runs, `skipped`, `failed`, `elapsed_s`, and `remaining_s`, `null` before the first run finishes |
| `explore_search_batch` | Once per batch of a search, after its runs | See [search progress](#search-progress) |
| `explore_end` | Once, at the end | `end` (`planned`, `complete`, `aborted` or `device_lost`), `rows`, `skipped`, `ok`, `non_finite`, `failed`, `elapsed_s`, and `output_dir`, `null` for a dry run. A search adds the fields of [search progress](#search-progress) |
| `explore_merge` | Once, at the end of a merge | `inputs`, `rows`, `ok`, `non_finite`, `failed`, `missing`, `output_dir` |

`skipped` counts the runs a resume kept, and `pending` the runs left to run.
The counts of `explore_end` and `explore_merge` cover every row of `runs.csv`, the rows a resume kept included.
`gpu_tracks` is the number of GPU runs alive at once.
None of these kinds is a benchmark kind, and one reader can take both streams.

```json
{"backend":"cpu","blocks":[{"configs":2,"design":"factorial","design_seed":null}],"configs":2,"cpu_lanes":4,"dry_run":false,"gpu_tracks":0,"kind":"explore_plan","model":"sir","pending":4,"projected_bytes":32768,"replicates":2,"runs":4,"series_rows":204,"shard":{"count":1,"index":0},"skipped":0,"threads_per_lane":1}
{"config_id":0,"kind":"explore_run","rep":0,"run_id":0,"seed":4320778953317010875,"status":"ok","stop_reason":"steps","ticks":50,"wall_ms":0.743085}
{"elapsed_s":0.015759,"end":"complete","failed":0,"kind":"explore_end","non_finite":0,"ok":4,"output_dir":"sir-sweep","rows":4,"skipped":0}
```

### Exit status

| Status | Meaning |
|---|---|
| 0 | Every run is `ok`, a dry run planned the sweep, or a merge holds every run and each is `ok` |
| 1 | An error stopped the sweep or the merge, such as a spec the model refuses, a directory that already holds results, a resume of another plan, or a lost GPU device |
| 2 | The command line itself was refused, before anything ran |
| 3 | The sweep ran to its end and some run is not `ok`, or a merge lacks some run or holds one that is not `ok` |

A search exits with the same statuses as a sweep.

## Searches

A spec file with a `[search]` table runs a search.
A search picks its configs, called **candidates**, a batch at a time, each batch from the results of the batches before.
An **evaluation** runs one candidate for every replicate, and a **re-evaluation** runs a candidate already evaluated for more replicates.
Flags cannot describe a search, and it runs from `--spec` alone.
The [searching guide](../guide/search.md) walks through two example specs.

```text
henad-cli [OPTIONS] --spec FILE --out DIR
```

`--out`, `--dry-run`, `--resume`, `--concurrent`, `--memory`, `--gpu-memory`, `--threads` and `--json` work as they do for a sweep.
`--shard` and `--retry-failed` are refused, for the reasons under [resuming a search](#resuming-a-search).
The runs of a search step on CPU lanes or GPU tracks as a sweep's runs do, and the rules of [concurrency](#concurrency) hold for its tables as well.

### The search table

A spec with a `[search]` table takes no `[[block]]` table.
Its other tables are read as for a sweep, and every table refuses a key it does not know.

| Key | Content |
|---|---|
| `algorithm` | `random`, `hill_climb`, `genetic` or `pse` |
| `max_evaluations` | Evaluations the search runs, re-evaluations included, at least 1 |
| `batch_size` | Most candidates of one batch, at least 1 |
| `objective` | `{ column, goal, aggregate }`. Every algorithm but `pse` needs one, and `pse` refuses one |
| `space` | Factors the search varies, written as a block's factors are |

`algorithm`, `max_evaluations`, `batch_size` and `space` have no default.
The keys of `objective` are these:

| Key | Default | Content |
|---|---|---|
| `column` | | A reducer column of `runs.csv`, as in `Infected:max` |
| `goal` | | `maximize` or `minimize` |
| `aggregate` | `median` | `median` or `mean`, the rule that folds a candidate's replicate values into one |

An algorithm's settings go in a table named after it, and a table for any other algorithm is refused.
`[search.hill_climb]` and `[search.genetic]` can be left out, and a key left out takes its default.
`[search.pse]` is needed, for its axes.
Random search takes no settings.

| Table | Key | Default | Range |
|---|---|---|---|
| `[search.hill_climb]` | `mutation_scale` | 0.1 | Above 0 |
| | `patience` | 5 | At least 1 |
| | `reevaluate` | `false` | `true` or `false` |
| `[search.genetic]` | `population` | 32 | 1 to 65536 |
| | `elite_count` | 2 | Below `population` |
| | `tournament_size` | 3 | 1 to 65536 |
| | `crossover_rate` | 0.9 | 0 to 1 |
| | `mutation_rate` | 0.2 | 0 to 1 |
| | `mutation_scale` | 0.1 | Above 0 |
| | `reevaluate_fraction` | 0.25 | 0 to 1 |
| `[search.pse]` | `x_axis`, `y_axis` | | `{ column, min, max, cells }`, with finite bounds, `min` below `max` and at least 1 cell, or `{ column, cells }` for an automatic range |
| | `initial_samples` | 64 | 0 or more, and at least 1 beside an automatic range |
| | `mutation_scale` | 0.1 | Above 0 |
| | `aggregate` | `median` | `median` or `mean` |

A factor of `space` takes a parameter with `param` or an action's tick with `action`, and its levels as `values`, `range` or `levels = "all"`, as a sampled block's factors do.
The search is checked against the model before its first run.
Each of these refuses it, along with the checks of a sweep's fixed values and actions:

- a `[search]` table beside a `[[block]]`, or a table of another algorithm
- a budget or a batch size of 0
- no objective, or an objective given to `pse`
- a setting outside the range above
- an axis with one of `min` and `max` and not the other
- an objective or axis column that no reducer writes, with the reducer columns in the error
- a parameter that `[set]` fixes and `space` varies, or a target that `space` names twice
- a factor a sampled block would refuse, such as an unknown parameter or a value out of range
- more runs than a 64-bit count holds

### Candidates

A candidate's **genome** holds one gene from 0 to 1 per factor of `space`, in order.
A range with no step is an ordered gene, and anything else a categorical one.
Gene `u` of an `f32` range decodes to `min + u * (max - min)`, rounded to `f32` and clamped to the range.
Gene `u` of an integer range decodes to `min + floor(u * (max - min + 1))`, and gene `u` of `m` listed levels to level `floor(u * m)`, each capped at the last value.
Two genomes share a **config** when every gene decodes to the same value.

A mutation changes each gene with some chance: `mutation_rate` for the genetic algorithm, and every gene for hill climbing and Pattern Space Exploration.
An ordered gene moves by `mutation_scale * (r1 + r2 - 1)`, where `r1` and `r2` are drawn uniformly from 0 to 1.
A result past 0 or 1 reflects back off the bound it crossed.
A categorical gene is drawn again uniformly.
Crossover takes each gene from either parent with chance one half.

The search draws from the seed `mix_seed(mix_seed(root ^ SEARCH_SALT))`, where `root` is the root seed, through the generator of the sampled designs.
No draw goes through a logarithm or a cosine, and a spec gives the same candidates on every platform.
Candidate ids count from 0 in the order the search asks for them, and a re-evaluation gets an id of its own.

### Runs and values

An evaluation of candidate `c` runs it `replicates` times.
Run `i` has the run id `c * replicates + i`, the config id `c`, the replicate index `offset + i` and the seed that the seed scheme gives that replicate index.
The offset is 0 for a first evaluation, and the number of replicates the candidate already has for a re-evaluation.
Under `independent`, a re-evaluation is seeded with the id of the candidate it repeats.
The search is told the values of a batch in candidate order, once every run of the batch has ended.

The value of a run is its reducer in the watched column.
A run with a failed status, or a value that is empty or not finite, is a failed replicate.
The objective counts a failed replicate as minus infinity when maximizing and infinity when minimizing, then takes the mean or the median, the median of an even count being the mean of the middle two.
A candidate's objective covers every replicate it has, its re-evaluations included.
Among equal objectives, the lower candidate id ranks first.
A Pattern Space Exploration leaves failed replicates out of each axis, and an evaluation with no value left on an axis lands in no cell.

### Algorithms

`random`
: Draws every gene uniformly.

`hill_climb`
: The first batch holds random candidates, and the best of them becomes the incumbent.
  Every later batch holds `batch_size` neighbours, each a mutation of the incumbent.
  With `reevaluate` and a batch of at least 2, the batch's first candidate re-evaluates the incumbent instead.
  The incumbent moves to the best neighbour of a batch when that neighbour's objective is strictly better than its own.
  After `patience` batches in a row without a move, the next batch draws new random candidates, with origin `restart`, and the climb starts again from the best of them.
  A neighbour whose config an earlier candidate or another neighbour of the batch has is drawn again, up to 16 times.
  When every draw repeats, the neighbour becomes a re-evaluation of the earlier candidate the first draw matched, at most one per candidate in a batch, or is left out when the first draw matched another neighbour.
  A batch can then hold fewer than `batch_size` candidates, and a re-evaluation in place of a neighbour never moves the incumbent.
  A `random` or `restart` candidate is drawn again the same way, and one whose draws all repeat becomes a re-evaluation of the earlier candidate.
  That candidate then counts among the candidates the climb starts from.

`genetic`
: Generation 0 holds `population` random candidates.
  Each later generation queues re-evaluations of the `ceil(reevaluate_fraction * population)` best members of the last one, best first, then `population - elite_count` children.
  A child's first parent wins a tournament of `tournament_size` members drawn with replacement, the earlier draw winning a tie.
  With chance `crossover_rate`, a second tournament gives a second parent and the genes cross over.
  The child's genes then mutate.
  A child whose config an earlier candidate or another child of the generation has mutates again, up to 16 times.
  When every draw repeats, the child becomes a re-evaluation of the earlier candidate the first draw matched, at most one per candidate in a generation, and that candidate joins the generation in the child's place.
  A child whose first draw matched another child of the generation is left out instead.
  Generation 0 draws again in the same way, and holds fewer than `population` candidates when the space has fewer configs.
  Once every candidate of the generation has been told, its members are the children, the candidates that joined in a child's place, and the `elite_count` best of the last generation, ranked with their re-evaluations included.
  A batch never holds the candidates of two generations.

`pse`
: The grid cuts each axis into `cells` cells of equal width from `min` to `max`, and a value lands in cell `floor((value - min) / (max - min) * cells)`, capped at the last cell.
  A value below `min` or above `max` lands in the edge cell, and its evaluation is marked `outside`.
  An axis without `min` and `max` has an automatic range, taken once the first `min(initial_samples, max_evaluations)` candidates are told.
  Its `min` and `max` are then the smallest and largest value those candidates gave on it, among those with a value on both axes, each moved out by 5% of the distance between them.
  A single value `v` gives `v - h` to `v + h`, with `h` the larger of 0.5 and `|v| / 2`, and no value gives 0 to 1.
  The evaluations told before the range was taken land in their cells then, in candidate order.
  Until then the archive is empty, and every candidate is random.
  The first candidate to land in a cell is its exemplar, and every candidate to land there counts as a hit.
  The first `min(initial_samples, max_evaluations)` candidates are random, and a batch that reaches the last of them ends there.
  With `initial_samples = 0`, the first candidate is random and alone in its batch.
  A batch asked while the archive is empty is random in full.
  Every other candidate draws two filled cells with replacement, keeps the one with fewer hits or the first on a tie, and mutates its exemplar.
  A batch breeds from the archive as it stood when the batch was asked for.

### Search tables

A search writes `runs.csv`, `series.csv`, `summary.csv` and `manifest.json` as a sweep does, with the candidate id in `config_id` and 0 in `block`.
A re-evaluation is a config of its own in `summary.csv`.
The search adds these tables:

| File | Content |
|---|---|
| `evaluations.csv` | One row per evaluation, written as its batch is told |
| `batches.csv` | One row per batch, written as it is told |
| `generations.csv` | One row per generation, from a `genetic` search alone |
| `best.csv` | Every candidate ranked, written when the search ends, from every algorithm but `pse` |
| `archive.csv` | Every filled cell, written when the search ends, from a `pse` search alone |

A value that is not finite, such as the objective of a candidate whose every replicate failed, is an empty cell.
The config columns hold one column per parameter and one per action, as in `runs.csv`.

#### `evaluations.csv`

| Column | Content |
|---|---|
| `candidate_id` | Id of the candidate |
| `batch` | Batch that asked for the candidate, counted from 0 |
| `origin` | `random`, `restart`, `neighbor`, `mutation`, `crossover` or `reevaluation` |
| `first_parent_id`, `second_parent_id` | Parents of the genome, empty when there is none. A `neighbor` or a `mutation` has a first parent, a `crossover` both, and a `reevaluation` neither |
| `reevaluated_id` | Candidate a `reevaluation` repeats, empty for any other origin |
| `replicate_offset` | Replicate index of the evaluation's first run |
| `replicates` | Runs of the evaluation |
| Config columns | The candidate's config |
| `failed` | Replicates with a watched value that is missing or not finite |
| `objective` | Objective over the evaluation's own replicates. Not written by `pse` |
| `pooled_objective` | Objective over every replicate the candidate has so far. A re-evaluation gives the value of the candidate it repeats. Not written by `pse` |
| `pooled_replicates` | Replicates behind `pooled_objective`. Not written by `pse` |
| `x`, `y` | Outputs of the evaluation on the two axes, empty when an axis has no value. `pse` alone |
| `x_index`, `y_index` | Cell the outputs land in, empty when they land in none or were told before an automatic range was taken. `pse` alone |
| `outside` | `true` when an output lay outside its axis, empty when the outputs have no cell. `pse` alone |
| `new_cell` | `true` for the first candidate to land in its cell, empty when the outputs have no cell, as for an evaluation told before an automatic range was taken. `pse` alone |

#### `batches.csv`

| Column | Content |
|---|---|
| `batch` | Batch, counted from 0 |
| `evaluations` | Evaluations told so far, the batch's included |
| `runs` | Runs of those evaluations |
| `best_candidate_id`, `best_objective` | Best candidate after the batch and its objective. Not written by `pse` |
| `filled_cells` | Cells the archive holds after the batch. `pse` alone |

#### `generations.csv`

| Column | Content |
|---|---|
| `generation` | Generation, counted from 0 |
| `best`, `median`, `worst` | Fitness of the generation's members, each over every replicate the member had when the generation ended |

#### `best.csv`

| Column | Content |
|---|---|
| `rank` | Rank of the candidate, counted from 1 |
| `candidate_id` | Id of the candidate |
| Config columns | The candidate's config |
| `pooled_objective` | Objective over every replicate |
| `pooled_replicates` | Replicates behind the objective |
| `pooled_failed` | Failed replicates among them |
| `evaluations` | Evaluations of the candidate, the first and each re-evaluation |
| `first_batch` | Batch that first asked for the candidate |

A re-evaluation adds its replicates to the candidate it repeats, and has no row of its own.

#### `archive.csv`

| Column | Content |
|---|---|
| `x_index`, `y_index` | Cell, in cell order by `x_index`, then `y_index` |
| `x_min`, `x_max`, `y_min`, `y_max` | Bounds of the cell |
| `hits` | Candidates that landed in the cell |
| `candidate_id` | The cell's exemplar, the first candidate to land in it |
| Config columns | The exemplar's config |
| `x`, `y` | The exemplar's outputs |

#### The manifest

A search's manifest reads `search` in `mode`.
`plan.plan_hash` holds the hash of the fixed values and actions, and `plan.configs` is `null`.
`spec.search` holds the `[search]` table as the search read it, and the manifest's `search` field its standing:

| Field | Content |
|---|---|
| `algorithm`, `max_evaluations`, `batch_size` | As in the spec |
| `search_hash` | Hash of the search, described under [resuming a search](#resuming-a-search) |
| `search_seed` | Seed the search draws from |
| `watched_columns` | Reducer columns each run reports to the search, the objective's or the two axes' |
| `evaluations`, `batch_count` | Evaluations and batches told |
| `best_candidate_id`, `best_objective` | Best candidate at the end and its objective, `null` for `pse` |
| `filled_cells` | Filled cells at the end, `null` for any other algorithm |
| `axis_ranges` | `x_min`, `x_max`, `y_min` and `y_max` of the grid, an automatic range as the initial samples set it. `null` for any other algorithm, or while an automatic range waits for the initial samples |

A search that fails records its standing at the failure, and zeros when it fails before its first batch is told.

### Search progress

The plan names the algorithm and counts evaluations in place of configs.
It lists the batch size, the objective or the axes, the space and the search seed.
Each axis gives its range, or `automatic range`:

```text
search of SIR Epidemic (sir): pse, 1200 evaluations x 2 replicates = 2400 runs
  batch size:   64 candidates
  axes:         Infected:max from 0 to 4096, 32 cells
                Infected:argmax from 0 to 50, 20 cells
  search space: infection_rate from 0.05 to 0.9
                recovery_rate from 0.01 to 0.3
  search seed:  17523302729290520563
  layout:       14 lanes of 1 thread each
  memory:       112.0 KiB projected for concurrent runs
  series rows:  0
```

The progress line adds the evaluations told and the best candidate or the filled cells.
A search ends with a line such as `1200/1200 evaluations, 298 cells filled`.

Under `--json`, the `search` field of `explore_plan` holds `algorithm`, `max_evaluations`, `batch_size`, `objective` (`column`, `goal` and `aggregate`, or `null`), `watched_columns`, `axes` (each axis of a `pse` as the plan prints it, or `null`), `space` (each factor as the plan prints it) and `search_seed`.
`explore_run` gives the candidate id in `config_id`.
An `explore_search_batch` line follows the runs of each batch, with `batch`, `evaluations` and `runs`.
It adds `best_candidate_id`, `best_objective` and `best_replicates`, or `filled_cells` for `pse`, and for `genetic` the number of finished generations in `generation_count`.
`explore_end` adds `evaluations`, then `best_candidate_id` and `best_objective`, or `filled_cells`.

```json
{"batch":0,"best_candidate_id":9,"best_objective":37.5,"best_replicates":4,"evaluations":32,"generation_count":1,"kind":"explore_search_batch","runs":128}
{"batch":18,"evaluations":1200,"filled_cells":298,"kind":"explore_search_batch","runs":2400}
```

### Resuming a search

`--resume` replays the search from its seed.
The search asks for its batches again, and a run that `runs.csv` holds is read back in place of running, when its `run_key` matches the run asked for.
The runs after the last one held run as usual.
`evaluations.csv`, `batches.csv` and `generations.csv` are written again from the start, and `best.csv` or `archive.csv` once the search ends.
The finished directory holds the same tables as a search run without a break, apart from the timing columns.

Every run held is read back as it ended, failed and timed-out runs included, and none runs again.
A run that came out differently would change every batch after it, and `--retry-failed` is refused.
A search runs whole, and `--shard` is refused too.

The resume refuses a directory whose search hash, model schema hash or column layout differs, a sweep's directory, and a `runs.csv` whose runs are out of order or whose key differs from the run asked for.
Every run held is checked against its key before any table is written, and a refused resume leaves the directory as it was.
The search hash covers the plan hash of the fixed values and actions, the replicate count, the algorithm, the budget, the batch size, the objective, the space and the algorithm's settings.
The timeout and the execution settings can change, and the budget and the replicate count cannot.

## Machine-readable output

`--json` replaces the report with one JSON object per line on stdout, leaving progress on stderr.
An `info` line comes first, then one `rep` line per timed rep as it finishes, then a `summary`.
A run killed part way still reports the reps it managed.
The `summary` records any `--act` schedule under `actions`, as one object with an `id` and a `tick` per entry.
With `--info` a `runtime` line comes before all of them.

With `--seed`, rep `i` is built from `base + i`, so `--reps` measures independent trajectories
rather than re-timing one. Without it every rep starts from the engine default and they all
replay the same run.

```json
{"kind":"info","engine":"henad","engine_version":"0.1.0","model":"boids","variant":"cpu","threads":1,"parallel_jobs":782,"adapter":null,"debug_build":false}
{"kind":"rep","rep":0,"seed":42,"steps":100,"warmup":10,"elapsed_s":1.234,"population":50000,"heap_bytes":2050020}
```

`parallel_jobs` is how many jobs one step splits into, or `null` for a GPU model.
`threads` is how many workers were available to take them.

This is the same shape every other engine in the [benchmarks](../benchmarks.md) speaks, so one
driver reads them all.
`--info --json` prints host and adapter details in the same stream.

A sweep has JSON lines of its own, listed under [its progress](#progress).

### Parameters as JSON

`--params --json` prints one line describing the model, for a tool that builds sweeps from outside.

| Field | Content |
|---|---|
| `kind` | `params` |
| `schema_version` | 1 |
| `model`, `name`, `backend` | Id, display name, and `cpu` or `gpu` |
| `schema_hash` | Hash of the model's parameters, stats and actions, as 16 hexadecimal digits |
| `params` | One object per parameter, in index order |
| `stats` | Label and RGBA colour of each declared stat |
| `stat_columns` | Columns a sweep writes for the stats, from a build at the defaults. Left out when that build fails |
| `actions` | Index, id and label of each action |
| `seed` | `{"type":"u64"}` |

A parameter object holds its `index`, `id`, `label`, `kind` (`f32`, `u32`, `bool` or `choice`), `apply` (`live` or `reload`), `format` (`plain` or `percent`) and `default`.
A number adds `min` and `max`, and an `f32` adds `step` when it declares one.
A `choice` adds its `options`, gives its `default` as an option name, and adds `default_index`.
Without `--json`, `--params` prints the text it always has.

```json
{"actions":[{"id":"seed_outbreak","index":0,"label":"Seed outbreak"}],"backend":"cpu","kind":"params","model":"sir","name":"SIR Epidemic","params":[{"apply":"live","default":0.3,"format":"plain","id":"infection_rate","index":2,"kind":"f32","label":"Infection Rate","max":1.0,"min":0.0,"step":0.01}],"schema_hash":"6ff1dc3971fd0a96","schema_version":1,"seed":{"type":"u64"},"stat_columns":["Susceptible","Infected","Recovered"],"stats":[{"color":[228,55,72,255],"label":"Infected"}]}
```

The line above is cut down to one parameter and one stat.
