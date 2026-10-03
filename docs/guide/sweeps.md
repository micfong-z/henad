---
title: Parameter sweeps
description: How to run a model over many parameter values and seeds from the command line, and read the results.
icon: material/tune-vertical
---

# Parameter sweeps

A single run tells you what one set of parameters did with one seed.
To see how a model responds to a parameter, you need many runs: the parameter at several values, and each value with several seeds.
A sweep makes those runs from the [command line](../reference/cli.md), and writes every result to a directory as CSV files.
The app runs the same sweeps from its [Sweep tab](app.md#sweep-tab), and plots their results in its [Results tab](app.md#results-tab).
When the configs worth running depend on the results, a [search](search.md) picks them as it goes.

Before we start, make sure you can [run the CLI](running.md#cli).
Every example here uses SIR, but any model that `--list` prints works the same way.

The commands below run the CLI from a clone of Henad's repository, as `cargo run --release -p henad-cli --`.
With the CLI installed, write `henad-cli` in place of that.
A [project of your own](your-project.md) runs its own models through `cargo run --release --bin my-model-cli --`, and SIR is not among them.
The template's `vote` sweeps the same way, as in `--vary density=0.45:0.55:0.05`, and its `specs/vote.toml` is a spec file to start from.

## A first sweep

Let's see how the size of an SIR epidemic depends on the infection rate:

``` bash
cargo run --release -p henad-cli -- sir \
  --vary infection_rate=0.1:0.5:0.1 --reps 5 --steps 500 --out sir-sweep
```

`--vary` names a parameter and the values it takes, here from 0.1 to 0.5 in steps of 0.1.
Each value gives one **config**, a full set of parameter values.
The parameters you do not vary keep their defaults.
`--reps 5` runs every config five times, each time with a different seed, and `--out` names the directory the results go to.

The sweep prints its plan before the first run, and a line of counts at the end:

```text
sweep of SIR Epidemic (sir): 5 configs x 5 replicates = 25 runs
  block 0:      factorial, 5 configs
  layout:       1 lane of 14 threads
  memory:       2.0 MiB projected for concurrent runs
  series rows:  12525
wrote 25 runs to sir-sweep in 3.2s: 25 ok, 0 non-finite, 0 failed
```

!!! warning "Release mode"

    A debug build runs sweeps too, but steps one to two orders of magnitude slower.
    Make sure that `--release` is used.
    The CLI prints a warning when it is not.

### Choosing the values

A range is written `min:max:step` and includes both ends.
Henad computes each value from its position, as `min + i * step`.
Rounding errors do not build up along the range, and `0:1:0.1` gives eleven values with the last one exactly 1.
An integer parameter can leave out the step, and then steps by 1.

A list takes the values separated by commas, written the way `--set` takes them:

``` bash
--vary recovery_rate=0.02,0.05,0.1
```

Spaces around each value are ignored.

`all` takes every value of a parameter that the app shows as a checkbox or a dropdown, such as `--vary network=all` for Virus on a Network.

`--set` still fixes a parameter for every config.
A parameter cannot be fixed and varied in the same sweep.

### Varying several parameters

Give `--vary` more than once, and every combination of the values runs:

``` bash
cargo run --release -p henad-cli -- sir \
  --vary infection_rate=0.1:0.5:0.1 --vary recovery_rate=0.02,0.05,0.1 \
  --reps 5 --steps 500 --out sir-grid
```

That is 5 × 3 = 15 configs.
The first `--vary` changes slowest, like the outer of two nested loops.

With `--zip`, the values pair up by position, and config `i` takes value `i` of every parameter.
Every `--vary` then needs the same number of values.

### Sampling the ranges

The number of combinations grows fast.
Three parameters at ten values each make a thousand configs.
`--sample` draws a set number of configs from the ranges instead:

``` bash
cargo run --release -p henad-cli -- sir \
  --vary infection_rate=0.05:0.95 --vary recovery_rate=0.01:0.2 \
  --sample lhs:40 --reps 5 --steps 500 --out sir-lhs
```

A range written `min:max`, with no step, stands for every value between its ends.
`lhs:40` draws 40 configs as a **Latin hypercube**.
The range of each parameter is cut into 40 equal strata, and each stratum holds exactly one config.
An `f32` range places that config at a random point inside its stratum.
Each parameter shuffles its strata on its own, and the configs spread over the whole space without lining up along a grid.
`random:40` draws every value of every config on its own, uniformly over its range.

A list, `all` or a range with a step gives a parameter a set of values to sample from.
A random design picks among them with equal chances.
A Latin hypercube gives each stratum the lowest value it covers, and takes each value equally often, to within one config.
The values a parameter takes then depend on the number of configs alone, and the design seed only decides which configs take them.
An integer range with no step is the set of every integer in it, such as `--vary initial_outbreak_size=1:50` for Virus on a Network.

The draws come from a **design seed**, and the plan prints it beside the block:

```text
  block 0:      lhs, 40 configs, design seed 11629814477949246596
```

A design seed draws the same configs every time, on any machine.
Unless you give one, it is derived from the root seed of the [next section](#replicates-and-seeds), and a new `--seed` draws a new design as well.
`--design-seed` fixes the design while `--seed` changes the runs.

### Checking a sweep first

`--dry-run` plans the sweep and sizes it from two builds, of the first config that builds without a fault and of the last config.
It then prints the plan and stops:

``` bash
cargo run --release -p henad-cli -- sir \
  --vary infection_rate=0.1:0.5:0.1 --reps 5 --steps 500 --dry-run
```

It catches the mistakes a real sweep would catch before its first run, such as an unknown parameter or a value out of range, and writes nothing.
The plan also tells you how much memory the runs will hold and how many rows `series.csv` will get.
Of those two builds, the one that holds more memory sets the figure.

## Replicates and seeds

Most models draw random numbers, and the same config gives a different result for each seed.
`--reps` runs each config several times.
Each run's seed is derived from one root seed, `--seed`.
The root is 0 when you leave it out.

The seed of replicate `r` mixes the root seed with a fixed salt, adds `r`, and mixes the sum again.
The mixing is [`mix_seed`](../reference/primitives.md#mix_seed), the same function the engine applies to any seed it is given.
Nothing about the config goes into it.
Replicate 0 of every config gets one seed, replicate 1 of every config another, and so on.

This is known as **common random numbers**.
Two configs then start each replicate from the same seed, and less of the difference between their results is noise.
You can see it in the [results below](#reading-the-results), where every infection rate starts from the same number of infected cells at tick 0.

`--independent-seeds` mixes the config's id into the seed as well, and every run then gets a seed of its own.
Use it when your analysis treats configs as independent samples.

Every row of `runs.csv` records its seed, and you can rebuild any run on its own from it.
Pass the seed to `--seed`, and the run's parameter values to `--set`:

``` bash
cargo run --release -p henad-cli -- sir --set infection_rate=0.2 \
  --seed 16795053913516373515 --steps 500 --export-stats run.csv
```

The run steps exactly as it did in the sweep.
Sampling more or less often never changes a run.

The app rebuilds a run from its row as well.
Open the directory in the [Results tab](app.md#opening-results), select the run, and press <span class="ui" markdown>:material-play-box-outline: Open</span> to build it at tick 0 or <span class="ui" markdown>:material-fast-forward: Open at end</span> to step it to its last tick.
<span class="ui" markdown>:material-content-copy: Copy command</span> copies a command line like the one above.

## Spec files

A sweep with many settings is easier to keep in a file.
A spec file is TOML, and holds every setting that changes a result.
Here is one for SIR.
Henad's repository keeps it at `crates/henad-explore/specs/sir_sweep.toml`.

??? example "`sir_sweep.toml`"

    ``` toml
    --8<-- "crates/henad-explore/specs/sir_sweep.toml"
    ```

Save it as `sir_sweep.toml` and run it:

``` bash
cargo run --release -p henad-cli -- --spec sir_sweep.toml --out sir-spec
```

The spec names its own model, and the command line leaves it out.
`--spec` refuses every flag that would change a result, such as `--steps` or `--vary`, and the file stays the whole record of the sweep.

We will go through the file one table at a time.

``` toml title="sir_sweep.toml"
--8<-- "crates/henad-explore/specs/sir_sweep.toml:model"
```

`model` is the id that `--list` prints, and `[set]` fixes a value for every config, as `--set` does.

``` toml
--8<-- "crates/henad-explore/specs/sir_sweep.toml:run"
```

`[run]` takes the place of `--steps`, `--warmup` and `--reps`, the last under the name `replicates`.
`timeout_s` and `stop` end a run early, as `--timeout` and `--stop` do, and [ending a run early](#ending-a-run-early) covers both.

``` toml
--8<-- "crates/henad-explore/specs/sir_sweep.toml:measure"
```

`[measure]` sets how often a run is sampled and what is kept, as described under [measuring a run](#measuring-a-run).
Each entry in `reducers` names a stat column and the kinds of reducer to apply to it.

``` toml
--8<-- "crates/henad-explore/specs/sir_sweep.toml:seeds"
```

`root` is the root seed, and `scheme` is `common` or `independent`.

``` toml
--8<-- "crates/henad-explore/specs/sir_sweep.toml:actions"
```

Each `[[action]]` fires one of the model's actions in every run, as `--act` does.
[Actions in a sweep](#actions-in-a-sweep) has the details.

### Blocks

A spec varies its parameters in blocks.
Each `[[block]]` lists its factors, the parameters it varies, and a design that combines their values.

``` toml
--8<-- "crates/henad-explore/specs/sir_sweep.toml:factorial"
```

A factor names a parameter with `param`, or the tick of an action with `action`.
It then gives its values as `values = [...]`, `range = { min, max, step }` or `levels = "all"`.
A `factorial` block runs every combination, as repeated `--vary` does.

``` toml
--8<-- "crates/henad-explore/specs/sir_sweep.toml:zip"
```

A `zip` block pairs the values by position, as `--zip` does.

``` toml
--8<-- "crates/henad-explore/specs/sir_sweep.toml:lhs"
```

An `lhs` block draws `samples` configs as a Latin hypercube, as `--sample lhs:40` does, and a `random` block draws them as `--sample random:40` does.
`design_seed` fixes a block's design seed.
Without it, each block derives its own from the root seed and its position in the file.
A `table` block reads its configs from a CSV file, as [designs from a table](#designs-from-a-table) shows.

The sweep runs the configs of every block, one block after the other.
This file has 45 + 3 + 40 = 88 configs, and with 8 replicates each, 704 runs.
The `block` column of `runs.csv` says which block a run's config came from.
A parameter can appear in several blocks, but only once within a block.

``` toml
--8<-- "crates/henad-explore/specs/sir_sweep.toml:execution"
```

`[execution]` decides how the runs are spread over the machine, and never changes a result.
`concurrent`, `memory` and `gpu_memory` take what `--concurrent`, `--memory` and `--gpu-memory` take, and those flags override them.

Every table refuses a key it does not know, and a misspelt key stops the sweep with an error.
A table you leave out takes its defaults: 1000 steps, no warm-up, one replicate, no stop condition or timeout, a sample every tick, the four default reducers, root seed 0 and common random numbers.
A block without a `design` is a factorial.

## Measuring a run

Each run samples its stats at the end of the warm-up, every `--stats-every` ticks after that, and at its last tick.
The samples feed two outputs.

The **series** keeps a sample every `--series-every` ticks, plus the last one.
`--series-every` has to be a multiple of `--stats-every`, and 0 turns the series off.

A **reducer** folds a run's samples of one stat into one number.
By default every stat gets four, `final`, `min`, `max` and `mean`.
They appear in `runs.csv` as `Infected:final`, `Infected:min` and so on.
`--no-default-reducers` drops them, and `--reduce Infected:max` adds one back.

`--reduce` also takes four kinds that no stat gets by default:

| Kind | Value |
|---|---|
| `argmax`, `argmin` | First sampled tick of the greatest or the least value |
| `first<=10` | First sampled tick where the value passes a comparison, here at most 10 |
| `mean@200..600` | Mean of the samples from tick 200 to tick 600, both included |

`first` takes any of `<`, `<=`, `>`, `>=`, `==` and `!=`, followed by a number.
It is empty for a run where the comparison never holds, and `mean@` is empty for a run with no sample in its window.
In the spec above, `Infected:argmax` is the tick of the epidemic's peak, and `Infected:first<=10` the first tick with at most 10 cells infected.

``` bash
--reduce Infected:argmax --reduce 'Infected:first<=10' --reduce Recovered:mean@200..600
```

Quote a kind that holds `<` or `>`, or the shell reads it as a redirect.

A reducer sees only the samples, and a tick it gives is always a sampled tick.
With `--stats-every 5`, the peak is found to within 5 ticks.

A sample costs time.
Ant Foraging, for one, converts its whole pheromone field for drawing before each sample.
When you only need the reducers, a larger `--stats-every` makes the sweep faster.

## Ending a run early

### Stop conditions

An SIR epidemic that has burnt out stays burnt out.
Stepping it further only costs time.
`--stop` ends a run at the first sample where a condition holds:

``` bash
cargo run --release -p henad-cli -- sir \
  --vary infection_rate=0.1:0.5:0.1 --reps 5 --steps 2000 --stats-every 5 \
  --stop 'Infected <= 0' --out sir-stop
```

A condition is a stat column, a comparator and a number.
The column is written as a reducer names it, and can hold spaces, as in `'Giant Component Share >= 0.5'` for Team Assembly.
The comparator is one of `<`, `<=`, `>`, `>=`, `==` and `!=`, read from the last run of those characters in the condition.
A label holding `<`, `>`, `=` or `!` therefore works too.
A NaN never meets a condition.

The condition is checked at each sample, and `--stats-every` sets how soon after the event a run stops.
A run that stops ends on that sample.
Its `ticks` column holds the sample's tick, `stop_reason` reads `condition`, and its series and reducers end there too.
The run does not count as failed.
An action due after the stop never fires, and a reducer whose window starts after it stays empty.

In a spec file the condition goes in `[run]`, as the [spec above](#spec-files) shows.
There it can take a `min_tick`, the first tick at which the condition can end a run.
`min_tick` keeps a condition that holds at the start from ending the run there.
`--stop` has no minimum tick.

### Timeouts

A config can turn out far slower than the rest, such as one whose population keeps growing.
`--timeout SECONDS`, or `timeout_s` in `[run]`, caps the time one run can take:

``` bash
cargo run --release -p henad-cli -- sir \
  --vary infection_rate=0.1:0.5:0.1 --reps 5 --steps 500 --timeout 600 --out sir-timeout
```

The clock counts the time a run spends stepping and sampling, and leaves out its build.
With several GPU runs at once, it counts a run's share of that time, as [concurrency](#concurrency) describes.
It is read between slices of steps, and a run ends a little after its limit.
A run past its limit gets the status `timed_out` and the stop reason `timeout`, and its note gives the tick it reached.
It counts as failed, and its reducers cover only the ticks it stepped.

The ticks a run reaches in a set time depend on the machine and its load.
A sweep with a timed-out run is not reproducible, and every promise of identical files on this page leaves such runs out.
[Resuming](#resuming-a-sweep) runs a timed-out run again.
The timeout is not part of the plan, and the resume can raise it or drop it.

## Actions in a sweep

A model's actions fire in a sweep as they do in a benchmark.
`--act` adds one to every run:

``` bash
cargo run --release -p henad-cli -- sir \
  --vary infection_rate=0.1:0.5:0.1 --act seed_outbreak@400 \
  --reps 5 --steps 1000 --out sir-second-wave
```

Every run seeds a second outbreak at tick 400.
`runs.csv` and `summary.csv` gain a column `action.seed_outbreak` after the parameters, with the action's tick in each config.

An action's tick can be a factor too.
`--vary action.NAME=LEVELS` varies the tick of the action `--act` added under that name:

``` bash
--act seed_outbreak@400 --vary action.seed_outbreak=200:600:100
```

The levels take the same forms as a parameter's, in whole ticks, and a sampled design draws ticks from a range with no step.
An action added with `--act` is named by its id.
A second `--act` with the same id is named `ID_2`, a third `ID_3`, and so on, skipping any name an earlier `--act` has taken.
Each can be varied on its own.
In a spec file, factors and columns call an `[[action]]` by its `name`.
An `[[action]]` without a `name` goes by its id.
The factorial block [above](#blocks) varies `second_wave` that way.

An action due at tick 0 fires before the first step.
One due at a later tick fires after the step that reaches it, and that tick's sample already shows its effect.
Actions due at the same tick fire in the order the command line or the spec lists them.
An action due past the last tick never fires, and the plan warns about it:

```text
warning: action 'seed_outbreak' is due after the last tick 300 in 1 config (latest tick 400) and will not run there
```

An action draws its random numbers from a stream of its own.
Two configs that differ only in an action's tick match on every sample before the earlier of the two ticks.

A model can refuse an action at a tick.
The refusal goes in the run's `note` column, and the run stays `ok`.

## Designs from a table

A design can come from somewhere else: another tool's sampler, a table of cases from a paper, or the points of an earlier sweep worth a closer look.
A design table holds one config per row:

```text title="sir_design.csv"
--8<-- "crates/henad-explore/specs/sir_design.csv"
```

The header names a parameter id or `action.NAME` in each column.
Row `i` after the header becomes config `i` of its block, counted from 0.
A parameter or action the table leaves out keeps its fixed value, its default or its own tick.
Values are written as `--set` takes them, and spaces around a field and blank lines are ignored.
A column that names nothing is refused, and the error lists the columns there can be.

`--design FILE` runs a table from the command line, in place of `--vary`.
The table is then the sweep's only block, and row `i` is config `i` of the sweep.
There an `action.NAME` column names an action `--act` adds, by its id.
A spec file names its table in a block:

``` toml title="sir_table.toml"
--8<-- "crates/henad-explore/specs/sir_table.toml:table"
```

Save the table as `sir_design.csv`, and the whole spec below as `sir_table.toml` beside it.

??? example "`sir_table.toml`"

    ``` toml
    --8<-- "crates/henad-explore/specs/sir_table.toml"
    ```

Then run it:

``` bash
cargo run --release -p henad-cli -- --spec sir_table.toml --out sir-table
```

`file` is relative to the spec file, and cannot be absolute or hold `..`.
The manifest records the table's path and a hash of its text.
Its copy of the spec holds the table itself under `table_text`, a key that a spec file can also use in place of `file`.

### Sensitivity analysis with SALib

[SALib](https://salib.readthedocs.io) draws the designs of a global sensitivity analysis, such as Sobol and Morris, and computes the indices from the outputs.
Henad draws neither design itself, and a design table carries one from SALib to Henad.

First, save the model's parameters and their bounds as JSON:

``` bash
cargo run --release -p henad-cli -- sir --params --json > sir-params.json
```

In Python, build SALib's problem from the bounds, draw a Sobol design, and write it as a design table:

``` python
import json
import pandas as pd
from SALib.sample import sobol as sobol_sample

params = {param["id"]: param for param in json.load(open("sir-params.json"))["params"]}
names = ["infection_rate", "recovery_rate"]
bounds = [[params[name]["min"], params[name]["max"]] for name in names]
problem = {"num_vars": len(names), "names": names, "bounds": bounds}
pd.DataFrame(sobol_sample.sample(problem, 256), columns=names).to_csv("sobol.csv", index=False)
```

With two parameters, 256 base samples give 1536 rows.
A `u32` parameter takes integers only, and its column needs rounding first, as in `.round().astype(int)`.

Run the table:

``` bash
cargo run --release -p henad-cli -- sir --design sobol.csv \
  --set grid_width=256 --set grid_height=256 --reps 4 --steps 500 --out sir-sobol
```

Then hand an output back to SALib:

``` python
from SALib.analyze import sobol

runs = pd.read_csv("sir-sobol/runs.csv")
peak = runs.groupby("config_id")["Infected:max"].mean()
indices = sobol.analyze(problem, peak.to_numpy())
print(indices["S1"], indices["ST"])
```

Grouping by `config_id` averages the replicates of each config, and puts the configs back in the order SALib drew them.
Check first that every run is `ok`, since a failed run's reducers cover only part of it.

## The output directory

```text
sir-sweep/
├── manifest.json
├── runs.csv
├── series.csv
└── summary.csv
```

`runs.csv`
: One row per run, with its ids and seed, its parameter values and action ticks, its status and timings, and one column per reducer.

`series.csv`
: The series of every run, one row per run and sampled tick.

`summary.csv`
: One row per config, with the mean, standard deviation, count and 95% confidence interval of every reducer over the replicates.

`manifest.json`
: Record of the sweep: the model and its schema, the resolved settings, the command line, the seed formula, the builds that ran it and the machine.

The [CLI reference](../reference/cli.md#output-directory) lists every column.

Each run is written as soon as the runs before it are.
A sweep you stop part way keeps what it wrote.
Henad refuses a directory that already holds results, so give each sweep a directory of its own, or [resume](#resuming-a-sweep) the one it stopped in.

A run can fail, for example when a model panics at one parameter value.
The failure is recorded in `runs.csv` with its status and the panic message, and the sweep carries on with the next run.
The command then exits with status 3 in place of 0, for a script to check.

## Concurrency

A sweep steps several runs at once, and the `layout` line of the plan shows how many.
Henad picks the layout from the first config that builds without a fault and the last config, whichever of the two holds more memory.

The layout never changes a result.
All three CSV files come out the same byte for byte at any `--concurrent`, apart from the `build_ms`, `wall_ms` and `steps_per_s` columns.

!!! note "GPU boids does not replay"

    `gpu_boids` is the one model whose runs differ between two sweeps with the same settings.
    Its neighbour index leaves the order of the boids within a cell open, and the order changes from run to run.
    Its manifest records `replays_exactly` as `false`, and the app's Results tab notes beside a run that the opened run might differ from its row.

### CPU lanes

On a CPU model, each run steps in a lane of its own.
A small model gets many lanes of one thread each, and a large model fewer lanes of more threads, down to a single lane on every core.
`--concurrent N` sets the number of lanes yourself, and `--memory` caps the bytes the lanes hold together:

``` bash
cargo run --release -p henad-cli -- sir \
  --vary infection_rate=0.1:0.5:0.1 --reps 5 --steps 500 \
  --concurrent 4 --memory 8000000000 --out sir-four-lanes
```

### GPU tracks

On a GPU model, each run steps on a **track** of its own, and the tracks share the GPU.
Henad visits the tracks in turn, and each visit hands the GPU a batch of up to 64 steps of that track's run.
`auto` picks at most four tracks, and a small grid gets all four:

``` bash
cargo run --release -p henad-cli -- gpu_sir --set grid_width=256 --set grid_height=256 \
  --vary infection_rate=0.1:0.5:0.1 --reps 5 --steps 500 --stats-every 5 --out gpu-sir-sweep
```

```text
  layout:       4 GPU runs at a time
  memory:       5.0 MiB projected for concurrent runs
```

A small model finishes each batch quickly, and a single run leaves the GPU waiting: for its next batch, and for each sample's stats to reach the CPU.
Other tracks fill those waits with batches of their own.
The gain depends on the model.
Time a short sweep at `--concurrent 1` and at `auto` before a long one.

A large model keeps the GPU busy on its own.
More tracks then split the GPU between runs and hold more of its memory.
With `auto`, a model of 1,048,576 cells or agents or more gets a single track, such as `gpu_sir` at its default grid of 1024 by 1024.

`--concurrent N` sets the number of tracks yourself, and `--gpu-memory` caps the bytes of GPU memory the runs on the tracks hold together.
A run starts only once it fits beside the runs already on the GPU.
A run that fits beside no other run steps alone.
The graphics API reports no total for a GPU's memory, and without `--gpu-memory` the largest buffer the GPU allows stands in for the cap.
A run that still finds the GPU out of memory waits for another run to finish, and the sweep runs one track fewer from then on.

With several tracks, the time the sweep spends visiting them is split evenly between their runs.
A run's `wall_ms`, its `steps_per_s` and its `--timeout` all read its share.
Pass `--concurrent 1` when the time of each run matters.

A fault in one run ends that run alone, and the other tracks carry on.
A GPU error that Henad cannot trace to one run fails every run on a track at the time, and `--retry-failed` on a [resume](#resuming-a-sweep) runs them again.
If the GPU itself is lost part way, for example when its driver resets, the sweep stops.
It keeps the runs it wrote, marks the manifest `incomplete` and exits with status 1.
`--resume` runs the rest.

## Resuming a sweep

A sweep can stop part way through: the process is killed, the machine restarts, or a batch job runs out of time.
Run the same command again with `--resume`, and the sweep carries on where it stopped:

``` bash
cargo run --release --locked -p henad-cli -- sir \
  --vary infection_rate=0.1:0.5:0.1 --reps 5 --steps 500 --out sir-sweep --resume
```

The plan counts the runs the directory holds and the runs left:

```text
  resume:       20 runs skipped, 5 to run
```

A resume first checks that the directory holds the same sweep.
The model and its declarations, the configs, the steps, the sampling, the stop condition and reducers, the actions, the seeds and the shard all have to match, and a resume of anything else is refused.
Only the replicate count and the timeout can change.
The manifest's `sessions` lists every process that wrote runs, with three builds each: Henad's, the binary's and that of the crate that registered the model.
A build records its commit, whether the sources differed from the commit, and a hash of the sources.
A resume warns when Henad's build or the model's differs from one a session recorded, and goes ahead.
An uncommitted edit to a kernel counts as a different build, since its source hash changes.
The [CLI reference](../reference/cli.md#builds) gives the fields and the rule that compares two builds.

The resume then repairs what a cut-off write left behind.
A partial last line of `runs.csv` is dropped, and so are the rows of `series.csv` whose run never reached `runs.csv`.
The resume keeps every run that ended `ok` or `non_finite`, and every run that failed on a fault.
A run that timed out runs again.
`--retry-failed` runs the failed runs again as well, for example under a build that fixes the fault.

When the resume ends, the three CSV files are the same as those of a sweep that ran without a break, byte for byte apart from the timing columns.
`--resume` on a directory with no results starts a fresh sweep, and a script can pass it every time.
A resume of a directory that another sweep is still writing to is refused.
A sweep leaves a file named `.lock` in its directory, killed or not, and the next resume locks it again, so it needs no clearing.
With `--dry-run`, a resume prints its counts and changes nothing.
In the desktop app, open the directory in the [Results tab](app.md#opening-results) and press <span class="ui" markdown>:material-play: Resume sweep</span>.

### Adding replicates

Five replicates can turn out too few, with confidence intervals too wide to tell two configs apart.
Resume with a higher `--reps`, and only the new replicates run:

``` bash
cargo run --release --locked -p henad-cli -- sir \
  --vary infection_rate=0.1:0.5:0.1 --reps 8 --steps 500 --out sir-sweep --resume
```

The seed of replicate `r` depends on the root seed and `r` alone, and the five replicates on disk are the first five of the eight.
Their rows keep their seeds and values, and take the run ids of the larger plan, where `run_id` is `config_id * 8 + rep`.
`summary.csv` is rebuilt over all eight.
In a spec file, raise `replicates` in `[run]` instead.
A resume can raise the replicate count and never lower it.

## Sharding a sweep

A sweep too large for one machine can be split into shards, each run on a machine of its own.
`--shard I/N` runs only the runs whose `run_id` leaves remainder `I` when divided by `N`, and writes them to a directory of its own:

``` bash
cargo run --release --locked -p henad-cli -- --spec sir_sweep.toml --shard 0/4 --out shard-0
```

Taking every `N`th run spreads the configs, heavy and light alike, evenly over the shards.
Once every shard has finished, `--merge` joins them:

``` bash
cargo run --release --locked -p henad-cli -- --merge shard-0 shard-1 shard-2 shard-3 --out sir-merged
```

`--merge` checks that the directories hold different shards of one plan, all at one replicate count.
It interleaves `runs.csv` and `series.csv` back into run order, rebuilds `summary.csv`, and lists the merged directories in the manifest under `merged_shards`.
The merged CSV files are the same as those of the sweep run in one piece, apart from the timing columns.
A merge warns when the shards ran different builds of Henad or of the model.

Build every shard, and every later resume, from one commit with `--locked`.
Cargo then builds from the committed `Cargo.lock` and refuses to change it.
A lockfile updated on one machine changes the source hash of the build there, and the merge or resume warns that the build changed.

A merge with a shard missing still writes what it has.
It warns about the missing runs, marks the manifest `incomplete` and exits with status 3.
Resuming the merged directory with the whole sweep's command, without `--shard`, runs the missing runs.

Raising the replicate count renumbers the runs, and a finished run can land in another shard.
Resume refuses a sharded directory whenever that happens.
To add replicates, merge the shards first, then resume the merged directory.

### A Slurm array job

On a cluster that runs [Slurm](https://slurm.schedmd.com), an array job runs one shard per task.
Build the CLI once with `cargo build --release --locked -p henad-cli`, then submit this script with `sbatch` from the repository root, with `sir_sweep.toml` saved there:

``` bash title="sweep.sbatch"
#!/bin/bash
#SBATCH --job-name=sir-sweep
#SBATCH --array=0-7
#SBATCH --cpus-per-task=16
#SBATCH --time=02:00:00

target/release/henad-cli --spec sir_sweep.toml \
  --shard "$SLURM_ARRAY_TASK_ID/8" --threads "$SLURM_CPUS_PER_TASK" \
  --out "sir-sweep/shard-$SLURM_ARRAY_TASK_ID" --resume
```

The array runs eight tasks, and task `i` runs shard `i/8`.
`--threads` sets the worker count to the task's cores.
Without it, rayon starts one worker per logical CPU it can see, and on a node that does not bind a task to its cores, that is every CPU of the node.
`--resume` lets a task that was cut off and submitted again carry on where it stopped.
When every task is done, merge the shards:

``` bash
target/release/henad-cli --merge sir-sweep/shard-* --out sir-sweep/merged
```

## Reading the results

The app's [Results tab](app.md#results-tab) plots a results directory with no code.
Open the directory there with <span class="ui" markdown>:material-folder-open-outline: Open results</span>, or name it when the app starts:

``` bash
cargo run --release --bin henad-app -- --open sir-sweep
```

In its **Response** view, pick **Infected, max** as the **Output** to plot the mean peak against the infection rate, with a 95% confidence interval on each point.
In its **Series** view, pick **Infected** as the **Stat** to draw the mean epidemic curve of each infection rate, with a band over the replicates.

Any tool that reads CSV can take the results.
Here they are in [pandas](https://pandas.pydata.org).
Start with `runs.csv`, one row per run:

``` python
import pandas as pd

runs = pd.read_csv("sir-sweep/runs.csv")
runs = runs[runs["status"] == "ok"]
print(runs.groupby("infection_rate")["Infected:max"].agg(["mean", "std", "count"]))
```

The filter keeps the runs that finished cleanly.
A failed run's reducers cover only the ticks before its fault.
The largest number infected at once climbs with the infection rate:

```text
                    mean          std  count
infection_rate
0.1             564624.6  2094.064302      5
0.2             700539.8  1499.381606      5
0.3             753997.6  1956.729746      5
0.4             782779.6  1719.644963      5
0.5             798298.8  1624.493829      5
```

`summary.csv` has these statistics already, one row per config, with a 95% confidence interval for each mean:

``` python
summary = pd.read_csv("sir-sweep/summary.csv")
print(summary[["infection_rate", "Infected:max:mean", "Infected:max:ci95_low", "Infected:max:ci95_high"]])
```

To draw the mean epidemic curve of each infection rate, join the series to the runs on `run_id`:

``` python
series = pd.read_csv("sir-sweep/series.csv").merge(runs[["run_id", "infection_rate"]], on="run_id")
curves = series.groupby(["infection_rate", "tick"])["Infected"].mean().unstack("infection_rate")
curves.plot()  # needs matplotlib
```

Every curve starts from the same count at tick 0, since replicate `r` of every config seeds the same initial infections.

*[CLI]: Command-line interface
*[CPU]: Central processing unit
*[GPU]: Graphics processing unit
*[CSV]: Comma-separated values
*[JSON]: JavaScript Object Notation
*[TOML]: Tom's Obvious Minimal Language
